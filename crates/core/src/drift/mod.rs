pub mod files;
pub mod packages;

pub use files::RenderedFile;

use std::path::PathBuf;

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct PackageRef {
    pub name: String,
    pub cask: bool,
}

impl PackageRef {
    pub fn formula(name: &str) -> Self {
        Self {
            name: name.to_string(),
            cask: false,
        }
    }

    pub fn cask(name: &str) -> Self {
        Self {
            name: name.to_string(),
            cask: true,
        }
    }
}

/// The seven concrete outcomes of the three-way diff, covering the four drift
/// classes from the design document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "class", rename_all = "snake_case")]
pub enum Drift {
    /// In the repository, never applied here. → install
    IncomingPackage(PackageRef),
    /// Managed file that is missing or outdated locally. → write
    IncomingFile { target: PathBuf, set: String },
    /// dotfix installed it, the user removed it locally. → adopt or reinstall
    LocallyRemoved(PackageRef),
    /// Installed here, no active set wants it. → adopt or ignore
    Unmanaged {
        /// Flattened so `--json` keeps the shape it had when this was a
        /// newtype variant: `name` and `cask` stay at the top level of the
        /// item, beside `class`. `dotfix status --json` is an interface.
        #[serde(flatten)]
        package: PackageRef,
        /// Sets that declare it but are switched off on this machine. Empty
        /// means it really is in no set. Carried on the drift because neither
        /// front end has the repository when it renders a report.
        declared_in: Vec<String>,
    },
    /// Dropped from its set, still installed. → uninstall if it is a leaf
    RemovedPackage {
        package: PackageRef,
        /// Installed packages still depending on it. Non-empty means the
        /// uninstall is skipped.
        blocked_by: Vec<String>,
        /// Sets that declare it but are switched off on this machine — the
        /// usual reason a package is suddenly unwanted. Naming it matters
        /// more here than for `Unmanaged`, because this action uninstalls.
        declared_in: Vec<String>,
    },
    /// Managed file dropped from its set, still on disk. → remove
    RemovedFile { target: PathBuf },
    /// Managed file edited locally. → write back or overwrite
    LocalEdit {
        target: PathBuf,
        set: String,
        /// Whether the repository file this was rendered from resolves any
        /// secrets. Carried here because `adopt` cannot otherwise tell, and
        /// writing such a file back would commit the resolved value.
        contains_secrets: bool,
    },
}

impl Drift {
    /// Stable handle for one drift item, derived rather than stored so it
    /// survives a re-inspection. The UI applies items by id; if the world moved
    /// in between, the id simply no longer matches and the item is skipped.
    pub fn id(&self) -> String {
        fn pkg(prefix: &str, p: &PackageRef) -> String {
            let kind = if p.cask { "cask" } else { "formula" };
            format!("{prefix}:{kind}:{}", p.name)
        }

        match self {
            Drift::IncomingPackage(p) => pkg("incoming_package", p),
            Drift::LocallyRemoved(p) => pkg("locally_removed", p),
            Drift::Unmanaged { package, .. } => pkg("unmanaged", package),
            Drift::RemovedPackage { package, .. } => pkg("removed_package", package),
            Drift::IncomingFile { target, .. } => {
                format!("incoming_file:{}", target.display())
            }
            Drift::RemovedFile { target } => format!("removed_file:{}", target.display()),
            Drift::LocalEdit { target, .. } => format!("local_edit:{}", target.display()),
        }
    }

    /// What the item is about: a package name or an absolute path.
    pub fn label(&self) -> String {
        match self {
            Drift::IncomingPackage(p)
            | Drift::LocallyRemoved(p)
            | Drift::Unmanaged { package: p, .. }
            | Drift::RemovedPackage { package: p, .. } => p.name.clone(),
            Drift::IncomingFile { target, .. }
            | Drift::RemovedFile { target }
            | Drift::LocalEdit { target, .. } => target.display().to_string(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Report {
    pub items: Vec<Drift>,
    /// Software a set declares but this machine does not have.
    ///
    /// Kept out of `items` on purpose. Everything in there is something
    /// dotfix can do — install, remove, write, adopt — and a row that looks
    /// actionable but has no action is a trap this codebase has fallen into
    /// before. dotfix cannot install these; it can only say they are absent
    /// and repeat the command the set's author wrote down.
    #[serde(default)]
    pub missing: Vec<Missing>,
}

/// A declared requirement that is not present here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Missing {
    pub name: String,
    pub set: String,
    /// What to run, verbatim from the set. Shown, never executed.
    pub hint: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Counts {
    pub incoming: usize,
    pub unmanaged: usize,
    pub removed: usize,
    pub local_edits: usize,
}

impl Report {
    /// Whether this machine matches the repository.
    ///
    /// A declared requirement that is absent counts: the repository says the
    /// machine needs it and it is not here, which is not "in sync" however
    /// little dotfix can do about it.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty() && self.missing.is_empty()
    }

    pub fn counts(&self) -> Counts {
        let mut c = Counts::default();
        for item in &self.items {
            match item {
                Drift::IncomingPackage(_) | Drift::IncomingFile { .. } => c.incoming += 1,
                Drift::Unmanaged { .. } | Drift::LocallyRemoved(_) => c.unmanaged += 1,
                Drift::RemovedPackage { .. } | Drift::RemovedFile { .. } => c.removed += 1,
                Drift::LocalEdit { .. } => c.local_edits += 1,
            }
        }
        c
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    // The brief's fixture is a `Vec` for readability alongside the other
    // test cases; clippy would rather it be an array since it is never
    // mutated, but changing the container type is not worth diverging from
    // the specified test code for.
    #[allow(clippy::useless_vec)]
    fn ids_are_unique_per_class_and_target() {
        let items = vec![
            Drift::IncomingPackage(PackageRef::formula("jq")),
            Drift::Unmanaged {
                package: PackageRef::formula("jq"),
                declared_in: Vec::new(),
            },
            Drift::IncomingPackage(PackageRef::cask("jq")),
            Drift::LocalEdit {
                target: PathBuf::from("/Users/test/.gitconfig"),
                set: "core".into(),
                contains_secrets: false,
            },
        ];
        let ids: Vec<String> = items.iter().map(Drift::id).collect();
        let mut unique = ids.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), ids.len(), "ids collided: {ids:?}");
    }

    #[test]
    fn an_id_is_stable_for_the_same_drift() {
        let a = Drift::IncomingPackage(PackageRef::formula("jq"));
        let b = Drift::IncomingPackage(PackageRef::formula("jq"));
        assert_eq!(a.id(), b.id());
        assert_eq!(a.id(), "incoming_package:formula:jq");
    }

    #[test]
    fn a_cask_and_a_formula_of_the_same_name_differ() {
        assert_ne!(
            Drift::IncomingPackage(PackageRef::formula("x")).id(),
            Drift::IncomingPackage(PackageRef::cask("x")).id()
        );
    }

    #[test]
    fn labels_name_the_package_or_the_path() {
        assert_eq!(
            Drift::Unmanaged {
                package: PackageRef::formula("jq"),
                declared_in: Vec::new(),
            }
            .label(),
            "jq"
        );
        assert_eq!(
            Drift::LocalEdit {
                target: PathBuf::from("/Users/test/.gitconfig"),
                set: "core".into(),
                contains_secrets: false,
            }
            .label(),
            "/Users/test/.gitconfig"
        );
    }
}

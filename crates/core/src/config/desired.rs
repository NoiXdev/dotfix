use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use crate::config::FileMode;

/// Everything the repository wants for one machine, with all paths made
/// absolute and all sets merged.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Desired {
    pub machine: String,
    pub brew: BTreeSet<String>,
    pub cask: BTreeSet<String>,
    pub files: Vec<ResolvedFile>,
    /// Shell fragments in the order they must be concatenated.
    pub fragments: Vec<Fragment>,
    /// Packages declared by sets this machine does *not* have switched on,
    /// keyed by package name.
    ///
    /// Without this, switching a set off makes everything it brought look
    /// like it was installed by hand and belongs to nothing — and the obvious
    /// reaction, ignoring it, writes a permanent per-machine exception for a
    /// package that is merely parked. The drift carries the set names so both
    /// front ends can say which set it is.
    pub inactive_brew: BTreeMap<String, Vec<String>>,
    pub inactive_cask: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedFile {
    pub set: String,
    pub source: PathBuf,
    pub target: PathBuf,
    pub mode: FileMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fragment {
    pub set: String,
    pub name: String,
    pub path: PathBuf,
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use crate::config::Repo;
    use crate::ports::fake::FakeFsys;

    fn repo_fs() -> FakeFsys {
        FakeFsys::from([
            ("/repo/dotfix.toml", "schema_version = 1\n"),
            (
                "/repo/sets/core/set.toml",
                "[packages]\nbrew = [\"alpha\"]\ncask = [\"charlie\"]\n\n[[files]]\nsource = \"files/rc.tmpl\"\ntarget = \"~/.rc\"\n",
            ),
            ("/repo/sets/core/files/rc.tmpl", "rc\n"),
            ("/repo/sets/core/shell/20-alias.zsh", "alias a=b\n"),
            ("/repo/sets/core/shell/00-first.zsh", "# first\n"),
            (
                "/repo/sets/web/set.toml",
                "[packages]\nbrew = [\"bravo\", \"alpha\"]\n",
            ),
            ("/repo/sets/web/shell/10-web.zsh", "export W=1\n"),
            (
                "/repo/sets/idle/set.toml",
                "[packages]\nbrew = [\"unused\"]\n",
            ),
            (
                "/repo/machines/box-one.toml",
                "sets = [\"core\", \"web\"]\n",
            ),
        ])
    }

    #[test]
    fn unions_packages_of_active_sets_only() {
        let fs = repo_fs();
        let repo = Repo::load(&fs, Path::new("/repo")).unwrap();
        let d = repo
            .resolve(&fs, "box-one", Path::new("/Users/test"))
            .unwrap();

        assert!(d.brew.contains("alpha"));
        assert!(d.brew.contains("bravo"));
        assert!(
            !d.brew.contains("unused"),
            "inactive set must not contribute"
        );
        assert!(d.cask.contains("charlie"));
    }

    #[test]
    fn orders_fragments_by_numeric_prefix_then_set_order() {
        let fs = repo_fs();
        let repo = Repo::load(&fs, Path::new("/repo")).unwrap();
        let d = repo
            .resolve(&fs, "box-one", Path::new("/Users/test"))
            .unwrap();

        let names: Vec<&str> = d.fragments.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["00-first.zsh", "10-web.zsh", "20-alias.zsh"]);
    }

    #[test]
    fn expands_tilde_in_file_targets() {
        let fs = repo_fs();
        let repo = Repo::load(&fs, Path::new("/repo")).unwrap();
        let d = repo
            .resolve(&fs, "box-one", Path::new("/Users/test"))
            .unwrap();

        assert_eq!(d.files.len(), 1);
        assert_eq!(d.files[0].target, PathBuf::from("/Users/test/.rc"));
        assert_eq!(
            d.files[0].source,
            PathBuf::from("/repo/sets/core/files/rc.tmpl")
        );
        assert_eq!(d.files[0].set, "core");
    }

    #[test]
    fn rejects_an_unknown_machine() {
        let fs = repo_fs();
        let repo = Repo::load(&fs, Path::new("/repo")).unwrap();
        let err = repo
            .resolve(&fs, "ghost", Path::new("/Users/test"))
            .unwrap_err();
        assert!(matches!(err, crate::Error::UnknownMachine(ref m) if m == "ghost"));
    }
}

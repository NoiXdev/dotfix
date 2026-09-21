use std::path::{Path, PathBuf};

use crate::config::Repo;
use crate::drift::{Drift, PackageRef};
use crate::error::{Error, Result};
use crate::ports::Fsys;

/// Filename shapes that almost always mean credentials. Matching one blocks
/// adoption — the most common way secrets reach a dotfiles repository is an
/// unconsidered "adopt everything".
pub const DENY_PATTERNS: &[&str] = &[
    "id_",
    ".pem",
    ".key",
    ".netrc",
    "credentials",
    ".s3cfg",
    "token",
    ".p12",
    ".keychain",
];

pub fn is_denied(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let full = path.to_string_lossy().to_ascii_lowercase();

    DENY_PATTERNS.iter().any(|p| {
        if p.starts_with('.') {
            name.ends_with(p)
        } else {
            name.contains(p) || full.contains(&format!("/{p}"))
        }
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Proposal {
    AddPackage {
        package: PackageRef,
        set: String,
    },
    DropPackage {
        package: PackageRef,
        set: String,
    },
    IgnorePackage {
        package: PackageRef,
    },
    WriteBackFile {
        target: PathBuf,
        source: PathBuf,
    },
    /// Record software that is here but no set declares. The counterpart to
    /// adopting a package: detection only runs at `init`, so anything
    /// installed later had no way in.
    DeclareRequirement {
        requirement: crate::config::Requirement,
        set: String,
    },
    RefuseFile {
        target: PathBuf,
        reason: String,
    },
}

/// The choices a user is offered for one piece of drift.
/// Software this machine has that no active set declares.
///
/// The mirror of an unmanaged package. Detection only ran at `init`, so
/// anything installed afterwards had no way into the repository short of
/// writing the entry by hand.
pub fn undeclared_requirements(
    repo: &Repo,
    active: &[String],
    fs: &dyn Fsys,
    home: &std::path::Path,
    default_set: &str,
) -> Vec<Proposal> {
    let declared: std::collections::BTreeSet<&str> = active
        .iter()
        .filter_map(|s| repo.sets.get(s))
        .flat_map(|s| s.requires.iter().map(|r| r.name.as_str()))
        .collect();

    crate::init::detect_requirements(fs, home)
        .into_iter()
        .filter(|r| !declared.contains(r.name.as_str()))
        .map(|requirement| Proposal::DeclareRequirement {
            requirement,
            set: default_set.to_string(),
        })
        .collect()
}

pub fn proposals_for(drift: &Drift, repo: &Repo, default_set: &str) -> Vec<Proposal> {
    match drift {
        Drift::Unmanaged { package, .. } => vec![
            Proposal::AddPackage {
                package: package.clone(),
                set: default_set.to_string(),
            },
            Proposal::IgnorePackage {
                package: package.clone(),
            },
        ],

        Drift::LocallyRemoved(package) => {
            let owner = owning_set(repo, package).unwrap_or_else(|| default_set.to_string());
            vec![Proposal::DropPackage {
                package: package.clone(),
                set: owner,
            }]
        }

        Drift::LocalEdit {
            target,
            set,
            contains_secrets,
        } => {
            // A file rendered from a template that resolves a secret holds
            // the real value on disk, at 0600. Writing it back would copy
            // that value into the repository at 0644 and push it — and would
            // replace the `{{ secret("name") }}` placeholder with the secret
            // itself, so the template is destroyed in the same stroke.
            //
            // The filename denylist below cannot catch this: the file is a
            // `.zshrc` or a `.gitconfig`, not something that looks like a
            // credential. Only the render knows, which is why the flag is
            // carried on the drift.
            if *contains_secrets {
                return vec![Proposal::RefuseFile {
                    target: target.clone(),
                    reason: "it is rendered from a template that resolves a secret — \
                             writing it back would commit the resolved value and lose \
                             the `{{ secret(\"name\") }}` placeholder. Edit the \
                             template in the repository instead"
                        .to_string(),
                }];
            }
            if is_denied(target) {
                return vec![Proposal::RefuseFile {
                    target: target.clone(),
                    reason: "looks like a credential file — add it as a template with \
                             `{{ secret(\"name\") }}` instead"
                        .to_string(),
                }];
            }
            // The assembled `.zshrc` is not one file's output — it is the
            // concatenation of the `shell/*.zsh` fragments of every active
            // set, which is why it carries a pseudo set name rather than a
            // real one. Writing it back would need a file that does not
            // exist, so say what to edit instead.
            if set == crate::engine::GENERATED_SET {
                return vec![Proposal::RefuseFile {
                    target: target.clone(),
                    reason: "it is assembled from the `shell/*.zsh` fragments of every \
                             active set, so there is no single file to write it back \
                             to. Edit the fragments under `sets/<set>/shell/` instead"
                        .to_string(),
                }];
            }

            // No `[[files]]` entry means nothing in this set renders that
            // path, so there is no file to write back *to*. The old fallback
            // produced the set directory itself, which accepting then tried
            // to overwrite with the file's contents.
            let Some(entry) = repo.sets.get(set).and_then(|s| {
                s.files
                    .iter()
                    .find(|f| target.ends_with(trim_tilde(&f.target)))
            }) else {
                return vec![Proposal::RefuseFile {
                    target: target.clone(),
                    reason: format!(
                        "no file in set `{set}` renders it, so there is nowhere to write \
                         it back to — add a `[[files]]` entry in `sets/{set}/set.toml` \
                         first"
                    ),
                }];
            };
            let source = repo.root.join("sets").join(set).join(&entry.source);
            vec![Proposal::WriteBackFile {
                target: target.clone(),
                source,
            }]
        }

        _ => vec![],
    }
}

fn trim_tilde(target: &str) -> &str {
    target.strip_prefix("~/").unwrap_or(target)
}

fn owning_set(repo: &Repo, package: &PackageRef) -> Option<String> {
    repo.sets.iter().find_map(|(name, set)| {
        let list = if package.cask {
            &set.packages.cask
        } else {
            &set.packages.brew
        };
        list.contains(&package.name).then(|| name.clone())
    })
}

/// Write one accepted proposal back into the repository.
pub fn apply_proposal(
    proposal: &Proposal,
    repo: &Repo,
    machine: &str,
    fs: &dyn Fsys,
) -> Result<()> {
    match proposal {
        Proposal::AddPackage { package, set } => {
            let mut cfg = repo
                .sets
                .get(set)
                .cloned()
                .ok_or_else(|| Error::Config(format!("unknown set `{set}`")))?;
            let list = if package.cask {
                &mut cfg.packages.cask
            } else {
                &mut cfg.packages.brew
            };
            if !list.contains(&package.name) {
                list.push(package.name.clone());
                list.sort();
            }
            write_packages(fs, repo, set, package.cask, list)
        }

        Proposal::DropPackage { package, set } => {
            let mut cfg = repo
                .sets
                .get(set)
                .cloned()
                .ok_or_else(|| Error::Config(format!("unknown set `{set}`")))?;
            let list = if package.cask {
                &mut cfg.packages.cask
            } else {
                &mut cfg.packages.brew
            };
            list.retain(|p| p != &package.name);
            write_packages(fs, repo, set, package.cask, list)
        }

        Proposal::IgnorePackage { package } => {
            let mut cfg = repo
                .machines
                .get(machine)
                .cloned()
                .ok_or_else(|| Error::UnknownMachine(machine.to_string()))?;
            if !cfg.ignore.contains(&package.name) {
                cfg.ignore.push(package.name.clone());
                cfg.ignore.sort();
            }
            let path = machine_path(repo, machine);
            let raw =
                crate::config::edit::put_strings(&fs.read(&path)?, &[], "ignore", &cfg.ignore)?;
            fs.write(&path, &raw, 0o644)
        }

        Proposal::WriteBackFile { target, source } => {
            let content = fs.read(target)?;
            fs.write(source, &content, 0o644)
        }

        Proposal::DeclareRequirement { requirement, set } => {
            // Appended rather than serialised: the set file keeps its
            // comments, its ordering, and the quoting its author chose.
            let path = set_path(repo, set);
            let mut pairs: Vec<(&str, &str)> = vec![("name", requirement.name.as_str())];
            if let Some(p) = &requirement.path {
                pairs.push(("path", p.as_str()));
            }
            if let Some(c) = &requirement.command {
                pairs.push(("command", c.as_str()));
            }
            if let Some(h) = &requirement.hint {
                pairs.push(("hint", h.as_str()));
            }
            let raw = crate::config::edit::push_table(&fs.read(&path)?, "requires", &pairs)?;
            fs.write(&path, &raw, 0o644)
        }

        Proposal::RefuseFile { target, reason } => Err(Error::Config(format!(
            "refusing to adopt {}: {reason}",
            target.display()
        ))),
    }
}

fn set_path(repo: &Repo, set: &str) -> PathBuf {
    repo.root.join("sets").join(set).join("set.toml")
}

fn machine_path(repo: &Repo, machine: &str) -> PathBuf {
    repo.root.join("machines").join(format!("{machine}.toml"))
}

/// Write one package list back into a set, leaving the rest of `set.toml`
/// exactly as its author wrote it.
///
/// Serialising the parsed struct was simpler and silently destroyed every
/// comment in the file, plus the author's field order. These files are meant
/// to be hand-edited; taking that away is not a fair price for adopting a
/// package.
fn write_packages(
    fs: &dyn Fsys,
    repo: &Repo,
    set: &str,
    cask: bool,
    list: &[String],
) -> Result<()> {
    let path = set_path(repo, set);
    let key = if cask { "cask" } else { "brew" };
    let raw = crate::config::edit::put_strings(&fs.read(&path)?, &["packages"], key, list)?;
    fs.write(&path, &raw, 0o644)
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::config::Repo;
    use crate::drift::{Drift, PackageRef};
    use crate::ports::fake::FakeFsys;

    fn fs() -> FakeFsys {
        FakeFsys::from([
            ("/repo/dotfix.toml", "schema_version = 1\n"),
            (
                "/repo/sets/core/set.toml",
                "[[files]]\nsource = \"files/rc\"\ntarget = \"~/.rc\"\n\n\
                 [packages]\nbrew = [\"alpha\"]\n",
            ),
            ("/repo/machines/box-one.toml", "sets = [\"core\"]\n"),
        ])
    }

    #[test]
    fn credential_shaped_files_are_denied() {
        assert!(is_denied(Path::new("/Users/test/.ssh/id_ed25519")));
        assert!(is_denied(Path::new("/Users/test/cert.pem")));
        assert!(is_denied(Path::new("/Users/test/.netrc")));
        assert!(is_denied(Path::new("/Users/test/.aws/credentials")));
        assert!(is_denied(Path::new("/Users/test/.s3cfg")));
        assert!(is_denied(Path::new("/Users/test/.github_token")));
    }

    #[test]
    fn ordinary_config_files_are_not_denied() {
        assert!(!is_denied(Path::new("/Users/test/.gitconfig")));
        assert!(!is_denied(Path::new("/Users/test/.zshrc")));
    }

    #[test]
    fn an_unmanaged_package_offers_adding_to_a_set_or_ignoring() {
        let fs = fs();
        let repo = Repo::load(&fs, Path::new("/repo")).unwrap();
        let out = proposals_for(
            &Drift::Unmanaged {
                package: PackageRef::formula("bravo"),
                declared_in: Vec::new(),
            },
            &repo,
            "core",
        );
        assert_eq!(
            out,
            vec![
                Proposal::AddPackage {
                    package: PackageRef::formula("bravo"),
                    set: "core".into(),
                },
                Proposal::IgnorePackage {
                    package: PackageRef::formula("bravo"),
                },
            ]
        );
    }

    #[test]
    fn a_locally_removed_package_offers_dropping_it_from_its_set() {
        let fs = fs();
        let repo = Repo::load(&fs, Path::new("/repo")).unwrap();
        let out = proposals_for(
            &Drift::LocallyRemoved(PackageRef::formula("alpha")),
            &repo,
            "core",
        );
        assert_eq!(
            out,
            vec![Proposal::DropPackage {
                package: PackageRef::formula("alpha"),
                set: "core".into(),
            }]
        );
    }

    #[test]
    fn a_local_edit_of_a_credential_file_is_refused_not_adopted() {
        let fs = fs();
        let repo = Repo::load(&fs, Path::new("/repo")).unwrap();
        let out = proposals_for(
            &Drift::LocalEdit {
                target: PathBuf::from("/Users/test/.s3cfg"),
                set: "infra".into(),
                contains_secrets: false,
            },
            &repo,
            "core",
        );
        assert!(matches!(out[0], Proposal::RefuseFile { .. }));
    }

    #[test]
    fn a_local_edit_of_a_file_that_resolves_a_secret_is_refused() {
        // The leak this closes: `~/.zshrc` rendered from a template with
        // `{{ secret("api_key") }}` holds the real key on disk at 0600.
        // `dotfix adopt` would copy that content into the repository at 0644
        // and push it — and the placeholder would be gone, replaced by the
        // secret itself. The filename denylist cannot catch it: a `.zshrc`
        // looks like nothing dangerous.
        let fs = fs();
        let repo = Repo::load(&fs, Path::new("/repo")).unwrap();
        let out = proposals_for(
            &Drift::LocalEdit {
                target: PathBuf::from("/Users/test/.zshrc"),
                set: "core".into(),
                contains_secrets: true,
            },
            &repo,
            "core",
        );

        match &out[0] {
            Proposal::RefuseFile { reason, .. } => {
                assert!(reason.contains("secret"), "{reason}");
                assert!(reason.contains("template"), "{reason}");
            }
            other => panic!("must refuse, got {other:?}"),
        }
    }

    #[test]
    fn a_local_edit_of_a_path_no_file_renders_is_refused_not_aimed_at_a_directory() {
        // Found on a real Mac: after `dotfix init --set-up-new` the first
        // drift is the generated `~/.zshrc`, and `core` has `files = []`. The
        // destination computed for it was the set *directory*, so accepting
        // the proposal tried to write the whole file over `sets/core`.
        let fs = fs();
        let repo = Repo::load(&fs, Path::new("/repo")).unwrap();
        let out = proposals_for(
            &Drift::LocalEdit {
                target: PathBuf::from("/Users/test/.zshrc"),
                set: crate::engine::GENERATED_SET.into(),
                contains_secrets: false,
            },
            &repo,
            "core",
        );

        match &out[0] {
            Proposal::RefuseFile { reason, .. } => {
                assert!(reason.contains("shell/*.zsh"), "{reason}");
                assert!(
                    !reason.contains("sets/generated"),
                    "must not name a path that cannot exist: {reason}"
                );
            }
            Proposal::WriteBackFile { source, .. } => {
                panic!("aimed at {} — which is the set directory", source.display())
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn an_ordinary_local_edit_is_still_written_back() {
        // The guard must not turn every hand edit into a refusal — writing a
        // tweaked plain file back into the repository is the feature.
        let fs = fs();
        let repo = Repo::load(&fs, Path::new("/repo")).unwrap();
        let out = proposals_for(
            &Drift::LocalEdit {
                target: PathBuf::from("/Users/test/.rc"),
                set: "core".into(),
                contains_secrets: false,
            },
            &repo,
            "core",
        );
        match &out[0] {
            Proposal::WriteBackFile { source, .. } => assert_eq!(
                source,
                &PathBuf::from("/repo/sets/core/files/rc"),
                "must aim at the file that renders it"
            ),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn adding_a_package_rewrites_the_set_file() {
        let fs = fs();
        let repo = Repo::load(&fs, Path::new("/repo")).unwrap();
        apply_proposal(
            &Proposal::AddPackage {
                package: PackageRef::formula("bravo"),
                set: "core".into(),
            },
            &repo,
            "box-one",
            &fs,
        )
        .unwrap();

        let written = fs.read(Path::new("/repo/sets/core/set.toml")).unwrap();
        assert!(written.contains("\"alpha\""));
        assert!(written.contains("\"bravo\""));
    }

    #[test]
    fn ignoring_a_package_rewrites_the_machine_file() {
        let fs = fs();
        let repo = Repo::load(&fs, Path::new("/repo")).unwrap();
        apply_proposal(
            &Proposal::IgnorePackage {
                package: PackageRef::formula("bravo"),
            },
            &repo,
            "box-one",
            &fs,
        )
        .unwrap();

        let written = fs.read(Path::new("/repo/machines/box-one.toml")).unwrap();
        assert!(written.contains("bravo"));
    }
}

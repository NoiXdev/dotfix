use std::path::{Path, PathBuf};

use crate::drift::{Drift, PackageRef};
use crate::engine::{Engine, Inspection};
use crate::error::Result;
use crate::paths::Paths;
use crate::render::zshrc::checksum;
use crate::state::Applied;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    InstallPackage(PackageRef),
    UninstallPackage(PackageRef),
    WriteFile {
        target: PathBuf,
        content: String,
        mode: u32,
    },
    Symlink {
        source: PathBuf,
        target: PathBuf,
    },
    RemoveFile {
        target: PathBuf,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    pub actions: Vec<Action>,
}

impl Plan {
    pub fn is_empty(&self) -> bool {
        self.actions.is_empty()
    }

    /// One human-readable line per action, for the confirmation prompt.
    pub fn describe(&self) -> Vec<String> {
        self.actions
            .iter()
            .map(|a| match a {
                Action::InstallPackage(p) => format!("install {}", p.name),
                Action::UninstallPackage(p) => format!("uninstall {}", p.name),
                Action::WriteFile { target, .. } => format!("write {}", target.display()),
                Action::Symlink { target, .. } => format!("link {}", target.display()),
                Action::RemoveFile { target } => format!("remove {}", target.display()),
            })
            .collect()
    }
}

/// Turn a report into executable actions.
///
/// Only `Incoming*` and `Removed*` are actionable. `Unmanaged`, `LocallyRemoved`
/// and `LocalEdit` are questions for `adopt`, never for `apply`.
pub fn plan(inspection: &Inspection) -> Plan {
    plan_where(inspection, |_| true)
}

/// Plan only the drift items whose [`Drift::id`] is in `ids`. Ids that no
/// longer match anything are skipped: the caller may have been looking at a
/// stale view.
pub fn plan_selected(inspection: &Inspection, ids: &[String]) -> Plan {
    plan_where(inspection, |d| ids.iter().any(|id| *id == d.id()))
}

/// Plan a per-file "discard my edit, take the repository's version" for
/// specific `Drift::LocalEdit` items. Deliberately a separate function from
/// [`plan_where`], not a new arm on it: bulk apply must never be able to
/// clobber a hand edit (see [`plan`]'s doc comment and
/// `never_plans_anything_for_unmanaged_or_local_edits`), while this is the
/// explicit, single-file opposite of that safety property, only ever
/// reachable by naming an id directly.
pub fn plan_overwrite(inspection: &Inspection, ids: &[String]) -> Plan {
    let mut actions = Vec::new();

    for item in inspection
        .report
        .items
        .iter()
        .filter(|d| ids.iter().any(|id| *id == d.id()))
    {
        if let Drift::LocalEdit { target, .. } = item
            && let Some(r) = inspection
                .rendered
                .iter()
                .find(|r| r.file.target == *target)
        {
            actions.push(Action::WriteFile {
                target: target.clone(),
                content: r.content.clone(),
                mode: if r.contains_secrets { 0o600 } else { 0o644 },
            });
        }
    }

    Plan { actions }
}

fn plan_where(inspection: &Inspection, keep: impl Fn(&Drift) -> bool) -> Plan {
    let mut actions = Vec::new();

    for item in inspection.report.items.iter().filter(|d| keep(d)) {
        match item {
            Drift::IncomingPackage(p) => actions.push(Action::InstallPackage(p.clone())),

            Drift::RemovedPackage {
                package,
                blocked_by,
                ..
            } if blocked_by.is_empty() => actions.push(Action::UninstallPackage(package.clone())),

            Drift::IncomingFile { target, .. } => {
                if let Some(r) = inspection
                    .rendered
                    .iter()
                    .find(|r| r.file.target == *target)
                {
                    match r.file.mode {
                        crate::config::FileMode::Symlink => actions.push(Action::Symlink {
                            source: r.file.source.clone(),
                            target: target.clone(),
                        }),
                        _ => actions.push(Action::WriteFile {
                            target: target.clone(),
                            content: r.content.clone(),
                            mode: if r.contains_secrets { 0o600 } else { 0o644 },
                        }),
                    }
                }
            }

            Drift::RemovedFile { target } => actions.push(Action::RemoveFile {
                target: target.clone(),
            }),

            _ => {}
        }
    }

    Plan { actions }
}

/// Execute a plan and return the new applied state. Every overwrite is backed
/// up first; a backup of a secret-bearing file keeps mode 0600.
pub fn execute(plan: &Plan, engine: &Engine<'_>, stamp: &str) -> Result<Applied> {
    let mut applied = Applied::load(engine.fs, &engine.paths.applied())?;

    for action in &plan.actions {
        match action {
            Action::InstallPackage(p) => {
                engine.brew.install(&p.name, p.cask)?;
                if p.cask {
                    applied.cask.insert(p.name.clone());
                } else {
                    applied.brew.insert(p.name.clone());
                }
            }
            Action::UninstallPackage(p) => {
                engine.brew.uninstall(&p.name, p.cask)?;
                if p.cask {
                    applied.cask.remove(&p.name);
                } else {
                    applied.brew.remove(&p.name);
                }
            }
            Action::WriteFile {
                target,
                content,
                mode,
            } => {
                backup(engine, &engine.paths, target, stamp, *mode)?;
                engine.fs.write(target, content, *mode)?;
                applied.files.insert(target.clone(), checksum(content));
            }
            Action::Symlink { source, target } => {
                engine.fs.symlink(source, target)?;
            }
            Action::RemoveFile { target } => {
                backup(engine, &engine.paths, target, stamp, 0o600)?;
                engine.fs.remove(target)?;
                applied.files.remove(target);
            }
        }
    }

    applied.save(engine.fs, &engine.paths.applied())?;
    Ok(applied)
}

fn backup(engine: &Engine<'_>, paths: &Paths, target: &Path, stamp: &str, mode: u32) -> Result<()> {
    if !engine.fs.exists(target) {
        return Ok(());
    }
    let existing = engine.fs.read(target)?;
    let dest = paths.backups(stamp).join(flatten(target));
    engine.fs.write(&dest, &existing, mode)
}

/// `/Users/test/.rc` → `Users_test_.rc`, so one backup directory holds files
/// from anywhere without nesting.
fn flatten(target: &Path) -> String {
    target
        .to_string_lossy()
        .trim_start_matches('/')
        .replace('/', "_")
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::config::{FileMode, ResolvedFile};
    use crate::drift::files::RenderedFile;
    use crate::drift::{Drift, PackageRef, Report};
    use crate::ports::Fsys;
    use crate::ports::fake::{FakeBrew, FakeExec, FakeFsys, FakeGit};

    fn rendered(target: &str, content: &str, secrets: bool) -> RenderedFile {
        RenderedFile {
            file: ResolvedFile {
                set: "core".into(),
                source: PathBuf::from("/repo/sets/core/files/x.tmpl"),
                target: PathBuf::from(target),
                mode: FileMode::Template,
            },
            content: content.to_string(),
            contains_secrets: secrets,
            secret_values: vec![],
        }
    }

    fn inspection(items: Vec<Drift>, rendered: Vec<RenderedFile>) -> Inspection {
        Inspection {
            desired: Default::default(),
            rendered,
            report: Report {
                items,
                missing: Vec::new(),
            },
        }
    }

    #[test]
    fn plans_installs_for_incoming_packages() {
        let p = plan(&inspection(
            vec![Drift::IncomingPackage(PackageRef::formula("alpha"))],
            vec![],
        ));
        assert_eq!(
            p.actions,
            vec![Action::InstallPackage(PackageRef::formula("alpha"))]
        );
    }

    #[test]
    fn skips_removals_that_are_still_depended_on() {
        let p = plan(&inspection(
            vec![Drift::RemovedPackage {
                package: PackageRef::formula("alpha"),
                blocked_by: vec!["bravo".into()],
                declared_in: Vec::new(),
            }],
            vec![],
        ));
        assert!(p.is_empty(), "a blocked removal must never be executed");
    }

    #[test]
    fn never_plans_anything_for_unmanaged_or_local_edits() {
        let p = plan(&inspection(
            vec![
                Drift::Unmanaged {
                    package: PackageRef::formula("stray"),
                    declared_in: Vec::new(),
                },
                Drift::LocallyRemoved(PackageRef::formula("gone")),
                Drift::LocalEdit {
                    target: PathBuf::from("/Users/test/.rc"),
                    set: "core".into(),
                    contains_secrets: false,
                },
            ],
            vec![],
        ));
        assert!(p.is_empty(), "those classes belong to adopt, not apply");
    }

    #[test]
    fn writes_files_containing_secrets_with_mode_600() {
        let (fs, brew, git, exec) = (
            FakeFsys::new(),
            FakeBrew::new([], []),
            FakeGit::new(),
            FakeExec::new([]),
        );
        let engine = Engine {
            fs: &fs,
            brew: &brew,
            git: &git,
            exec: &exec,
            paths: Paths::new(PathBuf::from("/Users/test")),
            user: "test".into(),
        };
        let insp = inspection(
            vec![Drift::IncomingFile {
                target: PathBuf::from("/Users/test/.s3cfg"),
                set: "infra".into(),
            }],
            vec![rendered("/Users/test/.s3cfg", "key = value", true)],
        );
        execute(&plan(&insp), &engine, "20260916-101500").unwrap();

        assert_eq!(
            fs.read(Path::new("/Users/test/.s3cfg")).unwrap(),
            "key = value"
        );
        assert_eq!(fs.mode_of(Path::new("/Users/test/.s3cfg")), Some(0o600));
    }

    /// End to end through `Engine::inspect`, `plan` and `execute`: a shell
    /// fragment that resolves a secret must make the generated `.zshrc`
    /// come out of the engine with `contains_secrets: true`, and that must
    /// in turn make `execute` write it 0600. This is the regression path
    /// for the "generated .zshrc always written 0644" bug — a test that
    /// only hand-builds a `RenderedFile` would not exercise it.
    #[test]
    fn writes_the_generated_zshrc_with_mode_600_when_a_fragment_resolves_a_secret() {
        let fs = FakeFsys::from([
            ("/repo/dotfix.toml", "schema_version = 1\n"),
            (
                "/repo/sets/core/set.toml",
                "[packages]\nbrew = [\"alpha\"]\n",
            ),
            (
                "/repo/sets/core/shell/10-path.zsh",
                "export TOKEN={{ secret(\"api_key\") }}\n",
            ),
            ("/repo/machines/box-one.toml", "sets = [\"core\"]\n"),
        ]);
        let brew = FakeBrew::new([], []);
        let git = FakeGit::new();
        // Default keychain reference for a secret named `api_key` is
        // `dotfix/api_key`, i.e. service `dotfix`, account `api_key`.
        let exec = FakeExec::new([(
            "security find-generic-password -s dotfix -a api_key -w",
            "s3cr3t\n",
        )]);
        let engine = Engine {
            fs: &fs,
            brew: &brew,
            git: &git,
            exec: &exec,
            paths: Paths::new(PathBuf::from("/Users/test")),
            user: "test".into(),
        };
        let local = crate::paths::LocalConfig {
            repo: PathBuf::from("/repo"),
            machine: "box-one".into(),
        };

        let insp = engine.inspect(&local).unwrap();
        execute(&plan(&insp), &engine, "20260916-101500").unwrap();

        assert_eq!(
            fs.mode_of(&engine.paths.zshrc()),
            Some(0o600),
            "a .zshrc carrying a secret must be written 0600, never 0644"
        );
    }

    #[test]
    fn backs_up_an_existing_file_before_overwriting_it() {
        let fs = FakeFsys::from([("/Users/test/.rc", "old content")]);
        let (brew, git, exec) = (FakeBrew::new([], []), FakeGit::new(), FakeExec::new([]));
        let engine = Engine {
            fs: &fs,
            brew: &brew,
            git: &git,
            exec: &exec,
            paths: Paths::new(PathBuf::from("/Users/test")),
            user: "test".into(),
        };
        let insp = inspection(
            vec![Drift::IncomingFile {
                target: PathBuf::from("/Users/test/.rc"),
                set: "core".into(),
            }],
            vec![rendered("/Users/test/.rc", "new content", false)],
        );
        execute(&plan(&insp), &engine, "20260916-101500").unwrap();

        let backup =
            PathBuf::from("/Users/test/.local/state/dotfix/backups/20260916-101500/Users_test_.rc");
        assert_eq!(fs.read(&backup).unwrap(), "old content");
        assert_eq!(
            fs.read(Path::new("/Users/test/.rc")).unwrap(),
            "new content"
        );
    }

    #[test]
    fn execute_returns_the_new_applied_state() {
        let (fs, brew, git, exec) = (
            FakeFsys::new(),
            FakeBrew::new([], []),
            FakeGit::new(),
            FakeExec::new([]),
        );
        let engine = Engine {
            fs: &fs,
            brew: &brew,
            git: &git,
            exec: &exec,
            paths: Paths::new(PathBuf::from("/Users/test")),
            user: "test".into(),
        };
        let insp = inspection(
            vec![
                Drift::IncomingPackage(PackageRef::formula("alpha")),
                Drift::IncomingFile {
                    target: PathBuf::from("/Users/test/.rc"),
                    set: "core".into(),
                },
            ],
            vec![rendered("/Users/test/.rc", "body", false)],
        );
        let applied = execute(&plan(&insp), &engine, "20260916-101500").unwrap();

        assert!(applied.brew.contains("alpha"));
        assert_eq!(
            applied.files[&PathBuf::from("/Users/test/.rc")],
            crate::render::zshrc::checksum("body")
        );
        assert_eq!(brew.installed(), vec!["alpha".to_string()]);
    }

    #[test]
    fn plan_selected_takes_only_the_requested_ids() {
        let insp = inspection(
            vec![
                Drift::IncomingPackage(PackageRef::formula("alpha")),
                Drift::IncomingPackage(PackageRef::formula("bravo")),
            ],
            vec![],
        );
        let p = plan_selected(&insp, &["incoming_package:formula:bravo".to_string()]);
        assert_eq!(
            p.actions,
            vec![Action::InstallPackage(PackageRef::formula("bravo"))]
        );
    }

    #[test]
    fn plan_selected_ignores_ids_that_no_longer_exist() {
        let insp = inspection(
            vec![Drift::IncomingPackage(PackageRef::formula("alpha"))],
            vec![],
        );
        let p = plan_selected(&insp, &["incoming_package:formula:vanished".to_string()]);
        assert!(
            p.is_empty(),
            "a stale selection must be skipped, never guessed at"
        );
    }

    #[test]
    fn plan_selected_still_refuses_a_blocked_removal() {
        let blocked = Drift::RemovedPackage {
            package: PackageRef::formula("alpha"),
            blocked_by: vec!["bravo".into()],
            declared_in: Vec::new(),
        };
        let id = blocked.id();
        let insp = inspection(vec![blocked], vec![]);
        assert!(plan_selected(&insp, &[id]).is_empty());
    }

    #[test]
    fn plan_overwrite_writes_the_rendered_content_for_a_local_edit() {
        let edit = Drift::LocalEdit {
            target: PathBuf::from("/Users/test/.gitconfig"),
            set: "core".into(),
            contains_secrets: false,
        };
        let id = edit.id();
        let insp = inspection(
            vec![edit],
            vec![rendered("/Users/test/.gitconfig", "repo content", false)],
        );
        let p = plan_overwrite(&insp, &[id]);
        assert_eq!(
            p.actions,
            vec![Action::WriteFile {
                target: PathBuf::from("/Users/test/.gitconfig"),
                content: "repo content".to_string(),
                mode: 0o644,
            }]
        );
    }

    #[test]
    fn plan_overwrite_skips_an_id_that_matches_nothing() {
        let insp = inspection(vec![], vec![]);
        let p = plan_overwrite(&insp, &["local_edit:/Users/test/.rc".to_string()]);
        assert!(
            p.is_empty(),
            "a stale selection must be skipped, never guessed at"
        );
    }

    #[test]
    fn plan_overwrite_ignores_a_non_local_edit_id() {
        let incoming = Drift::IncomingPackage(PackageRef::formula("alpha"));
        let id = incoming.id();
        let insp = inspection(vec![incoming], vec![]);
        let p = plan_overwrite(&insp, &[id]);
        assert!(
            p.is_empty(),
            "plan_overwrite must not become a second general-purpose apply"
        );
    }

    #[test]
    fn plan_overwrite_writes_mode_600_when_the_rendered_file_carries_a_secret() {
        let edit = Drift::LocalEdit {
            target: PathBuf::from("/Users/test/.s3cfg"),
            set: "infra".into(),
            contains_secrets: false,
        };
        let id = edit.id();
        let insp = inspection(
            vec![edit],
            vec![rendered("/Users/test/.s3cfg", "key = value", true)],
        );
        let p = plan_overwrite(&insp, &[id]);
        assert_eq!(
            p.actions,
            vec![Action::WriteFile {
                target: PathBuf::from("/Users/test/.s3cfg"),
                content: "key = value".to_string(),
                mode: 0o600,
            }]
        );
    }

    #[test]
    fn plan_with_every_id_equals_plan() {
        let insp = inspection(
            vec![
                Drift::IncomingPackage(PackageRef::formula("alpha")),
                Drift::RemovedPackage {
                    package: PackageRef::formula("gone"),
                    blocked_by: vec![],
                    declared_in: Vec::new(),
                },
            ],
            vec![],
        );
        let all: Vec<String> = insp.report.items.iter().map(Drift::id).collect();
        assert_eq!(plan_selected(&insp, &all), plan(&insp));
    }
}

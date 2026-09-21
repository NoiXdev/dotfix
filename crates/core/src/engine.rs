use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::config::{Desired, FileMode, Repo, ResolvedFile};
use crate::drift::files::RenderedFile;
use crate::drift::{Drift, Report, files, packages};
use crate::error::{Error, Result};
use crate::paths::{LocalConfig, Paths};
use crate::ports::{Brew, Exec, Fsys, Git};
use crate::render::{Rendered, Vars, render, zshrc};
use crate::secrets::{Resolver, provider_for};
use crate::state::Applied;

/// Read the installed packages a second time before believing an uninstall.
///
/// A cold Homebrew — one whose API cache has not been populated — can report
/// a *dependency* as a leaf, because it has not resolved the tap of the
/// package that requires it. Measured on a real machine: the first reading
/// listed `mkcert` and omitted `ddev/ddev/ddev`; the second, warm reading did
/// the opposite. Acting on the first would have proposed uninstalling
/// `mkcert` and broken `ddev`, and `brew uses --installed` cannot catch it
/// because it is blind in exactly the same way.
///
/// So the check is not a smarter query but a second look: if a package
/// queued for removal is no longer reported as installed, the first reading
/// was not trustworthy and the item is dropped. Deliberately one-sided — a
/// re-read that reveals *more* packages only delays an adoption prompt to the
/// next run, while a wrong uninstall destroys something.
///
/// The extra call happens only when an uninstall is actually on the table,
/// which is rare; an unchanged reading costs nothing but that one call.
fn confirm_uninstalls(items: Vec<Drift>, brew: &dyn Brew) -> Result<Vec<Drift>> {
    if !items
        .iter()
        .any(|i| matches!(i, Drift::RemovedPackage { .. }))
    {
        return Ok(items);
    }

    let still_installed: BTreeSet<String> = brew.leaves()?.into_iter().collect();
    Ok(items
        .into_iter()
        .filter(|item| match item {
            Drift::RemovedPackage { package, .. } if !package.cask => {
                still_installed.contains(&package.name)
            }
            _ => true,
        })
        .collect())
}

/// Which declared requirements are absent on this machine.
///
/// Only the active sets: a set that is switched off is not asking for
/// anything. Presence is a filesystem question — either a path exists, or a
/// name is on `PATH` — so nothing is executed to find out. That matters
/// more than it sounds: running a candidate to see whether it is installed
/// is how a check turns into an unintended side effect.
fn missing_requirements(
    repo: &Repo,
    active: &[String],
    fs: &dyn Fsys,
    home: &Path,
) -> Vec<crate::drift::Missing> {
    let mut out = Vec::new();
    for name in active {
        let Some(set) = repo.sets.get(name) else {
            continue;
        };
        for req in &set.requires {
            if requirement_present(req, fs, home) {
                continue;
            }
            out.push(crate::drift::Missing {
                name: req.name.clone(),
                set: name.clone(),
                hint: req.hint.clone(),
            });
        }
    }
    out
}

fn requirement_present(req: &crate::config::Requirement, fs: &dyn Fsys, home: &Path) -> bool {
    if let Some(path) = &req.path {
        let path = match path.strip_prefix("~/") {
            Some(rest) => home.join(rest),
            None => PathBuf::from(path),
        };
        return fs.exists(&path);
    }
    if let Some(command) = &req.command {
        return std::env::var_os("PATH")
            .map(|p| std::env::split_paths(&p).any(|dir| fs.exists(&dir.join(command))))
            .unwrap_or(false);
    }
    // Neither given: nothing to check, so nothing to report. A set author who
    // wrote an empty requirement gets silence rather than a permanent alarm.
    true
}

/// Pseudo set name for the generated `.zshrc`, which has no set of its own.
pub const GENERATED_SET: &str = "generated";

/// Wiring of the ports plus machine-local facts. Every command constructs one.
pub struct Engine<'a> {
    pub fs: &'a dyn Fsys,
    pub brew: &'a dyn Brew,
    pub git: &'a dyn Git,
    pub exec: &'a dyn Exec,
    pub paths: Paths,
    pub user: String,
}

/// What [`Engine::publish`] did.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Published {
    /// Files that were committed. Empty when there was nothing to commit —
    /// which is not the same as nothing to push.
    pub committed: Vec<String>,
    /// False when the repository has no remote, not when pushing failed.
    pub pushed: bool,
}

/// Everything a command needs: what should be, what was rendered, what differs.
pub struct Inspection {
    pub desired: Desired,
    pub rendered: Vec<RenderedFile>,
    pub report: Report,
}

impl Engine<'_> {
    /// Fast-forward the data repository. Never merges.
    ///
    /// A repository with no remote is a valid state — `init --set-up-new`
    /// produces exactly that until the user pushes it somewhere — so syncing
    /// it is a no-op rather than an error.
    pub fn sync(&self, local: &LocalConfig) -> Result<()> {
        if !self.git.has_remote(&local.repo)? {
            return Ok(());
        }
        self.git.fetch(&local.repo)?;
        self.git.pull_ff_only(&local.repo)
    }

    /// Commit what changed in the repository and send it to the remote.
    ///
    /// The counterpart `sync` never had. Everything dotfix writes — adopted
    /// packages, an edited set, a changed provider — landed in the working
    /// tree and stayed there, so a tool whose whole purpose is keeping
    /// several Macs in sync could pull but never publish, and changes
    /// reached the other machines only if the user remembered git.
    ///
    /// Commits everything in the repository, not a curated subset: it is a
    /// configuration repository, its whole content is the configuration, and
    /// deciding for the user which of their own edits to leave behind would
    /// be worse than committing all of them.
    pub fn publish(&self, local: &LocalConfig, message: &str) -> Result<Published> {
        let dirty = self.git.dirty_files(&local.repo)?;
        if !dirty.is_empty() {
            self.git.commit_all(&local.repo, message)?;
        }

        // No remote is a valid state — `init --set-up-new` leaves one that
        // way until the user points it somewhere — so there is nothing to
        // fail about, only nothing to send.
        let pushed = if self.git.has_remote(&local.repo)? {
            self.git.push(&local.repo)?;
            true
        } else {
            false
        };

        Ok(Published {
            committed: dirty,
            pushed,
        })
    }

    /// Render everything against a candidate provider, so a switch can fail
    /// before it is recorded rather than at the next `apply`.
    ///
    /// Nothing is written: the point is only whether every `secret("name")`
    /// the repository references can be found at the new provider. A missing
    /// one surfaces here, with the name in the error, instead of days later
    /// on a schedule with nobody watching.
    pub fn verify_secrets(
        &self,
        local: &LocalConfig,
        kind: crate::config::ProviderKind,
        vault: Option<&str>,
    ) -> Result<()> {
        let repo = Repo::load(self.fs, &local.repo)?;
        let machine = repo
            .machines
            .get(&local.machine)
            .ok_or_else(|| Error::UnknownMachine(local.machine.clone()))?
            .clone();
        let desired = repo.resolve(self.fs, &local.machine, &self.paths.home)?;

        let vars = Vars {
            home: self.paths.home.clone(),
            user: self.user.clone(),
            machine: local.machine.clone(),
            extra: machine.vars.clone(),
        };
        let provider = provider_for(kind, vault, &local.repo, &self.paths.home);
        let resolver = Resolver {
            provider: provider.as_ref(),
            kind,
            vault: vault.map(str::to_string),
            mapping: &machine.secrets,
            exec: self.exec,
        };

        for file in &desired.files {
            if file.mode == FileMode::Template {
                let source = self.fs.read(&file.source)?;
                render(&file.source, &source, &vars, &resolver)?;
            }
        }
        zshrc::generate(&desired.fragments, self.fs, &vars, &resolver)?;
        Ok(())
    }

    pub fn inspect(&self, local: &LocalConfig) -> Result<Inspection> {
        let repo = Repo::load(self.fs, &local.repo)?;
        let machine = repo
            .machines
            .get(&local.machine)
            .ok_or_else(|| Error::UnknownMachine(local.machine.clone()))?
            .clone();

        let desired = repo.resolve(self.fs, &local.machine, &self.paths.home)?;

        let vars = Vars {
            home: self.paths.home.clone(),
            user: self.user.clone(),
            machine: local.machine.clone(),
            extra: machine.vars.clone(),
        };

        let provider = provider_for(
            machine.secret_provider,
            machine.vault.as_deref(),
            &local.repo,
            &self.paths.home,
        );
        let resolver = Resolver {
            provider: provider.as_ref(),
            kind: machine.secret_provider,
            vault: machine.vault.clone(),
            mapping: &machine.secrets,
            exec: self.exec,
        };

        let mut rendered = Vec::new();
        let mut symlink_drift = Vec::new();

        for file in &desired.files {
            match file.mode {
                FileMode::Template => {
                    let source = self.fs.read(&file.source)?;
                    let Rendered {
                        content,
                        contains_secrets,
                        secret_values,
                    } = render(&file.source, &source, &vars, &resolver)?;
                    rendered.push(RenderedFile {
                        file: file.clone(),
                        content,
                        contains_secrets,
                        secret_values,
                    });
                }
                FileMode::Copy => rendered.push(RenderedFile {
                    file: file.clone(),
                    content: self.fs.read(&file.source)?,
                    contains_secrets: false,
                    secret_values: Vec::new(),
                }),
                FileMode::Symlink => {
                    if self.fs.read_link(&file.target).ok().as_ref() != Some(&file.source) {
                        symlink_drift.push(Drift::IncomingFile {
                            target: file.target.clone(),
                            set: file.set.clone(),
                        });
                    }
                }
            }
        }

        // The generated .zshrc is just another managed file.
        let Rendered {
            content: zshrc_content,
            contains_secrets: zshrc_contains_secrets,
            secret_values: zshrc_secret_values,
        } = zshrc::generate(&desired.fragments, self.fs, &vars, &resolver)?;
        rendered.push(RenderedFile {
            file: ResolvedFile {
                set: GENERATED_SET.into(),
                source: local.repo.clone(),
                target: self.paths.zshrc(),
                mode: FileMode::Template,
            },
            content: zshrc_content,
            contains_secrets: zshrc_contains_secrets,
            secret_values: zshrc_secret_values,
        });

        let applied = Applied::load(self.fs, &self.paths.applied())?;
        let actual_brew: BTreeSet<String> = self.brew.leaves()?.into_iter().collect();
        let actual_cask: BTreeSet<String> = self.brew.casks()?.into_iter().collect();

        let mut items = packages::diff(
            &desired,
            &actual_brew,
            &actual_cask,
            &applied,
            self.brew,
            &machine.ignore,
        )?;
        items = confirm_uninstalls(items, self.brew)?;

        let missing = missing_requirements(&repo, &machine.sets, self.fs, &self.paths.home);
        items.extend(files::diff(&rendered, &applied, self.fs)?);
        items.extend(symlink_drift);

        Ok(Inspection {
            desired,
            rendered,
            report: Report { items, missing },
        })
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::drift::{Drift, PackageRef};
    use crate::ports::fake::{FakeBrew, FakeExec, FakeFsys, FakeGit};

    fn fs() -> FakeFsys {
        FakeFsys::from([
            ("/repo/dotfix.toml", "schema_version = 1\n"),
            (
                "/repo/sets/core/set.toml",
                "[packages]\nbrew = [\"alpha\"]\n\n[[files]]\nsource = \"files/rc.tmpl\"\ntarget = \"~/.rc\"\n",
            ),
            ("/repo/sets/core/files/rc.tmpl", "home={{ home }}\n"),
            (
                "/repo/sets/core/shell/10-path.zsh",
                "export P={{ home }}/bin\n",
            ),
            ("/repo/machines/box-one.toml", "sets = [\"core\"]\n"),
        ])
    }

    fn local() -> LocalConfig {
        LocalConfig {
            repo: PathBuf::from("/repo"),
            machine: "box-one".into(),
        }
    }

    fn engine<'a>(
        fs: &'a FakeFsys,
        brew: &'a FakeBrew,
        git: &'a FakeGit,
        exec: &'a FakeExec,
    ) -> Engine<'a> {
        Engine {
            fs,
            brew,
            git,
            exec,
            paths: Paths::new(PathBuf::from("/Users/test")),
            user: "test".into(),
        }
    }

    #[test]
    fn renders_managed_files_and_reports_them_as_incoming() {
        let (fs, brew, git, exec) = (
            fs(),
            FakeBrew::new([], []),
            FakeGit::new(),
            FakeExec::new([]),
        );
        let out = engine(&fs, &brew, &git, &exec).inspect(&local()).unwrap();

        let rc = out
            .rendered
            .iter()
            .find(|r| r.file.target == Path::new("/Users/test/.rc"))
            .expect("managed file must be rendered");
        assert_eq!(rc.content, "home=/Users/test\n");
        assert!(out.report.items.contains(&Drift::IncomingFile {
            target: PathBuf::from("/Users/test/.rc"),
            set: "core".into(),
        }));
    }

    #[test]
    fn treats_the_generated_zshrc_as_a_managed_file() {
        let (fs, brew, git, exec) = (
            fs(),
            FakeBrew::new([], []),
            FakeGit::new(),
            FakeExec::new([]),
        );
        let out = engine(&fs, &brew, &git, &exec).inspect(&local()).unwrap();

        let zshrc_file = out
            .rendered
            .iter()
            .find(|r| r.file.target == Path::new("/Users/test/.zshrc"))
            .expect(".zshrc must be part of the inspection");
        assert!(zshrc_file.content.contains("export P=/Users/test/bin"));
        assert!(zshrc_file.content.starts_with(crate::render::zshrc::MARKER));
    }

    #[test]
    fn a_shell_fragment_resolving_a_secret_flags_the_generated_zshrc() {
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
        let out = engine(&fs, &brew, &git, &exec).inspect(&local()).unwrap();

        let zshrc_file = out
            .rendered
            .iter()
            .find(|r| r.file.target == Path::new("/Users/test/.zshrc"))
            .expect(".zshrc must be part of the inspection");
        assert!(
            zshrc_file.contains_secrets,
            "a fragment resolving a secret must flag the generated .zshrc so it is written 0600"
        );
        assert!(zshrc_file.secret_values.contains(&"s3cr3t".to_string()));
    }

    #[test]
    fn combines_package_and_file_drift_in_one_report() {
        let (fs, brew, git, exec) = (
            fs(),
            FakeBrew::new(["stray"], []),
            FakeGit::new(),
            FakeExec::new([]),
        );
        let out = engine(&fs, &brew, &git, &exec).inspect(&local()).unwrap();

        assert!(
            out.report
                .items
                .contains(&Drift::IncomingPackage(PackageRef::formula("alpha")))
        );
        assert!(out.report.items.contains(&Drift::Unmanaged {
            package: PackageRef::formula("stray"),
            declared_in: Vec::new(),
        }));
        assert!(
            out.report
                .items
                .iter()
                .any(|d| matches!(d, Drift::IncomingFile { .. }))
        );
    }

    #[test]
    fn a_repository_without_a_remote_syncs_as_a_no_op() {
        let (fs, brew, exec) = (fs(), FakeBrew::new([], []), FakeExec::new([]));
        let git = FakeGit::without_remote();
        engine(&fs, &brew, &git, &exec).sync(&local()).unwrap();
        assert!(
            git.calls().is_empty(),
            "a local-only repository must not be fetched: {:?}",
            git.calls()
        );
    }

    #[test]
    fn a_diverged_repository_is_reported_not_merged() {
        let (fs, brew, git, exec) = (
            fs(),
            FakeBrew::new([], []),
            FakeGit::diverged(),
            FakeExec::new([]),
        );
        let err = engine(&fs, &brew, &git, &exec).sync(&local()).unwrap_err();
        assert!(matches!(err, Error::Diverged));
    }

    #[test]
    fn an_uninstall_is_dropped_when_a_second_reading_disagrees() {
        // The real case, reduced: a cold Homebrew reported `mkcert` as a leaf
        // because it had not resolved the tap of `ddev`, which requires it.
        // Acting on that reading would have uninstalled a dependency.
        let brew = FakeBrew::new(["mkcert"], []).with_reread(["ddev/ddev/ddev"]);
        // The reading `inspect` has already taken by this point — the cold,
        // untrustworthy one that produced the item below.
        let _ = brew.leaves().unwrap();
        let items = vec![Drift::RemovedPackage {
            package: PackageRef::formula("mkcert"),
            blocked_by: Vec::new(),
            declared_in: Vec::new(),
        }];

        assert!(
            confirm_uninstalls(items, &brew).unwrap().is_empty(),
            "a package the second reading no longer sees must not be removed"
        );
    }

    #[test]
    fn an_uninstall_both_readings_agree_on_survives() {
        let brew = FakeBrew::new(["htop"], []).with_reread(["htop"]);
        let _ = brew.leaves().unwrap();
        let items = vec![Drift::RemovedPackage {
            package: PackageRef::formula("htop"),
            blocked_by: Vec::new(),
            declared_in: Vec::new(),
        }];

        assert_eq!(confirm_uninstalls(items, &brew).unwrap().len(), 1);
    }

    #[test]
    fn without_an_uninstall_no_second_reading_is_taken() {
        // The guard must not cost a brew call on the ordinary path, which has
        // nothing to remove.
        let brew = FakeBrew::new(["htop"], []).with_reread([]);
        let items = vec![Drift::Unmanaged {
            package: PackageRef::formula("htop"),
            declared_in: Vec::new(),
        }];

        assert_eq!(confirm_uninstalls(items, &brew).unwrap().len(), 1);
    }

    /// A repository with one set, built in memory — `Repo` has no Default
    /// and loading one from a fake filesystem would test the parser, not
    /// this.
    fn repo_with(name: &str, set: crate::config::SetConfig) -> Repo {
        let mut sets = std::collections::BTreeMap::new();
        sets.insert(name.to_string(), set);
        Repo {
            root: PathBuf::from("/repo"),
            config: crate::config::RepoConfig { schema_version: 1 },
            sets,
            machines: std::collections::BTreeMap::new(),
        }
    }

    fn req(name: &str, path: &str) -> crate::config::Requirement {
        crate::config::Requirement {
            name: name.into(),
            path: Some(path.into()),
            command: None,
            hint: Some(format!("install {name} somehow")),
        }
    }

    #[test]
    fn a_requirement_that_is_absent_is_reported_with_its_hint() {
        // The gap this closes: a shell fragment sourcing oh-my-zsh, a guard
        // that skips it when missing, and nothing anywhere saying it should
        // have been there. The machine looked set up and was not.
        let repo = repo_with(
            "core",
            crate::config::SetConfig {
                requires: vec![req("oh-my-zsh", "~/.oh-my-zsh")],
                ..Default::default()
            },
        );

        let missing = missing_requirements(
            &repo,
            &["core".to_string()],
            &FakeFsys::new(),
            Path::new("/Users/test"),
        );

        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0].name, "oh-my-zsh");
        assert_eq!(missing[0].set, "core");
        assert!(missing[0].hint.is_some(), "the user needs to be told how");
    }

    #[test]
    fn a_requirement_that_is_present_is_not_reported() {
        let repo = repo_with(
            "core",
            crate::config::SetConfig {
                requires: vec![req("oh-my-zsh", "~/.oh-my-zsh")],
                ..Default::default()
            },
        );
        // The path itself, not a file inside it: `FakeFsys` has no concept of
        // a directory. `RealFsys::exists` is `Path::exists`, which is true
        // for one — proven against a real filesystem in `ports::fsys`.
        let fs = FakeFsys::from([("/Users/test/.oh-my-zsh", "")]);

        assert!(
            missing_requirements(&repo, &["core".to_string()], &fs, Path::new("/Users/test"))
                .is_empty()
        );
    }

    #[test]
    fn a_switched_off_set_asks_for_nothing() {
        // A set that is not active is not asking, so its requirements are
        // not this machine's problem.
        let repo = repo_with(
            "mobile",
            crate::config::SetConfig {
                requires: vec![req("sdkman", "~/.sdkman")],
                ..Default::default()
            },
        );

        assert!(
            missing_requirements(
                &repo,
                &["core".to_string()],
                &FakeFsys::new(),
                Path::new("/x")
            )
            .is_empty()
        );
    }

    #[test]
    fn a_missing_requirement_means_the_machine_is_not_in_sync() {
        // Otherwise `dotfix status` would print "everything in sync" while
        // something the repository asks for is absent.
        let report = Report {
            items: Vec::new(),
            missing: vec![crate::drift::Missing {
                name: "nvm".into(),
                set: "core".into(),
                hint: None,
            }],
        };
        assert!(!report.is_empty());
    }

    #[test]
    fn publishing_commits_what_changed_and_pushes_it() {
        let git = FakeGit {
            dirty: vec!["sets/core/set.toml".into()],
            ..FakeGit::new()
        };
        let fs = FakeFsys::new();
        let brew = FakeBrew::new([], []);
        let exec = crate::ports::fake::FakeExec::new([]);
        let paths = Paths::new(PathBuf::from("/Users/test"));
        let engine = Engine {
            fs: &fs,
            git: &git,
            brew: &brew,
            exec: &exec,
            paths,
            user: "test".into(),
        };
        let local = LocalConfig {
            repo: PathBuf::from("/repo"),
            machine: "box-one".into(),
        };

        let done = engine.publish(&local, "chore: x").unwrap();
        assert_eq!(done.committed, vec!["sets/core/set.toml"]);
        assert!(done.pushed);
        assert!(
            git.calls().iter().any(|c| c.starts_with("commit")),
            "{:?}",
            git.calls()
        );
        assert!(git.calls().iter().any(|c| c == "push"), "{:?}", git.calls());
    }

    #[test]
    fn a_repository_without_a_remote_publishes_nothing_rather_than_failing() {
        // `init --set-up-new` leaves exactly that until the user points it
        // somewhere. Nothing to send is not an error.
        let git = FakeGit {
            no_remote: true,
            ..FakeGit::new()
        };
        let fs = FakeFsys::new();
        let brew = FakeBrew::new([], []);
        let exec = crate::ports::fake::FakeExec::new([]);
        let paths = Paths::new(PathBuf::from("/Users/test"));
        let engine = Engine {
            fs: &fs,
            git: &git,
            brew: &brew,
            exec: &exec,
            paths,
            user: "test".into(),
        };
        let local = LocalConfig {
            repo: PathBuf::from("/repo"),
            machine: "box-one".into(),
        };

        let done = engine.publish(&local, "chore: x").unwrap();
        assert!(!done.pushed);
        assert!(!git.calls().iter().any(|c| c == "push"));
    }
}

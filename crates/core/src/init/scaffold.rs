use std::path::{Path, PathBuf};

use crate::config::{MachineConfig, Packages, RepoConfig, Requirement, SCHEMA_VERSION, SetConfig};
use crate::error::{Error, Result};
use crate::init::{Plan, Source};
use crate::paths::Paths;
use crate::ports::{Brew, Fsys, Git};

/// The status-line hook, shipped with every new repository so a fresh machine
/// gets the background notification without extra steps.
///
/// `crates/cli/tests/shell_hook.rs` compares this byte-for-byte against the
/// hook it runs under real zsh. Change one and you must change the other.
pub const STATUS_FRAGMENT: &str = "\
# printed by dotfix when the background check found something
() {
  local f=${XDG_STATE_HOME:-$HOME/.local/state}/dotfix/status.line
  [[ -s $f ]] && print -r -- \"$(<$f)\"
}
";

/// Create the repository, or clone it. The only step that creates the
/// repository directory — and therefore the only one that has to clean up
/// after itself, so a retry never trips over a half-built `~/dotfiles`.
///
/// Two guards make this safe to call again after a *later* step failed and
/// the caller retries the whole run (`app/src-tauri/src/commands.rs`'s
/// `init_run` resumes past whichever steps already succeeded, but nothing
/// stops a caller — the CLI's `dotfix init` has no resume concept at all —
/// from invoking this function a second time against a repository that
/// already exists):
///
/// - If `root/dotfix.toml` is already there, this is a no-op that returns
///   the existing root without touching git or the filesystem at all.
///   Re-scaffolding would rewrite files to their already-committed
///   contents and then ask git to commit nothing, which fails — turning a
///   resumed run into a fresh failure for no reason.
/// - Whether `root` existed *before this call* is recorded up front, and
///   the failure path only removes it when this call is the one that
///   created it. A repository that predates this call — most concretely,
///   one a previous, successful call already built — must never be
///   removed because something later in *this* call went wrong.
pub fn create_or_clone(
    fs: &dyn Fsys,
    git: &dyn Git,
    brew: &dyn Brew,
    paths: &Paths,
    plan: &Plan,
) -> Result<PathBuf> {
    let root = paths.home.join("dotfiles");

    match &plan.source {
        Source::Clone { url } => {
            // Resuming a setup that was interrupted after the clone. Cloning
            // again would fail on a non-empty directory, so a run that got
            // this far once could never be finished without deleting the
            // repository by hand.
            //
            // Reused only when it is demonstrably the repository asked for:
            // "clone this URL" must not quietly become "use whatever happens
            // to be at this path".
            if fs.exists(&root.join("dotfix.toml")) {
                return match git.remote_url(&root)? {
                    Some(found) if crate::init::same_repository(&found, url) => Ok(root),
                    Some(found) => Err(Error::Config(format!(
                        "{} already holds a dotfix repository cloned from {found}, \
                         not {url}. Move it aside, or set this Mac up from {found} instead.",
                        root.display()
                    ))),
                    None => Err(Error::Config(format!(
                        "{} already holds a dotfix repository with no remote, so it \
                         cannot be the one you are cloning. Move it aside to continue.",
                        root.display()
                    ))),
                };
            }
            git.clone_to(url, &root)?;
            Ok(root)
        }
        Source::New => {
            if fs.exists(&root.join("dotfix.toml")) {
                return Ok(root);
            }

            // `root` is a directory, and `Fsys::exists` only ever answers
            // for an exact path — real filesystems track directories as
            // entries of their own, the in-memory test double does not — so
            // "did anything already live under `root`" has to be asked as
            // "is `list_dir` non-empty", not `exists`.
            let pre_existing = !fs.list_dir(&root)?.is_empty();
            git.init(&root)?;
            match scaffold(fs, &root, &plan.machine, brew, &paths.home)
                .and_then(|()| git.commit_all(&root, "chore: scaffold dotfix repository"))
            {
                Ok(()) => Ok(root),
                Err(e) => {
                    if !pre_existing {
                        remove_tree(fs, &root);
                    }
                    Err(e)
                }
            }
        }
    }
}

/// Best-effort removal of everything under `root`. Called only on the failure
/// path, where reporting the original error matters more than a tidy-up
/// problem — so removal errors are deliberately swallowed.
fn remove_tree(fs: &dyn Fsys, root: &Path) {
    let _ = fs.remove_dir_all(root);
}

/// Software this Mac has that Homebrew did not put here.
///
/// The counterpart to capturing the package list: these install themselves
/// into the home directory from a shell script, so nothing in `brew leaves`
/// mentions them — and a set that referenced one from a shell fragment had
/// no way to say it was needed. Recorded here so a second Mac is told.
///
/// A fixed list rather than a scan. Guessing what an unknown directory in
/// `$HOME` means would produce noise, and the hint has to be right: a wrong
/// install command is worse than none.
pub fn detect_requirements(fs: &dyn Fsys, home: &Path) -> Vec<Requirement> {
    const KNOWN: &[(&str, &str, &str)] = &[
        (
            "oh-my-zsh",
            ".oh-my-zsh",
            "sh -c \"$(curl -fsSL https://raw.githubusercontent.com/ohmyzsh/ohmyzsh/master/tools/install.sh)\"",
        ),
        (
            "nvm",
            ".nvm",
            "curl -o- https://raw.githubusercontent.com/nvm-sh/nvm/master/install.sh | bash",
        ),
        (
            "rustup",
            ".cargo/bin/rustup",
            "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh",
        ),
        ("pyenv", ".pyenv", "curl -fsSL https://pyenv.run | bash"),
        (
            "rbenv",
            ".rbenv",
            "curl -fsSL https://github.com/rbenv/rbenv-installer/raw/main/bin/rbenv-installer | bash",
        ),
        (
            "sdkman",
            ".sdkman",
            "curl -s \"https://get.sdkman.io\" | bash",
        ),
    ];

    KNOWN
        .iter()
        .filter(|(_, path, _)| fs.exists(&home.join(path)))
        .map(|(name, path, hint)| Requirement {
            name: (*name).to_string(),
            path: Some(format!("~/{path}")),
            command: None,
            hint: Some((*hint).to_string()),
        })
        .collect()
}

/// Where an imported `.zshrc` lands, relative to the repository root. Shared
/// so the front ends can report the import without knowing the layout.
pub const IMPORTED_FRAGMENT: &str = "sets/core/shell/00-imported.zsh";

/// Neutralise template syntax in text that was never meant to be a template.
///
/// Every fragment is rendered through minijinja before it reaches `.zshrc`,
/// so a `{{` or `{%` that happens to occur in an imported shell file would
/// abort rendering — and an import that can break the thing it was meant to
/// preserve is worse than no import. Both are emitted as literals.
fn escape_template_syntax(body: &str) -> String {
    body.replace("{{", "{{ '{{' }}").replace("{%", "{{ '{%' }}")
}

/// Path A: create a repository skeleton and propose a first set split from the
/// packages that are already installed. The user corrects it afterwards.
pub fn scaffold(
    fs: &dyn Fsys,
    root: &Path,
    machine: &str,
    brew: &dyn Brew,
    home: &Path,
) -> Result<()> {
    fs.write(
        &root.join("dotfix.toml"),
        &toml::to_string_pretty(&RepoConfig {
            schema_version: SCHEMA_VERSION,
        })
        .map_err(|e| Error::Config(format!("serialising dotfix.toml: {e}")))?,
        0o644,
    )?;

    let core = SetConfig {
        description: "Base set, active on every machine".into(),
        packages: Packages {
            brew: brew.leaves()?,
            cask: brew.casks()?,
        },
        files: Vec::new(),
        requires: detect_requirements(fs, home),
    };
    fs.write(
        &root.join("sets/core/set.toml"),
        &toml::to_string_pretty(&core)
            .map_err(|e| Error::Config(format!("serialising sets/core/set.toml: {e}")))?,
        0o644,
    )?;

    // Capture the shell configuration this machine already has, the same way
    // the package list is captured.
    //
    // Without this, `--set-up-new` promised a repository built "from what is
    // installed here" and delivered one for packages only: the shell side was
    // two placeholders, so the generated `.zshrc` was a stub and the first
    // overwrite replaced a real configuration with it. Importing makes the
    // first apply a near-no-op, which is what the promise implies.
    //
    // Named `00-` and written whole: the original file's internal order is
    // preserved exactly, and a Powerlevel10k instant-prompt block — which
    // must be the very first thing a shell runs — stays first.
    let existing = home.join(".zshrc");
    let imported = fs.exists(&existing) && !fs.read(&existing)?.trim().is_empty();
    if imported {
        let body = escape_template_syntax(&fs.read(&existing)?);
        fs.write(&root.join(IMPORTED_FRAGMENT), &body, 0o644)?;
    } else {
        // Nothing to import: a placeholder so the generated file is never empty.
        fs.write(
            &root.join("sets/core/shell/10-path.zsh"),
            "export PATH=\"{{ home }}/.local/bin:$PATH\"\n",
            0o644,
        )?;
    }
    fs.write(
        &root.join("sets/core/shell/99-dotfix-status.zsh"),
        STATUS_FRAGMENT,
        0o644,
    )?;

    let cfg = MachineConfig {
        sets: vec!["core".into()],
        ..Default::default()
    };
    fs.write(
        &machine_path(root, machine),
        &toml::to_string_pretty(&cfg)
            .map_err(|e| Error::Config(format!("serialising machine config: {e}")))?,
        0o644,
    )?;

    fs.write(
        &root.join(".gitignore"),
        "# never commit decrypted secrets\n*.decrypted\n",
        0o644,
    )?;

    Ok(())
}

fn machine_path(root: &Path, machine: &str) -> PathBuf {
    root.join("machines").join(format!("{machine}.toml"))
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::config::ProviderKind;
    use crate::init::{Plan, Source};
    use crate::paths::Paths;
    use crate::ports::Fsys;
    use crate::ports::fake::{FakeBrew, FakeFsys, FakeGit};

    fn plan() -> Plan {
        Plan {
            machine: "box-one".into(),
            source: Source::New,
            secret_provider: ProviderKind::Keychain,
            vault: None,
        }
    }

    fn paths() -> Paths {
        Paths::new(PathBuf::from("/Users/test"))
    }

    #[test]
    fn a_new_repository_gets_the_whole_skeleton() {
        let (fs, git, brew) = (
            FakeFsys::new(),
            FakeGit::new(),
            FakeBrew::new(["fake-pkg"], ["fake-cask"]),
        );
        let root = create_or_clone(&fs, &git, &brew, &paths(), &plan()).unwrap();

        assert_eq!(root, PathBuf::from("/Users/test/dotfiles"));
        for f in [
            "dotfix.toml",
            "sets/core/set.toml",
            "sets/core/shell/10-path.zsh",
            "sets/core/shell/99-dotfix-status.zsh",
            "machines/box-one.toml",
            ".gitignore",
        ] {
            assert!(fs.exists(&root.join(f)), "missing {f}");
        }
        assert!(
            fs.read(&root.join("sets/core/set.toml"))
                .unwrap()
                .contains("fake-pkg")
        );
    }

    #[test]
    fn a_new_repository_is_a_git_repository_with_one_commit() {
        let (fs, git, brew) = (FakeFsys::new(), FakeGit::new(), FakeBrew::new([], []));
        create_or_clone(&fs, &git, &brew, &paths(), &plan()).unwrap();

        let calls = git.calls();
        assert!(calls.iter().any(|c| c == "init"), "got {calls:?}");
        assert!(
            calls.iter().any(|c| c.starts_with("commit:")),
            "the scaffold must be committed, got {calls:?}"
        );
    }

    #[test]
    fn git_init_runs_before_anything_is_written() {
        // A failure while writing must leave no repository behind, so the git
        // repo has to exist first — otherwise cleanup cannot know what it owns.
        let (fs, git, brew) = (FakeFsys::new(), FakeGit::new(), FakeBrew::new([], []));
        create_or_clone(&fs, &git, &brew, &paths(), &plan()).unwrap();
        assert_eq!(git.calls().first().map(String::as_str), Some("init"));
    }

    #[test]
    fn a_failure_on_a_first_attempt_leaves_nothing_behind() {
        // Nothing exists under `root` before this call — a genuinely fresh
        // attempt, not a retry — so `pre_existing` is false and the failure
        // path is free to remove everything this call wrote, including
        // `dotfix.toml`, which `scaffold` writes before the brew calls that
        // `FakeBrew::failing` makes fail.
        let (fs, git) = (FakeFsys::new(), FakeGit::new());
        let brew = FakeBrew::failing();
        // A file that merely shares a prefix with the repo root must survive
        // — cleanup has to remove exactly `dotfiles/`, not everything that
        // starts with the same characters.
        fs.write(
            Path::new("/Users/test/dotfiles-backup/keep.txt"),
            "kept",
            0o644,
        )
        .unwrap();

        let err = create_or_clone(&fs, &git, &brew, &paths(), &plan()).unwrap_err();
        assert!(!err.to_string().is_empty());

        let leftovers: Vec<_> = fs
            .snapshot()
            .keys()
            .filter(|p| p.starts_with(Path::new("/Users/test/dotfiles")))
            .cloned()
            .collect();
        assert!(
            leftovers.is_empty(),
            "a first attempt that fails must not leave a half-built repo behind, found {leftovers:?}"
        );
        assert!(
            fs.exists(Path::new("/Users/test/dotfiles-backup/keep.txt")),
            "cleanup must not touch files outside the repository root"
        );
    }

    #[test]
    fn a_repository_that_already_has_dotfix_toml_is_a_no_op() {
        // Models the retry that follows a *later* step (e.g.
        // `install_agent`) failing after `create_or_clone` already
        // succeeded once: `dotfix.toml` is already there, so this call must
        // return the existing root without writing anything, running `git
        // init`/`commit_all` again, or asking `brew` for anything —
        // `FakeBrew::failing` would error immediately if any of that ran,
        // which is exactly what this test would catch.
        let (fs, git) = (FakeFsys::new(), FakeGit::new());
        let brew = FakeBrew::failing();
        fs.write(
            Path::new("/Users/test/dotfiles/dotfix.toml"),
            "schema_version = 1\n",
            0o644,
        )
        .unwrap();
        fs.write(
            Path::new("/Users/test/dotfiles/machines/box-one.toml"),
            "sets = [\"core\"]\n",
            0o644,
        )
        .unwrap();

        let root = create_or_clone(&fs, &git, &brew, &paths(), &plan()).unwrap();

        assert_eq!(root, PathBuf::from("/Users/test/dotfiles"));
        assert!(
            git.calls().is_empty(),
            "an already-scaffolded repository must not touch git again, got {:?}",
            git.calls()
        );
        assert!(
            fs.exists(Path::new("/Users/test/dotfiles/machines/box-one.toml")),
            "a resumed run must not disturb an existing repository"
        );
    }

    #[test]
    fn an_existing_repository_survives_a_failed_retry_attempt() {
        // `root` exists before this call (e.g. a previous attempt got this
        // far before crashing) but has no `dotfix.toml` yet, so the no-op
        // shortcut above does not apply and `scaffold` genuinely runs — and
        // fails, via `FakeBrew::failing`. This is the guard that matters on
        // its own: whatever this call writes on the way to that failure
        // must be cleaned up, but the pre-existing content must not be,
        // because this call did not create it.
        let (fs, git) = (FakeFsys::new(), FakeGit::new());
        let brew = FakeBrew::failing();
        fs.write(
            Path::new("/Users/test/dotfiles/README.md"),
            "pre-existing notes",
            0o644,
        )
        .unwrap();

        let err = create_or_clone(&fs, &git, &brew, &paths(), &plan()).unwrap_err();
        assert!(!err.to_string().is_empty());

        assert!(
            fs.exists(Path::new("/Users/test/dotfiles/README.md")),
            "a directory that existed before this call must survive even though this call's own work failed"
        );
    }

    #[test]
    fn cloning_uses_the_given_url() {
        let (fs, git, brew) = (FakeFsys::new(), FakeGit::new(), FakeBrew::new([], []));
        let plan = Plan {
            source: Source::Clone {
                url: "git@github.com:example/dotfiles.git".into(),
            },
            ..plan()
        };
        create_or_clone(&fs, &git, &brew, &paths(), &plan).unwrap();

        assert!(
            git.calls()
                .iter()
                .any(|c| c == "clone:git@github.com:example/dotfiles.git"),
            "got {:?}",
            git.calls()
        );
    }

    fn clone_plan(url: &str) -> Plan {
        Plan {
            source: Source::Clone { url: url.into() },
            ..plan()
        }
    }

    fn existing_repo() -> FakeFsys {
        FakeFsys::from([("/Users/test/dotfiles/dotfix.toml", "schema_version = 1\n")])
    }

    #[test]
    fn an_interrupted_clone_is_resumed_rather_than_cloned_again() {
        // The state an interrupted first run leaves behind. Cloning again
        // fails on a non-empty directory, so without this the only way
        // forward was deleting ~/dotfiles by hand.
        let fs = existing_repo();
        let git = FakeGit {
            remote_url: "git@github.com:example-user/dotfiles.git".into(),
            ..FakeGit::new()
        };
        let root = create_or_clone(
            &fs,
            &git,
            &FakeBrew::new([], []),
            &paths(),
            &clone_plan("https://github.com/example-user/dotfiles"),
        )
        .unwrap();

        assert_eq!(root, PathBuf::from("/Users/test/dotfiles"));
        assert!(
            !git.calls().iter().any(|c| c.starts_with("clone")),
            "must not clone over an existing repository: {:?}",
            git.calls()
        );
    }

    #[test]
    fn a_different_repository_at_the_target_is_refused_not_silently_used() {
        // "Clone this URL" must never quietly become "use whatever is here".
        let fs = existing_repo();
        let git = FakeGit {
            remote_url: "git@github.com:someone-else/dotfiles.git".into(),
            ..FakeGit::new()
        };
        let err = create_or_clone(
            &fs,
            &git,
            &FakeBrew::new([], []),
            &paths(),
            &clone_plan("git@github.com:example-user/dotfiles.git"),
        )
        .unwrap_err()
        .to_string();

        assert!(err.contains("someone-else"), "{err}");
        assert!(err.contains("example-user"), "{err}");
    }

    #[test]
    fn an_existing_repository_without_a_remote_is_refused() {
        let fs = existing_repo();
        let git = FakeGit {
            no_remote: true,
            ..FakeGit::new()
        };
        let err = create_or_clone(
            &fs,
            &git,
            &FakeBrew::new([], []),
            &paths(),
            &clone_plan("git@github.com:example-user/dotfiles.git"),
        )
        .unwrap_err()
        .to_string();

        assert!(err.contains("no remote"), "{err}");
    }

    #[test]
    fn an_existing_zshrc_is_imported_whole_and_first() {
        // The promise of `--set-up-new` is a repository built from what is
        // installed here. It kept that for packages and broke it for the
        // shell, which is how a real 140-line configuration came to be
        // replaced by a two-line stub.
        let fs = FakeFsys::from([(
            "/Users/test/.zshrc",
            "export ZSH=\"$HOME/.oh-my-zsh\"\nsource $ZSH/oh-my-zsh.sh\n",
        )]);
        scaffold(
            &fs,
            Path::new("/Users/test/dotfiles"),
            "box-one",
            &FakeBrew::new([], []),
            Path::new("/Users/test"),
        )
        .unwrap();

        let imported = fs
            .read(Path::new(
                "/Users/test/dotfiles/sets/core/shell/00-imported.zsh",
            ))
            .unwrap();
        assert!(imported.contains("oh-my-zsh.sh"), "{imported}");
        assert!(
            fs.read(Path::new(
                "/Users/test/dotfiles/sets/core/shell/10-path.zsh"
            ))
            .is_err(),
            "the placeholder exists only when there was nothing to import"
        );
    }

    #[test]
    fn without_a_zshrc_the_placeholder_still_appears() {
        let fs = FakeFsys::new();
        scaffold(
            &fs,
            Path::new("/Users/test/dotfiles"),
            "box-one",
            &FakeBrew::new([], []),
            Path::new("/Users/test"),
        )
        .unwrap();

        assert!(
            fs.read(Path::new(
                "/Users/test/dotfiles/sets/core/shell/10-path.zsh"
            ))
            .is_ok(),
            "the generated .zshrc must never be empty"
        );
    }

    #[test]
    fn an_empty_zshrc_counts_as_nothing_to_import() {
        let fs = FakeFsys::from([("/Users/test/.zshrc", "\n  \n")]);
        scaffold(
            &fs,
            Path::new("/Users/test/dotfiles"),
            "box-one",
            &FakeBrew::new([], []),
            Path::new("/Users/test"),
        )
        .unwrap();

        assert!(
            fs.read(Path::new(
                "/Users/test/dotfiles/sets/core/shell/00-imported.zsh"
            ))
            .is_err()
        );
    }

    #[test]
    fn template_syntax_in_an_imported_file_cannot_break_rendering() {
        // A shell file is not a template. A stray `{{` in one would otherwise
        // abort the render of the very file the import exists to preserve.
        assert_eq!(
            escape_template_syntax("echo {{ oops }} and {% raw %}"),
            "echo {{ '{{' }} oops }} and {{ '{%' }} raw %}"
        );
        assert_eq!(escape_template_syntax("export A=$B"), "export A=$B");
    }
}

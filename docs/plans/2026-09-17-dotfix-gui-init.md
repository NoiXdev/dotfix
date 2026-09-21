# dotfix GUI Onboarding Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let a fresh Mac be set up entirely from the menubar app, by moving `init` out of the CLI into `dotfix-core` as four discrete steps and driving them from a short wizard.

**Architecture:** `dotfix_core::init` gains four steps — `preflight`, `create_or_clone`, `configure_machine`, `install_agent` — each with a clear input and output, called by both the CLI (behind its prompts) and the app (behind its wizard). `preflight` never writes; only once it passes does anything touch the filesystem. Everything that talks to the outside world goes through the existing `Fsys`/`Git`/`Brew`/`Exec` ports, so the whole flow including the authentication ladder is testable against the existing fakes.

**Tech Stack:** Rust 1.96 (edition 2024), the existing `dotfix-core` ports, Tauri v2 commands, React + TypeScript + Tailwind + vitest in `app/`.

**Spec:** `docs/specs/2026-09-17-dotfix-gui-init-design.md`

**Predecessors:** phase 1 (`docs/plans/2026-09-16-dotfix-phase-1.md`) and phase 2 (`docs/plans/2026-09-16-dotfix-phase-2.md`), both complete and merged.

## Global Constraints

- Rust 1.96, edition 2024. macOS only.
- All code, comments, commit messages and UI strings in English.
- TDD: write the failing test, RUN it and confirm it fails for the expected reason, then implement.
- `cargo test --workspace`, `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and from `app/`: `npm run typecheck`, `npm test` — all must pass before every commit.
- Conventional Commits; every commit message ends with `Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>`.
- Public repository in intent: no personal data, no real machine names, **no secret values** anywhere — not in code, tests or fixtures. Secret *names* are fine.
- **dotfix never persists a credential of its own.** Tokens go to the macOS Keychain through git's credential helper; SSH private keys stay in `~/.ssh` at mode `0600`.
- **No command may be able to prompt.** Anything that could ask a question gets `BatchMode=yes` or an equivalent non-interactive flag. A GUI app has no controlling terminal: a prompt is a hang, not a question.
- Verification is LOCAL ONLY. Do not rely on GitHub Actions.
- `crates/cli/tests/*.rs` are the regression guard for the CLI rewrite. They must keep passing **unmodified**.

---

## Things the spec assumes that are not yet true

Two small facts an implementer will hit immediately:

1. **`doctor::Check`'s constructors are private.** `Check::ok` and `Check::fail` are plain `fn` inside `crates/core/src/doctor.rs`. `init` is a sibling module and cannot call them. Task 1 makes them `pub(crate)` — not `pub`; nothing outside the crate should be minting checks.
2. **`scaffold` and `STATUS_FRAGMENT` live in the CLI.** They are in `crates/cli/src/cmd/init.rs` and move to core in Task 2. The move must be verbatim — the scaffold's exact file set is what `crates/cli/tests/init.rs` and `crates/cli/tests/shell_hook.rs` assert on.

---

## File Structure

### `crates/core/src/init/` — the four steps

| File | Responsibility |
|---|---|
| `mod.rs` | `Plan`, `Source`, `Preflight`, and the four step functions |
| `scaffold.rs` | the repository skeleton, moved verbatim from the CLI |
| `preflight.rs` | the checks, returning `Vec<doctor::Check>` |
| `ssh.rs` | detection ladder, host-key pinning, deploy key, `~/.ssh/config` stanza |
| `remote.rs` | `gh repo create`, and handing a token to git's credential helper |

### Modified

| File | Change |
|---|---|
| `crates/core/src/doctor.rs` | `Check::ok`/`Check::fail` become `pub(crate)` |
| `crates/core/src/lib.rs` | `pub mod init;` |
| `crates/cli/src/cmd/init.rs` | shrinks to prompts plus four calls |
| `app/src-tauri/src/commands.rs` | wizard commands |
| `app/src-tauri/src/lib.rs` | register them |

### `app/src/` — the wizard

| File | Responsibility |
|---|---|
| `wizard/Wizard.tsx` | the four-step shell with per-step state and retry |
| `wizard/Answers.tsx` | machine name, path, URL, secret provider |
| `wizard/PreflightList.tsx` | renders `Check[]` — shared shape with doctor |
| `wizard/DeployKey.tsx` | public key, copy button, link, "test connection" |
| `App.tsx` | renders the wizard instead of the not-configured panel |

---

## Task 1: The init module, `Plan`, and pre-flight for a new repository

**Files:**
- Create: `crates/core/src/init/mod.rs`, `crates/core/src/init/preflight.rs`
- Modify: `crates/core/src/doctor.rs`, `crates/core/src/lib.rs`
- Test: inline `#[cfg(test)]` in `preflight.rs`

**Interfaces:**
- Consumes: `doctor::Check` (`{ name: &'static str, ok: bool, detail: String }`), `Fsys`, `Exec`, `Paths`, `ProviderKind`.
- Produces:
  - `init::Source::{New, Clone { url: String }}`
  - `init::Plan { machine: String, source: Source, secret_provider: ProviderKind, vault: Option<String> }`
  - `init::Preflight { checks: Vec<Check> }` with `Preflight::passes(&self) -> bool`
  - `init::preflight(fs: &dyn Fsys, exec: &dyn Exec, paths: &Paths, plan: &Plan) -> Preflight`
  - `doctor::Check::ok` / `::fail` become `pub(crate)`

- [ ] **Step 1: Write the failing tests**

`crates/core/src/init/preflight.rs`:

```rust
#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::config::ProviderKind;
    use crate::init::{Plan, Source};
    use crate::paths::Paths;
    use crate::ports::fake::{FakeExec, FakeFsys};

    fn paths() -> Paths {
        Paths::new(PathBuf::from("/Users/test"))
    }

    fn plan() -> Plan {
        Plan {
            machine: "box-one".into(),
            source: Source::New,
            secret_provider: ProviderKind::Keychain,
            vault: None,
        }
    }

    /// Everything present: git, brew, gh authenticated, target free.
    fn healthy_exec() -> FakeExec {
        FakeExec::new([
            ("git --version", "git version 2.51.0\n"),
            ("brew --version", "Homebrew 7.0.2\n"),
            ("gh auth status", "Logged in to github.com\n"),
        ])
    }

    fn named<'a>(p: &'a Preflight, name: &str) -> &'a crate::doctor::Check {
        p.checks
            .iter()
            .find(|c| c.name == name)
            .unwrap_or_else(|| panic!("no check named `{name}` in {:?}", p.checks))
    }

    #[test]
    fn a_ready_machine_passes() {
        let p = preflight(&FakeFsys::new(), &healthy_exec(), &paths(), &plan());
        assert!(p.passes(), "{:?}", p.checks);
    }

    #[test]
    fn a_missing_target_directory_is_what_we_want_not_a_failure() {
        let p = preflight(&FakeFsys::new(), &healthy_exec(), &paths(), &plan());
        assert!(named(&p, "target directory").ok);
    }

    #[test]
    fn an_existing_target_directory_blocks_and_says_where() {
        let fs = FakeFsys::from([("/Users/test/dotfiles/dotfix.toml", "schema_version = 1\n")]);
        let p = preflight(&fs, &healthy_exec(), &paths(), &plan());

        let check = named(&p, "target directory");
        assert!(!check.ok);
        assert!(check.detail.contains("/Users/test/dotfiles"));
        assert!(!p.passes());
    }

    #[test]
    fn missing_git_blocks() {
        let exec = FakeExec::new([("brew --version", "Homebrew 7.0.2\n")]);
        let p = preflight(&FakeFsys::new(), &exec, &paths(), &plan());
        assert!(!named(&p, "git").ok);
        assert!(!p.passes());
    }

    #[test]
    fn missing_gh_is_reported_but_does_not_block() {
        // `gh` only decides whether we can offer to create the remote for the
        // user. Setting up locally must not depend on it.
        let exec = FakeExec::new([
            ("git --version", "git version 2.51.0\n"),
            ("brew --version", "Homebrew 7.0.2\n"),
        ]);
        let p = preflight(&FakeFsys::new(), &exec, &paths(), &plan());

        assert!(!named(&p, "github cli").ok);
        assert!(p.passes(), "a missing gh must not block local setup");
    }

    #[test]
    fn an_empty_machine_name_blocks() {
        let mut plan = plan();
        plan.machine = String::new();
        let p = preflight(&FakeFsys::new(), &healthy_exec(), &paths(), &plan);
        assert!(!named(&p, "machine name").ok);
        assert!(!p.passes());
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p dotfix-core init::preflight`
Expected: FAIL — `cannot find function preflight`.

- [ ] **Step 3: Open up the Check constructors**

In `crates/core/src/doctor.rs`, change the two constructors from `fn` to
`pub(crate) fn`:

```rust
impl Check {
    pub(crate) fn ok(name: &'static str, detail: impl Into<String>) -> Self {
```

```rust
    pub(crate) fn fail(name: &'static str, detail: impl Into<String>) -> Self {
```

Not `pub`: nothing outside this crate should be minting checks.

- [ ] **Step 4: Implement the module and pre-flight**

`crates/core/src/init/mod.rs`:

```rust
//! First-time setup, as four steps either front end can drive.
//!
//! The split exists for failure reporting: a user in the app has no terminal
//! open to investigate, so the app must be able to say which step failed. The
//! ordering is binding — [`preflight`] never writes, and only once it passes
//! does [`create_or_clone`] touch the filesystem.

mod preflight;

pub use preflight::{Preflight, preflight};

use crate::config::ProviderKind;

/// Where this machine's configuration comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// Build a repository from what is installed on this machine.
    New,
    /// Clone an existing repository.
    Clone { url: String },
}

/// Everything the caller must decide before anything is written. Prompts are
/// the front end's business; the steps take a value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub machine: String,
    pub source: Source,
    pub secret_provider: ProviderKind,
    /// 1Password only.
    pub vault: Option<String>,
}
```

`crates/core/src/init/preflight.rs` (prepend to the tests):

```rust
use serde::Serialize;

use crate::doctor::Check;
use crate::init::{Plan, Source};
use crate::paths::Paths;
use crate::ports::{Exec, Fsys};

/// The result of looking before leaping. Writes nothing.
#[derive(Debug, Clone, Serialize)]
pub struct Preflight {
    pub checks: Vec<Check>,
}

impl Preflight {
    /// True when nothing blocks. A failed check that is merely informational
    /// (see `github cli`) is reported as not-ok but is not in `blocking`.
    pub fn passes(&self) -> bool {
        self.checks
            .iter()
            .filter(|c| is_blocking(c.name))
            .all(|c| c.ok)
    }
}

/// Which checks must pass before anything may be written.
fn is_blocking(name: &str) -> bool {
    !matches!(name, "github cli")
}

pub fn preflight(fs: &dyn Fsys, exec: &dyn Exec, paths: &Paths, plan: &Plan) -> Preflight {
    let mut checks = Vec::new();

    checks.push(if plan.machine.trim().is_empty() {
        Check::fail("machine name", "a name is required to identify this machine")
    } else {
        Check::ok("machine name", plan.machine.clone())
    });

    checks.push(match exec.run("git", &["--version"]) {
        Ok(v) => Check::ok("git", v.trim().to_string()),
        Err(e) => Check::fail("git", e.to_string()),
    });

    checks.push(match exec.run("brew", &["--version"]) {
        Ok(v) => Check::ok("homebrew", v.lines().next().unwrap_or("").to_string()),
        Err(e) => Check::fail("homebrew", e.to_string()),
    });

    // A free target directory is the normal state, so say so plainly rather
    // than only complaining when it is taken.
    let target = paths.home.join("dotfiles");
    checks.push(if fs.exists(&target.join("dotfix.toml")) {
        Check::fail(
            "target directory",
            format!("{} already holds a dotfix repository", target.display()),
        )
    } else {
        Check::ok("target directory", target.display().to_string())
    });

    // Informational: it only decides whether we can offer to create the remote.
    checks.push(match exec.run("gh", &["auth", "status"]) {
        Ok(_) => Check::ok("github cli", "authenticated — can create the remote for you"),
        Err(_) => Check::fail(
            "github cli",
            "not installed or not logged in — you will create the repository yourself",
        ),
    });

    if let Source::Clone { url } = &plan.source {
        checks.push(if url.trim().is_empty() {
            Check::fail("repository url", "a url is required to clone")
        } else {
            Check::ok("repository url", url.clone())
        });
    }

    Preflight { checks }
}
```

Add `pub mod init;` to `crates/core/src/lib.rs`, and derive `Serialize` on
`doctor::Check` if it does not already (the wizard renders it).

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p dotfix-core init && cargo clippy --all-targets -- -D warnings`
Expected: 6 PASS, no warnings.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "feat(core): add init plan and pre-flight checks"
```

---

## Task 2: Creating a new repository, with no half-built state

**Files:**
- Create: `crates/core/src/init/scaffold.rs`
- Modify: `crates/core/src/init/mod.rs`
- Test: inline `#[cfg(test)]` in `scaffold.rs`

**Interfaces:**
- Consumes: `Fsys`, `Git` (`init`, `commit_all`), `Brew` (`leaves`, `casks`), `Paths`, `Plan`/`Source` (Task 1), `RepoConfig`, `SCHEMA_VERSION`, `SetConfig`, `Packages`, `MachineConfig`.
- Produces:
  - `init::STATUS_FRAGMENT: &str`
  - `init::scaffold(fs, root, machine, brew) -> Result<()>`
  - `init::create_or_clone(fs, git, brew, paths, plan) -> Result<PathBuf>`

**Move it verbatim.** `scaffold` and `STATUS_FRAGMENT` currently live in
`crates/cli/src/cmd/init.rs`. Copy them across unchanged — their exact file set
is what `crates/cli/tests/init.rs` and `crates/cli/tests/shell_hook.rs` assert
on, and `shell_hook.rs` compares the scaffolded fragment byte-for-byte against
the hook it tests under real zsh.

- [ ] **Step 1: Write the failing tests**

`crates/core/src/init/scaffold.rs`:

```rust
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
        assert!(fs.read(&root.join("sets/core/set.toml")).unwrap().contains("fake-pkg"));
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
    fn a_failure_part_way_through_leaves_nothing_behind() {
        // FakeBrew::failing makes `leaves()` error, which happens after the
        // repo directory already exists.
        let (fs, git) = (FakeFsys::new(), FakeGit::new());
        let brew = FakeBrew::failing();

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
            "a retry must not trip over a half-built repo, found {leftovers:?}"
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
}
```

- [ ] **Step 2: Add the failing-brew fake**

In `crates/core/src/ports/fake.rs`, add a constructor and a flag so a brew
failure can be exercised:

```rust
    /// A brew whose queries fail, for testing cleanup paths.
    pub fn failing() -> Self {
        Self {
            fail: true,
            ..Default::default()
        }
    }
```

Add `fail: bool` to the struct, and return an error from `leaves` and `casks`
when it is set:

```rust
    fn leaves(&self) -> Result<Vec<String>> {
        if self.fail {
            return Err(Error::Command {
                cmd: "brew leaves".into(),
                stderr: "fake failure".into(),
            });
        }
        Ok(self.leaves.clone())
    }
```

Do the same in `casks`. Every existing `FakeBrew::new`/`seeded` caller keeps
working because `fail` defaults to `false`.

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p dotfix-core init::scaffold`
Expected: FAIL — `cannot find function create_or_clone`.

- [ ] **Step 4: Implement**

Prepend to `crates/core/src/init/scaffold.rs`:

```rust
use std::path::{Path, PathBuf};

use crate::config::{MachineConfig, Packages, RepoConfig, SCHEMA_VERSION, SetConfig};
use crate::error::Result;
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
            git.clone_to(url, &root)?;
            Ok(root)
        }
        Source::New => {
            git.init(&root)?;
            match scaffold(fs, &root, &plan.machine, brew)
                .and_then(|()| git.commit_all(&root, "chore: scaffold dotfix repository"))
            {
                Ok(()) => Ok(root),
                Err(e) => {
                    remove_tree(fs, &root);
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
    if let Ok(files) = fs.list_dir(root) {
        for f in files {
            let _ = fs.remove(&f);
        }
    }
}

pub fn scaffold(fs: &dyn Fsys, root: &Path, machine: &str, brew: &dyn Brew) -> Result<()> {
    // …moved verbatim from crates/cli/src/cmd/init.rs…
}
```

For `scaffold`, copy the existing body from `crates/cli/src/cmd/init.rs`
unchanged, along with its `machine_path` helper. Re-export from
`crates/core/src/init/mod.rs`:

```rust
mod scaffold;
pub use scaffold::{STATUS_FRAGMENT, create_or_clone, scaffold};
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p dotfix-core init && cargo clippy --all-targets -- -D warnings`
Expected: 5 new PASS plus Task 1's, no warnings.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "feat(core): create or clone the data repository as one step"
```

---

## Task 3: Machine configuration, the agent, and the CLI on top

**Files:**
- Modify: `crates/core/src/init/mod.rs`, `crates/cli/src/cmd/init.rs`
- Test: inline `#[cfg(test)]` in `crates/core/src/init/mod.rs`; the existing `crates/cli/tests/init.rs` is the regression guard

**Interfaces:**
- Consumes: `Fsys`, `Paths`, `LocalConfig`, `MachineConfig`, `agent::install`, `Plan` (Task 1), `create_or_clone` (Task 2).
- Produces:
  - `init::configure_machine(fs, repo: &Path, paths: &Paths, plan: &Plan) -> Result<()>`
  - `init::install_agent(fs, paths, binary: &Path, interval: u32, path_env: &str) -> Result<PathBuf>`
  - `init::DEFAULT_INTERVAL: u32 = 3600`

- [ ] **Step 1: Write the failing tests**

Append to `crates/core/src/init/mod.rs`:

```rust
#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::config::{MachineConfig, ProviderKind};
    use crate::paths::{LocalConfig, Paths};
    use crate::ports::Fsys;
    use crate::ports::fake::FakeFsys;

    fn paths() -> Paths {
        Paths::new(PathBuf::from("/Users/test"))
    }

    fn plan(provider: ProviderKind, vault: Option<&str>) -> Plan {
        Plan {
            machine: "box-one".into(),
            source: Source::New,
            secret_provider: provider,
            vault: vault.map(str::to_string),
        }
    }

    #[test]
    fn it_writes_the_local_pointer_so_every_later_command_finds_the_repo() {
        let fs = FakeFsys::new();
        configure_machine(
            &fs,
            Path::new("/Users/test/dotfiles"),
            &paths(),
            &plan(ProviderKind::Keychain, None),
        )
        .unwrap();

        let local = LocalConfig::load(&fs, &paths().local_config()).unwrap();
        assert_eq!(local.machine, "box-one");
        assert_eq!(local.repo, PathBuf::from("/Users/test/dotfiles"));
    }

    #[test]
    fn it_records_the_chosen_secret_provider_on_the_machine() {
        let fs = FakeFsys::new();
        configure_machine(
            &fs,
            Path::new("/Users/test/dotfiles"),
            &paths(),
            &plan(ProviderKind::OnePassword, Some("Example")),
        )
        .unwrap();

        let raw = fs
            .read(Path::new("/Users/test/dotfiles/machines/box-one.toml"))
            .unwrap();
        let cfg: MachineConfig = toml::from_str(&raw).unwrap();
        assert_eq!(cfg.secret_provider, ProviderKind::OnePassword);
        assert_eq!(cfg.vault.as_deref(), Some("Example"));
    }

    #[test]
    fn keychain_needs_no_vault() {
        let fs = FakeFsys::new();
        configure_machine(
            &fs,
            Path::new("/Users/test/dotfiles"),
            &paths(),
            &plan(ProviderKind::Keychain, Some("ignored")),
        )
        .unwrap();

        let raw = fs
            .read(Path::new("/Users/test/dotfiles/machines/box-one.toml"))
            .unwrap();
        let cfg: MachineConfig = toml::from_str(&raw).unwrap();
        assert_eq!(
            cfg.vault, None,
            "a vault only means something for 1Password"
        );
    }

    #[test]
    fn a_cloned_repository_keeps_the_sets_its_machine_file_already_had() {
        // The clone path must not stamp over a machine file the repository
        // already carries for this name.
        let fs = FakeFsys::from([(
            "/Users/test/dotfiles/machines/box-one.toml",
            "sets = [\"core\", \"web\"]\n",
        )]);
        configure_machine(
            &fs,
            Path::new("/Users/test/dotfiles"),
            &paths(),
            &plan(ProviderKind::Keychain, None),
        )
        .unwrap();

        let raw = fs
            .read(Path::new("/Users/test/dotfiles/machines/box-one.toml"))
            .unwrap();
        let cfg: MachineConfig = toml::from_str(&raw).unwrap();
        assert_eq!(cfg.sets, vec!["core", "web"]);
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p dotfix-core init::tests`
Expected: FAIL — `cannot find function configure_machine`.

- [ ] **Step 3: Implement both steps**

Append to `crates/core/src/init/mod.rs`:

```rust
use std::path::{Path, PathBuf};

use crate::error::Result;
use crate::paths::{LocalConfig, Paths};
use crate::ports::Fsys;

/// How often the background check runs, in seconds.
pub const DEFAULT_INTERVAL: u32 = 3600;

/// Record this machine in the repository and point the machine at the
/// repository. Merges into an existing machine file rather than replacing it,
/// so cloning a repository that already knows this machine keeps its sets.
pub fn configure_machine(
    fs: &dyn Fsys,
    repo: &Path,
    paths: &Paths,
    plan: &Plan,
) -> Result<()> {
    let machine_file = repo.join("machines").join(format!("{}.toml", plan.machine));

    let mut cfg: crate::config::MachineConfig = if fs.exists(&machine_file) {
        toml::from_str(&fs.read(&machine_file)?).map_err(|source| crate::Error::Toml {
            path: machine_file.clone(),
            source,
        })?
    } else {
        crate::config::MachineConfig {
            sets: vec!["core".into()],
            ..Default::default()
        }
    };

    cfg.secret_provider = plan.secret_provider;
    // A vault is meaningless outside 1Password; storing one would be a lie
    // the doctor would later report on.
    cfg.vault = match plan.secret_provider {
        crate::config::ProviderKind::OnePassword => plan.vault.clone(),
        _ => None,
    };

    let raw = toml::to_string_pretty(&cfg)
        .map_err(|e| crate::Error::Config(format!("serialising {}: {e}", machine_file.display())))?;
    fs.write(&machine_file, &raw, 0o644)?;

    LocalConfig {
        repo: repo.to_path_buf(),
        machine: plan.machine.clone(),
    }
    .save(fs, &paths.local_config())
}

/// Install the LaunchAgent for the hourly background check.
pub fn install_agent(
    fs: &dyn Fsys,
    paths: &Paths,
    binary: &Path,
    interval: u32,
    path_env: &str,
) -> Result<PathBuf> {
    crate::agent::install(fs, paths, binary, interval, path_env)
}
```

- [ ] **Step 4: Rewrite the CLI on top of the steps**

`crates/cli/src/cmd/init.rs` keeps its prompts and confirmation and calls the
four steps. Delete its local `scaffold`, `STATUS_FRAGMENT` and `machine_path` —
they now live in core.

```rust
use anyhow::{Result, bail};
use dotfix_core::init::{self, Plan, Source};
use dotfix_core::paths::Paths;
use dotfix_core::ports::{Fsys, RealBrew, RealExec, RealFsys, RealGit};

use crate::{ctx, ui};

pub fn run(
    repo_url: Option<String>,
    machine: Option<String>,
    set_up_new: bool,
    yes: bool,
) -> Result<()> {
    let home = ctx::home()?;
    let paths = Paths::new(home);
    let fs = RealFsys;

    if fs.exists(&paths.local_config()) {
        bail!(
            "{} already exists — dotfix is already set up on this machine",
            paths.local_config().display()
        );
    }

    let machine = match machine {
        Some(m) => m,
        None => ui::prompt("machine name")?,
    };

    let source = if set_up_new {
        Source::New
    } else {
        let url = match repo_url {
            Some(u) => u,
            None => ui::prompt("repository url")?,
        };
        Source::Clone { url }
    };

    let plan = Plan {
        machine,
        source,
        // The CLI keeps the default; the app offers the choice.
        secret_provider: Default::default(),
        vault: None,
    };

    let pre = init::preflight(&fs, &RealExec, &paths, &plan);
    for check in &pre.checks {
        let mark = if check.ok { "ok  " } else { "FAIL" };
        println!("  [{mark}] {:<18} {}", check.name, check.detail);
    }
    if !pre.passes() {
        bail!("setup cannot continue — see the failing checks above");
    }

    if !yes {
        let what = match &plan.source {
            Source::New => format!("create a new data repository at {}/dotfiles", paths.home.display()),
            Source::Clone { .. } => format!("clone into {}/dotfiles", paths.home.display()),
        };
        if !ui::confirm(&format!("{what} and install the background agent?"))? {
            println!("aborted");
            return Ok(());
        }
    }

    let repo = init::create_or_clone(&fs, &RealGit, &RealBrew, &paths, &plan)?;
    init::configure_machine(&fs, &repo, &paths, &plan)?;

    let binary = std::env::current_exe()?;
    let path_env = std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into());
    init::install_agent(&fs, &paths, &binary, init::DEFAULT_INTERVAL, &path_env)?;

    println!("dotfix is set up for machine `{}`", plan.machine);
    println!("repository: {}", repo.display());
    if matches!(plan.source, Source::New) {
        println!("everything installed went into set `core` — split it up by editing sets/");
        println!("no remote yet — add one and push when you want other machines to follow:");
        println!("  git -C {} remote add origin <url>", repo.display());
    }
    println!("next: review the repository, then run `dotfix apply`");
    Ok(())
}
```

- [ ] **Step 5: Run everything — the CLI tests are the guard**

Run: `cargo test --workspace && cargo clippy --all-targets -- -D warnings`
Expected: all PASS. `crates/cli/tests/init.rs` and
`crates/cli/tests/shell_hook.rs` must pass **unmodified** — do not edit them. If
`shell_hook.rs`'s byte comparison of the scaffolded fragment fails, the move of
`STATUS_FRAGMENT` was not verbatim.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "refactor: move init out of the CLI into dotfix-core"
```

---

## Task 4: The SSH detection ladder

**Files:**
- Create: `crates/core/src/init/ssh.rs`
- Modify: `crates/core/src/init/mod.rs`
- Test: inline `#[cfg(test)]` in `ssh.rs`

**Interfaces:**
- Consumes: `Exec`.
- Produces:
  - `init::ssh::Reachability::{Ready, NeedsKey, Unreachable { detail: String }}`
  - `init::ssh::probe(exec: &dyn Exec, host: &str) -> Reachability`
  - `init::ssh::PROBE_ARGS: &[&str]`

**Why `BatchMode=yes` is the whole point:** without it, this call from a GUI
with no controlling terminal waits forever for input nobody can give. With it,
SSH refuses to prompt and fails fast, which is an answer we can act on. The
first test below is what keeps that flag from being "tidied away" later.

- [ ] **Step 1: Write the failing tests**

`crates/core/src/init/ssh.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::fake::FakeExec;

    /// GitHub answers a successful `ssh -T` with exit status 1 and this line,
    /// which is why the message matters more than the status.
    const GREETING: &str = "Hi example-user! You've successfully authenticated, \
                            but GitHub does not provide shell access.\n";

    fn key(host: &str) -> String {
        format!("ssh {} -T git@{host}", PROBE_ARGS.join(" "))
    }

    #[test]
    fn the_probe_can_never_prompt() {
        assert!(
            PROBE_ARGS.contains(&"-o") && PROBE_ARGS.contains(&"BatchMode=yes"),
            "without BatchMode a GUI with no TTY hangs instead of failing: {PROBE_ARGS:?}"
        );
        assert!(
            PROBE_ARGS.iter().any(|a| a.starts_with("ConnectTimeout=")),
            "a probe with no timeout is a hang with extra steps: {PROBE_ARGS:?}"
        );
    }

    #[test]
    fn a_working_setup_is_ready() {
        let exec = FakeExec::new([(key("github.com").as_str(), GREETING)]);
        assert_eq!(probe(&exec, "github.com"), Reachability::Ready);
    }

    #[test]
    fn a_refused_key_asks_for_one() {
        let exec = FakeExec::new([]);
        assert_eq!(probe(&exec, "github.com"), Reachability::NeedsKey);
    }

    #[test]
    fn the_probe_targets_the_host_it_was_given() {
        let exec = FakeExec::new([]);
        let _ = probe(&exec, "github.com-dotfix");
        assert_eq!(exec.calls(), vec![key("github.com-dotfix")]);
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p dotfix-core init::ssh`
Expected: FAIL — `cannot find value PROBE_ARGS`.

- [ ] **Step 3: Implement**

Prepend to `crates/core/src/init/ssh.rs`:

```rust
use crate::ports::Exec;

/// Flags that make the probe safe to run from a GUI.
///
/// `BatchMode=yes` forbids SSH any interactive prompt. Without it this call
/// from an app with no controlling terminal blocks forever waiting for a
/// passphrase or a host-key confirmation nobody can type. `ConnectTimeout`
/// bounds the other way to hang: an unreachable host.
pub const PROBE_ARGS: &[&str] = &["-o", "BatchMode=yes", "-o", "ConnectTimeout=5"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reachability {
    /// Existing SSH setup works — clone directly, configure nothing.
    Ready,
    /// Reached the host, but it would not take our key.
    NeedsKey,
    /// Could not get that far.
    Unreachable { detail: String },
}

/// Ask the host whether our SSH setup already works, without ever being able
/// to prompt.
pub fn probe(exec: &dyn Exec, host: &str) -> Reachability {
    let target = format!("git@{host}");
    let mut args: Vec<&str> = PROBE_ARGS.to_vec();
    args.push("-T");
    args.push(&target);

    match exec.run("ssh", &args) {
        // GitHub exits non-zero even on success, so a successful run is still
        // unambiguous evidence.
        Ok(out) if out.contains("successfully authenticated") => Reachability::Ready,
        Ok(_) => Reachability::NeedsKey,
        Err(e) => {
            let text = e.to_string();
            if text.contains("successfully authenticated") {
                Reachability::Ready
            } else if text.contains("Permission denied") || text.contains("publickey") {
                Reachability::NeedsKey
            } else {
                Reachability::Unreachable { detail: text }
            }
        }
    }
}
```

Add `pub mod ssh;` to `crates/core/src/init/mod.rs`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p dotfix-core init::ssh && cargo clippy --all-targets -- -D warnings`
Expected: 4 PASS.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(core): probe ssh reachability without ever prompting"
```

---

## Task 5: Host-key pinning

**Files:**
- Modify: `crates/core/src/init/ssh.rs`
- Test: inline `#[cfg(test)]` in `ssh.rs`

**Interfaces:**
- Consumes: `Exec`, `Fsys`.
- Produces:
  - `init::ssh::GITHUB_FINGERPRINTS: &[&str]`
  - `init::ssh::ensure_host_known(fs, exec, home: &Path, host: &str, expected: &[&str]) -> Result<bool>` — `Ok(true)` when it added an entry, `Ok(false)` when one was already there

**The decision this encodes:** the obvious implementation is
`ssh-keyscan host >> known_hosts`, i.e. trust whatever answers first. Instead we
compare the scanned key's fingerprint against GitHub's published values,
compiled in. On a mismatch **nothing is written**. The cost, stated in the spec:
when GitHub rotates a host key, dotfix needs an update or refuses to proceed.

- [ ] **Step 1: Write the failing tests**

Append to the `tests` module in `crates/core/src/init/ssh.rs`:

```rust
    use std::path::Path;

    use crate::ports::Fsys;
    use crate::ports::fake::FakeFsys;

    const SCANNED: &str = "github.com ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl\n";
    const GOOD_FP: &str = "SHA256:+DiY3wvvV6TuJJhbpZisF/zLDA0zPMSvHdkr4UvCOqU";

    fn scan_exec() -> FakeExec {
        FakeExec::new([
            ("ssh-keyscan -t ed25519 github.com", SCANNED),
            (
                "ssh-keygen -lf -",
                &format!("256 {GOOD_FP} github.com (ED25519)\n"),
            ),
        ])
    }

    #[test]
    fn a_matching_fingerprint_is_written() {
        let fs = FakeFsys::new();
        let added =
            ensure_host_known(&fs, &scan_exec(), Path::new("/Users/test"), "github.com", &[GOOD_FP])
                .unwrap();

        assert!(added);
        let known = fs.read(Path::new("/Users/test/.ssh/known_hosts")).unwrap();
        assert!(known.contains("github.com ssh-ed25519"));
    }

    #[test]
    fn a_mismatched_fingerprint_writes_nothing_and_says_both_values() {
        let fs = FakeFsys::new();
        let err = ensure_host_known(
            &fs,
            &scan_exec(),
            Path::new("/Users/test"),
            "github.com",
            &["SHA256:definitely-not-the-right-one"],
        )
        .unwrap_err();

        let msg = err.to_string();
        assert!(msg.contains(GOOD_FP), "must report what it got: {msg}");
        assert!(msg.contains("definitely-not-the-right-one"), "and what it expected: {msg}");
        assert!(
            !fs.exists(Path::new("/Users/test/.ssh/known_hosts")),
            "a mismatch must leave known_hosts untouched"
        );
    }

    #[test]
    fn an_existing_entry_is_left_alone() {
        let fs = FakeFsys::from([("/Users/test/.ssh/known_hosts", SCANNED)]);
        let added =
            ensure_host_known(&fs, &scan_exec(), Path::new("/Users/test"), "github.com", &[GOOD_FP])
                .unwrap();

        assert!(!added, "an entry that is already there is not added twice");
        assert_eq!(fs.read(Path::new("/Users/test/.ssh/known_hosts")).unwrap(), SCANNED);
    }

    #[test]
    fn the_shipped_github_fingerprints_are_not_empty() {
        assert!(
            !GITHUB_FINGERPRINTS.is_empty(),
            "pinning with an empty list would silently accept anything"
        );
        assert!(GITHUB_FINGERPRINTS.iter().all(|f| f.starts_with("SHA256:")));
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p dotfix-core init::ssh`
Expected: FAIL — `cannot find function ensure_host_known`.

- [ ] **Step 3: Implement**

Append to `crates/core/src/init/ssh.rs`:

```rust
use std::path::Path;

use crate::error::{Error, Result};
use crate::ports::Fsys;

/// GitHub's published SSH host-key fingerprints.
///
/// Source: <https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/githubs-ssh-key-fingerprints>
///
/// Pinning these is deliberate: the alternative is trust-on-first-use, where a
/// first-time setup on a hostile network silently pins the wrong host. The
/// price is that a rotation on GitHub's side needs a dotfix update — which is
/// the trade the design chose, loudly rather than quietly.
pub const GITHUB_FINGERPRINTS: &[&str] = &[
    "SHA256:+DiY3wvvV6TuJJhbpZisF/zLDA0zPMSvHdkr4UvCOqU", // ed25519
    "SHA256:uNiVztksCsDhcc0u9e8BujQXVUpKZIDTMczCvj3tD2s", // rsa
];

/// Make sure `host` is in `known_hosts`, but only if its key is one we expect.
///
/// Returns whether an entry was added. Writes nothing on a mismatch.
pub fn ensure_host_known(
    fs: &dyn Fsys,
    exec: &dyn Exec,
    home: &Path,
    host: &str,
    expected: &[&str],
) -> Result<bool> {
    let known_hosts = home.join(".ssh/known_hosts");

    if fs.exists(&known_hosts) && fs.read(&known_hosts)?.contains(host) {
        return Ok(false);
    }

    let scanned = exec.run("ssh-keyscan", &["-t", "ed25519", host])?;
    let listed = exec.run("ssh-keygen", &["-lf", "-"])?;

    let fingerprint = listed
        .split_whitespace()
        .find(|t| t.starts_with("SHA256:"))
        .ok_or_else(|| {
            Error::Config(format!("could not read a fingerprint for {host} from `{listed}`"))
        })?;

    if !expected.contains(&fingerprint) {
        return Err(Error::Config(format!(
            "host key for {host} does not match a known fingerprint. Got {fingerprint}, \
             expected one of: {}. Nothing was written — do not continue on this network.",
            expected.join(", ")
        )));
    }

    let mut contents = if fs.exists(&known_hosts) {
        fs.read(&known_hosts)?
    } else {
        String::new()
    };
    if !contents.is_empty() && !contents.ends_with('\n') {
        contents.push('\n');
    }
    contents.push_str(&scanned);
    fs.write(&known_hosts, &contents, 0o644)?;
    Ok(true)
}
```

Note for the implementer: `ssh-keygen -lf -` reads the scanned key from stdin in
real use. The `Exec` port has no stdin, so pass the scanned key through a temp
file or extend `Exec` with a `run_with_stdin`. Extending the port is the honest
option — add `fn run_with_stdin(&self, program: &str, args: &[&str], stdin: &str) -> Result<String>`
to `Exec`, implement it in `RealExec` with `Stdio::piped`, and record it in
`FakeExec` under the same key format. Adjust the tests' fake keys accordingly.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p dotfix-core init::ssh && cargo clippy --all-targets -- -D warnings`
Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(core): pin github host keys instead of trusting first contact"
```

---

## Task 6: Deploy key generation

**Files:**
- Modify: `crates/core/src/init/ssh.rs`
- Test: inline `#[cfg(test)]` in `ssh.rs`

**Interfaces:**
- Consumes: `Exec`, `Fsys`.
- Produces:
  - `init::ssh::DeployKey { public: String, path: PathBuf, host_alias: String }`
  - `init::ssh::ensure_deploy_key(fs, exec, home, machine: &str) -> Result<DeployKey>`
  - `init::ssh::ensure_ssh_config(fs, home, key: &DeployKey) -> Result<()>`
  - `init::ssh::clone_url(host_alias: &str, owner_repo: &str) -> String`

- [ ] **Step 1: Write the failing tests**

Append to the `tests` module in `crates/core/src/init/ssh.rs`:

```rust
    #[test]
    fn a_generated_key_has_no_passphrase_and_is_private() {
        let fs = FakeFsys::new();
        let exec = FakeExec::new([(
            "ssh-keygen -t ed25519 -N  -C dotfix@box-one -f /Users/test/.ssh/dotfix_box-one_ed25519",
            "",
        )]);
        // ssh-keygen writes the files; the fake stands in for them.
        fs.write(
            Path::new("/Users/test/.ssh/dotfix_box-one_ed25519.pub"),
            "ssh-ed25519 AAAA... dotfix@box-one\n",
            0o644,
        )
        .unwrap();

        let key = ensure_deploy_key(&fs, &exec, Path::new("/Users/test"), "box-one").unwrap();

        assert!(key.public.starts_with("ssh-ed25519 "));
        assert_eq!(key.host_alias, "github.com-dotfix");
        let call = &exec.calls()[0];
        assert!(call.contains("-N "), "an empty passphrase is required: {call}");
        assert!(!call.contains("-N secret"), "never set a passphrase: {call}");
    }

    #[test]
    fn an_existing_key_is_reused_rather_than_regenerated() {
        let fs = FakeFsys::from([(
            "/Users/test/.ssh/dotfix_box-one_ed25519.pub",
            "ssh-ed25519 AAAA... dotfix@box-one\n",
        )]);
        let exec = FakeExec::new([]);

        let key = ensure_deploy_key(&fs, &exec, Path::new("/Users/test"), "box-one").unwrap();

        assert!(key.public.starts_with("ssh-ed25519 "));
        assert!(
            exec.calls().is_empty(),
            "regenerating would invalidate the deploy key already on GitHub"
        );
    }

    #[test]
    fn the_ssh_config_stanza_scopes_the_key_to_this_alias_only() {
        let fs = FakeFsys::new();
        let key = DeployKey {
            public: "ssh-ed25519 AAAA...".into(),
            path: "/Users/test/.ssh/dotfix_box-one_ed25519".into(),
            host_alias: "github.com-dotfix".into(),
        };
        ensure_ssh_config(&fs, Path::new("/Users/test"), &key).unwrap();

        let cfg = fs.read(Path::new("/Users/test/.ssh/config")).unwrap();
        assert!(cfg.contains("Host github.com-dotfix"));
        assert!(cfg.contains("HostName github.com"));
        assert!(cfg.contains("IdentityFile /Users/test/.ssh/dotfix_box-one_ed25519"));
        assert!(
            cfg.contains("IdentitiesOnly yes"),
            "without this the key leaks into the user's other ssh targets"
        );
    }

    #[test]
    fn writing_the_stanza_twice_does_not_duplicate_it() {
        let fs = FakeFsys::new();
        let key = DeployKey {
            public: "ssh-ed25519 AAAA...".into(),
            path: "/Users/test/.ssh/dotfix_box-one_ed25519".into(),
            host_alias: "github.com-dotfix".into(),
        };
        ensure_ssh_config(&fs, Path::new("/Users/test"), &key).unwrap();
        ensure_ssh_config(&fs, Path::new("/Users/test"), &key).unwrap();

        let cfg = fs.read(Path::new("/Users/test/.ssh/config")).unwrap();
        assert_eq!(cfg.matches("Host github.com-dotfix").count(), 1);
    }

    #[test]
    fn an_existing_ssh_config_is_appended_to_not_replaced() {
        let fs = FakeFsys::from([("/Users/test/.ssh/config", "Host *\n  AddKeysToAgent yes\n")]);
        let key = DeployKey {
            public: "ssh-ed25519 AAAA...".into(),
            path: "/Users/test/.ssh/dotfix_box-one_ed25519".into(),
            host_alias: "github.com-dotfix".into(),
        };
        ensure_ssh_config(&fs, Path::new("/Users/test"), &key).unwrap();

        let cfg = fs.read(Path::new("/Users/test/.ssh/config")).unwrap();
        assert!(cfg.contains("AddKeysToAgent yes"), "must not clobber the user's config");
        assert!(cfg.contains("Host github.com-dotfix"));
    }

    #[test]
    fn the_clone_url_uses_the_alias() {
        assert_eq!(
            clone_url("github.com-dotfix", "example/dotfiles"),
            "git@github.com-dotfix:example/dotfiles.git"
        );
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p dotfix-core init::ssh`
Expected: FAIL — `cannot find type DeployKey`.

- [ ] **Step 3: Implement**

Append to `crates/core/src/init/ssh.rs`:

```rust
use std::path::PathBuf;

/// A key dedicated to dotfix, scoped by an ssh_config alias to one repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeployKey {
    /// The public half — the only part that ever leaves this machine.
    pub public: String,
    pub path: PathBuf,
    pub host_alias: String,
}

const HOST_ALIAS: &str = "github.com-dotfix";

/// Generate a dotfix-only key, or reuse the one that is already there.
///
/// No passphrase: a passphrase would need an agent, which is the problem this
/// whole path exists to avoid. That is the standard shape for a deploy key,
/// and the mitigation is scope — the key opens exactly one repository, and
/// there is one per machine so revoking a machine is deleting one key.
pub fn ensure_deploy_key(
    fs: &dyn Fsys,
    exec: &dyn Exec,
    home: &Path,
    machine: &str,
) -> Result<DeployKey> {
    let path = home.join(".ssh").join(format!("dotfix_{machine}_ed25519"));
    let public_path = path.with_extension("pub");

    // Regenerating would invalidate the deploy key the user already added on
    // GitHub, so an existing key is always reused.
    if !fs.exists(&public_path) {
        let comment = format!("dotfix@{machine}");
        exec.run(
            "ssh-keygen",
            &[
                "-t",
                "ed25519",
                "-N",
                "",
                "-C",
                &comment,
                "-f",
                &path.display().to_string(),
            ],
        )?;
    }

    Ok(DeployKey {
        public: fs.read(&public_path)?.trim().to_string(),
        path,
        host_alias: HOST_ALIAS.to_string(),
    })
}

/// Append a stanza that uses this key for the alias and nothing else.
pub fn ensure_ssh_config(fs: &dyn Fsys, home: &Path, key: &DeployKey) -> Result<()> {
    let config = home.join(".ssh/config");
    let existing = if fs.exists(&config) {
        fs.read(&config)?
    } else {
        String::new()
    };

    let header = format!("Host {}", key.host_alias);
    if existing.contains(&header) {
        return Ok(());
    }

    let stanza = format!(
        "\n# added by dotfix — scopes its own key to this alias only\n\
         {header}\n  \
         HostName github.com\n  \
         User git\n  \
         IdentityFile {}\n  \
         IdentitiesOnly yes\n",
        key.path.display()
    );

    let mut contents = existing;
    if !contents.is_empty() && !contents.ends_with('\n') {
        contents.push('\n');
    }
    contents.push_str(&stanza);
    fs.write(&config, &contents, 0o600)
}

/// `github.com-dotfix` + `example/dotfiles` → `git@github.com-dotfix:example/dotfiles.git`
pub fn clone_url(host_alias: &str, owner_repo: &str) -> String {
    let repo = owner_repo.trim_end_matches(".git");
    format!("git@{host_alias}:{repo}.git")
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p dotfix-core init::ssh && cargo clippy --all-targets -- -D warnings`
Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(core): generate a scoped deploy key for cloning"
```

---

## Task 7: The token path and repository creation

**Files:**
- Create: `crates/core/src/init/remote.rs`
- Modify: `crates/core/src/init/mod.rs`
- Test: inline `#[cfg(test)]` in `remote.rs`

**Interfaces:**
- Consumes: `Exec`.
- Produces:
  - `init::remote::store_token(exec, host: &str, user: &str, token: &str) -> Result<()>`
  - `init::remote::RepoRequest { owner: String, name: String }`
  - `init::remote::describe(req: &RepoRequest) -> String` — what the confirmation shows
  - `init::remote::create_repo(exec, req: &RepoRequest) -> Result<String>` — returns the clone URL
  - `init::remote::https_url(owner_repo: &str) -> String`

**The rule this task must not break:** dotfix stores no credential of its own.
The token is handed once to git's credential helper and forgotten. Nothing
writes it to a dotfix file, a log, or an error message.

- [ ] **Step 1: Write the failing tests**

`crates/core/src/init/remote.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::fake::FakeExec;

    #[test]
    fn a_token_goes_to_the_keychain_and_nowhere_else() {
        let exec = FakeExec::new([("git credential-osxkeychain store", "")]);
        store_token(&exec, "github.com", "example-user", "ghp_example").unwrap();

        let calls = exec.calls();
        assert_eq!(calls, vec!["git credential-osxkeychain store".to_string()]);
        assert!(
            !calls.iter().any(|c| c.contains("ghp_example")),
            "the token must travel on stdin, never in an argument list where \
             it would show up in `ps` and in our own call log: {calls:?}"
        );
    }

    #[test]
    fn a_failed_store_does_not_echo_the_token() {
        let exec = FakeExec::new([]);
        let err = store_token(&exec, "github.com", "example-user", "ghp_example").unwrap_err();
        assert!(
            !err.to_string().contains("ghp_example"),
            "an error must never carry the value: {err}"
        );
    }

    #[test]
    fn the_confirmation_names_owner_name_and_visibility() {
        let text = describe(&RepoRequest {
            owner: "example-user".into(),
            name: "dotfiles".into(),
        });
        assert!(text.contains("example-user/dotfiles"));
        assert!(text.to_lowercase().contains("private"));
    }

    #[test]
    fn creating_a_repository_asks_gh_for_a_private_one() {
        let exec = FakeExec::new([(
            "gh repo create example-user/dotfiles --private --clone=false",
            "https://github.com/example-user/dotfiles\n",
        )]);
        let url = create_repo(
            &exec,
            &RepoRequest {
                owner: "example-user".into(),
                name: "dotfiles".into(),
            },
        )
        .unwrap();

        assert_eq!(url, "https://github.com/example-user/dotfiles");
        assert!(exec.calls()[0].contains("--private"));
    }

    #[test]
    fn https_url_is_built_from_owner_and_repo() {
        assert_eq!(
            https_url("example-user/dotfiles"),
            "https://github.com/example-user/dotfiles.git"
        );
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p dotfix-core init::remote`
Expected: FAIL — `cannot find function store_token`.

- [ ] **Step 3: Implement**

Prepend to `crates/core/src/init/remote.rs`:

```rust
use crate::error::Result;
use crate::ports::Exec;

/// What the user is asked to confirm before anything happens to their account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoRequest {
    pub owner: String,
    pub name: String,
}

/// The sentence shown in the confirmation. Says private explicitly, because
/// creating a public repository of someone's dotfiles would be a catastrophe
/// worth spelling out.
pub fn describe(req: &RepoRequest) -> String {
    format!(
        "Create the private repository {}/{} on GitHub using your `gh` login?",
        req.owner, req.name
    )
}

/// Hand the token to git's credential helper, which puts it in the macOS
/// Keychain. dotfix does not keep it.
///
/// The value travels on stdin — never as an argument, where it would be
/// visible in `ps` output and in our own call log.
pub fn store_token(exec: &dyn Exec, host: &str, user: &str, token: &str) -> Result<()> {
    let payload = format!("protocol=https\nhost={host}\nusername={user}\npassword={token}\n\n");
    exec.run_with_stdin("git", &["credential-osxkeychain", "store"], &payload)
        .map(|_| ())
}

/// Create the repository through the user's own authenticated `gh`, so dotfix
/// never needs account-wide access of its own.
pub fn create_repo(exec: &dyn Exec, req: &RepoRequest) -> Result<String> {
    let slug = format!("{}/{}", req.owner, req.name);
    let out = exec.run("gh", &["repo", "create", &slug, "--private", "--clone=false"])?;
    Ok(out.trim().to_string())
}

pub fn https_url(owner_repo: &str) -> String {
    let repo = owner_repo.trim_end_matches(".git");
    format!("https://github.com/{repo}.git")
}
```

`run_with_stdin` is the `Exec` method added in Task 5. If Task 5 chose the temp
file route instead, add it here — a token must not go through a temp file.

Add `pub mod remote;` to `crates/core/src/init/mod.rs`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p dotfix-core init && cargo clippy --all-targets -- -D warnings`
Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(core): add token storage via the keychain and gh repo creation"
```

---

## Task 8: Wizard commands

**Files:**
- Modify: `app/src-tauri/src/commands.rs`, `app/src-tauri/src/lib.rs`
- Test: inline `#[cfg(test)]` in `commands.rs`

**Interfaces:**
- Consumes: everything from Tasks 1–7; `Paths`, `RealFsys`/`RealExec`/`RealGit`/`RealBrew`.
- Produces these Tauri commands:
  - `init_preflight(plan: WizardPlan) -> Result<Preflight, String>`
  - `init_probe_ssh() -> Result<String, String>` — `"ready" | "needs_key" | "unreachable: …"`
  - `init_deploy_key(machine: String) -> Result<DeployKeyView, String>`
  - `init_store_token(user: String, token: String) -> Result<(), String>`
  - `init_create_repo(owner: String, name: String) -> Result<String, String>`
  - `init_run(plan: WizardPlan) -> Result<StepOutcome, String>` — runs create_or_clone → configure_machine → install_agent, reporting which step it reached
  - `WizardPlan { machine, mode: "new" | "clone", url: Option<String>, secret_provider: String, vault: Option<String> }`
  - `StepOutcome { completed: Vec<String>, repo: Option<String> }`

- [ ] **Step 1: Write the failing tests**

Append to the `tests` module in `app/src-tauri/src/commands.rs`:

```rust
    #[test]
    fn a_wizard_plan_converts_to_a_core_plan() {
        let wp = WizardPlan {
            machine: "box-one".into(),
            mode: "clone".into(),
            url: Some("git@github.com:example/dotfiles.git".into()),
            secret_provider: "1password".into(),
            vault: Some("Example".into()),
        };
        let plan = wp.to_plan().unwrap();

        assert_eq!(plan.machine, "box-one");
        assert_eq!(
            plan.source,
            dotfix_core::init::Source::Clone {
                url: "git@github.com:example/dotfiles.git".into()
            }
        );
        assert_eq!(plan.secret_provider, dotfix_core::config::ProviderKind::OnePassword);
        assert_eq!(plan.vault.as_deref(), Some("Example"));
    }

    #[test]
    fn a_clone_without_a_url_is_rejected_before_anything_runs() {
        let wp = WizardPlan {
            machine: "box-one".into(),
            mode: "clone".into(),
            url: None,
            secret_provider: "keychain".into(),
            vault: None,
        };
        assert!(wp.to_plan().is_err());
    }

    #[test]
    fn an_unknown_mode_is_rejected_rather_than_defaulted() {
        let wp = WizardPlan {
            machine: "box-one".into(),
            mode: "teleport".into(),
            url: None,
            secret_provider: "keychain".into(),
            vault: None,
        };
        let err = wp.to_plan().unwrap_err();
        assert!(err.contains("teleport"), "name what was wrong: {err}");
    }

    #[test]
    fn an_unknown_secret_provider_is_rejected_rather_than_defaulted() {
        let wp = WizardPlan {
            machine: "box-one".into(),
            mode: "new".into(),
            url: None,
            secret_provider: "magic".into(),
            vault: None,
        };
        // Silently falling back to Keychain would write a machine file that
        // disagrees with what the user picked.
        assert!(wp.to_plan().is_err());
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p dotfix-app commands`
Expected: FAIL — `cannot find type WizardPlan`.

- [ ] **Step 3: Implement**

Append to `app/src-tauri/src/commands.rs`:

```rust
use dotfix_core::init::{self, Plan, Source};
use dotfix_core::init::ssh::{self, Reachability};
use dotfix_core::init::remote::{self, RepoRequest};

/// The wizard's answers, as they cross from the webview. Kept as strings so
/// the frontend does not have to mirror Rust enums; converted once, here,
/// where an unknown value is an error rather than a silent default.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct WizardPlan {
    pub machine: String,
    pub mode: String,
    pub url: Option<String>,
    pub secret_provider: String,
    pub vault: Option<String>,
}

impl WizardPlan {
    pub fn to_plan(&self) -> std::result::Result<Plan, String> {
        let source = match self.mode.as_str() {
            "new" => Source::New,
            "clone" => Source::Clone {
                url: self
                    .url
                    .clone()
                    .filter(|u| !u.trim().is_empty())
                    .ok_or("cloning needs a repository url")?,
            },
            other => return Err(format!("unknown setup mode `{other}`")),
        };

        let secret_provider = match self.secret_provider.as_str() {
            "keychain" => dotfix_core::config::ProviderKind::Keychain,
            "1password" => dotfix_core::config::ProviderKind::OnePassword,
            "age" => dotfix_core::config::ProviderKind::Age,
            other => return Err(format!("unknown secret provider `{other}`")),
        };

        Ok(Plan {
            machine: self.machine.trim().to_string(),
            source,
            secret_provider,
            vault: self.vault.clone().filter(|v| !v.trim().is_empty()),
        })
    }
}

/// What `init_run` reached, so the wizard can mark steps done and retry only
/// the one that failed.
#[derive(Debug, Clone, serde::Serialize)]
pub struct StepOutcome {
    pub completed: Vec<String>,
    pub repo: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct DeployKeyView {
    pub public: String,
    pub host_alias: String,
}
```

Then the commands themselves, each constructing the real ports directly —
`Ctx::load` cannot be used here, because the whole point is that there is no
local configuration yet:

```rust
#[tauri::command]
pub fn init_preflight(plan: WizardPlan) -> Result<init::Preflight, String> {
    let plan = plan.to_plan()?;
    let paths = dotfix_core::paths::Paths::new(home()?);
    Ok(init::preflight(&RealFsys, &RealExec, &paths, &plan))
}

#[tauri::command]
pub fn init_run(app: tauri::AppHandle, plan: WizardPlan) -> Result<StepOutcome, String> {
    let plan = plan.to_plan()?;
    let paths = dotfix_core::paths::Paths::new(home()?);
    let mut completed = Vec::new();

    let repo = init::create_or_clone(&RealFsys, &RealGit, &RealBrew, &paths, &plan)
        .map_err(to_cmd_err)?;
    completed.push("create_or_clone".to_string());

    init::configure_machine(&RealFsys, &repo, &paths, &plan).map_err(to_cmd_err)?;
    completed.push("configure_machine".to_string());

    let binary = std::env::current_exe().map_err(|e| e.to_string())?;
    let path_env = std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into());
    init::install_agent(&RealFsys, &paths, &binary, init::DEFAULT_INTERVAL, &path_env)
        .map_err(to_cmd_err)?;
    completed.push("install_agent".to_string());

    // The window now has a configuration, so the menubar must stop saying
    // "not set up".
    if let Ok(o) = overview() {
        let _ = crate::tray::update(&app, &o);
    }

    Ok(StepOutcome {
        completed,
        repo: Some(repo.display().to_string()),
    })
}
```

Write `init_probe_ssh`, `init_deploy_key`, `init_store_token` and
`init_create_repo` the same way, each a thin wrapper over its core function,
and add a private `home()` helper reading `$HOME`. Register all six in
`lib.rs`'s `generate_handler!`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --workspace && cargo clippy --all-targets -- -D warnings`
Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(app): add the wizard's commands"
```

---

## Task 9: The wizard

**Files:**
- Create: `app/src/wizard/Wizard.tsx`, `Answers.tsx`, `PreflightList.tsx`, `DeployKey.tsx`
- Modify: `app/src/App.tsx`, `app/src/api.ts`, `app/src/types.ts`
- Test: `app/src/wizard/Wizard.test.tsx`, `Answers.test.tsx`, `PreflightList.test.tsx`

**Before starting:** invoke the `frontend-design` skill. Follow the language
already established in `app/src/components/` and `app/src/index.css`.

**Interfaces:**
- Consumes: the six commands from Task 8; the existing `Check` shape (`{ name, ok, detail }`).
- Produces: `<Wizard onDone>` rendered by `App.tsx` in place of the not-configured panel.

**What the wizard is:** four lines, one per step, each with its state. When one
fails the earlier ones stay done and only the failed one offers a retry. There
is no blanket "setup failed" — the line itself carries the explanation, because
the user has no terminal open to investigate.

- [ ] **Step 1: Write the failing tests**

`app/src/wizard/PreflightList.test.tsx`:

```tsx
import { render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { cleanup } from "@testing-library/react";

import PreflightList from "./PreflightList";

afterEach(cleanup);

describe("PreflightList", () => {
  it("marks failing checks for screen readers, not only by colour", () => {
    render(
      <PreflightList
        checks={[
          { name: "git", ok: true, detail: "git version 2.51.0" },
          { name: "homebrew", ok: false, detail: "not found" },
        ]}
      />,
    );
    expect(screen.getByText("homebrew").closest("li")).toHaveAttribute(
      "data-ok",
      "false",
    );
    expect(screen.getByText(/not found/)).toBeInTheDocument();
  });

  it("shows each check's detail so a failure explains itself", () => {
    render(
      <PreflightList
        checks={[{ name: "github cli", ok: false, detail: "not logged in" }]}
      />,
    );
    expect(screen.getByText(/not logged in/)).toBeInTheDocument();
  });
});
```

`app/src/wizard/Wizard.test.tsx`:

```tsx
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => invoke(...a) }));

import Wizard from "./Wizard";

afterEach(() => {
  cleanup();
  invoke.mockReset();
});

const passingPreflight = {
  checks: [{ name: "git", ok: true, detail: "2.51.0" }],
};

describe("Wizard", () => {
  it("will not run setup while pre-flight is failing", async () => {
    invoke.mockResolvedValue({
      checks: [{ name: "homebrew", ok: false, detail: "not found" }],
    });
    render(<Wizard onDone={vi.fn()} />);

    fireEvent.change(screen.getByLabelText(/machine name/i), {
      target: { value: "box-one" },
    });
    fireEvent.click(screen.getByRole("button", { name: /check/i }));

    await screen.findByText(/not found/);
    expect(screen.getByRole("button", { name: /set up/i })).toBeDisabled();
  });

  it("reports which step failed and offers a retry for that step only", async () => {
    invoke.mockImplementation((cmd: string) => {
      if (cmd === "init_preflight") return Promise.resolve(passingPreflight);
      if (cmd === "init_run")
        return Promise.reject("host key for github.com is not known");
      return Promise.resolve({});
    });
    render(<Wizard onDone={vi.fn()} />);

    fireEvent.change(screen.getByLabelText(/machine name/i), {
      target: { value: "box-one" },
    });
    fireEvent.click(screen.getByRole("button", { name: /check/i }));
    await screen.findByText(/2\.51\.0/);
    fireEvent.click(screen.getByRole("button", { name: /set up/i }));

    await screen.findByText(/host key for github.com is not known/);
    expect(screen.getByRole("button", { name: /retry/i })).toBeInTheDocument();
  });

  it("tells the parent when setup completed", async () => {
    const onDone = vi.fn();
    invoke.mockImplementation((cmd: string) => {
      if (cmd === "init_preflight") return Promise.resolve(passingPreflight);
      if (cmd === "init_run")
        return Promise.resolve({
          completed: ["create_or_clone", "configure_machine", "install_agent"],
          repo: "/Users/test/dotfiles",
        });
      return Promise.resolve({});
    });
    render(<Wizard onDone={onDone} />);

    fireEvent.change(screen.getByLabelText(/machine name/i), {
      target: { value: "box-one" },
    });
    fireEvent.click(screen.getByRole("button", { name: /check/i }));
    await screen.findByText(/2\.51\.0/);
    fireEvent.click(screen.getByRole("button", { name: /set up/i }));

    await waitFor(() => expect(onDone).toHaveBeenCalled());
  });
});
```

`app/src/wizard/Answers.test.tsx`:

```tsx
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import Answers from "./Answers";

afterEach(cleanup);

const base = {
  machine: "",
  mode: "new" as const,
  url: "",
  secretProvider: "keychain" as const,
  vault: "",
};

describe("Answers", () => {
  it("asks for a url only when cloning", () => {
    const { rerender } = render(<Answers value={base} onChange={vi.fn()} />);
    expect(screen.queryByLabelText(/repository url/i)).toBeNull();

    rerender(<Answers value={{ ...base, mode: "clone" }} onChange={vi.fn()} />);
    expect(screen.getByLabelText(/repository url/i)).toBeInTheDocument();
  });

  it("asks for a vault only for 1Password", () => {
    const { rerender } = render(<Answers value={base} onChange={vi.fn()} />);
    expect(screen.queryByLabelText(/vault/i)).toBeNull();

    rerender(
      <Answers value={{ ...base, secretProvider: "1password" }} onChange={vi.fn()} />,
    );
    expect(screen.getByLabelText(/vault/i)).toBeInTheDocument();
  });

  it("reports every change to the parent", () => {
    const onChange = vi.fn();
    render(<Answers value={base} onChange={onChange} />);
    fireEvent.change(screen.getByLabelText(/machine name/i), {
      target: { value: "box-one" },
    });
    expect(onChange).toHaveBeenCalledWith(
      expect.objectContaining({ machine: "box-one" }),
    );
  });
});
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd app && npm test`
Expected: FAIL — `Cannot find module './Wizard'`.

- [ ] **Step 3: Implement**

Add to `app/src/types.ts`:

```ts
export interface Check {
  name: string;
  ok: boolean;
  detail: string;
}

export interface Preflight {
  checks: Check[];
}

export interface StepOutcome {
  completed: string[];
  repo: string | null;
}

export interface WizardAnswers {
  machine: string;
  mode: "new" | "clone";
  url: string;
  secretProvider: "keychain" | "1password" | "age";
  vault: string;
}
```

Add to `app/src/api.ts`:

```ts
export const initPreflight = (plan: WizardPlanArg) =>
  call<Preflight>("init_preflight", { plan });

export const initRun = (plan: WizardPlanArg) =>
  call<StepOutcome>("init_run", { plan });
```

where `WizardPlanArg` is the snake_case shape the Rust `WizardPlan` expects
(`machine`, `mode`, `url`, `secret_provider`, `vault`) — convert from
`WizardAnswers` in one place so no component has to think about it.

Build `PreflightList` as a `<ul>` whose items carry `data-ok`, `Answers` as the
four fields with the two conditional ones, and `Wizard` as the shell holding
answers, pre-flight result, per-step state and the error of the step that
failed. `App.tsx` renders `<Wizard onDone={…}>` instead of the not-configured
panel, and on completion reloads the overview.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd app && npm test && npm run typecheck`
Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(app): add the first-run wizard"
```

---

## Task 10: The deploy-key and token screens

**Files:**
- Modify: `app/src/wizard/Wizard.tsx`, `app/src/wizard/DeployKey.tsx`, `app/src/api.ts`
- Test: `app/src/wizard/DeployKey.test.tsx`

**Interfaces:**
- Consumes: `init_probe_ssh`, `init_deploy_key`, `init_store_token`, `init_create_repo` (Task 8).
- Produces: the authentication branch of the clone path.

- [ ] **Step 1: Write the failing tests**

`app/src/wizard/DeployKey.test.tsx`:

```tsx
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import DeployKey from "./DeployKey";

afterEach(cleanup);

const key = { public: "ssh-ed25519 AAAA... dotfix@box-one", host_alias: "github.com-dotfix" };

describe("DeployKey", () => {
  it("shows the public key and never asks for the private one", () => {
    render(<DeployKey deployKey={key} onTest={vi.fn()} />);
    expect(screen.getByText(/ssh-ed25519 AAAA/)).toBeInTheDocument();
    expect(screen.queryByLabelText(/private key/i)).toBeNull();
  });

  it("spells out that write access must be granted", () => {
    render(<DeployKey deployKey={key} onTest={vi.fn()} />);
    expect(screen.getByText(/allow write access/i)).toBeInTheDocument();
  });

  it("offers a connection test", () => {
    const onTest = vi.fn();
    render(<DeployKey deployKey={key} onTest={onTest} />);
    fireEvent.click(screen.getByRole("button", { name: /test connection/i }));
    expect(onTest).toHaveBeenCalled();
  });
});
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd app && npm test`
Expected: FAIL — `Cannot find module './DeployKey'`.

- [ ] **Step 3: Implement**

`DeployKey` shows the public key in a `<pre>` with a copy button, a link that
opens `https://github.com/<owner>/<repo>/settings/keys/new`, the sentence about
**"Allow write access"**, and a "test connection" button wired to
`init_probe_ssh`.

In `Wizard`, the clone path runs `init_probe_ssh` first:
`ready` → straight to `init_run`; `needs_key` → offer the deploy key (default)
or a token field; `unreachable` → show the detail and a retry.

The token field must be `type="password"`, must never be echoed back after
submission, and its value must not be held in component state longer than the
call: pass it straight to `init_store_token` and clear it.

- [ ] **Step 4: Run everything**

Run:

```bash
cd app && npm test && npm run typecheck
cd .. && cargo test --workspace && cargo fmt --check && cargo clippy --all-targets -- -D warnings
```

Expected: all PASS.

- [ ] **Step 5: Run it for real**

Run: `cd app && npm run tauri dev`

Temporarily move `~/.config/dotfix/config.toml` aside so the wizard appears.
Walk the "new" path end to end and confirm a repository is created. Put the
config back afterwards. **This step is not optional** — the phase 2 tray defect
survived seven tasks of green tests precisely because nobody started the app.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "feat(app): add the deploy-key and token screens"
```

---

## Plan self-review

**Spec coverage**

| Spec section | Task |
|---|---|
| `Plan` / `Source` | 1 |
| `preflight` returning `doctor::Check`, never writing | 1 |
| `create_or_clone`, the only step that creates the directory | 2 |
| No half-built state on failure | 2 |
| `configure_machine`, `install_agent` | 3 |
| CLI keeps its prompts, calls the same steps | 3 |
| CLI integration tests unchanged as the regression guard | 3 |
| Detection ladder with `BatchMode=yes` | 4 |
| Host-key fingerprint pinning, nothing written on mismatch | 5 |
| Deploy key, no passphrase, `IdentitiesOnly yes` | 6 |
| Token via `git credential-osxkeychain`, not persisted by dotfix | 7 |
| `gh repo create` behind a confirmation naming owner/name/visibility | 7 |
| Wizard commands | 8 |
| Four steps, per-step retry, the line carries the explanation | 9 |
| Deploy-key screen, "Allow write access" | 10 |
| Secret provider and vault in the wizard | 8 (conversion), 9 (`Answers`) |
| No set splitting in the wizard | absent by construction |
| A real run on a real machine | 10, step 5 |

**Gaps found and closed during review**

1. `doctor::Check`'s constructors are private and `init` is a sibling module —
   Task 1 makes them `pub(crate)`. Without this the plan does not compile, and
   an implementer would have discovered it only mid-task.
2. `ssh-keygen -lf -` reads from stdin, which the `Exec` port cannot do. Task 5
   names the port extension explicitly rather than leaving it to be improvised,
   and Task 7 depends on the same method to keep the token off the argument
   list — where it would otherwise be visible in `ps`.
3. The clone path can meet a machine file the repository already has for this
   name. `configure_machine` merges rather than overwrites, with a test, so
   cloning does not silently discard that machine's sets.
4. `create_or_clone` running `git init` **before** writing is not cosmetic: the
   cleanup path needs the directory to exist first so it knows what it owns.
   Task 2 pins the ordering with its own test.

**Type consistency**

`Plan`/`Source` (Task 1) are consumed unchanged by Tasks 2, 3 and 8.
`Preflight { checks: Vec<Check> }` (Task 1) is what `PreflightList` renders
(Task 9), with `Check`'s three fields identical on both sides.
`DeployKey { public, path, host_alias }` (Task 6) is narrowed to
`DeployKeyView { public, host_alias }` at the command boundary (Task 8) — the
private key's path deliberately does not cross into the webview — and that is
the shape `DeployKey.tsx` consumes (Task 10). `StepOutcome.completed` uses the
step names `create_or_clone`, `configure_machine`, `install_agent`, matching the
function names in Tasks 2 and 3.

**Placeholder scan**

No `TBD`/`TODO`. Task 3's `scaffold` body is explicitly a verbatim move from a
named file rather than re-printed, because re-printing it would invite drift
from the two CLI tests that assert on its exact output.

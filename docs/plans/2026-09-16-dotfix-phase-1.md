# dotfix Phase 1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build `dotfix` — a macOS CLI that keeps Homebrew packages, shell configuration and application config files in sync across machines via a private git repository, with set-based composition, three-state drift detection, an hourly background check and a fresh-Mac bootstrap.

**Architecture:** A Rust workspace with two crates. `dotfix-core` holds all logic and never touches the outside world directly — it talks to Homebrew, the filesystem and git through three narrow traits (`Brew`, `Fsys`, `Git`), which tests replace with in-memory fakes. `dotfix` (the CLI binary) owns the side effects: it constructs the real port implementations, renders human output, prompts, and computes timestamps. Everything that decides is in `core`; everything that touches the world is in the ports or the CLI.

**Tech Stack:** Rust 1.96 (edition 2024), `clap` 4 (derive), `serde` + `toml` + `serde_json`, `minijinja` 2 for templating, `sha2` for checksums, `thiserror` in core / `anyhow` in the CLI, `plist` for the LaunchAgent, `tempfile` + `assert_cmd` for tests. GitHub Actions for CI, Homebrew tap for distribution.

**Spec:** `docs/specs/2026-09-16-dotfix-design.md`

## Global Constraints

- **Toolchain:** Rust 1.96, edition 2024. Pin via `rust-toolchain.toml` (`channel = "1.96"`).
- **Platform:** macOS only. Homebrew, Keychain and `launchd` are load-bearing. Do not add Linux/Windows shims.
- **Repository boundary:** `NoiXdev/dotfix` is **public**. It must contain no package lists, no machine names, no personal paths, and no reference to internal infrastructure. Test fixtures use invented names (`alpha`, `bravo`, `fake-pkg`), never the author's real packages.
- **Language:** all code, comments, commit messages, branch names and documentation in English.
- **Commits:** Conventional Commits (`feat`/`fix`/`docs`/`refactor`/`test`/`build`/`ci`/`chore`). Every commit ends with the footer `Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>`.
- **Lints:** `cargo clippy --all-targets -- -D warnings` must pass. `cargo fmt --check` must pass.
- **TDD:** every task writes the failing test first, runs it to confirm the failure, then implements.
- **Secrets:** secret values must never be written to logs, diffs, `status` output, backups in plaintext form, or test fixtures. Rendered files containing secrets are written with mode `0600`.
- **No network in unit tests.** `Git`, `Brew` and `Fsys` are always faked in `core` tests.

---

## Spec refinement discovered while planning

The spec defines four drift classes but the package side of the three-way diff
produces **five** meaningful outcomes. The missing one is the exact case the
three-state model was introduced to catch:

| Desired | Actual | Applied | Meaning | Class |
|---|---|---|---|---|
| ✓ | ✗ | ✗ | repo wants it, never installed here | `IncomingPackage` |
| ✓ | ✗ | ✓ | dotfix installed it, **user removed it locally** | `LocallyRemoved` ← missing in spec |
| ✗ | ✓ | ✗ | installed by hand, in no set | `Unmanaged` |
| ✗ | ✓ | ✓ | was in a set, removed from the set | `RemovedPackage` |
| ✓ | ✓ | — | in sync | none |

Without `LocallyRemoved`, a deliberate local `brew uninstall` is indistinguishable
from "another Mac added this package", and `dotfix` would reinstall it on every
run — precisely the failure the spec's three-state section argues against.
`LocallyRemoved` routes into the adopt flow: *remove it from the set, or
reinstall it here?*

Files additionally get `RemovedFile` (managed file dropped from a set) so stale
files do not linger forever. A managed file deleted locally is treated as
`IncomingFile` (re-create) rather than as an intentional removal — deleting a
config file is not a durable signal, and the escape hatch for "I do not want
this here" is deactivating the set.

**Total drift variants: 7, covering the spec's 4 classes.**

---

## File Structure

### `crates/core` — all logic, no side effects

| File | Responsibility |
|---|---|
| `src/lib.rs` | public re-exports only |
| `src/error.rs` | `Error` enum, `Result<T>` alias |
| `src/paths.rs` | `Paths` (state/config/backup locations), `LocalConfig` (which repo, which machine) |
| `src/config/set.rs` | `SetConfig`, `Packages`, `ManagedFile`, `FileMode` |
| `src/config/machine.rs` | `MachineConfig`, `ProviderKind` |
| `src/config/repo.rs` | `RepoConfig`, `Repo::load`, `Repo::resolve` → `Desired` |
| `src/ports/fsys.rs` | `Fsys` trait + `RealFsys` |
| `src/ports/brew.rs` | `Brew` trait + `RealBrew` |
| `src/ports/git.rs` | `Git` trait + `RealGit`, `Commit` |
| `src/ports/fake.rs` | `FakeFsys`, `FakeBrew`, `FakeGit` (cfg: test or `fakes` feature) |
| `src/state.rs` | `Applied` (applied.json) load/save |
| `src/drift/packages.rs` | three-way package diff |
| `src/drift/files.rs` | file diff (checksum based) |
| `src/drift/mod.rs` | `Drift`, `PackageRef`, `Report`, `Counts`, `detect()` |
| `src/render/template.rs` | `Vars`, `render()`, `Rendered`, minijinja env + `secret()` |
| `src/render/zshrc.rs` | fragment assembly, marker, checksum |
| `src/secrets/mod.rs` | `SecretProvider` trait, `Resolver`, default reference derivation |
| `src/secrets/keychain.rs` | `KeychainProvider` |
| `src/secrets/onepassword.rs` | `OnePasswordProvider` |
| `src/secrets/age.rs` | `AgeProvider` |
| `src/apply.rs` | `Action`, `Plan`, `plan()`, `execute()`, backups |
| `src/adopt.rs` | deny list, `AdoptProposal`, repo mutations |
| `src/agent.rs` | LaunchAgent plist generation |
| `src/doctor.rs` | `Check`, `CheckResult`, `run_checks()` |
| `src/status_line.rs` | one-line summary rendering |

### `crates/cli` — side effects, presentation

| File | Responsibility |
|---|---|
| `src/main.rs` | clap parsing, dispatch, exit codes |
| `src/cmd/status.rs` | `dotfix status` |
| `src/cmd/apply.rs` | `dotfix apply` |
| `src/cmd/adopt.rs` | `dotfix adopt` |
| `src/cmd/sets.rs` | `dotfix sets` |
| `src/cmd/doctor.rs` | `dotfix doctor` |
| `src/cmd/init.rs` | `dotfix init` (both paths) |
| `src/ui.rs` | colours, tables, confirmation prompts, redaction |

### Repository root

| File | Responsibility |
|---|---|
| `Cargo.toml` | workspace manifest |
| `rust-toolchain.toml` | pinned toolchain |
| `install.sh` | bootstrap script (pinned by tag) |
| `.github/workflows/ci.yml` | fmt, clippy, test, commit lint |
| `.github/workflows/release-cli.yml` | tag → tarball → release → tap bump |
| `README.md` | public-facing documentation |

---

## Task 1: Workspace scaffold, error type and CI

**Files:**
- Create: `Cargo.toml`, `rust-toolchain.toml`, `.gitignore`
- Create: `crates/core/Cargo.toml`, `crates/core/src/lib.rs`, `crates/core/src/error.rs`
- Create: `crates/cli/Cargo.toml`, `crates/cli/src/main.rs`
- Create: `.github/workflows/ci.yml`
- Test: `crates/core/src/error.rs` (inline `#[cfg(test)]` module)

**Interfaces:**
- Consumes: nothing.
- Produces: `dotfix_core::Error`, `dotfix_core::Result<T>`. Every later task returns `Result<T>`.

- [ ] **Step 1: Create the workspace manifest**

`Cargo.toml`:

```toml
[workspace]
members = ["crates/core", "crates/cli"]
resolver = "3"

[workspace.package]
edition = "2024"
rust-version = "1.96"
license = "MIT"
repository = "https://github.com/NoiXdev/dotfix"

[workspace.dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.9"
thiserror = "2"
anyhow = "1"
minijinja = "2"
sha2 = "0.10"
clap = { version = "4", features = ["derive"] }
plist = "1"
tempfile = "3"
```

`rust-toolchain.toml`:

```toml
[toolchain]
channel = "1.96"
components = ["rustfmt", "clippy"]
```

`.gitignore`:

```
/target
```

- [ ] **Step 2: Create both crate manifests**

`crates/core/Cargo.toml`:

```toml
[package]
name = "dotfix-core"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true

[features]
fakes = []

[dependencies]
serde.workspace = true
serde_json.workspace = true
toml.workspace = true
thiserror.workspace = true
minijinja.workspace = true
sha2.workspace = true
plist.workspace = true
```

`crates/cli/Cargo.toml`:

```toml
[package]
name = "dotfix"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true

[[bin]]
name = "dotfix"
path = "src/main.rs"

[dependencies]
dotfix-core = { path = "../core" }
anyhow.workspace = true
clap.workspace = true
serde_json.workspace = true

[dev-dependencies]
dotfix-core = { path = "../core", features = ["fakes"] }
tempfile.workspace = true
assert_cmd = "2"
predicates = "3"
```

- [ ] **Step 3: Write the failing test for the error type**

`crates/core/src/error.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_set_message_names_set_and_machine() {
        let err = Error::UnknownSet {
            set: "bravo".into(),
            machine: "alpha".into(),
        };
        assert_eq!(
            err.to_string(),
            "machine `alpha` references unknown set `bravo`"
        );
    }

    #[test]
    fn secret_error_does_not_contain_the_value() {
        let err = Error::Secret {
            name: "api_key".into(),
            reason: "not found in keychain".into(),
        };
        let msg = err.to_string();
        assert!(msg.contains("api_key"));
        assert!(msg.contains("not found in keychain"));
    }
}
```

- [ ] **Step 4: Run the test to verify it fails**

Run: `cargo test -p dotfix-core`
Expected: FAIL — `cannot find type Error in this scope`.

- [ ] **Step 5: Implement the error type**

Prepend to `crates/core/src/error.rs`:

```rust
use std::path::PathBuf;

/// Every fallible operation in `dotfix-core` returns this error.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("invalid TOML in {path}: {source}")]
    Toml {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },

    #[error("invalid JSON in {path}: {source}")]
    Json {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    #[error("machine `{machine}` references unknown set `{set}`")]
    UnknownSet { set: String, machine: String },

    #[error("machine `{0}` not found in repository")]
    UnknownMachine(String),

    #[error("command `{cmd}` failed: {stderr}")]
    Command { cmd: String, stderr: String },

    #[error("template error in {path}: {source}")]
    Template {
        path: PathBuf,
        #[source]
        source: minijinja::Error,
    },

    #[error("secret `{name}` could not be resolved: {reason}")]
    Secret { name: String, reason: String },

    #[error("repository has diverged from origin; resolve it manually")]
    Diverged,

    #[error("{0}")]
    Config(String),
}

pub type Result<T> = std::result::Result<T, Error>;
```

`crates/core/src/lib.rs`:

```rust
//! Core logic for dotfix. Contains no side effects: all interaction with
//! Homebrew, the filesystem and git goes through the traits in [`ports`].

pub mod error;
pub mod ports;

pub use error::{Error, Result};
```

Create `crates/core/src/ports/mod.rs` as an empty placeholder (`// filled in Task 3`) so `lib.rs` compiles.

`crates/cli/src/main.rs`:

```rust
fn main() {
    println!("dotfix");
}
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test --workspace && cargo fmt --check && cargo clippy --all-targets -- -D warnings`
Expected: all PASS.

- [ ] **Step 7: Add CI**

`.github/workflows/ci.yml`:

```yaml
name: CI

on:
  pull_request:
  push:
    branches: [main]

concurrency:
  group: ci-${{ github.ref }}
  cancel-in-progress: true

jobs:
  check:
    runs-on: macos-latest
    steps:
      - uses: actions/checkout@v5
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy
      - uses: Swatinem/rust-cache@v2
      - run: cargo fmt --check
      - run: cargo clippy --all-targets -- -D warnings
      - run: cargo test --workspace

  commitlint:
    runs-on: ubuntu-latest
    if: github.event_name == 'pull_request'
    steps:
      - uses: actions/checkout@v5
        with:
          fetch-depth: 0
      - uses: wagoid/commitlint-github-action@v6
```

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "build: scaffold cargo workspace, error type and CI"
```

---

## Task 2: Configuration model and repository loading

**Files:**
- Create: `crates/core/src/config/mod.rs`, `set.rs`, `machine.rs`, `repo.rs`
- Modify: `crates/core/src/lib.rs`
- Test: inline `#[cfg(test)]` in `set.rs`, `machine.rs`, `repo.rs`

**Interfaces:**
- Consumes: `Error`, `Result` (Task 1).
- Produces:
  - `SetConfig { description: String, packages: Packages, files: Vec<ManagedFile> }`
  - `Packages { brew: Vec<String>, cask: Vec<String> }`
  - `ManagedFile { source: String, target: String, mode: FileMode }`
  - `FileMode::{Template, Symlink, Copy}` (default `Template`)
  - `MachineConfig { sets, secret_provider, vault, vars, secrets, ignore }`
  - `ProviderKind::{Keychain, OnePassword, Age}` (default `Keychain`)
  - `RepoConfig { schema_version: u32 }`
  - `Repo { root, config, sets, machines }` with `Repo::parse(root, files) -> Result<Repo>`

Note: `Repo::load` (which reads from an `Fsys`) arrives in Task 3 once the trait
exists. This task parses from an already-collected map so it can be tested with
no filesystem at all.

- [ ] **Step 1: Write the failing tests for the set model**

`crates/core/src/config/set.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_full_set() {
        let toml = r#"
description = "Example set"

[packages]
brew = ["alpha", "bravo"]
cask = ["charlie"]

[[files]]
source = "files/example.tmpl"
target = "~/.example"
mode   = "template"
"#;
        let set: SetConfig = toml::from_str(toml).unwrap();
        assert_eq!(set.description, "Example set");
        assert_eq!(set.packages.brew, vec!["alpha", "bravo"]);
        assert_eq!(set.packages.cask, vec!["charlie"]);
        assert_eq!(set.files.len(), 1);
        assert_eq!(set.files[0].mode, FileMode::Template);
    }

    #[test]
    fn every_section_is_optional() {
        let set: SetConfig = toml::from_str("").unwrap();
        assert!(set.packages.brew.is_empty());
        assert!(set.files.is_empty());
    }

    #[test]
    fn file_mode_defaults_to_template() {
        let toml = r#"
[[files]]
source = "a"
target = "~/a"
"#;
        let set: SetConfig = toml::from_str(toml).unwrap();
        assert_eq!(set.files[0].mode, FileMode::Template);
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p dotfix-core config::set`
Expected: FAIL — `cannot find type SetConfig`.

- [ ] **Step 3: Implement the set model**

Prepend to `crates/core/src/config/set.rs`:

```rust
use serde::{Deserialize, Serialize};

/// One set: a named group of packages, shell fragments and managed files.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct SetConfig {
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub packages: Packages,
    #[serde(default)]
    pub files: Vec<ManagedFile>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct Packages {
    #[serde(default)]
    pub brew: Vec<String>,
    #[serde(default)]
    pub cask: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct ManagedFile {
    /// Path relative to the set directory.
    pub source: String,
    /// Destination on the machine; a leading `~` is expanded at resolve time.
    pub target: String,
    #[serde(default)]
    pub mode: FileMode,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FileMode {
    /// Render through the template engine before writing.
    #[default]
    Template,
    /// Symlink the repository file into place.
    Symlink,
    /// Copy verbatim.
    Copy,
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p dotfix-core config::set`
Expected: 3 PASS.

- [ ] **Step 5: Write the failing tests for the machine model**

`crates/core/src/config/machine.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_keychain_machine() {
        let toml = r#"
sets = ["core", "web"]
secret_provider = "keychain"

[vars]
git_email = "someone@example.com"
"#;
        let m: MachineConfig = toml::from_str(toml).unwrap();
        assert_eq!(m.sets, vec!["core", "web"]);
        assert_eq!(m.secret_provider, ProviderKind::Keychain);
        assert_eq!(m.vault, None);
        assert_eq!(m.vars["git_email"], "someone@example.com");
    }

    #[test]
    fn parses_a_onepassword_machine_with_vault_and_overrides() {
        let toml = r#"
sets = ["core"]
secret_provider = "1password"
vault = "Example"

[secrets]
api_key = "op://Example/service/api_key"
"#;
        let m: MachineConfig = toml::from_str(toml).unwrap();
        assert_eq!(m.secret_provider, ProviderKind::OnePassword);
        assert_eq!(m.vault.as_deref(), Some("Example"));
        assert_eq!(m.secrets["api_key"], "op://Example/service/api_key");
    }

    #[test]
    fn provider_defaults_to_keychain() {
        let m: MachineConfig = toml::from_str("sets = []").unwrap();
        assert_eq!(m.secret_provider, ProviderKind::Keychain);
        assert!(m.ignore.is_empty());
    }
}
```

- [ ] **Step 6: Run the tests to verify they fail**

Run: `cargo test -p dotfix-core config::machine`
Expected: FAIL — `cannot find type MachineConfig`.

- [ ] **Step 7: Implement the machine model**

Prepend to `crates/core/src/config/machine.rs`:

```rust
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Per-machine configuration: which sets are active and where secrets live.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct MachineConfig {
    #[serde(default)]
    pub sets: Vec<String>,
    #[serde(default)]
    pub secret_provider: ProviderKind,
    /// 1Password vault. Ignored by the other providers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vault: Option<String>,
    /// Template variables available as `{{ name }}`.
    #[serde(default)]
    pub vars: BTreeMap<String, String>,
    /// Overrides for where a named secret lives. Without an entry the provider
    /// derives a default location from the name.
    #[serde(default)]
    pub secrets: BTreeMap<String, String>,
    /// Packages that must never be reported as unmanaged on this machine.
    #[serde(default)]
    pub ignore: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    /// macOS Keychain — the default, needs no extra software.
    #[default]
    Keychain,
    #[serde(rename = "1password")]
    OnePassword,
    Age,
}
```

- [ ] **Step 8: Run the tests to verify they pass**

Run: `cargo test -p dotfix-core config::machine`
Expected: 3 PASS.

- [ ] **Step 9: Write the failing tests for repository parsing**

`crates/core/src/config/repo.rs`:

```rust
#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use super::*;

    fn files() -> BTreeMap<PathBuf, String> {
        BTreeMap::from([
            (
                PathBuf::from("/repo/dotfix.toml"),
                "schema_version = 1\n".to_string(),
            ),
            (
                PathBuf::from("/repo/sets/core/set.toml"),
                "[packages]\nbrew = [\"alpha\"]\n".to_string(),
            ),
            (
                PathBuf::from("/repo/machines/box-one.toml"),
                "sets = [\"core\"]\n".to_string(),
            ),
        ])
    }

    #[test]
    fn parses_sets_and_machines() {
        let repo = Repo::parse(PathBuf::from("/repo"), &files()).unwrap();
        assert_eq!(repo.config.schema_version, 1);
        assert_eq!(repo.sets.len(), 1);
        assert_eq!(repo.sets["core"].packages.brew, vec!["alpha"]);
        assert_eq!(repo.machines["box-one"].sets, vec!["core"]);
    }

    #[test]
    fn rejects_a_machine_referencing_an_unknown_set() {
        let mut f = files();
        f.insert(
            PathBuf::from("/repo/machines/box-two.toml"),
            "sets = [\"nope\"]\n".to_string(),
        );
        let err = Repo::parse(PathBuf::from("/repo"), &f).unwrap_err();
        assert!(matches!(
            err,
            Error::UnknownSet { ref set, ref machine } if set == "nope" && machine == "box-two"
        ));
    }

    #[test]
    fn reports_the_path_of_invalid_toml() {
        let mut f = files();
        f.insert(
            PathBuf::from("/repo/sets/core/set.toml"),
            "this is not toml =".to_string(),
        );
        let err = Repo::parse(PathBuf::from("/repo"), &f).unwrap_err();
        assert!(err.to_string().contains("/repo/sets/core/set.toml"));
    }
}
```

- [ ] **Step 10: Run the tests to verify they fail**

Run: `cargo test -p dotfix-core config::repo`
Expected: FAIL — `cannot find type Repo`.

- [ ] **Step 11: Implement repository parsing**

Prepend to `crates/core/src/config/repo.rs`:

```rust
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::{MachineConfig, SetConfig};
use crate::error::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct RepoConfig {
    pub schema_version: u32,
}

/// The parsed data repository.
#[derive(Debug, Clone)]
pub struct Repo {
    pub root: PathBuf,
    pub config: RepoConfig,
    pub sets: BTreeMap<String, SetConfig>,
    pub machines: BTreeMap<String, MachineConfig>,
}

impl Repo {
    /// Parse a repository from an already-collected map of path → contents.
    /// Keeping this separate from I/O makes the whole loader testable without
    /// a filesystem.
    pub fn parse(root: PathBuf, files: &BTreeMap<PathBuf, String>) -> Result<Repo> {
        let cfg_path = root.join("dotfix.toml");
        let cfg_raw = files
            .get(&cfg_path)
            .ok_or_else(|| Error::Config(format!("missing {}", cfg_path.display())))?;
        let config: RepoConfig = parse_toml(&cfg_path, cfg_raw)?;

        let mut sets = BTreeMap::new();
        let mut machines = BTreeMap::new();

        for (path, raw) in files {
            if let Some(name) = set_name(&root, path) {
                sets.insert(name, parse_toml(path, raw)?);
            } else if let Some(name) = machine_name(&root, path) {
                machines.insert(name, parse_toml(path, raw)?);
            }
        }

        for (machine, cfg) in &machines {
            for set in &cfg.sets {
                if !sets.contains_key(set) {
                    return Err(Error::UnknownSet {
                        set: set.clone(),
                        machine: machine.clone(),
                    });
                }
            }
        }

        Ok(Repo {
            root,
            config,
            sets,
            machines,
        })
    }
}

fn parse_toml<T: serde::de::DeserializeOwned>(path: &Path, raw: &str) -> Result<T> {
    toml::from_str(raw).map_err(|source| Error::Toml {
        path: path.to_path_buf(),
        source,
    })
}

/// `<root>/sets/<name>/set.toml` → `Some(name)`
fn set_name(root: &Path, path: &Path) -> Option<String> {
    let rest = path.strip_prefix(root.join("sets")).ok()?;
    let mut parts = rest.components();
    let name = parts.next()?.as_os_str().to_str()?.to_string();
    (parts.next()?.as_os_str() == "set.toml" && parts.next().is_none()).then_some(name)
}

/// `<root>/machines/<name>.toml` → `Some(name)`
fn machine_name(root: &Path, path: &Path) -> Option<String> {
    let rest = path.strip_prefix(root.join("machines")).ok()?;
    (rest.extension()?.to_str()? == "toml")
        .then(|| rest.file_stem()?.to_str().map(str::to_string))
        .flatten()
}
```

`crates/core/src/config/mod.rs`:

```rust
mod machine;
mod repo;
mod set;

pub use machine::{MachineConfig, ProviderKind};
pub use repo::{Repo, RepoConfig};
pub use set::{FileMode, ManagedFile, Packages, SetConfig};
```

Add `pub mod config;` to `crates/core/src/lib.rs`.

- [ ] **Step 12: Run the full suite**

Run: `cargo test --workspace && cargo clippy --all-targets -- -D warnings`
Expected: all PASS.

- [ ] **Step 13: Commit**

```bash
git add -A
git commit -m "feat(core): add set, machine and repository configuration model"
```

---

## Task 3: Ports — traits, real implementations and fakes

**Files:**
- Create: `crates/core/src/ports/fsys.rs`, `brew.rs`, `git.rs`, `fake.rs`
- Modify: `crates/core/src/ports/mod.rs`, `crates/core/src/config/repo.rs` (add `Repo::load`)
- Test: inline `#[cfg(test)]` in `fake.rs` and `repo.rs`

**Interfaces:**
- Consumes: `Error`, `Result` (Task 1); `Repo::parse` (Task 2).
- Produces:
  - `trait Fsys` — `read`, `write`, `exists`, `list_dir`, `remove`, `create_dir_all`, `symlink`, `read_link`
  - `trait Brew` — `leaves`, `casks`, `uses_installed`, `install`, `uninstall`
  - `trait Git` — `fetch`, `pull_ff_only`, `is_diverged`, `commit_all`, `push`, `clone_to`, `log`
  - `RealFsys`, `RealBrew`, `RealGit`
  - `FakeFsys`, `FakeBrew`, `FakeGit` (cfg `test` or feature `fakes`)
  - `Repo::load(fs: &dyn Fsys, root: &Path) -> Result<Repo>`

- [ ] **Step 1: Define the three traits**

`crates/core/src/ports/fsys.rs`:

```rust
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

/// Every filesystem access in `dotfix-core` goes through this trait so that
/// tests can run entirely in memory.
pub trait Fsys {
    fn read(&self, path: &Path) -> Result<String>;
    /// `mode` is a Unix permission bitmask, e.g. `0o600`.
    fn write(&self, path: &Path, contents: &str, mode: u32) -> Result<()>;
    fn exists(&self, path: &Path) -> bool;
    /// Recursive listing of files (not directories) below `path`.
    fn list_dir(&self, path: &Path) -> Result<Vec<PathBuf>>;
    fn remove(&self, path: &Path) -> Result<()>;
    fn create_dir_all(&self, path: &Path) -> Result<()>;
    fn symlink(&self, source: &Path, target: &Path) -> Result<()>;
    fn read_link(&self, path: &Path) -> Result<PathBuf>;
}

pub struct RealFsys;

impl Fsys for RealFsys {
    fn read(&self, path: &Path) -> Result<String> {
        std::fs::read_to_string(path).map_err(|source| Error::Io {
            path: path.to_path_buf(),
            source,
        })
    }

    fn write(&self, path: &Path, contents: &str, mode: u32) -> Result<()> {
        use std::os::unix::fs::PermissionsExt;

        if let Some(parent) = path.parent() {
            self.create_dir_all(parent)?;
        }
        std::fs::write(path, contents).map_err(|source| Error::Io {
            path: path.to_path_buf(),
            source,
        })?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).map_err(|source| {
            Error::Io {
                path: path.to_path_buf(),
                source,
            }
        })
    }

    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn list_dir(&self, path: &Path) -> Result<Vec<PathBuf>> {
        let mut out = Vec::new();
        let mut stack = vec![path.to_path_buf()];
        while let Some(dir) = stack.pop() {
            let entries = match std::fs::read_dir(&dir) {
                Ok(entries) => entries,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(source) => return Err(Error::Io { path: dir, source }),
            };
            for entry in entries {
                let entry = entry.map_err(|source| Error::Io {
                    path: dir.clone(),
                    source,
                })?;
                let p = entry.path();
                if p.is_dir() {
                    stack.push(p);
                } else {
                    out.push(p);
                }
            }
        }
        out.sort();
        Ok(out)
    }

    fn remove(&self, path: &Path) -> Result<()> {
        std::fs::remove_file(path).map_err(|source| Error::Io {
            path: path.to_path_buf(),
            source,
        })
    }

    fn create_dir_all(&self, path: &Path) -> Result<()> {
        std::fs::create_dir_all(path).map_err(|source| Error::Io {
            path: path.to_path_buf(),
            source,
        })
    }

    fn symlink(&self, source: &Path, target: &Path) -> Result<()> {
        if let Some(parent) = target.parent() {
            self.create_dir_all(parent)?;
        }
        if target.is_symlink() {
            self.remove(target)?;
        }
        std::os::unix::fs::symlink(source, target).map_err(|source| Error::Io {
            path: target.to_path_buf(),
            source,
        })
    }

    fn read_link(&self, path: &Path) -> Result<PathBuf> {
        std::fs::read_link(path).map_err(|source| Error::Io {
            path: path.to_path_buf(),
            source,
        })
    }
}
```

`crates/core/src/ports/brew.rs`:

```rust
use std::process::Command;

use crate::error::{Error, Result};

pub trait Brew {
    /// Top-level formulae — packages not pulled in as a dependency.
    fn leaves(&self) -> Result<Vec<String>>;
    fn casks(&self) -> Result<Vec<String>>;
    /// Installed packages that depend on `formula`. Empty means it is a leaf
    /// and safe to uninstall.
    fn uses_installed(&self, formula: &str) -> Result<Vec<String>>;
    fn install(&self, name: &str, cask: bool) -> Result<()>;
    fn uninstall(&self, name: &str, cask: bool) -> Result<()>;
}

pub struct RealBrew;

impl RealBrew {
    fn run(args: &[&str]) -> Result<String> {
        let output = Command::new("brew")
            .args(args)
            .output()
            .map_err(|e| Error::Command {
                cmd: format!("brew {}", args.join(" ")),
                stderr: e.to_string(),
            })?;
        if !output.status.success() {
            return Err(Error::Command {
                cmd: format!("brew {}", args.join(" ")),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            });
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    fn lines(raw: String) -> Vec<String> {
        raw.lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect()
    }
}

impl Brew for RealBrew {
    fn leaves(&self) -> Result<Vec<String>> {
        Ok(Self::lines(Self::run(&["leaves"])?))
    }

    fn casks(&self) -> Result<Vec<String>> {
        Ok(Self::lines(Self::run(&["list", "--cask"])?))
    }

    fn uses_installed(&self, formula: &str) -> Result<Vec<String>> {
        Ok(Self::lines(Self::run(&["uses", "--installed", formula])?))
    }

    fn install(&self, name: &str, cask: bool) -> Result<()> {
        let args = if cask {
            vec!["install", "--cask", name]
        } else {
            vec!["install", name]
        };
        Self::run(&args).map(|_| ())
    }

    fn uninstall(&self, name: &str, cask: bool) -> Result<()> {
        let args = if cask {
            vec!["uninstall", "--cask", name]
        } else {
            vec!["uninstall", name]
        };
        Self::run(&args).map(|_| ())
    }
}
```

`crates/core/src/ports/git.rs`:

```rust
use std::path::Path;
use std::process::Command;

use crate::error::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Commit {
    pub hash: String,
    pub subject: String,
    pub date: String,
}

pub trait Git {
    fn fetch(&self, root: &Path) -> Result<()>;
    /// Fast-forward only. Returns [`Error::Diverged`] when a merge would be
    /// required — dotfix never merges on the user's behalf.
    fn pull_ff_only(&self, root: &Path) -> Result<()>;
    fn is_diverged(&self, root: &Path) -> Result<bool>;
    fn commit_all(&self, root: &Path, message: &str) -> Result<()>;
    fn push(&self, root: &Path) -> Result<()>;
    fn clone_to(&self, url: &str, root: &Path) -> Result<()>;
    fn log(&self, root: &Path, limit: usize) -> Result<Vec<Commit>>;
}

pub struct RealGit;

impl RealGit {
    fn run(root: &Path, args: &[&str]) -> Result<String> {
        let output = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .map_err(|e| Error::Command {
                cmd: format!("git {}", args.join(" ")),
                stderr: e.to_string(),
            })?;
        if !output.status.success() {
            return Err(Error::Command {
                cmd: format!("git {}", args.join(" ")),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            });
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}

impl Git for RealGit {
    fn fetch(&self, root: &Path) -> Result<()> {
        Self::run(root, &["fetch", "--quiet"]).map(|_| ())
    }

    fn pull_ff_only(&self, root: &Path) -> Result<()> {
        if self.is_diverged(root)? {
            return Err(Error::Diverged);
        }
        Self::run(root, &["pull", "--ff-only", "--quiet"]).map(|_| ())
    }

    fn is_diverged(&self, root: &Path) -> Result<bool> {
        // "<behind> <ahead>" relative to the upstream branch.
        let raw = Self::run(root, &["rev-list", "--left-right", "--count", "@{u}...HEAD"])?;
        let mut parts = raw.split_whitespace();
        let behind: u32 = parts.next().unwrap_or("0").parse().unwrap_or(0);
        let ahead: u32 = parts.next().unwrap_or("0").parse().unwrap_or(0);
        Ok(behind > 0 && ahead > 0)
    }

    fn commit_all(&self, root: &Path, message: &str) -> Result<()> {
        Self::run(root, &["add", "-A"])?;
        Self::run(root, &["commit", "-m", message]).map(|_| ())
    }

    fn push(&self, root: &Path) -> Result<()> {
        Self::run(root, &["push", "--quiet"]).map(|_| ())
    }

    fn clone_to(&self, url: &str, root: &Path) -> Result<()> {
        let parent = root.parent().unwrap_or(Path::new("."));
        let name = root
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("dotfiles");
        Self::run(parent, &["clone", "--quiet", url, name]).map(|_| ())
    }

    fn log(&self, root: &Path, limit: usize) -> Result<Vec<Commit>> {
        let n = format!("-{limit}");
        let raw = Self::run(root, &["log", &n, "--pretty=format:%h%x1f%s%x1f%ad", "--date=short"])?;
        Ok(raw
            .lines()
            .filter_map(|line| {
                let mut f = line.split('\u{1f}');
                Some(Commit {
                    hash: f.next()?.to_string(),
                    subject: f.next()?.to_string(),
                    date: f.next()?.to_string(),
                })
            })
            .collect())
    }
}
```

- [ ] **Step 2: Write the failing tests for the fakes**

`crates/core/src/ports/fake.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_fsys_round_trips_a_write() {
        let fs = FakeFsys::new();
        let path = Path::new("/repo/a.txt");
        assert!(!fs.exists(path));
        fs.write(path, "hello", 0o644).unwrap();
        assert!(fs.exists(path));
        assert_eq!(fs.read(path).unwrap(), "hello");
        assert_eq!(fs.mode_of(path), Some(0o644));
    }

    #[test]
    fn fake_fsys_lists_recursively_and_sorted() {
        let fs = FakeFsys::from([
            ("/repo/sets/core/set.toml", "a"),
            ("/repo/sets/web/set.toml", "b"),
            ("/repo/dotfix.toml", "c"),
        ]);
        let found = fs.list_dir(Path::new("/repo/sets")).unwrap();
        assert_eq!(
            found,
            vec![
                PathBuf::from("/repo/sets/core/set.toml"),
                PathBuf::from("/repo/sets/web/set.toml"),
            ]
        );
    }

    #[test]
    fn fake_brew_records_installs() {
        let brew = FakeBrew::new(["alpha"], ["charlie"]);
        brew.install("bravo", false).unwrap();
        assert_eq!(brew.installed(), vec!["bravo".to_string()]);
        assert_eq!(brew.leaves().unwrap(), vec!["alpha".to_string()]);
    }

    #[test]
    fn fake_brew_reports_configured_dependents() {
        let brew = FakeBrew::new(["alpha"], []).with_uses("alpha", ["bravo"]);
        assert_eq!(brew.uses_installed("alpha").unwrap(), vec!["bravo"]);
        assert!(brew.uses_installed("charlie").unwrap().is_empty());
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p dotfix-core ports::fake`
Expected: FAIL — `cannot find type FakeFsys`.

- [ ] **Step 4: Implement the fakes**

Prepend to `crates/core/src/ports/fake.rs`:

```rust
//! In-memory test doubles. Available to downstream crates via the `fakes`
//! feature so that CLI integration tests can run without Homebrew or git.
#![cfg(any(test, feature = "fakes"))]

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::ports::{Brew, Commit, Fsys, Git};

#[derive(Default)]
pub struct FakeFsys {
    files: RefCell<BTreeMap<PathBuf, String>>,
    modes: RefCell<BTreeMap<PathBuf, u32>>,
    links: RefCell<BTreeMap<PathBuf, PathBuf>>,
}

impl FakeFsys {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from<const N: usize>(entries: [(&str, &str); N]) -> Self {
        let fs = Self::new();
        for (path, contents) in entries {
            fs.write(Path::new(path), contents, 0o644).unwrap();
        }
        fs
    }

    pub fn mode_of(&self, path: &Path) -> Option<u32> {
        self.modes.borrow().get(path).copied()
    }

    /// Snapshot of every file, for assertions.
    pub fn snapshot(&self) -> BTreeMap<PathBuf, String> {
        self.files.borrow().clone()
    }
}

impl Fsys for FakeFsys {
    fn read(&self, path: &Path) -> Result<String> {
        self.files
            .borrow()
            .get(path)
            .cloned()
            .ok_or_else(|| Error::Io {
                path: path.to_path_buf(),
                source: std::io::Error::from(std::io::ErrorKind::NotFound),
            })
    }

    fn write(&self, path: &Path, contents: &str, mode: u32) -> Result<()> {
        self.files
            .borrow_mut()
            .insert(path.to_path_buf(), contents.to_string());
        self.modes.borrow_mut().insert(path.to_path_buf(), mode);
        Ok(())
    }

    fn exists(&self, path: &Path) -> bool {
        self.files.borrow().contains_key(path) || self.links.borrow().contains_key(path)
    }

    fn list_dir(&self, path: &Path) -> Result<Vec<PathBuf>> {
        Ok(self
            .files
            .borrow()
            .keys()
            .filter(|p| p.starts_with(path))
            .cloned()
            .collect())
    }

    fn remove(&self, path: &Path) -> Result<()> {
        self.files.borrow_mut().remove(path);
        self.links.borrow_mut().remove(path);
        Ok(())
    }

    fn create_dir_all(&self, _path: &Path) -> Result<()> {
        Ok(())
    }

    fn symlink(&self, source: &Path, target: &Path) -> Result<()> {
        self.links
            .borrow_mut()
            .insert(target.to_path_buf(), source.to_path_buf());
        Ok(())
    }

    fn read_link(&self, path: &Path) -> Result<PathBuf> {
        self.links
            .borrow()
            .get(path)
            .cloned()
            .ok_or_else(|| Error::Io {
                path: path.to_path_buf(),
                source: std::io::Error::from(std::io::ErrorKind::NotFound),
            })
    }
}

#[derive(Default)]
pub struct FakeBrew {
    leaves: Vec<String>,
    casks: Vec<String>,
    uses: BTreeMap<String, Vec<String>>,
    installed: RefCell<Vec<String>>,
    uninstalled: RefCell<Vec<String>>,
}

impl FakeBrew {
    pub fn new<const A: usize, const B: usize>(
        leaves: [&str; A],
        casks: [&str; B],
    ) -> Self {
        Self {
            leaves: leaves.iter().map(|s| s.to_string()).collect(),
            casks: casks.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        }
    }

    pub fn with_uses<const N: usize>(mut self, formula: &str, users: [&str; N]) -> Self {
        self.uses.insert(
            formula.to_string(),
            users.iter().map(|s| s.to_string()).collect(),
        );
        self
    }

    pub fn installed(&self) -> Vec<String> {
        self.installed.borrow().clone()
    }

    pub fn uninstalled(&self) -> Vec<String> {
        self.uninstalled.borrow().clone()
    }
}

impl Brew for FakeBrew {
    fn leaves(&self) -> Result<Vec<String>> {
        Ok(self.leaves.clone())
    }

    fn casks(&self) -> Result<Vec<String>> {
        Ok(self.casks.clone())
    }

    fn uses_installed(&self, formula: &str) -> Result<Vec<String>> {
        Ok(self.uses.get(formula).cloned().unwrap_or_default())
    }

    fn install(&self, name: &str, _cask: bool) -> Result<()> {
        self.installed.borrow_mut().push(name.to_string());
        Ok(())
    }

    fn uninstall(&self, name: &str, _cask: bool) -> Result<()> {
        self.uninstalled.borrow_mut().push(name.to_string());
        Ok(())
    }
}

#[derive(Default)]
pub struct FakeGit {
    pub diverged: bool,
    pub commits: Vec<Commit>,
    pub calls: RefCell<Vec<String>>,
}

impl FakeGit {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn diverged() -> Self {
        Self {
            diverged: true,
            ..Default::default()
        }
    }

    pub fn calls(&self) -> Vec<String> {
        self.calls.borrow().clone()
    }

    fn record(&self, what: &str) {
        self.calls.borrow_mut().push(what.to_string());
    }
}

impl Git for FakeGit {
    fn fetch(&self, _root: &Path) -> Result<()> {
        self.record("fetch");
        Ok(())
    }

    fn pull_ff_only(&self, _root: &Path) -> Result<()> {
        self.record("pull");
        if self.diverged {
            return Err(Error::Diverged);
        }
        Ok(())
    }

    fn is_diverged(&self, _root: &Path) -> Result<bool> {
        Ok(self.diverged)
    }

    fn commit_all(&self, _root: &Path, message: &str) -> Result<()> {
        self.record(&format!("commit:{message}"));
        Ok(())
    }

    fn push(&self, _root: &Path) -> Result<()> {
        self.record("push");
        Ok(())
    }

    fn clone_to(&self, url: &str, _root: &Path) -> Result<()> {
        self.record(&format!("clone:{url}"));
        Ok(())
    }

    fn log(&self, _root: &Path, limit: usize) -> Result<Vec<Commit>> {
        Ok(self.commits.iter().take(limit).cloned().collect())
    }
}
```

`crates/core/src/ports/mod.rs`:

```rust
mod brew;
pub mod fake;
mod fsys;
mod git;

pub use brew::{Brew, RealBrew};
pub use fsys::{Fsys, RealFsys};
pub use git::{Commit, Git, RealGit};
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p dotfix-core ports::fake`
Expected: 4 PASS.

- [ ] **Step 6: Write the failing test for `Repo::load`**

Append to the `tests` module in `crates/core/src/config/repo.rs`:

```rust
    #[test]
    fn loads_a_repository_from_a_filesystem() {
        use crate::ports::fake::FakeFsys;

        let fs = FakeFsys::from([
            ("/repo/dotfix.toml", "schema_version = 1\n"),
            ("/repo/sets/core/set.toml", "[packages]\nbrew = [\"alpha\"]\n"),
            ("/repo/sets/core/shell/10-path.zsh", "export A=1\n"),
            ("/repo/machines/box-one.toml", "sets = [\"core\"]\n"),
        ]);

        let repo = Repo::load(&fs, Path::new("/repo")).unwrap();
        assert_eq!(repo.sets.len(), 1);
        assert_eq!(repo.machines.len(), 1);
    }
```

- [ ] **Step 7: Run the test to verify it fails**

Run: `cargo test -p dotfix-core config::repo::tests::loads_a_repository`
Expected: FAIL — `no function or associated item named load`.

- [ ] **Step 8: Implement `Repo::load`**

Add to the `impl Repo` block in `crates/core/src/config/repo.rs`:

```rust
    /// Read every `.toml` under `root` and parse it.
    pub fn load(fs: &dyn crate::ports::Fsys, root: &Path) -> Result<Repo> {
        let mut files = BTreeMap::new();
        for path in fs.list_dir(root)? {
            if path.extension().and_then(|e| e.to_str()) == Some("toml") {
                let contents = fs.read(&path)?;
                files.insert(path, contents);
            }
        }
        Repo::parse(root.to_path_buf(), &files)
    }
```

- [ ] **Step 9: Run the full suite**

Run: `cargo test --workspace && cargo clippy --all-targets -- -D warnings`
Expected: all PASS.

- [ ] **Step 10: Commit**

```bash
git add -A
git commit -m "feat(core): add brew, filesystem and git ports with in-memory fakes"
```

---

## Task 4: Resolving a machine to its desired state

**Files:**
- Modify: `crates/core/src/config/repo.rs`
- Create: `crates/core/src/config/desired.rs`
- Test: inline `#[cfg(test)]` in `desired.rs`

**Interfaces:**
- Consumes: `Repo` (Task 2), `Fsys` + `FakeFsys` (Task 3).
- Produces:
  - `Desired { machine: String, brew: BTreeSet<String>, cask: BTreeSet<String>, files: Vec<ResolvedFile>, fragments: Vec<Fragment> }`
  - `ResolvedFile { set: String, source: PathBuf, target: PathBuf, mode: FileMode }`
  - `Fragment { set: String, name: String, path: PathBuf }`
  - `Repo::resolve(&self, fs: &dyn Fsys, machine: &str, home: &Path) -> Result<Desired>`

- [ ] **Step 1: Write the failing tests**

`crates/core/src/config/desired.rs`:

```rust
#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;
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
            ("/repo/sets/idle/set.toml", "[packages]\nbrew = [\"unused\"]\n"),
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
        let d = repo.resolve(&fs, "box-one", Path::new("/Users/test")).unwrap();

        assert!(d.brew.contains("alpha"));
        assert!(d.brew.contains("bravo"));
        assert!(!d.brew.contains("unused"), "inactive set must not contribute");
        assert!(d.cask.contains("charlie"));
    }

    #[test]
    fn orders_fragments_by_numeric_prefix_then_set_order() {
        let fs = repo_fs();
        let repo = Repo::load(&fs, Path::new("/repo")).unwrap();
        let d = repo.resolve(&fs, "box-one", Path::new("/Users/test")).unwrap();

        let names: Vec<&str> = d.fragments.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["00-first.zsh", "10-web.zsh", "20-alias.zsh"]);
    }

    #[test]
    fn expands_tilde_in_file_targets() {
        let fs = repo_fs();
        let repo = Repo::load(&fs, Path::new("/repo")).unwrap();
        let d = repo.resolve(&fs, "box-one", Path::new("/Users/test")).unwrap();

        assert_eq!(d.files.len(), 1);
        assert_eq!(d.files[0].target, PathBuf::from("/Users/test/.rc"));
        assert_eq!(d.files[0].source, PathBuf::from("/repo/sets/core/files/rc.tmpl"));
        assert_eq!(d.files[0].set, "core");
    }

    #[test]
    fn rejects_an_unknown_machine() {
        let fs = repo_fs();
        let repo = Repo::load(&fs, Path::new("/repo")).unwrap();
        let err = repo.resolve(&fs, "ghost", Path::new("/Users/test")).unwrap_err();
        assert!(matches!(err, crate::Error::UnknownMachine(ref m) if m == "ghost"));
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p dotfix-core config::desired`
Expected: FAIL — `cannot find type Desired`.

- [ ] **Step 3: Implement the resolver**

Prepend to `crates/core/src/config/desired.rs`:

```rust
use std::collections::BTreeSet;
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
```

Add to `impl Repo` in `crates/core/src/config/repo.rs`:

```rust
    /// Merge the machine's active sets into a single [`Desired`] state.
    pub fn resolve(
        &self,
        fs: &dyn crate::ports::Fsys,
        machine: &str,
        home: &Path,
    ) -> Result<Desired> {
        let cfg = self
            .machines
            .get(machine)
            .ok_or_else(|| Error::UnknownMachine(machine.to_string()))?;

        let mut desired = Desired {
            machine: machine.to_string(),
            ..Default::default()
        };
        // Rank by position in the machine's `sets` list: author-controlled and
        // deterministic, used to break ties between equal numeric prefixes.
        let mut fragments: Vec<(u32, usize, String, Fragment)> = Vec::new();

        for (rank, set_name) in cfg.sets.iter().enumerate() {
            let set = &self.sets[set_name];
            desired.brew.extend(set.packages.brew.iter().cloned());
            desired.cask.extend(set.packages.cask.iter().cloned());

            let set_dir = self.root.join("sets").join(set_name);

            for file in &set.files {
                desired.files.push(ResolvedFile {
                    set: set_name.clone(),
                    source: set_dir.join(&file.source),
                    target: expand_home(&file.target, home),
                    mode: file.mode,
                });
            }

            for path in fs.list_dir(&set_dir.join("shell"))? {
                if path.extension().and_then(|e| e.to_str()) != Some("zsh") {
                    continue;
                }
                let name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or_default()
                    .to_string();
                fragments.push((
                    numeric_prefix(&name),
                    rank,
                    name.clone(),
                    Fragment {
                        set: set_name.clone(),
                        name,
                        path,
                    },
                ));
            }
        }

        fragments.sort_by(|a, b| (a.0, a.1, &a.2).cmp(&(b.0, b.1, &b.2)));
        desired.fragments = fragments.into_iter().map(|(_, _, _, f)| f).collect();

        Ok(desired)
    }
```

And the two helpers at the bottom of `repo.rs`:

```rust
/// `"20-alias.zsh"` → `20`. Fragments without a numeric prefix sort last.
fn numeric_prefix(name: &str) -> u32 {
    name.split('-')
        .next()
        .and_then(|p| p.parse().ok())
        .unwrap_or(u32::MAX)
}

fn expand_home(target: &str, home: &Path) -> PathBuf {
    match target.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None => PathBuf::from(target),
    }
}
```

Extend `crates/core/src/config/mod.rs`:

```rust
mod desired;
pub use desired::{Desired, Fragment, ResolvedFile};
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p dotfix-core config::desired`
Expected: 4 PASS.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(core): resolve a machine's active sets into a desired state"
```

---

## Task 5: Local paths, local config and applied state

**Files:**
- Create: `crates/core/src/paths.rs`, `crates/core/src/state.rs`
- Modify: `crates/core/src/lib.rs`
- Test: inline `#[cfg(test)]` in both files

**Interfaces:**
- Consumes: `Fsys` (Task 3), `ProviderKind` (Task 2).
- Produces:
  - `Paths { home, state_dir, config_dir }` with `Paths::new(home)`, `applied()`, `status_line()`, `local_config()`, `backups(stamp)`, `launch_agent()`
  - `LocalConfig { repo: PathBuf, machine: String }` with `load(fs, path)` / `save(fs, path)`
  - `Applied { brew: BTreeSet<String>, cask: BTreeSet<String>, files: BTreeMap<PathBuf, String> }` with `load(fs, path)` / `save(fs, path)`

Rationale: `LocalConfig` is deliberately **not** in the data repository — which
repository this machine uses and under which machine name is local knowledge and
must exist before the repository is readable.

- [ ] **Step 1: Write the failing tests for `Paths`**

`crates/core/src/paths.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_every_location_from_home() {
        let p = Paths::new(PathBuf::from("/Users/test"));
        assert_eq!(
            p.applied(),
            PathBuf::from("/Users/test/.local/state/dotfix/applied.json")
        );
        assert_eq!(
            p.status_line(),
            PathBuf::from("/Users/test/.local/state/dotfix/status.line")
        );
        assert_eq!(
            p.local_config(),
            PathBuf::from("/Users/test/.config/dotfix/config.toml")
        );
        assert_eq!(
            p.backups("20260916-101500"),
            PathBuf::from("/Users/test/.local/state/dotfix/backups/20260916-101500")
        );
        assert_eq!(
            p.launch_agent(),
            PathBuf::from("/Users/test/Library/LaunchAgents/dev.noix.dotfix.plist")
        );
    }

    #[test]
    fn local_config_round_trips() {
        use crate::ports::fake::FakeFsys;

        let fs = FakeFsys::new();
        let cfg = LocalConfig {
            repo: PathBuf::from("/Users/test/dotfiles"),
            machine: "box-one".into(),
        };
        let path = PathBuf::from("/Users/test/.config/dotfix/config.toml");
        cfg.save(&fs, &path).unwrap();
        assert_eq!(LocalConfig::load(&fs, &path).unwrap(), cfg);
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p dotfix-core paths`
Expected: FAIL — `cannot find type Paths`.

- [ ] **Step 3: Implement `Paths` and `LocalConfig`**

Prepend to `crates/core/src/paths.rs`:

```rust
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::ports::Fsys;

/// All machine-local locations dotfix uses. Derived from `$HOME` so tests can
/// point it at a fake root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    pub home: PathBuf,
}

impl Paths {
    pub fn new(home: PathBuf) -> Self {
        Self { home }
    }

    fn state_dir(&self) -> PathBuf {
        self.home.join(".local/state/dotfix")
    }

    pub fn applied(&self) -> PathBuf {
        self.state_dir().join("applied.json")
    }

    pub fn status_line(&self) -> PathBuf {
        self.state_dir().join("status.line")
    }

    pub fn backups(&self, stamp: &str) -> PathBuf {
        self.state_dir().join("backups").join(stamp)
    }

    pub fn local_config(&self) -> PathBuf {
        self.home.join(".config/dotfix/config.toml")
    }

    pub fn launch_agent(&self) -> PathBuf {
        self.home
            .join("Library/LaunchAgents/dev.noix.dotfix.plist")
    }

    pub fn zshrc(&self) -> PathBuf {
        self.home.join(".zshrc")
    }
}

/// Machine-local pointer to the data repository. Never stored in the repository
/// itself — it has to exist before the repository can be read.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct LocalConfig {
    pub repo: PathBuf,
    pub machine: String,
}

impl LocalConfig {
    pub fn load(fs: &dyn Fsys, path: &Path) -> Result<Self> {
        let raw = fs.read(path)?;
        toml::from_str(&raw).map_err(|source| Error::Toml {
            path: path.to_path_buf(),
            source,
        })
    }

    pub fn save(&self, fs: &dyn Fsys, path: &Path) -> Result<()> {
        let raw = toml::to_string_pretty(self)
            .map_err(|e| Error::Config(format!("serialising local config: {e}")))?;
        fs.write(path, &raw, 0o644)
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p dotfix-core paths`
Expected: 2 PASS.

- [ ] **Step 5: Write the failing tests for `Applied`**

`crates/core/src/state.rs`:

```rust
#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::ports::fake::FakeFsys;

    #[test]
    fn missing_file_yields_an_empty_state() {
        let fs = FakeFsys::new();
        let applied = Applied::load(&fs, Path::new("/state/applied.json")).unwrap();
        assert_eq!(applied, Applied::default());
    }

    #[test]
    fn round_trips() {
        let fs = FakeFsys::new();
        let path = PathBuf::from("/state/applied.json");

        let mut applied = Applied::default();
        applied.brew.insert("alpha".into());
        applied.cask.insert("charlie".into());
        applied
            .files
            .insert(PathBuf::from("/Users/test/.rc"), "abc123".into());

        applied.save(&fs, &path).unwrap();
        assert_eq!(Applied::load(&fs, &path).unwrap(), applied);
    }

    #[test]
    fn reports_the_path_of_corrupt_json() {
        let fs = FakeFsys::from([("/state/applied.json", "{ not json")]);
        let err = Applied::load(&fs, Path::new("/state/applied.json")).unwrap_err();
        assert!(err.to_string().contains("/state/applied.json"));
    }
}
```

- [ ] **Step 6: Run the tests to verify they fail**

Run: `cargo test -p dotfix-core state`
Expected: FAIL — `cannot find type Applied`.

- [ ] **Step 7: Implement `Applied`**

Prepend to `crates/core/src/state.rs`:

```rust
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::ports::Fsys;

/// What dotfix itself last wrote to this machine. The third leg of the
/// three-way diff: without it, "another machine added this" cannot be told
/// apart from "this machine removed it on purpose".
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct Applied {
    #[serde(default)]
    pub brew: BTreeSet<String>,
    #[serde(default)]
    pub cask: BTreeSet<String>,
    /// Target path → SHA-256 of the content dotfix wrote there.
    #[serde(default)]
    pub files: BTreeMap<PathBuf, String>,
}

impl Applied {
    /// A missing file means "dotfix has never run here" and yields the empty
    /// state, not an error.
    pub fn load(fs: &dyn Fsys, path: &Path) -> Result<Self> {
        if !fs.exists(path) {
            return Ok(Self::default());
        }
        let raw = fs.read(path)?;
        serde_json::from_str(&raw).map_err(|source| Error::Json {
            path: path.to_path_buf(),
            source,
        })
    }

    pub fn save(&self, fs: &dyn Fsys, path: &Path) -> Result<()> {
        let raw = serde_json::to_string_pretty(self).map_err(|source| Error::Json {
            path: path.to_path_buf(),
            source,
        })?;
        fs.write(path, &raw, 0o644)
    }
}
```

Add `pub mod paths;` and `pub mod state;` to `crates/core/src/lib.rs`.

- [ ] **Step 8: Run the full suite**

Run: `cargo test --workspace && cargo clippy --all-targets -- -D warnings`
Expected: all PASS.

- [ ] **Step 9: Commit**

```bash
git add -A
git commit -m "feat(core): add local paths, local config and applied-state tracking"
```

---

## Task 6: Package drift — the three-way diff

**Files:**
- Create: `crates/core/src/drift/mod.rs`, `crates/core/src/drift/packages.rs`
- Modify: `crates/core/src/lib.rs`
- Test: inline `#[cfg(test)]` in `packages.rs`

**Interfaces:**
- Consumes: `Desired` (Task 4), `Applied` (Task 5), `Brew` + `FakeBrew` (Task 3), `MachineConfig::ignore` (Task 2).
- Produces:
  - `PackageRef { name: String, cask: bool }`
  - `Drift` enum — `IncomingPackage`, `IncomingFile`, `LocallyRemoved`, `Unmanaged`, `RemovedPackage`, `RemovedFile`, `LocalEdit`
  - `Report { items: Vec<Drift> }` with `is_empty()`, `counts() -> Counts`
  - `Counts { incoming: usize, unmanaged: usize, removed: usize, local_edits: usize }`
  - `packages::diff(desired, actual_brew, actual_cask, applied, brew, ignore) -> Result<Vec<Drift>>`

This is the heart of the tool. The truth table from the spec-refinement section
at the top of this plan is the specification for `diff`.

- [ ] **Step 1: Write the failing tests**

`crates/core/src/drift/packages.rs`:

```rust
#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::config::Desired;
    use crate::ports::fake::FakeBrew;
    use crate::state::Applied;

    fn set(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    fn desired(brew: &[&str]) -> Desired {
        Desired {
            machine: "box-one".into(),
            brew: set(brew),
            ..Default::default()
        }
    }

    fn applied(brew: &[&str]) -> Applied {
        Applied {
            brew: set(brew),
            ..Default::default()
        }
    }

    #[test]
    fn desired_but_never_applied_is_incoming() {
        let brew = FakeBrew::new([], []);
        let out = diff(&desired(&["alpha"]), &set(&[]), &set(&[]), &applied(&[]), &brew, &[]).unwrap();
        assert_eq!(
            out,
            vec![Drift::IncomingPackage(PackageRef::formula("alpha"))]
        );
    }

    #[test]
    fn desired_and_applied_but_gone_locally_is_locally_removed() {
        let brew = FakeBrew::new([], []);
        let out = diff(
            &desired(&["alpha"]),
            &set(&[]),
            &set(&[]),
            &applied(&["alpha"]),
            &brew,
            &[],
        )
        .unwrap();
        assert_eq!(
            out,
            vec![Drift::LocallyRemoved(PackageRef::formula("alpha"))],
            "a deliberate local uninstall must not be silently reinstalled"
        );
    }

    #[test]
    fn installed_but_in_no_set_is_unmanaged() {
        let brew = FakeBrew::new(["bravo"], []);
        let out = diff(&desired(&[]), &set(&["bravo"]), &set(&[]), &applied(&[]), &brew, &[]).unwrap();
        assert_eq!(out, vec![Drift::Unmanaged(PackageRef::formula("bravo"))]);
    }

    #[test]
    fn ignored_packages_are_never_reported_as_unmanaged() {
        let brew = FakeBrew::new(["bravo"], []);
        let out = diff(
            &desired(&[]),
            &set(&["bravo"]),
            &set(&[]),
            &applied(&[]),
            &brew,
            &["bravo".to_string()],
        )
        .unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn dropped_from_a_set_is_removed_when_it_is_a_leaf() {
        let brew = FakeBrew::new(["alpha"], []);
        let out = diff(
            &desired(&[]),
            &set(&["alpha"]),
            &set(&[]),
            &applied(&["alpha"]),
            &brew,
            &[],
        )
        .unwrap();
        assert_eq!(
            out,
            vec![Drift::RemovedPackage {
                package: PackageRef::formula("alpha"),
                blocked_by: vec![],
            }]
        );
    }

    #[test]
    fn removal_is_blocked_when_something_still_depends_on_it() {
        let brew = FakeBrew::new(["alpha"], []).with_uses("alpha", ["bravo"]);
        let out = diff(
            &desired(&[]),
            &set(&["alpha"]),
            &set(&[]),
            &applied(&["alpha"]),
            &brew,
            &[],
        )
        .unwrap();
        assert_eq!(
            out,
            vec![Drift::RemovedPackage {
                package: PackageRef::formula("alpha"),
                blocked_by: vec!["bravo".to_string()],
            }]
        );
    }

    #[test]
    fn desired_and_installed_is_not_drift() {
        let brew = FakeBrew::new(["alpha"], []);
        let out = diff(
            &desired(&["alpha"]),
            &set(&["alpha"]),
            &set(&[]),
            &applied(&["alpha"]),
            &brew,
            &[],
        )
        .unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn casks_are_diffed_separately_and_keep_their_flag() {
        let brew = FakeBrew::new([], []);
        let mut d = Desired::default();
        d.cask = set(&["charlie"]);
        let out = diff(&d, &set(&[]), &set(&[]), &Applied::default(), &brew, &[]).unwrap();
        assert_eq!(out, vec![Drift::IncomingPackage(PackageRef::cask("charlie"))]);
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p dotfix-core drift::packages`
Expected: FAIL — `cannot find function diff`.

- [ ] **Step 3: Implement the drift types**

`crates/core/src/drift/mod.rs`:

```rust
pub mod packages;

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
    /// Installed by hand, in no set. → adopt or ignore
    Unmanaged(PackageRef),
    /// Dropped from its set, still installed. → uninstall if it is a leaf
    RemovedPackage {
        package: PackageRef,
        /// Installed packages still depending on it. Non-empty means the
        /// uninstall is skipped.
        blocked_by: Vec<String>,
    },
    /// Managed file dropped from its set, still on disk. → remove
    RemovedFile { target: PathBuf },
    /// Managed file edited locally. → write back or overwrite
    LocalEdit { target: PathBuf, set: String },
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Report {
    pub items: Vec<Drift>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Counts {
    pub incoming: usize,
    pub unmanaged: usize,
    pub removed: usize,
    pub local_edits: usize,
}

impl Report {
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn counts(&self) -> Counts {
        let mut c = Counts::default();
        for item in &self.items {
            match item {
                Drift::IncomingPackage(_) | Drift::IncomingFile { .. } => c.incoming += 1,
                Drift::Unmanaged(_) | Drift::LocallyRemoved(_) => c.unmanaged += 1,
                Drift::RemovedPackage { .. } | Drift::RemovedFile { .. } => c.removed += 1,
                Drift::LocalEdit { .. } => c.local_edits += 1,
            }
        }
        c
    }
}
```

- [ ] **Step 4: Implement the package diff**

Prepend to `crates/core/src/drift/packages.rs`:

```rust
use std::collections::BTreeSet;

use crate::config::Desired;
use crate::drift::{Drift, PackageRef};
use crate::error::Result;
use crate::ports::Brew;
use crate::state::Applied;

/// Three-way diff over packages.
///
/// | desired | actual | applied | outcome          |
/// |---------|--------|---------|------------------|
/// | yes     | no     | no      | IncomingPackage  |
/// | yes     | no     | yes     | LocallyRemoved   |
/// | no      | yes    | no      | Unmanaged        |
/// | no      | yes    | yes     | RemovedPackage   |
/// | yes     | yes    | —       | in sync          |
pub fn diff(
    desired: &Desired,
    actual_brew: &BTreeSet<String>,
    actual_cask: &BTreeSet<String>,
    applied: &Applied,
    brew: &dyn Brew,
    ignore: &[String],
) -> Result<Vec<Drift>> {
    let mut out = Vec::new();
    out.extend(diff_kind(
        &desired.brew,
        actual_brew,
        &applied.brew,
        brew,
        ignore,
        false,
    )?);
    out.extend(diff_kind(
        &desired.cask,
        actual_cask,
        &applied.cask,
        brew,
        ignore,
        true,
    )?);
    Ok(out)
}

fn diff_kind(
    desired: &BTreeSet<String>,
    actual: &BTreeSet<String>,
    applied: &BTreeSet<String>,
    brew: &dyn Brew,
    ignore: &[String],
    cask: bool,
) -> Result<Vec<Drift>> {
    let mut out = Vec::new();

    for name in desired.difference(actual) {
        let package = PackageRef {
            name: name.clone(),
            cask,
        };
        if applied.contains(name) {
            out.push(Drift::LocallyRemoved(package));
        } else {
            out.push(Drift::IncomingPackage(package));
        }
    }

    for name in actual.difference(desired) {
        if ignore.iter().any(|i| i == name) {
            continue;
        }
        let package = PackageRef {
            name: name.clone(),
            cask,
        };
        if applied.contains(name) {
            // Casks have no dependents; only formulae need the leaf check.
            let blocked_by = if cask {
                Vec::new()
            } else {
                brew.uses_installed(name)?
            };
            out.push(Drift::RemovedPackage {
                package,
                blocked_by,
            });
        } else {
            out.push(Drift::Unmanaged(package));
        }
    }

    Ok(out)
}
```

Add `pub mod drift;` to `crates/core/src/lib.rs`.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p dotfix-core drift::packages`
Expected: 8 PASS.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "feat(core): add three-way package drift detection"
```

---

## Task 7: Template rendering

**Files:**
- Create: `crates/core/src/render/mod.rs`, `crates/core/src/render/template.rs`
- Modify: `crates/core/src/lib.rs`
- Test: inline `#[cfg(test)]` in `template.rs`

**Interfaces:**
- Consumes: `Error`, `Result` (Task 1).
- Produces:
  - `Vars { home: PathBuf, user: String, machine: String, extra: BTreeMap<String, String> }`
  - `trait SecretLookup { fn get(&self, name: &str) -> Result<String>; }` — implemented in Task 8 by the real resolver; declared here so templates can be tested without a provider
  - `NoSecrets` — a `SecretLookup` that always errors, for templates that must not use secrets
  - `Rendered { content: String, contains_secrets: bool }`
  - `render(path: &Path, source: &str, vars: &Vars, secrets: &dyn SecretLookup) -> Result<Rendered>`

- [ ] **Step 1: Write the failing tests**

`crates/core/src/render/template.rs`:

```rust
#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};

    use super::*;

    struct StaticSecrets;

    impl SecretLookup for StaticSecrets {
        fn get(&self, name: &str) -> Result<String> {
            match name {
                "api_key" => Ok("s3cr3t".to_string()),
                other => Err(Error::Secret {
                    name: other.to_string(),
                    reason: "not configured".to_string(),
                }),
            }
        }
    }

    fn vars() -> Vars {
        Vars {
            home: PathBuf::from("/Users/test"),
            user: "test".into(),
            machine: "box-one".into(),
            extra: BTreeMap::from([("git_email".to_string(), "someone@example.com".to_string())]),
        }
    }

    #[test]
    fn substitutes_the_builtin_variables() {
        let out = render(
            Path::new("t.tmpl"),
            "export PNPM_HOME=\"{{ home }}/Library/pnpm\"\n# {{ user }}@{{ machine }}",
            &vars(),
            &NoSecrets,
        )
        .unwrap();
        assert_eq!(
            out.content,
            "export PNPM_HOME=\"/Users/test/Library/pnpm\"\n# test@box-one"
        );
        assert!(!out.contains_secrets);
    }

    #[test]
    fn substitutes_machine_vars() {
        let out = render(Path::new("t.tmpl"), "{{ git_email }}", &vars(), &NoSecrets).unwrap();
        assert_eq!(out.content, "someone@example.com");
    }

    #[test]
    fn resolves_secrets_and_flags_the_result() {
        let out = render(
            Path::new("t.tmpl"),
            "access_key = {{ secret(\"api_key\") }}",
            &vars(),
            &StaticSecrets,
        )
        .unwrap();
        assert_eq!(out.content, "access_key = s3cr3t");
        assert!(out.contains_secrets, "must be flagged so the file is written 0600");
    }

    #[test]
    fn an_unresolvable_secret_fails_and_names_the_file() {
        let err = render(
            Path::new("/repo/sets/infra/files/x.tmpl"),
            "{{ secret(\"missing\") }}",
            &vars(),
            &StaticSecrets,
        )
        .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("/repo/sets/infra/files/x.tmpl"));
    }

    #[test]
    fn an_unknown_variable_is_an_error_not_an_empty_string() {
        let err = render(Path::new("t.tmpl"), "{{ nope }}", &vars(), &NoSecrets).unwrap_err();
        assert!(matches!(err, Error::Template { .. }));
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p dotfix-core render::template`
Expected: FAIL — `cannot find function render`.

- [ ] **Step 3: Implement rendering**

Prepend to `crates/core/src/render/template.rs`:

```rust
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use minijinja::{Environment, UndefinedBehavior, Value};

use crate::error::{Error, Result};

/// Variables available to every template and shell fragment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Vars {
    pub home: PathBuf,
    pub user: String,
    pub machine: String,
    /// `[vars]` from the machine configuration.
    pub extra: BTreeMap<String, String>,
}

/// Resolves a logical secret name to its value. Implemented in the `secrets`
/// module; declared here so templates can be tested without a provider.
pub trait SecretLookup {
    fn get(&self, name: &str) -> Result<String>;
}

/// Rejects every secret. Use for content that must never contain credentials.
pub struct NoSecrets;

impl SecretLookup for NoSecrets {
    fn get(&self, name: &str) -> Result<String> {
        Err(Error::Secret {
            name: name.to_string(),
            reason: "secrets are not permitted in this context".to_string(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rendered {
    pub content: String,
    /// True when `secret()` was called. The caller must then write the file
    /// with mode 0600 and redact it in any output.
    pub contains_secrets: bool,
}

/// Render one template. Unknown variables are an error rather than an empty
/// string — a silently empty `PATH` entry is worse than a failed run.
pub fn render(
    path: &Path,
    source: &str,
    vars: &Vars,
    secrets: &dyn SecretLookup,
) -> Result<Rendered> {
    let used = Arc::new(AtomicBool::new(false));

    let mut env = Environment::new();
    env.set_undefined_behavior(UndefinedBehavior::Strict);

    // minijinja needs an owned, 'static closure; collect the lookups eagerly.
    let resolved = collect_secret_calls(source)
        .into_iter()
        .map(|name| secrets.get(&name).map(|value| (name, value)))
        .collect::<Result<BTreeMap<String, String>>>()
        .map_err(|e| annotate(path, e))?;

    let flag = Arc::clone(&used);
    env.add_function("secret", move |name: String| -> std::result::Result<Value, minijinja::Error> {
        flag.store(true, Ordering::Relaxed);
        resolved
            .get(&name)
            .map(|v| Value::from(v.clone()))
            .ok_or_else(|| {
                minijinja::Error::new(
                    minijinja::ErrorKind::InvalidOperation,
                    format!("unknown secret `{name}`"),
                )
            })
    });

    let mut ctx: BTreeMap<String, String> = vars.extra.clone();
    ctx.insert("home".into(), vars.home.display().to_string());
    ctx.insert("user".into(), vars.user.clone());
    ctx.insert("machine".into(), vars.machine.clone());

    let content = env
        .render_str(source, ctx)
        .map_err(|source| Error::Template {
            path: path.to_path_buf(),
            source,
        })?;

    Ok(Rendered {
        content,
        contains_secrets: used.load(Ordering::Relaxed),
    })
}

/// Extract every `secret("name")` argument from the source so the values can be
/// fetched before rendering starts.
fn collect_secret_calls(source: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut rest = source;
    while let Some(idx) = rest.find("secret(") {
        rest = &rest[idx + "secret(".len()..];
        let Some(open) = rest.find(['"', '\'']) else {
            break;
        };
        let quote = rest.as_bytes()[open] as char;
        let after = &rest[open + 1..];
        let Some(close) = after.find(quote) else {
            break;
        };
        names.push(after[..close].to_string());
        rest = &after[close + 1..];
    }
    names.sort();
    names.dedup();
    names
}

fn annotate(path: &Path, err: Error) -> Error {
    match err {
        Error::Secret { name, reason } => Error::Secret {
            name,
            reason: format!("{reason} (in {})", path.display()),
        },
        other => other,
    }
}
```

`crates/core/src/render/mod.rs`:

```rust
pub mod template;

pub use template::{NoSecrets, Rendered, SecretLookup, Vars, render};
```

Add `pub mod render;` to `crates/core/src/lib.rs`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p dotfix-core render::template`
Expected: 5 PASS.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(core): add template rendering with strict undefined handling"
```

---

## Task 8: Secret providers and the resolver

**Files:**
- Create: `crates/core/src/ports/exec.rs`
- Create: `crates/core/src/secrets/mod.rs`, `keychain.rs`, `onepassword.rs`, `age.rs`
- Modify: `crates/core/src/ports/mod.rs`, `crates/core/src/ports/fake.rs`, `crates/core/src/lib.rs`
- Test: inline `#[cfg(test)]` in `secrets/mod.rs` and each provider

**Interfaces:**
- Consumes: `ProviderKind` (Task 2), `SecretLookup` (Task 7), `Error`/`Result` (Task 1).
- Produces:
  - `trait Exec { fn run(&self, program: &str, args: &[&str]) -> Result<String>; }`, `RealExec`, `FakeExec`
  - `trait SecretProvider { fn get(&self, reference: &str) -> Result<String>; }`
  - `KeychainProvider`, `OnePasswordProvider { vault }`, `AgeProvider { repo_root }`
  - `default_reference(kind, vault, name) -> String`
  - `Resolver<'a>` implementing `SecretLookup`

`Brew` and `Git` keep their own `Command` calls — they are already behind traits,
which is what tests need. `Exec` exists because the three secret providers are
otherwise nothing *but* a shell call, and their argument construction is exactly
the part worth testing.

- [ ] **Step 1: Add the `Exec` port**

`crates/core/src/ports/exec.rs`:

```rust
use std::process::Command;

use crate::error::{Error, Result};

/// A single external command invocation, captured so that thin command
/// wrappers stay testable.
pub trait Exec {
    fn run(&self, program: &str, args: &[&str]) -> Result<String>;
}

pub struct RealExec;

impl Exec for RealExec {
    fn run(&self, program: &str, args: &[&str]) -> Result<String> {
        let output = Command::new(program)
            .args(args)
            .output()
            .map_err(|e| Error::Command {
                cmd: program.to_string(),
                stderr: e.to_string(),
            })?;
        if !output.status.success() {
            return Err(Error::Command {
                cmd: format!("{program} {}", args.join(" ")),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            });
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}
```

Append to `crates/core/src/ports/fake.rs`:

```rust
#[derive(Default)]
pub struct FakeExec {
    responses: BTreeMap<String, String>,
    calls: RefCell<Vec<String>>,
}

impl FakeExec {
    pub fn new<const N: usize>(responses: [(&str, &str); N]) -> Self {
        Self {
            responses: responses
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            calls: RefCell::new(Vec::new()),
        }
    }

    pub fn calls(&self) -> Vec<String> {
        self.calls.borrow().clone()
    }
}

impl crate::ports::Exec for FakeExec {
    fn run(&self, program: &str, args: &[&str]) -> Result<String> {
        let key = format!("{program} {}", args.join(" "));
        self.calls.borrow_mut().push(key.clone());
        self.responses
            .get(&key)
            .cloned()
            .ok_or_else(|| Error::Command {
                cmd: key,
                stderr: "no fake response configured".to_string(),
            })
    }
}
```

Extend `crates/core/src/ports/mod.rs` with `mod exec;` and `pub use exec::{Exec, RealExec};`.

- [ ] **Step 2: Write the failing tests for reference derivation and the resolver**

`crates/core/src/secrets/mod.rs`:

```rust
#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use super::*;
    use crate::config::ProviderKind;
    use crate::ports::fake::FakeExec;

    #[test]
    fn keychain_default_reference_is_service_slash_name() {
        assert_eq!(
            default_reference(ProviderKind::Keychain, None, "api_key"),
            "dotfix/api_key"
        );
    }

    #[test]
    fn onepassword_default_reference_uses_the_vault() {
        assert_eq!(
            default_reference(ProviderKind::OnePassword, Some("Example"), "api_key"),
            "op://Example/dotfix/api_key"
        );
    }

    #[test]
    fn age_default_reference_is_a_repository_relative_file() {
        assert_eq!(
            default_reference(ProviderKind::Age, None, "api_key"),
            "secrets/api_key.age"
        );
    }

    #[test]
    fn an_explicit_mapping_wins_over_the_default() {
        let exec = FakeExec::new([(
            "op read op://Other/thing/field",
            "mapped-value\n",
        )]);
        let provider = OnePasswordProvider {
            vault: Some("Example".into()),
        };
        let mapping = BTreeMap::from([(
            "api_key".to_string(),
            "op://Other/thing/field".to_string(),
        )]);
        let resolver = Resolver {
            provider: &provider,
            kind: ProviderKind::OnePassword,
            vault: Some("Example".into()),
            mapping: &mapping,
            exec: &exec,
        };
        assert_eq!(resolver.get("api_key").unwrap(), "mapped-value");
    }

    #[test]
    fn a_failed_lookup_reports_the_name_but_never_a_value() {
        let exec = FakeExec::new([]);
        let provider = KeychainProvider;
        let mapping = BTreeMap::new();
        let resolver = Resolver {
            provider: &provider,
            kind: ProviderKind::Keychain,
            vault: None,
            mapping: &mapping,
            exec: &exec,
        };
        let err = resolver.get("api_key").unwrap_err();
        assert!(err.to_string().contains("api_key"));
    }

    #[test]
    fn keychain_provider_builds_the_expected_command() {
        let exec = FakeExec::new([(
            "security find-generic-password -s dotfix -a api_key -w",
            "s3cr3t\n",
        )]);
        assert_eq!(
            KeychainProvider.get_with("dotfix/api_key", &exec).unwrap(),
            "s3cr3t"
        );
    }

    #[test]
    fn age_provider_decrypts_relative_to_the_repository() {
        let exec = FakeExec::new([(
            "age --decrypt --identity /Users/test/.config/dotfix/age.key /repo/secrets/api_key.age",
            "s3cr3t\n",
        )]);
        let provider = AgeProvider {
            repo_root: PathBuf::from("/repo"),
            identity: PathBuf::from("/Users/test/.config/dotfix/age.key"),
        };
        assert_eq!(
            provider.get_with("secrets/api_key.age", &exec).unwrap(),
            "s3cr3t"
        );
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p dotfix-core secrets`
Expected: FAIL — `cannot find function default_reference`.

- [ ] **Step 4: Implement the providers and resolver**

Prepend to `crates/core/src/secrets/mod.rs`:

```rust
mod age;
mod keychain;
mod onepassword;

use std::collections::BTreeMap;

pub use age::AgeProvider;
pub use keychain::KeychainProvider;
pub use onepassword::OnePasswordProvider;

use crate::config::ProviderKind;
use crate::error::{Error, Result};
use crate::ports::Exec;
use crate::render::SecretLookup;

/// A backend that turns a provider-specific reference into a value.
pub trait SecretProvider {
    fn get_with(&self, reference: &str, exec: &dyn Exec) -> Result<String>;
}

/// Where a named secret lives when the machine configuration gives no override.
pub fn default_reference(kind: ProviderKind, vault: Option<&str>, name: &str) -> String {
    match kind {
        ProviderKind::Keychain => format!("dotfix/{name}"),
        ProviderKind::OnePassword => {
            let vault = vault.unwrap_or("Private");
            format!("op://{vault}/dotfix/{name}")
        }
        ProviderKind::Age => format!("secrets/{name}.age"),
    }
}

/// Maps logical secret names to values: the machine decides *where*, the set
/// decides *what*.
pub struct Resolver<'a> {
    pub provider: &'a dyn SecretProvider,
    pub kind: ProviderKind,
    pub vault: Option<String>,
    pub mapping: &'a BTreeMap<String, String>,
    pub exec: &'a dyn Exec,
}

impl SecretLookup for Resolver<'_> {
    fn get(&self, name: &str) -> Result<String> {
        let reference = self.mapping.get(name).cloned().unwrap_or_else(|| {
            default_reference(self.kind, self.vault.as_deref(), name)
        });
        self.provider
            .get_with(&reference, self.exec)
            // The reason must never carry the value, only why the lookup failed.
            .map_err(|e| Error::Secret {
                name: name.to_string(),
                reason: short_reason(&e),
            })
            .map(|v| v.trim_end_matches('\n').to_string())
    }
}

fn short_reason(err: &Error) -> String {
    match err {
        Error::Command { stderr, .. } if !stderr.is_empty() => stderr.clone(),
        Error::Command { .. } => "lookup failed".to_string(),
        other => other.to_string(),
    }
}
```

`crates/core/src/secrets/keychain.rs`:

```rust
use crate::error::{Error, Result};
use crate::ports::Exec;
use crate::secrets::SecretProvider;

/// Reference format: `<service>/<account>`.
pub struct KeychainProvider;

impl SecretProvider for KeychainProvider {
    fn get_with(&self, reference: &str, exec: &dyn Exec) -> Result<String> {
        let (service, account) = reference.split_once('/').ok_or_else(|| Error::Secret {
            name: reference.to_string(),
            reason: "keychain references must look like `service/account`".to_string(),
        })?;
        exec.run(
            "security",
            &["find-generic-password", "-s", service, "-a", account, "-w"],
        )
    }
}
```

`crates/core/src/secrets/onepassword.rs`:

```rust
use crate::error::Result;
use crate::ports::Exec;
use crate::secrets::SecretProvider;

/// Reference format: `op://<vault>/<item>/<field>`.
pub struct OnePasswordProvider {
    pub vault: Option<String>,
}

impl SecretProvider for OnePasswordProvider {
    fn get_with(&self, reference: &str, exec: &dyn Exec) -> Result<String> {
        exec.run("op", &["read", reference])
    }
}
```

`crates/core/src/secrets/age.rs`:

```rust
use std::path::PathBuf;

use crate::error::Result;
use crate::ports::Exec;
use crate::secrets::SecretProvider;

/// Reference format: a path relative to the repository root.
pub struct AgeProvider {
    pub repo_root: PathBuf,
    pub identity: PathBuf,
}

impl SecretProvider for AgeProvider {
    fn get_with(&self, reference: &str, exec: &dyn Exec) -> Result<String> {
        let file = self.repo_root.join(reference);
        exec.run(
            "age",
            &[
                "--decrypt",
                "--identity",
                &self.identity.display().to_string(),
                &file.display().to_string(),
            ],
        )
    }
}
```

Add `pub mod secrets;` to `crates/core/src/lib.rs`.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p dotfix-core secrets`
Expected: 7 PASS.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "feat(core): add keychain, 1password and age secret providers"
```

---

## Task 9: Generating `.zshrc` from shell fragments

**Files:**
- Create: `crates/core/src/render/zshrc.rs`
- Modify: `crates/core/src/render/mod.rs`
- Test: inline `#[cfg(test)]` in `zshrc.rs`

**Interfaces:**
- Consumes: `Fragment` (Task 4), `Fsys` (Task 3), `Vars` + `render` (Task 7).
- Produces:
  - `MARKER: &str`, `CHECKSUM_PREFIX: &str`
  - `checksum(content: &str) -> String` (SHA-256 hex)
  - `generate(fragments, fs, vars, secrets) -> Result<String>`
  - `body_of(generated: &str) -> Option<&str>` and `recorded_checksum(generated: &str) -> Option<&str>` — used by file drift to tell "repo changed" from "edited by hand"

- [ ] **Step 1: Write the failing tests**

`crates/core/src/render/zshrc.rs`:

```rust
#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use super::*;
    use crate::config::Fragment;
    use crate::ports::fake::FakeFsys;
    use crate::render::{NoSecrets, Vars};

    fn vars() -> Vars {
        Vars {
            home: PathBuf::from("/Users/test"),
            user: "test".into(),
            machine: "box-one".into(),
            extra: BTreeMap::new(),
        }
    }

    fn fragments() -> Vec<Fragment> {
        vec![
            Fragment {
                set: "core".into(),
                name: "00-first.zsh".into(),
                path: PathBuf::from("/repo/sets/core/shell/00-first.zsh"),
            },
            Fragment {
                set: "web".into(),
                name: "10-web.zsh".into(),
                path: PathBuf::from("/repo/sets/web/shell/10-web.zsh"),
            },
        ]
    }

    fn fs() -> FakeFsys {
        FakeFsys::from([
            ("/repo/sets/core/shell/00-first.zsh", "# instant prompt\n"),
            ("/repo/sets/web/shell/10-web.zsh", "export W={{ home }}/w\n"),
        ])
    }

    #[test]
    fn concatenates_fragments_in_order_with_provenance_headers() {
        let out = generate(&fragments(), &fs(), &vars(), &NoSecrets).unwrap();
        let first = out.find("# instant prompt").unwrap();
        let second = out.find("export W=").unwrap();
        assert!(first < second, "fragment order must be preserved");
        assert!(out.contains("# --- core/00-first.zsh ---"));
        assert!(out.contains("# --- web/10-web.zsh ---"));
    }

    #[test]
    fn renders_templates_inside_fragments() {
        let out = generate(&fragments(), &fs(), &vars(), &NoSecrets).unwrap();
        assert!(out.contains("export W=/Users/test/w"));
        assert!(!out.contains("{{ home }}"));
    }

    #[test]
    fn starts_with_the_marker_and_a_checksum_of_the_body() {
        let out = generate(&fragments(), &fs(), &vars(), &NoSecrets).unwrap();
        assert!(out.starts_with(MARKER));
        let body = body_of(&out).unwrap();
        assert_eq!(recorded_checksum(&out).unwrap(), checksum(body));
    }

    #[test]
    fn ends_with_the_local_escape_hatch() {
        let out = generate(&fragments(), &fs(), &vars(), &NoSecrets).unwrap();
        assert!(out.trim_end().ends_with("[[ -f ~/.zshrc.local ]] && source ~/.zshrc.local"));
    }

    #[test]
    fn a_hand_edit_breaks_the_recorded_checksum() {
        let out = generate(&fragments(), &fs(), &vars(), &NoSecrets).unwrap();
        let tampered = format!("{out}\nalias sneaky=1\n");
        let body = body_of(&tampered).unwrap();
        assert_ne!(recorded_checksum(&tampered).unwrap(), checksum(body));
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p dotfix-core render::zshrc`
Expected: FAIL — `cannot find function generate`.

- [ ] **Step 3: Implement generation**

Prepend to `crates/core/src/render/zshrc.rs`:

```rust
use sha2::{Digest, Sha256};

use crate::config::Fragment;
use crate::error::Result;
use crate::ports::Fsys;
use crate::render::{SecretLookup, Vars, render};

pub const MARKER: &str = "# generated by dotfix — edit sets/*/shell/ instead";
pub const CHECKSUM_PREFIX: &str = "# dotfix-checksum: ";
const TRAILER: &str = "[[ -f ~/.zshrc.local ]] && source ~/.zshrc.local";

pub fn checksum(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Concatenate the active sets' shell fragments into a complete `.zshrc`.
///
/// Order is decided in [`crate::config::Repo::resolve`] and preserved here —
/// the Powerlevel10k instant-prompt block must stay at the very top.
pub fn generate(
    fragments: &[Fragment],
    fs: &dyn Fsys,
    vars: &Vars,
    secrets: &dyn SecretLookup,
) -> Result<String> {
    let mut body = String::new();

    for fragment in fragments {
        let source = fs.read(&fragment.path)?;
        let rendered = render(&fragment.path, &source, vars, secrets)?;
        body.push_str(&format!("# --- {}/{} ---\n", fragment.set, fragment.name));
        body.push_str(rendered.content.trim_end());
        body.push_str("\n\n");
    }

    body.push_str(TRAILER);
    body.push('\n');

    Ok(format!(
        "{MARKER}\n{CHECKSUM_PREFIX}{}\n\n{body}",
        checksum(&body)
    ))
}

/// Everything after the two header lines and the blank line separating them.
pub fn body_of(generated: &str) -> Option<&str> {
    let (_, rest) = generated.split_once('\n')?;
    let (checksum_line, rest) = rest.split_once('\n')?;
    checksum_line.starts_with(CHECKSUM_PREFIX).then_some(())?;
    rest.strip_prefix('\n')
}

pub fn recorded_checksum(generated: &str) -> Option<&str> {
    generated
        .lines()
        .nth(1)?
        .strip_prefix(CHECKSUM_PREFIX)
}
```

Extend `crates/core/src/render/mod.rs`:

```rust
pub mod zshrc;
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p dotfix-core render::zshrc`
Expected: 5 PASS.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(core): generate .zshrc from ordered set fragments"
```

---

## Task 10: File drift

**Files:**
- Create: `crates/core/src/drift/files.rs`
- Modify: `crates/core/src/drift/mod.rs`
- Test: inline `#[cfg(test)]` in `files.rs`

**Interfaces:**
- Consumes: `ResolvedFile` (Task 4), `Applied` (Task 5), `Fsys` (Task 3), `checksum` (Task 9).
- Produces:
  - `RenderedFile { file: ResolvedFile, content: String, contains_secrets: bool }`
  - `files::diff(rendered: &[RenderedFile], applied: &Applied, fs: &dyn Fsys) -> Result<Vec<Drift>>`

Decision (from the plan's spec-refinement section): a managed file that is
missing on disk is `IncomingFile`, not a deliberate removal. Deactivating the set
is the way to say "not here".

- [ ] **Step 1: Write the failing tests**

`crates/core/src/drift/files.rs`:

```rust
#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use super::*;
    use crate::config::{FileMode, ResolvedFile};
    use crate::ports::fake::FakeFsys;
    use crate::render::zshrc::checksum;
    use crate::state::Applied;

    fn rendered(content: &str) -> RenderedFile {
        RenderedFile {
            file: ResolvedFile {
                set: "core".into(),
                source: PathBuf::from("/repo/sets/core/files/rc.tmpl"),
                target: PathBuf::from("/Users/test/.rc"),
                mode: FileMode::Template,
            },
            content: content.to_string(),
            contains_secrets: false,
        }
    }

    fn applied_with(content: &str) -> Applied {
        Applied {
            files: BTreeMap::from([(PathBuf::from("/Users/test/.rc"), checksum(content))]),
            ..Default::default()
        }
    }

    #[test]
    fn a_missing_target_is_incoming() {
        let fs = FakeFsys::new();
        let out = diff(&[rendered("new")], &Applied::default(), &fs).unwrap();
        assert_eq!(
            out,
            vec![Drift::IncomingFile {
                target: PathBuf::from("/Users/test/.rc"),
                set: "core".into(),
            }]
        );
    }

    #[test]
    fn an_unchanged_target_is_not_drift() {
        let fs = FakeFsys::from([("/Users/test/.rc", "same")]);
        let out = diff(&[rendered("same")], &applied_with("same"), &fs).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn a_repository_change_with_no_local_edit_is_incoming() {
        let fs = FakeFsys::from([("/Users/test/.rc", "old")]);
        let out = diff(&[rendered("new")], &applied_with("old"), &fs).unwrap();
        assert!(matches!(out[0], Drift::IncomingFile { .. }));
    }

    #[test]
    fn a_local_edit_wins_over_a_repository_change() {
        let fs = FakeFsys::from([("/Users/test/.rc", "edited by hand")]);
        let out = diff(&[rendered("new")], &applied_with("old"), &fs).unwrap();
        assert_eq!(
            out,
            vec![Drift::LocalEdit {
                target: PathBuf::from("/Users/test/.rc"),
                set: "core".into(),
            }],
            "never overwrite a hand edit without asking"
        );
    }

    #[test]
    fn a_file_no_longer_desired_is_removed() {
        let fs = FakeFsys::from([("/Users/test/.rc", "stale")]);
        let out = diff(&[], &applied_with("stale"), &fs).unwrap();
        assert_eq!(
            out,
            vec![Drift::RemovedFile {
                target: PathBuf::from("/Users/test/.rc"),
            }]
        );
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p dotfix-core drift::files`
Expected: FAIL — `cannot find function diff`.

- [ ] **Step 3: Implement file drift**

Prepend to `crates/core/src/drift/files.rs`:

```rust
use std::collections::BTreeSet;

use crate::config::ResolvedFile;
use crate::drift::Drift;
use crate::error::Result;
use crate::ports::Fsys;
use crate::render::zshrc::checksum;
use crate::state::Applied;

/// A managed file with its content already rendered for this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedFile {
    pub file: ResolvedFile,
    pub content: String,
    pub contains_secrets: bool,
}

pub fn diff(rendered: &[RenderedFile], applied: &Applied, fs: &dyn Fsys) -> Result<Vec<Drift>> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();

    for item in rendered {
        let target = &item.file.target;
        seen.insert(target.clone());

        let want = checksum(&item.content);
        let recorded = applied.files.get(target);

        if !fs.exists(target) {
            out.push(Drift::IncomingFile {
                target: target.clone(),
                set: item.file.set.clone(),
            });
            continue;
        }

        let on_disk = checksum(&fs.read(target)?);

        match recorded {
            // dotfix wrote it and it is untouched: only the repository can differ
            Some(r) if *r == on_disk => {
                if *r != want {
                    out.push(Drift::IncomingFile {
                        target: target.clone(),
                        set: item.file.set.clone(),
                    });
                }
            }
            // Either never written by dotfix, or changed underneath it.
            _ if on_disk != want => out.push(Drift::LocalEdit {
                target: target.clone(),
                set: item.file.set.clone(),
            }),
            _ => {}
        }
    }

    for target in applied.files.keys() {
        if !seen.contains(target) && fs.exists(target) {
            out.push(Drift::RemovedFile {
                target: target.clone(),
            });
        }
    }

    Ok(out)
}
```

Extend `crates/core/src/drift/mod.rs`:

```rust
pub mod files;
pub use files::RenderedFile;
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p dotfix-core drift::files`
Expected: 5 PASS.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(core): add checksum-based file drift detection"
```

---

## Task 11: The engine — assembling a full inspection

**Files:**
- Create: `crates/core/src/engine.rs`
- Modify: `crates/core/src/secrets/mod.rs` (add `provider_for`), `crates/core/src/lib.rs`
- Test: inline `#[cfg(test)]` in `engine.rs`

**Interfaces:**
- Consumes: everything from Tasks 2–10.
- Produces:
  - `provider_for(kind, vault, repo_root, home) -> Box<dyn SecretProvider>`
  - `Engine<'a> { fs, brew, git, exec, paths, user }`
  - `Inspection { desired: Desired, rendered: Vec<RenderedFile>, report: Report }`
  - `Engine::inspect(&self, local: &LocalConfig) -> Result<Inspection>`

This is the single entry point every CLI command uses. The generated `.zshrc` is
treated as one more managed file so it flows through the same drift logic.

- [ ] **Step 1: Add `provider_for`**

Append to `crates/core/src/secrets/mod.rs` (above the test module):

```rust
use std::path::Path;

/// Build the backend a machine's configuration asks for.
pub fn provider_for(
    kind: ProviderKind,
    vault: Option<&str>,
    repo_root: &Path,
    home: &Path,
) -> Box<dyn SecretProvider> {
    match kind {
        ProviderKind::Keychain => Box::new(KeychainProvider),
        ProviderKind::OnePassword => Box::new(OnePasswordProvider {
            vault: vault.map(str::to_string),
        }),
        ProviderKind::Age => Box::new(AgeProvider {
            repo_root: repo_root.to_path_buf(),
            identity: home.join(".config/dotfix/age.key"),
        }),
    }
}
```

- [ ] **Step 2: Write the failing tests**

`crates/core/src/engine.rs`:

```rust
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
            ("/repo/sets/core/shell/10-path.zsh", "export P={{ home }}/bin\n"),
            ("/repo/machines/box-one.toml", "sets = [\"core\"]\n"),
            ("/Users/test/.config/dotfix/config.toml",
             "repo = \"/repo\"\nmachine = \"box-one\"\n"),
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
        let (fs, brew, git, exec) = (fs(), FakeBrew::new([], []), FakeGit::new(), FakeExec::new([]));
        let out = engine(&fs, &brew, &git, &exec).inspect(&local()).unwrap();

        let rc = out
            .rendered
            .iter()
            .find(|r| r.file.target == PathBuf::from("/Users/test/.rc"))
            .expect("managed file must be rendered");
        assert_eq!(rc.content, "home=/Users/test\n");
        assert!(out.report.items.contains(&Drift::IncomingFile {
            target: PathBuf::from("/Users/test/.rc"),
            set: "core".into(),
        }));
    }

    #[test]
    fn treats_the_generated_zshrc_as_a_managed_file() {
        let (fs, brew, git, exec) = (fs(), FakeBrew::new([], []), FakeGit::new(), FakeExec::new([]));
        let out = engine(&fs, &brew, &git, &exec).inspect(&local()).unwrap();

        let zshrc = out
            .rendered
            .iter()
            .find(|r| r.file.target == PathBuf::from("/Users/test/.zshrc"))
            .expect(".zshrc must be part of the inspection");
        assert!(zshrc.content.contains("export P=/Users/test/bin"));
        assert!(zshrc.content.starts_with(crate::render::zshrc::MARKER));
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

        assert!(out.report.items.contains(&Drift::IncomingPackage(PackageRef::formula("alpha"))));
        assert!(out.report.items.contains(&Drift::Unmanaged(PackageRef::formula("stray"))));
        assert!(out.report.items.iter().any(|d| matches!(d, Drift::IncomingFile { .. })));
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
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p dotfix-core engine`
Expected: FAIL — `cannot find type Engine`.

- [ ] **Step 4: Implement the engine**

Prepend to `crates/core/src/engine.rs`:

```rust
use std::collections::BTreeSet;

use crate::config::{Desired, FileMode, Repo, ResolvedFile};
use crate::drift::files::RenderedFile;
use crate::drift::{Drift, Report, files, packages};
use crate::error::{Error, Result};
use crate::paths::{LocalConfig, Paths};
use crate::ports::{Brew, Exec, Fsys, Git};
use crate::render::{Rendered, SecretLookup, Vars, render, zshrc};
use crate::secrets::{Resolver, provider_for};
use crate::state::Applied;

/// Wiring of the ports plus machine-local facts. Every command constructs one.
pub struct Engine<'a> {
    pub fs: &'a dyn Fsys,
    pub brew: &'a dyn Brew,
    pub git: &'a dyn Git,
    pub exec: &'a dyn Exec,
    pub paths: Paths,
    pub user: String,
}

/// Everything a command needs: what should be, what was rendered, what differs.
pub struct Inspection {
    pub desired: Desired,
    pub rendered: Vec<RenderedFile>,
    pub report: Report,
}

impl Engine<'_> {
    /// Fast-forward the data repository. Never merges.
    pub fn sync(&self, local: &LocalConfig) -> Result<()> {
        self.git.fetch(&local.repo)?;
        self.git.pull_ff_only(&local.repo)
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
                    } = render(&file.source, &source, &vars, &resolver)?;
                    rendered.push(RenderedFile {
                        file: file.clone(),
                        content,
                        contains_secrets,
                    });
                }
                FileMode::Copy => rendered.push(RenderedFile {
                    file: file.clone(),
                    content: self.fs.read(&file.source)?,
                    contains_secrets: false,
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
        rendered.push(RenderedFile {
            file: ResolvedFile {
                set: "(generated)".into(),
                source: local.repo.clone(),
                target: self.paths.zshrc(),
                mode: FileMode::Template,
            },
            content: zshrc::generate(&desired.fragments, self.fs, &vars, &resolver)?,
            contains_secrets: false,
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
        items.extend(files::diff(&rendered, &applied, self.fs)?);
        items.extend(symlink_drift);

        Ok(Inspection {
            desired,
            rendered,
            report: Report { items },
        })
    }
}
```

Add `pub mod engine;` to `crates/core/src/lib.rs`. Drop `SecretLookup` from the
import list if `clippy` reports it unused — `Resolver` is passed as a concrete
type here.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p dotfix-core engine && cargo clippy --all-targets -- -D warnings`
Expected: 4 PASS, no warnings.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "feat(core): add engine assembling desired state, rendering and drift"
```

---

## Task 12: Apply — plan and execute

**Files:**
- Create: `crates/core/src/apply.rs`
- Modify: `crates/core/src/lib.rs`
- Test: inline `#[cfg(test)]` in `apply.rs`

**Interfaces:**
- Consumes: `Inspection` (Task 11), `Drift` (Task 6), `Applied` (Task 5), ports (Task 3).
- Produces:
  - `Action` enum — `InstallPackage`, `UninstallPackage`, `WriteFile`, `Symlink`, `RemoveFile`
  - `Plan { actions: Vec<Action> }` with `is_empty()`, `describe() -> Vec<String>`
  - `plan(inspection: &Inspection) -> Plan`
  - `execute(plan: &Plan, engine: &Engine, stamp: &str) -> Result<Applied>`

The timestamp is passed in rather than read from the clock so `execute` stays
deterministic in tests.

- [ ] **Step 1: Write the failing tests**

`crates/core/src/apply.rs`:

```rust
#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::config::{FileMode, ResolvedFile};
    use crate::drift::files::RenderedFile;
    use crate::drift::{Drift, PackageRef, Report};
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
        }
    }

    fn inspection(items: Vec<Drift>, rendered: Vec<RenderedFile>) -> Inspection {
        Inspection {
            desired: Default::default(),
            rendered,
            report: Report { items },
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
            }],
            vec![],
        ));
        assert!(p.is_empty(), "a blocked removal must never be executed");
    }

    #[test]
    fn never_plans_anything_for_unmanaged_or_local_edits() {
        let p = plan(&inspection(
            vec![
                Drift::Unmanaged(PackageRef::formula("stray")),
                Drift::LocallyRemoved(PackageRef::formula("gone")),
                Drift::LocalEdit {
                    target: PathBuf::from("/Users/test/.rc"),
                    set: "core".into(),
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

        assert_eq!(fs.read(Path::new("/Users/test/.s3cfg")).unwrap(), "key = value");
        assert_eq!(fs.mode_of(Path::new("/Users/test/.s3cfg")), Some(0o600));
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

        let backup = PathBuf::from(
            "/Users/test/.local/state/dotfix/backups/20260916-101500/Users_test_.rc",
        );
        assert_eq!(fs.read(&backup).unwrap(), "old content");
        assert_eq!(fs.read(Path::new("/Users/test/.rc")).unwrap(), "new content");
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
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p dotfix-core apply`
Expected: FAIL — `cannot find function plan`.

- [ ] **Step 3: Implement plan and execute**

Prepend to `crates/core/src/apply.rs`:

```rust
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
    let mut actions = Vec::new();

    for item in &inspection.report.items {
        match item {
            Drift::IncomingPackage(p) => actions.push(Action::InstallPackage(p.clone())),

            Drift::RemovedPackage {
                package,
                blocked_by,
            } if blocked_by.is_empty() => {
                actions.push(Action::UninstallPackage(package.clone()))
            }

            Drift::IncomingFile { target, .. } => {
                if let Some(r) = inspection.rendered.iter().find(|r| r.file.target == *target) {
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

fn backup(
    engine: &Engine<'_>,
    paths: &Paths,
    target: &Path,
    stamp: &str,
    mode: u32,
) -> Result<()> {
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
```

Add `pub mod apply;` to `crates/core/src/lib.rs`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p dotfix-core apply`
Expected: 6 PASS.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(core): add apply planning and execution with backups"
```

---

## Task 13: Adopt — taking local state into the repository

**Files:**
- Create: `crates/core/src/adopt.rs`
- Modify: `crates/core/src/lib.rs`
- Test: inline `#[cfg(test)]` in `adopt.rs`

**Interfaces:**
- Consumes: `Drift` (Task 6), `Repo` (Task 2), `Fsys` (Task 3).
- Produces:
  - `DENY_PATTERNS: &[&str]`
  - `is_denied(path: &Path) -> bool`
  - `Proposal` enum — `AddPackage { package, set }`, `DropPackage { package, set }`, `IgnorePackage { package }`, `WriteBackFile { target, source }`, `RefuseFile { target, reason }`
  - `proposals_for(drift: &Drift, repo: &Repo, default_set: &str) -> Vec<Proposal>`
  - `apply_proposal(proposal: &Proposal, repo: &Repo, machine: &str, fs: &dyn Fsys) -> Result<()>`

- [ ] **Step 1: Write the failing tests**

`crates/core/src/adopt.rs`:

```rust
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
            ("/repo/sets/core/set.toml", "[packages]\nbrew = [\"alpha\"]\n"),
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
            &Drift::Unmanaged(PackageRef::formula("bravo")),
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
            },
            &repo,
            "core",
        );
        assert!(matches!(out[0], Proposal::RefuseFile { .. }));
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
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p dotfix-core adopt`
Expected: FAIL — `cannot find function is_denied`.

- [ ] **Step 3: Implement adopt**

Prepend to `crates/core/src/adopt.rs`:

```rust
use std::path::{Path, PathBuf};

use crate::config::Repo;
use crate::drift::{Drift, PackageRef};
use crate::error::{Error, Result};
use crate::ports::Fsys;

/// Filename shapes that almost always mean credentials. Matching one blocks
/// adoption — the most common way secrets reach a dotfiles repository is an
/// unconsidered "adopt everything".
pub const DENY_PATTERNS: &[&str] = &[
    "id_", ".pem", ".key", ".netrc", "credentials", ".s3cfg", "token", ".p12", ".keychain",
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
    AddPackage { package: PackageRef, set: String },
    DropPackage { package: PackageRef, set: String },
    IgnorePackage { package: PackageRef },
    WriteBackFile { target: PathBuf, source: PathBuf },
    RefuseFile { target: PathBuf, reason: String },
}

/// The choices a user is offered for one piece of drift.
pub fn proposals_for(drift: &Drift, repo: &Repo, default_set: &str) -> Vec<Proposal> {
    match drift {
        Drift::Unmanaged(package) => vec![
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

        Drift::LocalEdit { target, set } => {
            if is_denied(target) {
                return vec![Proposal::RefuseFile {
                    target: target.clone(),
                    reason: "looks like a credential file — add it as a template with \
                             `{{ secret(\"name\") }}` instead"
                        .to_string(),
                }];
            }
            let source = repo
                .sets
                .get(set)
                .and_then(|s| s.files.iter().find(|f| target.ends_with(trim_tilde(&f.target))))
                .map(|f| repo.root.join("sets").join(set).join(&f.source))
                .unwrap_or_else(|| repo.root.join("sets").join(set));
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
            write_toml(fs, &set_path(repo, set), &cfg)
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
            write_toml(fs, &set_path(repo, set), &cfg)
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
            write_toml(fs, &machine_path(repo, machine), &cfg)
        }

        Proposal::WriteBackFile { target, source } => {
            let content = fs.read(target)?;
            fs.write(source, &content, 0o644)
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

fn write_toml<T: serde::Serialize>(fs: &dyn Fsys, path: &Path, value: &T) -> Result<()> {
    let raw = toml::to_string_pretty(value)
        .map_err(|e| Error::Config(format!("serialising {}: {e}", path.display())))?;
    fs.write(path, &raw, 0o644)
}
```

Add `pub mod adopt;` to `crates/core/src/lib.rs`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p dotfix-core adopt`
Expected: 7 PASS.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(core): add adopt proposals with a credential deny list"
```

---

## Task 14: CLI skeleton, `status` and the status line

**Files:**
- Create: `crates/core/src/status_line.rs`
- Create: `crates/cli/src/ui.rs`, `crates/cli/src/cmd/mod.rs`, `crates/cli/src/cmd/status.rs`
- Modify: `crates/cli/src/main.rs`, `crates/core/src/lib.rs`
- Test: inline `#[cfg(test)]` in `status_line.rs`; `crates/cli/tests/status.rs`

**Interfaces:**
- Consumes: `Engine::inspect` (Task 11), `Report`/`Counts` (Task 6), `Paths` (Task 5).
- Produces:
  - `status_line::render(counts: &Counts) -> Option<String>` — `None` when there is nothing to report
  - CLI: `dotfix status [--json] [--write-status-line] [--no-sync]`
  - `ui::redact(text: &str, secrets: &[String]) -> String`

The shell hook prints this file verbatim. When there is no drift the file is
emptied, so the shell prints nothing at all.

- [ ] **Step 1: Write the failing tests for the status line**

`crates/core/src/status_line.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::drift::Counts;

    #[test]
    fn nothing_to_report_yields_no_line() {
        assert_eq!(render(&Counts::default()), None);
    }

    #[test]
    fn singular_and_plural_are_both_readable() {
        let one = render(&Counts {
            incoming: 1,
            ..Default::default()
        })
        .unwrap();
        assert_eq!(one, "↯ dotfix: 1 change   →  dotfix apply");

        let many = render(&Counts {
            incoming: 2,
            local_edits: 1,
            ..Default::default()
        })
        .unwrap();
        assert_eq!(many, "↯ dotfix: 2 changes · 1 changed config   →  dotfix apply");
    }

    #[test]
    fn unmanaged_only_points_at_adopt_instead_of_apply() {
        let line = render(&Counts {
            unmanaged: 3,
            ..Default::default()
        })
        .unwrap();
        assert_eq!(line, "↯ dotfix: 3 unmanaged   →  dotfix adopt");
    }

    #[test]
    fn the_line_is_a_single_line() {
        let line = render(&Counts {
            incoming: 1,
            unmanaged: 1,
            removed: 1,
            local_edits: 1,
        })
        .unwrap();
        assert!(!line.contains('\n'));
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p dotfix-core status_line`
Expected: FAIL — `cannot find function render`.

- [ ] **Step 3: Implement the status line**

Prepend to `crates/core/src/status_line.rs`:

```rust
use crate::drift::Counts;

/// One line for the shell hook, or `None` when everything is in sync.
///
/// Silence matters: a tool that greets every new terminal tab gets disabled
/// within a week.
pub fn render(counts: &Counts) -> Option<String> {
    let mut parts = Vec::new();

    if counts.incoming > 0 {
        parts.push(plural(counts.incoming, "change", "changes"));
    }
    if counts.removed > 0 {
        parts.push(format!("{} to remove", counts.removed));
    }
    if counts.local_edits > 0 {
        parts.push(plural(counts.local_edits, "changed config", "changed configs"));
    }
    if counts.unmanaged > 0 {
        parts.push(format!("{} unmanaged", counts.unmanaged));
    }

    if parts.is_empty() {
        return None;
    }

    // Only unmanaged drift means there is nothing to apply — point at adopt.
    let action = if counts.incoming == 0 && counts.removed == 0 && counts.local_edits == 0 {
        "dotfix adopt"
    } else {
        "dotfix apply"
    };

    Some(format!("↯ dotfix: {}   →  {action}", parts.join(" · ")))
}

fn plural(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        format!("{n} {one}")
    } else {
        format!("{n} {many}")
    }
}
```

Add `pub mod status_line;` to `crates/core/src/lib.rs`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p dotfix-core status_line`
Expected: 4 PASS.

- [ ] **Step 5: Write the failing CLI integration test**

`crates/cli/tests/status.rs`:

```rust
use std::fs;
use std::path::Path;

use assert_cmd::Command;

/// Build a minimal home + repository on disk and point dotfix at it via $HOME.
fn fixture(home: &Path) {
    let repo = home.join("dotfiles");
    fs::create_dir_all(repo.join("sets/core/shell")).unwrap();
    fs::create_dir_all(repo.join("machines")).unwrap();
    fs::create_dir_all(home.join(".config/dotfix")).unwrap();

    fs::write(repo.join("dotfix.toml"), "schema_version = 1\n").unwrap();
    fs::write(
        repo.join("sets/core/set.toml"),
        "[packages]\nbrew = [\"fake-pkg\"]\n",
    )
    .unwrap();
    fs::write(repo.join("sets/core/shell/10-x.zsh"), "export X=1\n").unwrap();
    fs::write(repo.join("machines/box-one.toml"), "sets = [\"core\"]\n").unwrap();
    fs::write(
        home.join(".config/dotfix/config.toml"),
        format!("repo = \"{}\"\nmachine = \"box-one\"\n", repo.display()),
    )
    .unwrap();
}

#[test]
fn status_json_lists_the_incoming_package() {
    let home = tempfile::tempdir().unwrap();
    fixture(home.path());

    let out = Command::cargo_bin("dotfix")
        .unwrap()
        .env("HOME", home.path())
        .env("DOTFIX_FAKE_BREW", "1")
        .args(["status", "--json", "--no-sync"])
        .output()
        .unwrap();

    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let classes: Vec<&str> = json["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["class"].as_str().unwrap())
        .collect();
    assert!(classes.contains(&"incoming_package"));
}

#[test]
fn write_status_line_creates_a_readable_one_liner() {
    let home = tempfile::tempdir().unwrap();
    fixture(home.path());

    Command::cargo_bin("dotfix")
        .unwrap()
        .env("HOME", home.path())
        .env("DOTFIX_FAKE_BREW", "1")
        .args(["status", "--write-status-line", "--no-sync"])
        .assert()
        .success();

    let line = fs::read_to_string(home.path().join(".local/state/dotfix/status.line")).unwrap();
    assert!(line.starts_with("↯ dotfix:"));
    assert!(!line.trim().is_empty());
}
```

`DOTFIX_FAKE_BREW=1` makes the binary substitute `FakeBrew` for `RealBrew` so
the test never touches the machine's real Homebrew. This is a test seam, not a
user-facing feature — document it as such in `main.rs`.

- [ ] **Step 6: Run the test to verify it fails**

Run: `cargo test -p dotfix --test status`
Expected: FAIL — unrecognised subcommand `status`.

- [ ] **Step 7: Implement the CLI**

`crates/cli/src/main.rs`:

```rust
mod cmd;
mod ui;

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use dotfix_core::paths::{LocalConfig, Paths};
use dotfix_core::ports::{Brew, Exec, Fsys, Git, RealBrew, RealExec, RealFsys, RealGit};

#[derive(Parser)]
#[command(name = "dotfix", version, about = "Keep macOS terminal setups in sync")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Show what differs between this machine and the repository
    Status {
        #[arg(long)]
        json: bool,
        /// Write the shell status line instead of printing a report
        #[arg(long)]
        write_status_line: bool,
        /// Skip the git fetch/pull
        #[arg(long)]
        no_sync: bool,
    },
}

/// Everything the commands need, constructed once.
pub struct Context_ {
    pub fs: Box<dyn Fsys>,
    pub brew: Box<dyn Brew>,
    pub git: Box<dyn Git>,
    pub exec: Box<dyn Exec>,
    pub paths: Paths,
    pub user: String,
    pub local: LocalConfig,
}

fn home() -> Result<PathBuf> {
    std::env::var("HOME")
        .map(PathBuf::from)
        .context("HOME is not set")
}

fn build_context() -> Result<Context_> {
    let home = home()?;
    let paths = Paths::new(home.clone());
    let fs = RealFsys;
    let local = LocalConfig::load(&fs, &paths.local_config()).with_context(|| {
        format!(
            "no local configuration at {} — run `dotfix init` first",
            paths.local_config().display()
        )
    })?;

    // Test seam: integration tests set DOTFIX_FAKE_BREW so they never touch the
    // machine's real Homebrew. Not a supported user-facing option.
    let brew: Box<dyn Brew> = if std::env::var("DOTFIX_FAKE_BREW").is_ok() {
        Box::new(dotfix_core::ports::fake::FakeBrew::new([], []))
    } else {
        Box::new(RealBrew)
    };

    Ok(Context_ {
        fs: Box::new(RealFsys),
        brew,
        git: Box::new(RealGit),
        exec: Box::new(RealExec),
        paths,
        user: std::env::var("USER").unwrap_or_else(|_| "unknown".into()),
        local,
    })
}

fn main() {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Status {
            json,
            write_status_line,
            no_sync,
        } => cmd::status::run(json, write_status_line, no_sync),
    };

    if let Err(err) = result {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}

pub use build_context as context;
```

`crates/cli/src/cmd/mod.rs`:

```rust
pub mod status;
```

`crates/cli/src/cmd/status.rs`:

```rust
use anyhow::Result;
use dotfix_core::engine::Engine;
use dotfix_core::status_line;

use crate::{context, ui};

pub fn run(json: bool, write_status_line: bool, no_sync: bool) -> Result<()> {
    let ctx = context()?;
    let engine = Engine {
        fs: ctx.fs.as_ref(),
        brew: ctx.brew.as_ref(),
        git: ctx.git.as_ref(),
        exec: ctx.exec.as_ref(),
        paths: ctx.paths.clone(),
        user: ctx.user.clone(),
    };

    if !no_sync {
        // A background run must stay silent when the network is down.
        if let Err(err) = engine.sync(&ctx.local) {
            if write_status_line {
                return Ok(());
            }
            eprintln!("warning: could not sync repository: {err}");
        }
    }

    let inspection = engine.inspect(&ctx.local)?;

    if write_status_line {
        let line = status_line::render(&inspection.report.counts()).unwrap_or_default();
        ctx.fs
            .write(&ctx.paths.status_line(), &line, 0o644)?;
        return Ok(());
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&inspection.report)?);
    } else {
        ui::print_report(&inspection.report);
    }

    Ok(())
}
```

`crates/cli/src/ui.rs`:

```rust
use dotfix_core::drift::{Drift, Report};

/// Replace every known secret value with a marker. Applied to anything that
/// could contain rendered file content.
pub fn redact(text: &str, secrets: &[String]) -> String {
    let mut out = text.to_string();
    for value in secrets {
        if !value.is_empty() {
            out = out.replace(value, "«redacted»");
        }
    }
    out
}

pub fn print_report(report: &Report) {
    if report.is_empty() {
        println!("everything in sync");
        return;
    }

    for item in &report.items {
        match item {
            Drift::IncomingPackage(p) => println!("  + {}  (install)", p.name),
            Drift::IncomingFile { target, set } => {
                println!("  + {}  ({set})", target.display())
            }
            Drift::LocallyRemoved(p) => {
                println!("  ? {}  (removed here — drop from set or reinstall)", p.name)
            }
            Drift::Unmanaged(p) => println!("  ? {}  (not in any set)", p.name),
            Drift::RemovedPackage {
                package,
                blocked_by,
            } if blocked_by.is_empty() => println!("  - {}  (uninstall)", package.name),
            Drift::RemovedPackage {
                package,
                blocked_by,
            } => println!(
                "  - {}  (skipped, still required by {})",
                package.name,
                blocked_by.join(", ")
            ),
            Drift::RemovedFile { target } => println!("  - {}", target.display()),
            Drift::LocalEdit { target, set } => {
                println!("  ~ {}  ({set} — edited locally)", target.display())
            }
        }
    }
}
```

- [ ] **Step 8: Run the tests to verify they pass**

Run: `cargo test --workspace && cargo clippy --all-targets -- -D warnings`
Expected: all PASS.

- [ ] **Step 9: Commit**

```bash
git add -A
git commit -m "feat(cli): add status command with json output and status line"
```

---

## Task 15: `dotfix apply`

**Files:**
- Create: `crates/cli/src/cmd/apply.rs`
- Modify: `crates/cli/src/main.rs`, `crates/cli/src/cmd/mod.rs`, `crates/cli/src/ui.rs`
- Test: `crates/cli/tests/apply.rs`

**Interfaces:**
- Consumes: `plan`/`execute` (Task 12), `Engine` (Task 11).
- Produces: `dotfix apply [--yes] [--dry-run] [--no-sync]`, `ui::confirm(prompt) -> Result<bool>`, `timestamp() -> String`.

- [ ] **Step 1: Write the failing integration test**

`crates/cli/tests/apply.rs`:

```rust
use std::fs;
use std::path::Path;

use assert_cmd::Command;

fn fixture(home: &Path) {
    let repo = home.join("dotfiles");
    fs::create_dir_all(repo.join("sets/core/files")).unwrap();
    fs::create_dir_all(repo.join("sets/core/shell")).unwrap();
    fs::create_dir_all(repo.join("machines")).unwrap();
    fs::create_dir_all(home.join(".config/dotfix")).unwrap();

    fs::write(repo.join("dotfix.toml"), "schema_version = 1\n").unwrap();
    fs::write(
        repo.join("sets/core/set.toml"),
        "[[files]]\nsource = \"files/rc.tmpl\"\ntarget = \"~/.rc\"\n",
    )
    .unwrap();
    fs::write(repo.join("sets/core/files/rc.tmpl"), "home={{ home }}\n").unwrap();
    fs::write(repo.join("sets/core/shell/10-x.zsh"), "export X=1\n").unwrap();
    fs::write(repo.join("machines/box-one.toml"), "sets = [\"core\"]\n").unwrap();
    fs::write(
        home.join(".config/dotfix/config.toml"),
        format!("repo = \"{}\"\nmachine = \"box-one\"\n", repo.display()),
    )
    .unwrap();
}

#[test]
fn dry_run_changes_nothing() {
    let home = tempfile::tempdir().unwrap();
    fixture(home.path());

    Command::cargo_bin("dotfix")
        .unwrap()
        .env("HOME", home.path())
        .env("DOTFIX_FAKE_BREW", "1")
        .args(["apply", "--dry-run", "--no-sync"])
        .assert()
        .success();

    assert!(!home.path().join(".rc").exists());
}

#[test]
fn apply_writes_files_and_records_applied_state() {
    let home = tempfile::tempdir().unwrap();
    fixture(home.path());

    Command::cargo_bin("dotfix")
        .unwrap()
        .env("HOME", home.path())
        .env("DOTFIX_FAKE_BREW", "1")
        .args(["apply", "--yes", "--no-sync"])
        .assert()
        .success();

    let rc = fs::read_to_string(home.path().join(".rc")).unwrap();
    assert_eq!(rc, format!("home={}\n", home.path().display()));

    let zshrc = fs::read_to_string(home.path().join(".zshrc")).unwrap();
    assert!(zshrc.contains("export X=1"));
    assert!(zshrc.contains("source ~/.zshrc.local"));

    assert!(home.path().join(".local/state/dotfix/applied.json").exists());
}

#[test]
fn a_second_apply_is_a_no_op() {
    let home = tempfile::tempdir().unwrap();
    fixture(home.path());

    for _ in 0..2 {
        Command::cargo_bin("dotfix")
            .unwrap()
            .env("HOME", home.path())
            .env("DOTFIX_FAKE_BREW", "1")
            .args(["apply", "--yes", "--no-sync"])
            .assert()
            .success();
    }

    let backups = home.path().join(".local/state/dotfix/backups");
    let count = fs::read_dir(&backups).map(|d| d.count()).unwrap_or(0);
    assert_eq!(count, 0, "an idempotent second run must not back anything up");
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p dotfix --test apply`
Expected: FAIL — unrecognised subcommand `apply`.

- [ ] **Step 3: Implement the command**

`crates/cli/src/cmd/apply.rs`:

```rust
use anyhow::Result;
use dotfix_core::apply::{execute, plan};
use dotfix_core::engine::Engine;

use crate::{context, ui};

pub fn run(yes: bool, dry_run: bool, no_sync: bool) -> Result<()> {
    let ctx = context()?;
    let engine = Engine {
        fs: ctx.fs.as_ref(),
        brew: ctx.brew.as_ref(),
        git: ctx.git.as_ref(),
        exec: ctx.exec.as_ref(),
        paths: ctx.paths.clone(),
        user: ctx.user.clone(),
    };

    if !no_sync {
        engine.sync(&ctx.local)?;
    }

    let inspection = engine.inspect(&ctx.local)?;
    let plan = plan(&inspection);

    if plan.is_empty() {
        println!("nothing to apply");
        return Ok(());
    }

    println!("plan:");
    for line in plan.describe() {
        println!("  {line}");
    }

    if dry_run {
        return Ok(());
    }
    if !yes && !ui::confirm("apply these changes?")? {
        println!("aborted");
        return Ok(());
    }

    execute(&plan, &engine, &ui::timestamp())?;
    println!("applied {} change(s)", plan.actions.len());

    // Refresh the status line so the next shell reflects reality immediately.
    let after = engine.inspect(&ctx.local)?;
    let line = dotfix_core::status_line::render(&after.report.counts()).unwrap_or_default();
    ctx.fs.write(&ctx.paths.status_line(), &line, 0o644)?;

    Ok(())
}
```

Append to `crates/cli/src/ui.rs`:

```rust
use std::io::{self, Write};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn confirm(prompt: &str) -> anyhow::Result<bool> {
    print!("{prompt} [y/N] ");
    io::stdout().flush()?;
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    Ok(matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes"))
}

/// Backup directory name. Seconds since the epoch keeps it sortable and needs
/// no date library.
pub fn timestamp() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs}")
}
```

Add the `Apply` variant to the `Command` enum in `main.rs`:

```rust
    /// Apply incoming changes and removals
    Apply {
        #[arg(long)]
        yes: bool,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        no_sync: bool,
    },
```

and the matching dispatch arm:

```rust
        Command::Apply {
            yes,
            dry_run,
            no_sync,
        } => cmd::apply::run(yes, dry_run, no_sync),
```

Add `pub mod apply;` to `crates/cli/src/cmd/mod.rs`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --workspace`
Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(cli): add apply command with plan preview and confirmation"
```

---

## Task 16: `dotfix adopt` and `dotfix sets`

**Files:**
- Create: `crates/cli/src/cmd/adopt.rs`, `crates/cli/src/cmd/sets.rs`
- Modify: `crates/cli/src/main.rs`, `crates/cli/src/cmd/mod.rs`
- Test: `crates/cli/tests/adopt.rs`

**Interfaces:**
- Consumes: `proposals_for`/`apply_proposal` (Task 13), `Engine` (Task 11).
- Produces: `dotfix adopt [--set <name>] [--yes] [--no-sync]`, `dotfix sets [--enable <name>] [--disable <name>]`.

- [ ] **Step 1: Write the failing integration test**

`crates/cli/tests/adopt.rs`:

```rust
use std::fs;
use std::path::Path;

use assert_cmd::Command;

fn fixture(home: &Path) {
    let repo = home.join("dotfiles");
    fs::create_dir_all(repo.join("sets/core/shell")).unwrap();
    fs::create_dir_all(repo.join("sets/extra")).unwrap();
    fs::create_dir_all(repo.join("machines")).unwrap();
    fs::create_dir_all(home.join(".config/dotfix")).unwrap();

    fs::write(repo.join("dotfix.toml"), "schema_version = 1\n").unwrap();
    fs::write(repo.join("sets/core/set.toml"), "[packages]\nbrew = []\n").unwrap();
    fs::write(repo.join("sets/core/shell/10-x.zsh"), "export X=1\n").unwrap();
    fs::write(repo.join("sets/extra/set.toml"), "[packages]\nbrew = []\n").unwrap();
    fs::write(repo.join("machines/box-one.toml"), "sets = [\"core\"]\n").unwrap();
    fs::write(
        home.join(".config/dotfix/config.toml"),
        format!("repo = \"{}\"\nmachine = \"box-one\"\n", repo.display()),
    )
    .unwrap();
}

#[test]
fn adopt_adds_an_unmanaged_package_to_the_chosen_set() {
    let home = tempfile::tempdir().unwrap();
    fixture(home.path());

    Command::cargo_bin("dotfix")
        .unwrap()
        .env("HOME", home.path())
        .env("DOTFIX_FAKE_BREW", "1")
        .env("DOTFIX_FAKE_LEAVES", "fake-pkg")
        .args(["adopt", "--set", "core", "--yes", "--no-sync"])
        .assert()
        .success();

    let set = fs::read_to_string(home.path().join("dotfiles/sets/core/set.toml")).unwrap();
    assert!(set.contains("fake-pkg"));
}

#[test]
fn sets_enable_writes_the_machine_file() {
    let home = tempfile::tempdir().unwrap();
    fixture(home.path());

    Command::cargo_bin("dotfix")
        .unwrap()
        .env("HOME", home.path())
        .env("DOTFIX_FAKE_BREW", "1")
        .args(["sets", "--enable", "extra"])
        .assert()
        .success();

    let machine = fs::read_to_string(home.path().join("dotfiles/machines/box-one.toml")).unwrap();
    assert!(machine.contains("extra"));
}
```

Extend the test seam in `main.rs` so `DOTFIX_FAKE_LEAVES` seeds the fake brew:

```rust
    let brew: Box<dyn Brew> = if std::env::var("DOTFIX_FAKE_BREW").is_ok() {
        let leaves: Vec<String> = std::env::var("DOTFIX_FAKE_LEAVES")
            .unwrap_or_default()
            .split(',')
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();
        Box::new(dotfix_core::ports::fake::FakeBrew::seeded(leaves, vec![]))
    } else {
        Box::new(RealBrew)
    };
```

and add the matching constructor to `FakeBrew` in `crates/core/src/ports/fake.rs`:

```rust
    pub fn seeded(leaves: Vec<String>, casks: Vec<String>) -> Self {
        Self {
            leaves,
            casks,
            ..Default::default()
        }
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p dotfix --test adopt`
Expected: FAIL — unrecognised subcommand `adopt`.

- [ ] **Step 3: Implement `adopt`**

`crates/cli/src/cmd/adopt.rs`:

```rust
use anyhow::{Result, bail};
use dotfix_core::adopt::{Proposal, apply_proposal, proposals_for};
use dotfix_core::config::Repo;
use dotfix_core::engine::Engine;

use crate::{context, ui};

pub fn run(set: Option<String>, yes: bool, no_sync: bool) -> Result<()> {
    let ctx = context()?;
    let engine = Engine {
        fs: ctx.fs.as_ref(),
        brew: ctx.brew.as_ref(),
        git: ctx.git.as_ref(),
        exec: ctx.exec.as_ref(),
        paths: ctx.paths.clone(),
        user: ctx.user.clone(),
    };

    if !no_sync {
        engine.sync(&ctx.local)?;
    }

    let inspection = engine.inspect(&ctx.local)?;
    let repo = Repo::load(ctx.fs.as_ref(), &ctx.local.repo)?;

    let default_set = match set {
        Some(s) if repo.sets.contains_key(&s) => s,
        Some(s) => bail!("unknown set `{s}`"),
        None => repo
            .machines
            .get(&ctx.local.machine)
            .and_then(|m| m.sets.first().cloned())
            .unwrap_or_else(|| "core".to_string()),
    };

    let mut accepted = 0usize;

    for drift in &inspection.report.items {
        for proposal in proposals_for(drift, &repo, &default_set) {
            let description = describe(&proposal);

            if let Proposal::RefuseFile { target, reason } = &proposal {
                println!("  refused {}: {reason}", target.display());
                continue;
            }

            if !yes && !ui::confirm(&format!("{description}?"))? {
                continue;
            }

            apply_proposal(&proposal, &repo, &ctx.local.machine, ctx.fs.as_ref())?;
            accepted += 1;
            // Stop after the first accepted proposal for this drift item: the
            // choices are alternatives, not a checklist.
            break;
        }
    }

    if accepted == 0 {
        println!("nothing adopted");
        return Ok(());
    }

    println!("adopted {accepted} item(s) — commit the repository when ready");
    Ok(())
}

fn describe(proposal: &Proposal) -> String {
    match proposal {
        Proposal::AddPackage { package, set } => format!("add {} to set `{set}`", package.name),
        Proposal::DropPackage { package, set } => {
            format!("drop {} from set `{set}`", package.name)
        }
        Proposal::IgnorePackage { package } => {
            format!("ignore {} on this machine", package.name)
        }
        Proposal::WriteBackFile { target, .. } => {
            format!("write {} back into the repository", target.display())
        }
        Proposal::RefuseFile { target, .. } => format!("refuse {}", target.display()),
    }
}
```

- [ ] **Step 4: Implement `sets`**

`crates/cli/src/cmd/sets.rs`:

```rust
use anyhow::{Result, bail};
use dotfix_core::config::Repo;

use crate::context;

pub fn run(enable: Option<String>, disable: Option<String>) -> Result<()> {
    let ctx = context()?;
    let repo = Repo::load(ctx.fs.as_ref(), &ctx.local.repo)?;

    let mut machine = repo
        .machines
        .get(&ctx.local.machine)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("unknown machine `{}`", ctx.local.machine))?;

    if enable.is_none() && disable.is_none() {
        for name in repo.sets.keys() {
            let mark = if machine.sets.contains(name) { "x" } else { " " };
            println!("  [{mark}] {name}");
        }
        return Ok(());
    }

    if let Some(name) = enable {
        if !repo.sets.contains_key(&name) {
            bail!("unknown set `{name}`");
        }
        if !machine.sets.contains(&name) {
            machine.sets.push(name);
        }
    }

    if let Some(name) = disable {
        machine.sets.retain(|s| s != &name);
    }

    let path = ctx
        .local
        .repo
        .join("machines")
        .join(format!("{}.toml", ctx.local.machine));
    ctx.fs
        .write(&path, &toml::to_string_pretty(&machine)?, 0o644)?;

    println!("updated {}", path.display());
    Ok(())
}
```

Add `toml = { workspace = true }` to `crates/cli/Cargo.toml`, register both
modules in `cmd/mod.rs`, and add the two subcommands plus dispatch arms to
`main.rs`:

```rust
    /// Take locally installed packages or edited configs into the repository
    Adopt {
        #[arg(long)]
        set: Option<String>,
        #[arg(long)]
        yes: bool,
        #[arg(long)]
        no_sync: bool,
    },
    /// List or toggle this machine's sets
    Sets {
        #[arg(long)]
        enable: Option<String>,
        #[arg(long)]
        disable: Option<String>,
    },
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --workspace && cargo clippy --all-targets -- -D warnings`
Expected: all PASS.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "feat(cli): add adopt and sets commands"
```

---

## Task 17: LaunchAgent and `dotfix doctor`

**Files:**
- Create: `crates/core/src/agent.rs`, `crates/core/src/doctor.rs`
- Create: `crates/cli/src/cmd/doctor.rs`
- Modify: `crates/core/src/lib.rs`, `crates/cli/src/main.rs`, `crates/cli/src/cmd/mod.rs`
- Test: inline `#[cfg(test)]` in `agent.rs` and `doctor.rs`

**Interfaces:**
- Consumes: `Paths` (Task 5), ports (Task 3, 8), `ProviderKind` (Task 2).
- Produces:
  - `agent::LABEL: &str`, `agent::plist(binary: &Path, interval: u32, path_env: &str) -> String`
  - `agent::install(fs, paths, binary, interval, path_env) -> Result<PathBuf>`
  - `doctor::Check { name, ok, detail }`, `doctor::run_checks(engine, local, provider) -> Vec<Check>`
  - CLI: `dotfix doctor [--install-agent]`

Both silent-failure modes from the spec are explicit checks here: a LaunchAgent
does not inherit the interactive `PATH`, and it may not reach the SSH key used
for the git remote.

- [ ] **Step 1: Write the failing tests for the plist**

`crates/core/src/agent.rs`:

```rust
#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::paths::Paths;
    use crate::ports::fake::FakeFsys;

    #[test]
    fn plist_runs_at_load_and_on_an_interval() {
        let out = plist(
            Path::new("/opt/homebrew/bin/dotfix"),
            3600,
            "/opt/homebrew/bin:/usr/bin:/bin",
        );
        assert!(out.contains("<key>RunAtLoad</key>"));
        assert!(out.contains("<key>StartInterval</key>"));
        assert!(out.contains("<integer>3600</integer>"));
    }

    #[test]
    fn plist_calls_status_with_write_status_line() {
        let out = plist(Path::new("/opt/homebrew/bin/dotfix"), 3600, "/usr/bin");
        assert!(out.contains("<string>status</string>"));
        assert!(out.contains("<string>--write-status-line</string>"));
    }

    #[test]
    fn plist_sets_path_because_launchd_does_not_inherit_it() {
        let out = plist(Path::new("/opt/homebrew/bin/dotfix"), 3600, "/opt/homebrew/bin:/usr/bin");
        assert!(out.contains("<key>EnvironmentVariables</key>"));
        assert!(out.contains("/opt/homebrew/bin:/usr/bin"));
    }

    #[test]
    fn install_writes_to_the_launch_agents_directory() {
        let fs = FakeFsys::new();
        let paths = Paths::new(PathBuf::from("/Users/test"));
        let written = install(&fs, &paths, Path::new("/opt/homebrew/bin/dotfix"), 3600, "/usr/bin")
            .unwrap();
        assert_eq!(
            written,
            PathBuf::from("/Users/test/Library/LaunchAgents/dev.noix.dotfix.plist")
        );
        assert!(fs.read(&written).unwrap().contains(LABEL));
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p dotfix-core agent`
Expected: FAIL — `cannot find function plist`.

- [ ] **Step 3: Implement the LaunchAgent**

Prepend to `crates/core/src/agent.rs`:

```rust
use std::path::{Path, PathBuf};

use crate::error::Result;
use crate::paths::Paths;
use crate::ports::Fsys;

pub const LABEL: &str = "dev.noix.dotfix";

/// A LaunchAgent runs at **login**, not at system boot. Boot-time execution
/// would need a LaunchDaemon running as root, which reaches neither the user
/// keychain nor Homebrew correctly.
pub fn plist(binary: &Path, interval: u32, path_env: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{LABEL}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{binary}</string>
        <string>status</string>
        <string>--write-status-line</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>StartInterval</key>
    <integer>{interval}</integer>
    <key>EnvironmentVariables</key>
    <dict>
        <key>PATH</key>
        <string>{path_env}</string>
    </dict>
    <key>ProcessType</key>
    <string>Background</string>
</dict>
</plist>
"#,
        binary = binary.display()
    )
}

pub fn install(
    fs: &dyn Fsys,
    paths: &Paths,
    binary: &Path,
    interval: u32,
    path_env: &str,
) -> Result<PathBuf> {
    let target = paths.launch_agent();
    fs.write(&target, &plist(binary, interval, path_env), 0o644)?;
    Ok(target)
}
```

- [ ] **Step 4: Write the failing tests for doctor**

`crates/core/src/doctor.rs`:

```rust
#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::config::ProviderKind;
    use crate::paths::Paths;
    use crate::ports::fake::{FakeExec, FakeFsys};

    fn paths() -> Paths {
        Paths::new(PathBuf::from("/Users/test"))
    }

    #[test]
    fn reports_a_missing_launch_agent() {
        let (fs, exec) = (FakeFsys::new(), FakeExec::new([]));
        let checks = run_checks(&fs, &exec, &paths(), ProviderKind::Keychain, None);
        let agent = checks.iter().find(|c| c.name == "launch agent").unwrap();
        assert!(!agent.ok);
    }

    #[test]
    fn reports_an_installed_launch_agent() {
        let fs = FakeFsys::from([(
            "/Users/test/Library/LaunchAgents/dev.noix.dotfix.plist",
            "<plist/>",
        )]);
        let exec = FakeExec::new([]);
        let checks = run_checks(&fs, &exec, &paths(), ProviderKind::Keychain, None);
        let agent = checks.iter().find(|c| c.name == "launch agent").unwrap();
        assert!(agent.ok);
    }

    #[test]
    fn checks_the_op_cli_only_for_onepassword_machines() {
        let (fs, exec) = (FakeFsys::new(), FakeExec::new([]));

        let keychain = run_checks(&fs, &exec, &paths(), ProviderKind::Keychain, None);
        assert!(!keychain.iter().any(|c| c.name == "1password cli"));

        let onepassword =
            run_checks(&fs, &exec, &paths(), ProviderKind::OnePassword, Some("Example"));
        assert!(onepassword.iter().any(|c| c.name == "1password cli"));
    }

    #[test]
    fn warns_when_ssh_config_does_not_persist_keys() {
        let fs = FakeFsys::from([("/Users/test/.ssh/config", "Host github.com\n")]);
        let exec = FakeExec::new([]);
        let checks = run_checks(&fs, &exec, &paths(), ProviderKind::Keychain, None);
        let ssh = checks.iter().find(|c| c.name == "ssh key access").unwrap();
        assert!(!ssh.ok);
        assert!(ssh.detail.contains("AddKeysToAgent"));
    }

    #[test]
    fn accepts_an_ssh_config_with_keychain_persistence() {
        let fs = FakeFsys::from([(
            "/Users/test/.ssh/config",
            "Host *\n  AddKeysToAgent yes\n  UseKeychain yes\n",
        )]);
        let exec = FakeExec::new([]);
        let checks = run_checks(&fs, &exec, &paths(), ProviderKind::Keychain, None);
        assert!(checks.iter().find(|c| c.name == "ssh key access").unwrap().ok);
    }
}
```

- [ ] **Step 5: Run the tests to verify they fail**

Run: `cargo test -p dotfix-core doctor`
Expected: FAIL — `cannot find function run_checks`.

- [ ] **Step 6: Implement doctor**

Prepend to `crates/core/src/doctor.rs`:

```rust
use crate::config::ProviderKind;
use crate::paths::Paths;
use crate::ports::{Exec, Fsys};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub name: &'static str,
    pub ok: bool,
    pub detail: String,
}

impl Check {
    fn ok(name: &'static str, detail: impl Into<String>) -> Self {
        Self {
            name,
            ok: true,
            detail: detail.into(),
        }
    }

    fn fail(name: &'static str, detail: impl Into<String>) -> Self {
        Self {
            name,
            ok: false,
            detail: detail.into(),
        }
    }
}

/// Everything that fails *silently* if misconfigured, checked loudly.
pub fn run_checks(
    fs: &dyn Fsys,
    exec: &dyn Exec,
    paths: &Paths,
    provider: ProviderKind,
    vault: Option<&str>,
) -> Vec<Check> {
    let mut checks = Vec::new();

    checks.push(match exec.run("brew", &["--version"]) {
        Ok(v) => Check::ok("homebrew", v.lines().next().unwrap_or("").to_string()),
        Err(e) => Check::fail("homebrew", e.to_string()),
    });

    checks.push(match exec.run("git", &["--version"]) {
        Ok(v) => Check::ok("git", v.trim().to_string()),
        Err(e) => Check::fail("git", e.to_string()),
    });

    checks.push(if fs.exists(&paths.launch_agent()) {
        Check::ok("launch agent", paths.launch_agent().display().to_string())
    } else {
        Check::fail(
            "launch agent",
            "not installed — run `dotfix doctor --install-agent`",
        )
    });

    // A LaunchAgent inherits neither the interactive PATH nor, without this,
    // the SSH key the git remote needs. Both fail without any visible error.
    let ssh_config = paths.home.join(".ssh/config");
    checks.push(match fs.read(&ssh_config) {
        Ok(contents)
            if contents.contains("AddKeysToAgent") && contents.contains("UseKeychain") =>
        {
            Check::ok("ssh key access", "AddKeysToAgent and UseKeychain are set")
        }
        Ok(_) => Check::fail(
            "ssh key access",
            "add `AddKeysToAgent yes` and `UseKeychain yes` to ~/.ssh/config, \
             otherwise the background check cannot reach the remote",
        ),
        Err(_) => Check::fail(
            "ssh key access",
            "no ~/.ssh/config — the background check may not reach the remote",
        ),
    });

    match provider {
        ProviderKind::Keychain => {
            checks.push(Check::ok("secret provider", "macOS Keychain"));
        }
        ProviderKind::OnePassword => {
            checks.push(match exec.run("op", &["--version"]) {
                Ok(v) => Check::ok("1password cli", v.trim().to_string()),
                Err(e) => Check::fail("1password cli", e.to_string()),
            });
            checks.push(match vault {
                Some(v) => Check::ok("1password vault", v.to_string()),
                None => Check::fail("1password vault", "no `vault` set for this machine"),
            });
        }
        ProviderKind::Age => {
            checks.push(match exec.run("age", &["--version"]) {
                Ok(v) => Check::ok("age", v.trim().to_string()),
                Err(e) => Check::fail("age", e.to_string()),
            });
            let identity = paths.home.join(".config/dotfix/age.key");
            checks.push(if fs.exists(&identity) {
                Check::ok("age identity", identity.display().to_string())
            } else {
                Check::fail("age identity", format!("missing {}", identity.display()))
            });
        }
    }

    checks
}
```

Add `pub mod agent;` and `pub mod doctor;` to `crates/core/src/lib.rs`.

- [ ] **Step 7: Implement the CLI command**

`crates/cli/src/cmd/doctor.rs`:

```rust
use anyhow::Result;
use dotfix_core::config::Repo;
use dotfix_core::{agent, doctor};

use crate::context;

const DEFAULT_INTERVAL: u32 = 3600;

pub fn run(install_agent: bool) -> Result<()> {
    let ctx = context()?;

    if install_agent {
        let binary = std::env::current_exe()?;
        let path_env = std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into());
        let written = agent::install(
            ctx.fs.as_ref(),
            &ctx.paths,
            &binary,
            DEFAULT_INTERVAL,
            &path_env,
        )?;
        println!("installed {}", written.display());
        println!("run: launchctl bootstrap gui/$(id -u) {}", written.display());
    }

    let repo = Repo::load(ctx.fs.as_ref(), &ctx.local.repo)?;
    let machine = repo.machines.get(&ctx.local.machine);

    let checks = doctor::run_checks(
        ctx.fs.as_ref(),
        ctx.exec.as_ref(),
        &ctx.paths,
        machine.map(|m| m.secret_provider).unwrap_or_default(),
        machine.and_then(|m| m.vault.as_deref()),
    );

    let mut failed = 0;
    for check in &checks {
        let mark = if check.ok { "ok  " } else { "FAIL" };
        println!("  [{mark}] {:<18} {}", check.name, check.detail);
        if !check.ok {
            failed += 1;
        }
    }

    if failed > 0 {
        println!("\n{failed} check(s) failed");
        std::process::exit(1);
    }
    Ok(())
}
```

Register the module and add the subcommand to `main.rs`:

```rust
    /// Verify the local setup
    Doctor {
        /// Write the LaunchAgent plist
        #[arg(long)]
        install_agent: bool,
    },
```

- [ ] **Step 8: Run the tests to verify they pass**

Run: `cargo test --workspace && cargo clippy --all-targets -- -D warnings`
Expected: all PASS.

- [ ] **Step 9: Commit**

```bash
git add -A
git commit -m "feat: add launch agent installation and doctor checks"
```

---

## Task 18: `dotfix init`

**Files:**
- Create: `crates/cli/src/cmd/init.rs`
- Modify: `crates/cli/src/main.rs`, `crates/cli/src/cmd/mod.rs`
- Test: `crates/cli/tests/init.rs`

**Interfaces:**
- Consumes: `LocalConfig`/`Paths` (Task 5), `agent::install` (Task 17), `Repo` (Task 2), `Brew` (Task 3).
- Produces: `dotfix init [--repo <url>] [--machine <name>] [--set-up-new] [--yes]`.

Two paths, as decided in the design: **set up new** (no repository yet — scaffold
and propose a set split from what is installed) and **sync from existing** (clone
and select sets). `init` is the only command that runs without a `LocalConfig`.

- [ ] **Step 1: Write the failing integration test**

`crates/cli/tests/init.rs`:

```rust
use std::fs;

use assert_cmd::Command;

#[test]
fn set_up_new_scaffolds_a_repository_and_local_config() {
    let home = tempfile::tempdir().unwrap();

    Command::cargo_bin("dotfix")
        .unwrap()
        .env("HOME", home.path())
        .env("DOTFIX_FAKE_BREW", "1")
        .env("DOTFIX_FAKE_LEAVES", "fake-pkg,other-pkg")
        .args(["init", "--set-up-new", "--machine", "box-one", "--yes"])
        .assert()
        .success();

    let repo = home.path().join("dotfiles");
    assert!(repo.join("dotfix.toml").exists());
    assert!(repo.join("sets/core/set.toml").exists());
    assert!(repo.join("machines/box-one.toml").exists());

    let core = fs::read_to_string(repo.join("sets/core/set.toml")).unwrap();
    assert!(core.contains("fake-pkg"), "installed packages become a proposal");

    let local = fs::read_to_string(home.path().join(".config/dotfix/config.toml")).unwrap();
    assert!(local.contains("box-one"));

    assert!(
        home.path()
            .join("Library/LaunchAgents/dev.noix.dotfix.plist")
            .exists()
    );
}

#[test]
fn init_refuses_to_overwrite_an_existing_local_config() {
    let home = tempfile::tempdir().unwrap();
    fs::create_dir_all(home.path().join(".config/dotfix")).unwrap();
    fs::write(
        home.path().join(".config/dotfix/config.toml"),
        "repo = \"/somewhere\"\nmachine = \"existing\"\n",
    )
    .unwrap();

    Command::cargo_bin("dotfix")
        .unwrap()
        .env("HOME", home.path())
        .env("DOTFIX_FAKE_BREW", "1")
        .args(["init", "--set-up-new", "--machine", "box-two", "--yes"])
        .assert()
        .failure();
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p dotfix --test init`
Expected: FAIL — unrecognised subcommand `init`.

- [ ] **Step 3: Implement `init`**

`crates/cli/src/cmd/init.rs`:

```rust
use std::path::PathBuf;

use anyhow::{Result, bail};
use dotfix_core::config::{MachineConfig, Packages, RepoConfig, SetConfig};
use dotfix_core::paths::{LocalConfig, Paths};
use dotfix_core::ports::{Brew, Exec, Fsys, Git, RealBrew, RealExec, RealFsys, RealGit};
use dotfix_core::{agent, ports};

const DEFAULT_INTERVAL: u32 = 3600;

pub fn run(
    repo_url: Option<String>,
    machine: Option<String>,
    set_up_new: bool,
    yes: bool,
) -> Result<()> {
    let home = PathBuf::from(std::env::var("HOME")?);
    let paths = Paths::new(home.clone());
    let fs = RealFsys;

    if fs.exists(&paths.local_config()) {
        bail!(
            "{} already exists — dotfix is already set up on this machine",
            paths.local_config().display()
        );
    }

    let machine = match machine {
        Some(m) => m,
        None => crate::ui::prompt("machine name")?,
    };

    let brew: Box<dyn Brew> = if std::env::var("DOTFIX_FAKE_BREW").is_ok() {
        let leaves: Vec<String> = std::env::var("DOTFIX_FAKE_LEAVES")
            .unwrap_or_default()
            .split(',')
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();
        Box::new(ports::fake::FakeBrew::seeded(leaves, vec![]))
    } else {
        Box::new(RealBrew)
    };

    let repo_root = if set_up_new {
        let root = home.join("dotfiles");
        scaffold(&fs, &root, &machine, brew.as_ref(), yes)?;
        root
    } else {
        let url = match repo_url {
            Some(u) => u,
            None => crate::ui::prompt("repository url")?,
        };
        let root = home.join("dotfiles");
        RealGit.clone_to(&url, &root)?;
        root
    };

    LocalConfig {
        repo: repo_root.clone(),
        machine: machine.clone(),
    }
    .save(&fs, &paths.local_config())?;

    let binary = std::env::current_exe()?;
    let path_env = std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into());
    agent::install(&fs, &paths, &binary, DEFAULT_INTERVAL, &path_env)?;

    println!("dotfix is set up for machine `{machine}`");
    println!("repository: {}", repo_root.display());
    println!("next: review the proposed sets, then run `dotfix apply`");

    let _exec: Box<dyn Exec> = Box::new(RealExec);
    Ok(())
}

/// Path A: create a repository skeleton and propose a first set split from the
/// packages that are already installed. The user corrects it afterwards.
fn scaffold(
    fs: &dyn Fsys,
    root: &PathBuf,
    machine: &str,
    brew: &dyn Brew,
    _yes: bool,
) -> Result<()> {
    fs.write(
        &root.join("dotfix.toml"),
        &toml::to_string_pretty(&RepoConfig { schema_version: 1 })?,
        0o644,
    )?;

    let core = SetConfig {
        description: "Base set, active on every machine".into(),
        packages: Packages {
            brew: brew.leaves()?,
            cask: brew.casks()?,
        },
        files: Vec::new(),
    };
    fs.write(
        &root.join("sets/core/set.toml"),
        &toml::to_string_pretty(&core)?,
        0o644,
    )?;

    // Placeholder fragment so the generated .zshrc is never empty.
    fs.write(
        &root.join("sets/core/shell/10-path.zsh"),
        "export PATH=\"{{ home }}/.local/bin:$PATH\"\n",
        0o644,
    )?;

    let cfg = MachineConfig {
        sets: vec!["core".into()],
        ..Default::default()
    };
    fs.write(
        &root.join("machines")
            .join(format!("{machine}.toml")),
        &toml::to_string_pretty(&cfg)?,
        0o644,
    )?;

    fs.write(
        &root.join(".gitignore"),
        "# never commit decrypted secrets\n*.decrypted\n",
        0o644,
    )?;

    println!(
        "scaffolded {} — everything installed went into set `core`; split it up with `dotfix sets` and by editing sets/",
        root.display()
    );
    Ok(())
}
```

Append the prompt helper to `crates/cli/src/ui.rs`:

```rust
pub fn prompt(label: &str) -> anyhow::Result<String> {
    print!("{label}: ");
    io::stdout().flush()?;
    let mut value = String::new();
    io::stdin().read_line(&mut value)?;
    Ok(value.trim().to_string())
}
```

Add the subcommand to `main.rs`, and make sure `init` is dispatched **before**
`build_context()` runs — it is the one command that must work without a local
configuration:

```rust
    /// Set up dotfix on this machine
    Init {
        /// Clone this repository (path B)
        #[arg(long)]
        repo: Option<String>,
        #[arg(long)]
        machine: Option<String>,
        /// Create a new data repository from what is installed here (path A)
        #[arg(long)]
        set_up_new: bool,
        #[arg(long)]
        yes: bool,
    },
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --workspace && cargo clippy --all-targets -- -D warnings`
Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(cli): add init with new-repository and clone paths"
```

---

## Task 19: Installer, Homebrew tap, release workflow and README

**Files:**
- Create: `install.sh`, `.github/workflows/release-cli.yml`, `README.md`
- Create (separate repository): `NoiXdev/homebrew-tap/Formula/dotfix.rb`
- Test: `crates/cli/tests/shell_hook.rs` (verifies the hook fragment under real zsh)

**Interfaces:**
- Consumes: the finished binary.
- Produces: an installable release and the documented bootstrap.

- [ ] **Step 1: Write the failing test for the shell hook**

The fragment printed into `.zshrc` must cost nothing at shell start and must
print nothing when there is no drift. Test it against the real `zsh`.

`crates/cli/tests/shell_hook.rs`:

```rust
use std::fs;
use std::process::Command;

const HOOK: &str = r#"() {
  local f=${XDG_STATE_HOME:-$HOME/.local/state}/dotfix/status.line
  [[ -s $f ]] && print -r -- "$(<$f)"
}"#;

fn run_hook(home: &std::path::Path) -> String {
    let out = Command::new("zsh")
        .arg("-c")
        .arg(HOOK)
        .env("HOME", home)
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).to_string()
}

#[test]
fn prints_nothing_when_there_is_no_status_file() {
    let home = tempfile::tempdir().unwrap();
    assert_eq!(run_hook(home.path()), "");
}

#[test]
fn prints_nothing_when_the_status_file_is_empty() {
    let home = tempfile::tempdir().unwrap();
    fs::create_dir_all(home.path().join(".local/state/dotfix")).unwrap();
    fs::write(home.path().join(".local/state/dotfix/status.line"), "").unwrap();
    assert_eq!(run_hook(home.path()), "");
}

#[test]
fn prints_the_line_when_there_is_drift() {
    let home = tempfile::tempdir().unwrap();
    fs::create_dir_all(home.path().join(".local/state/dotfix")).unwrap();
    fs::write(
        home.path().join(".local/state/dotfix/status.line"),
        "↯ dotfix: 2 changes   →  dotfix apply",
    )
    .unwrap();
    assert_eq!(
        run_hook(home.path()).trim(),
        "↯ dotfix: 2 changes   →  dotfix apply"
    );
}
```

- [ ] **Step 2: Run the tests to verify they pass**

Run: `cargo test -p dotfix --test shell_hook`
Expected: 3 PASS. (These pass immediately — the hook is a literal in the test.
Their job is to lock the behaviour before it is copied into `init`'s scaffold.)

- [ ] **Step 3: Ship the hook as part of the scaffold**

Modify `scaffold` in `crates/cli/src/cmd/init.rs` to write the fragment, so a
new repository has it from the start:

```rust
    fs.write(
        &root.join("sets/core/shell/99-dotfix-status.zsh"),
        "# printed by dotfix when the background check found something\n\
         () {\n\
         \x20 local f=${XDG_STATE_HOME:-$HOME/.local/state}/dotfix/status.line\n\
         \x20 [[ -s $f ]] && print -r -- \"$(<$f)\"\n\
         }\n",
        0o644,
    )?;
```

- [ ] **Step 4: Write the installer**

`install.sh` — pinned to a release tag by the URL used to fetch it, never to
`main`:

```bash
#!/usr/bin/env bash
# Bootstrap dotfix on a fresh Mac.
#   /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/NoiXdev/dotfix/v0.1.0/install.sh)"
set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "dotfix is macOS only" >&2
  exit 1
fi

if ! command -v brew >/dev/null 2>&1; then
  echo "==> installing Homebrew"
  /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
  if [[ -x /opt/homebrew/bin/brew ]]; then
    eval "$(/opt/homebrew/bin/brew shellenv)"
  fi
fi

echo "==> installing dotfix"
brew tap NoiXdev/tap
brew install dotfix

cat <<'EOF'

dotfix is installed. Next:

  dotfix init                  # clone an existing data repository
  dotfix init --set-up-new     # create one from this machine

EOF
```

- [ ] **Step 5: Write the release workflow**

`.github/workflows/release-cli.yml`:

```yaml
name: Release CLI

on:
  push:
    tags: ["v*"]

permissions:
  contents: write

jobs:
  release:
    runs-on: macos-latest
    steps:
      - uses: actions/checkout@v5
      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: aarch64-apple-darwin,x86_64-apple-darwin
      - uses: Swatinem/rust-cache@v2

      - name: Build both architectures
        run: |
          cargo build --release -p dotfix --target aarch64-apple-darwin
          cargo build --release -p dotfix --target x86_64-apple-darwin

      - name: Create a universal binary
        run: |
          mkdir -p dist
          lipo -create -output dist/dotfix \
            target/aarch64-apple-darwin/release/dotfix \
            target/x86_64-apple-darwin/release/dotfix
          tar -czf "dotfix-${GITHUB_REF_NAME}-macos-universal.tar.gz" -C dist dotfix
          shasum -a 256 "dotfix-${GITHUB_REF_NAME}-macos-universal.tar.gz" | tee checksum.txt

      - uses: softprops/action-gh-release@v2
        with:
          files: |
            dotfix-*-macos-universal.tar.gz
            checksum.txt

      - name: Bump the tap formula
        env:
          GH_TOKEN: ${{ secrets.TAP_TOKEN }}
        run: |
          SHA=$(cut -d' ' -f1 checksum.txt)
          gh workflow run bump.yml --repo NoiXdev/homebrew-tap \
            -f formula=dotfix -f version="${GITHUB_REF_NAME#v}" -f sha256="$SHA"
```

`TAP_TOKEN` is a fine-grained PAT with write access to `NoiXdev/homebrew-tap`
only. Document that in the README so a future maintainer knows why it exists.

- [ ] **Step 6: Add the tap formula**

In `NoiXdev/homebrew-tap`, `Formula/dotfix.rb`:

```ruby
class Dotfix < Formula
  desc "Keep macOS terminal setups in sync across machines"
  homepage "https://github.com/NoiXdev/dotfix"
  version "0.1.0"
  url "https://github.com/NoiXdev/dotfix/releases/download/v#{version}/dotfix-v#{version}-macos-universal.tar.gz"
  sha256 "REPLACE_ON_FIRST_RELEASE"
  license "MIT"

  depends_on :macos

  def install
    bin.install "dotfix"
  end

  test do
    assert_match "dotfix", shell_output("#{bin}/dotfix --version")
  end
end
```

The `sha256` placeholder is filled by the first release run; it is the one value
that cannot exist before the artefact does.

- [ ] **Step 7: Write the README**

`README.md` — public-facing, so it describes only dotfix itself:

````markdown
# dotfix

Keep several Macs in sync: Homebrew packages, shell configuration and
application config files, grouped into **sets** that each machine activates as
it needs them.

## Install

```bash
/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/NoiXdev/dotfix/v0.1.0/install.sh)"
dotfix init --set-up-new      # first machine
dotfix init                   # every machine after that
```

## How it works

Your configuration lives in a **private** git repository of your own; dotfix
itself stores nothing about you. A set is a directory with a package list,
shell fragments and managed files:

```
sets/web/
├── set.toml
├── shell/20-web.zsh
└── files/example.tmpl
```

Each machine picks its sets in `machines/<name>.toml`. `.zshrc` is generated
from the fragments of the active sets, so turning a set off removes its lines.

## Commands

| Command | Purpose |
|---|---|
| `dotfix status` | what differs (never writes) |
| `dotfix apply` | apply incoming changes, after confirmation |
| `dotfix adopt` | take locally installed packages or edited configs into the repository |
| `dotfix sets` | list or toggle this machine's sets |
| `dotfix doctor` | verify the setup |

## Secrets

Secrets never enter the repository. A set refers to a logical name:

```
access_key = {{ secret("s3_access_key") }}
```

and each machine says where that name lives — macOS Keychain (default),
1Password, or an `age`-encrypted file. `dotfix adopt` refuses files that look
like credentials and proposes a template instead.

## Background check

A LaunchAgent runs at login and hourly, writing a single status line that your
shell prints at startup. When nothing differs, it prints nothing.

## Development

```bash
cargo test --workspace
cargo clippy --all-targets -- -D warnings
```

Releases are cut by pushing a `v*` tag. The workflow builds a universal binary,
attaches it to the release and bumps the Homebrew formula in
`NoiXdev/homebrew-tap` (using the `TAP_TOKEN` secret, a fine-grained PAT scoped
to that repository).
````

- [ ] **Step 8: Verify the whole suite one last time**

Run: `cargo test --workspace && cargo fmt --check && cargo clippy --all-targets -- -D warnings`
Expected: all PASS.

- [ ] **Step 9: Commit**

```bash
git add -A
git commit -m "build: add installer, release workflow, tap formula and README"
```

---

## Follow-up outside this repository

Not a task in `dotfix`, but required before phase 2 and recorded here so it is
not forgotten:

- **`NoiXdev/github-workflows`** — add a `platforms` input to
  `create_tauri_changelog_version_release.yaml`. It currently builds macOS,
  Windows and Linux unconditionally; `dotfix` is macOS-only and the other two
  jobs would fail. Extending the shared workflow (rather than forking it into
  `dotfix`) matches that repository's own rule that shared workflows must stay
  genuinely reusable.
- **`NoiXdev/homebrew-tap`** — needs a `bump.yml` dispatch workflow accepting
  `formula`, `version` and `sha256`, which Task 19's release workflow calls.

---

## Plan self-review

**Spec coverage**

| Spec section | Task(s) |
|---|---|
| Two-repository boundary | 1, 19 (README), Global Constraints |
| Data model: sets, machines, `dotfix.toml` | 2 |
| Initial set split | 18 (`init --set-up-new` proposes it) |
| Templates, machine-independent paths | 7 |
| Generated `.zshrc`, fragment order, marker, checksum, `.zshrc.local` | 9 |
| Three states / three-way diff | 5 (state), 6 (packages), 10 (files) |
| Four drift classes (7 variants) | 6, 10, 11 |
| Leaf check before uninstall | 6, 12 |
| Commands `status`/`apply`/`adopt`/`sets`/`doctor`/`init` | 14, 15, 16, 17, 18 |
| `--json` on every command | 14 (`status`); the others print plans, not reports |
| No implicit merges | 3 (`pull_ff_only`), 11 (`sync`) |
| No write without backup | 12 |
| Plan before execution | 12, 15 |
| Shell hook, silent when clean | 14 (status line), 19 (fragment + zsh test) |
| LaunchAgent, login + hourly, PATH and SSH pitfalls | 17 |
| Secrets: logical name, per-machine location, three providers | 8, 11 |
| Redaction and `0600` | 12 (mode), 14 (`ui::redact`) |
| Adopt deny list | 13 |
| Bootstrap, both `init` paths, tag-pinned installer | 18, 19 |
| Formula and tap distribution | 19 |
| CI: fmt, clippy, test, commit lint | 1 |
| Testing strategy: fakes, integration, no network | 3, and every task thereafter |

**Gaps found and closed during review**

1. `--json` was specified for *every* command. Only `status` produces a report
   worth serialising; `apply`/`adopt` produce plans and prompts. Task 15 prints
   the plan as text. **Resolution:** acceptable for phase 1 — the machine-readable
   surface the app needs is `status --json`, and the app links `core` directly
   rather than parsing CLI output. Noted here so it is a decision, not an
   oversight.
2. `ui::redact` exists (Task 14) but no command currently prints rendered file
   content, so nothing calls it yet. It is needed the moment a config diff is
   shown — which is phase 2's "Configs" pane. **Resolution:** keep it, with the
   test; do not build a diff viewer in phase 1.
3. The spec says uninstall is proposed only for leaves. Task 6 detects the
   blockage and Task 12 refuses to plan it — verified both sides exist.

**Type consistency**

Checked across tasks: `PackageRef::{formula, cask}` (6) used identically in 12
and 13; `RenderedFile` (10) constructed in 11 and consumed in 12; `Applied.files`
keyed by `PathBuf` throughout (5, 10, 12); `checksum` (9) used by 10 and 12;
`Engine` field list identical in 11, 12, 14, 15, 16; `Paths::launch_agent` (5)
used by 17; `FakeBrew::seeded` added in 16 and reused in 18.

**Placeholder scan**

One intentional placeholder remains: `sha256 "REPLACE_ON_FIRST_RELEASE"` in the
tap formula, which cannot hold a real value before the first artefact exists.
Step 6 of Task 19 says so explicitly.

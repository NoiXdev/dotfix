# dotfix Phase 2 Implementation Plan — Menubar App

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Put a macOS menubar app on top of the Phase 1 engine: a tray glyph that shows at a glance whether anything drifted, and a window with the four drift areas plus a history view — no package search, no logic of its own.

**Architecture:** A third crate in the existing cargo workspace, `app/src-tauri`, linking `dotfix-core` **directly** — no subprocess, no parsing of CLI output, the same types the CLI uses. Everything the UI shows is derived in Rust by pure view-model functions that are unit-tested without a running Tauri app; the React frontend renders what it is handed and owns no decisions. Four small extensions land in `dotfix-core` first (stable drift ids, selective planning, set toggling, a redacted diff), because each is engine behaviour that the CLI can use too.

**Tech Stack:** Tauri v2 (`tray-icon`, `tauri-plugin-autostart`, `tauri-plugin-opener`), React 19 + TypeScript + Vite + Tailwind, vitest for the frontend, `cargo test` for Rust. Mirrors `notefix`, the closest sibling app in the organisation.

**Spec:** `docs/specs/2026-09-16-dotfix-design.md`

**Predecessor:** `docs/plans/2026-09-16-dotfix-phase-1.md` — complete, merged to `main` at `9d329e6`.

## Global Constraints

- **Toolchain:** Rust 1.96, edition 2024 for new crates (`notefix` uses 2021; do not copy that). Node 22+.
- **Platform:** macOS only. Do not add Windows/Linux targets to `tauri.conf.json`'s build matrix.
- **Identifier:** `dev.noix.dotfix` — matches the LaunchAgent label already shipped in Phase 1 (`dotfix_core::agent::LABEL`).
- **Repository boundary:** `NoiXdev/dotfix` is public-facing in intent (currently private). No personal data, no machine names, no internal infrastructure in code, fixtures or UI copy.
- **Language:** all code, comments, commit messages and UI strings in English.
- **Commits:** Conventional Commits; every commit ends with `Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>`.
- **Lints:** `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `npm run typecheck` and `npm test` must all pass.
- **TDD:** failing test first, confirmed failing, then implementation.
- **No logic in the frontend.** If the UI needs a decision made, the decision belongs in a tested Rust view-model function.
- **Secrets never reach the frontend.** Redaction happens in Rust. A test must assert that a known secret value cannot appear in any command's JSON output.
- **The CLI LaunchAgent stays.** The app's autostart is separate and additional; the background check must keep working when the app is closed or absent.

---

## Design decisions made while planning

### 1. Secret values must travel with the rendered file

Phase 1 left `dotfix_core::secrets::redact` without a consumer, and its
self-review flagged that the Configs pane would be the one to need it. Now it
does — and it cannot work as built: redaction needs the *resolved values*, which
only the renderer sees.

`Rendered` and `RenderedFile` therefore gain `secret_values: Vec<String>`.
Neither type is `Serialize`, so the values cannot leak by accident; the diff is
redacted in Rust and only the redacted result crosses into the webview. The
alternative — hiding diffs for secret-bearing files entirely — was rejected
because "this file changed, I may not see how" is exactly when a user reaches
for `cat` and defeats the protection.

### 2. Drift items need stable ids

The UI applies individual items, so each needs an id that survives a
re-inspection: `incoming_package:jq`, `local_edit:/Users/x/.gitconfig`. Ids are
derived, not stored. A command re-inspects and then filters by id rather than
trusting a snapshot the user might have been looking at for ten minutes — if
the world moved, the id simply no longer matches and the item is skipped.

### 3. Set toggling moves from the CLI into core

`crates/cli/src/cmd/sets.rs` mutates `MachineConfig` inline. The app needs the
same behaviour, so it moves to `dotfix_core::sets` and the CLI calls it. Two
call sites, one implementation, one set of tests.

---

## File Structure

### Additions to `crates/core` — engine behaviour the CLI can use too

| File | Responsibility |
|---|---|
| `src/drift/mod.rs` (modify) | `Drift::id()`, `Drift::target_label()` |
| `src/apply.rs` (modify) | `plan_selected(inspection, ids)` |
| `src/sets.rs` (new) | `toggle`, `active`, `save` — set membership per machine |
| `src/render/template.rs` (modify) | `Rendered.secret_values` |
| `src/drift/files.rs` (modify) | `RenderedFile.secret_values` |
| `src/diffview.rs` (new) | `DiffLine`, `FileDiff`, `unified()` — redacted line diff |

### `app/src-tauri` — the Rust side of the app

| File | Responsibility |
|---|---|
| `src/main.rs` | binary entry, delegates to `lib.rs` |
| `src/lib.rs` | Tauri builder, plugins, tray setup, activation policy |
| `src/ctx.rs` | builds an `Engine` from the real ports (mirrors `crates/cli/src/ctx.rs`) |
| `src/view.rs` | `Inspection` → view models. Pure, fully unit-tested |
| `src/commands.rs` | `#[tauri::command]` wrappers; no logic beyond calling `view`/core |
| `src/tray.rs` | tray icon + menu; glyph choice and menu model are pure functions |
| `capabilities/default.json` | permission set for the single window |
| `icons/` | generated app icons and tray template images |

### `app` — the frontend

| File | Responsibility |
|---|---|
| `src/types.ts` | TypeScript mirror of the view models |
| `src/api.ts` | typed `invoke()` wrappers, one per command |
| `src/App.tsx` | shell: area tabs, refresh, error banner |
| `src/areas/Changes.tsx` | incoming + removals, per-item and bulk apply |
| `src/areas/Unmanaged.tsx` | unmanaged + locally-removed, adopt or ignore |
| `src/areas/Configs.tsx` | locally edited files with a diff |
| `src/areas/Sets.tsx` | set toggles for this machine |
| `src/areas/History.tsx` | repository log |
| `src/components/*.tsx` | `Row`, `Badge`, `DiffView`, `Empty`, `Busy` |

### Repository root

| File | Responsibility |
|---|---|
| `.github/workflows/ci.yml` (modify) | add frontend typecheck + vitest |
| `.github/workflows/release-app.yml` (new) | signed, notarized app + DMG |
| `branding/dotfix.svg` (new) | monochrome master mark |
| `branding/generate-icons.sh` (new) | master → `.icns` + tray `@1x/@2x` |

---

## Task 1: Stable drift ids and selective planning

**Files:**
- Modify: `crates/core/src/drift/mod.rs`, `crates/core/src/apply.rs`
- Test: inline `#[cfg(test)]` in both

**Interfaces:**
- Consumes: `Drift`, `PackageRef`, `Report` (phase 1, `crates/core/src/drift/mod.rs`); `Inspection`, `plan` (phase 1, `crates/core/src/apply.rs`).
- Produces:
  - `Drift::id(&self) -> String` — stable across re-inspection
  - `Drift::label(&self) -> String` — the thing the id points at, for display
  - `apply::plan_selected(inspection: &Inspection, ids: &[String]) -> Plan`

- [ ] **Step 1: Write the failing tests for ids**

Append to the `tests` module in `crates/core/src/drift/mod.rs` (create the
module if the file has none):

```rust
#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn ids_are_unique_per_class_and_target() {
        let items = vec![
            Drift::IncomingPackage(PackageRef::formula("jq")),
            Drift::Unmanaged(PackageRef::formula("jq")),
            Drift::IncomingPackage(PackageRef::cask("jq")),
            Drift::LocalEdit {
                target: PathBuf::from("/Users/test/.gitconfig"),
                set: "core".into(),
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
            Drift::Unmanaged(PackageRef::formula("jq")).label(),
            "jq"
        );
        assert_eq!(
            Drift::LocalEdit {
                target: PathBuf::from("/Users/test/.gitconfig"),
                set: "core".into(),
            }
            .label(),
            "/Users/test/.gitconfig"
        );
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p dotfix-core drift::tests`
Expected: FAIL — `no method named id found`.

- [ ] **Step 3: Implement ids and labels**

Add to `crates/core/src/drift/mod.rs`, below the `Drift` enum:

```rust
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
            Drift::Unmanaged(p) => pkg("unmanaged", p),
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
            | Drift::Unmanaged(p)
            | Drift::RemovedPackage { package: p, .. } => p.name.clone(),
            Drift::IncomingFile { target, .. }
            | Drift::RemovedFile { target }
            | Drift::LocalEdit { target, .. } => target.display().to_string(),
        }
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p dotfix-core drift::tests`
Expected: 4 PASS.

- [ ] **Step 5: Write the failing tests for selective planning**

Append to the `tests` module in `crates/core/src/apply.rs`:

```rust
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
        };
        let id = blocked.id();
        let insp = inspection(vec![blocked], vec![]);
        assert!(plan_selected(&insp, &[id]).is_empty());
    }

    #[test]
    fn plan_with_every_id_equals_plan() {
        let insp = inspection(
            vec![
                Drift::IncomingPackage(PackageRef::formula("alpha")),
                Drift::RemovedPackage {
                    package: PackageRef::formula("gone"),
                    blocked_by: vec![],
                },
            ],
            vec![],
        );
        let all: Vec<String> = insp.report.items.iter().map(Drift::id).collect();
        assert_eq!(plan_selected(&insp, &all), plan(&insp));
    }
```

- [ ] **Step 6: Run the tests to verify they fail**

Run: `cargo test -p dotfix-core apply::tests::plan_selected`
Expected: FAIL — `cannot find function plan_selected`.

- [ ] **Step 7: Implement selective planning**

Refactor `plan` in `crates/core/src/apply.rs` so both paths share one body:

```rust
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

fn plan_where(inspection: &Inspection, keep: impl Fn(&Drift) -> bool) -> Plan {
    let mut actions = Vec::new();

    for item in inspection.report.items.iter().filter(|d| keep(d)) {
        match item {
            // ...existing match arms, unchanged...
        }
    }

    Plan { actions }
}
```

Move the existing `match item { ... }` body verbatim into `plan_where`; do not
change any arm.

- [ ] **Step 8: Run the whole suite**

Run: `cargo test --workspace && cargo clippy --all-targets -- -D warnings`
Expected: all PASS, no warnings.

- [ ] **Step 9: Commit**

```bash
git add -A
git commit -m "feat(core): add stable drift ids and selective planning"
```

---

## Task 2: Move set toggling into core

**Files:**
- Create: `crates/core/src/sets.rs`
- Modify: `crates/core/src/lib.rs`, `crates/cli/src/cmd/sets.rs`
- Test: inline `#[cfg(test)]` in `sets.rs`; existing `crates/cli/tests/adopt.rs` must keep passing unchanged

**Interfaces:**
- Consumes: `Repo`, `MachineConfig` (phase 1), `Fsys` (phase 1).
- Produces:
  - `sets::Entry { name: String, active: bool, description: String }`
  - `sets::list(repo: &Repo, machine: &str) -> Result<Vec<Entry>>`
  - `sets::toggle(repo: &Repo, machine: &str, name: &str, on: bool) -> Result<MachineConfig>`
  - `sets::save(fs: &dyn Fsys, repo: &Repo, machine: &str, cfg: &MachineConfig) -> Result<PathBuf>`

`toggle` returns the new configuration without writing it, so the caller
decides when to persist — the CLI writes immediately, the app writes and then
re-inspects.

- [ ] **Step 1: Write the failing tests**

`crates/core/src/sets.rs`:

```rust
#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::config::Repo;
    use crate::ports::Fsys;
    use crate::ports::fake::FakeFsys;

    fn fs() -> FakeFsys {
        FakeFsys::from([
            ("/repo/dotfix.toml", "schema_version = 1\n"),
            (
                "/repo/sets/core/set.toml",
                "description = \"Base\"\n[packages]\nbrew = []\n",
            ),
            (
                "/repo/sets/extra/set.toml",
                "description = \"Extra\"\n[packages]\nbrew = []\n",
            ),
            ("/repo/machines/box-one.toml", "sets = [\"core\"]\n"),
        ])
    }

    fn repo(fs: &FakeFsys) -> Repo {
        Repo::load(fs, Path::new("/repo")).unwrap()
    }

    #[test]
    fn list_marks_the_active_sets_and_keeps_descriptions() {
        let fs = fs();
        let entries = list(&repo(&fs), "box-one").unwrap();
        assert_eq!(
            entries,
            vec![
                Entry {
                    name: "core".into(),
                    active: true,
                    description: "Base".into(),
                },
                Entry {
                    name: "extra".into(),
                    active: false,
                    description: "Extra".into(),
                },
            ]
        );
    }

    #[test]
    fn toggling_on_adds_the_set() {
        let fs = fs();
        let cfg = toggle(&repo(&fs), "box-one", "extra", true).unwrap();
        assert!(cfg.sets.contains(&"extra".to_string()));
    }

    #[test]
    fn toggling_on_twice_does_not_duplicate() {
        let fs = fs();
        let cfg = toggle(&repo(&fs), "box-one", "core", true).unwrap();
        assert_eq!(cfg.sets, vec!["core"]);
    }

    #[test]
    fn toggling_off_removes_the_set() {
        let fs = fs();
        let cfg = toggle(&repo(&fs), "box-one", "core", false).unwrap();
        assert!(cfg.sets.is_empty());
    }

    #[test]
    fn an_unknown_set_is_rejected() {
        let fs = fs();
        let err = toggle(&repo(&fs), "box-one", "ghost", true).unwrap_err();
        assert!(err.to_string().contains("ghost"));
    }

    #[test]
    fn save_writes_the_machine_file_and_returns_its_path() {
        let fs = fs();
        let repo = repo(&fs);
        let cfg = toggle(&repo, "box-one", "extra", true).unwrap();
        let path = save(&fs, &repo, "box-one", &cfg).unwrap();

        assert_eq!(path, Path::new("/repo/machines/box-one.toml"));
        assert!(fs.read(&path).unwrap().contains("extra"));
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p dotfix-core sets`
Expected: FAIL — `cannot find function list`.

- [ ] **Step 3: Implement the module**

Prepend to `crates/core/src/sets.rs`:

```rust
use std::path::PathBuf;

use serde::Serialize;

use crate::config::{MachineConfig, Repo};
use crate::error::{Error, Result};
use crate::ports::Fsys;

/// One set as offered to a user: its name, whether this machine uses it, and
/// the set's own description.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Entry {
    pub name: String,
    pub active: bool,
    pub description: String,
}

/// Every set in the repository, in repository order, marked active or not.
pub fn list(repo: &Repo, machine: &str) -> Result<Vec<Entry>> {
    let cfg = machine_config(repo, machine)?;
    Ok(repo
        .sets
        .iter()
        .map(|(name, set)| Entry {
            name: name.clone(),
            active: cfg.sets.contains(name),
            description: set.description.clone(),
        })
        .collect())
}

/// The machine's configuration with one set switched on or off. Not written —
/// the caller decides when to persist.
pub fn toggle(repo: &Repo, machine: &str, name: &str, on: bool) -> Result<MachineConfig> {
    if !repo.sets.contains_key(name) {
        return Err(Error::Config(format!("unknown set `{name}`")));
    }

    let mut cfg = machine_config(repo, machine)?.clone();
    if on {
        if !cfg.sets.iter().any(|s| s == name) {
            cfg.sets.push(name.to_string());
        }
    } else {
        cfg.sets.retain(|s| s != name);
    }
    Ok(cfg)
}

pub fn save(
    fs: &dyn Fsys,
    repo: &Repo,
    machine: &str,
    cfg: &MachineConfig,
) -> Result<PathBuf> {
    let path = repo.root.join("machines").join(format!("{machine}.toml"));
    let raw = toml::to_string_pretty(cfg)
        .map_err(|e| Error::Config(format!("serialising {}: {e}", path.display())))?;
    fs.write(&path, &raw, 0o644)?;
    Ok(path)
}

fn machine_config<'a>(repo: &'a Repo, machine: &str) -> Result<&'a MachineConfig> {
    repo.machines
        .get(machine)
        .ok_or_else(|| Error::UnknownMachine(machine.to_string()))
}
```

Add `pub mod sets;` to `crates/core/src/lib.rs`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p dotfix-core sets`
Expected: 6 PASS.

- [ ] **Step 5: Rewrite the CLI command against the new module**

Replace the body of `crates/cli/src/cmd/sets.rs`:

```rust
use anyhow::{Result, bail};
use dotfix_core::config::Repo;
use dotfix_core::sets;

use crate::ctx;

pub fn run(enable: Option<String>, disable: Option<String>) -> Result<()> {
    let ctx = ctx::load()?;
    let repo = Repo::load(&ctx.fs, &ctx.local.repo)?;

    if enable.is_none() && disable.is_none() {
        for entry in sets::list(&repo, &ctx.local.machine)? {
            let mark = if entry.active { "x" } else { " " };
            println!("  [{mark}] {}", entry.name);
        }
        return Ok(());
    }

    let mut cfg = repo
        .machines
        .get(&ctx.local.machine)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("unknown machine `{}`", ctx.local.machine))?;

    if let Some(name) = enable {
        cfg = sets::toggle(&repo, &ctx.local.machine, &name, true)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
    }
    if let Some(name) = disable {
        // Re-read from the just-toggled config so `--enable a --disable b` works.
        if !repo.sets.contains_key(&name) {
            bail!("unknown set `{name}`");
        }
        cfg.sets.retain(|s| s != &name);
    }

    let path = sets::save(&ctx.fs, &repo, &ctx.local.machine, &cfg)?;
    println!("updated {}", path.display());
    Ok(())
}
```

- [ ] **Step 6: Run the full suite — the existing CLI tests must still pass unchanged**

Run: `cargo test --workspace && cargo clippy --all-targets -- -D warnings`
Expected: all PASS. `crates/cli/tests/adopt.rs::sets_enable_writes_the_machine_file` and
`::sets_without_flags_lists_active_and_inactive` are the regression guard here —
do not edit them.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "refactor(core): move set toggling out of the CLI into dotfix-core"
```

---

## Task 3: Carry secret values and produce a redacted diff

**Files:**
- Modify: `crates/core/src/render/template.rs`, `crates/core/src/drift/files.rs`, `crates/core/src/engine.rs`, `crates/core/src/lib.rs`, `Cargo.toml`, `crates/core/Cargo.toml`
- Create: `crates/core/src/diffview.rs`
- Test: inline `#[cfg(test)]` in `template.rs` and `diffview.rs`

**Interfaces:**
- Consumes: `Rendered`, `render` (phase 1); `RenderedFile` (phase 1); `Inspection` (phase 1); `secrets::redact`, `secrets::REDACTED` (phase 1).
- Produces:
  - `Rendered { content, contains_secrets, secret_values: Vec<String> }`
  - `RenderedFile { file, content, contains_secrets, secret_values: Vec<String> }`
  - `diffview::LineKind::{Context, Added, Removed}`
  - `diffview::DiffLine { kind: LineKind, text: String }`
  - `diffview::FileDiff { target: PathBuf, set: String, lines: Vec<DiffLine>, truncated: bool }`
  - `diffview::unified(inspection: &Inspection, target: &Path, fs: &dyn Fsys) -> Result<FileDiff>`
  - `diffview::MAX_LINES: usize`

Neither `Rendered` nor `RenderedFile` derives `Serialize`, so the values cannot
escape by accident. `FileDiff` does — and every line in it has already been put
through `redact`.

- [ ] **Step 1: Add the diff dependency**

In the workspace `Cargo.toml`, under `[workspace.dependencies]`:

```toml
similar = "2"
```

In `crates/core/Cargo.toml`, under `[dependencies]`:

```toml
similar.workspace = true
```

- [ ] **Step 2: Write the failing test for carried secret values**

Append to the `tests` module in `crates/core/src/render/template.rs`:

```rust
    #[test]
    fn a_rendered_template_carries_the_values_it_resolved() {
        let out = render(
            Path::new("t.tmpl"),
            "access_key = {{ secret(\"api_key\") }}",
            &vars(),
            &StaticSecrets,
        )
        .unwrap();
        assert_eq!(out.secret_values, vec!["s3cr3t".to_string()]);
    }

    #[test]
    fn a_template_without_secrets_carries_none() {
        let out = render(Path::new("t.tmpl"), "{{ user }}", &vars(), &NoSecrets).unwrap();
        assert!(out.secret_values.is_empty());
    }
```

- [ ] **Step 3: Run the test to verify it fails**

Run: `cargo test -p dotfix-core render::template::tests::a_rendered_template_carries`
Expected: FAIL — `no field secret_values on type Rendered`.

- [ ] **Step 4: Carry the values**

In `crates/core/src/render/template.rs`, extend the struct:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rendered {
    pub content: String,
    /// True when `secret()` was called. The caller must then write the file
    /// with mode 0600 and redact it in any output.
    pub contains_secrets: bool,
    /// The values that were resolved, so callers can redact them before
    /// showing content to anyone. Deliberately not `Serialize`.
    pub secret_values: Vec<String>,
}
```

`render` already collects every lookup up front into `resolved`; keep a copy of
its values before moving it into the closure, and return them:

```rust
    let secret_values: Vec<String> = resolved.values().cloned().collect();
```

Place that line immediately after `resolved` is built, and add
`secret_values` to the returned `Rendered`.

In `crates/core/src/drift/files.rs`, extend `RenderedFile` the same way:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedFile {
    pub file: ResolvedFile,
    pub content: String,
    pub contains_secrets: bool,
    /// Values resolved while rendering this file; used only for redaction.
    pub secret_values: Vec<String>,
}
```

In `crates/core/src/engine.rs`, fill the new field at each of the three
construction sites: the `FileMode::Template` arm passes the renderer's
`secret_values`, the `FileMode::Copy` arm and the generated `.zshrc` pass
`Vec::new()`. Update the `RenderedFile` literals in `crates/core/src/apply.rs`
and `crates/core/src/drift/files.rs` test helpers with `secret_values: vec![]`.

- [ ] **Step 5: Run the suite to verify it passes**

Run: `cargo test --workspace`
Expected: all PASS.

- [ ] **Step 6: Write the failing tests for the diff**

`crates/core/src/diffview.rs`:

```rust
#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::config::{FileMode, ResolvedFile};
    use crate::drift::files::RenderedFile;
    use crate::drift::Report;
    use crate::engine::Inspection;
    use crate::ports::fake::FakeFsys;

    fn inspection(content: &str, secret_values: Vec<String>) -> Inspection {
        Inspection {
            desired: Default::default(),
            rendered: vec![RenderedFile {
                file: ResolvedFile {
                    set: "core".into(),
                    source: PathBuf::from("/repo/sets/core/files/rc.tmpl"),
                    target: PathBuf::from("/Users/test/.rc"),
                    mode: FileMode::Template,
                },
                content: content.to_string(),
                contains_secrets: !secret_values.is_empty(),
                secret_values,
            }],
            report: Report::default(),
        }
    }

    #[test]
    fn shows_removed_and_added_lines_against_the_local_file() {
        let fs = FakeFsys::from([("/Users/test/.rc", "keep\nold\n")]);
        let d = unified(
            &inspection("keep\nnew\n", vec![]),
            Path::new("/Users/test/.rc"),
            &fs,
        )
        .unwrap();

        assert_eq!(
            d.lines,
            vec![
                DiffLine {
                    kind: LineKind::Context,
                    text: "keep".into()
                },
                DiffLine {
                    kind: LineKind::Removed,
                    text: "old".into()
                },
                DiffLine {
                    kind: LineKind::Added,
                    text: "new".into()
                },
            ]
        );
        assert_eq!(d.set, "core");
        assert!(!d.truncated);
    }

    #[test]
    fn an_identical_file_produces_only_context() {
        let fs = FakeFsys::from([("/Users/test/.rc", "same\n")]);
        let d = unified(
            &inspection("same\n", vec![]),
            Path::new("/Users/test/.rc"),
            &fs,
        )
        .unwrap();
        assert!(d.lines.iter().all(|l| l.kind == LineKind::Context));
    }

    #[test]
    fn a_secret_value_never_appears_in_the_diff() {
        let fs = FakeFsys::from([("/Users/test/.rc", "access_key = s3cr3t\nregion = eu\n")]);
        let d = unified(
            &inspection(
                "access_key = s3cr3t\nregion = us\n",
                vec!["s3cr3t".to_string()],
            ),
            Path::new("/Users/test/.rc"),
            &fs,
        )
        .unwrap();

        let all: String = d.lines.iter().map(|l| l.text.as_str()).collect();
        assert!(!all.contains("s3cr3t"), "secret leaked into diff: {all}");
        assert!(all.contains(crate::secrets::REDACTED));
        assert!(all.contains("region"));
    }

    #[test]
    fn a_missing_local_file_diffs_against_nothing() {
        let fs = FakeFsys::new();
        let d = unified(
            &inspection("new\n", vec![]),
            Path::new("/Users/test/.rc"),
            &fs,
        )
        .unwrap();
        assert_eq!(
            d.lines,
            vec![DiffLine {
                kind: LineKind::Added,
                text: "new".into()
            }]
        );
    }

    #[test]
    fn an_unmanaged_target_is_an_error() {
        let fs = FakeFsys::new();
        let err = unified(
            &inspection("x", vec![]),
            Path::new("/Users/test/.other"),
            &fs,
        )
        .unwrap_err();
        assert!(err.to_string().contains(".other"));
    }

    #[test]
    fn a_very_long_diff_is_truncated_and_says_so() {
        let long: String = (0..MAX_LINES + 50).map(|i| format!("line {i}\n")).collect();
        let fs = FakeFsys::from([("/Users/test/.rc", "")]);
        let d = unified(
            &inspection(&long, vec![]),
            Path::new("/Users/test/.rc"),
            &fs,
        )
        .unwrap();
        assert_eq!(d.lines.len(), MAX_LINES);
        assert!(d.truncated);
    }
}
```

- [ ] **Step 7: Run the tests to verify they fail**

Run: `cargo test -p dotfix-core diffview`
Expected: FAIL — `cannot find function unified`.

- [ ] **Step 8: Implement the diff**

Prepend to `crates/core/src/diffview.rs`:

```rust
use std::path::{Path, PathBuf};

use serde::Serialize;
use similar::{ChangeTag, TextDiff};

use crate::engine::Inspection;
use crate::error::{Error, Result};
use crate::ports::Fsys;
use crate::secrets::redact;

/// Upper bound on diff size. A config file that differs in thousands of lines
/// is not something anyone reads in a popover.
pub const MAX_LINES: usize = 400;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LineKind {
    Context,
    Added,
    Removed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiffLine {
    pub kind: LineKind,
    /// Already redacted. Never construct one of these from raw content.
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileDiff {
    pub target: PathBuf,
    pub set: String,
    pub lines: Vec<DiffLine>,
    pub truncated: bool,
}

/// Line diff between what is on disk and what the repository would write.
///
/// Every line is passed through [`redact`] with the values that were resolved
/// while rendering *this* file, so a secret cannot reach the caller — and
/// therefore cannot reach the webview.
pub fn unified(inspection: &Inspection, target: &Path, fs: &dyn Fsys) -> Result<FileDiff> {
    let rendered = inspection
        .rendered
        .iter()
        .find(|r| r.file.target == target)
        .ok_or_else(|| {
            Error::Config(format!("{} is not a managed file", target.display()))
        })?;

    let local = if fs.exists(target) {
        fs.read(target)?
    } else {
        String::new()
    };

    let values = &rendered.secret_values;
    let old = redact(&local, values);
    let new = redact(&rendered.content, values);

    let diff = TextDiff::from_lines(&old, &new);
    let mut lines = Vec::new();
    let mut truncated = false;

    for change in diff.iter_all_changes() {
        if lines.len() == MAX_LINES {
            truncated = true;
            break;
        }
        let kind = match change.tag() {
            ChangeTag::Equal => LineKind::Context,
            ChangeTag::Insert => LineKind::Added,
            ChangeTag::Delete => LineKind::Removed,
        };
        lines.push(DiffLine {
            kind,
            text: change.value().trim_end_matches('\n').to_string(),
        });
    }

    Ok(FileDiff {
        target: target.to_path_buf(),
        set: rendered.file.set.clone(),
        lines,
        truncated,
    })
}
```

Add `pub mod diffview;` to `crates/core/src/lib.rs`.

- [ ] **Step 9: Run the tests to verify they pass**

Run: `cargo test -p dotfix-core diffview && cargo clippy --all-targets -- -D warnings`
Expected: 6 PASS, no warnings.

- [ ] **Step 10: Commit**

```bash
git add -A
git commit -m "feat(core): add redacted file diffs backed by carried secret values"
```

---

## Task 4: App crate scaffold

**Files:**
- Modify: `Cargo.toml` (workspace members), `.github/workflows/ci.yml`
- Create: `app/package.json`, `app/vite.config.ts`, `app/tsconfig.json`, `app/index.html`, `app/src/main.tsx`, `app/src/App.tsx`, `app/src/index.css`
- Create: `app/src-tauri/Cargo.toml`, `app/src-tauri/build.rs`, `app/src-tauri/tauri.conf.json`, `app/src-tauri/capabilities/default.json`, `app/src-tauri/src/main.rs`, `app/src-tauri/src/lib.rs`
- Create: `app/.gitignore`

**Interfaces:**
- Consumes: nothing yet — this task only proves the shell builds.
- Produces: a `dotfix-app` crate in the workspace and an `npm test` / `npm run typecheck` that CI runs.

- [ ] **Step 1: Add the crate to the workspace**

In the root `Cargo.toml`:

```toml
[workspace]
members = ["crates/core", "crates/cli", "app/src-tauri"]
resolver = "3"
```

and under `[workspace.dependencies]`:

```toml
tauri = { version = "2", features = ["tray-icon"] }
tauri-build = "2"
tauri-plugin-autostart = "2"
tauri-plugin-opener = "2"
```

- [ ] **Step 2: Create the Rust side**

`app/src-tauri/Cargo.toml`:

```toml
[package]
name = "dotfix-app"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true

[lib]
name = "dotfix_app_lib"
crate-type = ["staticlib", "cdylib", "rlib"]

[build-dependencies]
tauri-build.workspace = true

[dependencies]
dotfix-core = { path = "../../crates/core" }
tauri.workspace = true
tauri-plugin-autostart.workspace = true
tauri-plugin-opener.workspace = true
serde.workspace = true
serde_json.workspace = true

[dev-dependencies]
dotfix-core = { path = "../../crates/core", features = ["fakes"] }
```

`app/src-tauri/build.rs`:

```rust
fn main() {
    tauri_build::build()
}
```

`app/src-tauri/src/main.rs`:

```rust
// Prevents an extra console window on Windows. dotfix is macOS-only, but the
// attribute is harmless and keeps the file identical to every other Tauri app.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    dotfix_app_lib::run()
}
```

`app/src-tauri/src/lib.rs`:

```rust
//! The dotfix menubar app. All decisions live in [`view`] and in
//! `dotfix-core`; this crate only wires them to Tauri.

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            // No dock icon: dotfix lives in the menubar.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running dotfix");
}
```

`app/src-tauri/tauri.conf.json`:

```json
{
  "$schema": "https://schema.tauri.app/config/2",
  "productName": "dotfix",
  "version": "0.1.0",
  "identifier": "dev.noix.dotfix",
  "build": {
    "beforeDevCommand": "npm run dev",
    "devUrl": "http://localhost:1421",
    "beforeBuildCommand": "npm run build",
    "frontendDist": "../dist"
  },
  "app": {
    "windows": [
      {
        "label": "main",
        "title": "dotfix",
        "width": 720,
        "height": 560,
        "visible": false,
        "resizable": true
      }
    ],
    "security": {
      "csp": "default-src 'self'; style-src 'self' 'unsafe-inline'"
    }
  },
  "bundle": {
    "active": true,
    "targets": ["app", "dmg"],
    "icon": [
      "icons/32x32.png",
      "icons/128x128.png",
      "icons/128x128@2x.png",
      "icons/icon.icns"
    ],
    "macOS": {
      "minimumSystemVersion": "13.0"
    }
  }
}
```

`app/src-tauri/capabilities/default.json`:

```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "default",
  "description": "Permissions for the single dotfix window",
  "windows": ["main"],
  "permissions": [
    "core:default",
    "core:event:default",
    "core:window:allow-show",
    "core:window:allow-hide",
    "core:window:allow-close",
    "core:window:allow-set-focus",
    "core:app:allow-version",
    "opener:default",
    "autostart:default"
  ]
}
```

- [ ] **Step 3: Create the frontend**

`app/package.json`:

```json
{
  "name": "dotfix-app",
  "private": true,
  "version": "0.1.0",
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "tsc && vite build",
    "preview": "vite preview",
    "tauri": "tauri",
    "typecheck": "tsc --noEmit",
    "test": "vitest run"
  },
  "dependencies": {
    "@tauri-apps/api": "^2",
    "@tauri-apps/plugin-autostart": "^2",
    "react": "^19",
    "react-dom": "^19"
  },
  "devDependencies": {
    "@tauri-apps/cli": "^2",
    "@testing-library/jest-dom": "^6",
    "@testing-library/react": "^16",
    "@types/react": "^19",
    "@types/react-dom": "^19",
    "@vitejs/plugin-react": "^5",
    "jsdom": "^26",
    "tailwindcss": "^4",
    "@tailwindcss/vite": "^4",
    "typescript": "^5",
    "vite": "^7",
    "vitest": "^3"
  }
}
```

`app/vite.config.ts`:

```ts
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

export default defineConfig({
  plugins: [react(), tailwindcss()],
  // Tauri serves the dev build from a fixed port and fails loudly if it moves.
  clearScreen: false,
  server: { port: 1421, strictPort: true },
  test: {
    environment: "jsdom",
    setupFiles: ["./src/test-setup.ts"],
  },
});
```

`app/tsconfig.json`:

```json
{
  "compilerOptions": {
    "target": "ES2022",
    "lib": ["ES2022", "DOM", "DOM.Iterable"],
    "module": "ESNext",
    "moduleResolution": "bundler",
    "jsx": "react-jsx",
    "strict": true,
    "noUnusedLocals": true,
    "noUnusedParameters": true,
    "noEmit": true,
    "skipLibCheck": true,
    "types": ["vitest/globals", "@testing-library/jest-dom"]
  },
  "include": ["src"]
}
```

`app/index.html`:

```html
<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>dotfix</title>
  </head>
  <body>
    <div id="root"></div>
    <script type="module" src="/src/main.tsx"></script>
  </body>
</html>
```

`app/src/index.css`:

```css
@import "tailwindcss";
```

`app/src/test-setup.ts`:

```ts
import "@testing-library/jest-dom/vitest";
```

`app/src/main.tsx`:

```tsx
import React from "react";
import ReactDOM from "react-dom/client";

import App from "./App";
import "./index.css";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
```

`app/src/App.tsx`:

```tsx
export default function App() {
  return <main className="p-4 text-sm">dotfix</main>;
}
```

`app/.gitignore`:

```
node_modules
dist
src-tauri/target
src-tauri/gen
```

- [ ] **Step 4: Verify both halves build**

Run:

```bash
cd app && npm install && npm run typecheck && npm test -- --passWithNoTests
cd .. && cargo build -p dotfix-app && cargo test --workspace
```

Expected: all succeed. `cargo test --workspace` must still report the phase 1
tests — adding a crate must not disturb them.

- [ ] **Step 5: Extend CI**

In `.github/workflows/ci.yml`, add to the `check` job after the existing cargo
steps:

```yaml
      - uses: actions/setup-node@v4
        with:
          node-version: 22
          cache: npm
          cache-dependency-path: app/package-lock.json
      - run: npm ci
        working-directory: app
      - run: npm run typecheck
        working-directory: app
      - run: npm test
        working-directory: app
```

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "build(app): scaffold the tauri menubar app crate and frontend"
```

---

## Task 5: View models — the app's only decisions

**Files:**
- Create: `app/src-tauri/src/view.rs`
- Modify: `app/src-tauri/src/lib.rs`
- Test: inline `#[cfg(test)]` in `view.rs`

**Interfaces:**
- Consumes: `Inspection`, `Report`, `Counts`, `Drift`, `Drift::id`, `Drift::label` (Tasks 1 and phase 1).
- Produces:
  - `view::Area::{Changes, Unmanaged, Configs}`
  - `view::Item { id, label, area, action, detail, actionable }`
  - `view::Overview { counts: Counts, items: Vec<Item>, status_line: Option<String> }`
  - `view::overview(inspection: &Inspection) -> Overview`
  - `view::items_in(overview: &Overview, area: Area) -> Vec<&Item>`

Every rule about what the UI shows lives here, tested without Tauri and without
a browser. The React components receive `Item`s and render them.

- [ ] **Step 1: Write the failing tests**

`app/src-tauri/src/view.rs`:

```rust
#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use dotfix_core::drift::{Drift, PackageRef, Report};
    use dotfix_core::engine::Inspection;

    use super::*;

    fn inspection(items: Vec<Drift>) -> Inspection {
        Inspection {
            desired: Default::default(),
            rendered: vec![],
            report: Report { items },
        }
    }

    #[test]
    fn sorts_each_drift_class_into_its_area() {
        let o = overview(&inspection(vec![
            Drift::IncomingPackage(PackageRef::formula("alpha")),
            Drift::RemovedPackage {
                package: PackageRef::formula("gone"),
                blocked_by: vec![],
            },
            Drift::Unmanaged(PackageRef::formula("stray")),
            Drift::LocallyRemoved(PackageRef::formula("removed-here")),
            Drift::LocalEdit {
                target: PathBuf::from("/Users/test/.gitconfig"),
                set: "core".into(),
            },
        ]));

        let labels = |a: Area| -> Vec<String> {
            items_in(&o, a).iter().map(|i| i.label.clone()).collect()
        };

        assert_eq!(labels(Area::Changes), vec!["alpha", "gone"]);
        assert_eq!(labels(Area::Unmanaged), vec!["stray", "removed-here"]);
        assert_eq!(labels(Area::Configs), vec!["/Users/test/.gitconfig"]);
    }

    #[test]
    fn a_blocked_removal_is_shown_but_not_actionable_and_says_why() {
        let o = overview(&inspection(vec![Drift::RemovedPackage {
            package: PackageRef::formula("openjdk"),
            blocked_by: vec!["maven".into(), "gradle".into()],
        }]));

        let item = &items_in(&o, Area::Changes)[0];
        assert!(!item.actionable);
        assert_eq!(item.detail, "still required by maven, gradle");
    }

    #[test]
    fn actions_name_what_would_happen() {
        let o = overview(&inspection(vec![
            Drift::IncomingPackage(PackageRef::formula("alpha")),
            Drift::RemovedPackage {
                package: PackageRef::formula("gone"),
                blocked_by: vec![],
            },
            Drift::Unmanaged(PackageRef::formula("stray")),
            Drift::LocalEdit {
                target: PathBuf::from("/Users/test/.rc"),
                set: "core".into(),
            },
        ]));

        let action = |label: &str| -> String {
            o.items
                .iter()
                .find(|i| i.label == label)
                .unwrap()
                .action
                .clone()
        };
        assert_eq!(action("alpha"), "install");
        assert_eq!(action("gone"), "uninstall");
        assert_eq!(action("stray"), "adopt");
        assert_eq!(action("/Users/test/.rc"), "review");
    }

    #[test]
    fn ids_come_straight_from_core_so_apply_can_use_them() {
        let drift = Drift::IncomingPackage(PackageRef::formula("alpha"));
        let expected = drift.id();
        let o = overview(&inspection(vec![drift]));
        assert_eq!(o.items[0].id, expected);
    }

    #[test]
    fn an_empty_report_has_no_items_and_no_status_line() {
        let o = overview(&inspection(vec![]));
        assert!(o.items.is_empty());
        assert_eq!(o.status_line, None);
    }

    #[test]
    fn the_status_line_matches_what_the_shell_would_print() {
        let insp = inspection(vec![Drift::IncomingPackage(PackageRef::formula("alpha"))]);
        let o = overview(&insp);
        assert_eq!(
            o.status_line,
            dotfix_core::status_line::render(&insp.report.counts())
        );
    }

    #[test]
    fn a_generated_file_is_labelled_by_its_path_and_set() {
        let o = overview(&inspection(vec![Drift::IncomingFile {
            target: PathBuf::from("/Users/test/.zshrc"),
            set: dotfix_core::engine::GENERATED_SET.into(),
        }]));
        let item = &items_in(&o, Area::Changes)[0];
        assert_eq!(item.label, "/Users/test/.zshrc");
        assert_eq!(item.detail, "generated");
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p dotfix-app view`
Expected: FAIL — `cannot find function overview`.

- [ ] **Step 3: Implement the view models**

Prepend to `app/src-tauri/src/view.rs`:

```rust
//! Turns an [`Inspection`] into exactly what the window shows.
//!
//! Every decision about grouping, wording and what may be acted on lives here,
//! so it can be tested without Tauri and without a browser. The React side
//! renders these structs and decides nothing.

use dotfix_core::drift::{Counts, Drift};
use dotfix_core::engine::Inspection;
use dotfix_core::status_line;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Area {
    /// Things dotfix can apply: installs, uninstalls, file writes.
    Changes,
    /// Things that need a decision before they can be managed.
    Unmanaged,
    /// Managed files edited by hand.
    Configs,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Item {
    /// [`Drift::id`], the handle `apply`/`adopt` take.
    pub id: String,
    pub label: String,
    pub area: Area,
    /// One word for what would happen: install, uninstall, write, remove,
    /// adopt, review.
    pub action: String,
    /// Secondary line: the owning set, or why an action is unavailable.
    pub detail: String,
    /// False when the action cannot be carried out — the row renders disabled
    /// with `detail` explaining why.
    pub actionable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Overview {
    pub counts: Counts,
    pub items: Vec<Item>,
    /// The same line the shell hook prints, so the window and the terminal can
    /// never disagree.
    pub status_line: Option<String>,
}

pub fn overview(inspection: &Inspection) -> Overview {
    let counts = inspection.report.counts();
    let items = inspection.report.items.iter().map(item_for).collect();

    Overview {
        counts,
        items,
        status_line: status_line::render(&counts),
    }
}

pub fn items_in(overview: &Overview, area: Area) -> Vec<&Item> {
    overview.items.iter().filter(|i| i.area == area).collect()
}

fn item_for(drift: &Drift) -> Item {
    let (area, action, detail, actionable) = match drift {
        Drift::IncomingPackage(_) => (Area::Changes, "install", String::new(), true),

        Drift::IncomingFile { set, .. } => (Area::Changes, "write", set.clone(), true),

        Drift::RemovedFile { .. } => (Area::Changes, "remove", String::new(), true),

        Drift::RemovedPackage { blocked_by, .. } if blocked_by.is_empty() => {
            (Area::Changes, "uninstall", String::new(), true)
        }
        Drift::RemovedPackage { blocked_by, .. } => (
            Area::Changes,
            "uninstall",
            format!("still required by {}", blocked_by.join(", ")),
            false,
        ),

        Drift::Unmanaged(_) => (Area::Unmanaged, "adopt", "in no set".to_string(), true),

        Drift::LocallyRemoved(_) => (
            Area::Unmanaged,
            "adopt",
            "removed on this machine".to_string(),
            true,
        ),

        Drift::LocalEdit { set, .. } => (Area::Configs, "review", set.clone(), true),
    };

    Item {
        id: drift.id(),
        label: drift.label(),
        area,
        action: action.to_string(),
        detail,
        actionable,
    }
}
```

Add `pub mod view;` to `app/src-tauri/src/lib.rs`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p dotfix-app view && cargo clippy --all-targets -- -D warnings`
Expected: 7 PASS, no warnings.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(app): add tested view models mapping drift into the four areas"
```

---

## Task 6: Engine wiring and Tauri commands

**Files:**
- Create: `app/src-tauri/src/ctx.rs`, `app/src-tauri/src/commands.rs`
- Modify: `app/src-tauri/src/lib.rs`
- Test: inline `#[cfg(test)]` in `ctx.rs` and `commands.rs`

**Interfaces:**
- Consumes: `Engine`, `Inspection`, `LocalConfig`, `Paths`, `RealFsys`/`RealBrew`/`RealGit`/`RealExec` (phase 1); `view::overview` (Task 5); `apply::plan_selected` (Task 1); `sets::{list, toggle, save}` (Task 2); `diffview::unified` (Task 3); `adopt::{proposals_for, apply_proposal, Proposal}` (phase 1); `Git::log` (phase 1).
- Produces:
  - `ctx::Ctx { fs, brew, git, exec, paths, user, local }` with `Ctx::load() -> Result<Ctx>` and `Ctx::engine(&self) -> Engine<'_>`
  - `commands::CmdError` (a `String` newtype-ish alias) and `to_cmd_err`
  - Tauri commands: `overview`, `refresh`, `apply_items`, `adopt_item`, `list_sets`, `toggle_set`, `file_diff`, `history`

Every command re-inspects rather than trusting a cached snapshot: the window
may have been open for an hour. Ids that no longer match are skipped, which is
handled inside `plan_selected`.

- [ ] **Step 1: Write the failing test for error mapping**

`app/src-tauri/src/commands.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_reach_the_frontend_as_readable_strings() {
        let err = dotfix_core::Error::UnknownMachine("box-nine".into());
        assert_eq!(
            to_cmd_err(err),
            "machine `box-nine` not found in repository"
        );
    }

    #[test]
    fn an_error_string_never_carries_a_secret_value() {
        // Error::Secret is built from a name and a reason; the value is never
        // one of its fields. This test locks that in.
        let err = dotfix_core::Error::Secret {
            name: "api_key".into(),
            reason: "item not found".into(),
        };
        let text = to_cmd_err(err);
        assert!(text.contains("api_key"));
        assert!(text.contains("item not found"));
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p dotfix-app commands`
Expected: FAIL — `cannot find function to_cmd_err`.

- [ ] **Step 3: Implement the context**

`app/src-tauri/src/ctx.rs`:

```rust
//! The real ports plus machine-local facts. Mirrors `crates/cli/src/ctx.rs`;
//! both construct the same `Engine`, which is the point of linking the core
//! crate directly instead of shelling out to the CLI.

use std::path::PathBuf;

use dotfix_core::engine::Engine;
use dotfix_core::error::{Error, Result};
use dotfix_core::paths::{LocalConfig, Paths};
use dotfix_core::ports::{RealBrew, RealExec, RealFsys, RealGit};

pub struct Ctx {
    pub fs: RealFsys,
    pub brew: RealBrew,
    pub git: RealGit,
    pub exec: RealExec,
    pub paths: Paths,
    pub user: String,
    pub local: LocalConfig,
}

impl Ctx {
    pub fn load() -> Result<Self> {
        let home = std::env::var("HOME")
            .map(PathBuf::from)
            .map_err(|_| Error::Config("HOME is not set".into()))?;
        let paths = Paths::new(home);
        let fs = RealFsys;
        let local = LocalConfig::load(&fs, &paths.local_config())?;

        Ok(Self {
            fs: RealFsys,
            brew: RealBrew,
            git: RealGit,
            exec: RealExec,
            paths,
            user: std::env::var("USER").unwrap_or_else(|_| "unknown".into()),
            local,
        })
    }

    pub fn engine(&self) -> Engine<'_> {
        Engine {
            fs: &self.fs,
            brew: &self.brew,
            git: &self.git,
            exec: &self.exec,
            paths: self.paths.clone(),
            user: self.user.clone(),
        }
    }
}
```

- [ ] **Step 4: Implement the commands**

Prepend to `app/src-tauri/src/commands.rs`:

```rust
//! Thin wrappers around `dotfix-core` and [`crate::view`]. A command may
//! orchestrate, never decide — anything worth a test belongs in `view` or in
//! the core crate.

use std::path::PathBuf;

use dotfix_core::adopt::{self, Proposal};
use dotfix_core::apply::{execute, plan_selected};
use dotfix_core::config::Repo;
use dotfix_core::diffview::{self, FileDiff};
use dotfix_core::ports::{Commit, Git};
use dotfix_core::sets::{self, Entry};

use crate::ctx::Ctx;
use crate::view::{self, Overview};

/// Tauri needs a `Serialize` error; core errors already render themselves
/// usefully and never carry secret values.
pub fn to_cmd_err(err: dotfix_core::Error) -> String {
    err.to_string()
}

fn seconds_since_epoch() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
        .to_string()
}

/// Current state without touching the network.
#[tauri::command]
pub fn overview() -> Result<Overview, String> {
    let ctx = Ctx::load().map_err(to_cmd_err)?;
    let inspection = ctx.engine().inspect(&ctx.local).map_err(to_cmd_err)?;
    Ok(view::overview(&inspection))
}

/// Pull first, then report. Surfaces a diverged repository as an error rather
/// than merging.
#[tauri::command]
pub fn refresh() -> Result<Overview, String> {
    let ctx = Ctx::load().map_err(to_cmd_err)?;
    let engine = ctx.engine();
    engine.sync(&ctx.local).map_err(to_cmd_err)?;
    let inspection = engine.inspect(&ctx.local).map_err(to_cmd_err)?;
    Ok(view::overview(&inspection))
}

/// Apply the selected drift items and return the state afterwards.
#[tauri::command]
pub fn apply_items(ids: Vec<String>) -> Result<Overview, String> {
    let ctx = Ctx::load().map_err(to_cmd_err)?;
    let engine = ctx.engine();

    let inspection = engine.inspect(&ctx.local).map_err(to_cmd_err)?;
    let plan = plan_selected(&inspection, &ids);
    if !plan.is_empty() {
        execute(&plan, &engine, &seconds_since_epoch()).map_err(to_cmd_err)?;
    }

    let after = engine.inspect(&ctx.local).map_err(to_cmd_err)?;
    Ok(view::overview(&after))
}

/// Adopt one drift item. `set` chooses the target set for a package;
/// `ignore = true` takes the ignore branch instead.
#[tauri::command]
pub fn adopt_item(id: String, set: Option<String>, ignore: bool) -> Result<Overview, String> {
    let ctx = Ctx::load().map_err(to_cmd_err)?;
    let engine = ctx.engine();
    let inspection = engine.inspect(&ctx.local).map_err(to_cmd_err)?;
    let repo = Repo::load(&ctx.fs, &ctx.local.repo).map_err(to_cmd_err)?;

    let default_set = set.unwrap_or_else(|| {
        repo.machines
            .get(&ctx.local.machine)
            .and_then(|m| m.sets.first().cloned())
            .unwrap_or_else(|| "core".to_string())
    });

    if let Some(drift) = inspection.report.items.iter().find(|d| d.id() == id) {
        let proposals = adopt::proposals_for(drift, &repo, &default_set);
        let chosen = proposals.into_iter().find(|p| {
            matches!(p, Proposal::IgnorePackage { .. }) == ignore
        });
        if let Some(proposal) = chosen {
            adopt::apply_proposal(&proposal, &repo, &ctx.local.machine, &ctx.fs)
                .map_err(to_cmd_err)?;
        }
    }

    let after = engine.inspect(&ctx.local).map_err(to_cmd_err)?;
    Ok(view::overview(&after))
}

#[tauri::command]
pub fn list_sets() -> Result<Vec<Entry>, String> {
    let ctx = Ctx::load().map_err(to_cmd_err)?;
    let repo = Repo::load(&ctx.fs, &ctx.local.repo).map_err(to_cmd_err)?;
    sets::list(&repo, &ctx.local.machine).map_err(to_cmd_err)
}

#[tauri::command]
pub fn toggle_set(name: String, on: bool) -> Result<Vec<Entry>, String> {
    let ctx = Ctx::load().map_err(to_cmd_err)?;
    let repo = Repo::load(&ctx.fs, &ctx.local.repo).map_err(to_cmd_err)?;

    let cfg = sets::toggle(&repo, &ctx.local.machine, &name, on).map_err(to_cmd_err)?;
    sets::save(&ctx.fs, &repo, &ctx.local.machine, &cfg).map_err(to_cmd_err)?;

    let repo = Repo::load(&ctx.fs, &ctx.local.repo).map_err(to_cmd_err)?;
    sets::list(&repo, &ctx.local.machine).map_err(to_cmd_err)
}

#[tauri::command]
pub fn file_diff(target: String) -> Result<FileDiff, String> {
    let ctx = Ctx::load().map_err(to_cmd_err)?;
    let inspection = ctx.engine().inspect(&ctx.local).map_err(to_cmd_err)?;
    diffview::unified(&inspection, &PathBuf::from(target), &ctx.fs).map_err(to_cmd_err)
}

#[tauri::command]
pub fn history(limit: usize) -> Result<Vec<Commit>, String> {
    let ctx = Ctx::load().map_err(to_cmd_err)?;
    ctx.git
        .log(&ctx.local.repo, limit.min(200))
        .map_err(to_cmd_err)
}
```

- [ ] **Step 5: Register the commands**

In `app/src-tauri/src/lib.rs`, add the modules and the handler:

```rust
pub mod commands;
pub mod ctx;
pub mod view;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            commands::overview,
            commands::refresh,
            commands::apply_items,
            commands::adopt_item,
            commands::list_sets,
            commands::toggle_set,
            commands::file_diff,
            commands::history,
        ])
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running dotfix");
}
```

- [ ] **Step 6: Run the tests**

Run: `cargo test -p dotfix-app && cargo clippy --all-targets -- -D warnings`
Expected: PASS, no warnings.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "feat(app): wire the engine into tauri commands"
```

---

## Task 7: Tray icon and status glyph

**Files:**
- Create: `app/src-tauri/src/tray.rs`
- Modify: `app/src-tauri/src/lib.rs`
- Test: inline `#[cfg(test)]` in `tray.rs`

**Interfaces:**
- Consumes: `Counts` (phase 1), `view::Overview` (Task 5).
- Produces:
  - `tray::Glyph::{Quiet, Drift}`
  - `tray::glyph_for(counts: &Counts) -> Glyph`
  - `tray::tooltip_for(overview: &Overview) -> String`
  - `tray::menu_model(overview: &Overview) -> Vec<(String, String)>` — (id, label) pairs
  - `tray::build(app: &tauri::AppHandle) -> tauri::Result<()>`
  - `tray::TRAY_ID: &str`

The glyph choice, tooltip and menu model are pure functions, following the
pattern `notefix` uses for its own tray. Only `build` touches Tauri.

- [ ] **Step 1: Write the failing tests**

`app/src-tauri/src/tray.rs`:

```rust
#[cfg(test)]
mod tests {
    use dotfix_core::drift::Counts;

    use super::*;
    use crate::view::Overview;

    fn overview(counts: Counts, status_line: Option<&str>) -> Overview {
        Overview {
            counts,
            items: vec![],
            status_line: status_line.map(str::to_string),
        }
    }

    #[test]
    fn a_clean_machine_gets_the_quiet_glyph() {
        assert_eq!(glyph_for(&Counts::default()), Glyph::Quiet);
    }

    #[test]
    fn any_drift_gets_the_drift_glyph() {
        for counts in [
            Counts {
                incoming: 1,
                ..Default::default()
            },
            Counts {
                unmanaged: 1,
                ..Default::default()
            },
            Counts {
                removed: 1,
                ..Default::default()
            },
            Counts {
                local_edits: 1,
                ..Default::default()
            },
        ] {
            assert_eq!(glyph_for(&counts), Glyph::Drift, "for {counts:?}");
        }
    }

    #[test]
    fn the_tooltip_reuses_the_status_line_so_it_cannot_disagree() {
        let o = overview(
            Counts {
                incoming: 2,
                ..Default::default()
            },
            Some("↯ dotfix: 2 changes   →  dotfix apply"),
        );
        assert_eq!(tooltip_for(&o), "↯ dotfix: 2 changes   →  dotfix apply");
    }

    #[test]
    fn a_quiet_tooltip_still_names_the_app() {
        let o = overview(Counts::default(), None);
        assert_eq!(tooltip_for(&o), "dotfix — everything in sync");
    }

    #[test]
    fn the_menu_offers_open_refresh_and_quit_in_that_order() {
        let o = overview(Counts::default(), None);
        let ids: Vec<String> = menu_model(&o).into_iter().map(|(id, _)| id).collect();
        assert_eq!(ids, vec!["open", "refresh", "quit"]);
    }

    #[test]
    fn the_open_entry_summarises_outstanding_work() {
        let o = overview(
            Counts {
                incoming: 3,
                ..Default::default()
            },
            Some("x"),
        );
        let (_, label) = menu_model(&o).into_iter().next().unwrap();
        assert_eq!(label, "Open dotfix (3 changes)");
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p dotfix-app tray`
Expected: FAIL — `cannot find type Glyph`.

- [ ] **Step 3: Implement the tray**

Prepend to `app/src-tauri/src/tray.rs`:

```rust
//! Menubar presence. The glyph, tooltip and menu model are pure functions so
//! they can be tested without a running app; only [`build`] touches Tauri.

use dotfix_core::drift::Counts;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager};

use crate::view::Overview;

pub const TRAY_ID: &str = "dotfix-tray";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Glyph {
    /// Nothing to do. Plain mark.
    Quiet,
    /// Something drifted. Mark with a status dot.
    Drift,
}

impl Glyph {
    /// Template images: monochrome with transparency, so macOS inverts them
    /// for a dark menubar and for the pressed state on its own.
    pub fn asset(self) -> &'static str {
        match self {
            Glyph::Quiet => "icons/tray-quiet.png",
            Glyph::Drift => "icons/tray-drift.png",
        }
    }
}

pub fn glyph_for(counts: &Counts) -> Glyph {
    let quiet = counts.incoming == 0
        && counts.unmanaged == 0
        && counts.removed == 0
        && counts.local_edits == 0;
    if quiet { Glyph::Quiet } else { Glyph::Drift }
}

/// The same sentence the shell prints, so the menubar and the terminal can
/// never tell the user two different things.
pub fn tooltip_for(overview: &Overview) -> String {
    overview
        .status_line
        .clone()
        .unwrap_or_else(|| "dotfix — everything in sync".to_string())
}

pub fn menu_model(overview: &Overview) -> Vec<(String, String)> {
    let c = &overview.counts;
    let outstanding = c.incoming + c.removed + c.local_edits + c.unmanaged;
    let open = if outstanding == 0 {
        "Open dotfix".to_string()
    } else if c.incoming > 0 {
        format!(
            "Open dotfix ({} change{})",
            c.incoming,
            if c.incoming == 1 { "" } else { "s" }
        )
    } else {
        format!("Open dotfix ({outstanding} to review)")
    };

    vec![
        ("open".to_string(), open),
        ("refresh".to_string(), "Check now".to_string()),
        ("quit".to_string(), "Quit".to_string()),
    ]
}

pub fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

pub fn build(app: &AppHandle) -> tauri::Result<()> {
    let overview = crate::commands::overview().ok();
    let glyph = overview
        .as_ref()
        .map(|o| glyph_for(&o.counts))
        .unwrap_or(Glyph::Quiet);

    let menu = Menu::new(app)?;
    for (id, label) in menu_model(&overview.clone().unwrap_or(Overview {
        counts: Counts::default(),
        items: vec![],
        status_line: None,
    })) {
        menu.append(&MenuItem::with_id(app, &id, &label, true, None::<&str>)?)?;
    }

    let icon = tauri::image::Image::from_path(
        app.path()
            .resource_dir()?
            .join(glyph.asset()),
    )?;

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .icon_as_template(true)
        .tooltip(overview.as_ref().map(tooltip_for).unwrap_or_default())
        .menu(&menu)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show_main(app),
            "refresh" => {
                let _ = crate::commands::refresh();
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;

    Ok(())
}
```

Call it from `setup` in `app/src-tauri/src/lib.rs`:

```rust
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            tray::build(app.handle())?;
            Ok(())
        })
```

and add `pub mod tray;`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p dotfix-app tray && cargo clippy --all-targets -- -D warnings`
Expected: 6 PASS, no warnings.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(app): add menubar tray with quiet and drift glyphs"
```

---

## Task 8: Frontend foundation — types, API and shell

> **Before starting Tasks 8–11:** invoke `frontend-design`. The visual
> direction below is the constraint set, not a finished design; that skill is
> where the look gets decided.

**Visual constraints (non-negotiable, derived from the spec):**
- A menubar popover-style window, 720×560, no dock icon. It is glanced at, not lived in.
- Four areas reachable without scrolling; the empty state is the *normal* state and must look calm, not broken.
- Never render a secret. The backend guarantees it; the UI must not re-introduce raw content from anywhere else.
- Monochrome-first, matching the tray glyph. Colour carries meaning only: one accent for "needs your decision", one for destructive.

**Files:**
- Create: `app/src/types.ts`, `app/src/api.ts`, `app/src/components/{Empty,Busy,Badge,Row}.tsx`
- Modify: `app/src/App.tsx`
- Test: `app/src/api.test.ts`, `app/src/App.test.tsx`

**Interfaces:**
- Consumes: the JSON shapes produced by `view::Overview`, `sets::Entry`, `diffview::FileDiff`, `ports::Commit`.
- Produces:
  - `types.ts`: `Area`, `Item`, `Counts`, `Overview`, `SetEntry`, `DiffLine`, `FileDiff`, `Commit`
  - `api.ts`: `getOverview`, `refresh`, `applyItems`, `adoptItem`, `listSets`, `toggleSet`, `fileDiff`, `history`
  - `components`: `<Empty>`, `<Busy>`, `<Badge>`, `<Row>`

- [ ] **Step 1: Write the failing tests**

`app/src/api.test.ts`:

```ts
import { beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => invoke(...a) }));

import { applyItems, getOverview, toggleSet } from "./api";

describe("api", () => {
  beforeEach(() => invoke.mockReset());

  it("calls the overview command with no arguments", async () => {
    invoke.mockResolvedValue({ counts: {}, items: [], status_line: null });
    await getOverview();
    expect(invoke).toHaveBeenCalledWith("overview");
  });

  it("passes selected ids through to apply_items", async () => {
    invoke.mockResolvedValue({ counts: {}, items: [], status_line: null });
    await applyItems(["incoming_package:formula:jq"]);
    expect(invoke).toHaveBeenCalledWith("apply_items", {
      ids: ["incoming_package:formula:jq"],
    });
  });

  it("surfaces a backend error as a thrown Error", async () => {
    invoke.mockRejectedValue("repository has diverged from origin");
    await expect(toggleSet("web", true)).rejects.toThrow(
      "repository has diverged from origin",
    );
  });
});
```

`app/src/App.test.tsx`:

```tsx
import { render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => invoke(...a) }));

import App from "./App";

const empty = { counts: { incoming: 0, unmanaged: 0, removed: 0, local_edits: 0 }, items: [], status_line: null };

describe("App", () => {
  beforeEach(() => invoke.mockReset());

  it("shows the calm empty state when nothing drifted", async () => {
    invoke.mockResolvedValue(empty);
    render(<App />);
    await waitFor(() => expect(screen.getByText(/everything in sync/i)).toBeInTheDocument());
  });

  it("shows an error banner when the backend fails", async () => {
    invoke.mockRejectedValue("repository has diverged from origin");
    render(<App />);
    await waitFor(() =>
      expect(screen.getByRole("alert")).toHaveTextContent(/diverged/i),
    );
  });
});
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd app && npm test`
Expected: FAIL — `Cannot find module './api'`.

- [ ] **Step 3: Write the types**

`app/src/types.ts`:

```ts
// Mirrors the serialised shapes from app/src-tauri/src/view.rs and dotfix-core.
// Keep field names in snake_case: they come straight from serde.

export type Area = "changes" | "unmanaged" | "configs";

export interface Counts {
  incoming: number;
  unmanaged: number;
  removed: number;
  local_edits: number;
}

export interface Item {
  id: string;
  label: string;
  area: Area;
  action: string;
  detail: string;
  actionable: boolean;
}

export interface Overview {
  counts: Counts;
  items: Item[];
  status_line: string | null;
}

export interface SetEntry {
  name: string;
  active: boolean;
  description: string;
}

export type LineKind = "context" | "added" | "removed";

export interface DiffLine {
  kind: LineKind;
  text: string;
}

export interface FileDiff {
  target: string;
  set: string;
  lines: DiffLine[];
  truncated: boolean;
}

export interface Commit {
  hash: string;
  subject: string;
  date: string;
}
```

- [ ] **Step 4: Write the API wrapper**

`app/src/api.ts`:

```ts
import { invoke } from "@tauri-apps/api/core";

import type { Commit, FileDiff, Overview, SetEntry } from "./types";

/// Tauri rejects with a bare string; turn that into a real Error once, here,
/// so no component has to care.
async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return args === undefined
      ? await invoke<T>(cmd)
      : await invoke<T>(cmd, args);
  } catch (err) {
    throw new Error(typeof err === "string" ? err : String(err));
  }
}

export const getOverview = () => call<Overview>("overview");
export const refresh = () => call<Overview>("refresh");

export const applyItems = (ids: string[]) => call<Overview>("apply_items", { ids });

export const adoptItem = (id: string, set: string | null, ignore: boolean) =>
  call<Overview>("adopt_item", { id, set, ignore });

export const listSets = () => call<SetEntry[]>("list_sets");
export const toggleSet = (name: string, on: boolean) =>
  call<SetEntry[]>("toggle_set", { name, on });

export const fileDiff = (target: string) => call<FileDiff>("file_diff", { target });
export const history = (limit: number) => call<Commit[]>("history", { limit });
```

- [ ] **Step 5: Write the shared components and the shell**

`app/src/components/Empty.tsx`:

```tsx
export default function Empty({ children }: { children: React.ReactNode }) {
  return (
    <p className="py-10 text-center text-sm text-neutral-500">{children}</p>
  );
}
```

`app/src/components/Busy.tsx`:

```tsx
export default function Busy() {
  return <p className="py-10 text-center text-sm text-neutral-500">Checking…</p>;
}
```

`app/src/components/Badge.tsx`:

```tsx
export default function Badge({ count }: { count: number }) {
  if (count === 0) return null;
  return (
    <span className="ml-1 rounded-full bg-neutral-200 px-1.5 text-xs tabular-nums">
      {count}
    </span>
  );
}
```

`app/src/components/Row.tsx`:

```tsx
export default function Row({
  label,
  detail,
  disabled = false,
  children,
}: {
  label: string;
  detail?: string;
  disabled?: boolean;
  children?: React.ReactNode;
}) {
  return (
    <li
      className={`flex items-center justify-between gap-3 border-b border-neutral-100 py-2 ${
        disabled ? "opacity-50" : ""
      }`}
    >
      <span className="min-w-0">
        <span className="block truncate font-medium">{label}</span>
        {detail ? (
          <span className="block truncate text-xs text-neutral-500">{detail}</span>
        ) : null}
      </span>
      {children}
    </li>
  );
}
```

`app/src/App.tsx`:

```tsx
import { useCallback, useEffect, useState } from "react";

import { getOverview } from "./api";
import Busy from "./components/Busy";
import Empty from "./components/Empty";
import type { Overview } from "./types";

export default function App() {
  const [overview, setOverview] = useState<Overview | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      setError(null);
      setOverview(await getOverview());
    } catch (err) {
      setError((err as Error).message);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  if (error) {
    return (
      <main className="p-4 text-sm">
        <p role="alert" className="rounded bg-red-50 p-3 text-red-800">
          {error}
        </p>
      </main>
    );
  }

  if (!overview) return <Busy />;

  return (
    <main className="p-4 text-sm">
      {overview.items.length === 0 ? (
        <Empty>Everything in sync</Empty>
      ) : (
        <p>{overview.items.length} item(s)</p>
      )}
    </main>
  );
}
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cd app && npm test && npm run typecheck`
Expected: all PASS.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "feat(app): add frontend types, api wrapper and shell"
```

---

## Task 9: Changes and Unmanaged areas

**Files:**
- Create: `app/src/areas/Changes.tsx`, `app/src/areas/Unmanaged.tsx`
- Modify: `app/src/App.tsx`
- Test: `app/src/areas/Changes.test.tsx`, `app/src/areas/Unmanaged.test.tsx`

**Interfaces:**
- Consumes: `Item`, `Overview` (Task 8); `applyItems`, `adoptItem`, `listSets` (Task 8).
- Produces: `<Changes items onApplied>`, `<Unmanaged items onAdopted>`.

- [ ] **Step 1: Write the failing tests**

`app/src/areas/Changes.test.tsx`:

```tsx
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { Item } from "../types";
import Changes from "./Changes";

const item = (over: Partial<Item> = {}): Item => ({
  id: "incoming_package:formula:jq",
  label: "jq",
  area: "changes",
  action: "install",
  detail: "",
  actionable: true,
  ...over,
});

describe("Changes", () => {
  it("lists each item with the action that would be taken", () => {
    render(<Changes items={[item()]} onApply={vi.fn()} />);
    expect(screen.getByText("jq")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /install/i })).toBeEnabled();
  });

  it("disables a blocked item and shows why", () => {
    render(
      <Changes
        items={[
          item({
            actionable: false,
            action: "uninstall",
            detail: "still required by maven",
            label: "openjdk",
          }),
        ]}
        onApply={vi.fn()}
      />,
    );
    expect(screen.getByRole("button", { name: /uninstall/i })).toBeDisabled();
    expect(screen.getByText(/still required by maven/)).toBeInTheDocument();
  });

  it("applies a single item by id", () => {
    const onApply = vi.fn();
    render(<Changes items={[item()]} onApply={onApply} />);
    fireEvent.click(screen.getByRole("button", { name: /install/i }));
    expect(onApply).toHaveBeenCalledWith(["incoming_package:formula:jq"]);
  });

  it("apply all sends only the actionable ids", () => {
    const onApply = vi.fn();
    render(
      <Changes
        items={[item(), item({ id: "blocked", actionable: false })]}
        onApply={onApply}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /apply all/i }));
    expect(onApply).toHaveBeenCalledWith(["incoming_package:formula:jq"]);
  });

  it("shows nothing to do when the list is empty", () => {
    render(<Changes items={[]} onApply={vi.fn()} />);
    expect(screen.getByText(/nothing to apply/i)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /apply all/i })).toBeNull();
  });
});
```

`app/src/areas/Unmanaged.test.tsx`:

```tsx
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { Item, SetEntry } from "../types";
import Unmanaged from "./Unmanaged";

const sets: SetEntry[] = [
  { name: "core", active: true, description: "Base" },
  { name: "web", active: true, description: "Web" },
];

const item: Item = {
  id: "unmanaged:formula:jq",
  label: "jq",
  area: "unmanaged",
  action: "adopt",
  detail: "in no set",
  actionable: true,
};

describe("Unmanaged", () => {
  it("offers every set as an adoption target", () => {
    render(<Unmanaged items={[item]} sets={sets} onAdopt={vi.fn()} />);
    const select = screen.getByRole("combobox", { name: /set for jq/i });
    expect(select).toHaveDisplayValue("core");
    expect(screen.getByRole("option", { name: "web" })).toBeInTheDocument();
  });

  it("adopts into the chosen set", () => {
    const onAdopt = vi.fn();
    render(<Unmanaged items={[item]} sets={sets} onAdopt={onAdopt} />);
    fireEvent.change(screen.getByRole("combobox", { name: /set for jq/i }), {
      target: { value: "web" },
    });
    fireEvent.click(screen.getByRole("button", { name: /^adopt$/i }));
    expect(onAdopt).toHaveBeenCalledWith("unmanaged:formula:jq", "web", false);
  });

  it("ignoring passes the ignore flag and no set", () => {
    const onAdopt = vi.fn();
    render(<Unmanaged items={[item]} sets={sets} onAdopt={onAdopt} />);
    fireEvent.click(screen.getByRole("button", { name: /ignore/i }));
    expect(onAdopt).toHaveBeenCalledWith("unmanaged:formula:jq", null, true);
  });

  it("is calm when there is nothing unmanaged", () => {
    render(<Unmanaged items={[]} sets={sets} onAdopt={vi.fn()} />);
    expect(screen.getByText(/nothing unmanaged/i)).toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd app && npm test`
Expected: FAIL — `Cannot find module './Changes'`.

- [ ] **Step 3: Implement the two areas**

`app/src/areas/Changes.tsx`:

```tsx
import Empty from "../components/Empty";
import Row from "../components/Row";
import type { Item } from "../types";

export default function Changes({
  items,
  onApply,
}: {
  items: Item[];
  onApply: (ids: string[]) => void;
}) {
  if (items.length === 0) return <Empty>Nothing to apply</Empty>;

  const actionable = items.filter((i) => i.actionable).map((i) => i.id);

  return (
    <section>
      {actionable.length > 0 && (
        <button
          type="button"
          className="mb-2 rounded bg-neutral-900 px-3 py-1 text-xs text-white"
          onClick={() => onApply(actionable)}
        >
          Apply all
        </button>
      )}
      <ul>
        {items.map((item) => (
          <Row
            key={item.id}
            label={item.label}
            detail={item.detail}
            disabled={!item.actionable}
          >
            <button
              type="button"
              disabled={!item.actionable}
              className="rounded border px-2 py-0.5 text-xs capitalize"
              onClick={() => onApply([item.id])}
            >
              {item.action}
            </button>
          </Row>
        ))}
      </ul>
    </section>
  );
}
```

`app/src/areas/Unmanaged.tsx`:

```tsx
import { useState } from "react";

import Empty from "../components/Empty";
import Row from "../components/Row";
import type { Item, SetEntry } from "../types";

export default function Unmanaged({
  items,
  sets,
  onAdopt,
}: {
  items: Item[];
  sets: SetEntry[];
  onAdopt: (id: string, set: string | null, ignore: boolean) => void;
}) {
  const [chosen, setChosen] = useState<Record<string, string>>({});
  if (items.length === 0) return <Empty>Nothing unmanaged</Empty>;

  const defaultSet = sets[0]?.name ?? "core";

  return (
    <ul>
      {items.map((item) => {
        const value = chosen[item.id] ?? defaultSet;
        return (
          <Row key={item.id} label={item.label} detail={item.detail}>
            <span className="flex items-center gap-1">
              <select
                aria-label={`Set for ${item.label}`}
                className="rounded border px-1 py-0.5 text-xs"
                value={value}
                onChange={(e) =>
                  setChosen({ ...chosen, [item.id]: e.target.value })
                }
              >
                {sets.map((s) => (
                  <option key={s.name} value={s.name}>
                    {s.name}
                  </option>
                ))}
              </select>
              <button
                type="button"
                className="rounded border px-2 py-0.5 text-xs"
                onClick={() => onAdopt(item.id, value, false)}
              >
                Adopt
              </button>
              <button
                type="button"
                className="rounded px-2 py-0.5 text-xs text-neutral-500"
                onClick={() => onAdopt(item.id, null, true)}
              >
                Ignore
              </button>
            </span>
          </Row>
        );
      })}
    </ul>
  );
}
```

- [ ] **Step 4: Wire both into the shell**

In `app/src/App.tsx`, add area tabs and render `<Changes>`/`<Unmanaged>` from
`overview.items.filter((i) => i.area === …)`, calling `applyItems` and
`adoptItem` and replacing state with the `Overview` each command returns.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cd app && npm test && npm run typecheck`
Expected: all PASS.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "feat(app): add changes and unmanaged areas"
```

---

## Task 10: Configs area with diff, and Sets

**Files:**
- Create: `app/src/components/DiffView.tsx`, `app/src/areas/Configs.tsx`, `app/src/areas/Sets.tsx`
- Modify: `app/src/App.tsx`
- Test: `app/src/components/DiffView.test.tsx`, `app/src/areas/Configs.test.tsx`, `app/src/areas/Sets.test.tsx`

**Interfaces:**
- Consumes: `FileDiff`, `DiffLine`, `Item`, `SetEntry` (Task 8); `fileDiff`, `toggleSet` (Task 8).
- Produces: `<DiffView diff>`, `<Configs items onLoadDiff onApply>`, `<Sets entries onToggle>`.

- [ ] **Step 1: Write the failing tests**

`app/src/components/DiffView.test.tsx`:

```tsx
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import type { FileDiff } from "../types";
import DiffView from "./DiffView";

const diff: FileDiff = {
  target: "/Users/test/.gitconfig",
  set: "core",
  lines: [
    { kind: "context", text: "[user]" },
    { kind: "removed", text: "  email = old@example.com" },
    { kind: "added", text: "  email = new@example.com" },
  ],
  truncated: false,
};

describe("DiffView", () => {
  it("marks added and removed lines for screen readers, not just by colour", () => {
    render(<DiffView diff={diff} />);
    expect(screen.getByText(/old@example.com/)).toHaveAttribute(
      "data-kind",
      "removed",
    );
    expect(screen.getByText(/new@example.com/)).toHaveAttribute(
      "data-kind",
      "added",
    );
  });

  it("says so when the diff was truncated", () => {
    render(<DiffView diff={{ ...diff, truncated: true }} />);
    expect(screen.getByText(/truncated/i)).toBeInTheDocument();
  });

  it("says nothing about truncation for a short diff", () => {
    render(<DiffView diff={diff} />);
    expect(screen.queryByText(/truncated/i)).toBeNull();
  });
});
```

`app/src/areas/Configs.test.tsx`:

```tsx
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { FileDiff, Item } from "../types";
import Configs from "./Configs";

const item: Item = {
  id: "local_edit:/Users/test/.gitconfig",
  label: "/Users/test/.gitconfig",
  area: "configs",
  action: "review",
  detail: "core",
  actionable: true,
};

const diff: FileDiff = {
  target: "/Users/test/.gitconfig",
  set: "core",
  lines: [{ kind: "added", text: "  email = new@example.com" }],
  truncated: false,
};

describe("Configs", () => {
  it("loads the diff only when a file is opened", async () => {
    const onLoadDiff = vi.fn().mockResolvedValue(diff);
    render(<Configs items={[item]} onLoadDiff={onLoadDiff} onApply={vi.fn()} />);

    expect(onLoadDiff).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: /\.gitconfig/ }));
    await waitFor(() =>
      expect(onLoadDiff).toHaveBeenCalledWith("/Users/test/.gitconfig"),
    );
    expect(await screen.findByText(/new@example.com/)).toBeInTheDocument();
  });

  it("offers overwriting the local file from the repository", async () => {
    const onApply = vi.fn();
    render(
      <Configs
        items={[item]}
        onLoadDiff={vi.fn().mockResolvedValue(diff)}
        onApply={onApply}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /\.gitconfig/ }));
    fireEvent.click(await screen.findByRole("button", { name: /overwrite/i }));
    expect(onApply).toHaveBeenCalledWith(["local_edit:/Users/test/.gitconfig"]);
  });

  it("is calm when no config was edited", () => {
    render(<Configs items={[]} onLoadDiff={vi.fn()} onApply={vi.fn()} />);
    expect(screen.getByText(/no local edits/i)).toBeInTheDocument();
  });
});
```

`app/src/areas/Sets.test.tsx`:

```tsx
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { SetEntry } from "../types";
import Sets from "./Sets";

const entries: SetEntry[] = [
  { name: "core", active: true, description: "Base set" },
  { name: "mobile", active: false, description: "iOS and Android" },
];

describe("Sets", () => {
  it("shows every set with its description and current state", () => {
    render(<Sets entries={entries} onToggle={vi.fn()} />);
    expect(screen.getByRole("checkbox", { name: /core/i })).toBeChecked();
    expect(screen.getByRole("checkbox", { name: /mobile/i })).not.toBeChecked();
    expect(screen.getByText("iOS and Android")).toBeInTheDocument();
  });

  it("toggling a set reports the new state", () => {
    const onToggle = vi.fn();
    render(<Sets entries={entries} onToggle={onToggle} />);
    fireEvent.click(screen.getByRole("checkbox", { name: /mobile/i }));
    expect(onToggle).toHaveBeenCalledWith("mobile", true);
  });

  it("switching a set off reports false", () => {
    const onToggle = vi.fn();
    render(<Sets entries={entries} onToggle={onToggle} />);
    fireEvent.click(screen.getByRole("checkbox", { name: /core/i }));
    expect(onToggle).toHaveBeenCalledWith("core", false);
  });
});
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd app && npm test`
Expected: FAIL — modules not found.

- [ ] **Step 3: Implement the components**

`app/src/components/DiffView.tsx`:

```tsx
import type { FileDiff } from "../types";

const MARK: Record<string, string> = {
  context: " ",
  added: "+",
  removed: "-",
};

export default function DiffView({ diff }: { diff: FileDiff }) {
  return (
    <div>
      <pre className="max-h-64 overflow-auto rounded bg-neutral-50 p-2 font-mono text-xs leading-5">
        {diff.lines.map((line, i) => (
          <span
            key={i}
            data-kind={line.kind}
            className={
              line.kind === "added"
                ? "block bg-emerald-50 text-emerald-900"
                : line.kind === "removed"
                  ? "block bg-rose-50 text-rose-900"
                  : "block text-neutral-600"
            }
          >
            {MARK[line.kind]} {line.text}
          </span>
        ))}
      </pre>
      {diff.truncated && (
        <p className="mt-1 text-xs text-neutral-500">
          Diff truncated — open the file to see the rest.
        </p>
      )}
    </div>
  );
}
```

`app/src/areas/Configs.tsx`:

```tsx
import { useState } from "react";

import DiffView from "../components/DiffView";
import Empty from "../components/Empty";
import type { FileDiff, Item } from "../types";

export default function Configs({
  items,
  onLoadDiff,
  onApply,
}: {
  items: Item[];
  onLoadDiff: (target: string) => Promise<FileDiff>;
  onApply: (ids: string[]) => void;
}) {
  const [open, setOpen] = useState<string | null>(null);
  const [diff, setDiff] = useState<FileDiff | null>(null);

  if (items.length === 0) return <Empty>No local edits</Empty>;

  async function show(item: Item) {
    if (open === item.id) {
      setOpen(null);
      setDiff(null);
      return;
    }
    setOpen(item.id);
    setDiff(await onLoadDiff(item.label));
  }

  return (
    <ul>
      {items.map((item) => (
        <li key={item.id} className="border-b border-neutral-100 py-2">
          <button
            type="button"
            className="block w-full truncate text-left font-medium"
            onClick={() => void show(item)}
          >
            {item.label}
          </button>
          <span className="text-xs text-neutral-500">{item.detail}</span>
          {open === item.id && diff && (
            <div className="mt-2">
              <DiffView diff={diff} />
              <button
                type="button"
                className="mt-2 rounded border px-2 py-0.5 text-xs"
                onClick={() => onApply([item.id])}
              >
                Overwrite from repository
              </button>
            </div>
          )}
        </li>
      ))}
    </ul>
  );
}
```

`app/src/areas/Sets.tsx`:

```tsx
import type { SetEntry } from "../types";

export default function Sets({
  entries,
  onToggle,
}: {
  entries: SetEntry[];
  onToggle: (name: string, on: boolean) => void;
}) {
  return (
    <ul>
      {entries.map((entry) => (
        <li key={entry.name} className="border-b border-neutral-100 py-2">
          <label className="flex items-center gap-2">
            <input
              type="checkbox"
              checked={entry.active}
              onChange={(e) => onToggle(entry.name, e.target.checked)}
            />
            <span>
              <span className="block font-medium">{entry.name}</span>
              <span className="block text-xs text-neutral-500">
                {entry.description}
              </span>
            </span>
          </label>
        </li>
      ))}
    </ul>
  );
}
```

Note: the checkbox is labelled by the surrounding `<label>`, which is why the
tests can find it by the set name.

- [ ] **Step 4: Wire both into the shell**

In `app/src/App.tsx`, render `<Configs>` with `onLoadDiff={fileDiff}` and
`<Sets>` with entries from `listSets()`, refreshing the overview after a
toggle since changing sets changes what drifts.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cd app && npm test && npm run typecheck`
Expected: all PASS.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "feat(app): add configs area with redacted diffs and set toggles"
```

---

## Task 11: History, window behaviour and autostart

**Files:**
- Create: `app/src/areas/History.tsx`
- Modify: `app/src/App.tsx`, `app/src-tauri/src/lib.rs`, `app/src-tauri/Cargo.toml`
- Test: `app/src/areas/History.test.tsx`

**Interfaces:**
- Consumes: `Commit` (Task 8); `history` (Task 8); `tray::show_main` (Task 7).
- Produces: `<History commits>`; closing the window hides it instead of quitting; autostart registered on first run.

- [ ] **Step 1: Write the failing test**

`app/src/areas/History.test.tsx`:

```tsx
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import type { Commit } from "../types";
import History from "./History";

const commits: Commit[] = [
  { hash: "abc1234", subject: "add jq to core", date: "2026-09-16" },
  { hash: "def5678", subject: "drop s3cmd from infra", date: "2026-09-15" },
];

describe("History", () => {
  it("lists commits newest first with date and subject", () => {
    render(<History commits={commits} />);
    const rows = screen.getAllByRole("listitem");
    expect(rows[0]).toHaveTextContent("add jq to core");
    expect(rows[0]).toHaveTextContent("2026-09-16");
    expect(rows[1]).toHaveTextContent("drop s3cmd from infra");
  });

  it("explains an empty history rather than showing a blank panel", () => {
    render(<History commits={[]} />);
    expect(screen.getByText(/no commits yet/i)).toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd app && npm test`
Expected: FAIL — `Cannot find module './History'`.

- [ ] **Step 3: Implement the history area**

`app/src/areas/History.tsx`:

```tsx
import Empty from "../components/Empty";
import type { Commit } from "../types";

export default function History({ commits }: { commits: Commit[] }) {
  if (commits.length === 0) return <Empty>No commits yet</Empty>;

  return (
    <ul>
      {commits.map((commit) => (
        <li
          key={commit.hash}
          className="flex items-baseline justify-between gap-3 border-b border-neutral-100 py-2"
        >
          <span className="min-w-0 truncate">{commit.subject}</span>
          <span className="shrink-0 font-mono text-xs text-neutral-500">
            {commit.date} {commit.hash}
          </span>
        </li>
      ))}
    </ul>
  );
}
```

Render it in `App.tsx` as a fifth tab, loading `history(50)` when opened.

- [ ] **Step 4: Make closing the window hide it**

A menubar app that quits when its window is closed loses its tray icon. In
`app/src-tauri/src/lib.rs`, add to the builder:

```rust
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // Keep the process — and the tray icon — alive.
                api.prevent_close();
                let _ = window.hide();
            }
        })
```

- [ ] **Step 5: Register autostart**

Add to `app/src-tauri/Cargo.toml` if not already present:

```toml
tauri-plugin-autostart.workspace = true
```

and in the builder:

```rust
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
```

Document in `README.md` under "Background check" that this is the *app's* own
autostart and that the CLI LaunchAgent installed by `dotfix init` is separate
and still required — the background check must keep running when the app is
not installed.

- [ ] **Step 6: Run everything**

Run:

```bash
cd app && npm test && npm run typecheck
cd .. && cargo test --workspace && cargo clippy --all-targets -- -D warnings
```

Expected: all PASS.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "feat(app): add history view, hide-on-close and autostart"
```

---

## Task 12: Logo and icons

**Files:**
- Create: `branding/dotfix.svg`, `branding/dotfix-drift.svg`, `branding/generate-icons.sh`
- Create (generated, committed): `app/src-tauri/icons/*`
- Modify: `README.md`
- Test: `branding/generate-icons.sh` is verified by running it and checking its outputs

**Interfaces:**
- Consumes: `tray::Glyph::asset()` (Task 7) — the file names it returns are the contract this task must satisfy: `icons/tray-quiet.png`, `icons/tray-drift.png`.
- Produces: `icon.icns`, `32x32.png`, `128x128.png`, `128x128@2x.png`, `tray-quiet.png`, `tray-quiet@2x.png`, `tray-drift.png`, `tray-drift@2x.png`.

**Design constraint from the spec:** the menubar case decides the mark. macOS
expects a **template image** there — monochrome, transparent, inverted
automatically for dark menubars and the pressed state. The mark must therefore
read at 16pt with no gradient and no fine detail. Design the monochrome glyph
**first** and derive the colour variant from it, never the other way round.

- [ ] **Step 1: Draw the monochrome master**

`branding/dotfix.svg` — a 1024×1024 artboard, pure black on transparent, no
gradients, no strokes thinner than 32 units at that size (≈0.5pt at 16pt).
`branding/dotfix-drift.svg` is the same mark plus a filled status dot in the
upper right, at least 128 units across so it survives downscaling.

Verification before generating anything: export both at 16×16 and look at
them. If the dot merges into the mark, the dot is too close or too small — fix
the SVG, not the export.

- [ ] **Step 2: Write the generator**

`branding/generate-icons.sh`:

```bash
#!/usr/bin/env bash
# Generate every app and tray asset from the two SVG masters.
# Requires: rsvg-convert (brew install librsvg), iconutil (macOS).
set -euo pipefail

cd "$(dirname "$0")/.."
OUT=app/src-tauri/icons
mkdir -p "$OUT"

render() { # svg size out
  rsvg-convert -w "$2" -h "$2" "$1" -o "$3"
}

# App icon sizes Tauri's bundler expects.
for size in 32 128; do
  render branding/dotfix.svg "$size" "$OUT/${size}x${size}.png"
done
render branding/dotfix.svg 256 "$OUT/128x128@2x.png"

# Tray template images: 16pt at 1x and 2x.
render branding/dotfix.svg 16 "$OUT/tray-quiet.png"
render branding/dotfix.svg 32 "$OUT/tray-quiet@2x.png"
render branding/dotfix-drift.svg 16 "$OUT/tray-drift.png"
render branding/dotfix-drift.svg 32 "$OUT/tray-drift@2x.png"

# .icns via an iconset.
ICONSET=$(mktemp -d)/dotfix.iconset
mkdir -p "$ICONSET"
for size in 16 32 64 128 256 512; do
  render branding/dotfix.svg "$size" "$ICONSET/icon_${size}x${size}.png"
  render branding/dotfix.svg "$((size * 2))" "$ICONSET/icon_${size}x${size}@2x.png"
done
iconutil -c icns "$ICONSET" -o "$OUT/icon.icns"

echo "generated:"
ls -1 "$OUT"
```

- [ ] **Step 3: Run it and verify every asset the code references exists**

```bash
chmod +x branding/generate-icons.sh
./branding/generate-icons.sh
test -f app/src-tauri/icons/tray-quiet.png
test -f app/src-tauri/icons/tray-drift.png
test -f app/src-tauri/icons/icon.icns
```

Expected: all three `test` commands succeed. These are exactly the paths
`Glyph::asset()` returns and `tauri.conf.json` lists — a mismatch here fails at
runtime, not at build time.

- [ ] **Step 4: Verify the app actually shows the glyph**

Run: `cd app && npm run tauri dev`
Expected: an icon appears in the menubar, inverts with a dark menubar, and the
menu opens. Close the window: the icon stays.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(app): add monochrome menubar mark and icon generation"
```

---

## Task 13: Signed, notarized release

**Files:**
- Create: `.github/workflows/release-app.yml`, `app/src-tauri/entitlements.plist`
- Modify: `README.md`
- Test: a dry-run release build locally before the workflow is trusted

**Interfaces:**
- Consumes: the finished app bundle.
- Produces: a notarized `.app` and `.dmg` attached to the GitHub release.

**Dependency:** the Homebrew cask step is **deferred** — `NoiXdev/homebrew-tap`
does not exist yet and its creation was postponed. This task therefore stops at
"notarized DMG on the release page". The cask bump is written here but left
commented out, with the reason inline, so it is one uncomment away when the tap
lands.

- [ ] **Step 1: Add the entitlements**

`app/src-tauri/entitlements.plist` — a menubar app that shells out to `brew`,
`git` and `security` cannot be sandboxed, but hardened runtime is required for
notarization:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>com.apple.security.cs.allow-unsigned-executable-memory</key>
    <true/>
    <key>com.apple.security.cs.disable-library-validation</key>
    <true/>
</dict>
</plist>
```

Reference it from `tauri.conf.json` under `bundle.macOS`:

```json
      "entitlements": "entitlements.plist"
```

- [ ] **Step 2: Verify a local release build first**

Run:

```bash
cd app && npm ci && npm run tauri build
```

Expected: `app/src-tauri/target/release/bundle/dmg/*.dmg` exists. Do this
before writing the workflow — a build that fails locally will fail in CI for
reasons that are much harder to read there.

- [ ] **Step 3: Write the release workflow**

`.github/workflows/release-app.yml`:

```yaml
name: Release App

on:
  workflow_dispatch:
    inputs:
      version:
        description: "Version to release, without the leading v"
        required: true
        type: string

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
      - uses: actions/setup-node@v4
        with:
          node-version: 22
          cache: npm
          cache-dependency-path: app/package-lock.json

      - run: npm ci
        working-directory: app

      - name: Build, sign and notarize
        working-directory: app
        env:
          APPLE_CERTIFICATE: ${{ secrets.APPLE_CERTIFICATE }}
          APPLE_CERTIFICATE_PASSWORD: ${{ secrets.APPLE_CERTIFICATE_PASSWORD }}
          APPLE_SIGNING_IDENTITY: ${{ secrets.APPLE_SIGNING_IDENTITY }}
          APPLE_ID: ${{ secrets.APPLE_ID }}
          APPLE_PASSWORD: ${{ secrets.APPLE_PASSWORD }}
          APPLE_TEAM_ID: ${{ secrets.APPLE_TEAM_ID }}
        run: npm run tauri build -- --target universal-apple-darwin

      - name: Collect the DMG
        run: |
          mkdir -p dist
          cp app/src-tauri/target/universal-apple-darwin/release/bundle/dmg/*.dmg \
             "dist/dotfix-${{ inputs.version }}-macos-universal.dmg"
          shasum -a 256 dist/*.dmg | tee dist/checksum.txt

      - uses: softprops/action-gh-release@v2
        with:
          tag_name: app-v${{ inputs.version }}
          files: |
            dist/*.dmg
            dist/checksum.txt

      # Deferred: NoiXdev/homebrew-tap does not exist yet. Uncomment once it
      # and its bump.yml are in place — see docs/plans/2026-09-16-dotfix-phase-1.md,
      # "Follow-up outside this repository".
      # - name: Bump the cask
      #   env:
      #     GH_TOKEN: ${{ secrets.TAP_TOKEN }}
      #   run: |
      #     SHA=$(cut -d' ' -f1 dist/checksum.txt)
      #     gh workflow run bump.yml --repo NoiXdev/homebrew-tap \
      #       -f formula=dotfix-app -f version="${{ inputs.version }}" -f sha256="$SHA"
```

- [ ] **Step 4: Document the secrets**

Add to `README.md` under Development:

```markdown
Releasing the app requires six repository secrets: `APPLE_CERTIFICATE`,
`APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_ID`,
`APPLE_PASSWORD` (an app-specific password) and `APPLE_TEAM_ID`. Without them
the build still produces a DMG, but macOS will warn on first launch.
```

- [ ] **Step 5: Verify the notarized artefact on a clean machine**

Download the DMG from the release, open it on a Mac that has never built the
app, and confirm no Gatekeeper warning appears. `spctl -a -vvv /Applications/dotfix.app`
must report `accepted`. This is the only way to know notarization worked —
a green workflow does not prove it.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "build(app): add signed and notarized release workflow"
```

---

## Plan self-review

**Spec coverage (Phase 2 scope)**

| Spec requirement | Task |
|---|---|
| Menubar only, no dock icon | 4 (`ActivationPolicy::Accessory`), 7 |
| `core` linked directly, not a subprocess | 4 (crate dependency), 6 |
| Area: Changes (Incoming + Removed), apply individually or all | 5, 9 |
| Area: Unmanaged, adopt into a set or ignore | 5, 9 |
| Area: Configs with diff, write back or overwrite | 3, 10 |
| Area: Sets on/off for this machine | 2, 10 |
| History view | 11 |
| No package search | — deliberately absent from every task |
| App uses `plugin-autostart`; CLI LaunchAgent stays independent | 11 |
| Template image, monochrome first, colour derived | 12 |
| Status-dot tray variant | 7 (`Glyph::Drift`), 12 |
| Notarization, Apple Developer ID available | 13 |
| Cask `dotfix-app` | 13 — **deferred with the tap**, written and commented |
| Secrets always redacted | 3 (redaction in Rust), 6 (error strings), Global Constraints |

**Gaps found and closed during review**

1. `dotfix_core::secrets::redact` had no consumer after Phase 1. It could not
   have one as built — redaction needs resolved values that only the renderer
   sees. Task 3 makes `RenderedFile` carry them and makes `redact` load-bearing
   in `diffview::unified`. This is the plan's most important change to the
   engine, and it is why Task 3 comes before any UI work.
2. The app applies *individual* items, which Phase 1's `plan()` could not
   express. Task 1 adds ids and `plan_selected`, and pins down that a stale id
   is skipped rather than guessed at.
3. Set toggling existed only inside the CLI command. Task 2 moves it to core
   before the app needs it, with the existing CLI integration tests as the
   regression guard.
4. `Drift::LocalEdit` has no "write back to the repository" path in the app.
   `adopt::proposals_for` returns `WriteBackFile` for it, and `adopt_item`
   would pick it — but the Configs area only offers *overwrite*. **Deliberate:**
   writing a hand-edited file back into a set is the one action that silently
   changes what every other machine gets. It stays a CLI operation
   (`dotfix adopt`) in Phase 2. Task 10's UI says "Overwrite from repository"
   and nothing else, so the omission is visible rather than implied.
5. Task 13 cannot complete while the Homebrew tap is deferred. Rather than
   pretend otherwise, the cask step is written, commented, and pointed at the
   phase 1 follow-up note.

**Type consistency**

Checked across tasks: `Drift::id()` (Task 1) is the id used by `plan_selected`
(Task 1), `view::Item.id` (Task 5), `apply_items`/`adopt_item` (Task 6) and
`applyItems`/`adoptItem` (Task 8). `sets::Entry { name, active, description }`
(Task 2) matches `SetEntry` (Task 8) and `<Sets entries>` (Task 10).
`diffview::{FileDiff, DiffLine, LineKind}` (Task 3) matches `FileDiff`/`DiffLine`
(Task 8) and `<DiffView diff>` (Task 10). `Glyph::asset()` (Task 7) returns
`icons/tray-quiet.png` and `icons/tray-drift.png`, which Task 12 generates
under exactly those names. `view::Overview { counts, items, status_line }`
(Task 5) matches `Overview` (Task 8) and `tray::tooltip_for` (Task 7).

**Placeholder scan**

No `TBD`/`TODO`. One intentionally inert block: the commented cask step in
Task 13, whose reason and unblocking condition are stated inline.

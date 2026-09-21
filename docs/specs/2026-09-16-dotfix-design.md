# dotfix — Design

**Date:** 2026-09-16
**Status:** Approved (design)
**Topic:** macOS terminal-environment sync — a set-based Homebrew/dotfiles manager with a CLI, a background check and a menubar app

## Summary

`dotfix` keeps several Macs in sync: Homebrew packages, shell configuration and
application config files. Configuration is grouped into **sets** (`core`, `web`,
`mobile`, `infra`, …); each machine activates the sets it needs. A background
agent detects drift once an hour and surfaces it as a single line at shell
startup; a menubar app shows the same information with one-click actions.

Two repositories are involved and the boundary between them is strict:

- **`NoiXdev/dotfix`** (public) — the tool. Rust workspace, contains no personal
  data and no package lists.
- **`example-user/dotfiles`** (private) — the data. Contains no code.

Sync is **bidirectional with review**: the repository is the source of truth, but
locally installed packages and locally edited config files are detected and
offered for adoption. Nothing is applied without confirmation.

## Goals

- One command to bring a fresh Mac to a known state.
- Per-machine composition of sets rather than one monolithic configuration.
- Machine-independent configuration — no hardcoded home directories or usernames.
- Locally installed packages are noticed and can be adopted into a set.
- Credentials never enter the repository.
- The tool is useful to strangers, not just to its author.

## Non-Goals

- **No package search / discovery UI.** Installing a new package is terminal work
  (`brew install`); `dotfix` manages what already exists.
- **No automatic git conflict resolution.** Divergence is reported; the user
  resolves it with normal git tooling.
- **No enforcement.** `dotfix` never reverts a local change on its own.
- **macOS only.** Homebrew, Keychain and `launchd` are load-bearing.
- **No Nix-style full reproducibility.** The adopt-driven workflow is
  deliberately incompatible with a purely declarative model.

## Rejected alternatives

**chezmoi as the engine.** It would supply templating, file management, `age`
encryption and 1Password integration for free. Rejected because chezmoi has no
first-class notion of sets — profiles must be emulated with template
conditionals and `.chezmoiignore`, making the project's central concept the
awkward part. It would also mean two mental models and a UI parsing CLI output.
The apparent secrets win is small: each provider is a single shell invocation.

**nix-darwin / Home Manager.** The correct answer for reproducible machines, but
fundamentally opposed to the adopt-after-the-fact workflow required here, and
disproportionate in learning cost for a terminal setup.

## Repositories and artifacts

| Artifact | Location | Visibility | Phase |
|---|---|---|---|
| Tool, CI, logo | `NoiXdev/dotfix` | public | 1 |
| Data repository | `example-user/dotfiles` | private | 1 |
| Spec and documentation | `NoiXdev/noix-docs` | public site | 0 |
| Product page (de + en) | `NoiXdev/noix.dev` | public | 3 |
| Homebrew tap | `NoiXdev/homebrew-tap` | public | 1 |

The public repository must contain no package lists, no machine names and no
references to internal infrastructure. Naming follows the existing product
family (`notefix`, `kontorfix`), hence `dotfix`; the binary is `dotfix`.

### Tool layout

```
dotfix/
├── crates/core/     # all logic, as a library
├── crates/cli/      # binary `dotfix`
└── app/             # Tauri v2 menubar app (phase 2)
```

The app links `core` **directly as a crate** — no subprocess, no text parsing,
identical types in CLI and UI.

## Data model

```
dotfiles/
├── dotfix.toml            # schema version, global defaults
├── machines/
│   ├── mbp-work.toml
│   └── mbp-home.toml
└── sets/
    ├── core/
    │   ├── set.toml
    │   ├── shell/         # .zshrc fragments, numeric prefixes
    │   │   ├── 00-p10k-instant-prompt.zsh
    │   │   ├── 10-omz.zsh
    │   │   └── 20-aliases.zsh
    │   └── files/
    │       └── gitconfig.tmpl
    ├── web/ mobile/ infra/ media/ native/
```

### Set

```toml
description = "Mobile development: iOS & Android"

[packages]
brew = ["cocoapods", "ios-deploy", "libimobiledevice", "xcodegen", "openjdk@17"]
cask = ["android-commandlinetools"]

[[files]]
source = "files/gradle.properties.tmpl"
target = "~/.gradle/gradle.properties"
mode   = "template"        # template | symlink | copy
```

### Machine

```toml
sets            = ["core", "web", "infra"]
secret_provider = "keychain"     # keychain (default) | 1password | age
# vault         = "Privat"       # 1password only

[vars]
git_email = "..."
```

`sets` is exactly the switch the app toggles. `vars` feed the templates.

### Initial set split

Derived from the author's current 25 top-level formulae and 4 casks; a starting
point that `dotfix adopt` lets the user correct interactively.

| Set | Packages |
|---|---|
| `core` | fzf, gh, powerlevel10k, actionlint |
| `web` | ddev, composer, caddy, libpq@16 |
| `mobile` | cocoapods, ios-deploy, libimobiledevice, xcodegen, openjdk@17, android-commandlinetools |
| `infra` | ansible, ansible-lint, hcloud, s3cmd, samba, sshpass |
| `media` | ffmpeg, librsvg, poppler |
| `native` | llvm, pkgconf |

### Templates

Files with `mode = "template"` receive `{{ home }}`, `{{ user }}`,
`{{ machine }}` plus everything from `[vars]`. This is what makes the
configuration machine-independent — e.g. `export PNPM_HOME="{{ home }}/Library/pnpm"`
instead of a hardcoded `/Users/<name>/…`.

### Generated `.zshrc`

`.zshrc` is **generated, not synced**: all `shell/*.zsh` fragments of the active
sets, ordered by numeric prefix, concatenated. The prefix is load-bearing — the
Powerlevel10k instant-prompt block must stay at the very top, hence `00-`.

The file ends with `source ~/.zshrc.local` if that file exists: an unmanaged
escape hatch for machine-specific additions.

The generated file carries a header marker and a checksum. If it is edited by
hand, the next run detects this and offers adoption or overwrite rather than
silently discarding the change.

## Engine

### Three states, not two

Comparing only *desired* against *actual* is insufficient: it cannot distinguish
"another machine added `jq`" from "this machine deliberately removed `jq`". Both
look identical. Therefore:

| State | Source | Meaning |
|---|---|---|
| **Desired** | repository, resolved for this machine | what should be |
| **Actual** | `brew list`, filesystem | what is |
| **Applied** | `~/.local/state/dotfix/applied.json` | what dotfix last wrote |

The comparison is a three-way diff. `Applied` is the memory that turns a
comparison into a decision.

### Drift classes

1. **Incoming** — present in the repository, not applied locally → *apply?*
2. **Unmanaged** — installed locally, in no set → *adopt into which set, or
   ignore permanently?*
3. **Removed** — was in a set, has been removed from it, still installed
   locally → *uninstall?*
4. **LocalEdit** — managed file edited locally → *write back to the set, or
   overwrite?*

Class 2 only reports; class 3 proposes uninstallation. Uninstall is proposed
**only for leaves** — `brew uses --installed` is checked first, otherwise the
entry is shown as "skipped, still required by X". This matters concretely:
`openjdk@17`, `libpq@16` and `pkgconf` are dependencies of other packages.

The menubar app renders exactly these four classes and implements no logic of
its own.

### Commands

```
dotfix status      # what differs (dry run, never writes)
dotfix apply       # apply Incoming + Removed, after confirmation
dotfix adopt       # take Unmanaged / LocalEdit into the repository, interactive
dotfix sets        # toggle this machine's sets
dotfix doctor      # verify setup: git, brew, secret provider, LaunchAgent, PATH, SSH
dotfix init        # first-time setup (see below)
```

Every command supports `--json`, which is also the format used for scripting and
tests.

### Guarantees

- **No implicit merges.** `dotfix` pulls with `--ff-only`; on divergence it
  reports and stops.
- **No write without backup.** Managed files are copied to
  `~/.local/state/dotfix/backups/<timestamp>/` before being overwritten.
- **Plan before execution.** `apply` computes and displays the full plan, then
  asks.

## Shell integration and background check

### Shell hook

The fragment added to the generated `.zshrc` starts no process:

```zsh
# 99-dotfix-status.zsh
() {
  local f=${XDG_STATE_HOME:-$HOME/.local/state}/dotfix/status.line
  [[ -s $f ]] && print -r -- "$(<$f)"
}
```

A zsh builtin reading a small file — no process spawn, no network, no `brew`.
When there is nothing to do the file is empty and **nothing is printed**: no
"everything up to date", no blank line. Otherwise:

```
↯ dotfix: 2 new packages · 1 changed config   →  dotfix apply
```

### LaunchAgent

`~/Library/LaunchAgents/dev.noix.dotfix.plist`, `RunAtLoad = true` and
`StartInterval = 3600`. It runs `dotfix status --write-status-line`: pull,
compute the four drift classes, write or clear the status line. On network
failure it exits silently and leaves the previous line untouched.

A LaunchAgent runs at **login**, not at system boot; boot-time execution would
require a LaunchDaemon running as root, which could reach neither the user
Keychain nor Homebrew correctly. Login plus hourly is the intended behaviour.

Installed by `dotfix init`, verified by `dotfix doctor`. Two failure modes
`doctor` checks explicitly, because both fail *silently* otherwise:

- A LaunchAgent does not inherit the interactive `PATH`; `brew` and `git` must be
  set in the plist.
- The remote is reached over SSH. Whether the agent can use the SSH key depends
  on `AddKeysToAgent` / `UseKeychain` in `~/.ssh/config`.

## Secrets

Secrets are **named in the set and located by the machine**. Writing the
provider reference directly into the template would break as soon as a second
machine uses a different provider.

In the set:

```
access_key = {{ secret "s3_access_key" }}
```

On a 1Password machine:

```toml
secret_provider = "1password"
vault           = "Privat"
[secrets]
s3_access_key = "op://Privat/s3cmd/access_key"   # optional override
```

On a Keychain machine no mapping is needed; each provider derives a default
location from the name (Keychain: service `dotfix`, account `<name>`;
1Password: `op://<vault>/dotfix/<name>`). Only exceptions are configured.

### Providers

| Provider | Mechanism | Role |
|---|---|---|
| `keychain` | `security find-generic-password -w` | **default** — OS-native, no dependency |
| `1password` | `op read`, vault selectable per machine | opt-in |
| `age` | decrypt file in the repository | opt-in |

Keychain is the default so that a fresh Mac — and any stranger installing the
tool — works without buying a licence. `doctor` verifies the `op` CLI and vault
access only on machines configured for 1Password.

### Rules

- Rendered files containing secrets are written `0600`; their backups likewise.
- Secret values are **always redacted** in `status`, diffs, logs and the app.
  The app shows `s3_access_key → resolvable`, never the value.
- `adopt` enforces a deny list (`id_*`, `*.pem`, `.netrc`, `.aws/credentials`,
  `.s3cfg`, `*token*`). Instead of adopting such a file it proposes creating a
  template with a secret reference. The most common way credentials reach a
  dotfiles repository is an unconsidered "adopt everything", so the tool blocks
  precisely there.

## Bootstrap

```bash
/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/NoiXdev/dotfix/<tag>/install.sh)"
dotfix init
```

The installer lives in the repository rather than on the website, avoiding
coupling to the `noix.dev` deployment. It pins a **release tag**, not `main`, so
an intermediate commit can never break a fresh machine. It installs Homebrew if
missing, then `dotfix` via `NoiXdev/homebrew-tap`.

`dotfix init` offers two paths:

**A — Set up new** (no data repository yet)
Create the repository skeleton → run `adopt` across this Mac: existing formulae,
casks and `.zshrc` are analysed and a set split is *proposed* for correction →
choose secret provider → initial commit and push → install LaunchAgent.

**B — Sync from existing** (second Mac)
Clone → machine name → select active sets → secret provider (and vault) →
display the apply plan → write only after confirmation → install LaunchAgent.

The secret provider must be available before the first `apply`; `doctor` checks
this and states what is missing rather than applying partially.

## Desktop app (phase 2)

Stack mirrors `notefix`: React + Vite + Tailwind, Tauri v2, no dock icon
(`ActivationPolicy::Accessory`), menubar only. `core` is linked directly.

| Area | Shows | Action |
|---|---|---|
| Changes | Incoming + Removed | apply individually or all |
| Unmanaged | Unmanaged | adopt → choose set, or ignore |
| Configs | LocalEdit, with diff | write back or overwrite |
| Sets | this machine's sets | on / off |

Plus a history view (the data repository's `git log`, presented readably). No
package search.

The app uses `plugin-autostart` for itself. The CLI LaunchAgent remains
independent — it must also run when the app is closed or not installed.

## Logo

The menubar case dictates the design: macOS expects a **template image** there —
monochrome, transparent, adapting to light/dark menubar and inverting on click.
The mark must therefore work at ~16pt without gradients or fine detail. The
monochrome glyph is designed **first**; the coloured variant for the app icon and
`noix.dev` is derived from it, not the other way round.

An SVG master lives in the repository; a script generates `.icns` and tray assets
at `@1x`/`@2x`. Styling follows the existing NoiXdev wordmarks. A second tray
variant carries a status dot (quiet / drift present).

## CI/CD and distribution

Two artifacts, two release paths:

- **Homebrew formula `dotfix`** — the CLI.
- **Homebrew cask `dotfix-app`** — the menubar app, signed and notarized
  (Apple Developer ID is available).

`ci.yml` on every PR: `cargo fmt --check`, `cargo clippy -D warnings`,
`cargo test`, plus the conventional-commit gate used across the organisation.

Phase 1 uses a small repository-local release workflow for the CLI tarball and
the tap bump. Phase 2 attaches the app to the shared
`NoiXdev/github-workflows` → `create_tauri_changelog_version_release.yaml`.

Two gaps in the shared workflow, to be addressed rather than worked around:

1. It builds macOS + Windows + Linux. `dotfix` is macOS-only, so the other two
   jobs would fail. The clean fix is a **`platforms` input** on the shared
   workflow, which benefits every repository and matches the organisation's own
   rule that shared workflows must be genuinely reusable.
2. It knows nothing about a CLI binary or a Homebrew tap — hence the separate
   phase 1 release path above.

## Testing

`core` does not talk to the outside world directly but through a narrow
interface (`brew`, filesystem, `git`). Tests replace it with fakes, making the
entire drift logic testable without real Homebrew and without a real repository.
This is a precondition for the TDD workflow used across these projects — for a
tool that invokes `brew uninstall`, tests are not optional.

- **Unit (core):** set resolution, three-way diff, all four drift classes, leaf
  detection, template rendering, fragment ordering, checksum handling.
- **Integration (CLI):** `init`/`status`/`apply`/`adopt` against a temporary
  repository and a fake brew, asserting on `--json` output.
- **Manual:** the app, and the real second-Mac run.

## Phases

| Phase | Content | Outcome |
|---|---|---|
| **0** | Spec and documentation scaffold in `noix-docs` | scope agreed |
| **1** | `core` + CLI + `adopt` + LaunchAgent + shell hook + secrets + CI + tap | works on Mac 1, then Mac 2 — the point of real value |
| **2** | Menubar app + logo + notarization + cask | graphical surface |
| **3** | Product page (de + en) on `noix.dev` | public |

Phase 1 is deliberately useful on its own: if the project stops there, the result
is a working sync setup rather than half a product.

## Decisions

| Question | Decision |
|---|---|
| Set granularity | packages + shell fragments + application configs |
| Sync direction | bidirectional, with review; nothing applied unconfirmed |
| Removal from a set | propose uninstall (leaves only) |
| Locally installed, in no set | report as unmanaged; never auto-removed |
| Shell startup | async, cached, throttled; one line, silent when clean |
| Trigger | LaunchAgent at login + hourly |
| Secret provider | Keychain default; 1Password (vault selectable) and age opt-in |
| Secret references | logical name in the set, location per machine |
| Engine | own Rust workspace; chezmoi and nix rejected |
| App scope | menubar + set editor + config diff + history; no package search |
| Installer | in the repository, pinned to a release tag |

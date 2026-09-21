<p align="center">
  <img src="branding/dotfix-icon.svg" alt="" width="96" height="96">
</p>

<h1 align="center">dotfix</h1>

<p align="center">
  Keep several Macs in sync — Homebrew packages, shell configuration and
  application config files, grouped into <strong>sets</strong> that each
  machine activates as it needs them.
</p>

<p align="center">
  <a href="https://github.com/NoiXdev/dotfix/actions/workflows/ci.yml"><img src="https://github.com/NoiXdev/dotfix/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <img src="https://img.shields.io/badge/macOS-13%2B-black" alt="macOS 13 or later">
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue" alt="MIT licence"></a>
  <a href="https://docs.noix.dev/dotfix/getting-started/introduction/"><img src="https://img.shields.io/badge/docs-docs.noix.dev-informational" alt="Documentation"></a>
</p>

---

A second Mac starts out as a week of small annoyances: the formula you forgot
to install, the alias that only exists on the other machine, the config file
you edited once and never copied over.

dotfix takes the declarative route. You describe what a machine should have; it
reports what differs and changes only what you approve. Nothing is applied
behind your back, and a file you edited by hand is never silently overwritten.
It comes as a command-line tool and a menubar app, both driving the same steps.

**[Read the documentation →](https://docs.noix.dev/dotfix/getting-started/introduction/)**

## Install

No release is tagged yet. Build it from source — you need a
[Rust toolchain](https://rustup.rs/):

```bash
git clone https://github.com/NoiXdev/dotfix.git
cd dotfix
cargo install --path crates/cli
```

The menubar app is optional and built separately; see
[Development](#development).

Then set this Mac up from whichever side you prefer — both run the same
steps.

**From the app.** A Mac the menubar app has never run on opens a short setup
flow instead of the usual window: build a repository from what is installed
here, or clone one you already have. It checks what is missing before it
writes anything, and names the step that failed if one does.

**From the terminal.**

```bash
dotfix init --set-up-new      # first machine: build a repository from this Mac
dotfix init                   # every machine after that: clone and apply
```

The hourly background check is the command-line tool running itself, so the
app installs that agent only when `dotfix` is on your `PATH` — and says so
rather than installing one that could never run.

## How it works

Your configuration lives in a **private git repository of your own**; dotfix
itself stores nothing about you. A set is a directory with a package list,
shell fragments and managed files:

```
sets/web/
├── set.toml
├── shell/20-web.zsh
└── files/example.tmpl
```

Each machine picks its sets in `machines/<name>.toml`. `.zshrc` is *generated*
from the fragments of the active sets, so turning a set off removes its lines.
Anything machine-specific that should never be shared goes in `~/.zshrc.local`,
which the generated file sources at the end.

Files can be templates, so nothing is tied to one machine:

```
export PNPM_HOME="{{ home }}/Library/pnpm"
```

## Sync model

dotfix compares three things: what the repository wants, what is on the
machine, and what dotfix itself last wrote. That third one is what lets it tell
"another Mac added this package" apart from "I removed it here on purpose".

| Situation | dotfix says |
|---|---|
| In the repository, not installed here | apply it? |
| Installed here, in no set | adopt into a set, or ignore? |
| Removed from its set, still installed | uninstall it? (only if nothing depends on it) |
| Installed by dotfix, removed here by hand | drop it from the set, or reinstall? |
| Managed file edited by hand | write it back, or overwrite? |

Nothing is ever applied without confirmation, nothing is overwritten without a
backup, and a diverged repository is reported rather than merged.

## Commands

| Command | Purpose |
|---|---|
| `dotfix status` | what differs (never writes; `--json` for scripting) |
| `dotfix apply` | apply incoming changes and removals, after confirmation |
| `dotfix adopt` | take locally installed packages or edited configs into the repository |
| `dotfix sets` | list or toggle this machine's sets |
| `dotfix doctor` | verify the setup |
| `dotfix init` | first-time setup |

## Secrets

Secrets never enter the repository. A set refers to a logical name:

```
access_key = {{ secret("s3_access_key") }}
```

and each machine says where that name lives — macOS Keychain (the default,
needs nothing installed), 1Password, or an `age`-encrypted file. Files that
resolve a secret are written `0600`, and values are redacted from all output.
`dotfix adopt` refuses files that look like credentials and proposes a template
instead. It also refuses to write a hand-edited file back into the repository
when that file was rendered from a template resolving a secret: the copy on
disk holds the real value, so writing it back would commit the secret and
replace the `{{ secret("name") }}` placeholder with it. Edit the template
instead.

## Background check

A LaunchAgent runs at login and once an hour, writing a single status line that
your shell prints at startup:

```
↯ dotfix: 2 changes · 1 changed config   →  dotfix apply
```

When nothing differs it prints nothing at all. The shell hook is a file read,
not a process — it costs nothing per terminal tab.

`dotfix doctor` checks the two things that otherwise fail silently: a
LaunchAgent does not inherit your interactive `PATH`, and it may not reach the
SSH key your git remote needs.

The menubar app registers itself as a login item too, so its tray icon is
available as soon as you log in. It does that only when it is running from
`/Applications` or `~/Applications`: the registration records the path it is
launched from, so a development build or a freshly bundled `.app` still
sitting in `target/` would otherwise claim the login item and then vanish at
the next `cargo clean`, leaving nothing to start at login and no error to
explain it. A copy running from anywhere else says so and leaves the login
item alone. If the registration itself fails — a managed Mac may forbid it —
the app reports it and runs anyway; you simply have to start it yourself.
That is a
**separate, additional** registration — it is not the LaunchAgent above and
does not replace it. The LaunchAgent installed by `dotfix init` is what runs
the hourly background check and writes the status line; it keeps working
whether or not the app is installed, running, or even open. Do not remove the
LaunchAgent thinking the app's login item covers it, and do not treat the
app's autostart as optional plumbing that only affects the window — disabling
either one only disables that one. The app's bundle identifier happens to be
`dev.noix.dotfix` — the same as the CLI LaunchAgent's Label — so its autostart
plugin must keep naming its own login item after the product name ("dotfix")
rather than the bundle identifier; naming it `dev.noix.dotfix` would make it
overwrite the CLI's LaunchAgent file and silently kill the background check.

## Development

```bash
cargo test --workspace
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```

`dotfix-core` holds all logic and talks to Homebrew, the filesystem and git
only through traits, so the entire drift engine is tested against in-memory
fakes. The CLI integration tests stub `brew` with a script on `PATH`, which
exercises the real command-building code rather than bypassing it.

Releasing the app requires six repository secrets: `APPLE_CERTIFICATE`,
`APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_ID`,
`APPLE_PASSWORD` (an app-specific password) and `APPLE_TEAM_ID`. The workflow
checks all six before it builds anything and stops if one is missing, and
after building it verifies the app is not merely ad-hoc signed — so an
unsigned or unnotarized DMG is never published rather than published with a
warning on first launch.

### Icons

`branding/` holds the mark as source SVGs: `dotfix.svg` and `dotfix-drift.svg`
are the monochrome, black-on-transparent template images the menubar tray
uses (macOS inverts these itself for dark menubars and the pressed state);
`dotfix-icon.svg` is the coloured variant, same bar geometry, used for the
Dock-facing app icon. Regenerate every exported asset from the three masters
with:

```bash
./branding/generate-icons.sh
```

This requires `rsvg-convert` (`brew install librsvg`) and macOS's built-in
`iconutil`, and writes exactly the files `app/src-tauri/tauri.conf.json` and
`app/src-tauri/src/tray.rs` reference under `app/src-tauri/icons/`, no more:
the tray glyphs are single images embedded with `include_bytes!`, so there
are no `@2x` tray variants to generate. Commit the result, it is not built by
CI.

Releases are cut by pushing a `v*` tag: the workflow builds a universal binary,
attaches it to the release and bumps the Homebrew formula in
`NoiXdev/homebrew-tap` using the `TAP_TOKEN` secret — a fine-grained PAT scoped
to that repository only.

> **Not yet wired up.** `NoiXdev/homebrew-tap` does not exist, so the formula
> in `packaging/dotfix.rb` has nowhere to be bumped to and the release
> workflows cannot complete. Creating the tap is what unblocks the first
> release, and with it the one-line installer.

## Documentation

The user documentation lives at
[docs.noix.dev/dotfix](https://docs.noix.dev/dotfix/getting-started/introduction/):

- [Your first machine](https://docs.noix.dev/dotfix/getting-started/first-machine/) — build a repository from the Mac you already use
- [Another Mac](https://docs.noix.dev/dotfix/getting-started/another-machine/) — join an existing repository over SSH, a deploy key, or a token
- [Sets](https://docs.noix.dev/dotfix/guide/sets/) — group packages, files and shell fragments
- [Secrets](https://docs.noix.dev/dotfix/guide/secrets/) — reference a password without storing it
- [CLI commands](https://docs.noix.dev/dotfix/reference/cli-commands/) — every command and option

## License

MIT

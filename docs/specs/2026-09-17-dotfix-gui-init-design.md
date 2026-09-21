# dotfix GUI onboarding — Design

**Date:** 2026-09-17
**Status:** Approved (design)
**Topic:** Running the first-time setup from the menubar app instead of sending the user to a terminal

## Summary

Today a machine without `~/.config/dotfix/config.toml` gets a window that says
"run `dotfix init` in a terminal". That is an odd thing for a graphical app to
say about its own setup, and it means the app cannot be the whole story on a
fresh Mac.

This design moves the init logic out of the CLI into `dotfix-core` as four
discrete steps, and drives them from a short wizard in the app. Both paths are
covered: building a repository from what is installed here, and cloning an
existing one. The CLI keeps its own prompts and calls the same four steps.

The hard part is not the wizard. It is that a GUI app has **no controlling
terminal**, so every command that might ask a question — an SSH passphrase, a
host-key confirmation — hangs or fails instead of prompting. Most of this
design is about that.

## Goals

- A fresh Mac can be set up entirely from the app.
- Every failure names the step that failed and what to do about it, because the
  user has no terminal open to investigate.
- The app never holds a secret value, in keeping with the project's existing
  secret model.
- `dotfix init` on the command line keeps working unchanged.

## Non-Goals

- **No set splitting in the wizard.** Everything installed goes into one `core`
  set; the existing Sets area is where it gets divided up, with the package
  list in view. That choice is revised several times in practice and does not
  belong in a first-run flow.
- **No credential storage of dotfix's own.** Tokens go to the macOS Keychain
  via git's credential helper; SSH keys stay in `~/.ssh`.
- **No account-wide GitHub access.** Repository creation goes through the
  user's own authenticated `gh`, never through a token the app holds.
- No Windows or Linux paths.

## Decisions

| Question | Decision |
|---|---|
| Which paths does the GUI cover? | Both: build-new and clone, with pre-flight checks before either |
| Authentication for cloning | Detect working SSH first; otherwise a generated deploy key; HTTPS token as a third option |
| Where a token is stored | macOS Keychain via `git credential-osxkeychain` — dotfix forgets it immediately |
| Creating the remote repository | `gh repo create --private` when `gh` is present and authenticated, after an explicit confirmation |
| Wizard depth | Machine name, path, URL when cloning, secret provider (and vault) — nothing else |
| Where init logic lives | `dotfix_core::init`, four steps, called by both the CLI and the app |

## Rejected alternatives

**One `init(Options) -> Result<()>` call.** Smallest surface, but all-or-nothing:
if cloning succeeds and the agent installation fails, the user cannot be told
where it stopped, and a retry trips over the half-built directory. The whole
point of this feature is failure reporting.

**The app shells out to the `dotfix` CLI.** Least code, but it contradicts the
phase 2 rule that the app links `core` directly — no subprocess, no parsed
text — and it would make the graphical path depend on the terminal tool being
installed, which on a fresh Mac it need not be.

## The four steps

New module `crates/core/src/init/`. What the caller must decide is a value, not
a sequence of prompts:

```rust
pub struct Plan {
    pub machine: String,
    pub source: Source,
    pub secret_provider: ProviderKind,
    pub vault: Option<String>,        // 1Password only
}

pub enum Source {
    /// Build a repository from what is installed on this machine.
    New,
    /// Clone an existing repository.
    Clone { url: String },
}
```

```rust
pub fn preflight(…, plan: &Plan) -> Preflight;                        // checks, writes nothing
pub fn create_or_clone(…, plan: &Plan) -> Result<PathBuf>;            // the only step that creates the repo
pub fn configure_machine(…, repo: &Path, plan: &Plan) -> Result<()>;  // machines/<name>.toml + local pointer
pub fn install_agent(…) -> Result<PathBuf>;                           // delegates to the existing agent::install
```

`Preflight` returns `Vec<doctor::Check>` — the exact type `dotfix doctor`
already uses. Setup and diagnosis therefore speak one vocabulary and share one
renderer, and whatever the wizard checks up front can be re-checked later with
`doctor`, yielding the same lines.

**Ordering is binding.** `preflight` never writes. Only once it passes does
`create_or_clone` touch the filesystem.

`crates/cli/src/cmd/init.rs` shrinks to its prompts plus these four calls. The
existing CLI integration tests are the regression guard and must not change.

## Pre-flight and authentication

Checks for `New`: git present, brew present, target directory free, `$HOME`
writable, and whether `gh` is installed **and authenticated** — that last one
decides whether repository creation is offered at all.

For `Clone`, additionally the only question that matters: can we reach the
remote?

### The detection ladder

```
ssh -T git@github.com -o BatchMode=yes -o ConnectTimeout=5
```

`BatchMode=yes` is not optional. It forbids SSH any interactive prompt; without
it this call from a GUI with no TTY would do exactly what the design exists to
prevent — wait for input nobody can give. With it we get a prompt failure and
know the path does not carry.

Succeeds → clone directly, no configuration needed. Fails → offer a deploy key,
with an HTTPS token as the fallback.

### Deploy key

1. Generate `~/.ssh/dotfix_<machine>_ed25519`, mode `0600`, **no passphrase** —
   a passphrase would require an agent again, which is the problem being solved.
2. Append a stanza to `~/.ssh/config` with a dedicated host alias and
   `IdentitiesOnly yes`, so the key applies to this repository only and does
   not reach into the user's other SSH destinations.
3. Public key to the clipboard, open `…/settings/keys/new`, and state plainly
   that **"Allow write access"** must be ticked — without it dotfix cannot push
   anything back.
4. A "test connection" button that re-runs step 1 of the ladder.

A deploy key is scoped to a single repository, and one per machine means
revoking a machine is deleting one key. Both are improvements on a shared
token.

### Host keys

The obvious approach is `ssh-keyscan github.com >> known_hosts`, i.e. trust on
first use. Instead dotfix compares the scanned fingerprint against GitHub's
**published** host-key fingerprints, compiled into the binary. On a mismatch
nothing is written and the wizard reports what it expected and what it got.

The cost is real and stated here so it is not a surprise: when GitHub rotates a
host key, dotfix needs an update or it refuses to proceed. That is the intended
trade — the alternative is a first-time setup on a hostile network silently
pinning the wrong host.

### Token path

Passed once to `git credential-osxkeychain store`. It then lives in the
Keychain and git retrieves it itself; dotfix does not persist it. This is what
keeps the README's claim that the app holds no secrets true.

### Repository creation

`gh repo create --private` runs only after a confirmation showing owner, name
and visibility — before anything happens to the user's account.

## Failure handling

The wizard shows four lines, one per step, each with its state. When one fails,
the preceding steps stay done and only the failed one offers a retry. There is
no blanket "setup failed": the user has no terminal open, so the line itself has
to be the explanation — not "clone failed" but "host key for github.com is not
known", with the button that fixes exactly that.

**No half-built state.** `create_or_clone` is the only step that creates the
repository directory. If it fails after writing something, it removes its own
directory before reporting, so a retry does not trip over a half-built
`~/dotfiles`. This is the same defect already fixed once in
`init --set-up-new`, anticipated rather than repeated.

**Interruption.** The wizard holds no state. Each launch runs the same check
that exists today: if there is no local configuration, the wizard starts from
the beginning. A repository that was created but never configured stays on disk
and surfaces in pre-flight as "target directory already exists", with the choice
to reuse it or pick another path.

## Testing

The four steps run against the existing fakes — `FakeFsys`, `FakeGit`,
`FakeBrew`, `FakeExec`. The authentication ladder is pure command-construction
logic and is therefore testable through `FakeExec`: which command with which
arguments, and which response leads to which branch. Fingerprint pinning gets a
test with a deliberately wrong fingerprint, which must result in **nothing**
being written to `known_hosts`.

**What tests do not cover:** real SSH, real network, a real GitHub response.
That boundary is stated up front rather than discovered afterwards — after
implementation this needs one run on a real machine, as the tray did. The
lesson from phase 2 is precisely that a green test run is no substitute for
actually starting the thing.

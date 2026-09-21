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
  missing: Missing[];
  undeclared: Undeclared[];
  status_line: string | null;
}

/** Software a set declares that this machine does not have. */
export interface Missing {
  name: string;
  set: string;
  /** What to run. Text for the user — dotfix never executes it. */
  hint: string | null;
}

/** A managed file. `source` is repository-relative, so opening it cannot
 * point outside the repository. */
/** Software present here that no active set declares. */
export interface Undeclared {
  name: string;
  hint: string | null;
}

export interface SetFile {
  target: string;
  source: string;
}

export interface SetFragment {
  name: string;
  source: string;
}

export interface SetEntry {
  name: string;
  active: boolean;
  description: string;
  brew: string[];
  cask: string[];
  files: SetFile[];
  /** In concatenation order, which is filename order. */
  fragments: SetFragment[];
  config_path: string;
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

/**
 * Mirrors NOT_CONFIGURED_PREFIX in app/src-tauri/src/ctx.rs. When a command
 * fails with a message starting like this, dotfix has simply never been set up
 * on this machine — that is a state to explain calmly, not an error to alarm
 * the user with. A Rust test pins the prefix on the other side.
 */
export const NOT_CONFIGURED_PREFIX =
  "dotfix is not set up on this machine yet.";

export const isNotConfigured = (message: string): boolean =>
  message.startsWith(NOT_CONFIGURED_PREFIX);

/**
 * Mirrors `dotfix_core::doctor::Check` — one requirement `init_preflight`
 * looked at (git installed, Homebrew present, `gh` logged in, ...) and
 * whether this machine already satisfies it.
 *
 * `blocking` is the backend's own decision about whether a failure here must
 * stop setup, serialised precisely so this side never has to re-derive it:
 * `gh` is reported but does not block, and a wizard that gated on "every
 * check passed" left a stock Mac without `gh` unable to set up at all.
 */
export interface Check {
  name: string;
  ok: boolean;
  detail: string;
  blocking: boolean;
}

/**
 * Mirrors `dotfix_core::init::Preflight`, the result of `init_preflight`.
 * Nothing blocks when every check either passed or was non-blocking — the
 * same rule `Preflight::passes()` applies on the Rust side.
 */
export interface Preflight {
  checks: Check[];
}

/**
 * Mirrors `commands::FailedStep` — which of `init_run`'s steps stopped the
 * run, and the message for that step specifically. Never guess which step a
 * bare error string came from; this is how `init_run` says it precisely.
 */
export interface FailedStep {
  step: string;
  message: string;
}

/**
 * Mirrors `commands::StepOutcome`, what `init_run` reached. `repo` is
 * `null` rather than absent so the frontend never has to distinguish "not
 * present" from "explicitly none" when serde carries an `Option<String>`
 * across the Tauri bridge. `failed` is `null` on success — `init_run`
 * itself resolves normally even when a step failed partway through, so a
 * mid-run failure is read from this field, not from a rejected promise.
 */
export interface StepOutcome {
  completed: string[];
  repo: string | null;
  failed: FailedStep | null;
  /** Whether an existing `~/.zshrc` was taken into the repository. */
  imported_zshrc: boolean;
}

/**
 * The wizard's form state, in the camelCase this codebase's TypeScript
 * uses. `api.ts` converts this to the snake_case `WizardPlan` shape Rust
 * expects in exactly one place, so no component here has to think about
 * that boundary.
 */
export interface WizardAnswers {
  machine: string;
  mode: "new" | "clone";
  url: string;
  secretProvider: "keychain" | "1password" | "age";
  vault: string;
}

/**
 * Mirrors `commands::DeployKeyView`, the result of `init_deploy_key`. Only
 * the public half and the ssh host alias cross into the webview — the
 * private key's path deliberately does not, since dotfix holds no
 * credential of its own and the private half never has a reason to leave
 * the machine it was generated on.
 */
export interface DeployKeyView {
  public: string;
  host_alias: string;
}

/**
 * Mirrors `commands::CloneTarget` — the URL a clone will really use, given
 * how it ended up authenticating, and the host key (if any) that has to be
 * pinned first.
 *
 * What the user typed is not always what can be cloned: a deploy key is
 * only ever offered for its own ssh alias, and a token only over https. The
 * backend derives this so the wizard can show it *before* setup runs —
 * dotfix must never quietly clone from somewhere other than what was
 * entered. `host_to_pin` is `null` for https and for hosts dotfix ships no
 * fingerprints for, which is exactly when `init_run` performs no host-key
 * step.
 */
export interface CloneTarget {
  url: string;
  host_to_pin: string | null;
}

/** How a clone will authenticate. Mirrors `dotfix_core::init::Auth`. */
export type AuthMethod = "ssh" | "deploy_key" | "token";

/**
 * Mirrors `settings::Settings`. The three values live in three places with
 * three different reaches — the provider travels to other machines in the
 * repository, the remote and the name do not — which is why the screen says
 * so rather than presenting one flat list.
 */
export interface Settings {
  machine: string;
  repo: string;
  /** `null` until a remote is added; `--set-up-new` leaves none. */
  remote: string | null;
  secret_provider: "keychain" | "1password" | "age";
  vault: string | null;
  /** Packages this machine never reports as unmanaged. */
  ignored: string[];
}

/** Mirrors `engine::Published`. `pushed` is false when the repository has no
 * remote, not when pushing failed — that surfaces as an error. */
export interface Published {
  committed: string[];
  pushed: boolean;
}

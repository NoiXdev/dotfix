import { invoke } from "@tauri-apps/api/core";

import type {
  AuthMethod,
  CloneTarget,
  Commit,
  DeployKeyView,
  FileDiff,
  Overview,
  Preflight,
  SetEntry,
  Published,
  Settings,
  StepOutcome,
  WizardAnswers,
} from "./types";

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

// Deliberately not `applyItems`: a `local_edit` id is refused by
// `apply_items`/`plan_selected` on purpose, so that a bulk apply can never
// silently clobber a hand edit. This is the explicit, single-file opposite
// of that safety property — "discard my edit, take the repository's
// version" — and only the Configs area's "Overwrite from repository"
// action should ever call it.
export const overwriteFiles = (ids: string[]) =>
  call<Overview>("overwrite_files", { ids });

export const adoptItem = (id: string, set: string | null, ignore: boolean) =>
  call<Overview>("adopt_item", { id, set, ignore });

export const listSets = () => call<SetEntry[]>("list_sets");

/** Open a set's `set.toml` in the user's editor. Editing happens there, not
 * in this window: a broken set.toml stops dotfix, and the thing that should
 * catch that is an editor the user already trusts. */
/** Open a repository file in the user's editor. The path is
 * repository-relative and the backend refuses anything that escapes. */
/** Record undeclared software into a set. Mirrors adopting a package. */
export const declareRequirement = (name: string, set: string) =>
  call<Overview>("declare_requirement", { name, set });

export const openInRepo = (relative: string) =>
  call<void>("open_in_repo", { relative });

/** Add or remove a package in a set. Rewrites set.toml from the parsed
 * structure, so comments in that file do not survive. */
export const editSetPackage = (
  set: string,
  pkg: string,
  cask: boolean,
  add: boolean,
) => call<SetEntry[]>("edit_set_package", { set, package: pkg, cask, add });
export const toggleSet = (name: string, on: boolean) =>
  call<SetEntry[]>("toggle_set", { name, on });

export const fileDiff = (target: string) => call<FileDiff>("file_diff", { target });
export const history = (limit: number) => call<Commit[]>("history", { limit });

// --- first-time setup wizard ---

/// The Rust `WizardPlan` shape (see app/src-tauri/src/commands.rs):
/// `secret_provider` is snake_case there, unlike the rest of this codebase's
/// TypeScript. Kept private so the only place that shape exists is this
/// file's own conversion below — no component reaches for it directly.
interface WizardPlanArg {
  machine: string;
  mode: string;
  url: string;
  secret_provider: string;
  vault: string;
}

/// The one place `WizardAnswers` crosses into the snake_case shape Rust
/// expects. Every wizard command below funnels through this, so a
/// component only ever has to think in `WizardAnswers`.
const toPlanArg = (answers: WizardAnswers): WizardPlanArg => ({
  machine: answers.machine,
  mode: answers.mode,
  url: answers.url,
  secret_provider: answers.secretProvider,
  vault: answers.vault,
});

export const initPreflight = (answers: WizardAnswers) =>
  call<Preflight>("init_preflight", { plan: toPlanArg(answers) });

// `completed` is every step name the wizard already has marked done — `[]`
// for a first attempt. `init_run` skips redoing those, which is what makes
// a retry resume instead of restart; see `StepOutcome.failed` for how a
// mid-run failure comes back (never a rejection once stepping starts).
// `auth` is how the clone will authenticate, which is what decides the URL
// it actually uses — see `initCloneTarget`.
export const initRun = (
  answers: WizardAnswers,
  completed: string[],
  auth: AuthMethod,
) => call<StepOutcome>("init_run", { plan: toPlanArg(answers), completed, auth });

/// Whether this machine's ssh setup already reaches `host`: `"ready"`,
/// `"needs_key"`, or `"unreachable: <detail>"`. Only ever probes — never
/// writes anything, never prompts. The host matters: a deploy key is scoped
/// to its own alias, so probing `github.com` would never offer it.
export const initProbeSsh = (host: string) =>
  call<string>("init_probe_ssh", { host });

/// The URL a clone will really use under `auth`, and the host key it needs
/// first. Derived in Rust by the same function `init_run` clones with, so
/// what the wizard shows and what it does cannot drift apart.
export const initCloneTarget = (url: string, auth: AuthMethod) =>
  call<CloneTarget>("init_clone_target", { url, auth });

/// Generate (or reuse) a deploy key scoped to this machine. The private
/// half never crosses this call — see `DeployKeyView`.
export const initDeployKey = (machine: string) =>
  call<DeployKeyView>("init_deploy_key", { machine });

/// Hand a token straight to the backend, which hands it straight to the
/// macOS Keychain via git's credential helper. Nothing on this side of the
/// call keeps a reference to `token` once this promise settles — see
/// `DeployKey.tsx`'s own handling of the field it comes from.
export const initStoreToken = (user: string, token: string) =>
  call<void>("init_store_token", { user, token });

export const readSettings = () => call<Settings>("read_settings");

export const setRemote = (url: string) =>
  call<Settings>("set_remote", { url });

/** Verified before it is recorded: a provider that cannot resolve the
 * repository's secrets is rejected here rather than failing at the next
 * apply. */
export const setProvider = (provider: string, vault: string | null) =>
  call<Settings>("set_provider", { provider, vault });

/** Stop ignoring a package. It returns to the unmanaged list. */
export const unignore = (name: string) => call<Settings>("unignore", { name });

export const renameMachine = (newName: string) =>
  call<Settings>("rename_machine", { newName });

/** Commit what changed and send it. The counterpart to `refresh`, which only
 * ever pulled. */
export const publish = () => call<Published>("publish");

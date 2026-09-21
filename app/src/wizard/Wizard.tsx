import { useState } from "react";

import {
  initCloneTarget,
  initDeployKey,
  initPreflight,
  initProbeSsh,
  initRun,
  initStoreToken,
} from "../api";
import type {
  AuthMethod,
  CloneTarget,
  DeployKeyView,
  Preflight,
  WizardAnswers,
} from "../types";
import Answers from "./Answers";
import DeployKey from "./DeployKey";
import PreflightList from "./PreflightList";

/**
 * What `init_probe_ssh` found, narrowed from its raw `"ready"` /
 * `"needs_key"` / `"unreachable: <detail>"` string into a state this
 * component can branch on. `"idle"` is "never probed yet" — distinct from
 * any real result — so the "Check connection" button only shows before the
 * first attempt; `onTest`/Retry cover every re-check after that.
 */
type AuthStatus = "idle" | "ready" | "needs_key" | "unreachable";

type StepStatus = "pending" | "running" | "done" | "failed";

interface StepDef {
  name: string;
  label: string;
}

/**
 * The steps `init_run` will attempt, in the order it attempts them.
 * `ensure_host_known` only exists for a clone whose host dotfix will
 * actually pin — never for a fresh repository, and never for a host it
 * ships no fingerprints for or a URL that carries no ssh host key at all.
 * `target.host_to_pin` is the backend's own answer to that question, so a
 * GitLab URL no longer shows a line that could only stay pending forever.
 */
function stepsFor(
  mode: WizardAnswers["mode"],
  target: CloneTarget | null,
): StepDef[] {
  const steps: StepDef[] =
    mode === "clone" && target?.host_to_pin
      ? [
          {
            name: "ensure_host_known",
            label: `Verify the ${target.host_to_pin} host key`,
          },
        ]
      : [];
  steps.push(
    {
      name: "create_or_clone",
      label:
        mode === "clone"
          ? "Clone the dotfiles repository"
          : "Create the dotfiles repository",
    },
    { name: "configure_machine", label: "Register this machine" },
    { name: "install_agent", label: "Install the background sync agent" },
  );
  return steps;
}

const emptyAnswers: WizardAnswers = {
  machine: "",
  mode: "new",
  url: "",
  secretProvider: "keychain",
  vault: "",
};

/**
 * The first-run wizard: the answers form, a pre-flight check, and the four
 * (or three, for a fresh repository) steps `init_run` performs — shown as
 * a shell, not a report, is the whole reason this component exists.
 *
 * The person looking at this has no terminal open, so a blanket "setup
 * failed" would leave them stuck. `init_run` resolves normally even when a
 * step fails partway through — `StepOutcome.failed` names exactly which
 * step stopped it and carries that step's own message — so this component
 * never has to guess: every name in `completed` is marked done, and
 * `failed.step` (only) is marked failed with `failed.message`. Retry passes
 * `completed` straight back, which is what lets `init_run` resume instead
 * of redoing steps that already succeeded — retrying a failed
 * `install_agent` never touches `create_or_clone` again.
 *
 * A rejected `init_run` call still exists, but only for a plan Rust
 * rejected before it started stepping at all (an unusable plan, or an
 * unreadable `$HOME`) — neither is one of the four numbered steps, so
 * there is nothing to pin it to, and it surfaces as a plain banner instead
 * of a step line.
 */
export default function Wizard({ onDone }: { onDone: () => void }) {
  const [imported, setImported] = useState(false);
  const [answers, setAnswers] = useState<WizardAnswers>(emptyAnswers);
  const [preflight, setPreflight] = useState<Preflight | null>(null);
  const [checking, setChecking] = useState(false);
  const [checkError, setCheckError] = useState<string | null>(null);
  const [running, setRunning] = useState(false);
  const [runError, setRunError] = useState<string | null>(null);
  const [stepStatus, setStepStatus] = useState<Record<string, StepStatus>>({});
  const [stepError, setStepError] = useState<string | null>(null);

  // The clone path's authentication branch: `init_probe_ssh` first, then
  // either a deploy key or a token, mirroring the preflight/run split
  // above — one flag per async action, one error per thing that can fail.
  const [authStatus, setAuthStatus] = useState<AuthStatus>("idle");
  const [authDetail, setAuthDetail] = useState<string | null>(null);
  const [probing, setProbing] = useState(false);
  const [deployKey, setDeployKey] = useState<DeployKeyView | null>(null);
  const [deployKeyError, setDeployKeyError] = useState<string | null>(null);
  const [tokenSubmitting, setTokenSubmitting] = useState(false);
  const [tokenError, setTokenError] = useState<string | null>(null);

  // How the clone will authenticate, and therefore which URL it will really
  // use. `"ssh"` until something says otherwise: a deploy key and a token
  // each imply a different URL, and the user is shown the result before
  // "Set up" does anything.
  const [authMethod, setAuthMethod] = useState<AuthMethod>("ssh");
  const [cloneTarget, setCloneTarget] = useState<CloneTarget | null>(null);
  const [cloneTargetError, setCloneTargetError] = useState<string | null>(null);

  const steps = stepsFor(answers.mode, cloneTarget);
  // Not "every check passed": the backend marks a check it merely reports
  // as non-blocking (`gh`, which only decides whether dotfix can offer to
  // create the remote). Re-deriving that rule here is what made setup
  // impossible on a stock Mac without `gh`, so this reads the flag the
  // backend serialised instead.
  const preflightOk =
    preflight !== null &&
    preflight.checks.every((check) => check.ok || !check.blocking);
  // "new" never needs GitHub auth at all — only a clone reaches a private
  // repository that could refuse the connection.
  const authOk = answers.mode !== "clone" || authStatus === "ready";
  // A clone cannot start until the URL it will use is known: `init_run`
  // resolves the same thing and would reject an unparseable URL outright.
  const targetOk = answers.mode !== "clone" || cloneTarget !== null;
  // Everything but the background agent worked: the repository is cloned or
  // created and this machine is registered, so dotfix is usable — only the
  // hourly check is missing (most often because the CLI it runs is not
  // installed). Retrying is offered on the step itself; this is the way out
  // that does not leave the wizard a dead end.
  const onlyAgentFailed =
    stepStatus.install_agent === "failed" &&
    steps
      .filter((step) => step.name !== "install_agent")
      .every((step) => stepStatus[step.name] === "done");

  // A changed answer can invalidate everything downstream of it — a
  // different machine name changes what `init_preflight` would even check,
  // and a mode switch changes which steps apply at all — so re-checking
  // and re-running are both required again rather than trusting stale
  // results.
  function handleAnswersChange(next: WizardAnswers) {
    setAnswers(next);
    setPreflight(null);
    setCheckError(null);
    setStepStatus({});
    setStepError(null);
    setRunError(null);
    setAuthStatus("idle");
    setAuthDetail(null);
    setDeployKey(null);
    setDeployKeyError(null);
    setTokenError(null);
    setAuthMethod("ssh");
    setCloneTarget(null);
    setCloneTargetError(null);
  }

  // Ask the backend which URL `init_run` would really clone under `method`,
  // so the wizard shows exactly what it is about to do rather than a second,
  // hand-rolled guess at the same rule.
  async function loadCloneTarget(method: AuthMethod) {
    if (answers.mode !== "clone") return;
    setCloneTargetError(null);
    try {
      setCloneTarget(await initCloneTarget(answers.url, method));
    } catch (err) {
      setCloneTarget(null);
      setCloneTargetError((err as Error).message);
    }
  }

  async function handleFetchDeployKey() {
    setDeployKeyError(null);
    try {
      setDeployKey(await initDeployKey(answers.machine));
    } catch (err) {
      setDeployKeyError((err as Error).message);
    }
  }

  // Probes ssh reachability against GitHub. Called once from "Check
  // connection" and again, unchanged, from the deploy-key screen's "Test
  // connection" — a mismatch there is exactly "try the same probe again
  // now that the key has been added", not a different operation.
  async function handleCheckAuth() {
    setProbing(true);
    setAuthDetail(null);
    // Once a deploy key exists it is scoped to its own alias with
    // `IdentitiesOnly yes`, so probing `github.com` would never offer it and
    // "Test connection" could never succeed. Probe what the clone will use.
    const method: AuthMethod = deployKey ? "deploy_key" : "ssh";
    const host = deployKey ? deployKey.host_alias : "github.com";
    try {
      const result = await initProbeSsh(host);
      if (result === "ready") {
        setAuthStatus("ready");
        setAuthMethod(method);
        await loadCloneTarget(method);
      } else if (result === "needs_key") {
        setAuthStatus("needs_key");
        if (!deployKey) await handleFetchDeployKey();
      } else {
        setAuthStatus("unreachable");
        setAuthDetail(result.replace(/^unreachable:\s*/, ""));
      }
    } catch (err) {
      setAuthStatus("unreachable");
      setAuthDetail((err as Error).message);
    } finally {
      setProbing(false);
    }
  }

  // The token arrives here as a plain function argument, used once and
  // handed straight to `initStoreToken` — it is never assigned to a
  // variable this component holds onto beyond the call itself, matching
  // how `DeployKey` already clears its own field the moment it calls this.
  async function handleSubmitToken(user: string, token: string) {
    setTokenSubmitting(true);
    setTokenError(null);
    try {
      await initStoreToken(user, token);
      // A token authenticates over https via the credential helper, not
      // ssh — re-probing ssh here would just report `needs_key` again. The
      // store succeeding is itself the signal that this machine is ready.
      setAuthStatus("ready");
      setAuthMethod("token");
      // And it changes the URL: git only ever offers a stored token over
      // https, so cloning the typed ssh URL would fail with
      // `Permission denied (publickey)` — pointing at the very mechanism
      // the user opted out of.
      await loadCloneTarget("token");
    } catch (err) {
      setTokenError((err as Error).message);
    } finally {
      setTokenSubmitting(false);
    }
  }

  async function handleCheck() {
    setChecking(true);
    setCheckError(null);
    try {
      setPreflight(await initPreflight(answers));
      // Resolve the clone URL for the authentication assumed so far, so the
      // step list and the confirmation below are right from the start.
      await loadCloneTarget(authMethod);
    } catch (err) {
      setCheckError((err as Error).message);
    } finally {
      setChecking(false);
    }
  }

  async function handleRunSetup() {
    const currentSteps = stepsFor(answers.mode, cloneTarget);
    // Every step this component already has marked done — `[]` on a first
    // attempt — is exactly what `init_run` needs to skip redoing them.
    const already = currentSteps
      .filter((step) => stepStatus[step.name] === "done")
      .map((step) => step.name);
    const next = currentSteps.find((step) => !already.includes(step.name));

    setRunning(true);
    setStepError(null);
    setRunError(null);
    if (next) {
      setStepStatus((prev) => ({ ...prev, [next.name]: "running" }));
    }

    try {
      const outcome = await initRun(answers, already, authMethod);
      const nextStatus: Record<string, StepStatus> = {};
      for (const step of currentSteps) {
        nextStatus[step.name] = outcome.completed.includes(step.name)
          ? "done"
          : "pending";
      }
      if (outcome.failed) {
        nextStatus[outcome.failed.step] = "failed";
        setStepStatus(nextStatus);
        setStepError(outcome.failed.message);
        setRunning(false);
        return;
      }
      setStepStatus(nextStatus);
      setRunning(false);
      // Finish silently unless something was taken over that the user should
      // know about. An extra click on every setup would be noise; saying
      // nothing about a shell configuration dotfix now owns is how a real
      // one came to be replaced by a stub.
      if (outcome.imported_zshrc) {
        setImported(true);
        return;
      }
      onDone();
    } catch (err) {
      setRunError((err as Error).message);
      setRunning(false);
    }
  }

  if (imported) {
    return (
      <div className="mx-auto flex w-full max-w-sm flex-col gap-4 py-2">
        <div className="text-center">
          <h2 className="text-base font-medium text-ink">Set up</h2>
        </div>
        <p className="text-sm text-ink">
          Your existing <code className="text-xs">~/.zshrc</code> was taken
          into the repository as{" "}
          <code className="text-xs">sets/core/shell/00-imported.zsh</code>.
        </p>
        <p className="text-sm text-ink-muted">
          It is preserved, not replaced — dotfix now generates your{" "}
          <code className="text-xs">~/.zshrc</code> from that file plus its own
          status line. Split it into smaller fragments when you want different
          machines to take different parts.
        </p>
        <button
          type="button"
          onClick={onDone}
          className="self-start rounded-md border border-control px-3 py-1.5 text-sm font-medium text-ink"
        >
          Continue
        </button>
      </div>
    );
  }

  return (
    <div className="mx-auto flex w-full max-w-sm flex-col gap-5 py-2">
      <div className="text-center">
        <h2 className="text-base font-medium text-ink">Set up dotfix</h2>
        <p className="mt-1 text-sm text-ink-muted">
          Tell dotfix about this Mac, then let it get to work.
        </p>
      </div>

      <fieldset disabled={running} className="contents">
        <Answers value={answers} onChange={handleAnswersChange} />
      </fieldset>

      <div className="flex flex-col gap-3">
        <button
          type="button"
          onClick={() => void handleCheck()}
          disabled={checking || running}
          className="self-start rounded-md border border-control px-3 py-1.5 text-sm font-medium text-ink disabled:opacity-50"
        >
          {checking ? "Checking…" : "Check requirements"}
        </button>

        {checkError ? (
          <p
            role="alert"
            className="rounded border border-destructive/30 bg-destructive-soft px-3 py-2 text-sm text-destructive"
          >
            {checkError}
          </p>
        ) : null}

        {preflight ? <PreflightList checks={preflight.checks} /> : null}
      </div>

      {answers.mode === "clone" && preflightOk && authStatus !== "ready" ? (
        <div className="flex flex-col gap-3 border-t border-hairline pt-4">
          <div>
            <h3 className="text-sm font-medium text-ink">
              Authenticate with GitHub
            </h3>
            <p className="mt-1 text-xs text-ink-muted">
              dotfix needs to read and write this repository over ssh before
              it can clone it.
            </p>
          </div>

          {authStatus === "idle" ? (
            <button
              type="button"
              onClick={() => void handleCheckAuth()}
              disabled={probing}
              className="self-start rounded-md border border-control px-3 py-1.5 text-sm font-medium text-ink disabled:opacity-50"
            >
              {probing ? "Checking…" : "Check connection"}
            </button>
          ) : null}

          {authStatus === "unreachable" ? (
            <div className="flex flex-col gap-2">
              <p
                role="alert"
                className="rounded border border-destructive/30 bg-destructive-soft px-3 py-2 text-sm text-destructive"
              >
                {authDetail}
              </p>
              <button
                type="button"
                onClick={() => void handleCheckAuth()}
                disabled={probing}
                className="self-start rounded-md border border-control px-3 py-1.5 text-sm font-medium text-ink disabled:opacity-50"
              >
                {probing ? "Checking…" : "Retry"}
              </button>
            </div>
          ) : null}

          {authStatus === "needs_key" ? (
            deployKey ? (
              <DeployKey
                deployKey={deployKey}
                onTest={() => void handleCheckAuth()}
                testing={probing}
                repoUrl={answers.url}
                onSubmitToken={(user, token) => void handleSubmitToken(user, token)}
                tokenSubmitting={tokenSubmitting}
                tokenError={tokenError}
              />
            ) : deployKeyError ? (
              <div className="flex flex-col gap-2">
                <p
                  role="alert"
                  className="rounded border border-destructive/30 bg-destructive-soft px-3 py-2 text-sm text-destructive"
                >
                  {deployKeyError}
                </p>
                <button
                  type="button"
                  onClick={() => void handleFetchDeployKey()}
                  className="self-start rounded-md border border-control px-3 py-1.5 text-sm font-medium text-ink"
                >
                  Retry
                </button>
              </div>
            ) : (
              <p className="text-sm text-ink-muted">Generating a deploy key…</p>
            )
          ) : null}
        </div>
      ) : null}

      {preflight ? (
        <ol className="flex flex-col">
          {steps.map((step) => {
            const status = stepStatus[step.name] ?? "pending";
            return (
              <li
                key={step.name}
                data-status={status}
                className="flex flex-col gap-1 border-b border-hairline py-2 text-sm"
              >
                <div className="flex items-center justify-between gap-3">
                  <span className="flex min-w-0 items-center gap-2">
                    <span
                      aria-hidden="true"
                      className={
                        status === "failed"
                          ? "text-destructive"
                          : "text-ink-muted"
                      }
                    >
                      {status === "done"
                        ? "✓"
                        : status === "failed"
                          ? "✗"
                          : status === "running"
                            ? "…"
                            : "○"}
                    </span>
                    <span
                      className={
                        status === "pending" ? "text-ink-muted" : "text-ink"
                      }
                    >
                      {step.label}
                    </span>
                    <span className="sr-only">{status}</span>
                  </span>
                  {status === "failed" ? (
                    <button
                      type="button"
                      onClick={() => void handleRunSetup()}
                      className="shrink-0 rounded-md border border-control px-2 py-1 text-xs font-medium text-ink"
                    >
                      Retry
                    </button>
                  ) : status === "running" ? (
                    <span className="text-xs text-ink-muted">Running…</span>
                  ) : status === "done" ? (
                    <span className="text-xs text-ink-muted">Done</span>
                  ) : null}
                </div>
                {/*
                  Its own full-width line, not a cell beside the Retry button.
                  These messages are sentences — "the `dotfix` command-line
                  tool is not installed" — and the row could only ever spare
                  about twenty characters, so a message sharing it was clipped
                  to nothing with no way to read the rest. A user whose setup
                  just failed has no terminal open; the text IS the recovery
                  instruction.
                */}
                {status === "failed" && stepError ? (
                  <p className="pl-6 text-xs break-words text-destructive">
                    {stepError}
                  </p>
                ) : null}
              </li>
            );
          })}
        </ol>
      ) : null}

      {onlyAgentFailed ? (
        <button
          type="button"
          onClick={onDone}
          className="self-start rounded-md border border-control px-3 py-1.5 text-sm font-medium text-ink"
        >
          Continue without the background check
        </button>
      ) : null}

      {cloneTargetError ? (
        <p
          role="alert"
          className="rounded border border-destructive/30 bg-destructive-soft px-3 py-2 text-sm text-destructive"
        >
          {cloneTargetError}
        </p>
      ) : null}

      {preflight &&
      cloneTarget &&
      cloneTarget.url.trim() !== answers.url.trim() ? (
        <p className="text-xs text-ink-muted">
          dotfix will clone{" "}
          <span className="font-mono text-ink">{cloneTarget.url}</span> — the
          same repository, reached the way this Mac authenticates.
        </p>
      ) : null}

      {runError ? (
        <p
          role="alert"
          className="rounded border border-destructive/30 bg-destructive-soft px-3 py-2 text-sm text-destructive"
        >
          {runError}
        </p>
      ) : null}

      <button
        type="button"
        onClick={() => void handleRunSetup()}
        disabled={!preflightOk || !authOk || !targetOk || running}
        className="self-start rounded-md bg-ink px-3 py-1.5 text-sm font-medium text-surface disabled:opacity-50"
      >
        Set up
      </button>
    </div>
  );
}

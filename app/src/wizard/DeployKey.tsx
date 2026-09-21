import { useState } from "react";

import type { DeployKeyView } from "../types";

const fieldClass =
  "rounded-md border border-control bg-surface px-2 py-1 text-sm text-ink";
const labelClass = "flex flex-col gap-1 text-sm";
const buttonClass =
  "self-start rounded-md border border-control px-3 py-1.5 text-sm font-medium text-ink disabled:opacity-50";

/**
 * Extract "owner/repo" from a GitHub clone URL, understanding both scp-like
 * syntax (`git@github.com:owner/repo.git`, including the ssh alias
 * `ensure_ssh_config` writes, e.g. `git@github.com-dotfix:owner/repo.git`)
 * and https syntax (`https://github.com/owner/repo.git`). Returns `null`
 * for anything else rather than guessing — the deploy-key settings page
 * this builds a link to only exists for a real GitHub repository, so a
 * link built from a misparsed host would send the user somewhere useless
 * or wrong.
 */
function githubOwnerRepo(url: string): { owner: string; repo: string } | null {
  const scp = /^(?:[^@\s]+@)?github\.com(?:-[\w.-]+)?:(.+)$/.exec(url.trim());
  const https = /^https:\/\/github\.com\/(.+)$/.exec(url.trim());
  const path = scp?.[1] ?? https?.[1];
  if (!path) return null;

  const [owner, repoWithSuffix] = path.split("/");
  if (!owner || !repoWithSuffix) return null;

  const repo = repoWithSuffix.replace(/\.git$/, "");
  return repo ? { owner, repo } : null;
}

/**
 * The clone path's authentication screen for a machine whose ssh setup
 * doesn't already reach GitHub. dotfix generated this deploy key locally —
 * only its public half is ever passed in here, so there is nothing for
 * this component to leak even if it wanted to.
 *
 * A token is the fallback, collapsed behind a toggle rather than shown by
 * default, because the deploy key is the safer default (scoped to one
 * repository, revocable independently of the user's own account) and the
 * brief is explicit that the token exists for organisations that forbid
 * deploy keys outright. The field is `type="password"`, its value lives in
 * this component's state only for as long as it takes to type it, and
 * `handleSubmitToken` clears that state in the same tick it hands the
 * value to `onSubmitToken` — nothing here holds onto it a moment longer
 * than the call, and nothing echoes it back afterward.
 */
export default function DeployKey({
  deployKey,
  onTest,
  testing = false,
  repoUrl,
  onSubmitToken,
  tokenSubmitting = false,
  tokenError = null,
}: {
  deployKey: DeployKeyView;
  onTest: () => void;
  testing?: boolean;
  repoUrl?: string;
  onSubmitToken?: (user: string, token: string) => void;
  tokenSubmitting?: boolean;
  tokenError?: string | null;
}) {
  const [copied, setCopied] = useState(false);
  const [showToken, setShowToken] = useState(false);
  const [tokenUser, setTokenUser] = useState("");
  const [token, setToken] = useState("");

  const ownerRepo = repoUrl ? githubOwnerRepo(repoUrl) : null;
  const addKeyHref = ownerRepo
    ? `https://github.com/${ownerRepo.owner}/${ownerRepo.repo}/settings/keys/new`
    : null;

  async function handleCopy() {
    try {
      await navigator.clipboard.writeText(deployKey.public);
      setCopied(true);
    } catch {
      // The key is still selectable text in the <pre> below, so copying by
      // hand stays possible even if clipboard access is denied.
    }
  }

  function handleSubmitToken() {
    const user = tokenUser;
    const value = token;
    // Cleared in this same tick, before the caller's promise (if any) has
    // even started — the value must not outlive this call by so much as
    // one render.
    setToken("");
    onSubmitToken?.(user, value);
  }

  return (
    <div className="flex flex-col gap-4">
      <p className="text-sm text-ink">
        dotfix generated a deploy key for this Mac. Add its public half to
        the repository on GitHub — the private half never leaves this
        machine.
      </p>

      <div className="flex flex-col gap-2">
        <pre className="overflow-x-auto rounded-md border border-control bg-surface p-2 font-mono text-xs text-ink">
          {deployKey.public}
        </pre>
        <div className="flex items-center gap-3">
          <button type="button" onClick={() => void handleCopy()} className={buttonClass}>
            {copied ? "Copied" : "Copy"}
          </button>
          {addKeyHref ? (
            <a
              href={addKeyHref}
              target="_blank"
              rel="noreferrer"
              className="text-sm font-medium text-ink underline"
            >
              Add deploy key on GitHub
            </a>
          ) : null}
        </div>
        <p className="text-xs text-ink-muted">
          On that page, allow write access — dotfix needs to push as well as
          pull.
        </p>
      </div>

      <button
        type="button"
        onClick={onTest}
        disabled={testing}
        className={buttonClass}
      >
        {testing ? "Testing…" : "Test connection"}
      </button>

      <div className="flex flex-col gap-2 border-t border-hairline pt-3">
        {showToken ? null : (
          <button
            type="button"
            onClick={() => setShowToken(true)}
            className="self-start text-xs font-medium text-ink-muted underline"
          >
            Use a personal access token instead
          </button>
        )}

        {showToken ? (
          <div className="flex flex-col gap-2">
            <label className={labelClass}>
              <span className="font-medium text-ink">GitHub username</span>
              <input
                type="text"
                value={tokenUser}
                onChange={(e) => setTokenUser(e.target.value)}
                className={fieldClass}
              />
            </label>
            <label className={labelClass}>
              <span className="font-medium text-ink">Personal access token</span>
              <input
                type="password"
                autoComplete="off"
                value={token}
                onChange={(e) => setToken(e.target.value)}
                className={fieldClass}
              />
            </label>
            {tokenError ? (
              <p
                role="alert"
                className="rounded border border-destructive/30 bg-destructive-soft px-3 py-2 text-sm text-destructive"
              >
                {tokenError}
              </p>
            ) : null}
            <button
              type="button"
              onClick={handleSubmitToken}
              disabled={tokenSubmitting || !tokenUser || !token}
              className={buttonClass}
            >
              {tokenSubmitting ? "Saving…" : "Save token"}
            </button>
          </div>
        ) : null}
      </div>
    </div>
  );
}

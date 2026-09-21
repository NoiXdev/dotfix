import { useState } from "react";

import Select from "../components/Select";
import type { Settings as SettingsValue } from "../types";

const PROVIDERS = [
  { value: "keychain", label: "macOS Keychain" },
  { value: "1password", label: "1Password" },
  { value: "age", label: "age (a key file)" },
];

/**
 * What `init` decided once, changeable afterwards.
 *
 * Grouped by where each value lives, not by what it does: the secret provider
 * is written into the repository and reaches every machine that syncs, while
 * the remote and this machine's name are local. A flat list would hide that,
 * and the difference is the whole reason one of these needs committing.
 */
export default function Settings({
  value,
  onSetRemote,
  onSetProvider,
  onRename,
  onUnignore,
}: {
  value: SettingsValue;
  onSetRemote: (url: string) => void;
  onSetProvider: (provider: string, vault: string | null) => void;
  onRename: (name: string) => void;
  onUnignore: (name: string) => void;
}) {
  const [remote, setRemote] = useState(value.remote ?? "");
  const [provider, setProvider] = useState<string>(value.secret_provider);
  const [vault, setVault] = useState(value.vault ?? "");
  const [name, setName] = useState(value.machine);

  const field =
    "w-full rounded-md border border-control bg-surface px-2 py-1 text-sm text-ink";
  const action =
    "self-start rounded-md border border-control px-2 py-1 text-xs font-medium text-ink disabled:opacity-50";

  return (
    <div className="flex flex-col gap-5">
      <section className="flex flex-col gap-2">
        <h3 className="text-xs font-medium text-ink">This machine only</h3>

        <label className="flex flex-col gap-1 text-sm">
          <span className="text-ink-muted">Remote</span>
          <input
            type="text"
            value={remote}
            onChange={(e) => setRemote(e.target.value)}
            placeholder="git@github.com:you/dotfiles.git"
            className={field}
          />
        </label>
        <button
          type="button"
          className={action}
          disabled={remote.trim() === (value.remote ?? "")}
          onClick={() => onSetRemote(remote.trim())}
        >
          Set remote
        </button>

        <label className="mt-2 flex flex-col gap-1 text-sm">
          <span className="text-ink-muted">Machine name</span>
          <input
            type="text"
            value={name}
            onChange={(e) => setName(e.target.value)}
            className={field}
          />
        </label>
        <p className="text-xs text-ink-muted">
          Renaming moves the machine file, this Mac's pointer to it, and a
          deploy key if one was generated. A key registered on GitHub keeps the
          old name as its title.
        </p>
        <button
          type="button"
          className={action}
          disabled={name.trim() === value.machine || name.trim() === ""}
          onClick={() => onRename(name.trim())}
        >
          Rename
        </button>
        <p className="mt-2 text-ink-muted">Ignored packages</p>
        {value.ignored.length === 0 ? (
          <p className="text-xs text-ink-muted">
            None. Ignoring a package on this machine hides it from the
            unmanaged list.
          </p>
        ) : (
          <ul className="flex flex-wrap gap-1">
            {value.ignored.map((name) => (
              <li key={name}>
                <span className="inline-flex items-center gap-1 rounded-md border border-control px-1.5 py-0.5 text-xs text-ink">
                  {name}
                  <button
                    type="button"
                    aria-label={`Stop ignoring ${name}`}
                    onClick={() => onUnignore(name)}
                    className="text-ink-muted"
                  >
                    ×
                  </button>
                </span>
              </li>
            ))}
          </ul>
        )}
      </section>

      <section className="flex flex-col gap-2 border-t border-hairline pt-4">
        <h3 className="text-xs font-medium text-ink">
          Shared with your other machines
        </h3>

        <Select
          label="Store secrets in"
          value={provider}
          onChange={setProvider}
          options={PROVIDERS}
        />
        {provider === "1password" ? (
          <label className="flex flex-col gap-1 text-sm">
            <span className="text-ink-muted">1Password vault</span>
            <input
              type="text"
              value={vault}
              onChange={(e) => setVault(e.target.value)}
              className={field}
            />
          </label>
        ) : null}
        <p className="text-xs text-ink-muted">
          Checked before it is saved: every secret your files reference has to
          be findable at the new provider, or nothing is changed. Commit the
          repository afterwards so your other machines see it.
        </p>
        <button
          type="button"
          className={action}
          disabled={
            provider === value.secret_provider &&
            (vault || null) === value.vault
          }
          onClick={() =>
            onSetProvider(provider, provider === "1password" ? vault : null)
          }
        >
          Save provider
        </button>
      </section>
    </div>
  );
}

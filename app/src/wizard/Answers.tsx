import Select from "../components/Select";
import type { WizardAnswers } from "../types";

const fieldClass =
  "rounded-md border border-control bg-surface px-2 py-1 text-sm text-ink";
const labelClass = "flex flex-col gap-1 text-sm";

/**
 * The plan's four fields, two of them conditional: a repository URL only
 * makes sense when cloning, and a vault name only makes sense when secrets
 * live in 1Password. Every keystroke is reported straight to the parent —
 * this form holds no state of its own — so `Wizard` decides when a plan is
 * complete enough to preflight, rather than this component guessing.
 */
export default function Answers({
  value,
  onChange,
}: {
  value: WizardAnswers;
  onChange: (next: WizardAnswers) => void;
}) {
  return (
    <div className="flex flex-col gap-3">
      <label className={labelClass}>
        <span className="font-medium text-ink">Machine name</span>
        <input
          type="text"
          value={value.machine}
          onChange={(e) => onChange({ ...value, machine: e.target.value })}
          placeholder="e.g. imac-studio"
          className={fieldClass}
        />
      </label>

      <Select
        label="Set up this Mac by"
        value={value.mode}
        onChange={(mode) =>
          onChange({ ...value, mode: mode as WizardAnswers["mode"] })
        }
        options={[
          { value: "new", label: "Creating a new dotfiles repository" },
          { value: "clone", label: "Cloning an existing dotfiles repository" },
        ]}
      />

      {value.mode === "clone" ? (
        <label className={labelClass}>
          <span className="font-medium text-ink">Repository URL</span>
          <input
            type="text"
            value={value.url}
            onChange={(e) => onChange({ ...value, url: e.target.value })}
            placeholder="git@github.com:you/dotfiles.git"
            className={fieldClass}
          />
        </label>
      ) : null}

      <Select
        label="Store secrets in"
        value={value.secretProvider}
        onChange={(secretProvider) =>
          onChange({
            ...value,
            secretProvider: secretProvider as WizardAnswers["secretProvider"],
          })
        }
        options={[
          { value: "keychain", label: "macOS Keychain" },
          { value: "1password", label: "1Password" },
          { value: "age", label: "age (a key file)" },
        ]}
      />

      {value.secretProvider === "1password" ? (
        <label className={labelClass}>
          <span className="font-medium text-ink">1Password vault name</span>
          <input
            type="text"
            value={value.vault}
            onChange={(e) => onChange({ ...value, vault: e.target.value })}
            placeholder="Private"
            className={fieldClass}
          />
        </label>
      ) : null}
    </div>
  );
}

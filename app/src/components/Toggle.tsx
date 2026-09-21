/**
 * An on/off switch.
 *
 * A checkbox says "this is one of several things I am selecting"; a switch
 * says "this takes effect now". Activating a set is the second kind — it
 * changes what dotfix wants the moment it is clicked — so it gets the
 * control that means that.
 *
 * `role="switch"` rather than a styled checkbox: a screen reader announces
 * "on"/"off" instead of "checked", and the keyboard behaviour (Space, Enter)
 * comes from the button element for free.
 */
export default function Toggle({
  checked,
  onChange,
  label,
  disabled = false,
}: {
  checked: boolean;
  onChange: (next: boolean) => void;
  /** Accessible name — the switch carries no visible text of its own. */
  label: string;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={`relative inline-flex h-4.5 w-8 shrink-0 items-center rounded-full border transition-colors disabled:opacity-50 ${
        checked ? "border-ink bg-ink" : "border-control bg-canvas"
      }`}
    >
      <span
        aria-hidden="true"
        className={`absolute size-3.5 rounded-full bg-surface shadow-sm transition-transform ${
          checked ? "translate-x-4" : "translate-x-0.5"
        }`}
      />
    </button>
  );
}

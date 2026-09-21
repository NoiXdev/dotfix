/**
 * One drift item, set, or history entry. A hairline underneath is the only
 * structure — no card, no shadow — so a full list of these reads as one
 * quiet column, the way `git status` output does.
 */
export default function Row({
  label,
  detail,
  disabled = false,
  children,
}: {
  label: string;
  detail?: string;
  disabled?: boolean;
  children?: React.ReactNode;
}) {
  return (
    <li
      className={`flex items-center justify-between gap-3 border-b border-hairline py-2 ${
        disabled ? "opacity-50" : ""
      }`}
    >
      <span className="min-w-0">
        <span className="block truncate text-sm font-medium text-ink">{label}</span>
        {detail ? (
          <span className="block truncate text-xs text-ink-muted">{detail}</span>
        ) : null}
      </span>
      {children}
    </li>
  );
}

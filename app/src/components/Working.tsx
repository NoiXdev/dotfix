/**
 * Covers the window while something is running.
 *
 * Applying a change or switching a set reaches out to Homebrew and git, which
 * takes seconds — and a window that neither moves nor refuses input during
 * those seconds reads as frozen. The backdrop is what says "this is busy, not
 * broken"; blocking the clicks underneath is the other half, since a second
 * toggle mid-flight would race the first.
 */
export default function Working({ label }: { label: string }) {
  return (
    <div
      role="status"
      aria-live="polite"
      className="absolute inset-0 z-20 flex items-center justify-center bg-canvas/75 backdrop-blur-[1px]"
    >
      <span className="flex items-center gap-2 rounded-md border border-control bg-surface px-3 py-2 shadow-sm">
        <span
          aria-hidden="true"
          className="size-3.5 animate-spin rounded-full border-2 border-hairline border-t-ink"
        />
        <span className="text-sm text-ink">{label}</span>
      </span>
    </div>
  );
}

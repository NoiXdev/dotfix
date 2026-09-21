/** Shown for the brief moment between opening the window and the first
 * overview arriving. No spinner — the window is glanced at, not lived in. */
export default function Busy() {
  return (
    <div className="flex h-full flex-col items-center justify-center">
      <p className="text-sm text-ink-muted">Checking…</p>
    </div>
  );
}

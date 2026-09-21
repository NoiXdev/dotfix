/**
 * The calm "nothing to do" state. Most launches land here, so it must read
 * as intentional — a quiet mark plus a short sentence — never as a broken
 * or unfinished screen.
 */
export default function Empty({ children }: { children: React.ReactNode }) {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-3 text-center">
      <svg
        width="26"
        height="26"
        viewBox="0 0 26 26"
        fill="none"
        aria-hidden="true"
        className="text-ink-muted"
      >
        <circle cx="13" cy="13" r="8.5" stroke="currentColor" strokeWidth="1.4" />
      </svg>
      <p className="text-sm text-ink-muted">{children}</p>
    </div>
  );
}

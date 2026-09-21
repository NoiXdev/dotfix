import type { Check } from "../types";

/**
 * The result of `init_preflight`, one line per requirement dotfix looked
 * for on this Mac (git, Homebrew, `gh` being logged in, ...). `data-ok`
 * carries the result for tests and any future styling, and a
 * screen-reader-only "passed"/"failed" word repeats it in text — so a
 * failing check is never colour-only information, which matters here more
 * than most places: the person reading this has no terminal to fall back
 * on if the line itself doesn't explain what's wrong. A check the backend
 * marked non-blocking reads "optional" rather than "failed", because it
 * does not stop setup — `data-blocking` carries that distinction too.
 */
export default function PreflightList({ checks }: { checks: Check[] }) {
  return (
    <ul className="flex flex-col">
      {checks.map((check) => (
        <li
          key={check.name}
          data-ok={check.ok}
          data-blocking={check.blocking}
          className="flex items-baseline justify-between gap-3 border-b border-hairline py-1.5 text-sm"
        >
          <span className="flex min-w-0 items-baseline gap-2">
            <span
              aria-hidden="true"
              className={check.ok ? "text-ink-muted" : "text-destructive"}
            >
              {check.ok ? "✓" : "✗"}
            </span>
            <span className="font-medium text-ink">{check.name}</span>
            <span className="sr-only">
              {check.ok ? "passed" : check.blocking ? "failed" : "optional"}
            </span>
          </span>
          <span
            className={`truncate text-xs ${
              check.ok ? "text-ink-muted" : "text-destructive"
            }`}
          >
            {check.detail}
          </span>
        </li>
      ))}
    </ul>
  );
}

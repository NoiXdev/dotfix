import type { DiffLine, FileDiff, LineKind } from "../types";

// A leading glyph carries the same information as the colour, so a reader
// who can't (or doesn't want to) rely on colour still sees which lines
// change. `aria-label` repeats it in words for screen readers, which don't
// reliably announce "+" or "-" as meaningful.
const MARK: Record<LineKind, string> = {
  context: " ",
  added: "+",
  removed: "-",
};

const WORD: Record<LineKind, string> = {
  context: "Unchanged",
  added: "Added",
  removed: "Removed",
};

const TONE: Record<LineKind, string> = {
  context: "text-ink-muted",
  added: "bg-added-soft text-ink",
  removed: "bg-destructive-soft text-ink",
};

function Line({ line }: { line: DiffLine }) {
  return (
    <span
      data-kind={line.kind}
      aria-label={`${WORD[line.kind]}: ${line.text}`}
      className={`block px-1 ${TONE[line.kind]}`}
    >
      <span aria-hidden="true">{MARK[line.kind]}</span> {line.text}
    </span>
  );
}

/**
 * A redacted, read-only line diff. `line.text` is rendered exactly as it
 * arrives — the backend has already replaced any resolved secret with
 * «redacted» before the diff ever reaches the webview, so this component's
 * only job is to not undo that: no other source of file content, no
 * reconstruction, and nothing here goes to the console.
 */
export default function DiffView({ diff }: { diff: FileDiff }) {
  return (
    <div>
      <pre
        aria-label={`Diff for ${diff.target}`}
        className="max-h-64 overflow-auto rounded-md border border-control bg-surface p-1 font-mono text-xs leading-5"
      >
        {diff.lines.map((line, i) => (
          <Line key={i} line={line} />
        ))}
      </pre>
      {diff.truncated && (
        <p className="mt-2 text-xs text-ink-muted">
          Diff truncated — open the file directly to see the rest.
        </p>
      )}
    </div>
  );
}

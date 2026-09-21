import type { Missing } from "../types";

/**
 * Software a set asks for that this machine does not have.
 *
 * Above the tabs rather than inside one: it is not drift, nothing here has a
 * button, and it applies to the machine as a whole. dotfix runs a fixed set
 * of programs and will not run an installer out of a git repository, so the
 * command is text to copy — which is also why it is selectable and wraps
 * rather than being truncated.
 */
export default function MissingSoftware({ items }: { items: Missing[] }) {
  if (items.length === 0) return null;

  return (
    <section
      aria-label="Not installed"
      className="mb-3 rounded-md border border-decision bg-decision-soft px-3 py-2"
    >
      <p className="text-xs font-medium text-decision">
        Not installed — dotfix cannot install these for you
      </p>
      <ul className="mt-1 flex flex-col gap-1">
        {items.map((item) => (
          <li key={`${item.set}:${item.name}`} className="text-xs text-ink">
            <span className="font-medium">{item.name}</span>
            <span className="text-ink-muted"> — needed by set `{item.set}`</span>
            {item.hint ? (
              <code className="mt-0.5 block font-mono text-[11px] break-all text-ink-muted select-all">
                {item.hint}
              </code>
            ) : null}
          </li>
        ))}
      </ul>
    </section>
  );
}

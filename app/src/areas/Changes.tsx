import Empty from "../components/Empty";
import Row from "../components/Row";
import type { Item } from "../types";

/**
 * Everything dotfix can carry out on its own: installs, uninstalls, file
 * writes, file removals. Each row's button repeats the `action` word the
 * view model already computed — this component never invents wording, it
 * only decides layout and which ids "Apply all" is allowed to send.
 */
export default function Changes({
  items,
  onApply,
}: {
  items: Item[];
  onApply: (ids: string[]) => void;
}) {
  if (items.length === 0) return <Empty>Nothing to apply</Empty>;

  const actionable = items.filter((i) => i.actionable).map((i) => i.id);

  return (
    <section>
      {actionable.length > 0 && (
        <button
          type="button"
          className="mb-2 rounded-md bg-ink px-3 py-1 text-xs font-medium text-surface"
          onClick={() => onApply(actionable)}
        >
          Apply all
        </button>
      )}
      <ul>
        {items.map((item) => (
          <Row
            key={item.id}
            label={item.label}
            detail={item.detail}
            disabled={!item.actionable}
          >
            <button
              type="button"
              disabled={!item.actionable}
              className="shrink-0 rounded-md border border-control px-2 py-0.5 text-xs capitalize text-ink disabled:cursor-not-allowed"
              onClick={() => onApply([item.id])}
            >
              {item.action}
            </button>
          </Row>
        ))}
      </ul>
    </section>
  );
}

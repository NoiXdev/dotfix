import { useState } from "react";

import Empty from "../components/Empty";
import Row from "../components/Row";
import Select from "../components/Select";
import type { Item, SetEntry, Undeclared } from "../types";

/**
 * Things that need a decision before dotfix can manage them. Two different
 * decisions share this area, and the item's `action` says which:
 *
 * - `adopt` — a package installed by hand and in no set. Pick a set and
 *   adopt it, or ignore it. "Adopt" resolves the decision into a set — the
 *   only reason this row exists — so its button carries the `decision`
 *   accent; "Ignore" is the quiet way out and stays unaccented.
 * - `drop` — a package dotfix installed that was then removed locally. The
 *   backend has exactly one proposal for it: drop it from the set it is
 *   already in. So there is no set to pick, and no Ignore: ignoring is only
 *   consulted for packages that are installed but in no set, so it would
 *   write the machine file and leave the row untouched. The backend refuses
 *   it now; this renders only the button that does something.
 */
export default function Unmanaged({
  items,
  sets,
  undeclared,
  onAdopt,
  onDeclare,
}: {
  items: Item[];
  sets: SetEntry[];
  /** Software present here that no active set declares. */
  undeclared: Undeclared[];
  onAdopt: (id: string, set: string | null, ignore: boolean) => void;
  onDeclare: (name: string, set: string) => void;
}) {
  const [chosen, setChosen] = useState<Record<string, string>>({});
  // Only truly empty when both lists are: undeclared software belongs in
  // this tab for the same reason packages do — it needs a decision — and an
  // "nothing unmanaged" would hide it.
  if (items.length === 0 && undeclared.length === 0) {
    return <Empty>Nothing unmanaged</Empty>;
  }

  const defaultSet = sets[0]?.name ?? "core";

  return (
    <>
      {undeclared.length > 0 ? (
        <section aria-label="Undeclared software" className="mb-3">
          <p className="mb-1 text-xs font-medium text-ink">
            Installed here, in no set
          </p>
          <ul>
            {undeclared.map((item) => (
              <li
                key={item.name}
                className="flex items-center justify-between gap-3 border-b border-hairline py-2"
              >
                <span className="min-w-0">
                  <span className="block truncate text-sm text-ink">
                    {item.name}
                  </span>
                  <span className="block truncate text-xs text-ink-muted">
                    not a Homebrew package — dotfix can record it, not install it
                  </span>
                </span>
                <span className="flex shrink-0 items-center gap-1.5">
                  <Select
                    compact
                    ariaLabel={`Set for ${item.name}`}
                    value={chosen[item.name] ?? sets[0]?.name ?? ""}
                    onChange={(name) =>
                      setChosen({ ...chosen, [item.name]: name })
                    }
                    options={sets.map((s) => ({ value: s.name, label: s.name }))}
                  />
                  <button
                    type="button"
                    className="shrink-0 rounded-md border border-decision px-2 py-0.5 text-xs font-medium text-decision"
                    onClick={() =>
                      onDeclare(item.name, chosen[item.name] ?? sets[0]?.name ?? "")
                    }
                  >
                    Declare
                  </button>
                </span>
              </li>
            ))}
          </ul>
        </section>
      ) : null}
    <ul>
      {items.map((item) => {
        const value = chosen[item.id] ?? defaultSet;
        if (item.action === "drop") {
          return (
            <Row key={item.id} label={item.label} detail={item.detail}>
              <button
                type="button"
                className="shrink-0 rounded-md border border-decision px-2 py-0.5 text-xs font-medium text-decision"
                onClick={() => onAdopt(item.id, null, false)}
              >
                Drop from set
              </button>
            </Row>
          );
        }
        return (
          <Row key={item.id} label={item.label} detail={item.detail}>
            <span className="flex shrink-0 items-center gap-1.5">
              <Select
                compact
                ariaLabel={`Set for ${item.label}`}
                value={value}
                onChange={(name) => setChosen({ ...chosen, [item.id]: name })}
                options={sets.map((s) => ({ value: s.name, label: s.name }))}
              />
              <button
                type="button"
                className="rounded-md border border-decision px-2 py-0.5 text-xs font-medium text-decision"
                onClick={() => onAdopt(item.id, value, false)}
              >
                Adopt
              </button>
              <button
                type="button"
                className="rounded-md px-2 py-0.5 text-xs text-ink-muted"
                onClick={() => onAdopt(item.id, null, true)}
              >
                Ignore
              </button>
            </span>
          </Row>
        );
      })}
    </ul>
    </>
  );
}

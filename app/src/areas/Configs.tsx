import { useState } from "react";

import DiffView from "../components/DiffView";
import Empty from "../components/Empty";
import type { FileDiff, Item } from "../types";

/**
 * Managed files the user edited by hand. Opening one loads the diff between
 * what is on disk and what the repository would write — lazily, so a long
 * list of local edits never fetches every file's contents up front.
 *
 * The only action offered is overwriting the local file from the
 * repository. Writing the local edit back into the repository is
 * deliberately absent here: it would silently change what every other
 * machine receives, so that stays a CLI operation (`dotfix adopt`) and the
 * backend refuses it outright for these ids.
 */
export default function Configs({
  items,
  onLoadDiff,
  onOverwrite,
}: {
  items: Item[];
  onLoadDiff: (target: string) => Promise<FileDiff>;
  /** Named for the one thing it does. It must reach `overwrite_files`, never
   * `apply_items` — see the note above — so calling it `onApply` would name
   * it after the exact command it is forbidden to call. */
  onOverwrite: (ids: string[]) => void;
}) {
  const [open, setOpen] = useState<string | null>(null);
  const [diff, setDiff] = useState<FileDiff | null>(null);

  if (items.length === 0) return <Empty>No local edits</Empty>;

  async function show(item: Item) {
    if (open === item.id) {
      setOpen(null);
      setDiff(null);
      return;
    }
    setOpen(item.id);
    setDiff(null);
    try {
      setDiff(await onLoadDiff(item.label));
    } catch {
      // The caller (the shell) surfaces the error; here we only stop
      // waiting so the row doesn't sit open with a diff that never arrives.
      setOpen(null);
    }
  }

  return (
    <ul>
      {items.map((item) => {
        const isOpen = open === item.id;
        return (
          <li key={item.id} className="border-b border-hairline py-2">
            <button
              type="button"
              aria-expanded={isOpen}
              className="flex w-full items-center justify-between gap-3 text-left"
              onClick={() => void show(item)}
            >
              <span className="min-w-0">
                <span className="block truncate text-sm font-medium text-ink">
                  {item.label}
                </span>
                {item.detail ? (
                  <span className="block truncate text-xs text-ink-muted">
                    {item.detail}
                  </span>
                ) : null}
              </span>
              <svg
                width="10"
                height="10"
                viewBox="0 0 10 10"
                fill="none"
                aria-hidden="true"
                className={`shrink-0 text-ink-muted transition-transform ${
                  isOpen ? "rotate-90" : ""
                }`}
              >
                <path
                  d="M2 1l5 4-5 4"
                  stroke="currentColor"
                  strokeWidth="1.4"
                  strokeLinecap="round"
                  strokeLinejoin="round"
                />
              </svg>
            </button>
            {isOpen && (
              <div className="mt-2">
                {diff ? (
                  <>
                    <DiffView diff={diff} />
                    <button
                      type="button"
                      className="mt-2 rounded-md border border-decision px-2 py-0.5 text-xs font-medium text-decision"
                      onClick={() => onOverwrite([item.id])}
                    >
                      Overwrite from repository
                    </button>
                  </>
                ) : (
                  <p className="text-xs text-ink-muted">Loading diff…</p>
                )}
              </div>
            )}
          </li>
        );
      })}
    </ul>
  );
}

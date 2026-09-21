import { useState } from "react";

import Empty from "../components/Empty";
import Toggle from "../components/Toggle";
import type { SetEntry } from "../types";

/**
 * Which sets this machine uses, and what each one brings.
 *
 * Each is a plain on/off decision — turning one off removes its packages and
 * shell fragments from what dotfix wants, so the caller must re-check drift
 * after any toggle here, not just record the new set list.
 *
 * The contents are collapsed by default and sit right under the switch that
 * controls them: the moment it matters most what a set contains is the moment
 * you are about to switch it off.
 *
 * Packages can be added and removed here; files and fragments open in the
 * user's editor instead. A `set.toml` is a list and edits well in a form; a
 * shell fragment is a program, and a text box in a menubar panel is the wrong
 * place to write one.
 */
export default function Sets({
  entries,
  onToggle,
  onOpen,
  onEditPackage,
}: {
  entries: SetEntry[];
  onToggle: (name: string, on: boolean) => void;
  onOpen: (relative: string) => void;
  onEditPackage: (
    set: string,
    pkg: string,
    cask: boolean,
    add: boolean,
  ) => void;
}) {
  const [open, setOpen] = useState<string | null>(null);
  const [adding, setAdding] = useState("");

  if (entries.length === 0) return <Empty>No sets configured</Empty>;

  return (
    <ul>
      {entries.map((entry) => {
        const expanded = open === entry.name;
        const counts = [
          [entry.brew.length, "formula", "formulae"],
          [entry.cask.length, "cask", "casks"],
          [entry.files.length, "file", "files"],
          [entry.fragments.length, "shell fragment", "shell fragments"],
        ] as const;
        const summary = counts
          .filter(([n]) => n > 0)
          .map(([n, one, many]) => `${n} ${n === 1 ? one : many}`)
          .join(" · ");

        return (
          <li key={entry.name} className="border-b border-hairline py-2">
            <div className="flex items-center gap-3">
              <Toggle
                checked={entry.active}
                onChange={(next) => onToggle(entry.name, next)}
                label={entry.name}
              />
              <button
                type="button"
                aria-expanded={expanded}
                onClick={() => {
                  setOpen(expanded ? null : entry.name);
                  setAdding("");
                }}
                className="min-w-0 flex-1 text-left"
              >
                <span className="block truncate text-sm font-medium text-ink">
                  {entry.name}
                </span>
                {entry.description ? (
                  <span className="block truncate text-xs text-ink-muted">
                    {entry.description}
                  </span>
                ) : null}
                <span className="block truncate text-[11px] text-ink-muted">
                  {summary || "empty"}
                </span>
              </button>
              <span aria-hidden="true" className="text-[0.6rem] text-ink-muted">
                {expanded ? "▾" : "▸"}
              </span>
            </div>

            {expanded ? (
              <div className="mt-2 flex flex-col gap-3 pl-11 text-xs">
                <Packages
                  set={entry.name}
                  label="Formulae"
                  items={entry.brew}
                  cask={false}
                  onEditPackage={onEditPackage}
                />
                <Packages
                  set={entry.name}
                  label="Casks"
                  items={entry.cask}
                  cask={true}
                  onEditPackage={onEditPackage}
                />

                <Openable
                  label="Files"
                  items={entry.files.map((f) => ({
                    text: f.target,
                    source: f.source,
                  }))}
                  onOpen={onOpen}
                />
                <Openable
                  label="Shell"
                  items={entry.fragments.map((f) => ({
                    text: f.name,
                    source: f.source,
                  }))}
                  onOpen={onOpen}
                />

                <form
                  className="flex items-center gap-2"
                  onSubmit={(e) => {
                    e.preventDefault();
                    if (!adding.trim()) return;
                    onEditPackage(entry.name, adding.trim(), false, true);
                    setAdding("");
                  }}
                >
                  <input
                    type="text"
                    value={adding}
                    onChange={(e) => setAdding(e.target.value)}
                    placeholder="add a formula"
                    aria-label={`Add a formula to ${entry.name}`}
                    className="w-40 rounded-md border border-control bg-surface px-2 py-1 text-xs text-ink"
                  />
                  <button
                    type="submit"
                    disabled={adding.trim() === ""}
                    className="rounded-md border border-control px-2 py-1 font-medium text-ink disabled:opacity-50"
                  >
                    Add
                  </button>
                </form>

                <button
                  type="button"
                  onClick={() => onOpen(entry.config_path)}
                  className="self-start rounded-md border border-control px-2 py-1 font-medium text-ink"
                >
                  Edit set.toml
                </button>
              </div>
            ) : null}
          </li>
        );
      })}
    </ul>
  );
}

function Packages({
  set,
  label,
  items,
  cask,
  onEditPackage,
}: {
  set: string;
  label: string;
  items: string[];
  cask: boolean;
  onEditPackage: (
    set: string,
    pkg: string,
    cask: boolean,
    add: boolean,
  ) => void;
}) {
  if (items.length === 0) return null;
  return (
    <div>
      <p className="font-medium text-ink">{label}</p>
      <ul className="mt-0.5 flex flex-wrap gap-1">
        {items.map((name) => (
          <li key={name}>
            <span className="inline-flex items-center gap-1 rounded-md border border-control px-1.5 py-0.5 text-ink">
              {name}
              <button
                type="button"
                aria-label={`Remove ${name} from ${set}`}
                onClick={() => onEditPackage(set, name, cask, false)}
                className="text-ink-muted"
              >
                ×
              </button>
            </span>
          </li>
        ))}
      </ul>
    </div>
  );
}

function Openable({
  label,
  items,
  onOpen,
}: {
  label: string;
  items: { text: string; source: string }[];
  onOpen: (relative: string) => void;
}) {
  if (items.length === 0) return null;
  return (
    <div>
      <p className="font-medium text-ink">{label}</p>
      <ul className="mt-0.5 flex flex-col gap-0.5">
        {items.map((item) => (
          <li key={item.source}>
            <button
              type="button"
              onClick={() => onOpen(item.source)}
              className="text-left text-ink-muted underline decoration-dotted underline-offset-2"
            >
              {item.text}
            </button>
          </li>
        ))}
      </ul>
    </div>
  );
}

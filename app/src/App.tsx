import { getCurrentWindow } from "@tauri-apps/api/window";
import { useCallback, useEffect, useState } from "react";

import {
  adoptItem,
  applyItems,
  fileDiff,
  getOverview,
  history,
  listSets,
  overwriteFiles,
  refresh,
  publish,
  toggleSet,
  openInRepo,
  declareRequirement,
  editSetPackage,
  readSettings,
  setRemote,
  setProvider,
  renameMachine,
  unignore,
} from "./api";
import Changes from "./areas/Changes";
import Configs from "./areas/Configs";
import History from "./areas/History";
import MissingSoftware from "./components/MissingSoftware";
import SettingsArea from "./areas/Settings";
import Sets from "./areas/Sets";
import Unmanaged from "./areas/Unmanaged";
import Badge from "./components/Badge";
import Busy from "./components/Busy";
import Working from "./components/Working";
import { isNotConfigured } from "./types";
import type {
  Settings, Commit, Overview, SetEntry } from "./types";
import Wizard from "./wizard/Wizard";

/** The five areas: Changes and Unmanaged act on drift, Configs reviews
 * hand-edited files, Sets decides which sets this machine uses, and History
 * is a read-only log of the configuration repository's commits. */
type Tab =
  | "changes"
  | "unmanaged"
  | "configs"
  | "sets"
  | "history"
  | "settings";

/**
 * The menubar window's shell: loads the overview (and the sets adopting
 * needs) on mount and whenever the window is shown again, then renders
 * exactly one of busy / wizard / error / tabbed-areas. A `getOverview`
 * failure that starts with `NOT_CONFIGURED_PREFIX` means dotfix has simply
 * never run on this Mac, so that state gets the wizard rather than the
 * plain error banner every other failure gets — completing it calls
 * `load()` again so the window shows the real overview it just created.
 * The tabs themselves are always reachable once the overview has
 * loaded — Sets in particular has nothing to do with drift, so it must stay
 * reachable even when nothing has drifted, which is the common case. Each
 * area renders its own calm empty state when it has nothing to show, rather
 * than this shell hiding the tabs behind a single top-level "all done"
 * screen. This only decides which of those top-level states is showing and
 * carries the commands each area can issue back to the backend, replacing
 * state with whatever `Overview` the command returns.
 */
export default function App() {
  const [overview, setOverview] = useState<Overview | null>(null);
  // What is running right now, or null. Doubles as the overlay's label, so
  // the window says which slow thing it is waiting on rather than just
  // sitting there looking broken.
  const [working, setWorking] = useState<string | null>(null);
  // Settings are not part of the drift picture and are read only when the
  // tab is opened, the same way history is.
  const [settings, setSettings] = useState<Settings | null>(null);
  const [sets, setSets] = useState<SetEntry[]>([]);
  const [commits, setCommits] = useState<Commit[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [tab, setTab] = useState<Tab>("changes");
  const [checking, setChecking] = useState(false);

  const load = useCallback(async () => {
    try {
      setError(null);
      const [nextOverview, nextSets] = await Promise.all([
        getOverview(),
        listSets(),
      ]);
      setOverview(nextOverview);
      setSets(nextSets);
    } catch (err) {
      setError((err as Error).message);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  // Closing the window only hides it — `on_window_event` in lib.rs prevents
  // the close so the process and the tray stay alive — which leaves this
  // React tree mounted. Without re-loading, re-opening the window would show
  // whatever was true at login for the rest of the day, while the hourly
  // LaunchAgent kept the shell status line correct and the two disagreed.
  //
  // Tauri's focus event rather than a `visibilitychange` listener: the window
  // is only ever re-shown by `tray::show_main`, which calls `show()` and then
  // `set_focus()`, so regaining focus is exactly the moment it becomes
  // visible again. It also covers the window being left open in the
  // background and clicked back into, and it does not depend on WKWebView
  // reporting a hidden NSWindow as `document.hidden`, which is not
  // guaranteed.
  useEffect(() => {
    const unlisten = getCurrentWindow().onFocusChanged(
      ({ payload: focused }) => {
        if (focused) void load();
      },
    );
    return () => {
      void unlisten.then((stop) => stop());
    };
  }, [load]);

  // `load` re-reads the machine; this pulls the repository first, so it is
  // the only thing in the window that can discover what another Mac pushed.
  // The set list is re-read after it, because a pull can add or remove sets.
  async function handleCheckNow() {
    setChecking(true);
    try {
      setError(null);
      setOverview(await refresh());
      setSets(await listSets());
    } catch (err) {
      setError((err as Error).message);
    } finally {
      setChecking(false);
    }
  }

  async function handleApply(ids: string[]) {
    setWorking("Applying…");
    try {
      setError(null);
      setOverview(await applyItems(ids));
    } catch (err) {
      setError((err as Error).message);
    } finally {
      setWorking(null);
    }
  }

  // Everything dotfix writes lands in the working tree and stayed there:
  // the window could pull but never publish.
  async function handlePublish() {
    setWorking("Pushing…");
    try {
      setError(null);
      const done = await publish();
      if (!done.pushed) {
        setError("No remote yet — add one in Settings to send your changes.");
      }
    } catch (err) {
      setError((err as Error).message);
    } finally {
      setWorking(null);
    }
  }

  async function handleDeclare(name: string, set: string) {
    setWorking("Recording…");
    try {
      setError(null);
      setOverview(await declareRequirement(name, set));
    } catch (err) {
      setError((err as Error).message);
    } finally {
      setWorking(null);
    }
  }

  async function handleAdopt(id: string, set: string | null, ignore: boolean) {
    setWorking("Adopting…");
    try {
      setError(null);
      setOverview(await adoptItem(id, set, ignore));
    } catch (err) {
      setError((err as Error).message);
    } finally {
      setWorking(null);
    }
  }

  // Deliberately not handleApply/applyItems: a `local_edit` id is refused by
  // apply_items/plan_selected on purpose, so a bulk apply can never silently
  // clobber a hand edit. overwrite_files is the separate, explicit path for
  // "discard my edit, take the repository's version" — the only command the
  // Configs area's overwrite action may ever call.
  async function handleOverwrite(ids: string[]) {
    setWorking("Overwriting…");
    try {
      setError(null);
      setOverview(await overwriteFiles(ids));
    } catch (err) {
      setError((err as Error).message);
    } finally {
      setWorking(null);
    }
  }

  // Loading a diff can fail (e.g. the file vanished between overview and
  // click); surface that on the shared error banner, then let it reject so
  // Configs stops waiting on it instead of showing a diff that never comes.
  async function handleLoadDiff(target: string) {
    try {
      setError(null);
      return await fileDiff(target);
    } catch (err) {
      setError((err as Error).message);
      throw err;
    }
  }

  // Turning a set on or off changes which packages and shell fragments
  // dotfix wants, so the overview — not just the set list — has to be
  // reloaded afterwards, or the drift picture would silently go stale.
  // Each of these has a `dotfix config` equivalent and calls the same core
  // function, so the window and the terminal cannot disagree about what a
  // setting means.
  async function runSetting(label: string, work: () => Promise<Settings>) {
    setWorking(label);
    try {
      setError(null);
      setSettings(await work());
      // A provider switch or a rename changes what dotfix wants, so the
      // drift picture just went stale.
      setOverview(await getOverview());
    } catch (err) {
      setError((err as Error).message);
    } finally {
      setWorking(null);
    }
  }

  // Editing a set changes what dotfix wants, so the drift picture goes
  // stale the same way a toggle makes it stale.
  async function runSetEdit(
    set: string,
    pkg: string,
    cask: boolean,
    add: boolean,
  ) {
    setWorking(add ? "Adding…" : "Removing…");
    try {
      setError(null);
      setSets(await editSetPackage(set, pkg, cask, add));
      setOverview(await getOverview());
    } catch (err) {
      setError((err as Error).message);
    } finally {
      setWorking(null);
    }
  }

  async function handleToggle(name: string, on: boolean) {
    setWorking("Updating sets…");
    try {
      setError(null);
      setSets(await toggleSet(name, on));
      setOverview(await getOverview());
    } catch (err) {
      setError((err as Error).message);
    } finally {
      setWorking(null);
    }
  }

  // History is a read-only log, not part of the drift overview, so it isn't
  // worth fetching on mount alongside it — only load it the moment the user
  // actually opens the tab.
  async function handleSelectTab(next: Tab) {
    setTab(next);
    if (next === "settings") {
      try {
        setError(null);
        setSettings(await readSettings());
      } catch (err) {
        setError((err as Error).message);
      }
      return;
    }
    if (next === "history") {
      try {
        setError(null);
        setCommits(await history(50));
      } catch (err) {
        setError((err as Error).message);
      }
    }
  }

  const changesCount = overview
    ? overview.counts.incoming + overview.counts.removed
    : 0;
  const unmanagedCount = overview?.counts.unmanaged ?? 0;
  const configsCount = overview?.counts.local_edits ?? 0;

  return (
    <div className="relative flex h-screen w-screen flex-col overflow-hidden bg-canvas font-sans text-ink">
      {working ? <Working label={working} /> : null}
      <header className="flex h-10 shrink-0 items-center justify-between border-b border-hairline px-4">
        <span className="text-[13px] font-medium">dotfix</span>
        <button
          type="button"
          disabled={checking}
          className="rounded-md border border-control px-2 py-0.5 text-xs text-ink-muted disabled:opacity-50"
          onClick={() => void handleCheckNow()}
        >
          {checking ? "Checking…" : "Check now"}
        </button>
        <button
          type="button"
          className="rounded-md border border-control px-2 py-0.5 text-xs text-ink-muted disabled:opacity-50"
          onClick={() => void handlePublish()}
        >
          Push
        </button>
      </header>
      <main className="flex-1 overflow-y-auto p-4 text-sm">
        {error && isNotConfigured(error) ? (
          <Wizard onDone={() => void load()} />
        ) : error ? (
          <p
            role="alert"
            className="rounded border border-destructive/30 bg-destructive-soft px-3 py-2 text-destructive"
          >
            {error}
          </p>
        ) : !overview ? (
          <Busy />
        ) : (
          <div className="flex h-full flex-col">
            <MissingSoftware items={overview.missing} />
            <div
              role="tablist"
              className="mb-3 flex shrink-0 gap-4 border-b border-hairline"
            >
              <button
                type="button"
                role="tab"
                aria-selected={tab === "changes"}
                className={`-mb-px flex items-center border-b-2 pb-2 text-[13px] font-medium ${
                  tab === "changes"
                    ? "border-ink text-ink"
                    : "border-transparent text-ink-muted"
                }`}
                onClick={() => setTab("changes")}
              >
                Changes
                <Badge count={changesCount} />
              </button>
              <button
                type="button"
                role="tab"
                aria-selected={tab === "unmanaged"}
                className={`-mb-px flex items-center border-b-2 pb-2 text-[13px] font-medium ${
                  tab === "unmanaged"
                    ? "border-ink text-ink"
                    : "border-transparent text-ink-muted"
                }`}
                onClick={() => setTab("unmanaged")}
              >
                Unmanaged
                <Badge count={unmanagedCount} />
              </button>
              <button
                type="button"
                role="tab"
                aria-selected={tab === "configs"}
                className={`-mb-px flex items-center border-b-2 pb-2 text-[13px] font-medium ${
                  tab === "configs"
                    ? "border-ink text-ink"
                    : "border-transparent text-ink-muted"
                }`}
                onClick={() => setTab("configs")}
              >
                Configs
                <Badge count={configsCount} />
              </button>
              <button
                type="button"
                role="tab"
                aria-selected={tab === "sets"}
                className={`-mb-px flex items-center border-b-2 pb-2 text-[13px] font-medium ${
                  tab === "sets"
                    ? "border-ink text-ink"
                    : "border-transparent text-ink-muted"
                }`}
                onClick={() => void handleSelectTab("sets")}
              >
                Sets
              </button>
              <button
                type="button"
                role="tab"
                aria-selected={tab === "history"}
                className={`-mb-px flex items-center border-b-2 pb-2 text-[13px] font-medium ${
                  tab === "history"
                    ? "border-ink text-ink"
                    : "border-transparent text-ink-muted"
                }`}
                onClick={() => void handleSelectTab("history")}
              >
                History
              </button>
              <button
                type="button"
                role="tab"
                aria-selected={tab === "settings"}
                className={`-mb-px flex items-center border-b-2 pb-2 text-[13px] font-medium ${
                  tab === "settings"
                    ? "border-ink text-ink"
                    : "border-transparent text-ink-muted"
                }`}
                onClick={() => void handleSelectTab("settings")}
              >
                Settings
              </button>
            </div>
            <div role="tabpanel" className="flex-1 overflow-y-auto">
              {tab === "changes" ? (
                <Changes
                  items={overview.items.filter((i) => i.area === "changes")}
                  onApply={(ids) => void handleApply(ids)}
                />
              ) : tab === "unmanaged" ? (
                <Unmanaged
                  items={overview.items.filter((i) => i.area === "unmanaged")}
                  sets={sets}
                  onAdopt={(id, set, ignore) =>
                    void handleAdopt(id, set, ignore)
                  }
                  undeclared={overview.undeclared}
                  onDeclare={(name, set) => void handleDeclare(name, set)}
                />
              ) : tab === "configs" ? (
                <Configs
                  items={overview.items.filter((i) => i.area === "configs")}
                  onLoadDiff={handleLoadDiff}
                  onOverwrite={(ids) => void handleOverwrite(ids)}
                />
              ) : tab === "sets" ? (
                <Sets
                  entries={sets}
                  onToggle={(name, on) => void handleToggle(name, on)}
                  onOpen={(relative) => void openInRepo(relative)}
                  onEditPackage={(set, pkg, cask, add) =>
                    void runSetEdit(set, pkg, cask, add)
                  }
                />
              ) : tab === "history" ? (
                <History commits={commits} />
              ) : settings ? (
                <SettingsArea
                  value={settings}
                  onSetRemote={(url) =>
                    void runSetting("Setting remote…", () => setRemote(url))
                  }
                  onSetProvider={(provider, vault) =>
                    void runSetting("Checking secrets…", () =>
                      setProvider(provider, vault),
                    )
                  }
                  onRename={(name) =>
                    void runSetting("Renaming…", () => renameMachine(name))
                  }
                  onUnignore={(name) =>
                    void runSetting("Updating…", () => unignore(name))
                  }
                />
              ) : (
                <Busy />
              )}
            </div>
          </div>
        )}
      </main>
    </div>
  );
}

import { act, cleanup, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => invoke(...a) }));

// The window is hidden, not destroyed, when it is closed, so the shell
// re-loads whenever it regains focus. Capture the handler so a test can fire
// it — there is no real Tauri window under jsdom.
const focusHandlers: Array<(event: { payload: boolean }) => void> = [];
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    onFocusChanged: (handler: (event: { payload: boolean }) => void) => {
      focusHandlers.push(handler);
      return Promise.resolve(() => {});
    },
  }),
}));

import App from "./App";

const empty = { counts: { incoming: 0, unmanaged: 0, removed: 0, local_edits: 0 }, items: [], missing: [], undeclared: [], status_line: null };

const oneChange = {
  counts: { incoming: 1, unmanaged: 0, removed: 0, local_edits: 0 },
  items: [
    {
      id: "incoming_package:formula:jq",
      label: "jq",
      area: "changes",
      action: "install",
      detail: "",
      actionable: true,
    },
  ],
      missing: [], undeclared: [],
  status_line: null,
};

describe("App", () => {
  // Reset after each test rather than before: resetting a vi.mock-referenced
  // vi.fn() from inside a beforeEach hook trips a Vitest bug (reproduced in
  // isolation, independent of this codebase, on Vitest 3.2.7 and 5.0.1) that
  // misattributes a phantom "unhandled rejection" failure to a later test's
  // correctly-awaited rejection. afterEach resets on exactly the same test
  // boundary without hitting it; it also unmounts between tests since this
  // project doesn't run testing-library's global auto-cleanup.
  afterEach(() => {
    invoke.mockReset();
    focusHandlers.length = 0;
    cleanup();
  });

  it("re-loads when the window is shown again rather than serving a snapshot", async () => {
    // Closing the window only hides it (`on_window_event` in lib.rs keeps
    // the process and the tray alive), so the React tree stays mounted. A
    // shell that only loads on mount would show whatever was true at login
    // for the rest of the day, while the hourly LaunchAgent updates the
    // shell status line and the two disagree.
    let overviewCalls = 0;
    invoke.mockImplementation((cmd: string) => {
      if (cmd === "overview") {
        overviewCalls += 1;
        return Promise.resolve(overviewCalls === 1 ? oneChange : empty);
      }
      if (cmd === "list_sets") return Promise.resolve([]);
      throw new Error(`unexpected command ${cmd}`);
    });

    render(<App />);
    await waitFor(() => expect(screen.getByText("jq")).toBeInTheDocument());
    await waitFor(() => expect(focusHandlers.length).toBeGreaterThan(0));

    await act(async () => {
      for (const handler of focusHandlers) handler({ payload: true });
    });

    await waitFor(() =>
      expect(screen.getByText(/nothing to apply/i)).toBeInTheDocument(),
    );
  });

  it("checks now through the refresh command, not a plain re-read", async () => {
    // `overview` reads the machine without touching the network; only
    // `refresh` pulls the repository first. "Check now" must be the latter,
    // or it can never discover what a colleague pushed.
    invoke.mockImplementation((cmd: string) => {
      if (cmd === "overview") return Promise.resolve(oneChange);
      if (cmd === "list_sets") return Promise.resolve([]);
      if (cmd === "refresh") return Promise.resolve(empty);
      throw new Error(`unexpected command ${cmd}`);
    });

    render(<App />);
    await waitFor(() => expect(screen.getByText("jq")).toBeInTheDocument());

    screen.getByRole("button", { name: /check now/i }).click();

    await waitFor(() => expect(invoke).toHaveBeenCalledWith("refresh"));
    await waitFor(() =>
      expect(screen.getByText(/nothing to apply/i)).toBeInTheDocument(),
    );
  });

  it("shows each area's own calm empty state when nothing drifted", async () => {
    invoke.mockImplementation((cmd: string) => {
      if (cmd === "overview") return Promise.resolve(empty);
      if (cmd === "list_sets")
        return Promise.resolve([{ name: "core", active: true, description: "Base set", brew: [], cask: [], files: [], fragments: [], config_path: "/repo/sets/x/set.toml" }]);
      throw new Error(`unexpected command ${cmd}`);
    });
    render(<App />);
    await waitFor(() =>
      expect(screen.getByText(/nothing to apply/i)).toBeInTheDocument(),
    );
  });

  it("keeps Sets reachable even when nothing has drifted", async () => {
    // No drift anywhere is the normal state for this tool, and Sets — which
    // sets this machine uses — has nothing to do with drift. It must not be
    // hidden behind a top-level "all done" screen that only Changes,
    // Unmanaged and Configs have any business showing.
    invoke.mockImplementation((cmd: string) => {
      if (cmd === "overview") return Promise.resolve(empty);
      if (cmd === "list_sets")
        return Promise.resolve([{ name: "core", active: true, description: "Base set", brew: [], cask: [], files: [], fragments: [], config_path: "/repo/sets/x/set.toml" }]);
      throw new Error(`unexpected command ${cmd}`);
    });
    render(<App />);
    await waitFor(() =>
      expect(screen.getByRole("tab", { name: /^sets/i })).toBeInTheDocument(),
    );

    screen.getByRole("tab", { name: /^sets/i }).click();

    await waitFor(() =>
      expect(screen.getByRole("switch", { name: /core/i })).toBeInTheDocument(),
    );
  });

  it("shows an error banner when the backend fails", async () => {
    invoke.mockRejectedValue("repository has diverged from origin");
    render(<App />);
    await waitFor(() =>
      expect(screen.getByRole("alert")).toHaveTextContent(/diverged/i),
    );
  });

  it("defaults to the changes tab and applies an item through the backend", async () => {
    const withItem = {
      counts: { incoming: 1, unmanaged: 0, removed: 0, local_edits: 0 },
      items: [
        {
          id: "incoming_package:formula:jq",
          label: "jq",
          area: "changes",
          action: "install",
          detail: "",
          actionable: true,
        },
      ],
      missing: [], undeclared: [],
      status_line: null,
    };
    invoke.mockImplementation((cmd: string) => {
      if (cmd === "overview") return Promise.resolve(withItem);
      if (cmd === "list_sets") return Promise.resolve([]);
      if (cmd === "apply_items") return Promise.resolve(empty);
      throw new Error(`unexpected command ${cmd}`);
    });

    render(<App />);
    await waitFor(() => expect(screen.getByText("jq")).toBeInTheDocument());
    expect(screen.getByRole("tab", { name: /changes/i })).toHaveAttribute(
      "aria-selected",
      "true",
    );

    screen.getByRole("button", { name: /install/i }).click();

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("apply_items", {
        ids: ["incoming_package:formula:jq"],
      }),
    );
    await waitFor(() =>
      expect(screen.getByText(/nothing to apply/i)).toBeInTheDocument(),
    );
  });

  it("switches to the unmanaged tab and adopts an item into the chosen set", async () => {
    const withBoth = {
      counts: { incoming: 1, unmanaged: 1, removed: 0, local_edits: 0 },
      items: [
        {
          id: "incoming_package:formula:jq",
          label: "jq",
          area: "changes",
          action: "install",
          detail: "",
          actionable: true,
        },
        {
          id: "unmanaged:formula:stray",
          label: "stray",
          area: "unmanaged",
          action: "adopt",
          detail: "in no set",
          actionable: true,
        },
      ],
      missing: [], undeclared: [],
      status_line: null,
    };
    invoke.mockImplementation((cmd: string) => {
      if (cmd === "overview") return Promise.resolve(withBoth);
      if (cmd === "list_sets")
        return Promise.resolve([{ name: "core", active: true, description: "Base", brew: [], cask: [], files: [], fragments: [], config_path: "/repo/sets/x/set.toml" }]);
      if (cmd === "adopt_item") return Promise.resolve(empty);
      throw new Error(`unexpected command ${cmd}`);
    });

    render(<App />);
    await waitFor(() => expect(screen.getByText("jq")).toBeInTheDocument());

    screen.getByRole("tab", { name: /unmanaged/i }).click();
    await waitFor(() => expect(screen.getByText("stray")).toBeInTheDocument());

    screen.getByRole("button", { name: /^adopt$/i }).click();

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("adopt_item", {
        id: "unmanaged:formula:stray",
        set: "core",
        ignore: false,
      }),
    );
    await waitFor(() =>
      expect(screen.getByText(/nothing unmanaged/i)).toBeInTheDocument(),
    );
  });

  it("switches to the configs tab and overwrites a local edit from its diff", async () => {
    const withConfig = {
      counts: { incoming: 0, unmanaged: 0, removed: 0, local_edits: 1 },
      items: [
        {
          id: "local_edit:/Users/test/.gitconfig",
          label: "/Users/test/.gitconfig",
          area: "configs",
          action: "review",
          detail: "core",
          actionable: true,
        },
      ],
      missing: [], undeclared: [],
      status_line: null,
    };
    const diff = {
      target: "/Users/test/.gitconfig",
      set: "core",
      lines: [{ kind: "added", text: "email = new@example.com" }],
      truncated: false,
    };
    invoke.mockImplementation((cmd: string) => {
      if (cmd === "overview") return Promise.resolve(withConfig);
      if (cmd === "list_sets") return Promise.resolve([]);
      if (cmd === "file_diff") return Promise.resolve(diff);
      if (cmd === "overwrite_files") return Promise.resolve(empty);
      throw new Error(`unexpected command ${cmd}`);
    });

    render(<App />);
    await waitFor(() =>
      expect(screen.getByRole("tab", { name: /configs/i })).toBeInTheDocument(),
    );
    screen.getByRole("tab", { name: /configs/i }).click();

    await waitFor(() =>
      expect(screen.getByRole("button", { name: /\.gitconfig/ })).toBeInTheDocument(),
    );
    screen.getByRole("button", { name: /\.gitconfig/ }).click();

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("file_diff", {
        target: "/Users/test/.gitconfig",
      }),
    );
    expect(await screen.findByText(/new@example.com/)).toBeInTheDocument();

    screen.getByRole("button", { name: /overwrite/i }).click();

    // This is the whole point of the fix: overwriting a hand-edited file
    // must reach overwrite_files, never apply_items — apply_items refuses a
    // local_edit id outright (plan_selected never plans one), so routing
    // this button through it would silently do nothing.
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("overwrite_files", {
        ids: ["local_edit:/Users/test/.gitconfig"],
      }),
    );
    expect(invoke).not.toHaveBeenCalledWith(
      "apply_items",
      expect.objectContaining({ ids: ["local_edit:/Users/test/.gitconfig"] }),
    );
    await waitFor(() =>
      expect(screen.getByText(/no local edits/i)).toBeInTheDocument(),
    );
  });

  it("switches to the sets tab, toggles a set, and refreshes the overview", async () => {
    const withItem = {
      counts: { incoming: 1, unmanaged: 0, removed: 0, local_edits: 0 },
      items: [
        {
          id: "incoming_package:formula:jq",
          label: "jq",
          area: "changes",
          action: "install",
          detail: "",
          actionable: true,
        },
      ],
      missing: [], undeclared: [],
      status_line: null,
    };
    const afterToggle = {
      counts: { incoming: 0, unmanaged: 0, removed: 2, local_edits: 0 },
      items: [
        {
          id: "removed_package:formula:stale",
          label: "stale",
          area: "changes",
          action: "uninstall",
          detail: "",
          actionable: true,
        },
      ],
      missing: [], undeclared: [],
      status_line: null,
    };
    const setsBefore = [
      { name: "core", active: true, description: "Base set", brew: [], cask: [], files: [], fragments: [], config_path: "/repo/sets/x/set.toml" },
      { name: "mobile", active: false, description: "iOS and Android", brew: [], cask: [], files: [], fragments: [], config_path: "/repo/sets/x/set.toml" },
    ];
    const setsAfter = [
      { name: "core", active: true, description: "Base set", brew: [], cask: [], files: [], fragments: [], config_path: "/repo/sets/x/set.toml" },
      { name: "mobile", active: true, description: "iOS and Android", brew: [], cask: [], files: [], fragments: [], config_path: "/repo/sets/x/set.toml" },
    ];

    let overviewCalls = 0;
    invoke.mockImplementation((cmd: string) => {
      if (cmd === "overview") {
        overviewCalls += 1;
        return Promise.resolve(overviewCalls === 1 ? withItem : afterToggle);
      }
      if (cmd === "list_sets") return Promise.resolve(setsBefore);
      if (cmd === "toggle_set") return Promise.resolve(setsAfter);
      throw new Error(`unexpected command ${cmd}`);
    });

    render(<App />);
    await waitFor(() => expect(screen.getByText("jq")).toBeInTheDocument());

    screen.getByRole("tab", { name: /^sets/i }).click();
    await waitFor(() =>
      expect(screen.getByRole("switch", { name: /mobile/i })).not.toBeChecked(),
    );

    screen.getByRole("switch", { name: /mobile/i }).click();

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("toggle_set", { name: "mobile", on: true }),
    );
    // The set list alone isn't enough: turning a set on or off changes what
    // drifts, so the shell must also pull a fresh overview afterwards.
    await waitFor(() =>
      expect(invoke.mock.calls.filter((c) => c[0] === "overview")).toHaveLength(2),
    );
    await waitFor(() =>
      expect(screen.getByRole("switch", { name: /mobile/i })).toBeChecked(),
    );

    const changesTab = screen.getByRole("tab", { name: /changes/i });
    expect(within(changesTab).getByText("2")).toBeInTheDocument();
  });

  it("loads history only once the history tab is opened, not eagerly", async () => {
    invoke.mockImplementation((cmd: string) => {
      if (cmd === "overview") return Promise.resolve(empty);
      if (cmd === "list_sets") return Promise.resolve([]);
      if (cmd === "history")
        return Promise.resolve([
          { hash: "abc1234", subject: "add jq to core", date: "2026-09-16" },
        ]);
      throw new Error(`unexpected command ${cmd}`);
    });

    render(<App />);
    await waitFor(() =>
      expect(screen.getByRole("tab", { name: /history/i })).toBeInTheDocument(),
    );

    // Loading the shell must not eagerly fetch history: only Changes,
    // Unmanaged and Configs are driven by the overview loaded on mount.
    expect(invoke).not.toHaveBeenCalledWith("history", expect.anything());

    screen.getByRole("tab", { name: /history/i }).click();

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("history", { limit: 50 }),
    );
    await waitFor(() =>
      expect(screen.getByText("add jq to core")).toBeInTheDocument(),
    );
  });
});

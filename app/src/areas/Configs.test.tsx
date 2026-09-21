import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { FileDiff, Item } from "../types";
import Configs from "./Configs";

const item: Item = {
  id: "local_edit:/Users/test/.gitconfig",
  label: "/Users/test/.gitconfig",
  area: "configs",
  action: "review",
  detail: "core",
  actionable: true,
};

const diff: FileDiff = {
  target: "/Users/test/.gitconfig",
  set: "core",
  lines: [{ kind: "added", text: "  email = new@example.com" }],
  truncated: false,
};

describe("Configs", () => {
  // This project doesn't run testing-library's global auto-cleanup (see
  // App.test.tsx); each `it` renders its own tree, so unmount between tests.
  afterEach(() => cleanup());

  it("loads the diff only when a file is opened", async () => {
    const onLoadDiff = vi.fn().mockResolvedValue(diff);
    render(<Configs items={[item]} onLoadDiff={onLoadDiff} onOverwrite={vi.fn()} />);

    expect(onLoadDiff).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: /\.gitconfig/ }));
    await waitFor(() =>
      expect(onLoadDiff).toHaveBeenCalledWith("/Users/test/.gitconfig"),
    );
    expect(await screen.findByText(/new@example.com/)).toBeInTheDocument();
  });

  it("offers overwriting the local file from the repository", async () => {
    const onOverwrite = vi.fn();
    render(
      <Configs
        items={[item]}
        onLoadDiff={vi.fn().mockResolvedValue(diff)}
        onOverwrite={onOverwrite}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /\.gitconfig/ }));
    fireEvent.click(await screen.findByRole("button", { name: /overwrite/i }));
    expect(onOverwrite).toHaveBeenCalledWith([
      "local_edit:/Users/test/.gitconfig",
    ]);
  });

  it("is calm when no config was edited", () => {
    render(<Configs items={[]} onLoadDiff={vi.fn()} onOverwrite={vi.fn()} />);
    expect(screen.getByText(/no local edits/i)).toBeInTheDocument();
  });
});

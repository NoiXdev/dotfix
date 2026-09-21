import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import type { FileDiff } from "../types";
import DiffView from "./DiffView";

const diff: FileDiff = {
  target: "/Users/test/.gitconfig",
  set: "core",
  lines: [
    { kind: "context", text: "[user]" },
    { kind: "removed", text: "  email = old@example.com" },
    { kind: "added", text: "  email = new@example.com" },
  ],
  truncated: false,
};

describe("DiffView", () => {
  // This project doesn't run testing-library's global auto-cleanup (see
  // App.test.tsx); each `it` renders its own tree, so unmount between tests.
  afterEach(() => cleanup());

  it("marks added and removed lines for screen readers, not just by colour", () => {
    render(<DiffView diff={diff} />);
    expect(screen.getByText(/old@example.com/)).toHaveAttribute(
      "data-kind",
      "removed",
    );
    expect(screen.getByText(/new@example.com/)).toHaveAttribute(
      "data-kind",
      "added",
    );
  });

  it("says so when the diff was truncated", () => {
    render(<DiffView diff={{ ...diff, truncated: true }} />);
    expect(screen.getByText(/truncated/i)).toBeInTheDocument();
  });

  it("says nothing about truncation for a short diff", () => {
    render(<DiffView diff={diff} />);
    expect(screen.queryByText(/truncated/i)).toBeNull();
  });
});

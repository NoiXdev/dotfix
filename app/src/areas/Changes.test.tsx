import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { Item } from "../types";
import Changes from "./Changes";

const item = (over: Partial<Item> = {}): Item => ({
  id: "incoming_package:formula:jq",
  label: "jq",
  area: "changes",
  action: "install",
  detail: "",
  actionable: true,
  ...over,
});

describe("Changes", () => {
  // This project doesn't run testing-library's global auto-cleanup (see
  // App.test.tsx); each `it` renders its own tree, so unmount between tests.
  afterEach(() => cleanup());

  it("lists each item with the action that would be taken", () => {
    render(<Changes items={[item()]} onApply={vi.fn()} />);
    expect(screen.getByText("jq")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /install/i })).toBeEnabled();
  });

  it("disables a blocked item and shows why", () => {
    render(
      <Changes
        items={[
          item({
            actionable: false,
            action: "uninstall",
            detail: "still required by maven",
            label: "openjdk",
          }),
        ]}
        onApply={vi.fn()}
      />,
    );
    expect(screen.getByRole("button", { name: /uninstall/i })).toBeDisabled();
    expect(screen.getByText(/still required by maven/)).toBeInTheDocument();
  });

  it("applies a single item by id", () => {
    const onApply = vi.fn();
    render(<Changes items={[item()]} onApply={onApply} />);
    fireEvent.click(screen.getByRole("button", { name: /install/i }));
    expect(onApply).toHaveBeenCalledWith(["incoming_package:formula:jq"]);
  });

  it("apply all sends only the actionable ids", () => {
    const onApply = vi.fn();
    render(
      <Changes
        items={[item(), item({ id: "blocked", actionable: false })]}
        onApply={onApply}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /apply all/i }));
    expect(onApply).toHaveBeenCalledWith(["incoming_package:formula:jq"]);
  });

  it("shows nothing to do when the list is empty", () => {
    render(<Changes items={[]} onApply={vi.fn()} />);
    expect(screen.getByText(/nothing to apply/i)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /apply all/i })).toBeNull();
  });
});

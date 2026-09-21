import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { SetEntry } from "../types";
import Sets from "./Sets";

const entries: SetEntry[] = [
  { name: "core", active: true, description: "Base set", brew: ["htop"], cask: [], files: [{ target: "~/.gitconfig", source: "sets/core/files/gitconfig" }], fragments: [{ name: "10-a.zsh", source: "sets/core/shell/10-a.zsh" }], config_path: "sets/core/set.toml" },
  { name: "mobile", active: false, description: "iOS and Android", brew: ["htop"], cask: [], files: [{ target: "~/.gitconfig", source: "sets/core/files/gitconfig" }], fragments: [{ name: "10-a.zsh", source: "sets/core/shell/10-a.zsh" }], config_path: "sets/mobile/set.toml" },
];

describe("Sets", () => {
  // This project doesn't run testing-library's global auto-cleanup (see
  // App.test.tsx); each `it` renders its own tree, so unmount between tests.
  afterEach(() => cleanup());

  it("shows every set with its description and current state", () => {
    render(<Sets entries={entries} onToggle={vi.fn()} onOpen={vi.fn()} onEditPackage={vi.fn()} />);
    expect(screen.getByRole("switch", { name: /core/i })).toBeChecked();
    expect(screen.getByRole("switch", { name: /mobile/i })).not.toBeChecked();
    expect(screen.getByText("iOS and Android")).toBeInTheDocument();
  });

  it("toggling a set reports the new state", () => {
    const onToggle = vi.fn();
    render(<Sets entries={entries} onToggle={onToggle} onOpen={vi.fn()} onEditPackage={vi.fn()} />);
    fireEvent.click(screen.getByRole("switch", { name: /mobile/i }));
    expect(onToggle).toHaveBeenCalledWith("mobile", true);
  });

  it("switching a set off reports false", () => {
    const onToggle = vi.fn();
    render(<Sets entries={entries} onToggle={onToggle} onOpen={vi.fn()} onEditPackage={vi.fn()} />);
    fireEvent.click(screen.getByRole("switch", { name: /core/i }));
    expect(onToggle).toHaveBeenCalledWith("core", false);
  });

  it("uses a switch, not a checkbox", () => {
    // A checkbox says "one of several things I am selecting"; a switch says
    // "this takes effect now". Activating a set is the second kind.
    render(<Sets entries={entries} onToggle={vi.fn()} onOpen={vi.fn()} onEditPackage={vi.fn()} />);
    expect(screen.queryByRole("checkbox")).toBeNull();
    expect(screen.getByRole("switch", { name: /core/i })).toBeChecked();
  });

  it("shows what a set contains and offers to edit its config", () => {
    // Until now the only way to learn what a set brings was to open the
    // repository — and that is needed most just before switching it off.
    const onOpenConfig = vi.fn();
    render(
      <Sets entries={entries} onToggle={vi.fn()} onOpen={onOpenConfig} onEditPackage={vi.fn()} />,
    );

    fireEvent.click(screen.getByRole("button", { expanded: false, name: /core/i }));
    expect(screen.getByText(/htop/)).toBeInTheDocument();
    expect(screen.getByText(/10-a\.zsh/)).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /edit set\.toml/i }));
    expect(onOpenConfig).toHaveBeenCalledWith("sets/core/set.toml");
  });

  it("removes a package from the set", () => {
    const onEditPackage = vi.fn();
    render(
      <Sets
        entries={entries}
        onToggle={vi.fn()}
        onOpen={vi.fn()}
        onEditPackage={onEditPackage}
      />,
    );
    fireEvent.click(screen.getByRole("button", { expanded: false, name: /core/i }));
    fireEvent.click(screen.getByRole("button", { name: /remove htop from core/i }));
    expect(onEditPackage).toHaveBeenCalledWith("core", "htop", false, false);
  });

  it("adds a package, and will not submit an empty name", () => {
    const onEditPackage = vi.fn();
    render(
      <Sets
        entries={entries}
        onToggle={vi.fn()}
        onOpen={vi.fn()}
        onEditPackage={onEditPackage}
      />,
    );
    fireEvent.click(screen.getByRole("button", { expanded: false, name: /core/i }));
    expect(screen.getByRole("button", { name: /^add$/i })).toBeDisabled();

    fireEvent.change(screen.getByLabelText(/add a formula to core/i), {
      target: { value: "ripgrep" },
    });
    fireEvent.click(screen.getByRole("button", { name: /^add$/i }));
    expect(onEditPackage).toHaveBeenCalledWith("core", "ripgrep", false, true);
  });

  it("opens an individual file rather than only set.toml", () => {
    // The whole point of carrying each source path: editing one fragment
    // should not mean opening the set and hunting for it.
    const onOpen = vi.fn();
    render(
      <Sets
        entries={entries}
        onToggle={vi.fn()}
        onOpen={onOpen}
        onEditPackage={vi.fn()}
      />,
    );
    fireEvent.click(screen.getByRole("button", { expanded: false, name: /core/i }));
    fireEvent.click(screen.getByRole("button", { name: "10-a.zsh" }));
    expect(onOpen).toHaveBeenCalledWith("sets/core/shell/10-a.zsh");
  });
});

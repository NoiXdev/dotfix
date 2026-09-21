import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { Settings as SettingsValue } from "../types";
import Settings from "./Settings";

const value: SettingsValue = {
  machine: "macbook-pro",
  repo: "/Users/test/dotfiles",
  remote: "git@github.com:you/dotfiles.git",
  secret_provider: "keychain",
  vault: null,
  ignored: ["caddy"],
};

describe("Settings", () => {
  afterEach(() => cleanup());

  it("separates what is local from what reaches the other machines", () => {
    // The provider is written into the repository and travels; the remote and
    // the name do not. A flat list would hide the one difference that decides
    // whether something needs committing.
    render(
      <Settings
        value={value}
        onSetRemote={vi.fn()}
        onSetProvider={vi.fn()}
        onRename={vi.fn()}
        onUnignore={vi.fn()}
      />,
    );
    expect(screen.getByText(/this machine only/i)).toBeInTheDocument();
    expect(screen.getByText(/shared with your other machines/i)).toBeInTheDocument();
  });

  it("does not offer to save a value that has not changed", () => {
    render(
      <Settings
        value={value}
        onSetRemote={vi.fn()}
        onSetProvider={vi.fn()}
        onRename={vi.fn()}
        onUnignore={vi.fn()}
      />,
    );
    expect(screen.getByRole("button", { name: /set remote/i })).toBeDisabled();
    expect(screen.getByRole("button", { name: /rename/i })).toBeDisabled();
    expect(screen.getByRole("button", { name: /save provider/i })).toBeDisabled();
  });

  it("reports a changed remote", () => {
    const onSetRemote = vi.fn();
    render(
      <Settings
        value={value}
        onSetRemote={onSetRemote}
        onSetProvider={vi.fn()}
        onRename={vi.fn()}
        onUnignore={vi.fn()}
      />,
    );
    fireEvent.change(screen.getByLabelText(/remote/i), {
      target: { value: "git@github.com:you/other.git" },
    });
    fireEvent.click(screen.getByRole("button", { name: /set remote/i }));
    expect(onSetRemote).toHaveBeenCalledWith("git@github.com:you/other.git");
  });

  it("asks for a vault once 1Password is chosen", () => {
    const onSetProvider = vi.fn();
    render(
      <Settings
        value={value}
        onSetRemote={vi.fn()}
        onSetProvider={onSetProvider}
        onRename={vi.fn()}
        onUnignore={vi.fn()}
      />,
    );
    expect(screen.queryByLabelText(/vault/i)).toBeNull();

    fireEvent.click(screen.getByRole("combobox", { name: /store secrets in/i }));
    fireEvent.click(screen.getByRole("option", { name: "1Password" }));

    fireEvent.change(screen.getByLabelText(/vault/i), {
      target: { value: "Private" },
    });
    fireEvent.click(screen.getByRole("button", { name: /save provider/i }));
    expect(onSetProvider).toHaveBeenCalledWith("1password", "Private");
  });

  it("lists what is ignored and offers to undo it", () => {
    // Ignoring used to remove a package from every view dotfix has, with the
    // machine file as the only record. A decision you cannot see is one you
    // cannot undo.
    const onUnignore = vi.fn();
    render(
      <Settings
        value={value}
        onSetRemote={vi.fn()}
        onSetProvider={vi.fn()}
        onRename={vi.fn()}
        onUnignore={onUnignore}
      />,
    );

    expect(screen.getByText("caddy")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /stop ignoring caddy/i }));
    expect(onUnignore).toHaveBeenCalledWith("caddy");
  });

  it("says plainly when nothing is ignored", () => {
    render(
      <Settings
        value={{ ...value, ignored: [] }}
        onSetRemote={vi.fn()}
        onSetProvider={vi.fn()}
        onRename={vi.fn()}
        onUnignore={vi.fn()}
      />,
    );
    expect(screen.getByText(/none\./i)).toBeInTheDocument();
  });
});

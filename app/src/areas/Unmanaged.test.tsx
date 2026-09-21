import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { Item, SetEntry } from "../types";
import Unmanaged from "./Unmanaged";

const sets: SetEntry[] = [
  { name: "core", active: true, description: "Base", brew: [], cask: [], files: [], fragments: [], config_path: "/repo/sets/x/set.toml" },
  { name: "web", active: true, description: "Web", brew: [], cask: [], files: [], fragments: [], config_path: "/repo/sets/x/set.toml" },
];

const item: Item = {
  id: "unmanaged:formula:jq",
  label: "jq",
  area: "unmanaged",
  action: "adopt",
  detail: "in no set",
  actionable: true,
};

// `view.rs` puts this in the Unmanaged area too, but it is a different
// decision: the only proposal the backend has for it is "drop it from its
// set", which is why its `action` is "drop" and not "adopt".
const locallyRemoved: Item = {
  id: "locally_removed:formula:alpha",
  label: "alpha",
  area: "unmanaged",
  action: "drop",
  detail: "removed on this machine",
  actionable: true,
};

describe("Unmanaged", () => {
  // This project doesn't run testing-library's global auto-cleanup (see
  // App.test.tsx); each `it` renders its own tree, so unmount between tests.
  afterEach(() => cleanup());

  it("offers every set as an adoption target", () => {
    render(<Unmanaged undeclared={[]} onDeclare={vi.fn()} items={[item]} sets={sets} onAdopt={vi.fn()} />);
    const field = screen.getByRole("combobox", { name: /set for jq/i });
    expect(field).toHaveDisplayValue("core");
    // The options exist only while the menu is open — clicking the field is
    // what a person does, and it is what makes them appear.
    fireEvent.click(field);
    expect(screen.getByRole("option", { name: "web" })).toBeInTheDocument();
  });

  it("adopts into the chosen set", () => {
    const onAdopt = vi.fn();
    render(<Unmanaged items={[item]} sets={sets} undeclared={[]} onDeclare={vi.fn()} onAdopt={onAdopt} />);
    fireEvent.click(screen.getByRole("combobox", { name: /set for jq/i }));
    fireEvent.click(screen.getByRole("option", { name: "web" }));
    fireEvent.click(screen.getByRole("button", { name: /^adopt$/i }));
    expect(onAdopt).toHaveBeenCalledWith("unmanaged:formula:jq", "web", false);
  });

  it("filters the sets as you type, which is the point of the search field", () => {
    render(<Unmanaged undeclared={[]} onDeclare={vi.fn()} items={[item]} sets={sets} onAdopt={vi.fn()} />);
    const field = screen.getByRole("combobox", { name: /set for jq/i });
    fireEvent.click(field);
    fireEvent.change(field, { target: { value: "we" } });
    expect(screen.getByRole("option", { name: "web" })).toBeInTheDocument();
    expect(screen.queryByRole("option", { name: "core" })).toBeNull();
  });

  it("says so rather than showing an empty menu when nothing matches", () => {
    render(<Unmanaged undeclared={[]} onDeclare={vi.fn()} items={[item]} sets={sets} onAdopt={vi.fn()} />);
    const field = screen.getByRole("combobox", { name: /set for jq/i });
    fireEvent.click(field);
    fireEvent.change(field, { target: { value: "zzz" } });
    expect(screen.queryAllByRole("option")).toHaveLength(0);
    expect(screen.getByText(/no match/i)).toBeInTheDocument();
  });

  it("ignoring passes the ignore flag and no set", () => {
    const onAdopt = vi.fn();
    render(<Unmanaged items={[item]} sets={sets} undeclared={[]} onDeclare={vi.fn()} onAdopt={onAdopt} />);
    fireEvent.click(screen.getByRole("button", { name: /ignore/i }));
    expect(onAdopt).toHaveBeenCalledWith("unmanaged:formula:jq", null, true);
  });

  it("offers dropping a locally removed package from its set", () => {
    const onAdopt = vi.fn();
    render(
      <Unmanaged items={[locallyRemoved]} sets={sets} undeclared={[]} onDeclare={vi.fn()} onAdopt={onAdopt} />,
    );
    fireEvent.click(screen.getByRole("button", { name: /drop from set/i }));
    expect(onAdopt).toHaveBeenCalledWith(
      "locally_removed:formula:alpha",
      null,
      false,
    );
  });

  it("offers no Ignore for a locally removed package, because ignoring one does nothing", () => {
    // `MachineConfig.ignore` is only consulted for packages that are
    // installed but in no set, so ignoring a locally-removed package would
    // write the machine file and leave the row exactly where it was. The
    // backend now refuses it outright; the button must not be there to
    // click in the first place.
    render(
      <Unmanaged undeclared={[]} onDeclare={vi.fn()} items={[locallyRemoved]} sets={sets} onAdopt={vi.fn()} />,
    );
    expect(
      screen.queryByRole("button", { name: /ignore/i }),
    ).not.toBeInTheDocument();
    // Nor a set picker: the set is the one the package is already in.
    expect(screen.queryByRole("combobox")).not.toBeInTheDocument();
  });

  it("is calm when there is nothing unmanaged", () => {
    render(<Unmanaged undeclared={[]} onDeclare={vi.fn()} items={[]} sets={sets} onAdopt={vi.fn()} />);
    expect(screen.getByText(/nothing unmanaged/i)).toBeInTheDocument();
  });

  it("offers to record software that is here but in no set", () => {
    // The mirror of an unmanaged package. Detection only ran at setup, so
    // anything installed since had no way into the repository.
    const onDeclare = vi.fn();
    render(
      <Unmanaged
        items={[]}
        sets={sets}
        undeclared={[{ name: "pyenv", hint: "curl -fsSL https://pyenv.run | bash" }]}
        onDeclare={onDeclare}
        onAdopt={vi.fn()}
      />,
    );

    expect(screen.getByText("pyenv")).toBeInTheDocument();
    expect(screen.getByText(/not install it/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /declare/i }));
    expect(onDeclare).toHaveBeenCalledWith("pyenv", "core");
  });

  it("says nothing about undeclared software when there is none", () => {
    render(
      <Unmanaged
        items={[]}
        sets={sets}
        undeclared={[]}
        onDeclare={vi.fn()}
        onAdopt={vi.fn()}
      />,
    );
    expect(screen.queryByText(/installed here, in no set/i)).toBeNull();
  });
});

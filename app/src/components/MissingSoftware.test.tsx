import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import MissingSoftware from "./MissingSoftware";

describe("MissingSoftware", () => {
  afterEach(() => cleanup());

  it("shows nothing at all when nothing is missing", () => {
    // The common state. A heading that is always there, saying "none", would
    // train the eye to skip the one time it says something.
    const { container } = render(<MissingSoftware items={[]} />);
    expect(container).toBeEmptyDOMElement();
  });

  it("names the set that asked, and the command to run", () => {
    render(
      <MissingSoftware
        items={[
          {
            name: "oh-my-zsh",
            set: "core",
            hint: "sh -c \"$(curl -fsSL https://example.com/install.sh)\"",
          },
        ]}
      />,
    );
    expect(screen.getByText("oh-my-zsh")).toBeInTheDocument();
    expect(screen.getByText(/needed by set `core`/)).toBeInTheDocument();
    expect(screen.getByText(/curl -fsSL/)).toBeInTheDocument();
  });

  it("copes with a requirement that carries no hint", () => {
    render(
      <MissingSoftware items={[{ name: "pyenv", set: "web", hint: null }]} />,
    );
    expect(screen.getByText("pyenv")).toBeInTheDocument();
  });
});

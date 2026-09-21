import { render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { cleanup } from "@testing-library/react";

import PreflightList from "./PreflightList";

afterEach(cleanup);

describe("PreflightList", () => {
  it("marks failing checks for screen readers, not only by colour", () => {
    render(
      <PreflightList
        checks={[
          { name: "git", ok: true, detail: "git version 2.51.0", blocking: true },
          { name: "homebrew", ok: false, detail: "not found", blocking: true },
        ]}
      />,
    );
    expect(screen.getByText("homebrew").closest("li")).toHaveAttribute(
      "data-ok",
      "false",
    );
    expect(screen.getByText(/not found/)).toBeInTheDocument();
  });

  it("shows each check's detail so a failure explains itself", () => {
    render(
      <PreflightList
        checks={[
          {
            name: "github cli",
            ok: false,
            detail: "not logged in",
            blocking: false,
          },
        ]}
      />,
    );
    expect(screen.getByText(/not logged in/)).toBeInTheDocument();
    // Non-blocking: reported, but it does not stop setup, and the
    // screen-reader text must not claim otherwise.
    expect(screen.getByText("github cli").closest("li")).toHaveAttribute(
      "data-blocking",
      "false",
    );
    expect(screen.getByText("optional")).toBeInTheDocument();
  });
});

import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import About from "./About";

const readAbout = vi.fn();
const openLink = vi.fn();
const checkUpdate = vi.fn();

vi.mock("../api", () => ({
  readAbout: () => readAbout(),
  openLink: (url: string) => openLink(url),
  checkUpdate: () => checkUpdate(),
}));

const current = { state: "current", version: null, url: null, reason: null };

const data = {
  version: "1.2.3",
  links: [
    { label: "Project page", url: "https://www.noix.dev/projekte/kontorfix/dotfix" },
    { label: "Documentation", url: "https://docs.noix.dev/dotfix" },
  ],
};

describe("About", () => {
  // Not automatic in this project: without it, the previous test's tree is
  // still mounted and every query finds two of everything.
  afterEach(cleanup);

  beforeEach(() => {
    readAbout.mockReset().mockResolvedValue(data);
    openLink.mockReset().mockResolvedValue(undefined);
    checkUpdate.mockReset().mockResolvedValue(current);
  });

  it("shows the version and both links", async () => {
    render(<About />);
    expect(await screen.findByText(/version 1\.2\.3/i)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Project page" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Documentation" })).toBeInTheDocument();
  });

  it("opens a link through the backend rather than navigating the window", async () => {
    render(<About />);
    fireEvent.click(await screen.findByRole("button", { name: "Documentation" }));
    expect(openLink).toHaveBeenCalledWith("https://docs.noix.dev/dotfix");
  });

  it("says so when the backend refuses instead of failing silently", async () => {
    openLink.mockRejectedValue(new Error("`https://evil.example` is not one of dotfix's own links"));
    render(<About />);
    fireEvent.click(await screen.findByRole("button", { name: "Project page" }));
    await waitFor(() =>
      expect(screen.getByRole("alert")).toHaveTextContent(/not one of dotfix's own links/i),
    );
  });

  it("offers a newer release and opens its page", async () => {
    checkUpdate.mockResolvedValue({
      state: "newer",
      version: "1.0.1",
      url: "https://github.com/NoiXdev/dotfix/releases/tag/v1.0.1",
      reason: null,
    });
    render(<About />);
    const offer = await screen.findByRole("button", {
      name: /1\.0\.1 is available/i,
    });
    fireEvent.click(offer);
    expect(openLink).toHaveBeenCalledWith(
      "https://github.com/NoiXdev/dotfix/releases/tag/v1.0.1",
    );
  });

  it("says nothing alarming when GitHub cannot be reached", async () => {
    // Being offline is not a problem with the installation, so it must not
    // produce an error banner — the version stays on screen either way.
    checkUpdate.mockResolvedValue({
      state: "unknown",
      version: null,
      url: null,
      reason: "Could not resolve host",
    });
    render(<About />);
    expect(await screen.findByText(/could not check/i)).toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(screen.getByText(/version 1\.2\.3/i)).toBeInTheDocument();
  });

  it("confirms when there is nothing newer", async () => {
    render(<About />);
    expect(await screen.findByText(/newest release/i)).toBeInTheDocument();
  });
});

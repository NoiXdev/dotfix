import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import DeployKey from "./DeployKey";

afterEach(cleanup);

const key = { public: "ssh-ed25519 AAAA... dotfix@box-one", host_alias: "github.com-dotfix" };

describe("DeployKey", () => {
  it("shows the public key and never asks for the private one", () => {
    render(<DeployKey deployKey={key} onTest={vi.fn()} />);
    expect(screen.getByText(/ssh-ed25519 AAAA/)).toBeInTheDocument();
    expect(screen.queryByLabelText(/private key/i)).toBeNull();
  });

  it("spells out that write access must be granted", () => {
    render(<DeployKey deployKey={key} onTest={vi.fn()} />);
    expect(screen.getByText(/allow write access/i)).toBeInTheDocument();
  });

  it("offers a connection test", () => {
    const onTest = vi.fn();
    render(<DeployKey deployKey={key} onTest={onTest} />);
    fireEvent.click(screen.getByRole("button", { name: /test connection/i }));
    expect(onTest).toHaveBeenCalled();
  });

  it("builds a link to add the key on the repository this machine is cloning", () => {
    render(
      <DeployKey
        deployKey={key}
        onTest={vi.fn()}
        repoUrl="git@github.com:example/dotfiles.git"
      />,
    );
    expect(screen.getByRole("link", { name: /add deploy key/i })).toHaveAttribute(
      "href",
      "https://github.com/example/dotfiles/settings/keys/new",
    );
  });

  it("offers no add-key link when the repository url cannot be parsed", () => {
    render(<DeployKey deployKey={key} onTest={vi.fn()} />);
    expect(screen.queryByRole("link")).toBeNull();
  });

  it("copies the public key to the clipboard", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.assign(navigator, { clipboard: { writeText } });
    render(<DeployKey deployKey={key} onTest={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: /copy/i }));
    await vi.waitFor(() => expect(writeText).toHaveBeenCalledWith(key.public));
  });

  it("the token field is a password field", () => {
    render(<DeployKey deployKey={key} onTest={vi.fn()} onSubmitToken={vi.fn()} />);
    fireEvent.click(
      screen.getByRole("button", { name: /use a personal access token/i }),
    );
    expect(screen.getByLabelText(/personal access token/i)).toHaveAttribute(
      "type",
      "password",
    );
  });

  it("submits the token then clears it, never showing it again", () => {
    const onSubmitToken = vi.fn();
    render(<DeployKey deployKey={key} onTest={vi.fn()} onSubmitToken={onSubmitToken} />);
    fireEvent.click(
      screen.getByRole("button", { name: /use a personal access token/i }),
    );

    fireEvent.change(screen.getByLabelText(/github username/i), {
      target: { value: "octocat" },
    });
    const tokenField = screen.getByLabelText(/personal access token/i);
    fireEvent.change(tokenField, { target: { value: "super-secret-token" } });
    fireEvent.click(screen.getByRole("button", { name: /save token/i }));

    expect(onSubmitToken).toHaveBeenCalledWith("octocat", "super-secret-token");
    expect(screen.getByLabelText(/personal access token/i)).toHaveValue("");
    expect(screen.queryByDisplayValue("super-secret-token")).toBeNull();
  });
});

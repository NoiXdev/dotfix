import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => invoke(...a) }));

import Wizard from "./Wizard";

afterEach(() => {
  cleanup();
  invoke.mockReset();
});

const passingPreflight = {
  checks: [{ name: "git", ok: true, detail: "2.51.0", blocking: true }],
};

describe("Wizard", () => {
  it("will not run setup while pre-flight is failing", async () => {
    invoke.mockResolvedValue({
      checks: [
        { name: "homebrew", ok: false, detail: "not found", blocking: true },
      ],
    });
    render(<Wizard onDone={vi.fn()} />);

    fireEvent.change(screen.getByLabelText(/machine name/i), {
      target: { value: "box-one" },
    });
    fireEvent.click(screen.getByRole("button", { name: /check/i }));

    await screen.findByText(/not found/);
    expect(screen.getByRole("button", { name: /set up/i })).toBeDisabled();
  });

  it("still allows setup when only a non-blocking check failed", async () => {
    // `gh` is reported but deliberately does not block: it only decides
    // whether dotfix can offer to create the remote. A stock Mac has no
    // `gh`, so gating on "every check passed" would make setup impossible.
    invoke.mockResolvedValue({
      checks: [
        { name: "git", ok: true, detail: "2.51.0", blocking: true },
        {
          name: "github cli",
          ok: false,
          detail: "not installed or not logged in",
          blocking: false,
        },
      ],
    });
    render(<Wizard onDone={vi.fn()} />);

    fireEvent.change(screen.getByLabelText(/machine name/i), {
      target: { value: "box-one" },
    });
    fireEvent.click(screen.getByRole("button", { name: /check requirements/i }));

    await screen.findByText(/not installed or not logged in/);
    expect(screen.getByRole("button", { name: /set up/i })).not.toBeDisabled();
  });

  it("marks exactly the failed step, leaves earlier steps done, and retries by resuming", async () => {
    // `init_run` resolves — it never rejects for a step-level failure once
    // stepping has started — naming `install_agent` as the one that failed
    // after `create_or_clone` and `configure_machine` already succeeded.
    invoke.mockImplementation((cmd: string) => {
      if (cmd === "init_preflight") return Promise.resolve(passingPreflight);
      if (cmd === "init_run")
        return Promise.resolve({
          completed: ["create_or_clone", "configure_machine"],
          repo: "/Users/test/dotfiles",
          failed: {
            step: "install_agent",
            message: "permission denied installing the LaunchAgent",
          },
        });
      return Promise.resolve({});
    });
    render(<Wizard onDone={vi.fn()} />);

    fireEvent.change(screen.getByLabelText(/machine name/i), {
      target: { value: "box-one" },
    });
    fireEvent.click(screen.getByRole("button", { name: /check/i }));
    await screen.findByText(/2\.51\.0/);
    fireEvent.click(screen.getByRole("button", { name: /set up/i }));

    await screen.findByText(/permission denied installing the LaunchAgent/);

    expect(
      screen.getByText("Create the dotfiles repository").closest("li"),
    ).toHaveAttribute("data-status", "done");
    expect(
      screen.getByText("Register this machine").closest("li"),
    ).toHaveAttribute("data-status", "done");
    expect(
      screen.getByText("Install the background sync agent").closest("li"),
    ).toHaveAttribute("data-status", "failed");

    fireEvent.click(screen.getByRole("button", { name: /retry/i }));

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith(
        "init_run",
        expect.objectContaining({
          completed: ["create_or_clone", "configure_machine"],
        }),
      ),
    );
  });

  it("lets the user finish when only the background agent could not be installed", async () => {
    // No `dotfix` CLI on this Mac, so no agent was installed rather than one
    // pointing at the app, which could never run. Everything dotfix needs to
    // work is already on disk — the wizard must not be a dead end.
    const onDone = vi.fn();
    invoke.mockImplementation((cmd: string) => {
      if (cmd === "init_preflight") return Promise.resolve(passingPreflight);
      if (cmd === "init_run")
        return Promise.resolve({
          completed: ["create_or_clone", "configure_machine"],
          repo: "/Users/test/dotfiles",
          failed: {
            step: "install_agent",
            message: "the `dotfix` command-line tool is not installed",
          },
        });
      return Promise.resolve({});
    });
    render(<Wizard onDone={onDone} />);

    fireEvent.change(screen.getByLabelText(/machine name/i), {
      target: { value: "box-one" },
    });
    fireEvent.click(screen.getByRole("button", { name: /check requirements/i }));
    await screen.findByText(/2\.51\.0/);
    fireEvent.click(screen.getByRole("button", { name: /set up/i }));

    const finish = await screen.findByRole("button", {
      name: /continue without the background check/i,
    });
    fireEvent.click(finish);
    expect(onDone).toHaveBeenCalled();
  });

  it("says so when it took over an existing shell configuration", async () => {
    // The wizard otherwise closes without a word about a file it now owns —
    // and silence there is how a real .zshrc came to be replaced by a stub.
    const onDone = vi.fn();
    invoke.mockImplementation((cmd: string) => {
      if (cmd === "init_preflight") return Promise.resolve(passingPreflight);
      if (cmd === "init_run")
        return Promise.resolve({
          completed: ["create_or_clone", "configure_machine", "install_agent"],
          repo: "/Users/test/dotfiles",
          failed: null,
          imported_zshrc: true,
        });
      return Promise.resolve({});
    });
    render(<Wizard onDone={onDone} />);

    fireEvent.change(screen.getByLabelText(/machine name/i), {
      target: { value: "box-one" },
    });
    fireEvent.click(screen.getByRole("button", { name: /check requirements/i }));
    await screen.findByText(/2\.51\.0/);
    fireEvent.click(screen.getByRole("button", { name: /set up/i }));

    await screen.findByText(/00-imported\.zsh/);
    expect(onDone).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: /continue/i }));
    expect(onDone).toHaveBeenCalled();
  });

  it("closes straight away when there was nothing to import", async () => {
    const onDone = vi.fn();
    invoke.mockImplementation((cmd: string) => {
      if (cmd === "init_preflight") return Promise.resolve(passingPreflight);
      if (cmd === "init_run")
        return Promise.resolve({
          completed: ["create_or_clone", "configure_machine", "install_agent"],
          repo: "/Users/test/dotfiles",
          failed: null,
          imported_zshrc: false,
        });
      return Promise.resolve({});
    });
    render(<Wizard onDone={onDone} />);

    fireEvent.change(screen.getByLabelText(/machine name/i), {
      target: { value: "box-one" },
    });
    fireEvent.click(screen.getByRole("button", { name: /check requirements/i }));
    await screen.findByText(/2\.51\.0/);
    fireEvent.click(screen.getByRole("button", { name: /set up/i }));

    await waitFor(() => expect(onDone).toHaveBeenCalled());
  });

  it("surfaces a plan Rust rejected outright as a banner, not a step failure", async () => {
    // Only reachable before any step runs — an unusable plan or an
    // unreadable `$HOME` — so there is no step line to attach it to.
    invoke.mockImplementation((cmd: string) => {
      if (cmd === "init_preflight") return Promise.resolve(passingPreflight);
      if (cmd === "init_run") return Promise.reject("HOME is not set");
      return Promise.resolve({});
    });
    render(<Wizard onDone={vi.fn()} />);

    fireEvent.change(screen.getByLabelText(/machine name/i), {
      target: { value: "box-one" },
    });
    fireEvent.click(screen.getByRole("button", { name: /check/i }));
    await screen.findByText(/2\.51\.0/);
    fireEvent.click(screen.getByRole("button", { name: /set up/i }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      /HOME is not set/,
    );
  });

  it("tells the parent when setup completed", async () => {
    const onDone = vi.fn();
    invoke.mockImplementation((cmd: string) => {
      if (cmd === "init_preflight") return Promise.resolve(passingPreflight);
      if (cmd === "init_run")
        return Promise.resolve({
          completed: ["create_or_clone", "configure_machine", "install_agent"],
          repo: "/Users/test/dotfiles",
          failed: null,
        });
      return Promise.resolve({});
    });
    render(<Wizard onDone={onDone} />);

    fireEvent.change(screen.getByLabelText(/machine name/i), {
      target: { value: "box-one" },
    });
    fireEvent.click(screen.getByRole("button", { name: /check/i }));
    await screen.findByText(/2\.51\.0/);
    fireEvent.click(screen.getByRole("button", { name: /set up/i }));

    await waitFor(() => expect(onDone).toHaveBeenCalled());
  });

  const cloneTarget = (
    url = "git@github.com:example/dotfiles.git",
    host_to_pin: string | null = "github.com",
  ) => ({ url, host_to_pin });

  /**
   * Pick an option from a `Select`. Clicking the field opens the menu — the
   * input is the combobox — and the option is then a real `option` role, so
   * this drives the component the way a person does rather than poking a
   * value into it.
   */
  function chooseOption(field: RegExp, option: RegExp) {
    fireEvent.click(screen.getByRole("combobox", { name: field }));
    fireEvent.click(screen.getByRole("option", { name: option }));
  }

  function startClone() {
    render(<Wizard onDone={vi.fn()} />);
    fireEvent.change(screen.getByLabelText(/machine name/i), {
      target: { value: "box-one" },
    });
    chooseOption(/set up this mac by/i, /cloning an existing/i);
    fireEvent.change(screen.getByLabelText(/repository url/i), {
      target: { value: "git@github.com:example/dotfiles.git" },
    });
    fireEvent.click(screen.getByRole("button", { name: /check requirements/i }));
  }

  it("skips straight past the auth screen when ssh already works", async () => {
    invoke.mockImplementation((cmd: string) => {
      if (cmd === "init_preflight") return Promise.resolve(passingPreflight);
      if (cmd === "init_probe_ssh") return Promise.resolve("ready");
      if (cmd === "init_clone_target") return Promise.resolve(cloneTarget());
      return Promise.resolve({});
    });
    startClone();
    await screen.findByText(/2\.51\.0/);

    fireEvent.click(screen.getByRole("button", { name: /check connection/i }));

    await waitFor(() =>
      expect(screen.queryByText(/checking the connection/i)).toBeNull(),
    );
    expect(
      screen.queryByRole("button", { name: /test connection/i }),
    ).toBeNull();
    expect(screen.getByRole("button", { name: /set up/i })).not.toBeDisabled();
  });

  it("offers the generated deploy key when ssh needs one", async () => {
    invoke.mockImplementation((cmd: string) => {
      if (cmd === "init_preflight") return Promise.resolve(passingPreflight);
      if (cmd === "init_probe_ssh") return Promise.resolve("needs_key");
      if (cmd === "init_clone_target") return Promise.resolve(cloneTarget());
      if (cmd === "init_deploy_key")
        return Promise.resolve({
          public: "ssh-ed25519 AAAA... dotfix@box-one",
          host_alias: "github.com-dotfix",
        });
      return Promise.resolve({});
    });
    startClone();
    await screen.findByText(/2\.51\.0/);

    expect(screen.getByRole("button", { name: /set up/i })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: /check connection/i }));

    await screen.findByText(/ssh-ed25519 AAAA/);
    expect(
      screen.getByRole("link", { name: /add deploy key/i }),
    ).toHaveAttribute(
      "href",
      "https://github.com/example/dotfiles/settings/keys/new",
    );
    expect(screen.getByRole("button", { name: /set up/i })).toBeDisabled();
  });

  it("shows the unreachable detail with a way to check again", async () => {
    invoke.mockImplementation((cmd: string) => {
      if (cmd === "init_preflight") return Promise.resolve(passingPreflight);
      if (cmd === "init_clone_target") return Promise.resolve(cloneTarget());
      if (cmd === "init_probe_ssh")
        return Promise.resolve("unreachable: connection timed out");
      return Promise.resolve({});
    });
    startClone();
    await screen.findByText(/2\.51\.0/);

    fireEvent.click(screen.getByRole("button", { name: /check connection/i }));

    await screen.findByText(/connection timed out/);
    expect(screen.getByRole("button", { name: /set up/i })).toBeDisabled();
    expect(screen.getByRole("button", { name: /retry/i })).toBeInTheDocument();
  });

  it("stores a token, unlocks setup, and never sends it a second time", async () => {
    invoke.mockImplementation((cmd: string, args?: Record<string, unknown>) => {
      if (cmd === "init_preflight") return Promise.resolve(passingPreflight);
      if (cmd === "init_probe_ssh") return Promise.resolve("needs_key");
      if (cmd === "init_clone_target")
        return Promise.resolve(
          args?.auth === "token"
            ? cloneTarget("https://github.com/example/dotfiles.git", null)
            : cloneTarget(),
        );
      if (cmd === "init_deploy_key")
        return Promise.resolve({
          public: "ssh-ed25519 AAAA... dotfix@box-one",
          host_alias: "github.com-dotfix",
        });
      if (cmd === "init_store_token") return Promise.resolve(undefined);
      return Promise.resolve({});
    });
    startClone();
    await screen.findByText(/2\.51\.0/);
    fireEvent.click(screen.getByRole("button", { name: /check connection/i }));
    await screen.findByText(/ssh-ed25519 AAAA/);

    fireEvent.click(
      screen.getByRole("button", { name: /use a personal access token/i }),
    );
    fireEvent.change(screen.getByLabelText(/github username/i), {
      target: { value: "octocat" },
    });
    fireEvent.change(screen.getByLabelText(/personal access token/i), {
      target: { value: "super-secret-token" },
    });
    fireEvent.click(screen.getByRole("button", { name: /save token/i }));

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("init_store_token", {
        user: "octocat",
        token: "super-secret-token",
      }),
    );
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: /set up/i }),
      ).not.toBeDisabled(),
    );
    // The deploy-key screen is gone now that auth is settled, and the token
    // input went with it — nothing is left around to echo the value back.
    expect(screen.queryByLabelText(/personal access token/i)).toBeNull();
    expect(
      invoke.mock.calls.filter(([cmd]) => cmd === "init_store_token"),
    ).toHaveLength(1);
  });

  // --- the effective clone url (final review, C3/I2) ---

  it("probes the alias the deploy key is scoped to, not github.com", async () => {
    // The generated stanza has `IdentitiesOnly yes`, so `ssh -T git@github.com`
    // never offers the new key and "Test connection" could never succeed.
    invoke.mockImplementation((cmd: string) => {
      if (cmd === "init_preflight") return Promise.resolve(passingPreflight);
      if (cmd === "init_probe_ssh") return Promise.resolve("needs_key");
      if (cmd === "init_clone_target") return Promise.resolve(cloneTarget());
      if (cmd === "init_deploy_key")
        return Promise.resolve({
          public: "ssh-ed25519 AAAA... dotfix@box-one",
          host_alias: "github.com-dotfix",
        });
      return Promise.resolve({});
    });
    startClone();
    await screen.findByText(/2\.51\.0/);

    fireEvent.click(screen.getByRole("button", { name: /check connection/i }));
    await screen.findByText(/ssh-ed25519 AAAA/);
    expect(invoke).toHaveBeenCalledWith("init_probe_ssh", {
      host: "github.com",
    });

    fireEvent.click(screen.getByRole("button", { name: /test connection/i }));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("init_probe_ssh", {
        host: "github.com-dotfix",
      }),
    );
  });

  it("shows the url it will really clone when it differs from the typed one", async () => {
    invoke.mockImplementation((cmd: string) => {
      if (cmd === "init_preflight") return Promise.resolve(passingPreflight);
      if (cmd === "init_probe_ssh") return Promise.resolve("ready");
      if (cmd === "init_clone_target")
        return Promise.resolve(
          cloneTarget("git@github.com-dotfix:example/dotfiles.git"),
        );
      return Promise.resolve({});
    });
    startClone();
    await screen.findByText(/2\.51\.0/);
    fireEvent.click(screen.getByRole("button", { name: /check connection/i }));

    await screen.findByText(/git@github\.com-dotfix:example\/dotfiles\.git/);
  });

  it("passes the authentication it settled on to init_run", async () => {
    invoke.mockImplementation((cmd: string) => {
      if (cmd === "init_preflight") return Promise.resolve(passingPreflight);
      if (cmd === "init_probe_ssh") return Promise.resolve("ready");
      if (cmd === "init_clone_target") return Promise.resolve(cloneTarget());
      if (cmd === "init_run")
        return Promise.resolve({
          completed: [
            "ensure_host_known",
            "create_or_clone",
            "configure_machine",
            "install_agent",
          ],
          repo: "/Users/test/dotfiles",
          failed: null,
        });
      return Promise.resolve({});
    });
    startClone();
    await screen.findByText(/2\.51\.0/);
    fireEvent.click(screen.getByRole("button", { name: /check connection/i }));
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: /set up/i }),
      ).not.toBeDisabled(),
    );
    fireEvent.click(screen.getByRole("button", { name: /set up/i }));

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith(
        "init_run",
        expect.objectContaining({ auth: "ssh" }),
      ),
    );
  });

  it("does not list a host-key step for a host it will never pin", async () => {
    // `init_run` only pins GitHub's published fingerprints, so a GitLab URL
    // left a step that could only ever stay pending.
    invoke.mockImplementation((cmd: string) => {
      if (cmd === "init_preflight") return Promise.resolve(passingPreflight);
      if (cmd === "init_probe_ssh") return Promise.resolve("ready");
      if (cmd === "init_clone_target")
        return Promise.resolve(
          cloneTarget("git@gitlab.com:example/dotfiles.git", null),
        );
      return Promise.resolve({});
    });
    render(<Wizard onDone={vi.fn()} />);
    fireEvent.change(screen.getByLabelText(/machine name/i), {
      target: { value: "box-one" },
    });
    chooseOption(/set up this mac by/i, /cloning an existing/i);
    fireEvent.change(screen.getByLabelText(/repository url/i), {
      target: { value: "git@gitlab.com:example/dotfiles.git" },
    });
    fireEvent.click(screen.getByRole("button", { name: /check requirements/i }));
    await screen.findByText(/2\.51\.0/);

    expect(screen.queryByText(/verify the github host key/i)).toBeNull();
    expect(
      screen.getByText(/clone the dotfiles repository/i),
    ).toBeInTheDocument();
  });
});

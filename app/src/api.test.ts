import { afterEach, describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => invoke(...a) }));

import {
  applyItems,
  getOverview,
  initCloneTarget,
  initDeployKey,
  initPreflight,
  initProbeSsh,
  initRun,
  initStoreToken,
  overwriteFiles,
  refresh,
  toggleSet,
} from "./api";

describe("api", () => {
  // Reset after each test rather than before: resetting a vi.mock-referenced
  // vi.fn() from inside a beforeEach hook trips a Vitest bug (reproduced in
  // isolation, independent of this codebase, on Vitest 3.2.7 and 5.0.1) that
  // misattributes a phantom "unhandled rejection" failure to a later test's
  // correctly-awaited rejection. afterEach resets on exactly the same test
  // boundary without hitting it.
  afterEach(() => invoke.mockReset());

  it("calls the overview command with no arguments", async () => {
    invoke.mockResolvedValue({ counts: {}, items: [], status_line: null });
    await getOverview();
    expect(invoke).toHaveBeenCalledWith("overview");
  });

  it("calls the refresh command with no arguments", async () => {
    // `overview` reads the machine; `refresh` pulls the repository first.
    // The window's "Check now" is the only caller of the latter.
    invoke.mockResolvedValue({ counts: {}, items: [], status_line: null });
    await refresh();
    expect(invoke).toHaveBeenCalledWith("refresh");
  });

  it("passes selected ids through to apply_items", async () => {
    invoke.mockResolvedValue({ counts: {}, items: [], status_line: null });
    await applyItems(["incoming_package:formula:jq"]);
    expect(invoke).toHaveBeenCalledWith("apply_items", {
      ids: ["incoming_package:formula:jq"],
    });
  });

  it("passes selected ids through to overwrite_files, not apply_items", async () => {
    invoke.mockResolvedValue({ counts: {}, items: [], status_line: null });
    await overwriteFiles(["local_edit:/Users/test/.gitconfig"]);
    expect(invoke).toHaveBeenCalledWith("overwrite_files", {
      ids: ["local_edit:/Users/test/.gitconfig"],
    });
  });

  it("sends the wizard's plan to init_preflight in Rust's snake_case shape", async () => {
    // `Wizard.test.tsx` only ever branches on the command name, so it would
    // pass identically if this emitted `secretProvider` instead of
    // `secret_provider` — a mismatch the mock cannot see, and one Rust
    // would only report as an "unknown secret provider ``" error at
    // runtime. Assert the exact payload so that mistake fails here instead.
    invoke.mockResolvedValue({ checks: [] });
    await initPreflight({
      machine: "box-one",
      mode: "clone",
      url: "git@github.com:example/dotfiles.git",
      secretProvider: "1password",
      vault: "Private",
    });
    expect(invoke).toHaveBeenCalledWith("init_preflight", {
      plan: {
        machine: "box-one",
        mode: "clone",
        url: "git@github.com:example/dotfiles.git",
        secret_provider: "1password",
        vault: "Private",
      },
    });
  });

  it("sends the wizard's plan and the already-completed steps to init_run", async () => {
    invoke.mockResolvedValue({ completed: [], repo: null, failed: null });
    await initRun(
      {
        machine: "box-one",
        mode: "new",
        url: "",
        secretProvider: "keychain",
        vault: "",
      },
      ["create_or_clone", "configure_machine"],
      "ssh",
    );
    expect(invoke).toHaveBeenCalledWith("init_run", {
      plan: {
        machine: "box-one",
        mode: "new",
        url: "",
        secret_provider: "keychain",
        vault: "",
      },
      completed: ["create_or_clone", "configure_machine"],
      auth: "ssh",
    });
  });

  it("probes the host it was given, not a hardcoded one", async () => {
    // A deploy key is scoped to its own alias with `IdentitiesOnly yes`, so
    // probing `github.com` after adding it would never offer that key.
    invoke.mockResolvedValue("ready");
    await initProbeSsh("github.com-dotfix");
    expect(invoke).toHaveBeenCalledWith("init_probe_ssh", {
      host: "github.com-dotfix",
    });
  });

  it("asks for the clone target under a given authentication method", async () => {
    invoke.mockResolvedValue({
      url: "https://github.com/example/dotfiles.git",
      host_to_pin: null,
    });
    await initCloneTarget("git@github.com:example/dotfiles.git", "token");
    expect(invoke).toHaveBeenCalledWith("init_clone_target", {
      url: "git@github.com:example/dotfiles.git",
      auth: "token",
    });
  });

  it("passes the machine name to init_deploy_key", async () => {
    invoke.mockResolvedValue({ public: "ssh-ed25519 AAAA...", host_alias: "github.com-dotfix" });
    await initDeployKey("box-one");
    expect(invoke).toHaveBeenCalledWith("init_deploy_key", { machine: "box-one" });
  });

  it("passes the username and token to init_store_token verbatim", async () => {
    // The token must reach the backend exactly as typed — no trimming, no
    // transformation on this side, since it is opaque to dotfix.
    invoke.mockResolvedValue(undefined);
    await initStoreToken("octocat", "super-secret-token");
    expect(invoke).toHaveBeenCalledWith("init_store_token", {
      user: "octocat",
      token: "super-secret-token",
    });
  });

  it("surfaces a backend error as a thrown Error", async () => {
    invoke.mockRejectedValue("repository has diverged from origin");
    await expect(toggleSet("web", true)).rejects.toThrow(
      "repository has diverged from origin",
    );
  });
});

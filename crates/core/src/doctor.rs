use serde::Serialize;

use crate::config::ProviderKind;
use crate::paths::Paths;
use crate::ports::{Exec, Fsys};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Check {
    pub name: &'static str,
    pub ok: bool,
    pub detail: String,
    /// Whether a failure here must stop whatever ran the check.
    ///
    /// Serialised deliberately: the wizard in the app gates its "Set up"
    /// button on this field. The rule lives here, on the value that crosses
    /// the boundary, so no front end has to re-derive it — one that did got
    /// it wrong and left setup impossible on a Mac without `gh`.
    pub blocking: bool,
}

impl Check {
    pub(crate) fn ok(name: &'static str, detail: impl Into<String>) -> Self {
        Self {
            name,
            ok: true,
            detail: detail.into(),
            blocking: true,
        }
    }

    pub(crate) fn fail(name: &'static str, detail: impl Into<String>) -> Self {
        Self {
            name,
            ok: false,
            detail: detail.into(),
            blocking: true,
        }
    }

    /// Mark a check as merely informational. Everything blocks by default —
    /// a check nobody has thought about should stop the run, not be ignored.
    pub(crate) fn non_blocking(mut self) -> Self {
        self.blocking = false;
        self
    }
}

/// Everything that fails *silently* if misconfigured, checked loudly.
pub fn run_checks(
    fs: &dyn Fsys,
    exec: &dyn Exec,
    paths: &Paths,
    provider: ProviderKind,
    vault: Option<&str>,
) -> Vec<Check> {
    let mut checks = Vec::new();

    checks.push(match exec.run("brew", &["--version"]) {
        Ok(v) => Check::ok("homebrew", v.lines().next().unwrap_or("").to_string()),
        Err(e) => Check::fail("homebrew", e.to_string()),
    });

    checks.push(match exec.run("git", &["--version"]) {
        Ok(v) => Check::ok("git", v.trim().to_string()),
        Err(e) => Check::fail("git", e.to_string()),
    });

    checks.push(if fs.exists(&paths.launch_agent()) {
        Check::ok("launch agent", paths.launch_agent().display().to_string())
    } else {
        Check::fail(
            "launch agent",
            "not installed — run `dotfix doctor --install-agent`",
        )
    });

    // A LaunchAgent inherits neither the interactive PATH nor, without this,
    // the SSH key the git remote needs. Both fail without any visible error.
    let ssh_config = paths.home.join(".ssh/config");
    checks.push(match fs.read(&ssh_config) {
        Ok(contents) if contents.contains("AddKeysToAgent") && contents.contains("UseKeychain") => {
            Check::ok("ssh key access", "AddKeysToAgent and UseKeychain are set")
        }
        Ok(_) => Check::fail(
            "ssh key access",
            "add `AddKeysToAgent yes` and `UseKeychain yes` to ~/.ssh/config, \
             otherwise the background check cannot reach the remote",
        ),
        Err(_) => Check::fail(
            "ssh key access",
            "no ~/.ssh/config — the background check may not reach the remote",
        ),
    });

    // The assembled shell configuration is a `.zshrc`, and a login shell that
    // is not zsh never reads it. Without this check dotfix would write the
    // file, report everything in sync, and change nothing about the shell the
    // user actually gets — working by appearance only.
    checks.push(match std::env::var("SHELL") {
        Ok(shell) if shell.ends_with("zsh") => Check::ok("login shell", shell),
        Ok(shell) => Check::fail(
            "login shell",
            format!(
                "{shell} — dotfix assembles a `.zshrc`, which this shell never reads. \
                 Manage its file with a `[[files]]` entry instead."
            ),
        )
        .non_blocking(),
        Err(_) => Check::ok("login shell", "not set — assuming zsh"),
    });

    checks.extend(provider_checks(fs, exec, paths, provider, vault));

    checks
}

/// The checks that depend on which secret provider a machine uses.
///
/// Split out of [`run_checks`] because `init::preflight` needs exactly these
/// and nothing else. Sharing the function rather than the idea means the
/// wizard warns about a missing `op` in the same words `dotfix doctor` uses
/// later — and, more to the point, that a provider added here cannot be
/// forgotten in setup, which is how choosing 1Password without `op` installed
/// used to pass pre-flight and only fail at the first `apply`.
///
/// Every check here blocks by default. `preflight` re-stamps that field from
/// its own rule, because a tool that is merely missing at setup time can be
/// installed five minutes later — see `init::preflight::is_blocking`.
pub(crate) fn provider_checks(
    fs: &dyn Fsys,
    exec: &dyn Exec,
    paths: &Paths,
    provider: ProviderKind,
    vault: Option<&str>,
) -> Vec<Check> {
    match provider {
        ProviderKind::Keychain => vec![Check::ok("secret provider", "macOS Keychain")],
        ProviderKind::OnePassword => vec![
            match exec.run("op", &["--version"]) {
                Ok(v) => Check::ok("1password cli", v.trim().to_string()),
                Err(e) => Check::fail("1password cli", e.to_string()),
            },
            match vault {
                Some(v) => Check::ok("1password vault", v.to_string()),
                None => Check::fail("1password vault", "no `vault` set for this machine"),
            },
        ],
        ProviderKind::Age => {
            let identity = paths.home.join(".config/dotfix/age.key");
            vec![
                match exec.run("age", &["--version"]) {
                    Ok(v) => Check::ok("age", v.trim().to_string()),
                    Err(e) => Check::fail("age", e.to_string()),
                },
                if fs.exists(&identity) {
                    Check::ok("age identity", identity.display().to_string())
                } else {
                    Check::fail("age identity", format!("missing {}", identity.display()))
                },
            ]
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::config::ProviderKind;
    use crate::paths::Paths;
    use crate::ports::fake::{FakeExec, FakeFsys};

    fn paths() -> Paths {
        Paths::new(PathBuf::from("/Users/test"))
    }

    #[test]
    fn reports_a_missing_launch_agent() {
        let (fs, exec) = (FakeFsys::new(), FakeExec::new([]));
        let checks = run_checks(&fs, &exec, &paths(), ProviderKind::Keychain, None);
        let agent = checks.iter().find(|c| c.name == "launch agent").unwrap();
        assert!(!agent.ok);
    }

    #[test]
    fn reports_an_installed_launch_agent() {
        let fs = FakeFsys::from([(
            "/Users/test/Library/LaunchAgents/dev.noix.dotfix.plist",
            "<plist/>",
        )]);
        let exec = FakeExec::new([]);
        let checks = run_checks(&fs, &exec, &paths(), ProviderKind::Keychain, None);
        let agent = checks.iter().find(|c| c.name == "launch agent").unwrap();
        assert!(agent.ok);
    }

    #[test]
    fn checks_the_op_cli_only_for_onepassword_machines() {
        let (fs, exec) = (FakeFsys::new(), FakeExec::new([]));

        let keychain = run_checks(&fs, &exec, &paths(), ProviderKind::Keychain, None);
        assert!(!keychain.iter().any(|c| c.name == "1password cli"));

        let onepassword = run_checks(
            &fs,
            &exec,
            &paths(),
            ProviderKind::OnePassword,
            Some("Example"),
        );
        assert!(onepassword.iter().any(|c| c.name == "1password cli"));
    }

    #[test]
    fn warns_when_ssh_config_does_not_persist_keys() {
        let fs = FakeFsys::from([("/Users/test/.ssh/config", "Host github.com\n")]);
        let exec = FakeExec::new([]);
        let checks = run_checks(&fs, &exec, &paths(), ProviderKind::Keychain, None);
        let ssh = checks.iter().find(|c| c.name == "ssh key access").unwrap();
        assert!(!ssh.ok);
        assert!(ssh.detail.contains("AddKeysToAgent"));
    }

    #[test]
    fn accepts_an_ssh_config_with_keychain_persistence() {
        let fs = FakeFsys::from([(
            "/Users/test/.ssh/config",
            "Host *\n  AddKeysToAgent yes\n  UseKeychain yes\n",
        )]);
        let exec = FakeExec::new([]);
        let checks = run_checks(&fs, &exec, &paths(), ProviderKind::Keychain, None);
        assert!(
            checks
                .iter()
                .find(|c| c.name == "ssh key access")
                .unwrap()
                .ok
        );
    }

    #[test]
    fn a_login_shell_that_is_not_zsh_is_reported_but_does_not_block() {
        // The failure this catches is silence: dotfix would write a `.zshrc`
        // nothing reads, report everything in sync, and change nothing about
        // the shell the user actually gets.
        //
        // Not blocking: the packages and every managed file still work, and
        // refusing to set up a Mac over its shell would be out of proportion.
        unsafe { std::env::set_var("SHELL", "/bin/bash") };
        let checks = run_checks(
            &FakeFsys::new(),
            &FakeExec::new([]),
            &paths(),
            ProviderKind::Keychain,
            None,
        );
        let check = checks.iter().find(|c| c.name == "login shell").unwrap();
        assert!(!check.ok);
        assert!(check.detail.contains("[[files]]"), "{}", check.detail);

        unsafe { std::env::set_var("SHELL", "/bin/zsh") };
        let checks = run_checks(
            &FakeFsys::new(),
            &FakeExec::new([]),
            &paths(),
            ProviderKind::Keychain,
            None,
        );
        assert!(checks.iter().find(|c| c.name == "login shell").unwrap().ok);
    }
}

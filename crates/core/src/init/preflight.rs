use serde::Serialize;

use crate::doctor::Check;
use crate::init::{Plan, Source};
use crate::paths::Paths;
use crate::ports::{Exec, Fsys};

/// The result of looking before leaping. Writes nothing.
#[derive(Debug, Clone, Serialize)]
pub struct Preflight {
    pub checks: Vec<Check>,
}

impl Preflight {
    /// True when nothing blocks. A failed check that is merely informational
    /// (see `github cli`) is reported as not-ok but is not blocking.
    ///
    /// This reads the same `Check::blocking` field the wizard reads, rather
    /// than a second copy of the rule: [`preflight`] stamps the field once,
    /// and everything downstream — including the webview, which cannot call
    /// this method at all — decides from that one value.
    pub fn passes(&self) -> bool {
        self.checks.iter().all(|c| c.ok || !c.blocking)
    }
}

/// Which checks must pass before anything may be written.
///
/// The rule is about the *kind* of failure, not its severity. A missing tool
/// (`gh`, `op`, `age`, an age identity) is informational: setup itself never
/// calls it, and the user can install it minutes later — blocking would
/// strand someone who wants the Mac configured first. Missing *input* — an
/// empty machine name, a 1Password vault the wizard asked for and did not
/// get — blocks, because it writes a machine file that cannot ever work.
fn is_blocking(name: &str) -> bool {
    !matches!(
        name,
        "github cli" | "1password cli" | "age" | "age identity"
    )
}

pub fn preflight(fs: &dyn Fsys, exec: &dyn Exec, paths: &Paths, plan: &Plan) -> Preflight {
    let mut checks = Vec::new();

    checks.push(if plan.machine.trim().is_empty() {
        Check::fail(
            "machine name",
            "a name is required to identify this machine",
        )
    } else {
        Check::ok("machine name", plan.machine.clone())
    });

    checks.push(match exec.run("git", &["--version"]) {
        Ok(v) => Check::ok("git", v.trim().to_string()),
        Err(e) => Check::fail("git", e.to_string()),
    });

    checks.push(match exec.run("brew", &["--version"]) {
        Ok(v) => Check::ok("homebrew", v.lines().next().unwrap_or("").to_string()),
        Err(e) => Check::fail("homebrew", e.to_string()),
    });

    // A free target directory is the normal state, so say so plainly rather
    // than only complaining when it is taken.
    //
    // An existing dotfix repository is NOT a failure: the common way to get
    // one is an interrupted first run, and blocking there left the user with
    // no way forward but deleting `~/dotfiles` by hand — at the exact moment
    // they are already frustrated. `create_or_clone` reuses it, and on the
    // clone path only once it has confirmed the remote is the repository that
    // was asked for; this check cannot do that itself, so it says what will
    // be attempted rather than promising it.
    let target = paths.home.join("dotfiles");
    checks.push(if fs.exists(&target.join("dotfix.toml")) {
        Check::ok(
            "target directory",
            match &plan.source {
                Source::New => format!(
                    "{} already holds a dotfix repository — it will be reused",
                    target.display()
                ),
                Source::Clone { .. } => format!(
                    "{} already holds a dotfix repository — it will be reused \
                     if it is the one you are cloning",
                    target.display()
                ),
            },
        )
    } else {
        Check::ok("target directory", target.display().to_string())
    });

    // Informational: it only decides whether we can offer to create the remote.
    checks.push(
        match exec.run("gh", &["auth", "status"]) {
            Ok(_) => Check::ok(
                "github cli",
                "authenticated — can create the remote for you",
            ),
            Err(_) => Check::fail(
                "github cli",
                "not installed or not logged in — you will create the repository yourself",
            ),
        }
        .non_blocking(),
    );

    // Whatever the plan's provider needs, asked now rather than at the first
    // `apply`. Literally the checks `dotfix doctor` runs, so the wizard and
    // the doctor cannot disagree about what a working 1Password setup is.
    checks.extend(crate::doctor::provider_checks(
        fs,
        exec,
        paths,
        plan.secret_provider,
        plan.vault.as_deref(),
    ));

    if let Source::Clone { url } = &plan.source {
        checks.push(if url.trim().is_empty() {
            Check::fail("repository url", "a url is required to clone")
        } else {
            Check::ok("repository url", url.clone())
        });
    }

    // One place decides what blocks. `Check` blocks by default, so a check
    // added above without a thought about this is treated as required.
    for check in &mut checks {
        check.blocking = is_blocking(check.name);
    }

    Preflight { checks }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::config::ProviderKind;
    use crate::init::{Plan, Source};
    use crate::paths::Paths;
    use crate::ports::fake::{FakeExec, FakeFsys};

    fn paths() -> Paths {
        Paths::new(PathBuf::from("/Users/test"))
    }

    fn plan() -> Plan {
        Plan {
            machine: "box-one".into(),
            source: Source::New,
            secret_provider: ProviderKind::Keychain,
            vault: None,
        }
    }

    /// Everything present: git, brew, gh authenticated, target free.
    fn healthy_exec() -> FakeExec {
        FakeExec::new([
            ("git --version", "git version 2.51.0\n"),
            ("brew --version", "Homebrew 7.0.2\n"),
            ("gh auth status", "Logged in to github.com\n"),
        ])
    }

    fn named<'a>(p: &'a Preflight, name: &str) -> &'a crate::doctor::Check {
        p.checks
            .iter()
            .find(|c| c.name == name)
            .unwrap_or_else(|| panic!("no check named `{name}` in {:?}", p.checks))
    }

    #[test]
    fn a_ready_machine_passes() {
        let p = preflight(&FakeFsys::new(), &healthy_exec(), &paths(), &plan());
        assert!(p.passes(), "{:?}", p.checks);
    }

    #[test]
    fn a_missing_target_directory_is_what_we_want_not_a_failure() {
        let p = preflight(&FakeFsys::new(), &healthy_exec(), &paths(), &plan());
        assert!(named(&p, "target directory").ok);
    }

    #[test]
    fn an_existing_repository_is_offered_for_reuse_not_treated_as_a_failure() {
        // An interrupted first run leaves exactly this state. Blocking here
        // meant the only way forward was deleting ~/dotfiles by hand.
        let fs = FakeFsys::from([("/Users/test/dotfiles/dotfix.toml", "schema_version = 1\n")]);
        let p = preflight(&fs, &healthy_exec(), &paths(), &plan());

        let check = named(&p, "target directory");
        assert!(check.ok, "{check:?}");
        assert!(check.detail.contains("/Users/test/dotfiles"));
        assert!(check.detail.contains("reused"));
        assert!(p.passes());
    }

    #[test]
    fn the_clone_path_does_not_promise_a_reuse_it_cannot_verify_here() {
        // Whether the repository already there is the one being cloned is a
        // question only `create_or_clone` can answer, because it needs git.
        let fs = FakeFsys::from([("/Users/test/dotfiles/dotfix.toml", "schema_version = 1\n")]);
        let mut plan = plan();
        plan.source = Source::Clone {
            url: "git@github.com:example-user/dotfiles.git".into(),
        };
        let p = preflight(&fs, &healthy_exec(), &paths(), &plan);

        let check = named(&p, "target directory");
        assert!(check.ok);
        assert!(
            check.detail.contains("if it is the one you are cloning"),
            "{check:?}"
        );
    }

    #[test]
    fn missing_git_blocks() {
        let exec = FakeExec::new([("brew --version", "Homebrew 7.0.2\n")]);
        let p = preflight(&FakeFsys::new(), &exec, &paths(), &plan());
        assert!(!named(&p, "git").ok);
        assert!(!p.passes());
    }

    #[test]
    fn missing_gh_is_reported_but_does_not_block() {
        // `gh` only decides whether we can offer to create the remote for the
        // user. Setting up locally must not depend on it.
        let exec = FakeExec::new([
            ("git --version", "git version 2.51.0\n"),
            ("brew --version", "Homebrew 7.0.2\n"),
        ]);
        let p = preflight(&FakeFsys::new(), &exec, &paths(), &plan());

        assert!(!named(&p, "github cli").ok);
        assert!(p.passes(), "a missing gh must not block local setup");
    }

    #[test]
    fn whether_a_check_blocks_is_serialised_not_re_derived_by_the_caller() {
        // The frontend gates its "Set up" button on this field. `passes()` is
        // a method and does not cross the Tauri boundary, so a caller that
        // re-implemented the rule would get it wrong — as the wizard did.
        let exec = FakeExec::new([
            ("git --version", "git version 2.51.0\n"),
            ("brew --version", "Homebrew 7.0.2\n"),
        ]);
        let p = preflight(&FakeFsys::new(), &exec, &paths(), &plan());

        assert!(
            !named(&p, "github cli").blocking,
            "gh only decides whether we can offer to create the remote"
        );
        assert!(named(&p, "git").blocking);
        assert!(named(&p, "machine name").blocking);
        assert!(named(&p, "target directory").blocking);
    }

    #[test]
    fn choosing_onepassword_without_op_installed_is_reported_at_setup_time() {
        // The gap this closes: the wizard asked which provider to use and then
        // never checked it, so the machine file recorded `1password` on a Mac
        // with no `op` and the first `apply` days later was the first anyone
        // heard of it.
        let mut plan = plan();
        plan.secret_provider = ProviderKind::OnePassword;
        plan.vault = Some("Private".into());

        let p = preflight(&FakeFsys::new(), &healthy_exec(), &paths(), &plan);
        assert!(!named(&p, "1password cli").ok);
    }

    #[test]
    fn a_missing_op_is_reported_but_does_not_block() {
        // Same rule as `gh`: a tool that is merely absent can be installed five
        // minutes from now, and setup itself never calls it. Blocking here
        // would strand a user who wants to set the Mac up first and install
        // 1Password after.
        let mut plan = plan();
        plan.secret_provider = ProviderKind::OnePassword;
        plan.vault = Some("Private".into());

        let p = preflight(&FakeFsys::new(), &healthy_exec(), &paths(), &plan);
        assert!(!named(&p, "1password cli").blocking);
        assert!(p.passes(), "a missing op must not block local setup");
    }

    #[test]
    fn onepassword_without_a_vault_blocks_because_that_is_missing_input_not_a_missing_tool() {
        // A vault is something the wizard asked for and the user left empty —
        // a hole in the plan, like an empty machine name. Writing the machine
        // file without it produces a configuration that cannot ever work.
        let mut plan = plan();
        plan.secret_provider = ProviderKind::OnePassword;
        plan.vault = None;

        let p = preflight(&FakeFsys::new(), &healthy_exec(), &paths(), &plan);
        let check = named(&p, "1password vault");
        assert!(!check.ok);
        assert!(check.blocking);
        assert!(!p.passes());
    }

    #[test]
    fn choosing_age_checks_both_the_binary_and_the_identity_file() {
        let mut plan = plan();
        plan.secret_provider = ProviderKind::Age;

        let p = preflight(&FakeFsys::new(), &healthy_exec(), &paths(), &plan);
        assert!(!named(&p, "age").ok);
        assert!(!named(&p, "age identity").ok);
        assert!(
            p.passes(),
            "neither blocks: the key is normally generated after setup"
        );
    }

    #[test]
    fn a_keychain_machine_is_asked_nothing_about_op_or_age() {
        // The default provider needs no external tool, so pre-flight must not
        // invent failures for tools this machine will never call.
        let p = preflight(&FakeFsys::new(), &healthy_exec(), &paths(), &plan());
        for absent in ["1password cli", "1password vault", "age", "age identity"] {
            assert!(
                !p.checks.iter().any(|c| c.name == absent),
                "keychain setup must not check `{absent}`"
            );
        }
        assert!(named(&p, "secret provider").ok);
    }

    #[test]
    fn an_empty_machine_name_blocks() {
        let mut plan = plan();
        plan.machine = String::new();
        let p = preflight(&FakeFsys::new(), &healthy_exec(), &paths(), &plan);
        assert!(!named(&p, "machine name").ok);
        assert!(!p.passes());
    }
}

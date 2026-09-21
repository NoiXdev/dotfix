//! First-time setup, as four steps either front end can drive.
//!
//! The split exists for failure reporting: a user in the app has no terminal
//! open to investigate, so the app must be able to say which step failed. The
//! ordering is binding — [`preflight`] never writes, and only once it passes
//! does [`create_or_clone`] touch the filesystem.

mod preflight;
pub mod remote;
mod scaffold;
pub mod ssh;

pub use preflight::{Preflight, preflight};
pub use scaffold::{
    IMPORTED_FRAGMENT, STATUS_FRAGMENT, create_or_clone, detect_requirements, scaffold,
};

use std::path::{Path, PathBuf};

use crate::config::ProviderKind;
use crate::error::{Error, Result};
use crate::paths::{LocalConfig, Paths};
use crate::ports::Fsys;

/// Where this machine's configuration comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// Build a repository from what is installed on this machine.
    New,
    /// Clone an existing repository.
    Clone { url: String },
}

/// Everything the caller must decide before anything is written. Prompts are
/// the front end's business; the steps take a value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub machine: String,
    pub source: Source,
    pub secret_provider: ProviderKind,
    /// 1Password only.
    pub vault: Option<String>,
}

/// How the clone will actually authenticate, which is what decides the URL
/// it must use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Auth {
    /// The machine's existing ssh setup already reaches the host.
    Ssh,
    /// A key dotfix generated, reachable only through its own ssh alias.
    DeployKey,
    /// A token in the macOS Keychain, which git offers over https only.
    Token,
}

/// Split `owner/repo` out of whatever the user typed.
///
/// Understands the shapes a GitHub clone URL actually comes in — scp-like
/// with or without a user, https, and `ssh://` — and returns an error for
/// anything else rather than guessing. Guessing here would mean cloning
/// from somewhere other than the repository the user named.
pub fn owner_repo(url: &str) -> Result<String> {
    let url = url.trim();
    let unusable = || {
        Error::Config(format!(
            "cannot tell the owner and repository from `{url}` — expected \
             something like git@github.com:owner/repo.git or \
             https://github.com/owner/repo.git"
        ))
    };

    // `scheme://[user@]host[:port]/owner/repo`, or scp-like `[user@]host:owner/repo`.
    // The scheme is checked first, because its `://` also contains a colon.
    let path = match url.split_once("://") {
        Some((_, rest)) => rest.split_once('/').map(|(_, path)| path),
        None => url.split_once(':').map(|(_, path)| path),
    }
    .ok_or_else(unusable)?;

    let path = path.trim_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);

    let mut segments = path.split('/');
    let owner = segments.next().unwrap_or_default();
    let repo = segments.next().unwrap_or_default();
    if owner.is_empty() || repo.is_empty() || segments.next().is_some() {
        return Err(unusable());
    }
    Ok(format!("{owner}/{repo}"))
}

/// Extract the host component from a git clone URL, understanding scp-like
/// syntax with or without an explicit user (`git@host:path` / `host:path`)
/// and full URL syntax (`scheme://[user@]host[:port]/path`). Returns `None`
/// for a shape it does not recognise rather than guessing.
pub fn url_host(url: &str) -> Option<&str> {
    if let Some(rest) = url.split("://").nth(1) {
        let after_at = rest.rsplit_once('@').map_or(rest, |(_, h)| h);
        let host = after_at.split(['/', ':']).next()?;
        return (!host.is_empty()).then_some(host);
    }

    if let Some((_, rest)) = url.split_once('@') {
        // scp-like syntax with an explicit user: `user@host:path`.
        let host = rest.split(':').next()?;
        return (!host.is_empty()).then_some(host);
    }

    // Bare scp-like syntax, no user: `host:path`. Git's scp shorthand has no
    // port field at all — everything after the first colon is the path,
    // always, so `example.com:22` is host `example.com` with path `22`, not
    // host `example.com` on port 22. (Ports only exist in the `ssh://`
    // branch above, where they are unambiguous because the scheme marks it
    // as a URL rather than scp syntax.) Treating an all-digit remainder as a
    // port here would skip the host-key check for a legitimate URL like
    // `git@github.com:1234` — so this does not look for one.
    let (host, _path) = url.split_once(':')?;
    (!host.is_empty()).then_some(host)
}

/// A clone URL's host, but only if it is GitHub itself or the ssh alias
/// [`ssh::HOST_ALIAS`] scopes a deploy key to. Matches on the host component
/// via [`url_host`] against the exact alias constant — not a naive substring
/// of the whole URL, and not a prefix, since the codebase only ever
/// generates that one alias and a prefix would also match an unrelated host
/// that merely starts with the same characters (e.g.
/// `github.com-evil.example`).
pub fn github_host(url: &str) -> Option<&str> {
    url_host(url).filter(|h| *h == "github.com" || *h == ssh::HOST_ALIAS)
}

/// The URL to actually clone from, derived from the authentication that will
/// carry it.
///
/// The URL the user typed is not always the URL to clone: a deploy key is
/// scoped to an ssh alias with `IdentitiesOnly yes`, so cloning the typed
/// `git@github.com:…` would never offer that key; a token lives in the
/// Keychain and git only presents it over https. Each authentication method
/// therefore implies its own URL, and the caller must show the result to the
/// user before it is used.
pub fn effective_clone_url(typed: &str, auth: Auth) -> Result<String> {
    Ok(match auth {
        Auth::Ssh => typed.trim().to_string(),
        Auth::DeployKey => {
            let repo = owner_repo(typed)?;
            require_github(typed, "a deploy key")?;
            ssh::clone_url(ssh::HOST_ALIAS, &repo)
        }
        Auth::Token => {
            let repo = owner_repo(typed)?;
            require_github(typed, "a token")?;
            remote::https_url(&repo)
        }
    })
}

/// Refuse a host the rewriting paths would silently replace with GitHub.
///
/// [`effective_clone_url`] builds the deploy-key and token URLs from
/// `owner/repo` alone, putting `github.com` back in front of it. For a typed
/// GitHub URL that is exactly right. For anything else it would clone a
/// *different repository that happens to share the name* — on a host the user
/// never named, possibly belonging to someone else. dotfix ships GitHub
/// fingerprints and a GitHub token flow and nothing more, so the honest
/// answer is to say so rather than to quietly substitute a remote.
fn require_github(typed: &str, method: &str) -> Result<()> {
    match github_host(typed) {
        Some(_) => Ok(()),
        None => Err(Error::Config(format!(
            "{method} only works with github.com, and `{}` is not on github.com — clone it over ssh instead",
            url_host(typed).unwrap_or(typed.trim())
        ))),
    }
}

/// Whether two clone URLs address the same repository.
///
/// Compared by host and `owner/repo` rather than by string, so the same
/// repository named over ssh and over https counts as the same — the shapes
/// differ, the repository does not. Anything neither side can parse falls
/// back to an exact comparison rather than a guess.
///
/// The host is compared strictly: `gitlab.com:acme/dotfiles` and
/// `github.com:acme/dotfiles` are different repositories that merely share a
/// name, and treating them as one is exactly the confusion this exists to
/// prevent.
pub fn same_repository(a: &str, b: &str) -> bool {
    match (owner_repo(a), owner_repo(b), url_host(a), url_host(b)) {
        (Ok(ra), Ok(rb), Some(ha), Some(hb)) => ra == rb && ha.eq_ignore_ascii_case(hb),
        _ => a.trim() == b.trim(),
    }
}

/// How often the background check runs, in seconds.
pub const DEFAULT_INTERVAL: u32 = 3600;

/// Record this machine in the repository and point the machine at the
/// repository.
///
/// Merges into an existing machine file rather than replacing it: cloning a
/// repository that already knows this machine keeps its sets *and* its secret
/// provider. Only a machine the repository has never recorded takes its
/// provider from the plan.
pub fn configure_machine(fs: &dyn Fsys, repo: &Path, paths: &Paths, plan: &Plan) -> Result<()> {
    let machine_file = repo.join("machines").join(format!("{}.toml", plan.machine));

    let already_recorded = fs.exists(&machine_file);
    let mut cfg: crate::config::MachineConfig = if already_recorded {
        toml::from_str(&fs.read(&machine_file)?).map_err(|source| Error::Toml {
            path: machine_file.clone(),
            source,
        })?
    } else {
        crate::config::MachineConfig {
            sets: vec!["core".into()],
            ..Default::default()
        }
    };

    // Only decide the provider for a machine the repository has never heard
    // of. When the file is already there — a reinstall, a restore, a second
    // run of setup against a repository that already knows this Mac — its
    // recorded provider and vault stand. Setup asked its question before the
    // clone existed, so its answer is necessarily the *older* information;
    // taking it would silently downgrade a working 1Password machine to the
    // wizard's default and break every secret at the next `apply`.
    //
    // Changing an already-recorded machine's provider is deliberately not a
    // setup operation: edit `machines/<name>.toml`, where the change is
    // visible and reviewable.
    if !already_recorded {
        cfg.secret_provider = plan.secret_provider;
        // A vault is meaningless outside 1Password; storing one would be a lie
        // the doctor would later report on.
        cfg.vault = match plan.secret_provider {
            ProviderKind::OnePassword => plan.vault.clone(),
            _ => None,
        };
    }

    let raw = toml::to_string_pretty(&cfg)
        .map_err(|e| Error::Config(format!("serialising {}: {e}", machine_file.display())))?;
    fs.write(&machine_file, &raw, 0o644)?;

    LocalConfig {
        repo: repo.to_path_buf(),
        machine: plan.machine.clone(),
    }
    .save(fs, &paths.local_config())
}

/// Install the LaunchAgent for the hourly background check.
pub fn install_agent(
    fs: &dyn Fsys,
    paths: &Paths,
    binary: &Path,
    interval: u32,
    path_env: &str,
) -> Result<PathBuf> {
    crate::agent::install(fs, paths, binary, interval, path_env)
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::config::{MachineConfig, ProviderKind};
    use crate::paths::{LocalConfig, Paths};
    use crate::ports::Fsys;
    use crate::ports::fake::FakeFsys;

    fn paths() -> Paths {
        Paths::new(PathBuf::from("/Users/test"))
    }

    fn plan(provider: ProviderKind, vault: Option<&str>) -> Plan {
        Plan {
            machine: "box-one".into(),
            source: Source::New,
            secret_provider: provider,
            vault: vault.map(str::to_string),
        }
    }

    #[test]
    fn owner_and_repo_come_out_of_every_shape_a_clone_url_takes() {
        for url in [
            "git@github.com:example-user/dotfiles.git",
            "github.com:example-user/dotfiles.git",
            "https://github.com/example-user/dotfiles.git",
            "ssh://git@github.com/example-user/dotfiles.git",
            "https://github.com/example-user/dotfiles",
            "  git@github.com:example-user/dotfiles  ",
        ] {
            assert_eq!(
                owner_repo(url).unwrap(),
                "example-user/dotfiles",
                "parsing `{url}`"
            );
        }
    }

    #[test]
    fn a_deploy_key_refuses_a_host_it_cannot_reach_instead_of_rewriting_it() {
        // Both of these paths derive their URL from `owner/repo` alone and
        // put GitHub back in front of it. Typing a GitLab URL and getting a
        // clone of a *same-named GitHub repository* — a different repository,
        // possibly someone else's — is the one outcome setup must never
        // produce silently.
        for auth in [Auth::DeployKey, Auth::Token] {
            let err = effective_clone_url("git@gitlab.com:example-user/dotfiles.git", auth)
                .expect_err("a non-GitHub host must not be rewritten to GitHub");
            let msg = err.to_string();
            assert!(msg.contains("gitlab.com"), "{msg}");
            assert!(msg.contains("github.com"), "{msg}");
        }
    }

    #[test]
    fn ssh_still_clones_a_non_github_host_exactly_as_typed() {
        // Only the two rewriting paths are restricted. Plain ssh carries the
        // URL through untouched, so a self-hosted remote keeps working — it
        // just gets no pinned host key, which `host_to_pin` already reports.
        assert_eq!(
            effective_clone_url("git@gitlab.com:example-user/dotfiles.git", Auth::Ssh).unwrap(),
            "git@gitlab.com:example-user/dotfiles.git"
        );
    }

    #[test]
    fn the_deploy_key_alias_is_accepted_as_github_not_rejected_as_a_stranger() {
        // A user pasting back the URL dotfix itself produced must not be told
        // it is the wrong host.
        assert_eq!(
            effective_clone_url(
                "git@github.com-dotfix:example-user/dotfiles.git",
                Auth::DeployKey
            )
            .unwrap(),
            "git@github.com-dotfix:example-user/dotfiles.git"
        );
    }

    #[test]
    fn the_unusable_url_message_reads_as_a_sentence() {
        // It used to carry runs of six spaces: a multi-line literal without
        // `\` continuations renders its own indentation. Nobody noticed
        // because the wizard clipped the text before it reached the screen.
        let msg = owner_repo("dotfiles").unwrap_err().to_string();
        assert!(!msg.contains("  "), "runaway whitespace in: {msg}");
    }

    #[test]
    fn the_same_repository_is_recognised_across_url_shapes() {
        assert!(same_repository(
            "git@github.com:example-user/dotfiles.git",
            "https://github.com/example-user/dotfiles"
        ));
        assert!(same_repository(
            "ssh://git@github.com/example-user/dotfiles.git",
            "git@github.com:example-user/dotfiles"
        ));
    }

    #[test]
    fn a_same_named_repository_on_another_host_is_a_different_repository() {
        assert!(!same_repository(
            "git@gitlab.com:example-user/dotfiles.git",
            "git@github.com:example-user/dotfiles.git"
        ));
    }

    #[test]
    fn a_different_repository_on_the_same_host_is_not_the_same() {
        assert!(!same_repository(
            "git@github.com:example-user/dotfiles.git",
            "git@github.com:example-user/other.git"
        ));
    }

    #[test]
    fn unparseable_urls_fall_back_to_an_exact_comparison() {
        assert!(same_repository("  weird  ", "weird"));
        assert!(!same_repository("weird", "other"));
    }

    #[test]
    fn a_url_it_cannot_parse_is_an_error_not_a_guess() {
        // Guessing here would clone from somewhere other than what the user
        // named, which is the one thing this must never do.
        for url in [
            "",
            "dotfiles",
            "git@github.com:",
            "https://github.com/",
            "https://github.com/one/two/three",
        ] {
            let err = owner_repo(url).unwrap_err().to_string();
            assert!(
                err.contains("cannot tell the owner and repository"),
                "`{url}` must be refused clearly: {err}"
            );
        }
    }

    #[test]
    fn working_ssh_clones_exactly_what_the_user_typed() {
        assert_eq!(
            effective_clone_url("git@github.com:example-user/dotfiles.git", Auth::Ssh).unwrap(),
            "git@github.com:example-user/dotfiles.git"
        );
    }

    #[test]
    fn a_deploy_key_clones_through_the_alias_that_scopes_it() {
        // The stanza `ensure_ssh_config` writes has `IdentitiesOnly yes`, so
        // the key is only ever offered for this alias. Cloning the typed
        // `git@github.com:…` would not use it at all.
        assert_eq!(
            effective_clone_url("git@github.com:example-user/dotfiles.git", Auth::DeployKey)
                .unwrap(),
            "git@github.com-dotfix:example-user/dotfiles.git"
        );
    }

    #[test]
    fn a_stored_token_clones_over_https_where_git_will_offer_it() {
        // A token in the Keychain is useless over ssh; cloning the typed ssh
        // URL fails with `Permission denied (publickey)`, pointing the user
        // at the very mechanism they chose not to use.
        assert_eq!(
            effective_clone_url("git@github.com:example-user/dotfiles.git", Auth::Token).unwrap(),
            "https://github.com/example-user/dotfiles.git"
        );
    }

    #[test]
    fn it_writes_the_local_pointer_so_every_later_command_finds_the_repo() {
        let fs = FakeFsys::new();
        configure_machine(
            &fs,
            Path::new("/Users/test/dotfiles"),
            &paths(),
            &plan(ProviderKind::Keychain, None),
        )
        .unwrap();

        let local = LocalConfig::load(&fs, &paths().local_config()).unwrap();
        assert_eq!(local.machine, "box-one");
        assert_eq!(local.repo, PathBuf::from("/Users/test/dotfiles"));
    }

    #[test]
    fn it_records_the_chosen_secret_provider_on_the_machine() {
        let fs = FakeFsys::new();
        configure_machine(
            &fs,
            Path::new("/Users/test/dotfiles"),
            &paths(),
            &plan(ProviderKind::OnePassword, Some("Example")),
        )
        .unwrap();

        let raw = fs
            .read(Path::new("/Users/test/dotfiles/machines/box-one.toml"))
            .unwrap();
        let cfg: MachineConfig = toml::from_str(&raw).unwrap();
        assert_eq!(cfg.secret_provider, ProviderKind::OnePassword);
        assert_eq!(cfg.vault.as_deref(), Some("Example"));
    }

    #[test]
    fn keychain_needs_no_vault() {
        let fs = FakeFsys::new();
        configure_machine(
            &fs,
            Path::new("/Users/test/dotfiles"),
            &paths(),
            &plan(ProviderKind::Keychain, Some("ignored")),
        )
        .unwrap();

        let raw = fs
            .read(Path::new("/Users/test/dotfiles/machines/box-one.toml"))
            .unwrap();
        let cfg: MachineConfig = toml::from_str(&raw).unwrap();
        assert_eq!(
            cfg.vault, None,
            "a vault only means something for 1Password"
        );
    }

    #[test]
    fn a_cloned_repository_keeps_the_sets_its_machine_file_already_had() {
        // The clone path must not stamp over a machine file the repository
        // already carries for this name.
        let fs = FakeFsys::from([(
            "/Users/test/dotfiles/machines/box-one.toml",
            "sets = [\"core\", \"web\"]\n",
        )]);
        configure_machine(
            &fs,
            Path::new("/Users/test/dotfiles"),
            &paths(),
            &plan(ProviderKind::Keychain, None),
        )
        .unwrap();

        let raw = fs
            .read(Path::new("/Users/test/dotfiles/machines/box-one.toml"))
            .unwrap();
        let cfg: MachineConfig = toml::from_str(&raw).unwrap();
        assert_eq!(cfg.sets, vec!["core", "web"]);
    }

    #[test]
    fn a_cloned_repository_keeps_the_secret_provider_its_machine_file_already_had() {
        // Setting this Mac up again from a repository that already knows it —
        // a reinstall, a restore — must not silently downgrade a working
        // 1Password machine to the wizard's default. The repository is the
        // record of how this machine already works; a setup wizard asking its
        // question again does not make the answer newer.
        let fs = FakeFsys::from([(
            "/Users/test/dotfiles/machines/box-one.toml",
            "sets = [\"core\"]\nsecret_provider = \"1password\"\nvault = \"Private\"\n",
        )]);
        configure_machine(
            &fs,
            Path::new("/Users/test/dotfiles"),
            &paths(),
            &plan(ProviderKind::Keychain, None),
        )
        .unwrap();

        let raw = fs
            .read(Path::new("/Users/test/dotfiles/machines/box-one.toml"))
            .unwrap();
        let cfg: MachineConfig = toml::from_str(&raw).unwrap();
        assert_eq!(cfg.secret_provider, ProviderKind::OnePassword);
        assert_eq!(cfg.vault.as_deref(), Some("Private"));
    }

    #[test]
    fn a_machine_the_repository_does_not_know_yet_takes_the_plans_provider() {
        // The other half of the rule: joining an existing repository with a
        // new machine has no recorded provider to preserve, so the wizard's
        // answer is the only answer there is.
        let fs = FakeFsys::from([(
            "/Users/test/dotfiles/machines/other-box.toml",
            "sets = [\"core\"]\n",
        )]);
        configure_machine(
            &fs,
            Path::new("/Users/test/dotfiles"),
            &paths(),
            &plan(ProviderKind::OnePassword, Some("Private")),
        )
        .unwrap();

        let raw = fs
            .read(Path::new("/Users/test/dotfiles/machines/box-one.toml"))
            .unwrap();
        let cfg: MachineConfig = toml::from_str(&raw).unwrap();
        assert_eq!(cfg.secret_provider, ProviderKind::OnePassword);
        assert_eq!(cfg.vault.as_deref(), Some("Private"));
    }
}

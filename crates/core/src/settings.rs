//! Changing what `init` decided once: the remote, the secret provider, the
//! machine's name.
//!
//! All three were write-once until now — `init` set them and nothing could
//! move them afterwards, which meant switching from Keychain to 1Password or
//! moving the repository to a different host was a hand edit in three files.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::config::{MachineConfig, ProviderKind, Repo};
use crate::error::{Error, Result};
use crate::paths::{LocalConfig, Paths};
use crate::ports::{Fsys, Git};

/// Everything the settings screen shows, gathered from the three places it
/// actually lives in. Which place matters: the provider travels to other
/// machines in the repository, the remote and the machine name do not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Settings {
    pub machine: String,
    pub repo: PathBuf,
    /// `None` when the repository has no remote — the normal state after
    /// `--set-up-new` until one is added.
    pub remote: Option<String>,
    pub secret_provider: ProviderKind,
    pub vault: Option<String>,
    /// Packages this machine never reports as unmanaged.
    ///
    /// Shown because it was not: ignoring something removed it from every
    /// view dotfix has, with the machine file as the only record. A decision
    /// you cannot see is one you cannot undo.
    pub ignored: Vec<String>,
}

pub fn read(fs: &dyn Fsys, git: &dyn Git, local: &LocalConfig) -> Result<Settings> {
    let repo = Repo::load(fs, &local.repo)?;
    let machine = repo
        .machines
        .get(&local.machine)
        .ok_or_else(|| Error::UnknownMachine(local.machine.clone()))?;

    Ok(Settings {
        machine: local.machine.clone(),
        repo: local.repo.clone(),
        remote: git.remote_url(&local.repo)?,
        secret_provider: machine.secret_provider,
        vault: machine.vault.clone(),
        ignored: machine.ignore.clone(),
    })
}

/// Point the repository at a different remote.
pub fn set_remote(git: &dyn Git, local: &LocalConfig, url: &str) -> Result<()> {
    let url = url.trim();
    if url.is_empty() {
        return Err(Error::Config("a remote url cannot be empty".into()));
    }
    git.set_remote_url(&local.repo, url)
}

/// Stop ignoring a package on this machine.
///
/// The inverse of the Ignore button, which had none: the package returns to
/// the unmanaged list at the next check, where it can be adopted or ignored
/// again.
pub fn unignore(fs: &dyn Fsys, local: &LocalConfig, package: &str) -> Result<Vec<String>> {
    let repo = Repo::load(fs, &local.repo)?;
    let mut ignored = repo
        .machines
        .get(&local.machine)
        .ok_or_else(|| Error::UnknownMachine(local.machine.clone()))?
        .ignore
        .clone();

    let before = ignored.len();
    ignored.retain(|p| p != package);
    if ignored.len() == before {
        return Err(Error::Config(format!(
            "`{package}` is not ignored on this machine"
        )));
    }

    let path = local
        .repo
        .join("machines")
        .join(format!("{}.toml", local.machine));
    let raw = crate::config::edit::put_strings(&fs.read(&path)?, &[], "ignore", &ignored)?;
    fs.write(&path, &raw, 0o644)?;
    Ok(ignored)
}

/// Rename this machine, everywhere the name is written.
///
/// The name is not one value in one file. It is the name of a file in the
/// repository, a field in the local configuration, and — when a deploy key
/// was generated — the key's file name and the `IdentityFile` line that
/// points at it. Missing one of those leaves dotfix looking for a machine
/// that no longer exists.
///
/// The order below is chosen so an interruption cannot strand the machine:
/// the new repository file is written *before* the old one is removed, and
/// the local pointer moves between them. A failure anywhere leaves a state
/// that still resolves — at worst with one stale file to delete by hand.
///
/// Not handled, and reported rather than hidden: a deploy key registered on
/// GitHub carries the old name as its title. Changing that means registering
/// a new key, which is a decision, not a rename.
pub fn rename_machine(
    fs: &dyn Fsys,
    paths: &Paths,
    local: &LocalConfig,
    new_name: &str,
) -> Result<LocalConfig> {
    let new_name = new_name.trim();
    if new_name.is_empty() {
        return Err(Error::Config("a machine name cannot be empty".into()));
    }
    if new_name == local.machine {
        return Err(Error::Config(format!(
            "this machine is already called `{new_name}`"
        )));
    }

    let machines = local.repo.join("machines");
    let old_file = machines.join(format!("{}.toml", local.machine));
    let new_file = machines.join(format!("{new_name}.toml"));
    if fs.exists(&new_file) {
        return Err(Error::Config(format!(
            "the repository already has a machine called `{new_name}`"
        )));
    }

    let body = fs.read(&old_file)?;
    fs.write(&new_file, &body, 0o644)?;

    let moved = LocalConfig {
        repo: local.repo.clone(),
        machine: new_name.to_string(),
    };
    moved.save(fs, &paths.local_config())?;

    rename_deploy_key(fs, paths, &local.machine, new_name)?;

    fs.remove(&old_file)?;
    Ok(moved)
}

/// Move a generated deploy key to the new name and repoint `~/.ssh/config`.
///
/// A no-op when no key was generated, which is the common case — most
/// machines reach the remote with the ssh setup they already had. Done before
/// the old machine file is removed, so a failure here leaves a repository
/// that still describes this machine under both names rather than neither.
fn rename_deploy_key(fs: &dyn Fsys, paths: &Paths, old: &str, new: &str) -> Result<()> {
    let from = deploy_key_path(paths, old);
    if !fs.exists(&from) {
        return Ok(());
    }
    let to = deploy_key_path(paths, new);

    for (from, to) in [(from.clone(), to.clone()), (with_pub(&from), with_pub(&to))] {
        if fs.exists(&from) {
            let body = fs.read(&from)?;
            // 0600 for the private half; the public one is harmless but there
            // is no reason to widen it either.
            fs.write(&to, &body, 0o600)?;
            fs.remove(&from)?;
        }
    }

    let config = paths.home.join(".ssh/config");
    if fs.exists(&config) {
        let body = fs.read(&config)?;
        let updated = body.replace(&from.to_string_lossy().to_string(), &to.to_string_lossy());
        if updated != body {
            fs.write(&config, &updated, 0o600)?;
        }
    }
    Ok(())
}

fn with_pub(key: &Path) -> PathBuf {
    let mut name = key.as_os_str().to_os_string();
    name.push(".pub");
    PathBuf::from(name)
}

/// Switch the secret provider, but only once every secret the repository
/// references can actually be found at the new one.
///
/// Writing first and discovering later is the failure mode worth avoiding:
/// the machine file would record a provider that cannot serve, and the next
/// `apply` — possibly days later, possibly on a schedule — would be the first
/// anyone heard of it. `verify` renders everything against the *new* provider
/// and is expected to fail loudly when a name is missing.
pub fn set_provider(
    fs: &dyn Fsys,
    local: &LocalConfig,
    provider: ProviderKind,
    vault: Option<String>,
    verify: &dyn Fn(ProviderKind, Option<&str>) -> Result<()>,
) -> Result<MachineConfig> {
    if provider == ProviderKind::OnePassword && vault.as_deref().unwrap_or("").trim().is_empty() {
        return Err(Error::Config(
            "1Password needs a vault name — without one dotfix cannot look anything up".into(),
        ));
    }

    verify(provider, vault.as_deref())?;

    let repo = Repo::load(fs, &local.repo)?;
    let mut cfg = repo
        .machines
        .get(&local.machine)
        .ok_or_else(|| Error::UnknownMachine(local.machine.clone()))?
        .clone();

    cfg.secret_provider = provider;
    // A vault means nothing outside 1Password; keeping one would be a claim
    // `doctor` would later contradict.
    cfg.vault = match provider {
        ProviderKind::OnePassword => vault,
        _ => None,
    };

    let path = local
        .repo
        .join("machines")
        .join(format!("{}.toml", local.machine));
    let name = match provider {
        ProviderKind::Keychain => "keychain",
        ProviderKind::OnePassword => "1password",
        ProviderKind::Age => "age",
    };
    // In place, so a comment in the machine file survives a provider change.
    let raw = fs.read(&path)?;
    let raw = crate::config::edit::put_string(&raw, &[], "secret_provider", Some(name))?;
    let raw = crate::config::edit::put_string(&raw, &[], "vault", cfg.vault.as_deref())?;
    fs.write(&path, &raw, 0o644)?;
    Ok(cfg)
}

/// Where a deploy key for `machine` would live, if one was generated.
pub fn deploy_key_path(paths: &Paths, machine: &str) -> PathBuf {
    paths
        .home
        .join(".ssh")
        .join(format!("dotfix_{machine}_ed25519"))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::ports::fake::{FakeFsys, FakeGit};

    fn paths() -> Paths {
        Paths::new(PathBuf::from("/Users/test"))
    }

    fn local() -> LocalConfig {
        LocalConfig {
            repo: PathBuf::from("/repo"),
            machine: "old-box".into(),
        }
    }

    fn fs() -> FakeFsys {
        FakeFsys::from([
            ("/repo/dotfix.toml", "schema_version = 1\n"),
            ("/repo/sets/core/set.toml", "[packages]\nbrew = []\n"),
            (
                "/repo/machines/old-box.toml",
                "sets = [\"core\"]\nsecret_provider = \"keychain\"\n",
            ),
        ])
    }

    #[test]
    fn it_reads_from_all_three_places_at_once() {
        let s = read(&fs(), &FakeGit::new(), &local()).unwrap();
        assert_eq!(s.machine, "old-box");
        assert_eq!(s.secret_provider, ProviderKind::Keychain);
        assert_eq!(
            s.remote.as_deref(),
            Some("git@github.com:example-user/dotfiles.git")
        );
    }

    #[test]
    fn renaming_moves_the_repository_file_and_the_local_pointer() {
        let fs = fs();
        let moved = rename_machine(&fs, &paths(), &local(), "new-box").unwrap();

        assert_eq!(moved.machine, "new-box");
        assert!(fs.read(Path::new("/repo/machines/new-box.toml")).is_ok());
        assert!(
            fs.read(Path::new("/repo/machines/old-box.toml")).is_err(),
            "the old name must not linger, or the repository claims two machines"
        );
        let stored = LocalConfig::load(&fs, &paths().local_config()).unwrap();
        assert_eq!(stored.machine, "new-box");
    }

    #[test]
    fn renaming_onto_an_existing_machine_is_refused() {
        // Two machines sharing a file is worse than a bad name.
        let fs = fs();
        fs.write(Path::new("/repo/machines/taken.toml"), "sets = []\n", 0o644)
            .unwrap();
        let err = rename_machine(&fs, &paths(), &local(), "taken")
            .unwrap_err()
            .to_string();
        assert!(err.contains("already has a machine"), "{err}");
    }

    #[test]
    fn switching_provider_writes_nothing_when_a_secret_cannot_be_found() {
        // The whole point: a provider recorded but unable to serve turns into
        // a failure days later, on a schedule, with no one watching.
        let fs = fs();
        let refuse = |_: ProviderKind, _: Option<&str>| {
            Err(Error::Config("no item named `smtp_password`".into()))
        };
        let err = set_provider(
            &fs,
            &local(),
            ProviderKind::OnePassword,
            Some("Private".into()),
            &refuse,
        )
        .unwrap_err()
        .to_string();

        assert!(err.contains("smtp_password"), "{err}");
        let raw = fs.read(Path::new("/repo/machines/old-box.toml")).unwrap();
        assert!(raw.contains("keychain"), "must still be the old provider");
    }

    #[test]
    fn switching_provider_records_it_once_everything_resolves() {
        let fs = fs();
        let ok = |_: ProviderKind, _: Option<&str>| Ok(());
        let cfg = set_provider(
            &fs,
            &local(),
            ProviderKind::OnePassword,
            Some("Private".into()),
            &ok,
        )
        .unwrap();

        assert_eq!(cfg.secret_provider, ProviderKind::OnePassword);
        assert_eq!(cfg.vault.as_deref(), Some("Private"));
    }

    #[test]
    fn onepassword_without_a_vault_is_refused_before_anything_is_checked() {
        let fs = fs();
        let never = |_: ProviderKind, _: Option<&str>| {
            panic!("verification must not run on an incomplete request")
        };
        let err = set_provider(&fs, &local(), ProviderKind::OnePassword, None, &never)
            .unwrap_err()
            .to_string();
        assert!(err.contains("vault"), "{err}");
    }

    #[test]
    fn leaving_onepassword_drops_the_vault() {
        let fs = fs();
        let ok = |_: ProviderKind, _: Option<&str>| Ok(());
        let cfg = set_provider(
            &fs,
            &local(),
            ProviderKind::Keychain,
            Some("Private".into()),
            &ok,
        )
        .unwrap();
        assert_eq!(cfg.vault, None, "a vault means nothing outside 1Password");
    }

    #[test]
    fn an_empty_remote_is_refused() {
        let err = set_remote(&FakeGit::new(), &local(), "  ")
            .unwrap_err()
            .to_string();
        assert!(err.contains("cannot be empty"), "{err}");
    }

    #[test]
    fn renaming_moves_a_deploy_key_and_repoints_ssh_config() {
        // The name is in the key's file name and in the IdentityFile line
        // that points at it. Missing either leaves ssh reaching for a file
        // that is no longer there.
        let fs = fs();
        fs.write(
            Path::new("/Users/test/.ssh/dotfix_old-box_ed25519"),
            "PRIVATE",
            0o600,
        )
        .unwrap();
        fs.write(
            Path::new("/Users/test/.ssh/dotfix_old-box_ed25519.pub"),
            "ssh-ed25519 AAAA dotfix@old-box",
            0o644,
        )
        .unwrap();
        fs.write(
            Path::new("/Users/test/.ssh/config"),
            "Host github.com-dotfix\n  IdentityFile /Users/test/.ssh/dotfix_old-box_ed25519\n",
            0o600,
        )
        .unwrap();

        rename_machine(&fs, &paths(), &local(), "new-box").unwrap();

        assert!(
            fs.read(Path::new("/Users/test/.ssh/dotfix_new-box_ed25519"))
                .is_ok()
        );
        assert!(
            fs.read(Path::new("/Users/test/.ssh/dotfix_old-box_ed25519"))
                .is_err()
        );
        let config = fs.read(Path::new("/Users/test/.ssh/config")).unwrap();
        assert!(config.contains("dotfix_new-box_ed25519"), "{config}");
        assert!(!config.contains("dotfix_old-box_ed25519"), "{config}");
    }

    #[test]
    fn renaming_without_a_deploy_key_touches_no_ssh_files() {
        // The common case: most machines reach the remote with the ssh setup
        // they already had.
        let fs = fs();
        rename_machine(&fs, &paths(), &local(), "new-box").unwrap();
        assert!(fs.read(Path::new("/Users/test/.ssh/config")).is_err());
    }
}

use std::path::Path;
use std::process::Command;

use crate::error::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Commit {
    pub hash: String,
    pub subject: String,
    pub date: String,
}

pub trait Git {
    /// Create a repository at `root` if there is none. Idempotent.
    fn init(&self, root: &Path) -> Result<()>;
    /// Whether any remote is configured. A data repository that only exists
    /// locally is a valid state — it just cannot be synced yet.
    fn has_remote(&self, root: &Path) -> Result<bool>;
    /// The `origin` remote's URL, or `None` when there is no remote.
    ///
    /// Needed to tell whether a repository already sitting at the target path
    /// is the one the user asked to clone. Reusing it without checking would
    /// turn "clone X" into "use whatever happens to be here".
    fn remote_url(&self, root: &Path) -> Result<Option<String>>;
    /// Point `origin` at `url`, adding the remote when there is none.
    fn set_remote_url(&self, root: &Path, url: &str) -> Result<()>;
    fn fetch(&self, root: &Path) -> Result<()>;
    /// Fast-forward only. Returns [`Error::Diverged`] when a merge would be
    /// required — dotfix never merges on the user's behalf.
    fn pull_ff_only(&self, root: &Path) -> Result<()>;
    fn is_diverged(&self, root: &Path) -> Result<bool>;
    /// Paths with uncommitted changes, as `git status --porcelain` reports
    /// them. Empty means there is nothing to commit.
    fn dirty_files(&self, root: &Path) -> Result<Vec<String>>;
    fn commit_all(&self, root: &Path, message: &str) -> Result<()>;
    fn push(&self, root: &Path) -> Result<()>;
    fn clone_to(&self, url: &str, root: &Path) -> Result<()>;
    fn log(&self, root: &Path, limit: usize) -> Result<Vec<Commit>>;
}

pub struct RealGit;

/// The environment a clone runs in, so that it fails instead of asking.
///
/// A GUI has no controlling terminal, so a question is a hang rather than a
/// question: launched from Finder there is nobody to answer, and launched
/// from a terminal git inherits that tty and blocks on "Username for
/// 'https://github.com':" — the command never returns and the wizard sits on
/// "Running…" forever. `GIT_TERMINAL_PROMPT=0` covers git's own prompts (a
/// missing credential, a private repository over https);
/// `BatchMode=yes` covers ssh's (a passphrase, an unknown host key), and
/// `ConnectTimeout` bounds the remaining way to hang, an unreachable host.
///
/// Clone only: the other commands run against a repository that is already
/// there, and `dotfix status` inheriting the user's own git configuration is
/// the point of it.
const CLONE_ENV: &[(&str, &str)] = &[
    ("GIT_TERMINAL_PROMPT", "0"),
    (
        "GIT_SSH_COMMAND",
        "ssh -o BatchMode=yes -o ConnectTimeout=5",
    ),
];

impl RealGit {
    fn run(root: &Path, args: &[&str]) -> Result<String> {
        Self::run_with_env(root, args, &[])
    }

    fn run_with_env(root: &Path, args: &[&str], env: &[(&str, &str)]) -> Result<String> {
        let mut command = Command::new("git");
        command.arg("-C").arg(root).args(args);
        for (key, value) in env {
            command.env(key, value);
        }
        let output = command.output().map_err(|e| Error::Command {
            cmd: format!("git {}", args.join(" ")),
            stderr: e.to_string(),
        })?;
        if !output.status.success() {
            return Err(Error::Command {
                cmd: format!("git {}", args.join(" ")),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            });
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}

impl Git for RealGit {
    fn init(&self, root: &Path) -> Result<()> {
        if root.join(".git").exists() {
            return Ok(());
        }
        std::fs::create_dir_all(root).map_err(|source| Error::Io {
            path: root.to_path_buf(),
            source,
        })?;
        Self::run(root, &["init", "--quiet", "--initial-branch=main"])?;

        // A brand-new Mac frequently has no global git identity. Failing the
        // very first commit dotfix makes would be a poor introduction, so fall
        // back to a repository-local one and leave any existing identity alone.
        if Self::run(root, &["config", "user.email"])
            .map(|v| v.trim().is_empty())
            .unwrap_or(true)
        {
            Self::run(root, &["config", "user.email", "dotfix@localhost"])?;
            Self::run(root, &["config", "user.name", "dotfix"])?;
        }
        Ok(())
    }

    fn has_remote(&self, root: &Path) -> Result<bool> {
        Ok(!Self::run(root, &["remote"])?.trim().is_empty())
    }

    fn remote_url(&self, root: &Path) -> Result<Option<String>> {
        // `config --get`, deliberately not `remote get-url`: the latter applies
        // `url.<base>.insteadOf` rewrites, so a user who routes GitHub over
        // `ssh://git@ssh.github.com:443/` — a common way through a firewall,
        // and the configuration on this maintainer's own Mac — would get that
        // rewritten form back. Comparing it against what they typed would then
        // report a mismatch for the very repository they just cloned. The
        // stored value is what the repository was configured with, which is
        // the question being asked.
        //
        // No `origin` makes git exit non-zero; that is the ordinary "no
        // remote" answer here, not a failure to report.
        Ok(Self::run(root, &["config", "--get", "remote.origin.url"])
            .ok()
            .map(|u| u.trim().to_string())
            .filter(|u| !u.is_empty()))
    }

    fn set_remote_url(&self, root: &Path, url: &str) -> Result<()> {
        // `set-url` fails when there is no remote yet, which is the ordinary
        // state of a repository built by `--set-up-new`; `add` covers it.
        if Self::run(root, &["remote", "set-url", "origin", url]).is_err() {
            Self::run(root, &["remote", "add", "origin", url])?;
        }
        Ok(())
    }

    fn fetch(&self, root: &Path) -> Result<()> {
        Self::run(root, &["fetch", "--quiet"]).map(|_| ())
    }

    fn pull_ff_only(&self, root: &Path) -> Result<()> {
        if self.is_diverged(root)? {
            return Err(Error::Diverged);
        }
        Self::run(root, &["pull", "--ff-only", "--quiet"]).map(|_| ())
    }

    fn is_diverged(&self, root: &Path) -> Result<bool> {
        // "<behind> <ahead>" relative to the upstream branch.
        let raw = Self::run(
            root,
            &["rev-list", "--left-right", "--count", "@{u}...HEAD"],
        )?;
        let mut parts = raw.split_whitespace();
        let behind: u32 = parts.next().unwrap_or("0").parse().unwrap_or(0);
        let ahead: u32 = parts.next().unwrap_or("0").parse().unwrap_or(0);
        Ok(behind > 0 && ahead > 0)
    }

    fn dirty_files(&self, root: &Path) -> Result<Vec<String>> {
        Ok(Self::run(root, &["status", "--porcelain"])?
            .lines()
            .filter_map(|l| l.get(3..).map(str::to_string))
            .collect())
    }

    fn commit_all(&self, root: &Path, message: &str) -> Result<()> {
        Self::run(root, &["add", "-A"])?;
        Self::run(root, &["commit", "-m", message]).map(|_| ())
    }

    fn push(&self, root: &Path) -> Result<()> {
        Self::run(root, &["push", "--quiet"]).map(|_| ())
    }

    fn clone_to(&self, url: &str, root: &Path) -> Result<()> {
        let parent = root.parent().unwrap_or(Path::new("."));
        let name = root
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("dotfiles");
        Self::run_with_env(parent, &["clone", "--quiet", url, name], CLONE_ENV).map(|_| ())
    }

    fn log(&self, root: &Path, limit: usize) -> Result<Vec<Commit>> {
        let n = format!("-{limit}");
        let raw = Self::run(
            root,
            &["log", &n, "--pretty=format:%h%x1f%s%x1f%ad", "--date=short"],
        )?;
        Ok(raw
            .lines()
            .filter_map(|line| {
                let mut f = line.split('\u{1f}');
                Some(Commit {
                    hash: f.next()?.to_string(),
                    subject: f.next()?.to_string(),
                    date: f.next()?.to_string(),
                })
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use super::*;

    /// A real git repository in a temp dir. These tests drive `RealGit`
    /// against actual git — the parsing in `is_diverged` and `log` is real
    /// logic, and a fake cannot prove it reads git's output correctly.
    fn repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        RealGit.init(dir.path()).unwrap();
        std::fs::write(dir.path().join("a.txt"), "one\n").unwrap();
        RealGit.commit_all(dir.path(), "first").unwrap();
        dir
    }

    fn run(root: &Path, args: &[&str]) {
        let out = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// Whether a clone can prompt is not observable from a test: it needs a
    /// controlling terminal and a remote that asks for credentials, and the
    /// failure mode being guarded against is precisely that the command
    /// never returns. What can be pinned is the environment the clone is
    /// given, which is where the guarantee lives.
    #[test]
    fn a_clone_runs_with_every_prompt_turned_off() {
        let env = |key: &str| {
            CLONE_ENV
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| *v)
                .unwrap_or_else(|| panic!("{key} must be set for a clone: {CLONE_ENV:?}"))
        };
        assert_eq!(
            env("GIT_TERMINAL_PROMPT"),
            "0",
            "without this git blocks on `Username for ...` and never returns"
        );
        let ssh = env("GIT_SSH_COMMAND");
        assert!(
            ssh.contains("BatchMode=yes"),
            "ssh must not be able to ask for a passphrase or a host key: {ssh}"
        );
        assert!(
            ssh.contains("ConnectTimeout="),
            "an unreachable host is the other way to hang: {ssh}"
        );
    }

    #[test]
    fn init_creates_a_repository_with_a_usable_identity() {
        let dir = tempfile::tempdir().unwrap();
        RealGit.init(dir.path()).unwrap();

        assert!(dir.path().join(".git").exists());
        // A brand-new Mac often has no global git identity; committing must
        // still work rather than failing on the very first command.
        std::fs::write(dir.path().join("x"), "x").unwrap();
        RealGit.commit_all(dir.path(), "works").unwrap();
    }

    #[test]
    fn init_is_idempotent() {
        let dir = repo();
        RealGit.init(dir.path()).unwrap();
        assert_eq!(RealGit.log(dir.path(), 10).unwrap().len(), 1);
    }

    #[test]
    fn a_repository_without_a_remote_reports_none() {
        let dir = repo();
        assert!(!RealGit.has_remote(dir.path()).unwrap());
    }

    #[test]
    fn a_repository_with_a_remote_reports_one() {
        let dir = repo();
        let other = tempfile::tempdir().unwrap();
        run(
            dir.path(),
            &["remote", "add", "origin", other.path().to_str().unwrap()],
        );
        assert!(RealGit.has_remote(dir.path()).unwrap());
    }

    #[test]
    fn log_parses_hash_subject_and_date() {
        let dir = repo();
        let commits = RealGit.log(dir.path(), 10).unwrap();

        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].subject, "first");
        assert_eq!(commits[0].hash.len(), 7);
        assert_eq!(commits[0].date.len(), 10, "expected YYYY-MM-DD");
    }

    #[test]
    fn log_returns_newest_first_and_respects_the_limit() {
        let dir = repo();
        std::fs::write(dir.path().join("b.txt"), "two\n").unwrap();
        RealGit.commit_all(dir.path(), "second").unwrap();

        let all = RealGit.log(dir.path(), 10).unwrap();
        assert_eq!(all[0].subject, "second");
        assert_eq!(RealGit.log(dir.path(), 1).unwrap().len(), 1);
    }

    #[test]
    fn a_subject_containing_the_field_separator_does_not_break_parsing() {
        let dir = repo();
        std::fs::write(dir.path().join("c.txt"), "three\n").unwrap();
        RealGit
            .commit_all(dir.path(), "feat: add a | pipe and a : colon")
            .unwrap();
        assert_eq!(
            RealGit.log(dir.path(), 1).unwrap()[0].subject,
            "feat: add a | pipe and a : colon"
        );
    }

    #[test]
    fn a_fresh_clone_has_not_diverged() {
        let origin = repo();
        let workdir = tempfile::tempdir().unwrap();
        let clone = workdir.path().join("clone");
        RealGit
            .clone_to(origin.path().to_str().unwrap(), &clone)
            .unwrap();

        assert!(!RealGit.is_diverged(&clone).unwrap());
        RealGit.pull_ff_only(&clone).unwrap();
    }

    #[test]
    fn being_only_behind_is_not_divergence_and_fast_forwards() {
        let origin = repo();
        let workdir = tempfile::tempdir().unwrap();
        let clone = workdir.path().join("clone");
        RealGit
            .clone_to(origin.path().to_str().unwrap(), &clone)
            .unwrap();

        std::fs::write(origin.path().join("upstream.txt"), "new\n").unwrap();
        RealGit
            .commit_all(origin.path(), "upstream change")
            .unwrap();

        RealGit.fetch(&clone).unwrap();
        assert!(!RealGit.is_diverged(&clone).unwrap());
        RealGit.pull_ff_only(&clone).unwrap();
        assert!(clone.join("upstream.txt").exists());
    }

    #[test]
    fn both_sides_moved_is_divergence_and_pull_refuses() {
        let origin = repo();
        let workdir = tempfile::tempdir().unwrap();
        let clone = workdir.path().join("clone");
        RealGit
            .clone_to(origin.path().to_str().unwrap(), &clone)
            .unwrap();

        std::fs::write(origin.path().join("theirs.txt"), "theirs\n").unwrap();
        RealGit.commit_all(origin.path(), "theirs").unwrap();
        std::fs::write(clone.join("mine.txt"), "mine\n").unwrap();
        RealGit.commit_all(&clone, "mine").unwrap();
        RealGit.fetch(&clone).unwrap();

        assert!(
            RealGit.is_diverged(&clone).unwrap(),
            "behind AND ahead must count as diverged"
        );
        assert!(
            matches!(RealGit.pull_ff_only(&clone), Err(Error::Diverged)),
            "dotfix must never merge on the user's behalf"
        );
    }

    #[test]
    fn it_reports_the_origin_url_and_none_without_one() {
        let dir = repo();
        let root = dir.path();

        assert_eq!(
            RealGit.remote_url(root).unwrap(),
            None,
            "a fresh repository has no origin"
        );

        run(
            root,
            &[
                "remote",
                "add",
                "origin",
                "git@github.com:example/dotfiles.git",
            ],
        );
        // Reported verbatim even though this machine's git config rewrites
        // `git@github.com:` to `ssh://git@ssh.github.com:443/` — the test that
        // first caught it ran on exactly such a machine.
        assert_eq!(
            RealGit.remote_url(root).unwrap().as_deref(),
            Some("git@github.com:example/dotfiles.git")
        );
    }

    #[test]
    fn setting_the_remote_adds_it_when_there_is_none_and_replaces_it_after() {
        // A repository from `--set-up-new` has no remote at all, so the
        // setter cannot assume `set-url` will work.
        let dir = repo();
        let root = dir.path();
        assert_eq!(RealGit.remote_url(root).unwrap(), None);

        RealGit
            .set_remote_url(root, "git@github.com:a/b.git")
            .unwrap();
        assert_eq!(
            RealGit.remote_url(root).unwrap().as_deref(),
            Some("git@github.com:a/b.git")
        );

        RealGit
            .set_remote_url(root, "git@github.com:c/d.git")
            .unwrap();
        assert_eq!(
            RealGit.remote_url(root).unwrap().as_deref(),
            Some("git@github.com:c/d.git")
        );
    }
}

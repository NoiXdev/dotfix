//! In-memory test doubles. Available to downstream crates via the `fakes`
//! feature so that CLI integration tests can run without Homebrew or git.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::ports::{Brew, Commit, Fsys, Git};

#[derive(Default)]
pub struct FakeFsys {
    files: RefCell<BTreeMap<PathBuf, String>>,
    modes: RefCell<BTreeMap<PathBuf, u32>>,
    links: RefCell<BTreeMap<PathBuf, PathBuf>>,
}

impl FakeFsys {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from<const N: usize>(entries: [(&str, &str); N]) -> Self {
        let fs = Self::new();
        for (path, contents) in entries {
            fs.write(Path::new(path), contents, 0o644).unwrap();
        }
        fs
    }

    pub fn mode_of(&self, path: &Path) -> Option<u32> {
        self.modes.borrow().get(path).copied()
    }

    /// Snapshot of every file, for assertions.
    pub fn snapshot(&self) -> BTreeMap<PathBuf, String> {
        self.files.borrow().clone()
    }
}

impl Fsys for FakeFsys {
    fn read(&self, path: &Path) -> Result<String> {
        self.files
            .borrow()
            .get(path)
            .cloned()
            .ok_or_else(|| Error::Io {
                path: path.to_path_buf(),
                source: std::io::Error::from(std::io::ErrorKind::NotFound),
            })
    }

    fn write(&self, path: &Path, contents: &str, mode: u32) -> Result<()> {
        self.files
            .borrow_mut()
            .insert(path.to_path_buf(), contents.to_string());
        self.modes.borrow_mut().insert(path.to_path_buf(), mode);
        Ok(())
    }

    fn exists(&self, path: &Path) -> bool {
        self.files.borrow().contains_key(path) || self.links.borrow().contains_key(path)
    }

    fn list_dir(&self, path: &Path) -> Result<Vec<PathBuf>> {
        Ok(self
            .files
            .borrow()
            .keys()
            .filter(|p| p.starts_with(path))
            .cloned()
            .collect())
    }

    fn remove(&self, path: &Path) -> Result<()> {
        self.files.borrow_mut().remove(path);
        self.links.borrow_mut().remove(path);
        Ok(())
    }

    fn remove_dir_all(&self, path: &Path) -> Result<()> {
        self.files.borrow_mut().retain(|p, _| !p.starts_with(path));
        self.modes.borrow_mut().retain(|p, _| !p.starts_with(path));
        self.links.borrow_mut().retain(|p, _| !p.starts_with(path));
        Ok(())
    }

    fn create_dir_all(&self, _path: &Path) -> Result<()> {
        Ok(())
    }

    fn symlink(&self, source: &Path, target: &Path) -> Result<()> {
        self.links
            .borrow_mut()
            .insert(target.to_path_buf(), source.to_path_buf());
        Ok(())
    }

    fn read_link(&self, path: &Path) -> Result<PathBuf> {
        self.links
            .borrow()
            .get(path)
            .cloned()
            .ok_or_else(|| Error::Io {
                path: path.to_path_buf(),
                source: std::io::Error::from(std::io::ErrorKind::NotFound),
            })
    }
}

#[derive(Default)]
pub struct FakeBrew {
    leaves: Vec<String>,
    casks: Vec<String>,
    uses: BTreeMap<String, Vec<String>>,
    installed: RefCell<Vec<String>>,
    uninstalled: RefCell<Vec<String>>,
    fail: bool,
    /// What the *second* and later `leaves()` calls return. Models a cold
    /// Homebrew, whose first reading can name a dependency as a leaf because
    /// it has not yet resolved the tap of the package that requires it.
    leaves_on_reread: Option<Vec<String>>,
    reads: RefCell<usize>,
}

impl FakeBrew {
    pub fn new<const A: usize, const B: usize>(leaves: [&str; A], casks: [&str; B]) -> Self {
        Self {
            leaves: leaves.iter().map(|s| s.to_string()).collect(),
            casks: casks.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        }
    }

    /// Make the second and later `leaves()` calls return something else.
    pub fn with_reread<const N: usize>(mut self, leaves: [&str; N]) -> Self {
        self.leaves_on_reread = Some(leaves.iter().map(|s| s.to_string()).collect());
        self
    }

    /// Same as [`FakeBrew::new`] but from owned vectors, for callers that build
    /// the lists at runtime.
    pub fn seeded(leaves: Vec<String>, casks: Vec<String>) -> Self {
        Self {
            leaves,
            casks,
            ..Default::default()
        }
    }

    /// A brew whose queries fail, for testing cleanup paths.
    pub fn failing() -> Self {
        Self {
            fail: true,
            ..Default::default()
        }
    }

    pub fn with_uses<const N: usize>(mut self, formula: &str, users: [&str; N]) -> Self {
        self.uses.insert(
            formula.to_string(),
            users.iter().map(|s| s.to_string()).collect(),
        );
        self
    }

    pub fn installed(&self) -> Vec<String> {
        self.installed.borrow().clone()
    }

    pub fn uninstalled(&self) -> Vec<String> {
        self.uninstalled.borrow().clone()
    }
}

impl Brew for FakeBrew {
    fn leaves(&self) -> Result<Vec<String>> {
        if self.fail {
            return Err(Error::Command {
                cmd: "brew leaves".into(),
                stderr: "fake failure".into(),
            });
        }
        let first = {
            let mut n = self.reads.borrow_mut();
            *n += 1;
            *n == 1
        };
        Ok(match (&self.leaves_on_reread, first) {
            (Some(later), false) => later.clone(),
            _ => self.leaves.clone(),
        })
    }

    fn casks(&self) -> Result<Vec<String>> {
        if self.fail {
            return Err(Error::Command {
                cmd: "brew casks".into(),
                stderr: "fake failure".into(),
            });
        }
        Ok(self.casks.clone())
    }

    fn uses_installed(&self, formula: &str) -> Result<Vec<String>> {
        Ok(self.uses.get(formula).cloned().unwrap_or_default())
    }

    fn install(&self, name: &str, _cask: bool) -> Result<()> {
        self.installed.borrow_mut().push(name.to_string());
        Ok(())
    }

    fn uninstall(&self, name: &str, _cask: bool) -> Result<()> {
        self.uninstalled.borrow_mut().push(name.to_string());
        Ok(())
    }
}

pub struct FakeGit {
    pub diverged: bool,
    /// Whether the fake repository has a remote. Defaults to true so existing
    /// tests keep exercising the sync path.
    pub no_remote: bool,
    /// What `remote_url` reports when there is a remote.
    pub remote_url: String,
    /// Overwrites the above once `set_remote_url` has been called.
    pub remote: RefCell<Option<String>>,
    /// What `dirty_files` reports.
    pub dirty: Vec<String>,
    pub commits: Vec<Commit>,
    pub calls: RefCell<Vec<String>>,
}

/// Hand-written rather than derived: `remote_url` must default to a plausible
/// URL, not the empty string, or a test that forgets to set it would assert
/// against a value no real repository ever reports.
impl Default for FakeGit {
    fn default() -> Self {
        Self {
            diverged: false,
            no_remote: false,
            remote_url: "git@github.com:example-user/dotfiles.git".to_string(),
            remote: RefCell::new(None),
            dirty: Vec::new(),
            commits: Vec::new(),
            calls: RefCell::new(Vec::new()),
        }
    }
}

impl FakeGit {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn diverged() -> Self {
        Self {
            diverged: true,
            ..Default::default()
        }
    }

    /// A repository that exists only locally, with no remote configured.
    pub fn without_remote() -> Self {
        Self {
            no_remote: true,
            ..Default::default()
        }
    }

    pub fn calls(&self) -> Vec<String> {
        self.calls.borrow().clone()
    }

    fn record(&self, what: &str) {
        self.calls.borrow_mut().push(what.to_string());
    }
}

impl Git for FakeGit {
    fn init(&self, _root: &Path) -> Result<()> {
        self.record("init");
        Ok(())
    }

    fn has_remote(&self, _root: &Path) -> Result<bool> {
        Ok(!self.no_remote)
    }

    fn set_remote_url(&self, _root: &Path, url: &str) -> Result<()> {
        self.record(&format!("set_remote_url {url}"));
        *self.remote.borrow_mut() = Some(url.to_string());
        Ok(())
    }

    fn remote_url(&self, _root: &Path) -> Result<Option<String>> {
        Ok(match self.remote.borrow().clone() {
            Some(set) => Some(set),
            None if self.no_remote => None,
            None => Some(self.remote_url.clone()),
        })
    }

    fn fetch(&self, _root: &Path) -> Result<()> {
        self.record("fetch");
        Ok(())
    }

    fn pull_ff_only(&self, _root: &Path) -> Result<()> {
        self.record("pull");
        if self.diverged {
            return Err(Error::Diverged);
        }
        Ok(())
    }

    fn is_diverged(&self, _root: &Path) -> Result<bool> {
        Ok(self.diverged)
    }

    fn dirty_files(&self, _root: &Path) -> Result<Vec<String>> {
        Ok(self.dirty.clone())
    }

    fn commit_all(&self, _root: &Path, message: &str) -> Result<()> {
        self.record(&format!("commit:{message}"));
        Ok(())
    }

    fn push(&self, _root: &Path) -> Result<()> {
        self.record("push");
        Ok(())
    }

    fn clone_to(&self, url: &str, _root: &Path) -> Result<()> {
        self.record(&format!("clone:{url}"));
        Ok(())
    }

    fn log(&self, _root: &Path, limit: usize) -> Result<Vec<Commit>> {
        Ok(self.commits.iter().take(limit).cloned().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_fsys_round_trips_a_write() {
        let fs = FakeFsys::new();
        let path = Path::new("/repo/a.txt");
        assert!(!fs.exists(path));
        fs.write(path, "hello", 0o644).unwrap();
        assert!(fs.exists(path));
        assert_eq!(fs.read(path).unwrap(), "hello");
        assert_eq!(fs.mode_of(path), Some(0o644));
    }

    #[test]
    fn fake_fsys_lists_recursively_and_sorted() {
        let fs = FakeFsys::from([
            ("/repo/sets/core/set.toml", "a"),
            ("/repo/sets/web/set.toml", "b"),
            ("/repo/dotfix.toml", "c"),
        ]);
        let found = fs.list_dir(Path::new("/repo/sets")).unwrap();
        assert_eq!(
            found,
            vec![
                PathBuf::from("/repo/sets/core/set.toml"),
                PathBuf::from("/repo/sets/web/set.toml"),
            ]
        );
    }

    #[test]
    fn remove_dir_all_removes_files_nested_several_levels_under_the_prefix() {
        let fs = FakeFsys::from([
            ("/repo/dotfix.toml", "a"),
            ("/repo/sets/core/shell/10-path.zsh", "b"),
            ("/repo/.git/objects/deep/nested/blob", "c"),
        ]);
        fs.remove_dir_all(Path::new("/repo")).unwrap();

        assert!(fs.snapshot().is_empty(), "found {:?}", fs.snapshot());
    }

    #[test]
    fn remove_dir_all_leaves_files_outside_the_prefix_untouched() {
        let fs = FakeFsys::from([
            ("/repo/dotfix.toml", "a"),
            ("/repo-backup/dotfix.toml", "b"),
            ("/other/file.txt", "c"),
        ]);
        fs.remove_dir_all(Path::new("/repo")).unwrap();

        let remaining: Vec<_> = fs.snapshot().keys().cloned().collect();
        assert_eq!(
            remaining,
            vec![
                PathBuf::from("/other/file.txt"),
                PathBuf::from("/repo-backup/dotfix.toml"),
            ]
        );
    }

    #[test]
    fn remove_dir_all_also_removes_symlinks_under_the_prefix() {
        let fs = FakeFsys::new();
        fs.symlink(Path::new("/elsewhere"), Path::new("/repo/link"))
            .unwrap();
        fs.remove_dir_all(Path::new("/repo")).unwrap();

        assert!(!fs.exists(Path::new("/repo/link")));
    }

    #[test]
    fn remove_dir_all_succeeds_on_a_path_that_does_not_exist() {
        let fs = FakeFsys::new();
        assert!(fs.remove_dir_all(Path::new("/nowhere")).is_ok());
    }

    #[test]
    fn fake_brew_records_installs() {
        let brew = FakeBrew::new(["alpha"], ["charlie"]);
        brew.install("bravo", false).unwrap();
        assert_eq!(brew.installed(), vec!["bravo".to_string()]);
        assert_eq!(brew.leaves().unwrap(), vec!["alpha".to_string()]);
    }

    #[test]
    fn fake_brew_reports_configured_dependents() {
        let brew = FakeBrew::new(["alpha"], []).with_uses("alpha", ["bravo"]);
        assert_eq!(brew.uses_installed("alpha").unwrap(), vec!["bravo"]);
        assert!(brew.uses_installed("charlie").unwrap().is_empty());
    }
}

#[derive(Default)]
pub struct FakeExec {
    responses: BTreeMap<String, String>,
    errors: BTreeMap<String, String>,
    calls: RefCell<Vec<String>>,
}

impl FakeExec {
    pub fn new<const N: usize>(responses: [(&str, &str); N]) -> Self {
        Self {
            responses: responses
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            calls: RefCell::new(Vec::new()),
            errors: BTreeMap::new(),
        }
    }

    pub fn with_error(mut self, key: &str, stderr: &str) -> Self {
        self.errors.insert(key.to_string(), stderr.to_string());
        self
    }

    pub fn calls(&self) -> Vec<String> {
        self.calls.borrow().clone()
    }

    /// Records the call and resolves it against the configured responses and
    /// errors, keyed by `program` and `args` only. Shared by `run` and
    /// `run_with_stdin` so that a caller's stdin — which may be a secret —
    /// never enters `calls()` or the lookup key.
    fn dispatch(&self, program: &str, args: &[&str]) -> Result<String> {
        let key = format!("{program} {}", args.join(" "));
        self.calls.borrow_mut().push(key.clone());
        if let Some(stderr) = self.errors.get(&key) {
            return Err(Error::Command {
                cmd: key,
                stderr: stderr.clone(),
            });
        }
        self.responses
            .get(&key)
            .cloned()
            .ok_or_else(|| Error::Command {
                cmd: key,
                stderr: "no fake response configured".to_string(),
            })
    }
}

impl crate::ports::Exec for FakeExec {
    fn run(&self, program: &str, args: &[&str]) -> Result<String> {
        self.dispatch(program, args)
    }

    fn run_with_stdin(&self, program: &str, args: &[&str], _stdin: &str) -> Result<String> {
        self.dispatch(program, args)
    }
}

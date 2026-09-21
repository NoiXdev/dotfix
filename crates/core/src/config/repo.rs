use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::{Desired, Fragment, MachineConfig, ResolvedFile, SetConfig};
use crate::error::{Error, Result};

/// The repository layout this build understands. Bump it only together with
/// a migration, and only when the change is not backwards compatible.
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct RepoConfig {
    pub schema_version: u32,
}

/// The parsed data repository.
#[derive(Debug, Clone)]
pub struct Repo {
    pub root: PathBuf,
    pub config: RepoConfig,
    pub sets: BTreeMap<String, SetConfig>,
    pub machines: BTreeMap<String, MachineConfig>,
}

impl Repo {
    /// Parse a repository from an already-collected map of path → contents.
    /// Keeping this separate from I/O makes the whole loader testable without
    /// a filesystem.
    pub fn parse(root: PathBuf, files: &BTreeMap<PathBuf, String>) -> Result<Repo> {
        let cfg_path = root.join("dotfix.toml");
        let cfg_raw = files
            .get(&cfg_path)
            .ok_or_else(|| Error::Config(format!("missing {}", cfg_path.display())))?;
        let config: RepoConfig = parse_toml(&cfg_path, cfg_raw)?;

        // Checked before anything else is read: a repository written by a newer
        // dotfix may contain files this build cannot parse, and "update dotfix"
        // is a far more useful message than a TOML error. Silently
        // misinterpreting a repository is the worst outcome for a tool that
        // runs `brew uninstall`.
        if config.schema_version != SCHEMA_VERSION {
            return Err(Error::Config(if config.schema_version > SCHEMA_VERSION {
                format!(
                    "repository uses schema version {}, this dotfix understands {SCHEMA_VERSION} \
                     — update dotfix",
                    config.schema_version
                )
            } else {
                format!(
                    "repository uses schema version {}, which this dotfix does not support \
                     (expected {SCHEMA_VERSION})",
                    config.schema_version
                )
            }));
        }

        let mut sets: BTreeMap<String, SetConfig> = BTreeMap::new();
        let mut machines: BTreeMap<String, MachineConfig> = BTreeMap::new();

        for (path, raw) in files {
            if let Some(name) = set_name(&root, path) {
                sets.insert(name, parse_toml(path, raw)?);
            } else if let Some(name) = machine_name(&root, path) {
                machines.insert(name, parse_toml(path, raw)?);
            }
        }

        for (machine, cfg) in &machines {
            for set in &cfg.sets {
                if !sets.contains_key(set) {
                    return Err(Error::UnknownSet {
                        set: set.clone(),
                        machine: machine.clone(),
                    });
                }
            }
        }

        Ok(Repo {
            root,
            config,
            sets,
            machines,
        })
    }

    /// Read every `.toml` under `root` and parse it.
    pub fn load(fs: &dyn crate::ports::Fsys, root: &Path) -> Result<Repo> {
        let mut files = BTreeMap::new();
        for path in fs.list_dir(root)? {
            if path.extension().and_then(|e| e.to_str()) == Some("toml") {
                let contents = fs.read(&path)?;
                files.insert(path, contents);
            }
        }
        Repo::parse(root.to_path_buf(), &files)
    }

    /// Merge the machine's active sets into a single [`Desired`] state.
    pub fn resolve(
        &self,
        fs: &dyn crate::ports::Fsys,
        machine: &str,
        home: &Path,
    ) -> Result<Desired> {
        let cfg = self
            .machines
            .get(machine)
            .ok_or_else(|| Error::UnknownMachine(machine.to_string()))?;

        let mut desired = Desired {
            machine: machine.to_string(),
            ..Default::default()
        };
        // Everything the other sets declare, so a package left behind by a set
        // that was switched off can say where it came from instead of looking
        // like it belongs nowhere.
        for (set_name, set) in &self.sets {
            if cfg.sets.contains(set_name) {
                continue;
            }
            for name in &set.packages.brew {
                desired
                    .inactive_brew
                    .entry(name.clone())
                    .or_default()
                    .push(set_name.clone());
            }
            for name in &set.packages.cask {
                desired
                    .inactive_cask
                    .entry(name.clone())
                    .or_default()
                    .push(set_name.clone());
            }
        }

        // Rank by position in the machine's `sets` list: author-controlled and
        // deterministic, used to break ties between equal numeric prefixes.
        let mut fragments: Vec<(u32, usize, String, Fragment)> = Vec::new();

        for (rank, set_name) in cfg.sets.iter().enumerate() {
            let set = &self.sets[set_name];
            desired.brew.extend(set.packages.brew.iter().cloned());
            desired.cask.extend(set.packages.cask.iter().cloned());

            let set_dir = self.root.join("sets").join(set_name);

            for file in &set.files {
                desired.files.push(ResolvedFile {
                    set: set_name.clone(),
                    source: set_dir.join(&file.source),
                    target: expand_home(&file.target, home),
                    mode: file.mode,
                });
            }

            for path in fs.list_dir(&set_dir.join("shell"))? {
                if path.extension().and_then(|e| e.to_str()) != Some("zsh") {
                    continue;
                }
                let name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or_default()
                    .to_string();
                fragments.push((
                    numeric_prefix(&name),
                    rank,
                    name.clone(),
                    Fragment {
                        set: set_name.clone(),
                        name,
                        path,
                    },
                ));
            }
        }

        fragments.sort_by(|a, b| (a.0, a.1, &a.2).cmp(&(b.0, b.1, &b.2)));
        desired.fragments = fragments.into_iter().map(|(_, _, _, f)| f).collect();

        Ok(desired)
    }
}

/// `"20-alias.zsh"` → `20`. Fragments without a numeric prefix sort last.
fn numeric_prefix(name: &str) -> u32 {
    name.split('-')
        .next()
        .and_then(|p| p.parse().ok())
        .unwrap_or(u32::MAX)
}

fn expand_home(target: &str, home: &Path) -> PathBuf {
    match target.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None => PathBuf::from(target),
    }
}

fn parse_toml<T: serde::de::DeserializeOwned>(path: &Path, raw: &str) -> Result<T> {
    toml::from_str(raw).map_err(|source| Error::Toml {
        path: path.to_path_buf(),
        source,
    })
}

/// `<root>/sets/<name>/set.toml` → `Some(name)`
fn set_name(root: &Path, path: &Path) -> Option<String> {
    let rest = path.strip_prefix(root.join("sets")).ok()?;
    let parts: Vec<_> = rest.components().collect();
    if parts.len() != 2 || parts[1].as_os_str() != "set.toml" {
        return None;
    }
    Some(parts[0].as_os_str().to_str()?.to_string())
}

/// `<root>/machines/<name>.toml` → `Some(name)`
fn machine_name(root: &Path, path: &Path) -> Option<String> {
    let rest = path.strip_prefix(root.join("machines")).ok()?;
    if rest.components().count() != 1 || rest.extension()?.to_str()? != "toml" {
        return None;
    }
    Some(rest.file_stem()?.to_str()?.to_string())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use super::*;

    fn files() -> BTreeMap<PathBuf, String> {
        BTreeMap::from([
            (
                PathBuf::from("/repo/dotfix.toml"),
                "schema_version = 1\n".to_string(),
            ),
            (
                PathBuf::from("/repo/sets/core/set.toml"),
                "[packages]\nbrew = [\"alpha\"]\n".to_string(),
            ),
            (
                PathBuf::from("/repo/machines/box-one.toml"),
                "sets = [\"core\"]\n".to_string(),
            ),
        ])
    }

    #[test]
    fn parses_sets_and_machines() {
        let repo = Repo::parse(PathBuf::from("/repo"), &files()).unwrap();
        assert_eq!(repo.config.schema_version, 1);
        assert_eq!(repo.sets.len(), 1);
        assert_eq!(repo.sets["core"].packages.brew, vec!["alpha"]);
        assert_eq!(repo.machines["box-one"].sets, vec!["core"]);
    }

    #[test]
    fn rejects_a_machine_referencing_an_unknown_set() {
        let mut f = files();
        f.insert(
            PathBuf::from("/repo/machines/box-two.toml"),
            "sets = [\"nope\"]\n".to_string(),
        );
        let err = Repo::parse(PathBuf::from("/repo"), &f).unwrap_err();
        assert!(matches!(
            err,
            Error::UnknownSet { ref set, ref machine } if set == "nope" && machine == "box-two"
        ));
    }

    #[test]
    fn reports_the_path_of_invalid_toml() {
        let mut f = files();
        f.insert(
            PathBuf::from("/repo/sets/core/set.toml"),
            "this is not toml =".to_string(),
        );
        let err = Repo::parse(PathBuf::from("/repo"), &f).unwrap_err();
        assert!(err.to_string().contains("/repo/sets/core/set.toml"));
    }

    #[test]
    fn loads_a_repository_from_a_filesystem() {
        use crate::ports::fake::FakeFsys;

        let fs = FakeFsys::from([
            ("/repo/dotfix.toml", "schema_version = 1\n"),
            (
                "/repo/sets/core/set.toml",
                "[packages]\nbrew = [\"alpha\"]\n",
            ),
            ("/repo/sets/core/shell/10-path.zsh", "export A=1\n"),
            ("/repo/machines/box-one.toml", "sets = [\"core\"]\n"),
        ]);

        let repo = Repo::load(&fs, Path::new("/repo")).unwrap();
        assert_eq!(repo.sets.len(), 1);
        assert_eq!(repo.machines.len(), 1);
    }

    #[test]
    fn accepts_the_schema_version_it_understands() {
        let repo = Repo::parse(PathBuf::from("/repo"), &files()).unwrap();
        assert_eq!(repo.config.schema_version, SCHEMA_VERSION);
    }

    #[test]
    fn a_newer_schema_tells_the_user_to_update_dotfix() {
        let mut f = files();
        f.insert(
            PathBuf::from("/repo/dotfix.toml"),
            "schema_version = 2\n".to_string(),
        );
        let err = Repo::parse(PathBuf::from("/repo"), &f).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("schema version 2"), "got: {msg}");
        assert!(msg.contains("understands 1"), "got: {msg}");
        assert!(msg.contains("update dotfix"), "got: {msg}");
    }

    #[test]
    fn an_unknown_older_schema_is_refused_rather_than_guessed_at() {
        let mut f = files();
        f.insert(
            PathBuf::from("/repo/dotfix.toml"),
            "schema_version = 0\n".to_string(),
        );
        let err = Repo::parse(PathBuf::from("/repo"), &f).unwrap_err();
        assert!(err.to_string().contains("schema version 0"));
    }

    #[test]
    fn the_version_is_checked_before_anything_else_is_parsed() {
        // A repository written by a newer dotfix may well contain set files
        // this version cannot read. The version error must win, so the user
        // gets "update dotfix" instead of a confusing parse error.
        let mut f = files();
        f.insert(
            PathBuf::from("/repo/dotfix.toml"),
            "schema_version = 99\n".to_string(),
        );
        f.insert(
            PathBuf::from("/repo/sets/core/set.toml"),
            "this is not valid toml =".to_string(),
        );
        let err = Repo::parse(PathBuf::from("/repo"), &f).unwrap_err();
        assert!(
            err.to_string().contains("update dotfix"),
            "version check must run first, got: {err}"
        );
    }
}

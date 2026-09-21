use std::path::PathBuf;

use serde::Serialize;

use crate::config::{MachineConfig, Repo, SetConfig};
use crate::error::{Error, Result};
use crate::ports::Fsys;

/// One set as offered to a user: its name, whether this machine uses it, and
/// the set's own description.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Entry {
    pub name: String,
    pub active: bool,
    pub description: String,
    /// What the set actually contains, so a front end can show it beside the
    /// switch that turns it on. Without this the only way to learn what a set
    /// brings is to open the repository — and the moment it matters most is
    /// the moment you are about to switch it off.
    pub brew: Vec<String>,
    pub cask: Vec<String>,
    /// Managed files: where they land, and the template they come from.
    pub files: Vec<SetFile>,
    /// Shell fragments, in the order they are concatenated.
    pub fragments: Vec<SetFragment>,
    /// The set's own `set.toml`, repository-relative like the rest — a front
    /// end never receives an absolute path it could point elsewhere.
    pub config_path: String,
}

/// A managed file, as a set declares it. `source` is repository-relative so
/// a front end can ask to open it without being handed an absolute path it
/// could point anywhere.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SetFile {
    pub target: String,
    pub source: String,
}

/// A shell fragment. `name` is what decides its place in the assembled
/// `.zshrc`; `source` is repository-relative, like [`SetFile`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SetFragment {
    pub name: String,
    pub source: String,
}

/// Every set in the repository, in repository order, marked active or not.
///
/// `fs` is needed for the shell fragments: those are files on disk, not
/// entries in `set.toml`, so they can only be listed by looking.
pub fn list(repo: &Repo, fs: &dyn Fsys, machine: &str) -> Result<Vec<Entry>> {
    let cfg = machine_config(repo, machine)?;
    repo.sets
        .iter()
        .map(|(name, set)| {
            let dir = repo.root.join("sets").join(name);
            let mut fragments: Vec<SetFragment> = fs
                .list_dir(&dir.join("shell"))
                .unwrap_or_default()
                .iter()
                .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("zsh"))
                .filter_map(|p| {
                    p.file_name().and_then(|n| n.to_str()).map(|n| SetFragment {
                        name: n.to_string(),
                        source: format!("sets/{name}/shell/{n}"),
                    })
                })
                .collect();
            // Concatenation order is by file name, which is why they are
            // numbered; showing them in any other order would be a lie.
            fragments.sort_by(|a, b| a.name.cmp(&b.name));

            Ok(Entry {
                name: name.clone(),
                active: cfg.sets.contains(name),
                description: set.description.clone(),
                brew: set.packages.brew.clone(),
                cask: set.packages.cask.clone(),
                files: set
                    .files
                    .iter()
                    .map(|f| SetFile {
                        target: f.target.clone(),
                        source: format!("sets/{name}/{}", f.source),
                    })
                    .collect(),
                fragments,
                config_path: format!("sets/{name}/set.toml"),
            })
        })
        .collect()
}

/// Add or remove a package in a set.
///
/// Writes `set.toml` from the parsed structure, which is the only way to do
/// it — and means comments and field order in that file do not survive. That
/// is the price of editing it from anywhere but an editor, and it applies to
/// `dotfix adopt` today just as much.
pub fn edit_package(
    fs: &dyn Fsys,
    repo: &Repo,
    set: &str,
    package: &str,
    cask: bool,
    add: bool,
) -> Result<SetConfig> {
    let package = package.trim();
    if package.is_empty() {
        return Err(Error::Config("a package name cannot be empty".into()));
    }

    let mut cfg = repo
        .sets
        .get(set)
        .cloned()
        .ok_or_else(|| Error::Config(format!("unknown set `{set}`")))?;

    let list = if cask {
        &mut cfg.packages.cask
    } else {
        &mut cfg.packages.brew
    };

    if add {
        if list.iter().any(|p| p == package) {
            return Err(Error::Config(format!(
                "set `{set}` already has `{package}`"
            )));
        }
        list.push(package.to_string());
        list.sort();
    } else {
        let before = list.len();
        list.retain(|p| p != package);
        if list.len() == before {
            return Err(Error::Config(format!(
                "set `{set}` does not have `{package}`"
            )));
        }
    }

    // Edited in place rather than serialised from the struct: these files are
    // meant to be read and written by hand, and a comment someone left is not
    // a field, so it would not survive the round trip.
    let path = repo.root.join("sets").join(set).join("set.toml");
    let key = if cask { "cask" } else { "brew" };
    let raw = crate::config::edit::put_strings(&fs.read(&path)?, &["packages"], key, list)?;
    fs.write(&path, &raw, 0o644)?;
    Ok(cfg)
}

/// The machine's configuration with one set switched on or off. Not written —
/// the caller decides when to persist.
pub fn toggle(repo: &Repo, machine: &str, name: &str, on: bool) -> Result<MachineConfig> {
    if !repo.sets.contains_key(name) {
        return Err(Error::Config(format!("unknown set `{name}`")));
    }

    let mut cfg = machine_config(repo, machine)?.clone();
    if on {
        if !cfg.sets.iter().any(|s| s == name) {
            cfg.sets.push(name.to_string());
        }
    } else {
        cfg.sets.retain(|s| s != name);
    }
    Ok(cfg)
}

pub fn save(fs: &dyn Fsys, repo: &Repo, machine: &str, cfg: &MachineConfig) -> Result<PathBuf> {
    let path = repo.root.join("machines").join(format!("{machine}.toml"));
    let raw = toml::to_string_pretty(cfg)
        .map_err(|e| Error::Config(format!("serialising {}: {e}", path.display())))?;
    fs.write(&path, &raw, 0o644)?;
    Ok(path)
}

fn machine_config<'a>(repo: &'a Repo, machine: &str) -> Result<&'a MachineConfig> {
    repo.machines
        .get(machine)
        .ok_or_else(|| Error::UnknownMachine(machine.to_string()))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::config::Repo;
    use crate::ports::Fsys;
    use crate::ports::fake::FakeFsys;

    fn fs() -> FakeFsys {
        FakeFsys::from([
            ("/repo/dotfix.toml", "schema_version = 1\n"),
            (
                "/repo/sets/core/set.toml",
                "description = \"Base\"\n\n[[files]]\nsource = \"files/gitconfig\"\n\
                 target = \"~/.gitconfig\"\n\n[packages]\nbrew = [\"htop\"]\n\
                 cask = [\"figma\"]\n",
            ),
            ("/repo/sets/core/shell/20-b.zsh", "b\n"),
            ("/repo/sets/core/shell/10-a.zsh", "a\n"),
            ("/repo/sets/core/shell/notes.txt", "ignored\n"),
            (
                "/repo/sets/extra/set.toml",
                "description = \"Extra\"\n[packages]\nbrew = []\n",
            ),
            ("/repo/machines/box-one.toml", "sets = [\"core\"]\n"),
        ])
    }

    fn repo(fs: &FakeFsys) -> Repo {
        Repo::load(fs, Path::new("/repo")).unwrap()
    }

    #[test]
    fn list_marks_the_active_sets_and_keeps_descriptions() {
        let fs = fs();
        let entries = list(&repo(&fs), &fs, "box-one").unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "core");
        assert!(entries[0].active);
        assert_eq!(entries[0].description, "Base");
        assert_eq!(entries[1].name, "extra");
        assert!(!entries[1].active);
    }

    #[test]
    fn an_entry_carries_what_the_set_contains() {
        // The moment it matters most is the moment you are about to switch a
        // set off, and until now the only way to see that was the repository.
        let fs = fs();
        let core = list(&repo(&fs), &fs, "box-one").unwrap().remove(0);

        assert_eq!(core.brew, vec!["htop"]);
        assert_eq!(core.cask, vec!["figma"]);
        assert_eq!(core.files[0].target, "~/.gitconfig");
        assert_eq!(
            core.config_path, "sets/core/set.toml",
            "repository-relative, so opening it cannot escape the repository"
        );
    }

    #[test]
    fn fragments_are_listed_in_the_order_they_are_concatenated() {
        // Numbered file names *are* the order. Showing them in any other
        // would misrepresent what the assembled .zshrc does.
        let fs = fs();
        let core = list(&repo(&fs), &fs, "box-one").unwrap().remove(0);
        assert_eq!(
            core.fragments
                .iter()
                .map(|f| f.name.as_str())
                .collect::<Vec<_>>(),
            vec!["10-a.zsh", "20-b.zsh"]
        );
    }

    #[test]
    fn toggling_on_adds_the_set() {
        let fs = fs();
        let cfg = toggle(&repo(&fs), "box-one", "extra", true).unwrap();
        assert!(cfg.sets.contains(&"extra".to_string()));
    }

    #[test]
    fn toggling_on_twice_does_not_duplicate() {
        let fs = fs();
        let cfg = toggle(&repo(&fs), "box-one", "core", true).unwrap();
        assert_eq!(cfg.sets, vec!["core"]);
    }

    #[test]
    fn toggling_off_removes_the_set() {
        let fs = fs();
        let cfg = toggle(&repo(&fs), "box-one", "core", false).unwrap();
        assert!(cfg.sets.is_empty());
    }

    #[test]
    fn an_unknown_set_is_rejected() {
        let fs = fs();
        let err = toggle(&repo(&fs), "box-one", "ghost", true).unwrap_err();
        assert!(err.to_string().contains("ghost"));
    }

    #[test]
    fn save_writes_the_machine_file_and_returns_its_path() {
        let fs = fs();
        let repo = repo(&fs);
        let cfg = toggle(&repo, "box-one", "extra", true).unwrap();
        let path = save(&fs, &repo, "box-one", &cfg).unwrap();

        assert_eq!(path, Path::new("/repo/machines/box-one.toml"));
        assert!(fs.read(&path).unwrap().contains("extra"));
    }

    #[test]
    fn adding_a_package_writes_it_into_the_set() {
        let fs = fs();
        let cfg = edit_package(&fs, &repo(&fs), "core", "ripgrep", false, true).unwrap();
        assert!(cfg.packages.brew.contains(&"ripgrep".to_string()));

        let raw = fs.read(Path::new("/repo/sets/core/set.toml")).unwrap();
        assert!(raw.contains("ripgrep"), "{raw}");
    }

    #[test]
    fn removing_a_package_takes_it_out() {
        let fs = fs();
        let cfg = edit_package(&fs, &repo(&fs), "core", "htop", false, false).unwrap();
        assert!(!cfg.packages.brew.contains(&"htop".to_string()));
    }

    #[test]
    fn adding_something_already_there_is_an_error_not_a_duplicate() {
        let fs = fs();
        let err = edit_package(&fs, &repo(&fs), "core", "htop", false, true)
            .unwrap_err()
            .to_string();
        assert!(err.contains("already has"), "{err}");
    }

    #[test]
    fn removing_something_absent_says_so_rather_than_succeeding_quietly() {
        // A no-op reported as success is how a button comes to do nothing
        // and nobody notices.
        let fs = fs();
        let err = edit_package(&fs, &repo(&fs), "core", "nope", false, false)
            .unwrap_err()
            .to_string();
        assert!(err.contains("does not have"), "{err}");
    }

    #[test]
    fn an_entry_carries_where_its_files_and_fragments_live() {
        // Repository-relative, so a front end can ask to open one without
        // being handed an absolute path it could point anywhere.
        let fs = fs();
        let core = list(&repo(&fs), &fs, "box-one").unwrap().remove(0);
        assert_eq!(core.files[0].target, "~/.gitconfig");
        assert_eq!(core.files[0].source, "sets/core/files/gitconfig");
        assert_eq!(core.fragments[0].name, "10-a.zsh");
        assert_eq!(core.fragments[0].source, "sets/core/shell/10-a.zsh");
    }
}

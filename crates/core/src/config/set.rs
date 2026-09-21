use serde::{Deserialize, Serialize};

/// One set: a named group of packages, shell fragments and managed files.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct SetConfig {
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub packages: Packages,
    #[serde(default)]
    pub files: Vec<ManagedFile>,
    /// Software this set needs that Homebrew does not provide.
    ///
    /// oh-my-zsh, nvm and anything else installed by a shell script live
    /// outside the package manager, so a set could reference them from a
    /// shell fragment while nothing declared that they had to exist. On a
    /// fresh machine the fragment's own guard then skipped them in silence:
    /// no theme, no aliases, and no error either.
    #[serde(default)]
    pub requires: Vec<Requirement>,
}

/// One piece of software a set needs, and how to tell whether it is here.
///
/// Reported, never installed. dotfix runs a fixed set of programs — brew,
/// git, ssh, op, age — and an `install` field would turn a file in a git
/// repository into a script it executes on every machine that syncs. The
/// command to run is carried as `hint`: text for the user, not for dotfix.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Requirement {
    pub name: String,
    /// Present when this path exists. `~` is expanded against the home
    /// directory. Mutually exclusive with `command`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Present when this is on `PATH`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// What the user should run. Shown, never executed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct Packages {
    #[serde(default)]
    pub brew: Vec<String>,
    #[serde(default)]
    pub cask: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct ManagedFile {
    /// Path relative to the set directory.
    pub source: String,
    /// Destination on the machine; a leading `~` is expanded at resolve time.
    pub target: String,
    #[serde(default)]
    pub mode: FileMode,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FileMode {
    /// Render through the template engine before writing.
    #[default]
    Template,
    /// Symlink the repository file into place.
    Symlink,
    /// Copy verbatim.
    Copy,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_full_set() {
        let toml = r#"
description = "Example set"

[packages]
brew = ["alpha", "bravo"]
cask = ["charlie"]

[[files]]
source = "files/example.tmpl"
target = "~/.example"
mode   = "template"
"#;
        let set: SetConfig = toml::from_str(toml).unwrap();
        assert_eq!(set.description, "Example set");
        assert_eq!(set.packages.brew, vec!["alpha", "bravo"]);
        assert_eq!(set.packages.cask, vec!["charlie"]);
        assert_eq!(set.files.len(), 1);
        assert_eq!(set.files[0].mode, FileMode::Template);
    }

    #[test]
    fn every_section_is_optional() {
        let set: SetConfig = toml::from_str("").unwrap();
        assert!(set.packages.brew.is_empty());
        assert!(set.files.is_empty());
    }

    #[test]
    fn file_mode_defaults_to_template() {
        let toml = r#"
[[files]]
source = "a"
target = "~/a"
"#;
        let set: SetConfig = toml::from_str(toml).unwrap();
        assert_eq!(set.files[0].mode, FileMode::Template);
    }
}

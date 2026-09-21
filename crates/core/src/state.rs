use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::ports::Fsys;

/// What dotfix itself last wrote to this machine. The third leg of the
/// three-way diff: without it, "another machine added this" cannot be told
/// apart from "this machine removed it on purpose".
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct Applied {
    #[serde(default)]
    pub brew: BTreeSet<String>,
    #[serde(default)]
    pub cask: BTreeSet<String>,
    /// Target path → SHA-256 of the content dotfix wrote there.
    #[serde(default)]
    pub files: BTreeMap<PathBuf, String>,
}

impl Applied {
    /// A missing file means "dotfix has never run here" and yields the empty
    /// state, not an error.
    pub fn load(fs: &dyn Fsys, path: &Path) -> Result<Self> {
        if !fs.exists(path) {
            return Ok(Self::default());
        }
        let raw = fs.read(path)?;
        serde_json::from_str(&raw).map_err(|source| Error::Json {
            path: path.to_path_buf(),
            source,
        })
    }

    pub fn save(&self, fs: &dyn Fsys, path: &Path) -> Result<()> {
        let raw = serde_json::to_string_pretty(self).map_err(|source| Error::Json {
            path: path.to_path_buf(),
            source,
        })?;
        fs.write(path, &raw, 0o644)
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::ports::fake::FakeFsys;

    #[test]
    fn missing_file_yields_an_empty_state() {
        let fs = FakeFsys::new();
        let applied = Applied::load(&fs, Path::new("/state/applied.json")).unwrap();
        assert_eq!(applied, Applied::default());
    }

    #[test]
    fn round_trips() {
        let fs = FakeFsys::new();
        let path = PathBuf::from("/state/applied.json");

        let mut applied = Applied::default();
        applied.brew.insert("alpha".into());
        applied.cask.insert("charlie".into());
        applied
            .files
            .insert(PathBuf::from("/Users/test/.rc"), "abc123".into());

        applied.save(&fs, &path).unwrap();
        assert_eq!(Applied::load(&fs, &path).unwrap(), applied);
    }

    #[test]
    fn reports_the_path_of_corrupt_json() {
        let fs = FakeFsys::from([("/state/applied.json", "{ not json")]);
        let err = Applied::load(&fs, Path::new("/state/applied.json")).unwrap_err();
        assert!(err.to_string().contains("/state/applied.json"));
    }
}

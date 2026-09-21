use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::ports::Fsys;

/// All machine-local locations dotfix uses. Derived from `$HOME` so tests can
/// point it at a fake root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    pub home: PathBuf,
}

impl Paths {
    pub fn new(home: PathBuf) -> Self {
        Self { home }
    }

    fn state_dir(&self) -> PathBuf {
        self.home.join(".local/state/dotfix")
    }

    pub fn applied(&self) -> PathBuf {
        self.state_dir().join("applied.json")
    }

    pub fn status_line(&self) -> PathBuf {
        self.state_dir().join("status.line")
    }

    pub fn backups(&self, stamp: &str) -> PathBuf {
        self.state_dir().join("backups").join(stamp)
    }

    pub fn local_config(&self) -> PathBuf {
        self.home.join(".config/dotfix/config.toml")
    }

    pub fn launch_agent(&self) -> PathBuf {
        self.home.join("Library/LaunchAgents/dev.noix.dotfix.plist")
    }

    pub fn zshrc(&self) -> PathBuf {
        self.home.join(".zshrc")
    }
}

/// Machine-local pointer to the data repository. Never stored in the repository
/// itself — it has to exist before the repository can be read.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct LocalConfig {
    pub repo: PathBuf,
    pub machine: String,
}

impl LocalConfig {
    pub fn load(fs: &dyn Fsys, path: &Path) -> Result<Self> {
        let raw = fs.read(path)?;
        toml::from_str(&raw).map_err(|source| Error::Toml {
            path: path.to_path_buf(),
            source,
        })
    }

    pub fn save(&self, fs: &dyn Fsys, path: &Path) -> Result<()> {
        let raw = toml::to_string_pretty(self)
            .map_err(|e| Error::Config(format!("serialising local config: {e}")))?;
        fs.write(path, &raw, 0o644)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_every_location_from_home() {
        let p = Paths::new(PathBuf::from("/Users/test"));
        assert_eq!(
            p.applied(),
            PathBuf::from("/Users/test/.local/state/dotfix/applied.json")
        );
        assert_eq!(
            p.status_line(),
            PathBuf::from("/Users/test/.local/state/dotfix/status.line")
        );
        assert_eq!(
            p.local_config(),
            PathBuf::from("/Users/test/.config/dotfix/config.toml")
        );
        assert_eq!(
            p.backups("20260916-101500"),
            PathBuf::from("/Users/test/.local/state/dotfix/backups/20260916-101500")
        );
        assert_eq!(
            p.launch_agent(),
            PathBuf::from("/Users/test/Library/LaunchAgents/dev.noix.dotfix.plist")
        );
    }

    #[test]
    fn local_config_round_trips() {
        use crate::ports::fake::FakeFsys;

        let fs = FakeFsys::new();
        let cfg = LocalConfig {
            repo: PathBuf::from("/Users/test/dotfiles"),
            machine: "box-one".into(),
        };
        let path = PathBuf::from("/Users/test/.config/dotfix/config.toml");
        cfg.save(&fs, &path).unwrap();
        assert_eq!(LocalConfig::load(&fs, &path).unwrap(), cfg);
    }
}

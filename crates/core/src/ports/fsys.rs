use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

/// Every filesystem access in `dotfix-core` goes through this trait so that
/// tests can run entirely in memory.
pub trait Fsys {
    fn read(&self, path: &Path) -> Result<String>;
    /// `mode` is a Unix permission bitmask, e.g. `0o600`.
    fn write(&self, path: &Path, contents: &str, mode: u32) -> Result<()>;
    fn exists(&self, path: &Path) -> bool;
    /// Recursive listing of files (not directories) below `path`.
    fn list_dir(&self, path: &Path) -> Result<Vec<PathBuf>>;
    fn remove(&self, path: &Path) -> Result<()>;
    /// Remove `path` and everything below it, files and directories alike. A
    /// missing `path` is success, not an error — cleanup must not fail
    /// because there was nothing to clean.
    fn remove_dir_all(&self, path: &Path) -> Result<()>;
    fn create_dir_all(&self, path: &Path) -> Result<()>;
    fn symlink(&self, source: &Path, target: &Path) -> Result<()>;
    fn read_link(&self, path: &Path) -> Result<PathBuf>;
}

pub struct RealFsys;

impl Fsys for RealFsys {
    fn read(&self, path: &Path) -> Result<String> {
        std::fs::read_to_string(path).map_err(|source| Error::Io {
            path: path.to_path_buf(),
            source,
        })
    }

    fn write(&self, path: &Path, contents: &str, mode: u32) -> Result<()> {
        use std::os::unix::fs::PermissionsExt;

        if let Some(parent) = path.parent() {
            self.create_dir_all(parent)?;
        }
        std::fs::write(path, contents).map_err(|source| Error::Io {
            path: path.to_path_buf(),
            source,
        })?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).map_err(|source| {
            Error::Io {
                path: path.to_path_buf(),
                source,
            }
        })
    }

    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn list_dir(&self, path: &Path) -> Result<Vec<PathBuf>> {
        let mut out = Vec::new();
        let mut stack = vec![path.to_path_buf()];
        while let Some(dir) = stack.pop() {
            let entries = match std::fs::read_dir(&dir) {
                Ok(entries) => entries,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(source) => return Err(Error::Io { path: dir, source }),
            };
            for entry in entries {
                let entry = entry.map_err(|source| Error::Io {
                    path: dir.clone(),
                    source,
                })?;
                let p = entry.path();
                if p.is_dir() {
                    stack.push(p);
                } else {
                    out.push(p);
                }
            }
        }
        out.sort();
        Ok(out)
    }

    fn remove(&self, path: &Path) -> Result<()> {
        std::fs::remove_file(path).map_err(|source| Error::Io {
            path: path.to_path_buf(),
            source,
        })
    }

    fn remove_dir_all(&self, path: &Path) -> Result<()> {
        match std::fs::remove_dir_all(path) {
            Ok(()) => Ok(()),
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(source) => Err(Error::Io {
                path: path.to_path_buf(),
                source,
            }),
        }
    }

    fn create_dir_all(&self, path: &Path) -> Result<()> {
        std::fs::create_dir_all(path).map_err(|source| Error::Io {
            path: path.to_path_buf(),
            source,
        })
    }

    fn symlink(&self, source: &Path, target: &Path) -> Result<()> {
        if let Some(parent) = target.parent() {
            self.create_dir_all(parent)?;
        }
        if target.is_symlink() {
            self.remove(target)?;
        }
        std::os::unix::fs::symlink(source, target).map_err(|source| Error::Io {
            path: target.to_path_buf(),
            source,
        })
    }

    fn read_link(&self, path: &Path) -> Result<PathBuf> {
        std::fs::read_link(path).map_err(|source| Error::Io {
            path: path.to_path_buf(),
            source,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The property requirement detection rests on: oh-my-zsh, nvm and the
    /// rest are *directories*, and a check that only saw files would report
    /// every one of them as missing on a machine that has them.
    ///
    /// Worth a real filesystem rather than the fake, which has no concept of
    /// a directory at all and would agree with any implementation.
    #[test]
    fn exists_is_true_for_a_directory_not_only_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join(".oh-my-zsh");
        std::fs::create_dir(&nested).unwrap();

        assert!(
            RealFsys.exists(&nested),
            "a directory is a thing that exists"
        );
        assert!(!RealFsys.exists(&dir.path().join("absent")));
    }
}

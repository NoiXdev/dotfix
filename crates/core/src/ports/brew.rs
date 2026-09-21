use std::process::Command;

use crate::error::{Error, Result};

pub trait Brew {
    /// Top-level formulae — packages not pulled in as a dependency.
    fn leaves(&self) -> Result<Vec<String>>;
    fn casks(&self) -> Result<Vec<String>>;
    /// Installed packages that depend on `formula`. Empty means it is a leaf
    /// and safe to uninstall.
    fn uses_installed(&self, formula: &str) -> Result<Vec<String>>;
    fn install(&self, name: &str, cask: bool) -> Result<()>;
    fn uninstall(&self, name: &str, cask: bool) -> Result<()>;
}

pub struct RealBrew;

impl RealBrew {
    fn run(args: &[&str]) -> Result<String> {
        let output = Command::new("brew")
            .args(args)
            .output()
            .map_err(|e| Error::Command {
                cmd: format!("brew {}", args.join(" ")),
                stderr: e.to_string(),
            })?;
        if !output.status.success() {
            return Err(Error::Command {
                cmd: format!("brew {}", args.join(" ")),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            });
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    fn lines(raw: String) -> Vec<String> {
        raw.lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect()
    }
}

impl Brew for RealBrew {
    fn leaves(&self) -> Result<Vec<String>> {
        Ok(Self::lines(Self::run(&["leaves"])?))
    }

    fn casks(&self) -> Result<Vec<String>> {
        Ok(Self::lines(Self::run(&["list", "--cask"])?))
    }

    fn uses_installed(&self, formula: &str) -> Result<Vec<String>> {
        Ok(Self::lines(Self::run(&["uses", "--installed", formula])?))
    }

    fn install(&self, name: &str, cask: bool) -> Result<()> {
        let args = if cask {
            vec!["install", "--cask", name]
        } else {
            vec!["install", name]
        };
        Self::run(&args).map(|_| ())
    }

    fn uninstall(&self, name: &str, cask: bool) -> Result<()> {
        let args = if cask {
            vec!["uninstall", "--cask", name]
        } else {
            vec!["uninstall", name]
        };
        Self::run(&args).map(|_| ())
    }
}

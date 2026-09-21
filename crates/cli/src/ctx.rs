use std::path::PathBuf;

use anyhow::{Context, Result};
use dotfix_core::engine::Engine;
use dotfix_core::paths::{LocalConfig, Paths};
use dotfix_core::ports::{RealBrew, RealExec, RealFsys, RealGit};

/// The real ports plus machine-local facts, constructed once per command.
pub struct Ctx {
    pub fs: RealFsys,
    pub brew: RealBrew,
    pub git: RealGit,
    pub exec: RealExec,
    pub paths: Paths,
    pub user: String,
    pub local: LocalConfig,
}

impl Ctx {
    pub fn engine(&self) -> Engine<'_> {
        Engine {
            fs: &self.fs,
            brew: &self.brew,
            git: &self.git,
            exec: &self.exec,
            paths: self.paths.clone(),
            user: self.user.clone(),
        }
    }
}

pub fn home() -> Result<PathBuf> {
    std::env::var("HOME")
        .map(PathBuf::from)
        .context("HOME is not set")
}

/// Load the machine-local configuration. Every command except `init` needs it.
pub fn load() -> Result<Ctx> {
    let paths = Paths::new(home()?);
    let fs = RealFsys;
    let local = LocalConfig::load(&fs, &paths.local_config()).with_context(|| {
        format!(
            "no local configuration at {} — run `dotfix init` first",
            paths.local_config().display()
        )
    })?;

    Ok(Ctx {
        fs: RealFsys,
        brew: RealBrew,
        git: RealGit,
        exec: RealExec,
        paths,
        user: std::env::var("USER").unwrap_or_else(|_| "unknown".into()),
        local,
    })
}

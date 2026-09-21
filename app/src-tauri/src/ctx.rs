//! The real ports plus machine-local facts. Mirrors `crates/cli/src/ctx.rs`;
//! both construct the same `Engine`, which is the point of linking the core
//! crate directly instead of shelling out to the CLI.

use std::path::PathBuf;

use dotfix_core::engine::Engine;
use dotfix_core::error::{Error, Result};
use dotfix_core::paths::{LocalConfig, Paths};
use dotfix_core::ports::{Fsys, RealBrew, RealExec, RealFsys, RealGit};

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
    pub fn load() -> Result<Self> {
        let home = std::env::var("HOME")
            .map(PathBuf::from)
            .map_err(|_| Error::Config("HOME is not set".into()))?;
        let paths = Paths::new(home);
        let fs = RealFsys;
        // "Not set up yet" is a state, not a failure, and it is the very first
        // thing a new user hits — the window must not greet them with a raw
        // `io error ... (os error 2)` naming a path they have never heard of.
        // A config file that exists but cannot be parsed is a different story
        // and keeps its underlying error.
        let config_path = paths.local_config();
        if !fs.exists(&config_path) {
            return Err(Error::Config(not_configured_message(&config_path)));
        }
        let local = LocalConfig::load(&fs, &config_path)?;

        Ok(Self {
            fs: RealFsys,
            brew: RealBrew,
            git: RealGit,
            exec: RealExec,
            paths,
            user: std::env::var("USER").unwrap_or_else(|_| "unknown".into()),
            local,
        })
    }

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

/// Stable opening of [`not_configured_message`]. The frontend matches on this
/// to render calm onboarding instead of a red error banner, so it must not
/// change without changing `NOT_CONFIGURED_PREFIX` in `app/src/types.ts` too.
/// The test below is what keeps the two in step.
pub const NOT_CONFIGURED_PREFIX: &str = "dotfix is not set up on this machine yet.";

/// What the window reports when dotfix has never been set up on this
/// machine. The frontend turns this into the setup wizard rather than
/// rendering the sentence, so it no longer sends anyone to a terminal —
/// setup happens in this window. It still has to read sensibly wherever an
/// error string ends up, which is why it names the state and the file it is
/// missing rather than a command.
pub fn not_configured_message(config_path: &std::path::Path) -> String {
    format!(
        "{NOT_CONFIGURED_PREFIX} Setting it up here creates or clones your \
         configuration repository and writes {}.",
        config_path.display()
    )
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn the_not_configured_message_explains_the_state_without_sending_anyone_to_a_terminal() {
        let msg = not_configured_message(Path::new("/Users/test/.config/dotfix/config.toml"));
        assert!(
            !msg.contains("terminal"),
            "the wizard replaced that instruction — setup happens in the window: {msg}"
        );
        assert!(!msg.contains("os error"), "must not leak a raw io error");
        assert!(msg.contains("/Users/test/.config/dotfix/config.toml"));
        assert!(
            msg.starts_with(NOT_CONFIGURED_PREFIX),
            "the frontend matches this prefix — see NOT_CONFIGURED_PREFIX in app/src/types.ts"
        );
    }
}

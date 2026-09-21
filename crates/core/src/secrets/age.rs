use std::path::PathBuf;

use crate::error::Result;
use crate::ports::Exec;
use crate::secrets::{SecretProvider, value};

/// Reference format: a path relative to the repository root.
pub struct AgeProvider {
    pub repo_root: PathBuf,
    pub identity: PathBuf,
}

impl SecretProvider for AgeProvider {
    fn get_with(&self, reference: &str, exec: &dyn Exec) -> Result<String> {
        let file = self.repo_root.join(reference);
        exec.run(
            "age",
            &[
                "--decrypt",
                "--identity",
                &self.identity.display().to_string(),
                &file.display().to_string(),
            ],
        )
        .map(value)
    }
}

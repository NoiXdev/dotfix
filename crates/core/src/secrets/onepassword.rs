use crate::error::Result;
use crate::ports::Exec;
use crate::secrets::{SecretProvider, value};

/// Reference format: `op://<vault>/<item>/<field>`.
pub struct OnePasswordProvider {
    pub vault: Option<String>,
}

impl SecretProvider for OnePasswordProvider {
    fn get_with(&self, reference: &str, exec: &dyn Exec) -> Result<String> {
        exec.run("op", &["read", reference]).map(value)
    }
}

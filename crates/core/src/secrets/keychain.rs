use crate::error::{Error, Result};
use crate::ports::Exec;
use crate::secrets::{SecretProvider, value};

/// Reference format: `<service>/<account>`.
pub struct KeychainProvider;

impl SecretProvider for KeychainProvider {
    fn get_with(&self, reference: &str, exec: &dyn Exec) -> Result<String> {
        let (service, account) = reference.split_once('/').ok_or_else(|| Error::Secret {
            name: reference.to_string(),
            reason: "keychain references must look like `service/account`".to_string(),
        })?;
        exec.run(
            "security",
            &["find-generic-password", "-s", service, "-a", account, "-w"],
        )
        .map(value)
    }
}

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Per-machine configuration: which sets are active and where secrets live.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct MachineConfig {
    #[serde(default)]
    pub sets: Vec<String>,
    #[serde(default)]
    pub secret_provider: ProviderKind,
    /// 1Password vault. Ignored by the other providers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vault: Option<String>,
    /// Template variables available as `{{ name }}`.
    #[serde(default)]
    pub vars: BTreeMap<String, String>,
    /// Overrides for where a named secret lives. Without an entry the provider
    /// derives a default location from the name.
    #[serde(default)]
    pub secrets: BTreeMap<String, String>,
    /// Packages that must never be reported as unmanaged on this machine.
    #[serde(default)]
    pub ignore: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    /// macOS Keychain — the default, needs no extra software.
    #[default]
    Keychain,
    #[serde(rename = "1password")]
    OnePassword,
    Age,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_keychain_machine() {
        let toml = r#"
sets = ["core", "web"]
secret_provider = "keychain"

[vars]
git_email = "someone@example.com"
"#;
        let m: MachineConfig = toml::from_str(toml).unwrap();
        assert_eq!(m.sets, vec!["core", "web"]);
        assert_eq!(m.secret_provider, ProviderKind::Keychain);
        assert_eq!(m.vault, None);
        assert_eq!(m.vars["git_email"], "someone@example.com");
    }

    #[test]
    fn parses_a_onepassword_machine_with_vault_and_overrides() {
        let toml = r#"
sets = ["core"]
secret_provider = "1password"
vault = "Example"

[secrets]
api_key = "op://Example/service/api_key"
"#;
        let m: MachineConfig = toml::from_str(toml).unwrap();
        assert_eq!(m.secret_provider, ProviderKind::OnePassword);
        assert_eq!(m.vault.as_deref(), Some("Example"));
        assert_eq!(m.secrets["api_key"], "op://Example/service/api_key");
    }

    #[test]
    fn provider_defaults_to_keychain() {
        let m: MachineConfig = toml::from_str("sets = []").unwrap();
        assert_eq!(m.secret_provider, ProviderKind::Keychain);
        assert!(m.ignore.is_empty());
    }
}

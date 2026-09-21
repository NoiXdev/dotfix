mod age;
mod keychain;
mod onepassword;

use std::collections::BTreeMap;
use std::path::Path;

pub use age::AgeProvider;
pub use keychain::KeychainProvider;
pub use onepassword::OnePasswordProvider;

use crate::config::ProviderKind;
use crate::error::{Error, Result};
use crate::ports::Exec;
use crate::render::SecretLookup;

/// Strip the trailing newline every secret CLI appends. The value itself
/// never ends in one.
pub(crate) fn value(raw: String) -> String {
    raw.trim_end_matches('\n').to_string()
}

/// Marker written in place of a secret value in any human-visible output.
pub const REDACTED: &str = "«redacted»";

/// Replace every resolved secret value with [`REDACTED`]. Anything that may
/// carry rendered file content must pass through this before being printed,
/// logged or diffed.
pub fn redact(text: &str, values: &[String]) -> String {
    let mut out = text.to_string();
    for value in values {
        if !value.is_empty() {
            out = out.replace(value, REDACTED);
        }
    }
    out
}

/// A backend that turns a provider-specific reference into a value.
pub trait SecretProvider {
    fn get_with(&self, reference: &str, exec: &dyn Exec) -> Result<String>;
}

/// Where a named secret lives when the machine configuration gives no override.
pub fn default_reference(kind: ProviderKind, vault: Option<&str>, name: &str) -> String {
    match kind {
        ProviderKind::Keychain => format!("dotfix/{name}"),
        ProviderKind::OnePassword => {
            let vault = vault.unwrap_or("Private");
            format!("op://{vault}/dotfix/{name}")
        }
        ProviderKind::Age => format!("secrets/{name}.age"),
    }
}

/// Build the backend a machine's configuration asks for.
pub fn provider_for(
    kind: ProviderKind,
    vault: Option<&str>,
    repo_root: &Path,
    home: &Path,
) -> Box<dyn SecretProvider> {
    match kind {
        ProviderKind::Keychain => Box::new(KeychainProvider),
        ProviderKind::OnePassword => Box::new(OnePasswordProvider {
            vault: vault.map(str::to_string),
        }),
        ProviderKind::Age => Box::new(AgeProvider {
            repo_root: repo_root.to_path_buf(),
            identity: home.join(".config/dotfix/age.key"),
        }),
    }
}

/// Maps logical secret names to values: the machine decides *where*, the set
/// decides *what*.
pub struct Resolver<'a> {
    pub provider: &'a dyn SecretProvider,
    pub kind: ProviderKind,
    pub vault: Option<String>,
    pub mapping: &'a BTreeMap<String, String>,
    pub exec: &'a dyn Exec,
}

impl SecretLookup for Resolver<'_> {
    fn get(&self, name: &str) -> Result<String> {
        let reference = self
            .mapping
            .get(name)
            .cloned()
            .unwrap_or_else(|| default_reference(self.kind, self.vault.as_deref(), name));
        self.provider
            .get_with(&reference, self.exec)
            // The reason must never carry the value, only why the lookup failed.
            .map_err(|e| Error::Secret {
                name: name.to_string(),
                reason: short_reason(&e),
            })
    }
}

fn short_reason(err: &Error) -> String {
    match err {
        Error::Command { stderr, .. } if !stderr.is_empty() => stderr.clone(),
        Error::Command { .. } => "lookup failed".to_string(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use super::*;
    use crate::config::ProviderKind;
    use crate::ports::fake::FakeExec;

    #[test]
    fn redaction_removes_every_occurrence_of_a_value() {
        let text = "access_key = s3cr3t\nsecret_key = s3cr3t\nregion = eu";
        let out = redact(text, &["s3cr3t".to_string()]);
        assert!(!out.contains("s3cr3t"));
        assert_eq!(out.matches(REDACTED).count(), 2);
        assert!(out.contains("region = eu"));
    }

    #[test]
    fn redaction_ignores_empty_values() {
        assert_eq!(redact("unchanged", &[String::new()]), "unchanged");
    }

    #[test]
    fn keychain_default_reference_is_service_slash_name() {
        assert_eq!(
            default_reference(ProviderKind::Keychain, None, "api_key"),
            "dotfix/api_key"
        );
    }

    #[test]
    fn onepassword_default_reference_uses_the_vault() {
        assert_eq!(
            default_reference(ProviderKind::OnePassword, Some("Example"), "api_key"),
            "op://Example/dotfix/api_key"
        );
    }

    #[test]
    fn age_default_reference_is_a_repository_relative_file() {
        assert_eq!(
            default_reference(ProviderKind::Age, None, "api_key"),
            "secrets/api_key.age"
        );
    }

    #[test]
    fn an_explicit_mapping_wins_over_the_default() {
        let exec = FakeExec::new([("op read op://Other/thing/field", "mapped-value\n")]);
        let provider = OnePasswordProvider {
            vault: Some("Example".into()),
        };
        let mapping =
            BTreeMap::from([("api_key".to_string(), "op://Other/thing/field".to_string())]);
        let resolver = Resolver {
            provider: &provider,
            kind: ProviderKind::OnePassword,
            vault: Some("Example".into()),
            mapping: &mapping,
            exec: &exec,
        };
        assert_eq!(resolver.get("api_key").unwrap(), "mapped-value");
    }

    #[test]
    fn a_failed_lookup_reports_the_name_but_never_a_value() {
        let exec = FakeExec::new([]);
        let provider = KeychainProvider;
        let mapping = BTreeMap::new();
        let resolver = Resolver {
            provider: &provider,
            kind: ProviderKind::Keychain,
            vault: None,
            mapping: &mapping,
            exec: &exec,
        };
        let err = resolver.get("api_key").unwrap_err();
        assert!(err.to_string().contains("api_key"));
    }

    #[test]
    fn keychain_provider_builds_the_expected_command() {
        let exec = FakeExec::new([(
            "security find-generic-password -s dotfix -a api_key -w",
            "s3cr3t\n",
        )]);
        assert_eq!(
            KeychainProvider.get_with("dotfix/api_key", &exec).unwrap(),
            "s3cr3t"
        );
    }

    #[test]
    fn age_provider_decrypts_relative_to_the_repository() {
        let exec = FakeExec::new([(
            "age --decrypt --identity /Users/test/.config/dotfix/age.key /repo/secrets/api_key.age",
            "s3cr3t\n",
        )]);
        let provider = AgeProvider {
            repo_root: PathBuf::from("/repo"),
            identity: PathBuf::from("/Users/test/.config/dotfix/age.key"),
        };
        assert_eq!(
            provider.get_with("secrets/api_key.age", &exec).unwrap(),
            "s3cr3t"
        );
    }
}

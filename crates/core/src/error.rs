use std::path::PathBuf;

/// Every fallible operation in `dotfix-core` returns this error.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("invalid TOML in {path}: {source}")]
    Toml {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },

    #[error("invalid JSON in {path}: {source}")]
    Json {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    #[error("machine `{machine}` references unknown set `{set}`")]
    UnknownSet { set: String, machine: String },

    #[error("machine `{0}` not found in repository")]
    UnknownMachine(String),

    #[error("command `{cmd}` failed: {stderr}")]
    Command { cmd: String, stderr: String },

    #[error("template error in {path}: {source}")]
    Template {
        path: PathBuf,
        #[source]
        source: minijinja::Error,
    },

    #[error("secret `{name}` could not be resolved: {reason}")]
    Secret { name: String, reason: String },

    #[error("repository has diverged from origin; resolve it manually")]
    Diverged,

    #[error("{0}")]
    Config(String),
}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_set_message_names_set_and_machine() {
        let err = Error::UnknownSet {
            set: "bravo".into(),
            machine: "alpha".into(),
        };
        assert_eq!(
            err.to_string(),
            "machine `alpha` references unknown set `bravo`"
        );
    }

    #[test]
    fn secret_error_does_not_contain_the_value() {
        let err = Error::Secret {
            name: "api_key".into(),
            reason: "not found in keychain".into(),
        };
        let msg = err.to_string();
        assert!(msg.contains("api_key"));
        assert!(msg.contains("not found in keychain"));
    }
}

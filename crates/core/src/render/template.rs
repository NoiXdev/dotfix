use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use minijinja::{Environment, UndefinedBehavior, Value};

use crate::error::{Error, Result};

/// Variables available to every template and shell fragment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Vars {
    pub home: PathBuf,
    pub user: String,
    pub machine: String,
    /// `[vars]` from the machine configuration.
    pub extra: BTreeMap<String, String>,
}

/// Resolves a logical secret name to its value. Implemented in the `secrets`
/// module; declared here so templates can be tested without a provider.
pub trait SecretLookup {
    fn get(&self, name: &str) -> Result<String>;
}

/// Rejects every secret. Use for content that must never contain credentials.
pub struct NoSecrets;

impl SecretLookup for NoSecrets {
    fn get(&self, name: &str) -> Result<String> {
        Err(Error::Secret {
            name: name.to_string(),
            reason: "secrets are not permitted in this context".to_string(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rendered {
    pub content: String,
    /// True when `secret()` was called. The caller must then write the file
    /// with mode 0600 and redact it in any output.
    pub contains_secrets: bool,
    /// The values that were resolved, so callers can redact them before
    /// showing content to anyone. Deliberately not `Serialize`.
    pub secret_values: Vec<String>,
}

/// Render one template. Unknown variables are an error rather than an empty
/// string — a silently empty `PATH` entry is worse than a failed run.
pub fn render(
    path: &Path,
    source: &str,
    vars: &Vars,
    secrets: &dyn SecretLookup,
) -> Result<Rendered> {
    let used = Arc::new(AtomicBool::new(false));

    let mut env = Environment::new();
    env.set_undefined_behavior(UndefinedBehavior::Strict);
    // Jinja2 drops a template's trailing newline by default. For generated
    // config files that is wrong — a file without one breaks appends.
    env.set_keep_trailing_newline(true);

    // minijinja needs an owned, 'static closure; collect the lookups eagerly.
    let resolved = collect_secret_calls(source)
        .into_iter()
        .map(|name| secrets.get(&name).map(|value| (name, value)))
        .collect::<Result<BTreeMap<String, String>>>()
        .map_err(|e| annotate(path, e))?;
    let secret_values: Vec<String> = resolved.values().cloned().collect();

    let flag = Arc::clone(&used);
    env.add_function(
        "secret",
        move |name: String| -> std::result::Result<Value, minijinja::Error> {
            flag.store(true, Ordering::Relaxed);
            resolved
                .get(&name)
                .map(|v| Value::from(v.clone()))
                .ok_or_else(|| {
                    minijinja::Error::new(
                        minijinja::ErrorKind::InvalidOperation,
                        format!("unknown secret `{name}`"),
                    )
                })
        },
    );

    let mut ctx: BTreeMap<String, String> = vars.extra.clone();
    ctx.insert("home".into(), vars.home.display().to_string());
    ctx.insert("user".into(), vars.user.clone());
    ctx.insert("machine".into(), vars.machine.clone());

    let content = env
        .render_str(source, ctx)
        .map_err(|source| Error::Template {
            path: path.to_path_buf(),
            source,
        })?;

    Ok(Rendered {
        content,
        contains_secrets: used.load(Ordering::Relaxed),
        secret_values,
    })
}

/// Extract every `secret("name")` argument from the source so the values can be
/// fetched before rendering starts.
fn collect_secret_calls(source: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut rest = source;
    while let Some(idx) = rest.find("secret(") {
        rest = &rest[idx + "secret(".len()..];
        let Some(open) = rest.find(['"', '\'']) else {
            break;
        };
        let quote = rest.as_bytes()[open] as char;
        let after = &rest[open + 1..];
        let Some(close) = after.find(quote) else {
            break;
        };
        names.push(after[..close].to_string());
        rest = &after[close + 1..];
    }
    names.sort();
    names.dedup();
    names
}

fn annotate(path: &Path, err: Error) -> Error {
    match err {
        Error::Secret { name, reason } => Error::Secret {
            name,
            reason: format!("{reason} (in {})", path.display()),
        },
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};

    use super::*;

    struct StaticSecrets;

    impl SecretLookup for StaticSecrets {
        fn get(&self, name: &str) -> Result<String> {
            match name {
                "api_key" => Ok("s3cr3t".to_string()),
                other => Err(Error::Secret {
                    name: other.to_string(),
                    reason: "not configured".to_string(),
                }),
            }
        }
    }

    fn vars() -> Vars {
        Vars {
            home: PathBuf::from("/Users/test"),
            user: "test".into(),
            machine: "box-one".into(),
            extra: BTreeMap::from([("git_email".to_string(), "someone@example.com".to_string())]),
        }
    }

    #[test]
    fn substitutes_the_builtin_variables() {
        let out = render(
            Path::new("t.tmpl"),
            "export PNPM_HOME=\"{{ home }}/Library/pnpm\"\n# {{ user }}@{{ machine }}",
            &vars(),
            &NoSecrets,
        )
        .unwrap();
        assert_eq!(
            out.content,
            "export PNPM_HOME=\"/Users/test/Library/pnpm\"\n# test@box-one"
        );
        assert!(!out.contains_secrets);
    }

    #[test]
    fn keeps_a_trailing_newline() {
        let out = render(
            Path::new("t.tmpl"),
            "home={{ home }}\n",
            &vars(),
            &NoSecrets,
        )
        .unwrap();
        assert_eq!(out.content, "home=/Users/test\n");
    }

    #[test]
    fn substitutes_machine_vars() {
        let out = render(Path::new("t.tmpl"), "{{ git_email }}", &vars(), &NoSecrets).unwrap();
        assert_eq!(out.content, "someone@example.com");
    }

    #[test]
    fn resolves_secrets_and_flags_the_result() {
        let out = render(
            Path::new("t.tmpl"),
            "access_key = {{ secret(\"api_key\") }}",
            &vars(),
            &StaticSecrets,
        )
        .unwrap();
        assert_eq!(out.content, "access_key = s3cr3t");
        assert!(
            out.contains_secrets,
            "must be flagged so the file is written 0600"
        );
    }

    #[test]
    fn an_unresolvable_secret_fails_and_names_the_file() {
        let err = render(
            Path::new("/repo/sets/infra/files/x.tmpl"),
            "{{ secret(\"missing\") }}",
            &vars(),
            &StaticSecrets,
        )
        .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("/repo/sets/infra/files/x.tmpl"));
    }

    #[test]
    fn an_unknown_variable_is_an_error_not_an_empty_string() {
        let err = render(Path::new("t.tmpl"), "{{ nope }}", &vars(), &NoSecrets).unwrap_err();
        assert!(matches!(err, Error::Template { .. }));
    }

    #[test]
    fn a_rendered_template_carries_the_values_it_resolved() {
        let out = render(
            Path::new("t.tmpl"),
            "access_key = {{ secret(\"api_key\") }}",
            &vars(),
            &StaticSecrets,
        )
        .unwrap();
        assert_eq!(out.secret_values, vec!["s3cr3t".to_string()]);
    }

    #[test]
    fn a_template_without_secrets_carries_none() {
        let out = render(Path::new("t.tmpl"), "{{ user }}", &vars(), &NoSecrets).unwrap();
        assert!(out.secret_values.is_empty());
    }
}

use crate::error::Result;
use crate::ports::Exec;

/// What the user is asked to confirm before anything happens to their account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoRequest {
    pub owner: String,
    pub name: String,
}

/// The sentence shown in the confirmation. Says private explicitly, because
/// creating a public repository of someone's dotfiles would be a catastrophe
/// worth spelling out.
pub fn describe(req: &RepoRequest) -> String {
    format!(
        "Create the private repository {}/{} on GitHub using your `gh` login?",
        req.owner, req.name
    )
}

/// Hand the token to git's credential helper, which puts it in the macOS
/// Keychain. dotfix does not keep it.
///
/// The value travels on stdin — never as an argument, where it would be
/// visible in `ps` output and in our own call log.
pub fn store_token(exec: &dyn Exec, host: &str, user: &str, token: &str) -> Result<()> {
    let payload = format!("protocol=https\nhost={host}\nusername={user}\npassword={token}\n\n");
    exec.run_with_stdin("git", &["credential-osxkeychain", "store"], &payload)
        .map(|_| ())
}

/// Create the repository through the user's own authenticated `gh`, so dotfix
/// never needs account-wide access of its own.
pub fn create_repo(exec: &dyn Exec, req: &RepoRequest) -> Result<String> {
    let slug = format!("{}/{}", req.owner, req.name);
    let out = exec.run(
        "gh",
        &["repo", "create", &slug, "--private", "--clone=false"],
    )?;
    Ok(out.trim().to_string())
}

pub fn https_url(owner_repo: &str) -> String {
    let repo = owner_repo.trim_end_matches(".git");
    format!("https://github.com/{repo}.git")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::fake::FakeExec;

    #[test]
    fn a_token_goes_to_the_keychain_and_nowhere_else() {
        let exec = FakeExec::new([("git credential-osxkeychain store", "")]);
        store_token(&exec, "github.com", "example-user", "ghp_example").unwrap();

        let calls = exec.calls();
        assert_eq!(calls, vec!["git credential-osxkeychain store".to_string()]);
        assert!(
            !calls.iter().any(|c| c.contains("ghp_example")),
            "the token must travel on stdin, never in an argument list where \
             it would show up in `ps` and in our own call log: {calls:?}"
        );
    }

    #[test]
    fn a_failed_store_does_not_echo_the_token() {
        let exec = FakeExec::new([]);
        let err = store_token(&exec, "github.com", "example-user", "ghp_example").unwrap_err();
        assert!(
            !err.to_string().contains("ghp_example"),
            "an error must never carry the value: {err}"
        );
    }

    #[test]
    fn the_confirmation_names_owner_name_and_visibility() {
        let text = describe(&RepoRequest {
            owner: "example-user".into(),
            name: "dotfiles".into(),
        });
        assert!(text.contains("example-user/dotfiles"));
        assert!(text.to_lowercase().contains("private"));
    }

    #[test]
    fn creating_a_repository_asks_gh_for_a_private_one() {
        let exec = FakeExec::new([(
            "gh repo create example-user/dotfiles --private --clone=false",
            "https://github.com/example-user/dotfiles\n",
        )]);
        let url = create_repo(
            &exec,
            &RepoRequest {
                owner: "example-user".into(),
                name: "dotfiles".into(),
            },
        )
        .unwrap();

        assert_eq!(url, "https://github.com/example-user/dotfiles");
        assert!(exec.calls()[0].contains("--private"));
    }

    #[test]
    fn https_url_is_built_from_owner_and_repo() {
        assert_eq!(
            https_url("example-user/dotfiles"),
            "https://github.com/example-user/dotfiles.git"
        );
    }
}

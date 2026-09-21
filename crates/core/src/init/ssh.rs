use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::ports::{Exec, Fsys};

/// Flags that make the probe safe to run from a GUI.
///
/// `BatchMode=yes` forbids SSH any interactive prompt. Without it this call
/// from an app with no controlling terminal blocks forever waiting for a
/// passphrase or a host-key confirmation nobody can type. `ConnectTimeout`
/// bounds the other way to hang: an unreachable host.
pub const PROBE_ARGS: &[&str] = &["-o", "BatchMode=yes", "-o", "ConnectTimeout=5"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reachability {
    /// Existing SSH setup works — clone directly, configure nothing.
    Ready,
    /// Reached the host, but it would not take our key.
    NeedsKey,
    /// Could not get that far.
    Unreachable { detail: String },
}

/// Ask the host whether our SSH setup already works, without ever being able
/// to prompt.
pub fn probe(exec: &dyn Exec, host: &str) -> Reachability {
    let target = format!("git@{host}");
    let mut args: Vec<&str> = PROBE_ARGS.to_vec();
    args.push("-T");
    args.push(&target);

    match exec.run("ssh", &args) {
        // GitHub exits non-zero even on success, so a successful run is still
        // unambiguous evidence.
        Ok(out) if out.contains("successfully authenticated") => Reachability::Ready,
        Ok(_) => Reachability::NeedsKey,
        Err(e) => {
            let text = e.to_string();
            if text.contains("successfully authenticated") {
                Reachability::Ready
            } else if text.contains("Permission denied") || text.contains("publickey") {
                Reachability::NeedsKey
            } else {
                Reachability::Unreachable { detail: text }
            }
        }
    }
}

/// GitHub's published SSH host-key fingerprints.
///
/// Source: <https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/githubs-ssh-key-fingerprints>
///
/// Pinning these is deliberate: the alternative is trust-on-first-use, where a
/// first-time setup on a hostile network silently pins the wrong host. The
/// price is that a rotation on GitHub's side needs a dotfix update — which is
/// the trade the design chose, loudly rather than quietly.
pub const GITHUB_FINGERPRINTS: &[&str] = &[
    "SHA256:+DiY3wvvV6TuJJhbpZisF/zLDA0zPMSvHdkr4UvCOqU", // ed25519
    "SHA256:uNiVztksCsDhcc0u9e8BujQXVUpKZIDTMczCvj3tD2s", // rsa
];

/// Make sure `host` is in `known_hosts`, but only if its key is one we
/// expect.
///
/// Returns whether an entry was added. Writes nothing on a mismatch: the
/// obvious implementation here is `ssh-keyscan host >> known_hosts`, i.e.
/// trust whatever answers first. We scan the key, but only accept it once its
/// fingerprint matches `expected` — GitHub's published values, compiled in.
pub fn ensure_host_known(
    fs: &dyn Fsys,
    exec: &dyn Exec,
    home: &Path,
    host: &str,
    expected: &[&str],
) -> Result<bool> {
    let known_hosts = home.join(".ssh/known_hosts");

    if fs.exists(&known_hosts)
        && (already_known(&fs.read(&known_hosts)?, host) || known_to_ssh(exec, &known_hosts, host))
    {
        return Ok(false);
    }

    let scanned = exec.run("ssh-keyscan", &["-t", "ed25519", host])?;
    // `ssh-keygen -lf -` reads the key to fingerprint from stdin, so it goes
    // through `run_with_stdin` rather than as an argument.
    let listed = exec.run_with_stdin("ssh-keygen", &["-lf", "-"], &scanned)?;

    let fingerprint = listed
        .split_whitespace()
        .find(|t| t.starts_with("SHA256:"))
        .ok_or_else(|| {
            Error::Config(format!(
                "could not read a fingerprint for {host} from `{listed}`"
            ))
        })?;

    if !expected.contains(&fingerprint) {
        return Err(Error::Config(format!(
            "host key for {host} does not match a known fingerprint. Got {fingerprint}, \
             expected one of: {}. Nothing was written — do not continue on this network.",
            expected.join(", ")
        )));
    }

    let mut contents = if fs.exists(&known_hosts) {
        fs.read(&known_hosts)?
    } else {
        String::new()
    };
    if !contents.is_empty() && !contents.ends_with('\n') {
        contents.push('\n');
    }
    contents.push_str(&scanned);
    fs.write(&known_hosts, &contents, 0o644)?;
    Ok(true)
}

/// A host name as ssh compares them: lowercase, and with at most one trailing
/// root dot removed, so `GitHub.com`, `github.com.` and `github.com` are all
/// one host.
fn normalise_host(host: &str) -> String {
    host.trim()
        .strip_suffix('.')
        .unwrap_or(host.trim())
        .to_ascii_lowercase()
}

/// Whether ssh itself considers `host` known.
///
/// [`already_known`] reads the file as text, which by design cannot see
/// entries written under `HashKnownHosts yes`: those store `|1|salt|hash` in
/// place of the host name, and only ssh knows the salt. Asking `ssh-keygen
/// -F` is the only way to match them.
///
/// Without this, a machine with hashing enabled — the common case, and the
/// case on the maintainer's own Mac — appends a fresh plaintext `github.com`
/// line on every run, because the text scan never finds the hashed one.
///
/// A non-zero exit means "not found", which is the answer we want; so does an
/// unreadable file. Both fall through to the scan, where the fingerprint
/// check still guards what gets written. Failing open here is safe precisely
/// because this function can only ever *skip* work, never authorise it.
fn known_to_ssh(exec: &dyn Exec, known_hosts: &Path, host: &str) -> bool {
    let path = known_hosts.to_string_lossy();
    exec.run("ssh-keygen", &["-F", host, "-f", &path])
        .is_ok_and(|found| !found.trim().is_empty())
}

/// Whether `known_hosts` already holds an entry for `host`.
///
/// Anchored to the host field of each line, never a substring of the file: a
/// machine whose `known_hosts` holds `[ssh.github.com]:443` or
/// `gist.github.com` — both of which contain "github.com" — would otherwise
/// skip the scan and the fingerprint check entirely, which is the whole
/// point of this module. The host field may list several names separated by
/// commas, and may be preceded by a marker like `@cert-authority`.
fn already_known(contents: &str, host: &str) -> bool {
    let host = normalise_host(host);
    contents.lines().any(|line| {
        let mut fields = line.split_whitespace();
        let first = match fields.next() {
            Some(f) if f.starts_with('#') => return false,
            Some(f) if f.starts_with('@') => match fields.next() {
                Some(f) => f,
                None => return false,
            },
            Some(f) => f,
            None => return false,
        };
        first.split(',').any(|name| normalise_host(name) == host)
    })
}

/// A key dedicated to dotfix, scoped by an ssh_config alias to one repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeployKey {
    /// The public half — the only part that ever leaves this machine.
    pub public: String,
    pub path: PathBuf,
    pub host_alias: String,
}

/// The ssh alias a deploy key is scoped to. Public so callers that decide
/// whether a clone URL targets GitHub (e.g. the app's `init_run`, before it
/// pins the host key) can match against this exact value instead of
/// duplicating it — a prefix match or a copy of the literal would silently
/// drift from this constant.
pub const HOST_ALIAS: &str = "github.com-dotfix";

/// Build the `ssh-keygen` arguments for a dotfix-only deploy key.
///
/// Kept separate from [`ensure_deploy_key`] so the no-passphrase property can
/// be tested directly against the arguments a real invocation would use,
/// rather than through a fake that cannot simulate `ssh-keygen`'s file side
/// effects.
fn keygen_args(path: &Path, machine: &str) -> Vec<String> {
    vec![
        "-t".to_string(),
        "ed25519".to_string(),
        "-N".to_string(),
        String::new(),
        "-C".to_string(),
        format!("dotfix@{machine}"),
        "-f".to_string(),
        path.display().to_string(),
    ]
}

/// Generate a dotfix-only key, or reuse the one that is already there.
///
/// No passphrase: a passphrase would need an agent, which is the problem this
/// whole path exists to avoid. That is the standard shape for a deploy key,
/// and the mitigation is scope — the key opens exactly one repository, and
/// there is one per machine so revoking a machine is deleting one key.
pub fn ensure_deploy_key(
    fs: &dyn Fsys,
    exec: &dyn Exec,
    home: &Path,
    machine: &str,
) -> Result<DeployKey> {
    let dir = home.join(".ssh");
    let name = format!("dotfix_{machine}_ed25519");
    let path = dir.join(&name);
    // Never `with_extension("pub")`: a machine called `mbp.local` gives
    // `dotfix_mbp.local_ed25519`, whose "extension" is `local_ed25519`, so
    // that would look for `dotfix_mbp.pub`. The read would fail and the
    // retry would re-run `ssh-keygen` against an existing path, which asks
    // whether to overwrite — a question a GUI cannot answer.
    let public_path = dir.join(format!("{name}.pub"));

    // Regenerating would invalidate the deploy key the user already added on
    // GitHub, so an existing key is always reused.
    if !fs.exists(&public_path) {
        let args = keygen_args(&path, machine);
        let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
        exec.run("ssh-keygen", &arg_refs)?;
    }

    Ok(DeployKey {
        public: fs.read(&public_path)?.trim().to_string(),
        path,
        host_alias: HOST_ALIAS.to_string(),
    })
}

/// Append a stanza that uses this key for the alias and nothing else.
pub fn ensure_ssh_config(fs: &dyn Fsys, home: &Path, key: &DeployKey) -> Result<()> {
    let config = home.join(".ssh/config");
    let existing = if fs.exists(&config) {
        fs.read(&config)?
    } else {
        String::new()
    };

    if declares_host(&existing, &key.host_alias) {
        return Ok(());
    }

    let stanza = format!(
        "\n# added by dotfix — scopes its own key to this alias only\n\
         Host {alias}\n  \
         HostName github.com\n  \
         User git\n  \
         IdentityFile {identity}\n  \
         IdentitiesOnly yes\n",
        alias = key.host_alias,
        identity = key.path.display()
    );

    let mut contents = existing;
    if !contents.is_empty() && !contents.ends_with('\n') {
        contents.push('\n');
    }
    contents.push_str(&stanza);
    fs.write(&config, &contents, 0o600)
}

/// Whether an ssh_config already declares `alias` as a `Host` pattern.
///
/// Anchored the same way [`already_known`] is: a substring match over the
/// whole file counts a commented-out `# Host github.com-dotfix (disabled)`
/// as the real thing and silently skips appending the stanza, leaving the
/// key configured nowhere.
fn declares_host(contents: &str, alias: &str) -> bool {
    contents.lines().any(|line| {
        let mut fields = line.split_whitespace();
        match fields.next() {
            Some(keyword) if keyword.eq_ignore_ascii_case("Host") => {
                fields.any(|pattern| pattern == alias)
            }
            _ => false,
        }
    })
}

/// `github.com-dotfix` + `example/dotfiles` → `git@github.com-dotfix:example/dotfiles.git`
pub fn clone_url(host_alias: &str, owner_repo: &str) -> String {
    let repo = owner_repo.trim_end_matches(".git");
    format!("git@{host_alias}:{repo}.git")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::fake::FakeExec;

    /// GitHub answers a successful `ssh -T` with exit status 1 and this line,
    /// which is why the message matters more than the status.
    const GREETING: &str = "Hi example-user! You've successfully authenticated, \
                            but GitHub does not provide shell access.\n";

    fn key(host: &str) -> String {
        format!("ssh {} -T git@{host}", PROBE_ARGS.join(" "))
    }

    #[test]
    fn the_probe_can_never_prompt() {
        assert!(
            PROBE_ARGS.contains(&"-o") && PROBE_ARGS.contains(&"BatchMode=yes"),
            "without BatchMode a GUI with no TTY hangs instead of failing: {PROBE_ARGS:?}"
        );
        assert!(
            PROBE_ARGS.iter().any(|a| a.starts_with("ConnectTimeout=")),
            "a probe with no timeout is a hang with extra steps: {PROBE_ARGS:?}"
        );
    }

    #[test]
    fn a_working_setup_is_ready() {
        let exec = FakeExec::new([(key("github.com").as_str(), GREETING)]);
        assert_eq!(probe(&exec, "github.com"), Reachability::Ready);
    }

    #[test]
    fn a_refused_key_asks_for_one() {
        let exec = FakeExec::new([]).with_error(
            &key("github.com"),
            "git@github.com: Permission denied (publickey).",
        );
        assert_eq!(probe(&exec, "github.com"), Reachability::NeedsKey);
    }

    #[test]
    fn the_probe_targets_the_host_it_was_given() {
        let exec = FakeExec::new([]).with_error(
            &key("github.com-dotfix"),
            "git@github.com-dotfix: Permission denied (publickey).",
        );
        let _ = probe(&exec, "github.com-dotfix");
        assert_eq!(exec.calls(), vec![key("github.com-dotfix")]);
    }

    #[test]
    fn an_unreachable_host_is_reported_with_detail() {
        let error_msg = "ssh: connect to host example.com port 22: Operation timed out";
        let exec = FakeExec::new([]).with_error(&key("example.com"), error_msg);
        let result = probe(&exec, "example.com");
        assert_eq!(
            result,
            Reachability::Unreachable {
                detail: format!("command `{}` failed: {error_msg}", key("example.com"))
            }
        );
    }

    use std::path::Path;

    use crate::ports::Fsys;
    use crate::ports::fake::FakeFsys;

    const SCANNED: &str = "github.com ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl\n";
    const GOOD_FP: &str = "SHA256:+DiY3wvvV6TuJJhbpZisF/zLDA0zPMSvHdkr4UvCOqU";

    fn scan_exec() -> FakeExec {
        FakeExec::new([
            ("ssh-keyscan -t ed25519 github.com", SCANNED),
            (
                "ssh-keygen -lf -",
                &format!("256 {GOOD_FP} github.com (ED25519)\n"),
            ),
        ])
    }

    #[test]
    fn a_matching_fingerprint_is_written() {
        let fs = FakeFsys::new();
        let added = ensure_host_known(
            &fs,
            &scan_exec(),
            Path::new("/Users/test"),
            "github.com",
            &[GOOD_FP],
        )
        .unwrap();

        assert!(added);
        let known = fs.read(Path::new("/Users/test/.ssh/known_hosts")).unwrap();
        assert!(known.contains("github.com ssh-ed25519"));
    }

    #[test]
    fn a_mismatched_fingerprint_writes_nothing_and_says_both_values() {
        let fs = FakeFsys::new();
        let err = ensure_host_known(
            &fs,
            &scan_exec(),
            Path::new("/Users/test"),
            "github.com",
            &["SHA256:definitely-not-the-right-one"],
        )
        .unwrap_err();

        let msg = err.to_string();
        assert!(msg.contains(GOOD_FP), "must report what it got: {msg}");
        assert!(
            msg.contains("definitely-not-the-right-one"),
            "and what it expected: {msg}"
        );
        assert!(
            !fs.exists(Path::new("/Users/test/.ssh/known_hosts")),
            "a mismatch must leave known_hosts untouched"
        );
    }

    #[test]
    fn an_existing_entry_is_left_alone() {
        let fs = FakeFsys::from([("/Users/test/.ssh/known_hosts", SCANNED)]);
        let added = ensure_host_known(
            &fs,
            &scan_exec(),
            Path::new("/Users/test"),
            "github.com",
            &[GOOD_FP],
        )
        .unwrap();

        assert!(!added, "an entry that is already there is not added twice");
        assert_eq!(
            fs.read(Path::new("/Users/test/.ssh/known_hosts")).unwrap(),
            SCANNED
        );
    }

    #[test]
    fn an_unrelated_entry_that_merely_contains_the_host_does_not_skip_the_check() {
        // Both of these contain the substring "github.com" while being
        // entirely different hosts. A substring match over the whole file
        // would skip scanning and fingerprint verification altogether — the
        // one thing this function exists to do.
        for other in [
            "[ssh.github.com]:443 ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOther\n",
            "gist.github.com ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOther\n",
        ] {
            let fs = FakeFsys::from([("/Users/test/.ssh/known_hosts", other)]);
            let added = ensure_host_known(
                &fs,
                &scan_exec(),
                Path::new("/Users/test"),
                "github.com",
                &[GOOD_FP],
            )
            .unwrap();

            assert!(added, "`{other}` is not an entry for github.com");
            let known = fs.read(Path::new("/Users/test/.ssh/known_hosts")).unwrap();
            assert!(known.contains(other.trim()), "must keep what was there");
            assert!(known.contains("github.com ssh-ed25519"));
        }
    }

    #[test]
    fn a_host_entry_is_matched_ignoring_case_and_a_trailing_dot() {
        // `GitHub.com` and `github.com.` are the same host. Neither may be
        // pinned a second time.
        let fs = FakeFsys::from([(
            "/Users/test/.ssh/known_hosts",
            "GitHub.com ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkV\n",
        )]);
        let added = ensure_host_known(
            &fs,
            &scan_exec(),
            Path::new("/Users/test"),
            "github.com.",
            &[GOOD_FP],
        )
        .unwrap();
        assert!(!added);
    }

    const HASHED_GITHUB: &str = "|1|F4gwXrBIl3FQeTu1QTOb2mixr8w=|t/0Fk+S4ZaEAbgY+kCZO8rswT7I= \
         ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkV\n";

    /// Like [`scan_exec`], but `ssh-keygen -F` reports the host as present —
    /// which is what a real run against a hashed `known_hosts` returns.
    fn exec_with_hashed_hit() -> FakeExec {
        FakeExec::new([
            ("ssh-keyscan -t ed25519 github.com", SCANNED),
            (
                "ssh-keygen -lf -",
                &format!("256 {GOOD_FP} github.com (ED25519)\n"),
            ),
            (
                "ssh-keygen -F github.com -f /Users/test/.ssh/known_hosts",
                "# Host github.com found: line 2\n|1|abc=|def= ssh-ed25519 AAAA\n",
            ),
        ])
    }

    #[test]
    fn a_hashed_entry_counts_as_known_and_is_not_duplicated() {
        // With `HashKnownHosts yes` the host field is `|1|salt|hash`, not the
        // name — so reading the file as text finds no github.com and appends a
        // second, plaintext entry. On every run. The maintainer's own Mac has
        // 92 of 98 lines hashed and already carries one such duplicate.
        //
        // Only ssh can match a hashed entry, because only ssh knows the salt.
        let fs = FakeFsys::from([("/Users/test/.ssh/known_hosts", HASHED_GITHUB)]);
        let added = ensure_host_known(
            &fs,
            &exec_with_hashed_hit(),
            Path::new("/Users/test"),
            "github.com",
            &[GOOD_FP],
        )
        .unwrap();

        assert!(!added, "a hashed entry is still an entry");
        assert_eq!(
            fs.read(Path::new("/Users/test/.ssh/known_hosts")).unwrap(),
            HASHED_GITHUB,
            "nothing may be appended"
        );
    }

    #[test]
    fn a_hashed_file_without_this_host_still_gets_the_entry() {
        // The other half: hashing must not make every host look known. When
        // `ssh-keygen -F` finds nothing it exits non-zero, and the scan and
        // fingerprint check proceed exactly as before.
        let fs = FakeFsys::from([(
            "/Users/test/.ssh/known_hosts",
            "|1|other=|salt= ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOther\n",
        )]);
        let exec = scan_exec().with_error(
            "ssh-keygen -F github.com -f /Users/test/.ssh/known_hosts",
            "",
        );
        let added = ensure_host_known(
            &fs,
            &exec,
            Path::new("/Users/test"),
            "github.com",
            &[GOOD_FP],
        )
        .unwrap();

        assert!(added);
        assert!(
            fs.read(Path::new("/Users/test/.ssh/known_hosts"))
                .unwrap()
                .contains("github.com ssh-ed25519")
        );
    }

    #[test]
    fn a_comma_separated_entry_still_counts_as_known() {
        let fs = FakeFsys::from([(
            "/Users/test/.ssh/known_hosts",
            "github.com,140.82.121.4 ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkV\n",
        )]);
        let added = ensure_host_known(
            &fs,
            &scan_exec(),
            Path::new("/Users/test"),
            "github.com",
            &[GOOD_FP],
        )
        .unwrap();
        assert!(!added);
    }

    #[test]
    fn the_shipped_github_fingerprints_are_not_empty() {
        assert!(
            !GITHUB_FINGERPRINTS.is_empty(),
            "pinning with an empty list would silently accept anything"
        );
        assert!(GITHUB_FINGERPRINTS.iter().all(|f| f.starts_with("SHA256:")));
    }

    // `keygen_args` is tested directly rather than through a `FakeExec`
    // round-trip: `FakeExec` cannot simulate `ssh-keygen` writing the key
    // files it is asked to produce, so no fake arrangement can exercise
    // "generate, then read what was generated" without one test lying about
    // what the other tests for. Testing the pure argument-building function
    // is a stronger check anyway — it inspects the actual flags rather than
    // string-matching a joined command line.
    #[test]
    fn keygen_args_have_no_passphrase_and_scope_the_comment_to_the_machine() {
        let args = keygen_args(
            Path::new("/Users/test/.ssh/dotfix_box-one_ed25519"),
            "box-one",
        );

        assert!(
            args.windows(2).any(|w| w[0] == "-t" && w[1] == "ed25519"),
            "must request an ed25519 key: {args:?}"
        );

        let n_index = args
            .iter()
            .position(|a| a == "-N")
            .expect("must pass -N to set a passphrase: {args:?}");
        assert_eq!(
            args.get(n_index + 1).map(String::as_str),
            Some(""),
            "an empty passphrase is required — a passphrase needs an agent, \
             which is the whole problem this key exists to avoid: {args:?}"
        );

        let c_index = args
            .iter()
            .position(|a| a == "-C")
            .expect("must pass -C to set a comment: {args:?}");
        assert_eq!(
            args.get(c_index + 1).map(String::as_str),
            Some("dotfix@box-one"),
            "the comment must carry the machine name: {args:?}"
        );
    }

    #[test]
    fn an_existing_key_is_reused_rather_than_regenerated() {
        let fs = FakeFsys::from([(
            "/Users/test/.ssh/dotfix_box-one_ed25519.pub",
            "ssh-ed25519 AAAA... dotfix@box-one\n",
        )]);
        let exec = FakeExec::new([]);

        let key = ensure_deploy_key(&fs, &exec, Path::new("/Users/test"), "box-one").unwrap();

        assert!(key.public.starts_with("ssh-ed25519 "));
        assert!(
            exec.calls().is_empty(),
            "regenerating would invalidate the deploy key already on GitHub"
        );
    }

    #[test]
    fn the_ssh_config_stanza_scopes_the_key_to_this_alias_only() {
        let fs = FakeFsys::new();
        let key = DeployKey {
            public: "ssh-ed25519 AAAA...".into(),
            path: "/Users/test/.ssh/dotfix_box-one_ed25519".into(),
            host_alias: "github.com-dotfix".into(),
        };
        ensure_ssh_config(&fs, Path::new("/Users/test"), &key).unwrap();

        let cfg = fs.read(Path::new("/Users/test/.ssh/config")).unwrap();
        assert!(cfg.contains("Host github.com-dotfix"));
        assert!(cfg.contains("HostName github.com"));
        assert!(cfg.contains("IdentityFile /Users/test/.ssh/dotfix_box-one_ed25519"));
        assert!(
            cfg.contains("IdentitiesOnly yes"),
            "without this the key leaks into the user's other ssh targets"
        );
    }

    #[test]
    fn writing_the_stanza_twice_does_not_duplicate_it() {
        let fs = FakeFsys::new();
        let key = DeployKey {
            public: "ssh-ed25519 AAAA...".into(),
            path: "/Users/test/.ssh/dotfix_box-one_ed25519".into(),
            host_alias: "github.com-dotfix".into(),
        };
        ensure_ssh_config(&fs, Path::new("/Users/test"), &key).unwrap();
        ensure_ssh_config(&fs, Path::new("/Users/test"), &key).unwrap();

        let cfg = fs.read(Path::new("/Users/test/.ssh/config")).unwrap();
        assert_eq!(cfg.matches("Host github.com-dotfix").count(), 1);
    }

    #[test]
    fn a_commented_out_stanza_does_not_pass_for_the_real_one() {
        // A line mentioning the alias inside a comment is not a `Host`
        // declaration. Treating it as one left the key configured nowhere,
        // so every clone through the alias failed.
        let fs = FakeFsys::from([(
            "/Users/test/.ssh/config",
            "# Host github.com-dotfix (disabled)\n",
        )]);
        let key = DeployKey {
            public: "ssh-ed25519 AAAA...".into(),
            path: "/Users/test/.ssh/dotfix_box-one_ed25519".into(),
            host_alias: "github.com-dotfix".into(),
        };
        ensure_ssh_config(&fs, Path::new("/Users/test"), &key).unwrap();

        let cfg = fs.read(Path::new("/Users/test/.ssh/config")).unwrap();
        assert!(
            cfg.contains("IdentityFile /Users/test/.ssh/dotfix_box-one_ed25519"),
            "the real stanza must still be appended: {cfg}"
        );
    }

    #[test]
    fn an_alias_listed_among_several_patterns_counts_as_declared() {
        let fs = FakeFsys::from([(
            "/Users/test/.ssh/config",
            "Host example.com github.com-dotfix\n  User git\n",
        )]);
        let key = DeployKey {
            public: "ssh-ed25519 AAAA...".into(),
            path: "/Users/test/.ssh/dotfix_box-one_ed25519".into(),
            host_alias: "github.com-dotfix".into(),
        };
        ensure_ssh_config(&fs, Path::new("/Users/test"), &key).unwrap();

        let cfg = fs.read(Path::new("/Users/test/.ssh/config")).unwrap();
        assert!(
            !cfg.contains("added by dotfix"),
            "the alias is already declared: {cfg}"
        );
    }

    #[test]
    fn a_machine_name_containing_a_dot_still_finds_its_public_key() {
        // `with_extension` would turn `dotfix_mbp.local_ed25519` into
        // `dotfix_mbp.pub`, so the read failed and every retry re-ran
        // `ssh-keygen` against an existing path — which asks to overwrite
        // and dies on the nulled stdin of a GUI.
        let fs = FakeFsys::from([(
            "/Users/test/.ssh/dotfix_mbp.local_ed25519.pub",
            "ssh-ed25519 AAAA... dotfix@mbp.local\n",
        )]);
        let exec = FakeExec::new([]);

        let key = ensure_deploy_key(&fs, &exec, Path::new("/Users/test"), "mbp.local").unwrap();

        assert_eq!(
            key.path,
            Path::new("/Users/test/.ssh/dotfix_mbp.local_ed25519")
        );
        assert!(key.public.starts_with("ssh-ed25519 "));
        assert!(
            exec.calls().is_empty(),
            "the existing key must be found, not regenerated"
        );
    }

    #[test]
    fn an_existing_ssh_config_is_appended_to_not_replaced() {
        let fs = FakeFsys::from([("/Users/test/.ssh/config", "Host *\n  AddKeysToAgent yes\n")]);
        let key = DeployKey {
            public: "ssh-ed25519 AAAA...".into(),
            path: "/Users/test/.ssh/dotfix_box-one_ed25519".into(),
            host_alias: "github.com-dotfix".into(),
        };
        ensure_ssh_config(&fs, Path::new("/Users/test"), &key).unwrap();

        let cfg = fs.read(Path::new("/Users/test/.ssh/config")).unwrap();
        assert!(
            cfg.contains("AddKeysToAgent yes"),
            "must not clobber the user's config"
        );
        assert!(cfg.contains("Host github.com-dotfix"));
    }

    #[test]
    fn the_clone_url_uses_the_alias() {
        assert_eq!(
            clone_url("github.com-dotfix", "example/dotfiles"),
            "git@github.com-dotfix:example/dotfiles.git"
        );
    }
}

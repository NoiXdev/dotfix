//! The fragment dotfix writes into `.zshrc` must cost nothing at shell start
//! and print nothing when there is no drift. Verified against real zsh.

use std::fs;
use std::path::Path;
use std::process::Command;

const HOOK: &str = r#"() {
  local f=${XDG_STATE_HOME:-$HOME/.local/state}/dotfix/status.line
  [[ -s $f ]] && print -r -- "$(<$f)"
}"#;

fn run_hook(home: &Path) -> String {
    let out = Command::new("zsh")
        .arg("-c")
        .arg(HOOK)
        .env("HOME", home)
        .env_remove("XDG_STATE_HOME")
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn write_status_line(home: &Path, line: &str) {
    let dir = home.join(".local/state/dotfix");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("status.line"), line).unwrap();
}

#[test]
fn prints_nothing_when_there_is_no_status_file() {
    let home = tempfile::tempdir().unwrap();
    assert_eq!(run_hook(home.path()), "");
}

#[test]
fn prints_nothing_when_the_status_file_is_empty() {
    let home = tempfile::tempdir().unwrap();
    write_status_line(home.path(), "");
    assert_eq!(run_hook(home.path()), "");
}

#[test]
fn prints_the_line_when_there_is_drift() {
    let home = tempfile::tempdir().unwrap();
    write_status_line(home.path(), "↯ dotfix: 2 changes   →  dotfix apply");
    assert_eq!(
        run_hook(home.path()).trim(),
        "↯ dotfix: 2 changes   →  dotfix apply"
    );
}

/// The fragment `init` scaffolds must be exactly the one tested above.
#[test]
fn the_scaffolded_fragment_matches_the_tested_hook() {
    let home = tempfile::tempdir().unwrap();
    let bin = home.path().join("fakebin");
    fs::create_dir_all(&bin).unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        let script = "#!/bin/sh\ncase \"$1\" in leaves) : ;; esac\nexit 0\n";
        fs::write(bin.join("brew"), script).unwrap();
        fs::set_permissions(bin.join("brew"), fs::Permissions::from_mode(0o755)).unwrap();
    }

    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    assert_cmd::Command::cargo_bin("dotfix")
        .unwrap()
        .env("HOME", home.path())
        .env("PATH", path)
        .args(["init", "--set-up-new", "--machine", "box-one", "--yes"])
        .assert()
        .success();

    let fragment = fs::read_to_string(
        home.path()
            .join("dotfiles/sets/core/shell/99-dotfix-status.zsh"),
    )
    .unwrap();
    assert!(
        fragment.contains(HOOK),
        "scaffolded fragment drifted from the tested hook:\n{fragment}"
    );
}

//! Shared fixtures for the CLI integration tests.
//!
//! Homebrew is stubbed by putting a small shell script named `brew` at the
//! front of `PATH`. That exercises the real `RealBrew` code path instead of
//! bypassing it, and keeps the production binary free of test seams.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use assert_cmd::Command;

/// Write a fake `brew` that reports `leaves` and records every call into
/// `<home>/fakebin/calls.log`.
pub fn fake_brew(home: &Path, leaves: &[&str]) -> PathBuf {
    let bin = home.join("fakebin");
    fs::create_dir_all(&bin).unwrap();

    let script = format!(
        r#"#!/bin/sh
log="$(dirname "$0")/calls.log"
echo "$@" >> "$log"
case "$1" in
  leaves) printf '%s' '{leaves}' ;;
  --version) echo "Homebrew 7.0.0" ;;
  *) : ;;
esac
exit 0
"#,
        leaves = leaves.iter().map(|l| format!("{l}\n")).collect::<String>()
    );

    let path = bin.join("brew");
    fs::write(&path, script).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    bin
}

#[allow(dead_code)]
pub fn brew_calls(home: &Path) -> String {
    fs::read_to_string(home.join("fakebin/calls.log")).unwrap_or_default()
}

/// A `dotfix` invocation with `$HOME` pointed at the fixture and the fake
/// `brew` first on `PATH`.
pub fn dotfix(home: &Path) -> Command {
    let bin = home.join("fakebin");
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let mut cmd = Command::cargo_bin("dotfix").unwrap();
    cmd.env("HOME", home).env("PATH", path);
    cmd
}

/// Minimal repository: one set with one package, one fragment, one machine.
#[allow(dead_code)]
pub fn fixture(home: &Path) -> PathBuf {
    let repo = home.join("dotfiles");
    fs::create_dir_all(repo.join("sets/core/files")).unwrap();
    fs::create_dir_all(repo.join("sets/core/shell")).unwrap();
    fs::create_dir_all(repo.join("sets/extra")).unwrap();
    fs::create_dir_all(repo.join("machines")).unwrap();
    fs::create_dir_all(home.join(".config/dotfix")).unwrap();

    fs::write(repo.join("dotfix.toml"), "schema_version = 1\n").unwrap();
    fs::write(
        repo.join("sets/core/set.toml"),
        "[packages]\nbrew = [\"fake-pkg\"]\n\n[[files]]\nsource = \"files/rc.tmpl\"\ntarget = \"~/.rc\"\n",
    )
    .unwrap();
    fs::write(repo.join("sets/core/files/rc.tmpl"), "home={{ home }}\n").unwrap();
    fs::write(repo.join("sets/core/shell/10-x.zsh"), "export X=1\n").unwrap();
    fs::write(repo.join("sets/extra/set.toml"), "[packages]\nbrew = []\n").unwrap();
    fs::write(repo.join("machines/box-one.toml"), "sets = [\"core\"]\n").unwrap();
    fs::write(
        home.join(".config/dotfix/config.toml"),
        format!("repo = \"{}\"\nmachine = \"box-one\"\n", repo.display()),
    )
    .unwrap();

    repo
}

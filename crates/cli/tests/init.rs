mod common;

use std::fs;

use common::{dotfix, fake_brew};

#[test]
fn set_up_new_scaffolds_a_repository_and_local_config() {
    let home = tempfile::tempdir().unwrap();
    fake_brew(home.path(), &["fake-pkg", "other-pkg"]);

    dotfix(home.path())
        .args(["init", "--set-up-new", "--machine", "box-one", "--yes"])
        .assert()
        .success();

    let repo = home.path().join("dotfiles");
    assert!(repo.join("dotfix.toml").exists());
    assert!(repo.join("sets/core/set.toml").exists());
    assert!(repo.join("machines/box-one.toml").exists());

    let core = fs::read_to_string(repo.join("sets/core/set.toml")).unwrap();
    assert!(
        core.contains("fake-pkg") && core.contains("other-pkg"),
        "installed packages become a proposal, got: {core}"
    );

    let hook = fs::read_to_string(repo.join("sets/core/shell/99-dotfix-status.zsh")).unwrap();
    assert!(hook.contains("status.line"));

    let local = fs::read_to_string(home.path().join(".config/dotfix/config.toml")).unwrap();
    assert!(local.contains("box-one"));

    assert!(
        home.path()
            .join("Library/LaunchAgents/dev.noix.dotfix.plist")
            .exists()
    );
}

#[test]
fn a_scaffolded_repository_applies_cleanly() {
    let home = tempfile::tempdir().unwrap();
    fake_brew(home.path(), &["fake-pkg"]);

    dotfix(home.path())
        .args(["init", "--set-up-new", "--machine", "box-one", "--yes"])
        .assert()
        .success();

    dotfix(home.path())
        .args(["apply", "--yes", "--no-sync"])
        .assert()
        .success();

    let zshrc = fs::read_to_string(home.path().join(".zshrc")).unwrap();
    assert!(zshrc.contains(".local/bin"));
    assert!(zshrc.contains("status.line"));
}

#[test]
fn init_refuses_to_overwrite_an_existing_local_config() {
    let home = tempfile::tempdir().unwrap();
    fake_brew(home.path(), &[]);
    fs::create_dir_all(home.path().join(".config/dotfix")).unwrap();
    fs::write(
        home.path().join(".config/dotfix/config.toml"),
        "repo = \"/somewhere\"\nmachine = \"existing\"\n",
    )
    .unwrap();

    dotfix(home.path())
        .args(["init", "--set-up-new", "--machine", "box-two", "--yes"])
        .assert()
        .failure();
}

/// Regression: `init --set-up-new` used to leave a plain directory behind, so
/// the very next `dotfix apply` — without `--no-sync`, i.e. how a user actually
/// runs it — failed with "not a git repository".
#[test]
fn a_scaffolded_repository_is_a_git_repository_with_an_initial_commit() {
    let home = tempfile::tempdir().unwrap();
    fake_brew(home.path(), &["fake-pkg"]);

    dotfix(home.path())
        .args(["init", "--set-up-new", "--machine", "box-one", "--yes"])
        .assert()
        .success();

    let repo = home.path().join("dotfiles");
    assert!(
        repo.join(".git").exists(),
        "scaffold must create a git repo"
    );

    let log = std::process::Command::new("git")
        .arg("-C")
        .arg(&repo)
        .args(["log", "--oneline"])
        .output()
        .unwrap();
    assert!(log.status.success());
    assert_eq!(
        String::from_utf8_lossy(&log.stdout).lines().count(),
        1,
        "expected exactly one initial commit"
    );

    let status = std::process::Command::new("git")
        .arg("-C")
        .arg(&repo)
        .args(["status", "--porcelain"])
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&status.stdout).trim().is_empty(),
        "the scaffold must be fully committed"
    );
}

/// Regression: the normal invocation has no `--no-sync`.
#[test]
fn apply_works_on_a_scaffolded_repository_without_no_sync() {
    let home = tempfile::tempdir().unwrap();
    fake_brew(home.path(), &["fake-pkg"]);

    dotfix(home.path())
        .args(["init", "--set-up-new", "--machine", "box-one", "--yes"])
        .assert()
        .success();

    // No --no-sync: a local-only repository has no remote, which must be a
    // no-op rather than a hard failure.
    dotfix(home.path())
        .args(["apply", "--yes"])
        .assert()
        .success();
    dotfix(home.path()).arg("status").assert().success();

    assert!(
        fs::read_to_string(home.path().join(".zshrc"))
            .unwrap()
            .contains(".local/bin")
    );
}

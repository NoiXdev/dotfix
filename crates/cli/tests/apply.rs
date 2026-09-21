mod common;

use std::fs;

use common::{brew_calls, dotfix, fake_brew, fixture};

#[test]
fn dry_run_changes_nothing() {
    let home = tempfile::tempdir().unwrap();
    fixture(home.path());
    fake_brew(home.path(), &[]);

    dotfix(home.path())
        .args(["apply", "--dry-run", "--no-sync"])
        .assert()
        .success();

    assert!(!home.path().join(".rc").exists());
    assert!(!brew_calls(home.path()).contains("install fake-pkg"));
}

#[test]
fn apply_writes_files_installs_packages_and_records_state() {
    let home = tempfile::tempdir().unwrap();
    fixture(home.path());
    fake_brew(home.path(), &[]);

    dotfix(home.path())
        .args(["apply", "--yes", "--no-sync"])
        .assert()
        .success();

    let rc = fs::read_to_string(home.path().join(".rc")).unwrap();
    assert_eq!(rc, format!("home={}\n", home.path().display()));

    let zshrc = fs::read_to_string(home.path().join(".zshrc")).unwrap();
    assert!(zshrc.contains("export X=1"));
    assert!(zshrc.contains("source ~/.zshrc.local"));

    assert!(brew_calls(home.path()).contains("install fake-pkg"));
    assert!(
        home.path()
            .join(".local/state/dotfix/applied.json")
            .exists()
    );
}

#[test]
fn a_second_apply_is_a_no_op() {
    let home = tempfile::tempdir().unwrap();
    fixture(home.path());
    // After the first run the package must look installed, otherwise the second
    // run legitimately sees it as missing again.
    fake_brew(home.path(), &["fake-pkg"]);

    for _ in 0..2 {
        dotfix(home.path())
            .args(["apply", "--yes", "--no-sync"])
            .assert()
            .success();
    }

    let backups = home.path().join(".local/state/dotfix/backups");
    let count = fs::read_dir(&backups).map(|d| d.count()).unwrap_or(0);
    assert_eq!(
        count, 0,
        "an idempotent second run must not back anything up"
    );
}

mod common;

use std::fs;

use common::{dotfix, fake_brew, fixture};

#[test]
fn doctor_fails_when_the_launch_agent_is_missing() {
    let home = tempfile::tempdir().unwrap();
    fixture(home.path());
    fake_brew(home.path(), &[]);

    let out = dotfix(home.path()).arg("doctor").output().unwrap();

    assert!(!out.status.success(), "a missing agent must be a failure");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("launch agent"), "got: {stdout}");
    assert!(stdout.contains("check(s) failed"), "got: {stdout}");
}

#[test]
fn install_agent_writes_a_plist_and_that_check_then_passes() {
    let home = tempfile::tempdir().unwrap();
    fixture(home.path());
    fake_brew(home.path(), &[]);

    let out = dotfix(home.path())
        .args(["doctor", "--install-agent"])
        .output()
        .unwrap();

    let plist = home
        .path()
        .join("Library/LaunchAgents/dev.noix.dotfix.plist");
    assert!(plist.exists(), "{}", String::from_utf8_lossy(&out.stdout));

    let contents = fs::read_to_string(&plist).unwrap();
    assert!(contents.contains("--write-status-line"));
    assert!(contents.contains("<key>StartInterval</key>"));

    let after = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(after.contains("[ok  ] launch agent"), "got: {after}");
}

#[test]
fn doctor_warns_when_ssh_config_cannot_persist_keys() {
    let home = tempfile::tempdir().unwrap();
    fixture(home.path());
    fake_brew(home.path(), &[]);
    fs::create_dir_all(home.path().join(".ssh")).unwrap();
    fs::write(home.path().join(".ssh/config"), "Host github.com\n").unwrap();

    let out = dotfix(home.path()).arg("doctor").output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);

    assert!(stdout.contains("AddKeysToAgent"), "got: {stdout}");
}

#[test]
fn keychain_is_the_reported_default_provider() {
    let home = tempfile::tempdir().unwrap();
    fixture(home.path());
    fake_brew(home.path(), &[]);

    let out = dotfix(home.path()).arg("doctor").output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);

    assert!(stdout.contains("macOS Keychain"), "got: {stdout}");
    assert!(
        !stdout.contains("1password"),
        "1Password must only be checked where it is configured"
    );
}

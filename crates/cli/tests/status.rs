mod common;

use std::fs;

use common::{dotfix, fake_brew, fixture};

#[test]
fn status_json_lists_the_incoming_package() {
    let home = tempfile::tempdir().unwrap();
    fixture(home.path());
    fake_brew(home.path(), &[]);

    let out = dotfix(home.path())
        .args(["status", "--json", "--no-sync"])
        .output()
        .unwrap();

    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let classes: Vec<&str> = json["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["class"].as_str().unwrap())
        .collect();
    assert!(classes.contains(&"incoming_package"));
    assert!(classes.contains(&"incoming_file"));
}

#[test]
fn a_locally_installed_package_shows_up_as_unmanaged() {
    let home = tempfile::tempdir().unwrap();
    fixture(home.path());
    fake_brew(home.path(), &["stray-pkg"]);

    let out = dotfix(home.path())
        .args(["status", "--json", "--no-sync"])
        .output()
        .unwrap();

    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let unmanaged = json["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|i| i["class"] == "unmanaged" && i["name"] == "stray-pkg");
    assert!(unmanaged, "got: {}", String::from_utf8_lossy(&out.stdout));
}

#[test]
fn write_status_line_creates_a_readable_one_liner() {
    let home = tempfile::tempdir().unwrap();
    fixture(home.path());
    fake_brew(home.path(), &[]);

    dotfix(home.path())
        .args(["status", "--write-status-line", "--no-sync"])
        .assert()
        .success();

    let line = fs::read_to_string(home.path().join(".local/state/dotfix/status.line")).unwrap();
    assert!(line.starts_with("↯ dotfix:"), "got: {line}");
    assert!(!line.contains('\n'));
}

/// A repository written by a newer dotfix must produce an actionable message,
/// not a parse error and not a silent misreading.
#[test]
fn a_future_schema_version_is_refused_with_a_clear_message() {
    let home = tempfile::tempdir().unwrap();
    let repo = fixture(home.path());
    fake_brew(home.path(), &[]);

    fs::write(repo.join("dotfix.toml"), "schema_version = 2\n").unwrap();

    let out = dotfix(home.path())
        .args(["status", "--no-sync"])
        .output()
        .unwrap();

    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("update dotfix"), "got: {stderr}");
}

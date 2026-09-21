mod common;

use std::fs;

use common::{dotfix, fake_brew, fixture};

#[test]
fn adopt_adds_an_unmanaged_package_to_the_chosen_set() {
    let home = tempfile::tempdir().unwrap();
    let repo = fixture(home.path());
    fake_brew(home.path(), &["stray-pkg"]);

    dotfix(home.path())
        .args(["adopt", "--set", "extra", "--yes", "--no-sync"])
        .assert()
        .success();

    let set = fs::read_to_string(repo.join("sets/extra/set.toml")).unwrap();
    assert!(set.contains("stray-pkg"), "got: {set}");
}

#[test]
fn adopt_refuses_a_credential_shaped_file() {
    let home = tempfile::tempdir().unwrap();
    let repo = fixture(home.path());
    fake_brew(home.path(), &[]);

    // A managed file whose target looks like credentials, edited locally.
    fs::write(
        repo.join("sets/core/set.toml"),
        "[packages]\nbrew = []\n\n[[files]]\nsource = \"files/s3.tmpl\"\ntarget = \"~/.s3cfg\"\n",
    )
    .unwrap();
    fs::write(
        repo.join("sets/core/files/s3.tmpl"),
        "access_key = PLACEHOLDER\n",
    )
    .unwrap();
    fs::write(home.path().join(".s3cfg"), "access_key = REAL_SECRET\n").unwrap();

    let out = dotfix(home.path())
        .args(["adopt", "--yes", "--no-sync"])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("refused"), "got: {stdout}");
    // The secret must not have been written into the repository.
    let tmpl = fs::read_to_string(repo.join("sets/core/files/s3.tmpl")).unwrap();
    assert!(!tmpl.contains("REAL_SECRET"));
}

#[test]
fn sets_enable_writes_the_machine_file() {
    let home = tempfile::tempdir().unwrap();
    let repo = fixture(home.path());
    fake_brew(home.path(), &[]);

    dotfix(home.path())
        .args(["sets", "--enable", "extra"])
        .assert()
        .success();

    let machine = fs::read_to_string(repo.join("machines/box-one.toml")).unwrap();
    assert!(machine.contains("extra"), "got: {machine}");
}

#[test]
fn sets_without_flags_lists_active_and_inactive() {
    let home = tempfile::tempdir().unwrap();
    fixture(home.path());
    fake_brew(home.path(), &[]);

    let out = dotfix(home.path()).arg("sets").output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("[x] core"), "got: {stdout}");
    assert!(stdout.contains("[ ] extra"), "got: {stdout}");
}

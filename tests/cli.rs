use assert_cmd::{Command, assert::Assert};
use predicates::prelude::*;
use std::{env, path::Path};

static ASSET_UUID: &str = "a09c9ba5-45e0-40b8-82cb-55c93ff49125";

fn new_cmd(homedir: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_immichctl"));
    cmd.env("HOME", homedir);
    cmd
}

fn login(homedir: &Path) -> Assert {
    // Load .env file if it exists.
    dotenvy::dotenv().ok();

    let server_url = env::var("IMMICH_SERVER_URL");
    let api_key = env::var("IMMICH_API_KEY");

    // This test is ignored if the environment variables are not set.
    if let (Ok(server), Ok(key)) = (server_url, api_key) {
        let mut cmd = new_cmd(homedir);
        cmd.arg("login").arg(server).arg("--apikey").arg(key);
        cmd.assert().success()
    } else {
        panic!(
            "IMMICH_SERVER_URL and IMMICH_API_KEY environment variables must be set for this test."
        );
    }
}

#[test]
fn test_version_not_logged_in() {
    let homedir = tempfile::tempdir().unwrap();
    let mut cmd = new_cmd(homedir.path());
    cmd.arg("version");
    cmd.assert()
        .success()
        .stdout(
            predicate::str::contains("immichctl version:").and(predicate::str::contains(
                "Not logged in. Cannot determine server version.",
            )),
        );
}

#[test]
fn test_login() {
    let homedir = tempfile::tempdir().unwrap();

    let mut cmd = new_cmd(homedir.path());
    cmd.arg("login")
        .arg("http://immich.")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "Please provide both server URL and --apikey to login",
        ));

    let assert = login(homedir.path());
    assert.stdout(predicate::str::contains("Login successful"));

    let mut cmd = new_cmd(homedir.path());
    cmd.arg("login")
        .assert()
        .success()
        .stdout(predicate::str::contains("Currently logged in to:"));
}

#[test]
fn test_logout() {
    let homedir = tempfile::tempdir().unwrap();
    login(homedir.path());

    let mut cmd = new_cmd(homedir.path());
    cmd.arg("logout");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Logged out."));

    let mut cmd = new_cmd(homedir.path());
    cmd.arg("login")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Not logged in"));
}

#[test]
fn test_selection_not_logged_in() {
    let homedir = tempfile::tempdir().unwrap();
    let mut cmd = new_cmd(homedir.path());
    cmd.arg("selection").arg("add").arg("--id").arg(ASSET_UUID);
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("Error: Not logged in."));
}

#[test]
fn test_selection_add_remove_id() {
    let homedir = tempfile::tempdir().unwrap();
    login(homedir.path());

    let mut cmd = new_cmd(homedir.path());
    cmd.arg("selection").arg("clear");
    cmd.assert().success();

    let mut cmd = new_cmd(homedir.path());
    cmd.arg("selection").arg("add").arg("--id").arg(ASSET_UUID);
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Added 1 asset(s) to selection."));

    let mut cmd = new_cmd(homedir.path());
    cmd.arg("selection").arg("count");
    cmd.assert().success().stdout(predicate::eq("1\n"));

    let mut cmd = new_cmd(homedir.path());
    cmd.arg("selection")
        .arg("remove")
        .arg("--id")
        .arg(ASSET_UUID);
    cmd.assert().success().stdout(predicate::str::contains(
        "Removed 1 asset(s) from selection.",
    ));

    let mut cmd = new_cmd(homedir.path());
    cmd.arg("selection").arg("count");
    cmd.assert().success().stdout(predicate::eq("0\n"));
}

#[test]
fn test_selection_album() {
    let homedir = tempfile::tempdir().unwrap();
    login(homedir.path());

    let mut cmd = new_cmd(homedir.path());
    cmd.arg("selection")
        .arg("add")
        .arg("--album")
        .arg("CF Day EU 2025");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Added 7 asset(s) to selection."));

    let mut cmd = new_cmd(homedir.path());
    cmd.arg("selection")
        .arg("remove")
        .arg("--album")
        .arg("CF Day EU 2025");
    cmd.assert().success().stdout(predicate::str::contains(
        "Removed 7 asset(s) from selection.",
    ));
}

#[test]
fn test_selection_tag() {
    let homedir = tempfile::tempdir().unwrap();
    login(homedir.path());

    let mut cmd = new_cmd(homedir.path());
    cmd.arg("selection")
        .arg("add")
        .arg("--tag")
        .arg("immichctl/tag1");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Added 2 asset(s) to selection."));

    let mut cmd = new_cmd(homedir.path());
    cmd.arg("selection")
        .arg("remove")
        .arg("--tag")
        .arg("immichctl/tag1");
    cmd.assert().success().stdout(predicate::str::contains(
        "Removed 2 asset(s) from selection.",
    ));
}

use assert_cmd::{Command, assert::Assert};
use predicates::prelude::*;
use serial_test::serial;
use std::{env, path::Path};

/*
    The following tests require an Immich server to be running and accessible.
    Set the environment variables IMMICH_SERVER_URL and IMMICH_API_KEY
    to point to the server and provide a valid API key in a .env file.

    These tests are marked with #[serial] to ensure they run sequentially,
    as they depend on shared state (the login session).

    Tests assume the server has certain assets and albums/tags set up:
    - An album named "CF Day EU 2025" containing 7 assets (not modified by tests).
    - A tag named "immichctl/tag1" assigned to 2 assets (not modified by tests).
    - A tag named "immichctl/test_tag" with no assets assigned (modified by tests).
    - An asset with ASSET_UUID exists on the server.

    Tests are supposed to cleanup after running, i.e. all resources on the server are as described above.
    Run test test_cleanup manually in case failing tests resulted in inconsistent server state.
*/

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
#[serial]
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
#[serial]
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
#[serial]
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
#[serial]
fn test_assets_search_not_logged_in() {
    let homedir = tempfile::tempdir().unwrap();
    let mut cmd = new_cmd(homedir.path());
    cmd.arg("assets").arg("search").arg("--id").arg(ASSET_UUID);
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("Error: Not logged in."));
}

#[test]
#[serial]
fn test_assets_search_id() {
    let homedir = tempfile::tempdir().unwrap();
    login(homedir.path());

    let mut cmd = new_cmd(homedir.path());
    cmd.arg("assets").arg("clear");
    cmd.assert().success();

    let mut cmd = new_cmd(homedir.path());
    cmd.arg("assets").arg("search").arg("--id").arg(ASSET_UUID);
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Added 1 asset(s) to selection."));

    let mut cmd = new_cmd(homedir.path());
    cmd.arg("assets").arg("count");
    cmd.assert().success().stdout(predicate::eq("1\n"));

    let mut cmd = new_cmd(homedir.path());
    cmd.arg("assets")
        .arg("search")
        .arg("--remove")
        .arg("--id")
        .arg(ASSET_UUID);
    cmd.assert().success().stdout(predicate::str::contains(
        "Removed 1 asset(s) from selection.",
    ));

    let mut cmd = new_cmd(homedir.path());
    cmd.arg("assets").arg("count");
    cmd.assert().success().stdout(predicate::eq("0\n"));
}

#[test]
#[serial]
fn test_assets_search_album() {
    let homedir = tempfile::tempdir().unwrap();
    login(homedir.path());

    let mut cmd = new_cmd(homedir.path());
    cmd.arg("assets")
        .arg("search")
        .arg("--album")
        .arg("CF Day EU 2025");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Added 7 asset(s) to selection."));

    let mut cmd = new_cmd(homedir.path());
    cmd.arg("assets")
        .arg("search")
        .arg("--remove")
        .arg("--album")
        .arg("CF Day EU 2025");
    cmd.assert().success().stdout(predicate::str::contains(
        "Removed 7 asset(s) from selection.",
    ));
}

#[test]
#[serial]
fn test_assets_search_tag() {
    let homedir = tempfile::tempdir().unwrap();
    login(homedir.path());

    let mut cmd = new_cmd(homedir.path());
    cmd.arg("assets")
        .arg("search")
        .arg("--tag")
        .arg("immichctl/tag1");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Added 2 asset(s) to selection."));

    let mut cmd = new_cmd(homedir.path());
    cmd.arg("assets")
        .arg("search")
        .arg("--remove")
        .arg("--tag")
        .arg("immichctl/tag1");
    cmd.assert().success().stdout(predicate::str::contains(
        "Removed 2 asset(s) from selection.",
    ));
}

#[test]
#[serial]
fn test_assets_list() {
    let homedir = tempfile::tempdir().unwrap();
    login(homedir.path());

    let mut cmd = new_cmd(homedir.path());
    cmd.arg("assets").arg("search").arg("--id").arg(ASSET_UUID);
    cmd.assert().success();

    let mut cmd = new_cmd(homedir.path());
    cmd.arg("assets").arg("list");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("PXL_20251007_101205558.jpg"));

    let mut cmd = new_cmd(homedir.path());
    cmd.arg("assets")
        .arg("list")
        .arg("-c")
        .arg("id")
        .arg("-c")
        .arg("file");
    cmd.assert().success().stdout(predicate::str::contains(
        ASSET_UUID.to_owned() + ",PXL_20251007_101205558.jpg",
    ));

    let mut cmd = new_cmd(homedir.path());
    cmd.arg("assets").arg("list").arg("--format").arg("json");
    cmd.assert().success().stdout(
        predicate::str::contains("PXL_20251007_101205558.jpg")
            .and(predicate::str::contains(ASSET_UUID))
            .and(predicate::str::contains("[{")),
    );

    let mut cmd = new_cmd(homedir.path());
    cmd.arg("assets")
        .arg("list")
        .arg("--format")
        .arg("json-pretty");
    cmd.assert().success().stdout(
        predicate::str::contains("PXL_20251007_101205558.jpg")
            .and(predicate::str::contains(ASSET_UUID))
            .and(predicate::str::contains("[\n  {")),
    );
}

#[test]
#[serial]
fn test_assets_refresh_exif_data() {
    let homedir = tempfile::tempdir().unwrap();
    login(homedir.path());

    let mut cmd = new_cmd(homedir.path());
    cmd.arg("assets").arg("search").arg("--id").arg(ASSET_UUID);
    cmd.assert().success();

    let mut cmd = new_cmd(homedir.path());
    cmd.arg("assets")
        .arg("list")
        .arg("-c")
        .arg("file")
        .arg("-c")
        .arg("datetime")
        .arg("-c")
        .arg("exif-datetime");
    cmd.assert().success().stdout(predicate::str::contains(
        "PXL_20251007_101205558.jpg,2025-10-07T12:12:05.558+02:00,\n",
    ));

    let mut cmd = new_cmd(homedir.path());
    cmd.arg("assets").arg("refresh");
    cmd.assert().success();

    let mut cmd = new_cmd(homedir.path());
    cmd.arg("assets")
        .arg("list")
        .arg("-c")
        .arg("file")
        .arg("-c")
        .arg("datetime")
        .arg("-c")
        .arg("exif-datetime");
    cmd.assert().success().stdout(predicate::str::contains(
        "PXL_20251007_101205558.jpg,2025-10-07T12:12:05.558+02:00,2025-10-07T12:12:05.558+02:00\n",
    ));
}

#[test]
#[serial]
fn test_assets_datatime_dryrun() {
    let homedir = tempfile::tempdir().unwrap();
    login(homedir.path());

    let mut cmd = new_cmd(homedir.path());
    cmd.arg("assets").arg("search").arg("--id").arg(ASSET_UUID);
    cmd.assert().success();

    let mut cmd = new_cmd(homedir.path());
    cmd.arg("assets").arg("datetime").arg("--dry-run");
    cmd.assert().success().stdout(predicate::str::contains(
        "PXL_20251007_101205558.jpg: 2025-10-07 12:12:05.558 +02:00 -> 2025-10-07 12:12:05.558 +02:00",
    ));

    let mut cmd = new_cmd(homedir.path());
    cmd.arg("assets")
        .arg("datetime")
        .arg("--timezone")
        .arg("+00:00")
        .arg("--dry-run");
    cmd.assert().success().stdout(predicate::str::contains(
        "PXL_20251007_101205558.jpg: 2025-10-07 12:12:05.558 +02:00 -> 2025-10-07 10:12:05.558 +00:00",
    ));

    let mut cmd = new_cmd(homedir.path());
    cmd.arg("assets")
        .arg("datetime")
        .arg("--offset")
        .arg("+1h30m")
        .arg("--dry-run");
    cmd.assert().success().stdout(predicate::str::contains(
        "PXL_20251007_101205558.jpg: 2025-10-07 12:12:05.558 +02:00 -> 2025-10-07 13:42:05.558 +02:00",
    ));
}

#[test]
#[serial]
fn test_assets_datatime() {
    let homedir = tempfile::tempdir().unwrap();
    login(homedir.path());

    let mut cmd = new_cmd(homedir.path());
    cmd.arg("assets").arg("search").arg("--id").arg(ASSET_UUID);
    cmd.assert().success();

    // dummy change
    let mut cmd = new_cmd(homedir.path());
    cmd.arg("assets").arg("datetime");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Updated date/time for 1 assets."));
    let mut cmd = new_cmd(homedir.path());
    cmd.arg("assets")
        .arg("list")
        .arg("-c")
        .arg("file")
        .arg("-c")
        .arg("datetime")
        .arg("-c")
        .arg("exif-datetime");
    cmd.assert().success().stdout(predicate::str::contains(
        "PXL_20251007_101205558.jpg,2025-10-07T12:12:05.558+02:00,2025-10-07T12:12:05.558+02:00\n",
    ));

    // for more tests: need a command to reset the datetime to the original value
}

#[test]
#[serial]
fn test_tag() {
    let homedir = tempfile::tempdir().unwrap();
    login(homedir.path());

    // check that test_tag is not used
    let mut cmd = new_cmd(homedir.path());
    cmd.arg("assets")
        .arg("search")
        .arg("--tag")
        .arg("immichctl/test_tag");
    cmd.assert().success();
    let mut cmd = new_cmd(homedir.path());
    cmd.arg("assets").arg("count");
    cmd.assert().success().stdout(predicate::eq("0\n"));

    let mut cmd = new_cmd(homedir.path());
    cmd.arg("assets")
        .arg("search")
        .arg("--tag")
        .arg("immichctl/tag1");
    cmd.assert().success();
    let mut cmd = new_cmd(homedir.path());
    cmd.arg("assets").arg("count");
    cmd.assert().success().stdout(predicate::eq("2\n"));

    let mut cmd = new_cmd(homedir.path());
    cmd.arg("tag").arg("assign").arg("immichctl/test_tag");
    cmd.assert().success().stdout(predicate::str::contains(
        "Tagged 2 assets with 'immichctl/test_tag'.",
    ));

    let mut cmd = new_cmd(homedir.path());
    cmd.arg("tag").arg("unassign").arg("immichctl/test_tag");
    cmd.assert().success().stdout(predicate::str::contains(
        "Untagged 2 assets from 'immichctl/test_tag'.",
    ));
}

#[test]
#[serial]
fn test_cleanup() {
    let homedir = tempfile::tempdir().unwrap();
    login(homedir.path());

    // check that test_tag is not used
    let mut cmd = new_cmd(homedir.path());
    cmd.arg("assets")
        .arg("search")
        .arg("--tag")
        .arg("immichctl/test_tag");
    cmd.assert().success();

    let mut cmd = new_cmd(homedir.path());
    cmd.arg("tag").arg("unassign").arg("immichctl/test_tag");
    cmd.assert().success();
}

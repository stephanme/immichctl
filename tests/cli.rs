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

fn wait_for_running_jobs(homedir: &Path) {
    let mut ok = false;
    for _i in 1..=50 {
        std::thread::sleep(std::time::Duration::from_millis(200));
        let mut cmd = new_cmd(homedir);
        cmd.arg("curl").arg("/jobs");
        let output = cmd.output().expect("curl /jobs failed");
        let stdout = String::from_utf8_lossy(&output.stdout);
        //println!("Jobs: {}", stdout);
        // parse stdout as json and wait until no jobs are running, i.e. *.queueStatus.active == false
        let jobs: serde_json::Value =
            serde_json::from_str(&stdout).expect("Invalid JSON from /jobs");
        let active_queues = jobs
            .as_object()
            .expect("Jobs is not an object")
            .iter()
            .filter(|job| {
                job.1
                    .get("queueStatus")
                    .and_then(|qs| qs.get("isActive"))
                    .and_then(|a| a.as_bool())
                    .unwrap_or(true)
            })
            .count();

        if active_queues == 0 {
            //println!("No active jobs remaining after {} iterations", _i);
            ok = true;
            break;
        }
    }
    assert!(ok, "Background jobs still active after waiting period");
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
    let mut listcmd = new_cmd(homedir.path());
    listcmd
        .arg("assets")
        .arg("list")
        .arg("-c")
        .arg("file")
        .arg("-c")
        .arg("datetime")
        .arg("-c")
        .arg("exif-datetime");
    listcmd.assert().success().stdout(predicate::str::contains(
        "PXL_20251007_101205558.jpg,2025-10-07T12:12:05.558+02:00,2025-10-07T12:12:05.558+02:00\n",
    ));

    // offset +1h
    let mut cmd = new_cmd(homedir.path());
    cmd.arg("assets").arg("datetime").arg("--offset").arg("+1h");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Updated date/time for 1 assets."));
    // PUT assets returns updated exif metadata but stale asset metadata, asset metadata seems to update by an immich background job (sidecar job)
    listcmd.assert().success().stdout(predicate::str::contains(
        "PXL_20251007_101205558.jpg,2025-10-07T12:12:05.558+02:00,2025-10-07T13:12:05.558+02:00\n",
    ));
    wait_for_running_jobs(homedir.path());
    let mut refreshcmd = new_cmd(homedir.path());
    refreshcmd.arg("assets").arg("refresh");
    refreshcmd.assert().success();
    listcmd.assert().success().stdout(predicate::str::contains(
        "PXL_20251007_101205558.jpg,2025-10-07T13:12:05.558+02:00,2025-10-07T13:12:05.558+02:00\n",
    ));

    // timezone
    let mut cmd = new_cmd(homedir.path());
    cmd.arg("assets")
        .arg("datetime")
        .arg("--timezone")
        .arg("+00:00");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Updated date/time for 1 assets."));
    // PUT assets returns updated exif metadata but stale asset metadata, asset metadata seems to update by an immich background job (metadataExtraction job)
    listcmd.assert().success().stdout(predicate::str::contains(
        "PXL_20251007_101205558.jpg,2025-10-07T13:12:05.558+02:00,2025-10-07T11:12:05.558+00:00\n",
    ));
    wait_for_running_jobs(homedir.path());
    refreshcmd.assert().success();
    // TODO: bug in immich server v2.4.1: after timezone change w/o dateTimeOriginal change the asset localDateTime is not updated, only exif-datetime is updated
    // Expected:
    //  "PXL_20251007_101205558.jpg,2025-10-07T11:12:05.558+00:00,2025-10-07T11:12:05.558+00:00\n",
    listcmd.assert().success().stdout(predicate::str::contains(
        "PXL_20251007_101205558.jpg,2025-10-07T13:12:05.558+02:00,2025-10-07T11:12:05.558+00:00\n",
    ));

    // reset datetime of ASSET_UUID
    let mut cmd = new_cmd(homedir.path());
    cmd.arg("curl")
        .arg("--method")
        .arg("put")
        .arg("assets/".to_owned() + ASSET_UUID)
        .arg("--data")
        .arg("{\"dateTimeOriginal\":\"2025-10-07T12:12:05.558+02:00\"}");
    cmd.assert().success();
    wait_for_running_jobs(homedir.path());
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
fn test_curl() {
    let homedir = tempfile::tempdir().unwrap();
    login(homedir.path());

    // GET
    let mut cmd = new_cmd(homedir.path());
    cmd.arg("curl").arg("server/version");
    cmd.assert().success().stdout(
        predicate::str::is_match(r#"\{\s*"major":\s*\d+,\s*"minor":\s*\d+,\s*"patch":\s*\d+\s*\}"#)
            .unwrap(),
    );

    // 404
    let mut cmd = new_cmd(homedir.path());
    cmd.arg("curl").arg("unknown/endpoint").arg("-X").arg("GET");
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("404"));

    // with query parameters
    let mut cmd = new_cmd(homedir.path());
    cmd.arg("curl")
        .arg("albums?assertId=".to_owned() + ASSET_UUID);
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("assetCount"));

    // POST with json data
    let mut cmd = new_cmd(homedir.path());
    cmd.arg("curl")
        .arg("--method")
        .arg("post")
        .arg("search/metadata")
        .arg("--data")
        .arg("{\"id\":\"".to_owned() + ASSET_UUID + "\"}");
    cmd.assert().success().stdout(
        predicate::str::contains("assets").and(
            predicate::str::contains(ASSET_UUID).and(predicate::str::contains("\"total\": 1")),
        ),
    );

    // POST with form data
    let mut cmd = new_cmd(homedir.path());
    cmd.arg("curl")
        .arg("--method")
        .arg("post")
        .arg("search/metadata")
        .arg("--data")
        .arg("id=".to_owned() + ASSET_UUID);
    cmd.assert().success().stdout(
        predicate::str::contains("assets").and(
            predicate::str::contains(ASSET_UUID).and(predicate::str::contains("\"total\": 1")),
        ),
    );
}

// reset any changes made by tests
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

    // reset datetime of ASSET_UUID
    let mut cmd = new_cmd(homedir.path());
    cmd.arg("curl")
        .arg("--method")
        .arg("put")
        .arg("assets/".to_owned() + ASSET_UUID)
        .arg("--data")
        .arg("{\"dateTimeOriginal\":\"2025-10-07T12:12:05.558+02:00\"}");
    cmd.assert().success();
    wait_for_running_jobs(homedir.path());
}

---
name: update-immich-api
description: This skill should be used when the user asks to "update the immich API spec", "update immich-openapi-specs.json", "fetch the latest immich OpenAPI spec", "update the API", or "sync the immich spec".
disable-model-invocation: true
argument-hint: "[version]"
allowed-tools: [Bash, Read, Write, WebFetch, WebSearch]
---

# Update Immich API Spec

Fetch the latest released `immich-openapi-specs.json` from the immich GitHub repository, update the local copy, then build and test the project.

## Arguments

Optional version tag: $ARGUMENTS

## Instructions

### Step 1 — Determine the target version

If the user provided a version argument (e.g. `v1.130.0`), use that tag.

Otherwise, find the latest release tag by fetching the GitHub releases API:

```
GET https://api.github.com/repos/immich-app/immich/releases/latest
```

Extract the `tag_name` field from the JSON response. This is the version to use.

### Step 2 — Download the OpenAPI spec

Fetch the raw spec file for the determined version tag:

```
https://raw.githubusercontent.com/immich-app/immich/<tag_name>/open-api/immich-openapi-specs.json
```

### Step 3 — Update the local file

Write the downloaded content to `immich-openapi-specs.json` in the project root (`/Users/D047883/SAPDevelop/git/immichctl/immich-openapi-specs.json`).

Before overwriting, note the previous file size or `info.version` field so you can report the old vs new version to the user.

### Step 4 — Build the project

Run:

```bash
cd /Users/D047883/SAPDevelop/git/immichctl && cargo build 2>&1
```

Report any build errors to the user. If the build fails, restore the previous spec file content and inform the user.

### Step 5 — Run the tests

Run:

```bash
cd /Users/D047883/SAPDevelop/git/immichctl && cargo test 2>&1
```

Report test results (passed / failed / ignored counts).

### Step 6 — Report outcome

Summarise:
- Previous spec version → new spec version
- Build result (success or failure with errors)
- Test result (counts and any failures)

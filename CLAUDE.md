# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Rust Conventions

- For Rust coding conventions and best practices, see `.github/instructions/rust.instructions.md`.

## Project Overview

immichctl is a Rust command-line tool to manage [Immich](https://docs.immich.app) assets and implement missing UI functions. It handles timezone correction, tag management, album operations, and asset download — but not upload (covered by tools like immich-go).

**Spec**: See `README.md` for the full command specification.

## Architecture

```
src/
  main.rs            — CLI entry point; defines clap subcommand tree (Cli, Commands, AssetCommands, TagCommands, AlbumCommands)
  immichctl.rs       — Core ImmichCtl struct; orchestrates config, client, and asset store; delegates to subcommand modules
  timedelta.rs       — Custom parser for time offsets (e.g. "1d2h30m")
  immichctl/
    config.rs        — .immichctl/config.json: stores server URL + API key
    assets.rs        — .immichctl/assets.json: local asset selection store
    asset_cmd.rs     — Asset command implementations: search, list, count, clear, refresh, datetime adjust, download
    tag_cmd.rs       — Tag commands: assign, unassign, list
    album_cmd.rs     — Album commands: assign, unassign, list
    server_cmd.rs    — Server commands: version, login, logout
    curl_cmd.rs      — Raw API request proxy
    download_cmd.rs  — Download logic (uses POST /download/info + /download/archive)
build.rs             — Filters immich-openapi-specs.json to only allowed endpoints, generates Rust client via progenitor
```

**Key patterns**:
- `build.rs` filters the OpenAPI spec to a whitelist of endpoints (`/server/version`, `/auth/validateToken`, `/search/metadata`, `/assets/{id}`, `/tags`, `/tags/{id}`, `/tags/{id}/assets`, `/albums`, `/albums/{id}/assets`, `/download/info`, `/download/archive`), prunes unused components, then uses progenitor to generate a typed client. The generated code is `include!`d in `immichctl.rs`.
- `ImmichCtl` holds config, an eagerly-initialized `Result<Client>` (recreated on login), and the assets file path. Subcommand modules are called as methods on `ImmichCtl`.
- Asset selection is persisted locally in `~/.immichctl/assets.json` — commands work on this selection rather than the server.

## Commands

### Development
```bash
# Build
cargo build

# Run (with verbose logging)
RUST_LOG=debug cargo run -- <args>

# Run a single test
cargo test <test_name>

# Run all tests
cargo test

# Lint
cargo clippy

# Formatting
cargo fmt
```

### Testing
- Unit tests live alongside source code in `#[cfg(test)]` modules.
- Integration tests are in `tests/cli.rs`.
- Tests use `mockito` for HTTP mocking and `assert_cmd` + `predicates` for CLI testing.
- Use `create_immichctl_with_server()` helper in `immichctl.rs` tests to spin up a mock server.

## Immich API Client Generation

The API client is generated at compile time via progenitor from `immich-openapi-specs.json`. The `build.rs` script:
1. Parses the OpenAPI spec
2. Retains only whitelisted endpoints and methods
3. Recursively prunes unreferenced schemas/components
4. Generates typed Rust client code, formatted with prettyplease

To add a new endpoint, add it to the `allowed` HashMap in `build.rs`, then rebuild (`cargo build`).

## Configuration

- Login info: `$HOME/.immichctl/config.json` (server URL + API key)
- Asset selection: `$HOME/.immichctl/assets.json`

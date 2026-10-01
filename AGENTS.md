# AGENTS.md

Guidance for AI coding agents (pi and anything else that reads `AGENTS.md`) working in this repository.

## Project Overview

immichctl is a Rust command-line tool to manage [Immich](https://docs.immich.app) assets and implement missing UI functions. It handles timezone correction, tag management, album operations, and asset download — but not upload (covered by tools like immich-go).

**Spec**: See `README.md` for the full command specification.

## Commands

```bash
cargo build                      # build (regenerates the API client via build.rs)
RUST_LOG=debug cargo run -- <args>   # run with verbose logging
cargo test                       # all tests
cargo test <test_name>           # single test
cargo clippy --all-targets       # lint
cargo fmt                        # format
cargo fmt -- --check             # format check (used in CI)
```

Before finishing any change, the CI gate must pass: `cargo fmt -- --check`, `cargo clippy --all-targets`, `cargo build`, `cargo test`.

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
immich-openapi-specs.json  — Vendored Immich OpenAPI 3.0 spec at repo root (version in its info.version field)
build.rs             — Filters immich-openapi-specs.json to allowed endpoints, generates Rust client via progenitor
```

**Key patterns**:

- `build.rs` filters the OpenAPI spec to a whitelist of endpoints (`/server/version`, `/auth/validateToken`, `/search/metadata`, `/assets/{id}`, `/tags`, `/tags/{id}`, `/tags/{id}/assets`, `/albums`, `/albums/{id}/assets`, `/download/info`, `/download/archive`), prunes unused components, then uses progenitor to generate a typed client. The generated code is `include!`d in `immichctl.rs`.
- `ImmichCtl` holds config, an eagerly-initialized `Result<Client>` (recreated on login), and the assets file path. Subcommand modules are called as methods on `ImmichCtl`.
- Asset selection is persisted locally in `~/.immichctl/assets.json` — commands work on this selection rather than the server.

## Immich API Client Generation

The API client is generated at compile time via progenitor from `immich-openapi-specs.json`. `build.rs`:

1. Parses the OpenAPI spec
2. Retains only whitelisted endpoints and methods
3. Recursively prunes unreferenced schemas/components
4. Generates typed Rust client code, formatted with prettyplease

To add a new endpoint, add it to the `allowed` HashMap in `build.rs`, then rebuild.

To update the vendored spec to the latest immich release, run the `/skill:update-immich-api` project skill (`.pi/skills/update-immich-api/SKILL.md`). It fetches the spec for the latest release tag (or a tag you pass as an argument), then builds, tests, formats, and lints.

## Configuration

- Login info: `$HOME/.immichctl/config.json` (server URL + API key)
- Asset selection: `$HOME/.immichctl/assets.json`

Never commit credentials. `.env` is gitignored; keep server URLs and API keys out of tracked files.

## Testing

- Unit tests live alongside the code they test in `#[cfg(test)] mod tests`.
- Integration tests are in `tests/cli.rs`.
- Tests use `mockito` for HTTP mocking and `assert_cmd` + `predicates` for CLI testing.
- Use the `create_immichctl_with_server()` helper in `immichctl.rs` tests to spin up a mock server.

## Rust Conventions

Follow idiomatic Rust (Rust Book, Rust API Guidelines, RFC 430 naming). The rules that matter most here:

- **Errors**: use `Result<T, E>` and `?` for propagation. No `unwrap()` / `expect()` in non-test code; return meaningful errors with context (`anyhow` in `main`/CLI paths).
- **Borrowing**: take `&str` / `&T` parameters unless ownership is needed. Do not `.clone()` a `Copy` type (e.g. `Uuid`) — clippy's `clone_on_copy` is enabled and CI enforces it. Avoid unnecessary allocations and premature `.collect()`.
- **Iterators**: prefer iterators and combinators over index-based loops and deeply nested control flow.
- **Types over flags**: prefer enums and newtypes over `bool` parameters and loose primitives.
- **Docs**: rustdoc `///` on public items, describing behaviour, error conditions, and safety. Comments explain *why*, not what the line already says.
- **Style**: `rustfmt` output only, no warnings from `cargo clippy --all-targets`; keep `main.rs` thin and logic in modules.

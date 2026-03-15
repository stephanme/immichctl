# Documentation
- find documentation in `README.md`
- use the documentation as specification

# Testing
- write unit tests for new or changed code
- integration tests are implemented in `tests/cli.rs`
- run `cargo test` for executing tests

# Immich API spec
- `build.rs` contains a list of immich endpoints that are already used by immichctl. Prefer those endpoints over adding new ones.
- the Immich API spec is located in `immich-openapi-specs.json`
- the API spec is translated into a Rust client using progenitor, see `build.rs`

# Rust Coding Conventions and Best Practices
- see @.github/instructions/rust.instructions.md
# AGENTS.md

Guidance for coding agents working on ferrosheet. Human contributors: see [CONTRIBUTING.md](CONTRIBUTING.md), which this file follows.

## Project

Fast spreadsheet viewer and light editor in Rust. Two crates:

- `crates/ferrosheet-core`: reading, writing, formatting, selection and search. No UI dependencies.
- `crates/ferrosheet`: the egui desktop app.

## Checks

Run before every push; CI runs the same on Windows and Linux:

```bash
cargo fmt --all --check
cargo clippy --profile ci --workspace --all-targets -- -D warnings
cargo test --profile ci --workspace
```

On Linux the build needs the packages listed in `.github/workflows/ci.yml`.

## Rules

- Keep UI code out of `ferrosheet-core`.
- The reader must never panic on a broken file. Changes to reading code come with a test file that shows the problem; new fixtures can join the list in `crates/ferrosheet-core/tests/robustness.rs`.
- Bug fixes come with a test that fails without the fix.
- Test fixtures and screenshots must not contain personal or confidential data.
- Commits use a GitHub noreply address. No email addresses, local paths or machine names in code, commits or pull requests.

## Release

Bump `version` in the root `Cargo.toml` in its own pull request. Once it is merged to `main`, `.github/workflows/release.yml` creates the tag `v<version>`, builds the signed assets and publishes the GitHub release. Do not push release tags by hand.

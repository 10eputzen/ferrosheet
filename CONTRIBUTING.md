# Contributing

Thanks for your interest. Bug reports, sample files that open incorrectly and small fixes are very welcome. For larger features, please open an issue first so we can agree on the approach before you spend time on it; the [roadmap](docs/roadmap.md) shows what is planned.

## Setup

Install Rust (the toolchain is pinned in `rust-toolchain.toml`) and run the app from source:

```bash
cargo run -p ferrosheet -- path/to/file.xlsx
```

On Linux you need the build dependencies listed in [`.github/workflows/ci.yml`](.github/workflows/ci.yml).

## Before you open a pull request

```bash
cargo fmt --all --check
cargo clippy --profile ci --workspace --all-targets -- -D warnings
cargo test --profile ci --workspace
```

CI runs the same checks on Windows and Linux for every pull request.

## Guidelines

- `ferrosheet-core` stays free of UI dependencies; reading, formatting, selection and search belong there, egui code in `ferrosheet`.
- The reader must never panic on a broken file. If you touch reading code, add a test with a file that shows the problem; new fixtures can join the list in `crates/ferrosheet-core/tests/robustness.rs`, which truncates and corrupts them.
- Bug fixes come with a test that fails without the fix.
- Test fixtures must not contain personal or confidential data.

By contributing you agree that your contribution is dual licensed under the [Apache License 2.0](LICENSE-APACHE) and the [MIT license](LICENSE-MIT), without any additional terms, and that you follow the [Code of Conduct](CODE_OF_CONDUCT.md).

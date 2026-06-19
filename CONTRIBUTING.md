# Contributing to dagalog

Thank you for your interest in contributing! This guide covers how to build, test,
and submit changes.

---

## Prerequisites

- **Rust 1.85 or later** — install via [rustup.rs](https://rustup.rs/)
- **cargo-audit** — `cargo install cargo-audit` (used in CI)

---

## Building

```sh
cargo build
```

To build the release binary:

```sh
cargo build --release
# Binary is at target/release/dagalog
```

---

## Running tests

Run the full test suite:

```sh
cargo test --workspace
```

Run tests for a single crate:

```sh
cargo test -p dag-rdf
cargo test -p sparql-parser
cargo test -p dagalog
```

Run a single test by name:

```sh
cargo test test_add_and_get_resource
```

Some tests are `#[ignore]`'d by default because they require downloading large files.
Run them explicitly if needed:

```sh
cargo test --workspace -- --ignored
```

---

## CI quality checks

These mirror the checks run in CI (`.github/workflows/ci.yml`). Run them before
opening a PR:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test --workspace --release
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --document-private-items
cargo audit
```

Fix formatting automatically with:

```sh
cargo fmt --all
```

---

## Development workflow

This project follows **test-driven development**:

1. Open an issue or discuss the change first for anything non-trivial.
2. Write tests that fail (red phase). New tests are marked `#[ignore]` initially
   so the suite stays green while they are reviewed.
3. Implement, working through tests one at a time — easiest first.
4. Remove `#[ignore]` from each test as it passes.
5. Check for code smells before moving to the next test.

Do not open a PR that skips straight to implementation without tests. The tests are
the specification; they are reviewed before the implementation starts.

---

## Code style

- Rust edition 2024.
- `cargo fmt` defaults — no manual overrides in `rustfmt.toml`.
- Clippy with `-D warnings` — all warnings are errors in CI.
- No `unsafe` without a documented justification.
- Comments explain *why*, not *what*. One short line max; no multi-paragraph docstrings.

---

## Adding a dependency

Before adding a crate to `Cargo.toml`:

1. Check `cargo audit` to make sure the crate has no known advisories.
2. Check the crate's maintenance status (last release, open issues).
3. Prefer crates already used in the workspace (e.g., `serde`, `tokio`, `nom`).

When adding or removing dependencies, run the minimal-versions check:

```sh
cargo +nightly update -Z minimal-versions && cargo check --workspace --all-targets
```

This requires nightly Rust and mutates `Cargo.lock`; reset it afterwards with
`cargo update`.

---

## Project structure

See [CLAUDE.md](CLAUDE.md) for a full per-crate description of the codebase, and
[docs/dev/](docs/dev/) for architecture decision records and feature plans.

---

## Opening a pull request

- Target the `main` branch.
- Include a clear description of what the change does and why.
- Link to any related issues.
- Make sure all CI checks pass.
- Keep PRs focused — one logical change per PR.

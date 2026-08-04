# Plan: ignore RUSTSEC-2026-0235 in cargo-audit ([#375](https://github.com/daghovland/rdf-datalog/issues/375))

## Problem

CI's Security Audit job (`rustsec/audit-check@v2`, reading `.cargo/audit.toml`)
started failing on every branch once [RUSTSEC-2026-0235](https://rustsec.org/advisories/RUSTSEC-2026-0235.html)
(an insufficient-archive-validation bug in `rkyv` 0.7, out-of-bounds reads on
untrusted archives) was published to the advisory database. `rkyv 0.7.46` is
pinned in `Cargo.lock` as an optional dependency of `rust_decimal` (itself a
transitive dependency via `ingress`).

Confirmed while reviewing PRs #374/#376 (both failed only on this check):
`cargo tree -p rust_decimal -e features` shows no `rkyv` edge, and
`cargo tree -i rkyv` resolves to an empty graph for this workspace's target —
`rkyv` is present in `Cargo.lock` (which lists a package's full potential
dependency set across all features/targets) but never actually compiled into
any workspace binary. `cargo-audit` scans `Cargo.lock` directly rather than
the actually-activated feature graph, so it flags the lockfile entry
regardless.

## Why not upgrade instead

There's nothing to upgrade *to* from this workspace's side: no crate here
depends on `rkyv` directly, and `rust_decimal`'s `rkyv` feature isn't
activated by anything in the dependency graph. An upstream bump of
`rust_decimal` wouldn't change whether its optional `rkyv` feature is
enabled here either way. The advisory's own patched-version range
(`>=0.8.17`) is a statement about the crate `rkyv` itself, not something
this workspace's `Cargo.toml` files can act on directly.

## Fix

Add `RUSTSEC-2026-0235` to `.cargo/audit.toml`'s `ignore` list, mirroring the
existing `RUSTSEC-2023-0071` entry's pattern (short code comment explaining
why the advisory doesn't apply here, plus a link back to the tracking issue).
No code changes elsewhere.

## Out of scope / re-check trigger

If any future dependency change activates `rust_decimal`'s `rkyv` feature
(visible as a new edge in `cargo tree -p rust_decimal -e features`), this
ignore entry needs to be revisited — it's conditioned on the feature staying
inactive, not on the advisory itself being wrong.

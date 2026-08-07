# Plan: add an HTTP readiness endpoint (`GET /$/ready`)

Issue: [#414](https://github.com/daghovland/rdf-datalog/issues/414)

## Problem

Downstream consumers (the `records` library, via Testcontainers) can only
wait for the internal TCP port to be listening
(`UntilInternalTcpPortIsAvailable(3030)`), which doesn't guarantee the
server has actually finished initializing (router, dataset registry,
persistence/reasoning setup, admin API) and is answering HTTP requests.

## Architecture finding (read before implementing — this determines the fix's shape)

`sparql_endpoint::serve_on_listener` (`sparql_endpoint/src/lib.rs`) binds
the TCP listener *first* (`tokio::net::TcpListener::bind`), then
synchronously does ALL initialization — changelog open + replay,
`IncrementalReasoner::new` (full initial materialisation), dataset registry
construction — and only builds the `axum` router and calls `axum::serve`
*after* all of that completes. So there genuinely is a window where the OS
TCP port is open (a bare `UntilInternalTcpPortIsAvailable` check would pass)
but `axum::serve` hasn't started accepting/routing HTTP requests yet,
especially with a large changelog to replay or a slow initial rules
materialisation.

Crucially: once `axum::serve` *is* running, **every** route is already
fully live — there is no further async/background initialization after
that point in this codebase's current architecture. This means the
existing `GET /$/ping` handler (`sparql_endpoint::admin::admin_ping`,
already wired at `/$/ping` in `server.rs`, currently documented as a pure
"liveness check") is *already* a fully correct readiness signal by
construction: it can only ever respond 200 once the router (and therefore
every other route) is live. There is no additional "warming up" state to
track.

## Fix

Add `GET /$/ready` as an additional route, using the same or a
near-identical handler to `admin_ping` (reuse `admin_ping` directly unless
there's a good reason for a distinct handler/response body — the issue
suggests a small JSON or plain-text body either way). Document *why* it's
equivalent to `/$/ping` in this architecture (a short doc comment on the
handler or route registration, referencing this issue/PR) so a future
reader doesn't wonder why there are two routes doing the same thing.

Update `docs/user/deployment.md`'s admin API table to add the new
`GET /$/ready` row, and clarify in prose (near the table, or as a note on
both rows) that `/$/ping` and `/$/ready` are both true liveness+readiness
checks in this deployment (no separate "started but not ready" state
exists) — so downstream Testcontainers/Kubernetes-style tooling can use
either the liveness or the readiness route interchangeably.

## Tests (TDD)

- Integration test (`sparql_endpoint/tests/` — check for existing tests of
  `/$/ping` first and mirror the pattern) confirming `GET /$/ready` returns
  200 with a small body, via the real HTTP test-server setup used
  elsewhere in this crate's tests.
- No behavior-under-load test is meaningful here (per the architecture
  finding above, there's no partial-init state to simulate) — don't invent
  one.

## Out of scope

#413 (GHCR release-tag atomicity) and #415 (Fuseki assembler Turtle
payload documentation) are separate issues from the same reporter — not
part of this PR.

# SSRF guard gaps — plan (issue #365)

Related: [#365](https://github.com/daghovland/rdf-datalog/issues/365) (this issue),
[#135](https://github.com/daghovland/rdf-datalog/issues/135) (original SSRF hardening epic).

`sparql_endpoint/src/sparql_update.rs` gates SPARQL `LOAD` fetches with
`ssrf_preflight`/`is_blocked_ip`, active whenever an operator opts into
`NetworkPolicy::Allow`/`AllowList` (default remains `Deny`). Three gaps remain:

1. IPv4 loopback (127.0.0.0/8) is unconditionally **not** blocked, with a comment
   saying this is for wiremock tests — a test convenience baked into production code.
2. The IPv6 blocklist only checks `is_loopback()`. Missing: unique-local (`fc00::/7`),
   link-local (`fe80::/10`), and IPv4-mapped addresses (`::ffff:169.254.169.254`).
3. TOCTOU: `ssrf_preflight` resolves the hostname once; the actual
   `reqwest::blocking::Client::get(url).send()` does its own independent DNS
   resolution at connect time. A rebinding attacker can serve a safe IP to the
   preflight and a private IP to the real connect.

## Fix shape

- `is_blocked_ip(ip, allow_loopback: bool) -> bool`: keep all existing IPv4 checks;
  gate IPv4 loopback behind `allow_loopback` (was unconditionally allowed). Expand
  IPv6: `is_loopback()`, `is_unspecified()`, `is_unicast_link_local()` (stable, covers
  `fe80::/10`), `is_unique_local()` (stable, covers `fc00::/7`), and unmap
  `to_ipv4()` (covers both `::ffff:a.b.c.d` and `::a.b.c.d`) and recurse into the
  IPv4 rules with the same `allow_loopback` — **the v6-native checks must run
  before the unmap+recurse**, otherwise `::1`'s IPv4-mapped form (`0.0.0.1`) would
  slip past the IPv4 loopback check when `allow_loopback` is true for a real test
  server but `::1` itself should still be blocked as an IPv6 address in that same
  configuration (loopback exception only applies to the literal families explicitly
  allowed for wiremock, i.e. `127.0.0.1`/`::1`, decided per address family).
- `ssrf_preflight(url, allow_loopback) -> Result<Vec<SocketAddr>, String>`: switch
  from manual `format!("{host}:{port}").to_socket_addrs()` to
  `parsed.socket_addrs(|| parsed.port_or_known_default())` (handles IPv6 bracket
  literals correctly — the manual format string cannot). Returns the *validated*
  resolved addresses instead of `()`, so the caller can pin them.
- `fetch_rdf`: after preflight, build the `reqwest::blocking::ClientBuilder` with
  `.resolve_to_addrs(host, &validated_addrs)`. This pins the connection to exactly
  the addresses validated by preflight — reqwest/hyper will not perform a second,
  independent DNS resolution for that host, closing the TOCTOU window structurally
  (not by re-checking a second resolution result).
- Threading `allow_loopback`: add `allow_loopback: bool` as a new parameter on
  `apply_prepared_update` (default `false` at every existing call site) and a new
  `Config`/`AppState` field `allow_loopback_for_ssrf_tests: bool` (doc'd as a
  test-only escape hatch, default `false`, **no CLI flag** — set only by the
  wiremock-based test harness in `sparql_endpoint/tests/common/mod.rs`).

## Tests (red phase, `#[ignore]`)

Unit tests in `sparql_endpoint/src/sparql_update.rs` (`#[cfg(test)] mod tests`),
since the fix is entirely reachable from private functions in-crate:

- `test_is_blocked_loopback_v4_by_default` — `is_blocked_ip(127.0.0.1, false)` → blocked.
- `test_is_not_blocked_loopback_v4_when_allowed` — `is_blocked_ip(127.0.0.1, true)` → not blocked (renamed/adapted from the existing `test_is_not_blocked_loopback_v4`).
- `test_is_blocked_ipv6_unique_local` — `fc00::1` → blocked.
- `test_is_blocked_ipv6_link_local` — `fe80::1` → blocked.
- `test_is_blocked_ipv6_mapped_metadata` — `::ffff:169.254.169.254` → blocked.
- `test_is_blocked_ipv6_mapped_loopback_respects_allow_loopback` — `::ffff:127.0.0.1` blocked when `allow_loopback=false`, not blocked when `true`.
- `test_ssrf_preflight_blocks_loopback_v4_by_default` — URL-level, `allow_loopback=false`.
- `test_ssrf_preflight_handles_ipv6_literal_url` — `http://[fd00::1]/` must be rejected with the "blocked" message, not a DNS-resolution error (covers the `socket_addrs()` fix directly, since the old `format!` + `to_socket_addrs` path mishandles bracketed IPv6 literals).
- `test_fetch_rdf_pins_resolved_address_closes_toctou` — spins up a real wiremock
  server on loopback (`allow_loopback=true`), calls a resolver-injectable variant
  of `fetch_rdf` with a stub resolver that asserts it is invoked exactly once,
  and confirms the fetch still succeeds (proving the actual connect used the
  address pinned from that single resolution, not a second one).

Integration test in `sparql_endpoint/tests/ssrf_protection.rs`:

- `test_load_blocks_loopback_by_default` — full HTTP stack, `NetworkPolicy::Allow`
  but *without* the test-only loopback bypass (new `common::TestServer` helper
  that leaves `allow_loopback_for_ssrf_tests` at its default `false`), LOAD from
  a real wiremock server on `127.0.0.1` must return 500. This is the one test that
  exercises the full `Config` → `AppState` → `apply_prepared_update` threading,
  since the unit tests above only exercise `sparql_update.rs`'s private functions
  directly.

## Regression audit

`sparql_endpoint/tests/{ssrf_protection,load_network,allowlist}.rs` all call
`common::TestServer::start_writable_with_network_policy`, and every one of those
tests relies on successfully reaching a wiremock server bound to `127.0.0.1`
under `NetworkPolicy::Allow`/`AllowList` — i.e. they are testing something *other*
than the loopback block (cross-host redirects, body-size caps, allow-list prefix
matching, etc.), so `start_writable_with_network_policy` sets
`allow_loopback_for_ssrf_tests: true` unconditionally. Only the new
`test_load_blocks_loopback_by_default` test needs the strict (bypass-off) path,
via a new `start_writable_with_network_policy_strict` helper.

`test_allowlist_still_blocks_private_ip` (RFC 1918, not loopback) and
`test_load_blocks_rfc1918_ip`/`test_load_blocks_link_local_ip` (non-loopback
ranges) are unaffected by the loopback bypass either way and keep passing for
their original reason.

## Out of scope

`jsonld_parser/src/lib.rs`'s external `@context` fetch mirrors some of this SSRF
logic but has its own gate; not touched here — a follow-up issue will be filed
(unlabeled, per the `ready`-label workflow) if the same gaps apply there.

# LLM Chat Architecture Plan (scoping)

Scoping doc for [#185](https://github.com/daghovland/rdf-datalog/issues/185), answering the
open questions it raises. Part of epic [#184](https://github.com/daghovland/rdf-datalog/issues/184)
(LLM-powered chat window) and the general frontend epic
[#41](https://github.com/daghovland/rdf-datalog/issues/41). No implementation here — see
"Follow-up issues" below for what this doc unblocks.

## 1. Where does the API key live, and who calls the provider?

**Recommendation: Option A — the browser calls the provider directly** with a user-supplied key,
using Anthropic's `anthropic-dangerous-direct-browser-access` header. The server is not involved
in the LLM call at all.

Grounds for this, specific to what's already in the repo rather than a general preference:

- `sparql_endpoint/src/frontend.html` and `deploy/` currently set **no `Content-Security-Policy`**
  (verified: no `Content-Security-Policy` or `connect-src` directive anywhere in either). A
  same-origin page with no CSP can `fetch()` any cross-origin host from client JS today, so
  Option A needs zero new server configuration to work — not even a CSP allowlist entry. If a CSP
  is added later (worth doing regardless, independent of this feature), it will need a
  `connect-src` entry for the chosen provider's API host; that's a one-line addition, not a
  redesign.
- Option B (server proxies the call) would mean adding an async outbound HTTP client to
  `sparql_endpoint`, which is exactly the surface the `NetworkPolicy` work
  ([#120](https://github.com/daghovland/rdf-datalog/issues/120),
  [#137](https://github.com/daghovland/rdf-datalog/issues/137),
  [#138](https://github.com/daghovland/rdf-datalog/issues/138)) built specific SSRF hardening for
  (private-IP blocking, cross-host redirect blocking, body-size caps, prefix allowlisting) — and
  that machinery exists precisely because *any* server-initiated outbound fetch in this codebase
  is treated as a real attack surface, not a formality. Reusing it for "call one specific, fixed
  LLM endpoint" would be overkill; building a second, narrower outbound path just for the chat
  feature would duplicate that hardening. Either way it's substantially more code than Option A
  for no corresponding benefit — the provider's own API key auth is the access control regardless
  of who calls it.
- The key still sits in browser memory / `localStorage`, with the XSS exposure that implies. This
  is accepted as a known, documented trade-off (see "Key storage" below), not hidden.

**Two credentials in the browser.** When `AuthConfig::ApiKey` is active
([`docs/plans/AUTH.md`](https://github.com/daghovland/rdf-datalog/blob/main/docs/plans/AUTH.md)
Tier 1), the chat panel holds *two* secrets client-side at once: the dagalog bearer token (needed
for the panel's own `/sparql` calls once the user reviews and runs a drafted query) and the LLM
provider key (needed for the chat calls). They have different blast radii — the dagalog token
scopes to this one dataset, the provider key scopes to the user's entire LLM account/billing — and
must not be conflated or accidentally logged/sent together to either party. The implementation
issue for the chat panel should keep them in separate storage keys with separate input fields, and
the one-time privacy notice (see §5) should mention both origins the page will talk to.

**Key storage.** `localStorage`, scoped to the origin the panel is served from, with an explicit
"forget key" control. No attempt at more sophisticated in-browser secret storage — anything
JS-accessible is equally exposed to the same XSS threat model, so added complexity there buys
nothing.

## 2. What can the agent actually do?

**Recommendation: draft-only for the first implementation.** The agent proposes SPARQL/Turtle
text; the user reviews it and clicks "run," reusing the existing query/upload UI as the execution
path. No new execution or safety machinery is needed beyond what the UI already has.

This is also what makes Option A coherent, not just simpler: **every query or update the panel
ever issues goes through the existing `/sparql` HTTP path**, so it is already governed by
`auth_middleware` → `classify()` → `--read-only` → the `owl:Nothing` 409-rollback check
([docs/plans/AUTH.md](https://github.com/daghovland/rdf-datalog/blob/main/docs/plans/AUTH.md)),
with zero new server-side permission code required. In particular,
[#164](https://github.com/daghovland/rdf-datalog/issues/164) fixed `classify()` to inspect POST
bodies (`Content-Type: application/sparql-update`, or a form `update=` field) rather than trusting
the path alone — so an LLM-drafted "query" that's actually a mutation still gets classified as
`Write` and challenged/blocked correctly, whether the user pastes it manually or the panel submits
it via "run this." Draft-only doesn't need to reimplement any of that; it inherits it for free by
routing through the same endpoint the manual query box already uses.

Tool-calling (the agent issuing queries directly, in a loop, without a manual click per action) is
deferred to a distinct follow-up issue, to be scoped only once draft-only's prompt/context design
is validated in practice.

## 3. Safety rails, if/when a tool-calling loop is added

Answered now so the eventual follow-up issue has a concrete target rather than reopening the
question from scratch:

- **`--read-only` must be respected automatically, with no separate flag.** Because a tool-calling
  loop would still submit through `/sparql`, this falls out of the existing server behavior
  (`docs/plans/AUTH.md`: "When `--read-only` is active the server enforces `Read`-only regardless
  of auth credentials") — the loop's mutating calls simply get rejected server-side. No new
  client-side "is this server read-only" check needs to be correct for safety to hold, though the
  UI should still surface the rejection legibly rather than let the agent silently retry.
- **Every mutating action needs an explicit per-action confirmation click**, even inside a loop.
  Auto-approving a sequence of writes is exactly the scope draft-only is deferring; when
  tool-calling lands it should still default to "propose, user clicks Run" per action, with
  "auto-run reads" as a plausible relaxation (reads are already gated by `--read-only`/auth same as
  writes, but are non-destructive) rather than "auto-run everything."
- **A per-session query-count/token budget** is worth adding at that point, mirroring the existing
  `--query-timeout` CLI-configurable-limit pattern already used elsewhere in `src/main.rs` (e.g.
  `--max-rdf-upload-bytes`, `--max-rml-upload-bytes` — see PR
  [#275](https://github.com/daghovland/rdf-datalog/pull/275),
  [#277](https://github.com/daghovland/rdf-datalog/pull/277)) — a client-side counter for a
  browser-direct architecture (there's no server-side session to attach a budget to under Option
  A), reset per chat session, surfaced in the panel.

## 4. Provider abstraction

**Recommendation: hardcode to Claude (Anthropic Messages API) for the first cut.** A second
provider is straightforward to generalize to once the first one's concrete request/response shape
and streaming behavior are known; a pluggable interface designed before that is a guess at an
abstraction boundary that will likely be wrong in some detail (streaming events, tool-call JSON
shape, error format all differ meaningfully between providers). This mirrors how this repo's other
integrations were built incrementally rather than abstracted up front (e.g. `NetworkPolicy` started
as `Deny | Ignore | Allow` and only grew `AllowList` once a concrete need for it existed).

## 5. Context sent to the provider

**Recommendation: schema/shape summary, not raw triples, and not a full introspection tool-loop
in the first cut.** Concretely, reuse what the server already exposes rather than inventing a new
summarization path:

- **SPARQL 1.1 Service Description** and **VoID** endpoints
  ([docs/architecture/PROTOCOLS.md](https://github.com/daghovland/rdf-datalog/blob/main/docs/architecture/PROTOCOLS.md)
  §3–4, both already implemented) already provide `void:triples`, `void:distinctSubjects`,
  `void:distinctPredicates`, and `void:vocabulary` — exactly the shape of summary a chat agent
  needs to write plausible SPARQL without seeing actual data.
- A predicate/class-count `SELECT` (e.g. `SELECT ?p (COUNT(*) AS ?n) WHERE { ?s ?p ?o } GROUP BY ?p`)
  run through the existing query path, if a finer-grained predicate list is wanted than VoID
  provides.
- Nothing is sent until the user asks a question that references the dataset — no context dump on
  chat-open.
- Full SPARQL-introspection "tools" the agent calls on demand (list predicates, sample triples
  matching a pattern) is a natural extension once tool-calling (§3) exists, but is out of scope for
  draft-only: draft-only has no loop to call tools from, so it would just be a fixed context blob
  either way, and VoID/Service Description already provide that blob.

**Privacy notice.** This is a privacy decision, not just a UX one: opting into chat sends whatever
context is included (schema summary at minimum, more if the user pastes data into the chat) to a
third-party API, along with the user's own key. Show a one-time notice before the first chat
message is sent, not a silent default — following the same "explicit, persisted, per-viewer"
pattern already used for saved graph-view state
([PR #255](https://github.com/daghovland/rdf-datalog/pull/255), which persists node layout to
`localStorage` keyed per-query): a `localStorage` flag ("don't show again") gates the notice, no
server-side tracking of whether it was shown.

## Follow-up implementation issues

Filed as sub-issues of [#184](https://github.com/daghovland/rdf-datalog/issues/184), each at
Status `Todo` (awaiting the user's review before any agent may pick them up):

1. **Chat UI panel** — collapsible panel in `frontend.html`, provider-key input/storage (separate
   from the dagalog bearer token field), one-time privacy notice, message history (session-only,
   no persistence requirement).
2. **Claude Messages API integration, draft-only** — browser-direct `fetch()` to Anthropic's
   Messages API using `anthropic-dangerous-direct-browser-access`; agent drafts SPARQL/Turtle into
   the existing query/upload boxes for manual review and run; context = VoID/Service Description
   summary, sent only once the user asks a dataset-referencing question.
3. **Tool-calling loop (stretch)** — agent can call a constrained tool set (run SELECT, propose
   INSERT/DELETE with per-action confirmation) in a loop; respects `--read-only` and per-session
   budget per §3 above. Explicitly deferred until draft-only (#2) has shipped and its prompt/context
   design is validated with real usage.

## References

- Scoping issue: [#185](https://github.com/daghovland/rdf-datalog/issues/185)
- Epic: [#184](https://github.com/daghovland/rdf-datalog/issues/184)
- Frontend epic: [#41](https://github.com/daghovland/rdf-datalog/issues/41)
- Frontend plan: [docs/plans/FRONTEND_PLAN.md](https://github.com/daghovland/rdf-datalog/blob/main/docs/plans/FRONTEND_PLAN.md)
- Auth plan: [docs/plans/AUTH.md](https://github.com/daghovland/rdf-datalog/blob/main/docs/plans/AUTH.md)
- Protocol compliance reference (Service Description / VoID):
  [docs/architecture/PROTOCOLS.md](https://github.com/daghovland/rdf-datalog/blob/main/docs/architecture/PROTOCOLS.md)
- Body-based request classification: [#164](https://github.com/daghovland/rdf-datalog/issues/164)
- Outbound network policy / SSRF hardening precedent (why Option B is expensive):
  [#120](https://github.com/daghovland/rdf-datalog/issues/120),
  [#137](https://github.com/daghovland/rdf-datalog/issues/137),
  [#138](https://github.com/daghovland/rdf-datalog/issues/138)
- Graph-view localStorage persistence precedent: [PR #255](https://github.com/daghovland/rdf-datalog/pull/255)

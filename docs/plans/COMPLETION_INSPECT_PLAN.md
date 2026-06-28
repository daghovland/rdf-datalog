# `complete_request`/`inspect_request` for `dagalog-kernel`

> Tracked under [#28 Jupyter kernel epic](https://github.com/daghovland/rdf-datalog/issues/28).

## Problem

`dagalog-kernel`'s `handle_shell_message` (`src/sockets.rs`) has no match arms
for `complete_request` or `inspect_request`; both fall through to the
`other => eprintln!("unhandled shell message type")` branch and the Jupyter
client gets no reply. This means JupyterLab's Tab-completion and Shift-Tab
hover docs do nothing for SPARQL cells.

Tracked as [issue #23](https://github.com/daghovland/rdf-datalog/issues/23)
(completion) and [issue #24](https://github.com/daghovland/rdf-datalog/issues/24)
(inspect).

## Scope

Both features only need to understand plain SPARQL cell text (the default
cell type). `%%`-magic cells are out of scope — completion/inspect on a
`%%rml`/`%%datalog`/... cell falls back to "no matches" / "not found", same
as an unrecognized language would.

Keyword and builtin-function lists are taken directly from the full SPARQL 1.1 grammar, so
completion might suggest something the engine can't execute. This is intentional, to trigger:
implementation of these lacking features. The list below is the implemented keywords:

- Keywords: `SELECT`, `CONSTRUCT`, `ASK`, `DESCRIBE`, `WHERE`, `FROM`,
  `FILTER`, `OPTIONAL`, `UNION`, `MINUS`, `GRAPH`, `BIND`, `VALUES`, `GROUP`,
  `BY`, `HAVING`, `ORDER`, `ASC`, `DESC`, `LIMIT`, `OFFSET`, `DISTINCT`,
  `PREFIX`, `AS`, `EXISTS`, `NOT`, `SEPARATOR`, `UNDEF`, `TRUE`, `FALSE`.
- Builtin functions: `STR`, `LANG`, `LANGMATCHES`, `DATATYPE`, `BOUND`,
  `ISIRI`, `ISURI`, `ISBLANK`, `ISLITERAL`, `STRLEN`, `REGEX`.

Prefix completion only considers `PREFIX` declarations already present
earlier in the *same cell's* text (no cross-cell prefix memory — the kernel
session doesn't persist declared prefixes today, and adding that is out of
scope here).

## Design

New module `dagalog-kernel/src/completion.rs`, pure functions (no ZMQ/IO),
so they're unit-testable directly:

```rust
pub struct Completion {
    pub matches: Vec<String>,
    pub cursor_start: usize,
    pub cursor_end: usize,
}

/// Find the partial token ending at `cursor_pos` in `code` and return
/// matching keywords/builtin-functions/declared-prefixes.
pub fn complete(code: &str, cursor_pos: usize) -> Completion;

/// Find the identifier/function name at `cursor_pos` and return a short doc
/// string for it, if recognized.
pub fn inspect(code: &str, cursor_pos: usize) -> Option<String>;
```

`complete`: walks backward from `cursor_pos` over identifier characters
(`[A-Za-z_]`) to find the partial-word start, then matches case-insensitively
against (keywords ++ builtin functions ++ prefixes already declared in
`code`). Empty partial word (cursor right after whitespace/`{`/`(`) → no
matches, not "everything" (avoids a useless 40-item dump).

`inspect`: same partial-word extraction, but takes the *whole* word under/
before the cursor (not just up to cursor — inspect can be invoked with the
cursor in the middle of a word) and looks it up case-insensitively in a
builtin-function doc table. Returns `None` for keywords (no useful "doc" for
`SELECT`) and unrecognized words.

Wiring in `sockets.rs`:
- `"complete_request"` arm: read `code`/`cursor_pos` from `msg.content`, call
  `completion::complete`, reply with
  `{"status": "ok", "matches": [...], "cursor_start": ..., "cursor_end": ..., "metadata": {}}`.
- `"inspect_request"` arm: read `code`/`cursor_pos`, call `completion::inspect`,
  reply with
  `{"status": "ok", "found": bool, "data": {"text/plain": "..."} or {}, "metadata": {}}`.

No changes to `cell/mod.rs`, `CellType`, or `dispatch_cell` — these are shell
messages independent of cell execution.

## Test plan

### Unit tests — `dagalog-kernel/src/completion.rs`

1. `test_complete_keyword_prefix` — `"SEL"` at cursor 3 → matches includes
   `"SELECT"`, `cursor_start == 0`, `cursor_end == 3`.
2. `test_complete_function_prefix` — `"...FILTER(reg"` → matches includes
   `"REGEX"`.
3. `test_complete_declared_prefix` — `"PREFIX foaf: <...>\nfo"` → matches
   includes `"foaf"`.
4. `test_complete_no_partial_word_returns_empty` — cursor right after a space
   → empty matches.
5. `test_inspect_known_function_returns_doc` — cursor inside `"REGEX"` →
   `Some(doc)` mentioning "regular expression".
6. `test_inspect_unknown_word_returns_none` — cursor inside `"Alice"` →
   `None`.

### Integration tests — `dagalog-kernel/tests/notebook_e2e.rs`

7. `test_complete_request_over_zmq` — via `KernelHarness`: send a raw
   `complete_request` with `code = "SEL"`, `cursor_pos = 3`; assert the
   `complete_reply` contains `"SELECT"` in `matches`.
8. `test_inspect_request_over_zmq` — via `KernelHarness`: send a raw
   `inspect_request` with `code = "REGEX"`, `cursor_pos = 2`; assert
   `inspect_reply.found == true` and `data["text/plain"]` mentions "regular
   expression".

These two integration tests need a new `KernelHarness::request` helper (the
existing harness only exposes `execute`/`shutdown`) that sends an arbitrary
shell message and waits for the matching shell reply by `msg_id`.

All 8 tests will be written `#[ignore]`d with stub
`unimplemented!()` bodies (or, for the integration tests, relying on the
still-unhandled message types) to compile, per CLAUDE.md TDD phasing.
Implementation proceeds test-by-test after review, easiest first:
4 → 1 → 2 → 3 → 6 → 5 → 7 → 8.

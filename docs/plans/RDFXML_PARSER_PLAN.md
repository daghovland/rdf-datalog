# RDF/XML Parser Plan

Tracked in epic: [#240](https://github.com/daghovland/rdf-datalog/issues/240) — "Epic: RDF/XML parsing support".
This issue: [#750](https://github.com/daghovland/rdf-datalog/issues/750) — core `oxrdfxml`-based parser crate wiring.
Follow-up sub-issues (already filed by the epic, explicitly NOT this issue's scope):
[#751](https://github.com/daghovland/rdf-datalog/issues/751) (CLI + `sparql_endpoint` GSP/`LOAD` wiring),
[#752](https://github.com/daghovland/rdf-datalog/issues/752) (un-skip the W3C subquery-suite `sq01`–`sq10` RDF/XML fixtures).

This document plans adding RDF/XML (`.rdf`) parsing to the `turtle` crate,
reusing the `oxrdf`→`GraphElement` conversion helpers already written for
Turtle/TriG/N-Triples/N-Quads rather than writing a second term-model
adapter.

## Where it goes: `turtle` crate, not a new crate

The epic left this "TBD at implementation time". Decision: **`turtle`
crate**, not a new sibling crate. Reasons:

- `oxrdfxml` produces the exact same `oxrdf::{Triple, NamedOrBlankNode,
  Term, Literal}` types that `oxttl` already produces for Turtle/TriG/
  N-Triples/N-Quads. `turtle/src/lib.rs` already has `intern_subject`,
  `intern_named_node`, `intern_term`, and `convert_literal` written against
  exactly these types — RDF/XML needs zero new conversion code, only a new
  parser-driving function that calls the same helpers.
- A new crate would duplicate the `dag-rdf`/`oxrdf` dependency wiring and
  buy nothing: there's no RDF/XML-specific term-model concern that would
  justify its own crate (unlike, say, `manchester_parser` vs.
  `owl_functional_parser`, which produce a genuinely different target type,
  `owl_ontology::Ontology`, from parsers with materially different
  grammars). Here the target type (`Datastore` quads) and the source
  term model (`oxrdf`) are identical to what's already in `turtle`.
- `turtle` is already the "wrap an `oxttl`/`oxrdf`-family streaming parser
  and intern the results" crate; "turtle" is a slight misnomer for what it
  now does (RDF serialization-format parsing in general), but renaming the
  crate is a separate concern outside this issue's scope and not something
  to do unilaterally here.

## API shape

Mirrors `parse_turtle` / `parse_turtle_with_base` (added for base-IRI
handling in PR [#102](https://github.com/daghovland/rdf-datalog/issues/102)):

```rust
pub fn parse_rdfxml<R: Read>(datastore: &mut Datastore, reader: R) -> Result<(), RdfXmlParseError>;
pub fn parse_rdfxml_with_base<R: Read>(datastore: &mut Datastore, reader: R, base_iri: &str) -> Result<(), RdfXmlParseError>;
```

`oxrdfxml::RdfXmlParser` has its own error type (`RdfXmlParseError`,
distinct from `oxttl::TurtleParseError` — different crate, different
syntax-error variants), so these functions return `RdfXmlParseError`
rather than reusing Turtle's error type. `RdfXmlParser::new().for_reader
(reader)` yields `Result<oxrdf::Triple, RdfXmlParseError>` — the same
`Triple { subject: NamedOrBlankNode, predicate: NamedNode, object: Term }`
shape as `oxttl`'s parsers, so the existing `intern_subject` /
`intern_named_node` / `intern_term` helpers apply unchanged.

The `rdf-12` feature is enabled on `oxrdfxml` (matching `oxttl`'s existing
`rdf-12` feature in this crate — PR [#152](https://github.com/daghovland/rdf-datalog/issues/152) added `Term::Triple` handling
in `intern_term`). RDF/XML 1.1 itself has no syntax for RDF 1.2 triple
terms, so `oxrdfxml` is not actually expected to ever emit `Term::Triple`
from real RDF/XML input — enabling the feature just keeps the `oxrdf`
version/feature set aligned with `oxttl`'s so both parsers share one
`oxrdf` type (required anyway, since `intern_term` pattern-matches on
`Term::Triple` only when the feature is on).

Literal handling reuses `convert_literal` verbatim (relevant provenance:
PR [#340](https://github.com/daghovland/rdf-datalog/issues/340), literal round-tripping via `format_literal`/
`parse_literal_term` — not directly touched by this issue, since RDF/XML
has no analogous "wrap a single literal in a throwaway triple" need, but
confirms `convert_literal`'s three-variant `LiteralString`/`LangLiteral`/
`TypedLiteral` behavior is the established contract for all formats in
this crate, including the new one).

## RDF/XML edge cases this issue tests

Per the epic's 2026-07-22 survey:

1. **Striped syntax** — alternating `rdf:Description`/property-element
   layers, with `rdf:parseType="Resource"` to collapse striping without an
   intermediate blank-node element.
2. **`rdf:parseType="Collection"`** — produces an RDF list (`rdf:first`/
   `rdf:rest`/`rdf:nil`) from XML child elements with no explicit list
   syntax.
3. **`rdf:parseType="Literal"`** — produces an `rdf:XMLLiteral`-typed
   literal from the element's inner XML content. Note: `oxrdfxml`
   canonicalizes in-scope XML namespace declarations onto the literal's
   root element (so the lexical form is self-contained independent of its
   original document context) rather than copying the source markup
   byte-for-byte — tests assert against the canonicalized form.
4. **`rdf:ID` vs `rdf:about` vs `rdf:resource`** — `rdf:ID` mints a
   fragment IRI relative to the in-scope base and is also used for
   reification shorthand; `rdf:about` names a subject by (possibly
   relative) IRI; `rdf:resource` names an object by (possibly relative)
   IRI, as opposed to a nested element.
5. **`xml:lang` scoping** — language tags apply to a literal unless
   overridden/cleared by a nested `xml:lang`, and are inherited down the
   element tree per XML scoping rules.
6. **`xml:base` scoping** — relative IRIs resolve against the nearest
   enclosing `xml:base`, which can be set at any element, not just the
   document root; also tests `parse_rdfxml_with_base`'s caller-supplied
   base interacting with an in-document `xml:base`.
7. Plain `rdf:about`/property-element round trip (baseline smoke test,
   analogous to `parse_simple_turtle`).
8. Bare property attributes (`schema:name="Bar"` directly on the subject
   element, no child element) — common real-world shorthand.
9. Blank node subjects via `rdf:nodeID`.
10. Malformed input (unclosed tag) returns `Err`, not a panic — baseline
    error-path coverage mirroring the existing `*_rejects_*` tests for
    other formats in this file.

## TDD phases

1. This plan doc (done).
2. Add `oxrdfxml` dependency to `turtle/Cargo.toml` (`rdf-12` feature,
   matching `oxttl`). Add `#[ignore]`d tests for all edge cases above,
   stub `parse_rdfxml`/`parse_rdfxml_with_base` signatures so the crate
   compiles (bodies can `unimplemented!()` initially — tests are ignored,
   so this never runs).
3. Implement `parse_rdfxml`, un-ignore the baseline smoke test (#7) first,
   then work through: bare property attributes (#8), blank nodes (#9),
   `rdf:ID`/`rdf:about`/`rdf:resource` (#4), `xml:lang` (#5), striped
   syntax + `parseType="Resource"` (#1), `parseType="Collection"` (#2),
   `parseType="Literal"` (#3), malformed-input error path (#10).
4. Implement `parse_rdfxml_with_base`, un-ignore `xml:base` scoping (#6).
5. `cargo fmt`/`clippy`/`test --workspace`, open PR.

## Out of scope (this issue)

- CLI (`-d`/`--data` format detection) and notebook-kernel wiring — #751.
- `sparql_endpoint` Graph Store Protocol / `LOAD` wiring (currently
  returns an explicit "not yet supported" error for `application/rdf+xml`
  in the `LOAD` path, and silently mis-maps to Turtle in the GSP upload
  path per the epic's noted latent bug) — #751.
- W3C subquery-suite `sq01`–`sq10` un-skip — #752.
- A serializer (`oxrdfxml` has one, but nothing in the epic or this issue
  asks for RDF/XML output; `turtle`'s own `serialize.rs` only covers
  Turtle/TriG/N-Quads today). Not filing a separate issue for this since
  the epic itself never asked for it — revisit only if a real need shows
  up.

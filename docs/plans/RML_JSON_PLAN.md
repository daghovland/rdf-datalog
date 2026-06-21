# RML JSON Source Plan

## Goal

Extend the `rml` crate to support JSON and JSONL (newline-delimited JSON) as
`LogicalSource` inputs, using JSONPath as the reference formulation. This lets
data engineers map hierarchical JSON data (REST API responses, JSONL event
streams) to RDF triples using the same standard `apply_rml_mapping` API as CSV.

## Spec references

- RML 1.0 §LogicalSource — <https://www.w3.org/TR/rml/#logical-source>
- JSONPath (RFC 9535) — <https://www.rfc-editor.org/rfc/rfc9535>
- `rml:JSONPath` — reference formulation IRI in the W3C RML 1.0 namespace
- RML W3C test cases (JSON subset) — <https://github.com/kg-construct/rml-test-cases>
- `ql:JSONPath` (`http://semweb.mmlab.be/ns/ql#JSONPath`) — Dimou-lab namespace,
  older tooling; treated as alias at load time

---

## Scope

**In scope:**
- JSON file source (`rml:referenceFormulation rml:JSONPath`)
- JSONL file source (one JSON object per line, same formulation)
- `rml:iterator` JSONPath to select the iterable array (e.g. `$.students[*]`)
- `rml:reference` JSONPath expressions to extract fields from each JSON object
- Template placeholders that match reference keys (e.g. `{$.id}`)
- Scalar value coercion: strings, numbers, booleans → string for term generation
- Nested object access via JSONPath (e.g. `$.address.city`)
- Array values: first element used; empty array → triple skipped (None)

**Deferred:**
- SQL/JDBC sources
- XML / XPath sources
- Join across JSON sources (`rml:JoinCondition` on JSON keys)
- FunctionMap (FNML)
- Parallel/partitioned execution
- Dimou-lab `rr:` (R2RML) compatibility shim for JSON

---

## Architecture: the `Row` abstraction

The CSV source maps column names to string values.
The JSON source maps JSONPath expressions to `serde_json::Value` results.
These differ only in *how a reference is resolved* against a row.

The cleanest extension is a `SourceRow` trait that abstracts reference lookup:

```rust
// sources/mod.rs
pub trait SourceRow {
    /// Resolve a reference expression against this row.
    /// Returns None if the reference is absent, null, or an empty array.
    fn get_str(&self, reference: &str) -> Option<String>;
}
```

**`CsvRow`** wraps the existing `HashMap<String, String>`:
```rust
pub struct CsvRow(pub HashMap<String, String>);
impl SourceRow for CsvRow {
    fn get_str(&self, reference: &str) -> Option<String> {
        let v = self.0.get(reference)?;
        if v.is_empty() { None } else { Some(v.clone()) }
    }
}
```

**`JsonRow`** wraps a `serde_json::Value` (one JSON object):
```rust
pub struct JsonRow(pub serde_json::Value);
impl SourceRow for JsonRow {
    fn get_str(&self, reference: &str) -> Option<String> {
        // evaluate `reference` as a JSONPath expression against self.0
        // return first result coerced to String, or None
    }
}
```

The engine's `eval_format_function` changes signature to:
```rust
fn eval_format_function(ff: &FormatFunction, row: &dyn SourceRow, ds: &mut Datastore) -> Option<GraphElementId>
```

This requires a minor refactor of `engine.rs` and `template.rs` (which currently
take `&RawRow`). The CSV path stays identical in behaviour; only the type changes.

`RawRow` becomes an alias kept for backward compatibility in tests:
```rust
pub type RawRow = CsvRow;
```

---

## Files to add

### `rml/src/sources/json.rs`

```rust
pub struct JsonSource {
    pub path: PathBuf,
    pub format: JsonFormat,
    pub iterator: Option<String>,  // JSONPath to select the iterable
}

pub enum JsonFormat {
    Json,   // standard JSON file, single document
    Jsonl,  // newline-delimited JSON (one object per line)
}

impl JsonSource {
    pub fn rows(&self) -> Box<dyn Iterator<Item = Result<JsonRow, RmlError>> + '_>
}
```

**JSON mode** (`JsonFormat::Json`):
1. Read and parse the whole file with `serde_json::from_str`
2. Apply `iterator` JSONPath (e.g. `$.students[*]`) to get an array of objects
3. If `iterator` is absent, treat the document root as a single-element array
4. Yield each element as a `JsonRow`

**JSONL mode** (`JsonFormat::Jsonl`):
1. Read line by line with a `BufReader`
2. Skip blank lines
3. Parse each non-blank line as a JSON object with `serde_json::from_str`
4. If `iterator` is set, apply it to each parsed object before yielding
5. Yield each resulting object (or single object if no iterator) as a `JsonRow`

---

## Files to change

### `rml/src/ast.rs`

```rust
pub enum ReferenceFormulation {
    Csv,
    JsonPath,    // new: rml:JSONPath
}
```

The `LogicalSource.iterator` field (already `Option<String>`) becomes meaningful
for JSON: it holds the JSONPath expression that selects the iterable array.

### `rml/src/plan.rs`

`LogicalScan` already carries `reference_formulation` and `iterator`. No changes
needed to the plan types.

### `rml/src/sources/mod.rs`

Add `pub mod json;`, the `SourceRow` trait, and `CsvRow`/`JsonRow` wrappers.
`RawRow` becomes a type alias for `CsvRow`.

### `rml/src/engine.rs`

- `execute_plan`: dispatch to `CsvSource` or `JsonSource` based on
  `scan.reference_formulation`
- `eval_format_function(ff, row: &dyn SourceRow, ds)` — uses `row.get_str(reference)` 
  instead of `row.get(col)`

### `rml/src/template.rs`

`expand_template(template, row: &dyn SourceRow, encode)` — same logic,
`row.get_str(placeholder_key)` instead of `row.get(col)`.

### `rml/src/loader.rs`

Recognize `rml:JSONPath` (and `ql:JSONPath`) as `ReferenceFormulation::JsonPath`.
Detect JSONL from file extension (`.jsonl` or `.ndjson`) or an explicit property
if the spec provides one. The plan is to auto-detect by extension.

---

## JSONPath evaluation

Use the `serde_json_path` crate (RFC 9535 compliant):

```toml
serde_json_path = "0.7"
```

Evaluation strategy in `JsonRow::get_str`:
```rust
fn get_str(&self, reference: &str) -> Option<String> {
    let path = serde_json_path::JsonPath::parse(reference).ok()?;
    let node_list = path.query(&self.0);
    let first = node_list.first()?;
    scalar_to_string(first)
}

fn scalar_to_string(v: &serde_json::Value) -> Option<String> {
    match v {
        Value::String(s) => if s.is_empty() { None } else { Some(s.clone()) },
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b)   => Some(b.to_string()),
        Value::Null      => None,
        Value::Array(a)  => a.first().and_then(scalar_to_string),
        Value::Object(_) => None,  // nested objects not coercible to string
    }
}
```

Note: if `reference` is not a valid JSONPath (e.g. a plain field name like `name`
without the `$.` prefix), the crate may reject it. The loader should either:
a) Auto-prefix with `$.` when the reference doesn't start with `$`, or
b) Try bare field lookup on the JSON object as a fallback.

Option (a) is simpler and correct for the common case: `rml:reference "name"` in
a JSON mapping → evaluate `$.name`.

---

## Loader changes: recognizing JSON mappings

In `loader.rs`, after checking `rml:referenceFormulation`:

```rust
Some(s) if s == rml("JSONPath") || s == ql("JSONPath") => {
    ReferenceFormulation::JsonPath
}
```

where `ql(local) = format!("http://semweb.mmlab.be/ns/ql#{local}")`.

JSONL detection: in `engine.rs`/`execute_plan`, check the file extension:
```rust
let format = if path.extension().map_or(false, |e| e == "jsonl" || e == "ndjson") {
    JsonFormat::Jsonl
} else {
    JsonFormat::Json
};
```

---

## Template placeholder convention for JSON

In CSV: `{id}` — the column name is the placeholder
In JSON: `{$.id}` — the JSONPath expression is the placeholder

The `expand_template` function treats the text between `{` and `}` as the
reference key, which is exactly what `get_str` receives. No change to the
template expansion algorithm is needed. The W3C RML test cases use `{$.id}`.

---

## W3C test case fixtures

Copy the JSON subset of the W3C RML test cases into `rml/tests/fixtures/`:

| Test case | Feature |
|---|---|
| `rmltc0001b` | Simple JSON → one predicate, one row |
| `rmltc0002b` | Multiple predicates from JSON fields |
| `rmltc0007c` | Language-tagged literal from JSON field |
| `rmltc0007d` | Datatype literal from JSON number field |
| `rmltc0009b` | Named graph from JSON mapping |
| `rmltc0010b` | rml:class shorthand with JSON source |
| `rmltc0014a` | Nested JSON object via deep JSONPath |
| `rmltc0015a` | JSON array iteration with rml:iterator |
| `jsonl_basic` | JSONL source (local fixture, not W3C) |

Each fixture directory: `input.json` (or `input.jsonl`), `mapping.ttl`, and
expected triples verified by the test.

---

## TDD phases

### Phase 1 — `SourceRow` trait + refactor (red → green)

Tests in `rml/tests/source_row_tests.rs`:
- `csv_row_get_str_returns_value` — simple CSV field lookup
- `csv_row_empty_returns_none`
- `csv_row_missing_returns_none`
- `json_row_get_str_simple_field` — `$.name` against `{"name": "Alice"}`
- `json_row_get_str_nested` — `$.address.city` against `{"address": {"city": "Paris"}}`
- `json_row_number_coerced_to_string` — `$.age` → `"30"`
- `json_row_bool_coerced_to_string` — `$.active` → `"true"`
- `json_row_null_returns_none`
- `json_row_empty_string_returns_none`
- `json_row_array_first_element` — `$.tags` against `{"tags": ["rdf", "owl"]}` → `"rdf"`
- `json_row_empty_array_returns_none`
- `json_row_bare_field_name_without_dollar` — `"name"` auto-prefixed to `$.name`

After red phase, refactor engine + template to use `&dyn SourceRow`. All
existing CSV tests must remain green.

### Phase 2 — JSON source (red → green)

Tests in `rml/tests/json_tests.rs`:
- `json_source_reads_single_object` — file with `[{"id": 1, "name": "Alice"}]`
- `json_source_reads_multiple_objects` — array with two elements
- `json_source_iterator_selects_array` — `$.students[*]` extracts nested array
- `json_source_no_iterator_treats_root_as_single` — root object, no iterator
- `json_source_empty_array_yields_no_rows`
- `json_source_missing_file_yields_error`
- `jsonl_source_reads_lines` — three JSONL lines → three rows
- `jsonl_source_skips_blank_lines`
- `jsonl_source_stops_on_parse_error`
- `json_source_nested_field_via_jsonpath`

### Phase 3 — Loader recognizes JSON (red → green)

Tests in `rml/tests/loader_tests.rs` (new cases, keep existing CSV tests):
- `loader_parses_json_reference_formulation` — `rml:JSONPath` → `ReferenceFormulation::JsonPath`
- `loader_parses_ql_jsonpath_alias` — Dimou `ql:JSONPath` also works
- `loader_parses_iterator_string` — `rml:iterator "$.students[*]"` → `iterator: Some("$.students[*]")`
- `loader_parses_jsonpath_reference` — `rml:reference "$.name"` → `TermMap::Reference("$.name")`

### Phase 4 — End-to-end W3C JSON fixtures (red → green)

Tests in `rml/tests/json_end_to_end.rs`:
- One test per fixture in `rml/tests/fixtures/rmltc000Xb/`
- Same structure as `end_to_end.rs`: call `apply_rml_mapping`, assert triples present

### Phase 5 — Integration tests (red → green)

Tests in `tests/rml_json_integration.rs` (root crate):
- `json_mapped_data_is_queryable_with_sparql`
- `json_combined_with_turtle_ontology`
- `json_plus_owlrl_reasoning`
- `json_iterator_filters_to_nested_array`

---

## Dependencies to add

`rml/Cargo.toml`:
```toml
serde_json = "1"
serde_json_path = "0.7"
```

Note: `serde_json` is likely already a transitive dependency of the workspace
(via `jsonld_parser`). Adding it explicitly to `rml/Cargo.toml` makes it
available without relying on transitive resolution.

---

## Worked example

**Input (`students.json`)**:
```json
{
  "students": [
    {"id": 1, "name": "Alice"},
    {"id": 2, "name": "Bob"}
  ]
}
```

**Mapping (`mapping.ttl`)**:
```turtle
@prefix rml: <http://w3id.org/rml/> .
@prefix ex:  <http://example.com/> .

<http://example.com/StudentMap>
    a rml:TriplesMap ;
    rml:logicalSource [
        rml:source "students.json" ;
        rml:referenceFormulation rml:JSONPath ;
        rml:iterator "$.students[*]"
    ] ;
    rml:subjectMap [
        rml:template "http://example.com/Student/{$.id}"
    ] ;
    rml:predicateObjectMap [
        rml:predicate ex:name ;
        rml:objectMap [ rml:reference "$.name" ]
    ] .
```

**Expected triples**:
```turtle
<http://example.com/Student/1> <http://example.com/name> "Alice" .
<http://example.com/Student/2> <http://example.com/name> "Bob" .
```

**Pipeline trace**:
1. Loader: parses mapping, `iterator = Some("$.students[*]")`, `reference = "$.name"` as `TermMap::Reference("$.name")`
2. Translate: `Projection(Scan(...))` plan with `Subject → Dynamic(Template("http://example.com/Student/{$.id}"))`, `Predicate → Constant(ex:name)`, `Object → Dynamic(Reference("$.name"))`
3. Optimizer: constant-folds `ex:name` predicate (already constant, no change)
4. Engine: opens `students.json`, evaluates `$.students[*]`, gets two JSON objects, for each:
   - `$.id` → `"1"` → subject `<http://example.com/Student/1>`
   - `$.name` → `"Alice"` → literal `"Alice"`
   - inserts triple

---

## Backward compatibility

- All existing CSV tests remain green — `CsvRow` implements `SourceRow` identically
  to the current `HashMap<String, String>` logic
- `apply_rml_mapping` API is unchanged
- Mapping files with `rml:referenceFormulation rml:CSV` continue to work

---

## Open questions

1. **JSONPath crate choice**: `serde_json_path` (RFC 9535) vs `jsonpath-rust`
   (more established, older standard). Evaluate at implementation time. Prefer
   RFC 9535 for long-term spec alignment.

2. **Auto-prefix `$.**`: Should bare field names like `"name"` auto-expand to
   `"$.name"` in JSONPath mode? Yes, as a convenience — many existing RML
   mappings use bare names. The fallback order: try as JSONPath, if invalid,
   try as top-level object key.

3. **JSONL detection**: By file extension (`.jsonl`, `.ndjson`) or by explicit
   property in the mapping (`rml:sourceType rml:JSONL`)? Extension is simpler
   and works for the common case. Add an explicit property only if there's a
   practical need.

# RML XML Source Plan

> Part of [#25 RML pipeline remaining source/function gaps](https://github.com/daghovland/rdf-datalog/issues/25). `XmlSource`/`XmlRow` (`rml/src/sources/xml.rs`),
> loader recognition of `rml:XPath`/`ql:XPath`, 9 end-to-end fixture tests
> (`rml/tests/xml_end_to_end.rs`) and 5 root-crate integration tests
> (`tests/rml_xml_integration.rs`) all green. Documented in
> `docs/user/rml-mapping.md` and `README.md`.

## Goal

Extend the `rml` crate to support XML files as `LogicalSource` inputs, using XPath
as the reference formulation. This lets data engineers map structured XML data
(REST/SOAP API responses, data exports, configuration files) to RDF triples using the
same `apply_rml_mapping` API already used for CSV and JSON.

## Spec references

- RML 1.0 §LogicalSource — <https://www.w3.org/TR/rml/#logical-source>
- XPath 1.0 (W3C) — <https://www.w3.org/TR/xpath/>
- `rml:XPath` — reference formulation IRI: `http://w3id.org/rml/XPath`
- `ql:XPath` (`http://semweb.mmlab.be/ns/ql#XPath`) — Dimou-lab namespace, treated
  as alias (same as `ql:JSONPath` for JSON)
- RML W3C test cases (XML subset) — <https://github.com/kg-construct/rml-test-cases>

---

## Scope

**In scope:**
- XML file source (`rml:referenceFormulation rml:XPath`)
- `rml:iterator` XPath to select the repeating nodes (e.g. `/students/student`)
- `rml:reference` XPath expressions evaluated relative to each selected node
  (e.g. `name`, `@id`, `address/city`)
- Template placeholders matching reference XPath keys (e.g. `{@id}`, `{name}`)
- Text node and attribute value extraction as strings
- Namespaced element names in iterators and references (namespace-aware XPath 1.0)

**Deferred:**
- XPath 2.0 / 3.0 features (functions, sequences, constructors)
- Multiple documents joined via `rml:JoinCondition`
- XSLT preprocessing
- Streaming / StAX processing of large XML files
- FunctionMap (FNML) over XML fields

---

## Architecture: `XmlRow` in the `SourceRow` abstraction

`XmlRow` is a new `SourceRow` implementation that holds a serialized XML fragment
(the text of one selected node). XPath reference evaluation re-parses this fragment
on demand. The approach avoids lifetime entanglement between the DOM tree and the row:

```
XmlSource::rows()
    → parse full document
    → evaluate iterator XPath → Vec<Node>
    → for each Node: serialize subtree to String
    → yield XmlRow(String)        ← owns its own XML text

XmlRow::get_str(reference)
    → re-parse self.0 as mini-document
    → evaluate reference XPath against root
    → return string value of first result, or None
```

The re-parse overhead is acceptable for RML workloads (mapping files are typically
tens of thousands of rows at most). A caching layer can be added later if profiling
shows it matters.

### `XmlRow`

```rust
pub struct XmlRow(pub String);   // serialized XML of one selected node

impl SourceRow for XmlRow {
    fn get_str(&self, reference: &str) -> Option<String> {
        // parse self.0 into a temporary Document
        // evaluate `reference` as an XPath 1.0 expression
        // return the string-value of the first result, or None if no match
    }
}
```

---

## XPath crate choice: `sxd-document` + `sxd-xpath`

Both crates are pure Rust, XPath 1.0 compliant, and compose cleanly:

| Crate | Role |
|---|---|
| `sxd-document` | XML parsing, DOM, node serialization |
| `sxd-xpath` | XPath 1.0 evaluation against `sxd-document` nodes |

```toml
sxd-document = "0.3"
sxd-xpath    = "0.4"
```

**Evaluation pattern** in `XmlRow::get_str`:

```rust
use sxd_document::parser as xml_parser;
use sxd_xpath::{Factory, Context, Value};

fn get_str(&self, reference: &str) -> Option<String> {
    let package = xml_parser::parse(&self.0).ok()?;
    let document = package.as_document();
    let factory = Factory::new();
    let xpath = factory.build(reference).ok()??;
    let context = Context::new();
    let root = document.root().children().into_iter()
        .find(|n| n.element().is_some())?;
    match xpath.evaluate(&context, root).ok()? {
        Value::String(s) if !s.is_empty() => Some(s),
        Value::Nodeset(ns) => {
            let node = ns.iter().next()?;
            let s = node.string_value();
            if s.is_empty() { None } else { Some(s) }
        }
        _ => None,
    }
}
```

**Iterator evaluation** in `XmlSource::collect_rows`:

```rust
use sxd_document::parser as xml_parser;
use sxd_xpath::{Factory, Context};

fn collect_rows(&self) -> Result<Vec<XmlRow>, RmlError> {
    let text = std::fs::read_to_string(&self.path)?;
    let package = xml_parser::parse(&text)?;
    let document = package.as_document();
    let factory = Factory::new();
    let iterator_expr = self.iterator.as_deref().unwrap_or("/*");
    let xpath = factory.build(iterator_expr)?.ok_or(RmlError::InvalidXPath)?;
    let context = Context::new();
    let root_node = ...;   // document root element
    let nodeset = xpath.evaluate(&context, root_node)?.nodeset()?;
    nodeset.iter()
        .map(|node| serialize_node(node))
        .map(|xml| Ok(XmlRow(xml)))
        .collect()
}
```

Node serialization uses `sxd_document::writer::format_document` on a temporary
single-node document, or the node's raw string representation.

---

## Files to add

### `rml/src/sources/xml.rs`

```rust
pub struct XmlSource {
    pub path: PathBuf,
    pub iterator: Option<String>,   // XPath selecting repeating nodes
}

impl XmlSource {
    pub fn new(path: PathBuf) -> Self
    pub fn with_iterator(self, iterator: String) -> Self
    pub fn rows(&self) -> Box<dyn Iterator<Item = Result<XmlRow, RmlError>> + '_>
    fn collect_rows(&self) -> Result<Vec<XmlRow>, RmlError>
}
```

---

## Files to change

### `rml/src/ast.rs`

```rust
pub enum ReferenceFormulation {
    Csv,
    JsonPath,
    XPath,      // new: rml:XPath
}
```

### `rml/src/lib.rs`

```rust
pub enum RmlError {
    // existing variants ...
    Xml { file: std::path::PathBuf, source: sxd_document::parser::Error },
    InvalidXPath,
}
```

### `rml/src/sources/mod.rs`

Add `pub mod xml;` and re-export `XmlSource`, `XmlRow`.

### `rml/src/loader.rs`

Recognize `rml:XPath` and `ql:XPath`:

```rust
Some(s) if s == rml("XPath") || s == ql("XPath") => ReferenceFormulation::XPath,
```

### `rml/src/engine.rs`

Add dispatch arm in `execute_plan`:

```rust
ReferenceFormulation::XPath => {
    let mut source = XmlSource::new(path);
    if let Some(iter) = &scan.iterator {
        source = source.with_iterator(iter.clone());
    }
    for row_result in source.rows() {
        let row = row_result?;
        execute_row(proj, &row, ds)?;
    }
}
```

---

## Template placeholder convention for XML

In XML mappings, `rml:reference` holds an XPath expression such as `name` or `@id`.
Template placeholders mirror the reference expression directly:

```turtle
rml:subjectMap [ rml:template "http://example.com/Student/{@id}" ] ;
rml:predicateObjectMap [
    rml:predicate ex:name ;
    rml:objectMap [ rml:reference "name" ]
] .
```

The text between `{` and `}` in the template is passed verbatim to `XmlRow::get_str`
as the XPath reference. No normalization is needed; `expand_template` already passes
the placeholder text to `row.get_str`.

---

## W3C test case fixtures

Copy the XML subset of the W3C RML test cases into `rml/tests/fixtures/`:

| Directory | Feature |
|---|---|
| `rmltc0001c` | Simple XML → one predicate, one row (`<id>` + `<name>`) |
| `rmltc0002c` | Multiple predicates from sibling XML elements |
| `rmltc0007e` | Language-tagged literal from an XML element |
| `rmltc0007f` | Datatype literal from an XML element (`xsd:integer`) |
| `rmltc0009c` | Named graph from XML mapping |
| `rmltc0010c` | `rml:class` shorthand with XML source |
| `rmltc0014b` | Nested XML element via deep XPath (`address/city`) |
| `rmltc0015b` | Repeated XML elements with `rml:iterator` |

Each fixture directory contains: `input.xml`, `mapping.ttl`, expected triples
verified by the test function.

**Example fixture — `rmltc0001c`:**

```xml
<!-- input.xml -->
<students>
  <student>
    <id>10</id>
    <name>Venus Williams</name>
  </student>
</students>
```

```turtle
# mapping.ttl
@prefix rml: <http://w3id.org/rml/> .
@prefix ex:  <http://example.com/> .

<http://example.com/StudentMap>
    a rml:TriplesMap ;
    rml:logicalSource [
        rml:source "input.xml" ;
        rml:referenceFormulation rml:XPath ;
        rml:iterator "/students/student"
    ] ;
    rml:subjectMap [
        rml:template "http://example.com/Student/{id}"
    ] ;
    rml:predicateObjectMap [
        rml:predicate ex:name ;
        rml:objectMap [ rml:reference "name" ]
    ] .
```

---

## TDD phases

### Phase 1 — `XmlRow` + `SourceRow` impl (red → green)

Tests in `rml/tests/xml_row_tests.rs`:

- `xml_row_get_str_simple_element` — `"name"` against `<student><name>Alice</name></student>` → `"Alice"`
- `xml_row_get_str_attribute` — `"@id"` against `<student id="10">` → `"10"`
- `xml_row_get_str_nested_element` — `"address/city"` against `<student><address><city>Paris</city></address></student>` → `"Paris"`
- `xml_row_get_str_missing_element_returns_none` — `"age"` when no `<age>` child
- `xml_row_get_str_empty_element_returns_none` — `<name></name>` → None
- `xml_row_get_str_text_node_explicit` — `"name/text()"` → same as `"name"`
- `xml_row_number_as_text` — `<id>42</id>` → `"42"`
- `xml_row_implements_source_row_trait_object` — `&dyn SourceRow` dispatch works

### Phase 2 — `XmlSource::rows()` (red → green)

Tests in `rml/tests/xml_source_tests.rs`:

- `xml_source_reads_single_element` — file with one `<student>`, iterator `/students/student`
- `xml_source_reads_multiple_elements` — two `<student>` elements → two rows
- `xml_source_no_iterator_defaults_to_root` — no `iterator` → single row wrapping document root
- `xml_source_empty_nodeset_yields_no_rows` — iterator matches nothing
- `xml_source_missing_file_yields_error` — non-existent path → `RmlError`
- `xml_source_malformed_xml_yields_error` — bad XML → `RmlError::Xml`
- `xml_source_attribute_accessible_in_row` — `@id` reference works after node selection
- `xml_source_nested_iterator` — iterator `/root/items/item` extracts deeply nested nodes

### Phase 3 — Loader recognizes XPath (red → green)

Tests added to `rml/tests/loader_tests.rs`:

- `loader_parses_xpath_reference_formulation` — `rml:XPath` → `ReferenceFormulation::XPath`
- `loader_parses_ql_xpath_alias` — `ql:XPath` also recognized
- `loader_parses_xml_iterator` — `rml:iterator "/students/student"` → `iterator: Some(...)`
- `loader_parses_xpath_reference` — `rml:reference "name"` → `TermMap::Reference("name")`

### Phase 4 — End-to-end W3C XML fixtures (red → green)

Tests in `rml/tests/xml_end_to_end.rs`:

- `rmltc0001c_simple_xml` — one student, one predicate
- `rmltc0002c_multiple_predicates` — `<id>` and `<name>` both mapped
- `rmltc0007e_language_tagged_literal` — `rml:language "en"` with XML source
- `rmltc0007f_datatype_literal` — `rml:datatype xsd:integer` with XML number element
- `rmltc0009c_named_graph` — triples placed in a named graph
- `rmltc0010c_rml_class` — `rml:class` shorthand injects `rdf:type` triple
- `rmltc0014b_nested_element` — `address/city` deep path
- `rmltc0015b_repeated_elements` — multiple `<student>` nodes iterated

Engine dispatch is wired in this phase (same as JSON Phase 4).

### Phase 5 — Integration tests in root crate (red → green)

Tests in `tests/rml_xml_integration.rs`:

- `xml_mapped_data_is_queryable_with_sparql` — load XML via RML, run SELECT, check results
- `xml_subject_iris_follow_template` — `{@id}` IRI template produces correct subject
- `xml_combined_with_turtle_ontology` — load ontology Turtle + XML mapping, query
- `xml_plus_owlrl_reasoning_infers_superclass_membership` — OWL-RL over XML-sourced data
- `xml_deep_xpath_reference` — nested element access via multi-step XPath

---

## Dependencies to add

`rml/Cargo.toml`:
```toml
sxd-document = "0.3"
sxd-xpath    = "0.4"
```

Both crates have no C dependencies and compile on stable Rust.

---

## Worked example

**Input (`students.xml`)**:
```xml
<students>
  <student id="1">
    <name>Alice</name>
  </student>
  <student id="2">
    <name>Bob</name>
  </student>
</students>
```

**Mapping (`mapping.ttl`)**:
```turtle
@prefix rml: <http://w3id.org/rml/> .
@prefix ex:  <http://example.com/> .

<http://example.com/StudentMap>
    a rml:TriplesMap ;
    rml:logicalSource [
        rml:source "students.xml" ;
        rml:referenceFormulation rml:XPath ;
        rml:iterator "/students/student"
    ] ;
    rml:subjectMap [
        rml:template "http://example.com/Student/{@id}"
    ] ;
    rml:predicateObjectMap [
        rml:predicate ex:name ;
        rml:objectMap [ rml:reference "name" ]
    ] .
```

**Expected triples**:
```turtle
<http://example.com/Student/1> <http://example.com/name> "Alice" .
<http://example.com/Student/2> <http://example.com/name> "Bob" .
```

**Pipeline trace**:
1. Loader: parses mapping → `ReferenceFormulation::XPath`, `iterator = Some("/students/student")`, `reference = "name"` as `TermMap::Reference("name")`
2. Translate: `Projection(Scan(...))` plan as usual
3. Engine: opens `students.xml`, parses DOM, evaluates `/students/student` → two `<student>` nodes, serializes each to an `XmlRow`
4. For each `XmlRow`:
   - `get_str("@id")` → `"1"` / `"2"` → subject IRI
   - `get_str("name")` → `"Alice"` / `"Bob"` → literal object
   - inserts triple

---

## Backward compatibility

- All existing CSV and JSON tests remain green — only a new `XPath` dispatch arm is
  added to `execute_plan`; existing arms are unchanged
- `apply_rml_mapping` API is unchanged
- Mapping files with `rml:CSV` or `rml:JSONPath` continue to work exactly as before

---

## Open questions

1. **XPath 1.0 vs 2.0**: `sxd-xpath` implements XPath 1.0. The W3C RML test cases
   use XPath 1.0 paths, so this is sufficient. If XPath 2.0 is needed later, the
   `xpath2` crate or an XSLT processor would be required.

2. **Node serialization**: `sxd-document` does not expose a single-node serializer
   directly. Options: (a) build a new single-node document and serialize it; (b) use
   `sxd_document::writer::format_document` on a one-element wrapper; (c) walk the
   node to produce a simple string. Evaluate at implementation time — option (b) is
   cleanest.

3. **Default iterator**: When `rml:iterator` is absent, should the engine yield one
   row per top-level element or treat the document root as a single row? JSON used
   "root is a single row". XML convention is typically "iterate over the root's
   children". The plan defaults to the document root element as a single row (same as
   JSON) and the user must specify an iterator for repeated records.

4. **Namespace-prefixed XPath**: If XML uses namespace prefixes, XPath must bind them.
   The RML spec does not define a way to pass namespace bindings in the mapping. For
   Phase 1 scope, use `local-name()` XPath function as a workaround if namespaces are
   present; formal namespace binding support can be deferred.

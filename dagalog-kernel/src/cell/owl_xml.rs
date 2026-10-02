use dag_rdf::Datastore;
use std::path::Path;

/// Load an OWL 2 XML Serialization (`.owx`/`.owl`) file and materialise it
/// fully: ABox assertions become quads (`owl2rl2datalog::assert_abox`) and
/// TBox axioms are compiled to Datalog rules and evaluated immediately
/// (`owl2rl2datalog::owl2datalog` + `datalog::evaluate_rules`).
///
/// Mirrors [`crate::cell::functional::execute_functional_file`] exactly —
/// see that function's doc comment for why reasoning must happen at load
/// time rather than being deferred to a later `%%reason` cell: an OWL/XML
/// TBox axiom has no RDF triple representation either (there is no RDF
/// round-trip for frame-based/XML OWL syntaxes today, tracked in
/// [#177](https://github.com/daghovland/rdf-datalog/issues/177)).
///
/// `owl_xml_parser` does not yet parse ABox assertion axioms
/// ([#608](https://github.com/daghovland/rdf-datalog/issues/608) is open),
/// so a document containing one fails to parse with a clear error rather
/// than silently dropping it — the "skipped ABox assertion" warning that
/// `%%manchester`/`%%functional` can surface doesn't apply here yet; it
/// will once #608 lands and `assert_abox` has something to skip.
///
/// See [#609](https://github.com/daghovland/rdf-datalog/issues/609).
pub fn execute_owl_xml_file(ds: &mut Datastore, path: &Path) -> Result<String, String> {
    let src = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    let ontology = owl_xml_parser::parse(&src)
        .map_err(|e| format!("OWL 2 XML Serialization parse error: {e}"))?;

    let before = ds.named_graphs.quad_count;
    let abox_report = owl2rl2datalog::assert_abox(ds, &ontology);
    let abox_added = abox_report.triples_added;
    let axiom_count = ontology.axioms.len();

    let rules = owl2rl2datalog::owl2datalog(&mut ds.resources, &ontology);
    let rule_count = rules.len();
    datalog::evaluate_rules(rules, ds).map_err(|e| e.to_string())?;

    let total_added = ds.named_graphs.quad_count - before;
    let mut status = format!(
        "Loaded {} axiom{} ({} ABox triple{} asserted), applied {} rule{}, \
         {} triple{} added in total.",
        axiom_count,
        if axiom_count == 1 { "" } else { "s" },
        abox_added,
        if abox_added == 1 { "" } else { "s" },
        rule_count,
        if rule_count == 1 { "" } else { "s" },
        total_added,
        if total_added == 1 { "" } else { "s" },
    );
    // Surface skipped (non-atomic) ABox assertions instead of silently
    // dropping them — see
    // [#366](https://github.com/daghovland/rdf-datalog/issues/366). The
    // underlying gap (no RDF encoding for complex class/property expressions)
    // is tracked separately in
    // [#373](https://github.com/daghovland/rdf-datalog/issues/373). Always
    // empty today since `owl_xml_parser` has no ABox assertions to skip yet
    // (#608), kept for parity with `%%manchester`/`%%functional` and to
    // start working automatically once #608 lands.
    if !abox_report.skipped.is_empty() {
        status.push_str(&format!(
            " WARNING: {} ABox assertion{} skipped (not materialisable as a single ground \
             triple; see issue #373).",
            abox_report.skipped.len(),
            if abox_report.skipped.len() == 1 {
                ""
            } else {
                "s"
            },
        ));
    }
    Ok(status)
}

#[cfg(test)]
mod tests {
    use super::*;

    const OWX: &str = r#"<?xml version="1.0"?>
<Ontology xmlns="http://www.w3.org/2002/07/owl#" ontologyIRI="http://example.org/animals">
    <Declaration><Class IRI="http://example.org/Animal"/></Declaration>
    <Declaration><Class IRI="http://example.org/Dog"/></Declaration>
    <SubClassOf>
        <Class IRI="http://example.org/Dog"/>
        <Class IRI="http://example.org/Animal"/>
    </SubClassOf>
</Ontology>
"#;

    fn write_fixture(dir: &std::path::Path, contents: &str) -> std::path::PathBuf {
        let p = dir.join("animals.owx");
        std::fs::write(&p, contents).expect("write fixture");
        p
    }

    /// Insert `ex:fido a ex:Dog` directly, standing in for an ABox that
    /// (until #608) `owl_xml_parser` cannot supply itself — mirrors the
    /// equivalent helper in `src/lib.rs`'s test module.
    fn assert_fido_is_dog(ds: &mut Datastore) {
        turtle::parse_turtle(
            ds,
            "@prefix ex: <http://example.org/> .\nex:fido a ex:Dog .\n".as_bytes(),
        )
        .expect("insert ABox fact");
    }

    fn fido_is_animal(ds: &Datastore) -> bool {
        let get = |iri: &str| {
            ds.resources
                .resource_map
                .get(&dag_rdf::GraphElement::NodeOrEdge(
                    dag_rdf::RdfResource::Iri(dag_rdf::IriReference(iri.to_string())),
                ))
                .copied()
        };
        let (fido, rdf_type, animal) = match (
            get("http://example.org/fido"),
            get("http://www.w3.org/1999/02/22-rdf-syntax-ns#type"),
            get("http://example.org/Animal"),
        ) {
            (Some(f), Some(t), Some(a)) => (f, t, a),
            _ => return false,
        };
        !ds.quads_matching(None, Some(fido), Some(rdf_type), Some(animal))
            .is_empty()
    }

    #[test]
    fn test_owl_xml_file_materialises_and_reasons_over_tbox() {
        let tmp = std::env::temp_dir().join(format!(
            "dagalog_kernel_owl_xml_test_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&tmp).expect("create temp dir");
        let path = write_fixture(&tmp, OWX);

        let mut ds = Datastore::new(1_000);
        assert_fido_is_dog(&mut ds);
        let msg = execute_owl_xml_file(&mut ds, &path).expect("should load animals.owx");
        assert!(msg.contains("axiom"), "status should mention axioms: {msg}");
        assert!(msg.contains("rule"), "status should mention rules: {msg}");
        assert!(
            fido_is_animal(&ds),
            "fido should be inferred as an Animal via the TBox rule"
        );

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn test_owl_xml_file_missing_returns_error() {
        let mut ds = Datastore::new(1_000);
        let result =
            execute_owl_xml_file(&mut ds, std::path::Path::new("/nonexistent/animals.owx"));
        assert!(result.is_err(), "missing file should return an error");
    }
}

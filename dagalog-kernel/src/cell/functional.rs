use dag_rdf::Datastore;
use std::path::Path;

/// Load an OWL 2 Functional-Style Syntax (`.ofn`) file and materialise it
/// fully: ABox assertions become quads (`owl2rl2datalog::assert_abox`) and
/// TBox axioms are compiled to Datalog rules and evaluated immediately
/// (`owl2rl2datalog::owl2datalog` + `datalog::evaluate_rules`).
///
/// Mirrors [`crate::cell::manchester::execute_manchester_file`] exactly — see
/// that function's doc comment for why reasoning must happen at load time
/// rather than being deferred to a later `%%reason` cell: a Functional-Style
/// TBox axiom has no RDF triple representation either (there is no RDF
/// round-trip for frame-based/s-expression OWL syntaxes today, tracked in
/// [#177](https://github.com/daghovland/rdf-datalog/issues/177)). See
/// [#633](https://github.com/daghovland/rdf-datalog/issues/633).
pub fn execute_functional_file(ds: &mut Datastore, path: &Path) -> Result<String, String> {
    let src = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    let ontology = owl_functional_parser::parse(&src)
        .map_err(|e| format!("OWL 2 Functional-Style Syntax parse error: {e}"))?;

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
    // [#373](https://github.com/daghovland/rdf-datalog/issues/373).
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

    const OFN: &str = r#"
Prefix(:=<http://example.org/>)
Ontology(
    Declaration(Class(:Animal))
    Declaration(Class(:Dog))
    Declaration(NamedIndividual(:fido))
    SubClassOf(:Dog :Animal)
    ClassAssertion(:Dog :fido)
)
"#;

    fn write_fixture(dir: &std::path::Path, contents: &str) -> std::path::PathBuf {
        let p = dir.join("animals.ofn");
        std::fs::write(&p, contents).expect("write fixture");
        p
    }

    #[test]
    fn test_functional_file_materialises_abox_and_reasons() {
        let tmp = std::env::temp_dir().join(format!(
            "dagalog_kernel_functional_test_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&tmp).expect("create temp dir");
        let path = write_fixture(&tmp, OFN);

        let mut ds = Datastore::new(1_000);
        let msg = execute_functional_file(&mut ds, &path).expect("should load animals.ofn");
        assert!(msg.contains("axiom"), "status should mention axioms: {msg}");
        assert!(msg.contains("rule"), "status should mention rules: {msg}");

        // The inferred triple (fido a Animal) can only be present if both
        // assert_abox (ABox -> quads) and owl2datalog + evaluate_rules
        // (TBox -> rules -> materialisation) ran.
        let get = |iri: &str| {
            ds.resources
                .resource_map
                .get(&dag_rdf::GraphElement::NodeOrEdge(
                    dag_rdf::RdfResource::Iri(dag_rdf::IriReference(iri.to_string())),
                ))
                .copied()
        };
        let fido = get("http://example.org/fido").expect("fido should be interned");
        let rdf_type =
            get("http://www.w3.org/1999/02/22-rdf-syntax-ns#type").expect("rdf:type interned");
        let animal = get("http://example.org/Animal").expect("Animal should be interned");
        assert!(
            !ds.quads_matching(None, Some(fido), Some(rdf_type), Some(animal))
                .is_empty(),
            "fido should be inferred as an Animal"
        );

        std::fs::remove_dir_all(&tmp).ok();
    }

    /// Regression coverage mirroring Manchester's
    /// `test_manchester_file_reports_skipped_abox_assertion`
    /// ([#366](https://github.com/daghovland/rdf-datalog/issues/366)): a
    /// `ClassAssertion` with a non-atomic class expression (`ObjectUnionOf`)
    /// has no single-ground-triple RDF encoding and must be surfaced in the
    /// cell's returned status message, not just dropped with a `log::warn!`
    /// the notebook user never sees.
    const COMPLEX_ABOX_OFN: &str = r#"
Prefix(:=<http://example.org/>)
Ontology(
    Declaration(Class(:Animal))
    Declaration(Class(:Dog))
    Declaration(Class(:Cat))
    Declaration(NamedIndividual(:fido))
    SubClassOf(:Dog :Animal)
    ClassAssertion(ObjectUnionOf(:Dog :Cat) :fido)
)
"#;

    #[test]
    fn test_functional_file_reports_skipped_abox_assertion() {
        let tmp = std::env::temp_dir().join(format!(
            "dagalog_kernel_functional_skip_test_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&tmp).expect("create temp dir");
        let path = write_fixture(&tmp, COMPLEX_ABOX_OFN);

        let mut ds = Datastore::new(1_000);
        let msg = execute_functional_file(&mut ds, &path).expect("should load the ontology");
        assert!(
            msg.to_lowercase().contains("skip"),
            "status should mention the skipped ABox assertion: {msg}"
        );

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn test_functional_file_missing_returns_error() {
        let mut ds = Datastore::new(1_000);
        let result =
            execute_functional_file(&mut ds, std::path::Path::new("/nonexistent/animals.ofn"));
        assert!(result.is_err(), "missing file should return an error");
    }
}

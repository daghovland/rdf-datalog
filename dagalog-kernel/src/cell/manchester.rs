use dag_rdf::Datastore;
use std::path::Path;

/// Load an OWL 2 Manchester Syntax (`.omn`) file and materialise it fully:
/// ABox assertions become quads (`owl2rl2datalog::assert_abox`) and TBox
/// axioms are compiled to Datalog rules and evaluated immediately
/// (`owl2rl2datalog::owl2datalog` + `datalog::evaluate_rules`).
///
/// Unlike `%%turtle`/`%%load`, whose triples a later `%%reason` cell can
/// re-derive axioms from via `rdf_owl_translator::rdf2owl`, a Manchester
/// TBox axiom never becomes an RDF triple — there is no RDF round-trip for
/// frame-based OWL syntaxes today (tracked in
/// [#177](https://github.com/daghovland/rdf-datalog/issues/177)). So this
/// magic must apply reasoning at load time or the TBox is silently lost:
/// `%%reason` run afterwards would see only the ABox quads and have no way
/// to reconstruct `SubClassOf:` etc. from them. See
/// [#161](https://github.com/daghovland/rdf-datalog/issues/161).
pub fn execute_manchester_file(ds: &mut Datastore, path: &Path) -> Result<String, String> {
    let src = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    let ontology = manchester_parser::parse(&src)
        .map_err(|e| format!("Manchester Syntax parse error: {e}"))?;

    let before = ds.named_graphs.quad_count;
    let abox_added = owl2rl2datalog::assert_abox(ds, &ontology);
    let axiom_count = ontology.axioms.len();

    let rules = owl2rl2datalog::owl2datalog(&mut ds.resources, &ontology);
    let rule_count = rules.len();
    datalog::evaluate_rules(rules, ds);

    let total_added = ds.named_graphs.quad_count - before;
    Ok(format!(
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
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const OMN: &str = r#"
Prefix: : <http://example.org/>
Ontology:
Class: Animal
Class: Dog
    SubClassOf: Animal
Individual: fido
    Types: Dog
"#;

    fn write_fixture(dir: &std::path::Path, contents: &str) -> std::path::PathBuf {
        let p = dir.join("animals.omn");
        std::fs::write(&p, contents).expect("write fixture");
        p
    }

    #[test]
    fn test_manchester_file_materialises_abox_and_reasons() {
        let tmp = std::env::temp_dir().join(format!(
            "dagalog_kernel_manchester_test_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&tmp).expect("create temp dir");
        let path = write_fixture(&tmp, OMN);

        let mut ds = Datastore::new(1_000);
        let msg = execute_manchester_file(&mut ds, &path).expect("should load animals.omn");
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

    #[test]
    fn test_manchester_file_missing_returns_error() {
        let mut ds = Datastore::new(1_000);
        let result =
            execute_manchester_file(&mut ds, std::path::Path::new("/nonexistent/animals.omn"));
        assert!(result.is_err(), "missing file should return an error");
    }
}

use std::path::PathBuf;

use ingress::{GraphElement, IriReference, RdfResource};
use rml::ast::{
    LogicalSource, LogicalSourceRef, MappingDocument, ObjectMap, PredicateObjectMap,
    ReferenceFormulation, SubjectMap, TermMap, TermType, TriplesMap,
};
use rml::optimizer::constant_fold;
use rml::plan::{GenerationLogic, LogicalPlan, OutputAttr, TermPattern};
use rml::translate::translate;

fn simple_triples_map(source_file: &str, subject_template: &str) -> TriplesMap {
    TriplesMap {
        id: IriReference("http://example.com/TM".to_string()),
        logical_source: LogicalSource {
            source: LogicalSourceRef::File(PathBuf::from(source_file)),
            reference_formulation: ReferenceFormulation::Csv,
            iterator: None,
        },
        subject_map: SubjectMap {
            term_map: TermMap::Template(subject_template.to_string()),
            term_type: TermType::Iri,
            classes: vec![],
            graph_maps: vec![],
        },
        predicate_object_maps: vec![PredicateObjectMap {
            predicate_maps: vec![(
                TermMap::Constant(GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(
                    "http://example.com/name".to_string(),
                )))),
                TermType::Iri,
            )],
            object_maps: vec![ObjectMap {
                term_map: TermMap::Reference("name".to_string()),
                term_type: TermType::Iri,
                language: None,
                datatype: None,
                parent_triples_map: None,
            }],
            graph_maps: vec![],
        }],
    }
}

// ── translate() ───────────────────────────────────────────────────────────────

#[test]
//#[ignore]
fn translate_one_predicate_object_map_yields_one_plan() {
    let doc = MappingDocument {
        triples_maps: vec![simple_triples_map("data.csv", "http://example.com/{id}")],
    };
    let plans = translate(&doc);
    assert_eq!(plans.len(), 1);
}

#[test]
//#[ignore]
fn translate_two_predicate_object_maps_yield_two_plans() {
    let mut tm = simple_triples_map("data.csv", "http://example.com/{id}");
    tm.predicate_object_maps.push(PredicateObjectMap {
        predicate_maps: vec![(
            TermMap::Constant(GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(
                "http://example.com/age".to_string(),
            )))),
            TermType::Iri,
        )],
        object_maps: vec![ObjectMap {
            term_map: TermMap::Reference("age".to_string()),
            term_type: TermType::Literal,
            language: None,
            datatype: None,
            parent_triples_map: None,
        }],
        graph_maps: vec![],
    });
    let doc = MappingDocument {
        triples_maps: vec![tm],
    };
    let plans = translate(&doc);
    assert_eq!(plans.len(), 2);
}

#[test]
//#[ignore]
fn translate_class_shorthand_adds_extra_plan() {
    let mut tm = simple_triples_map("data.csv", "http://example.com/{id}");
    tm.subject_map
        .classes
        .push(IriReference("http://example.com/Person".to_string()));
    let doc = MappingDocument {
        triples_maps: vec![tm],
    };
    let plans = translate(&doc);
    // 1 data triple plan + 1 rdf:type triple plan from rml:class
    assert_eq!(plans.len(), 2);
}

#[test]
//#[ignore]
fn translate_class_plan_has_constant_rdf_type_predicate() {
    let mut tm = simple_triples_map("data.csv", "http://example.com/{id}");
    tm.subject_map
        .classes
        .push(IriReference("http://example.com/Person".to_string()));
    let doc = MappingDocument {
        triples_maps: vec![tm],
    };
    let plans = translate(&doc);

    // Find the plan whose Predicate is Constant(rdf:type)
    let rdf_type_plan = plans.iter().find(|p| {
        if let LogicalPlan::Projection(proj) = p {
            proj.attrs.iter().any(|(attr, logic)| {
                *attr == OutputAttr::Predicate
                    && matches!(
                        logic,
                        GenerationLogic::Constant(GraphElement::NodeOrEdge(
                            RdfResource::Iri(iri)
                        )) if iri.0 == "http://www.w3.org/1999/02/22-rdf-syntax-ns#type"
                    )
            })
        } else {
            false
        }
    });
    assert!(
        rdf_type_plan.is_some(),
        "expected a plan with rdf:type predicate"
    );
}

#[test]
//#[ignore]
fn translate_subject_template_with_column_is_dynamic() {
    let doc = MappingDocument {
        triples_maps: vec![simple_triples_map("data.csv", "http://example.com/{id}")],
    };
    let plans = translate(&doc);
    if let LogicalPlan::Projection(proj) = &plans[0] {
        let (attr, logic) = proj
            .attrs
            .iter()
            .find(|(a, _)| *a == OutputAttr::Subject)
            .unwrap();
        assert_eq!(*attr, OutputAttr::Subject);
        assert!(
            matches!(logic, GenerationLogic::Dynamic(ff) if matches!(ff.pattern, TermPattern::Template(_))),
            "expected Dynamic(Template) for subject with column reference"
        );
    } else {
        panic!("expected Projection plan");
    }
}

#[test]
//#[ignore]
fn translate_constant_term_map_is_constant_in_plan() {
    // TermMap::Constant should translate directly to GenerationLogic::Constant
    let doc = MappingDocument {
        triples_maps: vec![simple_triples_map("data.csv", "http://example.com/{id}")],
    };
    let plans = translate(&doc);
    if let LogicalPlan::Projection(proj) = &plans[0] {
        let (_, pred_logic) = proj
            .attrs
            .iter()
            .find(|(a, _)| *a == OutputAttr::Predicate)
            .unwrap();
        assert!(
            matches!(pred_logic, GenerationLogic::Constant(_)),
            "constant predicate IRI should be GenerationLogic::Constant after translate"
        );
    } else {
        panic!("expected Projection plan");
    }
}

// ── constant_fold() ───────────────────────────────────────────────────────────

#[test]
//#[ignore]
fn constant_fold_leaves_column_template_dynamic() {
    let doc = MappingDocument {
        triples_maps: vec![simple_triples_map("data.csv", "http://example.com/{id}")],
    };
    let plans = constant_fold(translate(&doc));
    if let LogicalPlan::Projection(proj) = &plans[0] {
        let (_, logic) = proj
            .attrs
            .iter()
            .find(|(a, _)| *a == OutputAttr::Subject)
            .unwrap();
        assert!(
            matches!(logic, GenerationLogic::Dynamic(_)),
            "template with {{id}} column ref must stay Dynamic after folding"
        );
    } else {
        panic!("expected Projection plan");
    }
}

#[test]
//#[ignore]
fn constant_fold_converts_no_placeholder_template_to_constant() {
    // A Template with no {…} is a constant and should be folded
    let mut tm = simple_triples_map("data.csv", "http://example.com/{id}");
    tm.predicate_object_maps[0].object_maps[0] = ObjectMap {
        term_map: TermMap::Template("http://example.com/ConstantObject".to_string()),
        term_type: TermType::Iri,
        language: None,
        datatype: None,
        parent_triples_map: None,
    };
    let doc = MappingDocument {
        triples_maps: vec![tm],
    };
    let plans = constant_fold(translate(&doc));

    if let LogicalPlan::Projection(proj) = &plans[0] {
        let (_, obj_logic) = proj
            .attrs
            .iter()
            .find(|(a, _)| *a == OutputAttr::Object)
            .unwrap();
        assert!(
            matches!(obj_logic, GenerationLogic::Constant(_)),
            "no-placeholder template should be folded to Constant"
        );
    } else {
        panic!("expected Projection plan");
    }
}

#[test]
//#[ignore]
fn constant_fold_already_constant_term_maps_unchanged() {
    let doc = MappingDocument {
        triples_maps: vec![simple_triples_map("data.csv", "http://example.com/{id}")],
    };
    let before = translate(&doc);
    let after = constant_fold(before.clone());
    // Predicate was already Constant; folding must not change it
    let get_pred = |plans: &[LogicalPlan]| {
        if let LogicalPlan::Projection(proj) = &plans[0] {
            proj.attrs
                .iter()
                .find(|(a, _)| *a == OutputAttr::Predicate)
                .map(|(_, l)| l.clone())
        } else {
            None
        }
    };
    assert_eq!(get_pred(&before), get_pred(&after));
}

#[test]
//#[ignore]
fn translate_sets_scan_from_logical_source() {
    let doc = MappingDocument {
        triples_maps: vec![simple_triples_map(
            "students.csv",
            "http://example.com/{id}",
        )],
    };
    let plans = translate(&doc);
    if let LogicalPlan::Projection(proj) = &plans[0] {
        assert!(
            matches!(&*proj.input, LogicalPlan::Scan(s) if s.source == LogicalSourceRef::File(PathBuf::from("students.csv")))
        );
    } else {
        panic!("expected Projection wrapping a Scan");
    }
}

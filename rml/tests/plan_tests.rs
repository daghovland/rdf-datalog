use std::path::PathBuf;

use ingress::{GraphElement, IriReference, RdfResource};
use rml::ast::JoinConditionRef;
use rml::ast::{
    LogicalSource, LogicalSourceRef, MappingDocument, ObjectMap, PredicateObjectMap,
    ReferenceFormulation, SqlConnection, SqlQuery, SqlSourceRef, SubjectMap, TermMap, TermType,
    TriplesMap,
};
use rml::optimizer::constant_fold;
use rml::plan::{GenerationLogic, JoinAlgorithm, LogicalPlan, OutputAttr, TermPattern};
use rml::translate::{choose_join_algorithm, translate};

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
                join_conditions: vec![],
            }],
            graph_maps: vec![],
        }],
    }
}

// ── translate() ───────────────────────────────────────────────────────────────

#[test]
fn translate_one_predicate_object_map_yields_one_plan() {
    let doc = MappingDocument {
        triples_maps: vec![simple_triples_map("data.csv", "http://example.com/{id}")],
    };
    let plans = translate(&doc).unwrap();
    assert_eq!(plans.len(), 1);
}

#[test]
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
            join_conditions: vec![],
        }],
        graph_maps: vec![],
    });
    let doc = MappingDocument {
        triples_maps: vec![tm],
    };
    let plans = translate(&doc).unwrap();
    assert_eq!(plans.len(), 2);
}

#[test]
fn translate_class_shorthand_adds_extra_plan() {
    let mut tm = simple_triples_map("data.csv", "http://example.com/{id}");
    tm.subject_map
        .classes
        .push(IriReference("http://example.com/Person".to_string()));
    let doc = MappingDocument {
        triples_maps: vec![tm],
    };
    let plans = translate(&doc).unwrap();
    // 1 data triple plan + 1 rdf:type triple plan from rml:class
    assert_eq!(plans.len(), 2);
}

#[test]
fn translate_class_plan_has_constant_rdf_type_predicate() {
    let mut tm = simple_triples_map("data.csv", "http://example.com/{id}");
    tm.subject_map
        .classes
        .push(IriReference("http://example.com/Person".to_string()));
    let doc = MappingDocument {
        triples_maps: vec![tm],
    };
    let plans = translate(&doc).unwrap();

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
fn translate_subject_template_with_column_is_dynamic() {
    let doc = MappingDocument {
        triples_maps: vec![simple_triples_map("data.csv", "http://example.com/{id}")],
    };
    let plans = translate(&doc).unwrap();
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
fn translate_constant_term_map_is_constant_in_plan() {
    // TermMap::Constant should translate directly to GenerationLogic::Constant
    let doc = MappingDocument {
        triples_maps: vec![simple_triples_map("data.csv", "http://example.com/{id}")],
    };
    let plans = translate(&doc).unwrap();
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
fn constant_fold_leaves_column_template_dynamic() {
    let doc = MappingDocument {
        triples_maps: vec![simple_triples_map("data.csv", "http://example.com/{id}")],
    };
    let plans = constant_fold(translate(&doc).unwrap());
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
fn constant_fold_converts_no_placeholder_template_to_constant() {
    // A Template with no {…} is a constant and should be folded
    let mut tm = simple_triples_map("data.csv", "http://example.com/{id}");
    tm.predicate_object_maps[0].object_maps[0] = ObjectMap {
        term_map: TermMap::Template("http://example.com/ConstantObject".to_string()),
        term_type: TermType::Iri,
        language: None,
        datatype: None,
        parent_triples_map: None,
        join_conditions: vec![],
    };
    let doc = MappingDocument {
        triples_maps: vec![tm],
    };
    let plans = constant_fold(translate(&doc).unwrap());

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
fn constant_fold_already_constant_term_maps_unchanged() {
    let doc = MappingDocument {
        triples_maps: vec![simple_triples_map("data.csv", "http://example.com/{id}")],
    };
    let before = translate(&doc).unwrap();
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
fn translate_sets_scan_from_logical_source() {
    let doc = MappingDocument {
        triples_maps: vec![simple_triples_map(
            "students.csv",
            "http://example.com/{id}",
        )],
    };
    let plans = translate(&doc).unwrap();
    if let LogicalPlan::Projection(proj) = &plans[0] {
        assert!(
            matches!(&*proj.input, LogicalPlan::Scan(s) if s.source == LogicalSourceRef::File(PathBuf::from("students.csv")))
        );
    } else {
        panic!("expected Projection wrapping a Scan");
    }
}

// ── rml:joinCondition → LogicalPlan::Join (red phase; see RML_JOIN_PLAN.md) ──

fn sport_parent_triples_map() -> TriplesMap {
    let mut tm = simple_triples_map("sport.csv", "http://example.com/sport/{ID}");
    tm.id = IriReference("http://example.com/SportMap".to_string());
    tm
}

fn triples_map_with_join(parent_id: &str, conditions: Vec<JoinConditionRef>) -> TriplesMap {
    let mut tm = simple_triples_map("student.csv", "http://example.com/student/{ID}");
    tm.predicate_object_maps.push(PredicateObjectMap {
        predicate_maps: vec![(
            TermMap::Constant(GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(
                "http://example.com/practises".to_string(),
            )))),
            TermType::Iri,
        )],
        object_maps: vec![ObjectMap {
            term_map: TermMap::Reference(String::new()),
            term_type: TermType::Iri,
            language: None,
            datatype: None,
            parent_triples_map: Some(IriReference(parent_id.to_string())),
            join_conditions: conditions,
        }],
        graph_maps: vec![],
    });
    tm
}

#[test]
fn translate_object_map_with_parent_triples_map_yields_join_plan() {
    let tm = triples_map_with_join(
        "http://example.com/SportMap",
        vec![JoinConditionRef {
            child: "Sport".to_string(),
            parent: "ID".to_string(),
        }],
    );
    let doc = MappingDocument {
        triples_maps: vec![tm, sport_parent_triples_map()],
    };
    let plans = translate(&doc).unwrap();
    assert!(
        plans.iter().any(|p| matches!(
            p,
            LogicalPlan::Projection(proj) if matches!(&*proj.input, LogicalPlan::Join(_))
        )),
        "expected a Projection wrapping a LogicalPlan::Join for the predicateObjectMap with a parentTriplesMap"
    );
}

#[test]
fn translate_dangling_parent_triples_map_returns_mapping_parse_error() {
    // rml:parentTriplesMap references a TriplesMap id that is not present
    // anywhere in the mapping document — malformed-but-parseable input that
    // must surface as a Result::Err, not crash the process. See
    // https://github.com/daghovland/rdf-datalog/issues/363.
    let tm = triples_map_with_join(
        "http://example.com/NoSuchTriplesMap",
        vec![JoinConditionRef {
            child: "Sport".to_string(),
            parent: "ID".to_string(),
        }],
    );
    let doc = MappingDocument {
        triples_maps: vec![tm],
    };
    let err = translate(&doc).expect_err("dangling parentTriplesMap must not panic");
    assert!(
        matches!(err, rml::RmlError::MappingParse(_)),
        "expected RmlError::MappingParse, got {err:?}"
    );
}

fn find_join(plans: &[LogicalPlan]) -> &rml::plan::LogicalJoin {
    plans
        .iter()
        .find_map(|p| match p {
            LogicalPlan::Projection(proj) => match proj.input.as_ref() {
                LogicalPlan::Join(j) => Some(j),
                _ => None,
            },
            _ => None,
        })
        .expect("expected a Projection wrapping a LogicalPlan::Join")
}

#[test]
fn translate_join_condition_maps_child_to_left_parent_to_right() {
    let tm = triples_map_with_join(
        "http://example.com/SportMap",
        vec![JoinConditionRef {
            child: "Sport".to_string(),
            parent: "ID".to_string(),
        }],
    );
    let doc = MappingDocument {
        triples_maps: vec![tm, sport_parent_triples_map()],
    };
    let plans = translate(&doc).unwrap();
    let join = find_join(&plans);
    assert_eq!(join.conditions.len(), 1);
    assert_eq!(join.conditions[0].left_column, "Sport");
    assert_eq!(join.conditions[0].right_column, "ID");
}

#[test]
fn translate_multi_column_join_condition_preserves_all_conditions() {
    let tm = triples_map_with_join(
        "http://example.com/SportMap",
        vec![
            JoinConditionRef {
                child: "Sport".to_string(),
                parent: "ID".to_string(),
            },
            JoinConditionRef {
                child: "Year".to_string(),
                parent: "Year".to_string(),
            },
        ],
    );
    let doc = MappingDocument {
        triples_maps: vec![tm, sport_parent_triples_map()],
    };
    let plans = translate(&doc).unwrap();
    let join = find_join(&plans);
    assert_eq!(join.conditions.len(), 2);
}

// ── choose_join_algorithm (RML_SQL_PLAN.md "Efficient joins", tier 1/2 — see #354) ──

fn sql_source(db_path: &str, table: &str) -> LogicalSourceRef {
    LogicalSourceRef::Sql(SqlSourceRef {
        connection: SqlConnection::Sqlite(db_path.into()),
        query: SqlQuery::Table(table.to_string()),
    })
}

#[test]
fn choose_join_algorithm_same_connection_sql_sql_is_pushdown() {
    let child = sql_source("school.sqlite", "student");
    let parent = sql_source("school.sqlite", "sport");
    assert_eq!(
        choose_join_algorithm(&child, &parent),
        JoinAlgorithm::SqlPushdown
    );
}

#[test]
fn choose_join_algorithm_different_connection_sql_sql_is_hash_join() {
    let child = sql_source("students.sqlite", "student");
    let parent = sql_source("sports.sqlite", "sport");
    assert_eq!(
        choose_join_algorithm(&child, &parent),
        JoinAlgorithm::HashJoin
    );
}

#[test]
fn choose_join_algorithm_sql_csv_is_hash_join() {
    let child = sql_source("school.sqlite", "student");
    let parent = LogicalSourceRef::File(PathBuf::from("sport.csv"));
    assert_eq!(
        choose_join_algorithm(&child, &parent),
        JoinAlgorithm::HashJoin
    );
}

#[test]
fn choose_join_algorithm_csv_csv_is_hash_join() {
    let child = LogicalSourceRef::File(PathBuf::from("student.csv"));
    let parent = LogicalSourceRef::File(PathBuf::from("sport.csv"));
    assert_eq!(
        choose_join_algorithm(&child, &parent),
        JoinAlgorithm::HashJoin
    );
}

fn sql_triples_map(id: &str, db_path: &str, table: &str, subject_template: &str) -> TriplesMap {
    let mut tm = simple_triples_map("unused.csv", subject_template);
    tm.id = IriReference(id.to_string());
    tm.logical_source.source = sql_source(db_path, table);
    tm
}

#[test]
fn translate_same_connection_sql_join_uses_sql_pushdown_algorithm() {
    let mut child = sql_triples_map(
        "http://example.com/TM",
        "school.sqlite",
        "student",
        "http://example.com/student/{ID}",
    );
    child.predicate_object_maps.push(PredicateObjectMap {
        predicate_maps: vec![(
            TermMap::Constant(GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(
                "http://example.com/practises".to_string(),
            )))),
            TermType::Iri,
        )],
        object_maps: vec![ObjectMap {
            term_map: TermMap::Reference(String::new()),
            term_type: TermType::Iri,
            language: None,
            datatype: None,
            parent_triples_map: Some(IriReference("http://example.com/SportMap".to_string())),
            join_conditions: vec![JoinConditionRef {
                child: "Sport".to_string(),
                parent: "ID".to_string(),
            }],
        }],
        graph_maps: vec![],
    });
    let parent = sql_triples_map(
        "http://example.com/SportMap",
        "school.sqlite",
        "sport",
        "http://example.com/sport/{ID}",
    );
    let doc = MappingDocument {
        triples_maps: vec![child, parent],
    };
    let plans = translate(&doc).unwrap();
    let join = find_join(&plans);
    assert_eq!(join.algorithm, JoinAlgorithm::SqlPushdown);
}

#[test]
fn translate_sql_csv_join_stays_hash_join() {
    let mut child = sql_triples_map(
        "http://example.com/TM",
        "school.sqlite",
        "student",
        "http://example.com/student/{ID}",
    );
    child.predicate_object_maps.push(PredicateObjectMap {
        predicate_maps: vec![(
            TermMap::Constant(GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(
                "http://example.com/practises".to_string(),
            )))),
            TermType::Iri,
        )],
        object_maps: vec![ObjectMap {
            term_map: TermMap::Reference(String::new()),
            term_type: TermType::Iri,
            language: None,
            datatype: None,
            parent_triples_map: Some(IriReference("http://example.com/SportMap".to_string())),
            join_conditions: vec![JoinConditionRef {
                child: "Sport".to_string(),
                parent: "ID".to_string(),
            }],
        }],
        graph_maps: vec![],
    });
    // Parent is a CSV source, not SQL on the same connection — must stay on
    // the source-agnostic hash join (RML_JOIN_PLAN.md).
    let parent = sport_parent_triples_map();
    let doc = MappingDocument {
        triples_maps: vec![child, parent],
    };
    let plans = translate(&doc).unwrap();
    let join = find_join(&plans);
    assert_eq!(join.algorithm, JoinAlgorithm::HashJoin);
}

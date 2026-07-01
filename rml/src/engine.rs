use std::collections::HashMap;
use std::path::Path;

use dag_rdf::{Datastore, GraphElementId, RdfLiteral, RdfResource, Triple};
use ingress::{GraphElement, IriReference};

use crate::RmlError;
use crate::ast::{ReferenceFormulation, TermType};
use crate::plan::{
    FormatFunction, GenerationLogic, LogicalJoin, LogicalPlan, LogicalProjection, LogicalScan,
    OutputAttr, TermPattern,
};
use crate::sandbox::confine_path;
use crate::sources::SourceRow;
use crate::sources::csv::CsvSource;
use crate::sources::json::JsonSource;
use crate::sources::xml::XmlSource;
use crate::template::{expand_template, is_valid_iri_scheme};

pub fn execute(
    plans: &[LogicalPlan],
    base_dir: &Path,
    datastore: &mut Datastore,
) -> Result<(), RmlError> {
    for plan in plans {
        execute_plan(plan, base_dir, datastore)?;
    }
    Ok(())
}

fn execute_plan(plan: &LogicalPlan, base_dir: &Path, ds: &mut Datastore) -> Result<(), RmlError> {
    let LogicalPlan::Projection(proj) = plan else {
        return Ok(());
    };
    match proj.input.as_ref() {
        LogicalPlan::Scan(scan) => {
            for row in scan_rows(scan, base_dir)? {
                execute_row(proj, row.as_ref(), ds)?;
            }
        }
        LogicalPlan::Join(join) => execute_join(proj, join, base_dir, ds)?,
        LogicalPlan::Projection(_) => {}
    }
    Ok(())
}

fn scan_rows(scan: &LogicalScan, base_dir: &Path) -> Result<Vec<Box<dyn SourceRow>>, RmlError> {
    let crate::ast::LogicalSourceRef::File(rel_path) = &scan.source;
    let rel_path = std::path::Path::new(rel_path);
    // Reject absolute paths and '..' escapes via the sandbox.
    let canonical_path = confine_path(base_dir, rel_path)?;

    let rows = match scan.reference_formulation {
        ReferenceFormulation::Csv => {
            let source = CsvSource::new(canonical_path.clone());
            source
                .rows()
                .map(|r| r.map(|raw| Box::new(crate::sources::CsvRow(raw)) as Box<dyn SourceRow>))
                .collect::<Result<Vec<_>, _>>()?
        }
        ReferenceFormulation::JsonPath => {
            let mut source = JsonSource::new(canonical_path.clone());
            if let Some(iter) = &scan.iterator {
                source = source.with_iterator(iter.clone());
            }
            source
                .rows()
                .map(|r| r.map(|row| Box::new(row) as Box<dyn SourceRow>))
                .collect::<Result<Vec<_>, _>>()?
        }
        ReferenceFormulation::XPath => {
            let mut source = XmlSource::new(canonical_path.clone());
            if let Some(iter) = &scan.iterator {
                source = source.with_iterator(iter.clone());
            }
            source
                .rows()
                .map(|r| r.map(|row| Box::new(row) as Box<dyn SourceRow>))
                .collect::<Result<Vec<_>, _>>()?
        }
    };
    Ok(rows)
}

/// Hash join: the right (parent) side is materialised and indexed by its
/// join-column values; the left (child) side is streamed and probed against
/// that index. Child and parent rows are kept separate throughout — the
/// join `Object` attribute is evaluated against the matched parent row,
/// every other attribute against the child row — since both sides may use
/// overlapping column names for unrelated values.
fn execute_join(
    proj: &LogicalProjection,
    join: &LogicalJoin,
    base_dir: &Path,
    ds: &mut Datastore,
) -> Result<(), RmlError> {
    let LogicalPlan::Scan(left_scan) = join.left.as_ref() else {
        return Ok(());
    };
    let LogicalPlan::Scan(right_scan) = join.right.as_ref() else {
        return Ok(());
    };

    let parent_rows = scan_rows(right_scan, base_dir)?;
    let mut parent_index: HashMap<Vec<String>, Vec<usize>> = HashMap::new();
    for (idx, row) in parent_rows.iter().enumerate() {
        if let Some(key) = join_key(join, row.as_ref(), JoinSide::Parent) {
            parent_index.entry(key).or_default().push(idx);
        }
    }

    let child_rows = scan_rows(left_scan, base_dir)?;
    for child_row in &child_rows {
        let Some(key) = join_key(join, child_row.as_ref(), JoinSide::Child) else {
            continue;
        };
        let Some(indices) = parent_index.get(&key) else {
            continue;
        };
        for &idx in indices {
            execute_join_row(proj, child_row.as_ref(), parent_rows[idx].as_ref(), ds)?;
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum JoinSide {
    Child,
    Parent,
}

fn join_key(join: &LogicalJoin, row: &dyn SourceRow, side: JoinSide) -> Option<Vec<String>> {
    join.conditions
        .iter()
        .map(|c| {
            let column = match side {
                JoinSide::Child => &c.left_column,
                JoinSide::Parent => &c.right_column,
            };
            row.get_str(column)
        })
        .collect()
}

fn execute_row(
    proj: &LogicalProjection,
    row: &dyn SourceRow,
    ds: &mut Datastore,
) -> Result<(), RmlError> {
    let s = eval_attr(proj, OutputAttr::Subject, row, ds);
    let p = eval_attr(proj, OutputAttr::Predicate, row, ds);
    let o = eval_attr(proj, OutputAttr::Object, row, ds);
    let (Some(s), Some(p), Some(o)) = (s, p, o) else {
        return Ok(());
    };
    let graph_id = eval_attr(proj, OutputAttr::Graph, row, ds);
    emit_triple(s, p, o, graph_id, ds);
    Ok(())
}

fn execute_join_row(
    proj: &LogicalProjection,
    child_row: &dyn SourceRow,
    parent_row: &dyn SourceRow,
    ds: &mut Datastore,
) -> Result<(), RmlError> {
    let s = eval_attr(proj, OutputAttr::Subject, child_row, ds);
    let p = eval_attr(proj, OutputAttr::Predicate, child_row, ds);
    let o = eval_attr(proj, OutputAttr::Object, parent_row, ds);
    let (Some(s), Some(p), Some(o)) = (s, p, o) else {
        return Ok(());
    };
    let graph_id = eval_attr(proj, OutputAttr::Graph, child_row, ds);
    emit_triple(s, p, o, graph_id, ds);
    Ok(())
}

fn emit_triple(
    s: GraphElementId,
    p: GraphElementId,
    o: GraphElementId,
    graph_id: Option<GraphElementId>,
    ds: &mut Datastore,
) {
    match graph_id {
        Some(g) => {
            ds.add_named_graph_triple(
                g,
                Triple {
                    subject: s,
                    predicate: p,
                    obj: o,
                },
            );
        }
        None => {
            ds.add_triple(Triple {
                subject: s,
                predicate: p,
                obj: o,
            });
        }
    }
}

fn eval_attr(
    proj: &LogicalProjection,
    target: OutputAttr,
    row: &dyn SourceRow,
    ds: &mut Datastore,
) -> Option<GraphElementId> {
    let (_, logic) = proj.attrs.iter().find(|(a, _)| *a == target)?;
    eval_logic(logic, row, ds)
}

fn eval_logic(
    logic: &GenerationLogic,
    row: &dyn SourceRow,
    ds: &mut Datastore,
) -> Option<GraphElementId> {
    match logic {
        GenerationLogic::Constant(elem) => Some(ds.add_resource(elem.clone())),
        GenerationLogic::Dynamic(ff) => eval_format_function(ff, row, ds),
    }
}

fn eval_format_function(
    ff: &FormatFunction,
    row: &dyn SourceRow,
    ds: &mut Datastore,
) -> Option<GraphElementId> {
    let encode = matches!(ff.term_type, TermType::Iri);
    let lexical = match &ff.pattern {
        TermPattern::Template(t) => expand_template(t, row, encode)?,
        TermPattern::Reference(key) => {
            let val = row.get_str(key)?;
            if encode {
                crate::template::percent_encode(&val)
            } else {
                val
            }
        }
    };

    let id = match ff.term_type {
        TermType::Iri => {
            if !is_valid_iri_scheme(&lexical) {
                return None;
            }
            let elem = GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(lexical)));
            ds.add_resource(elem)
        }
        TermType::BlankNode => ds.resources.get_or_create_named_anon_resource(lexical),
        TermType::Literal => {
            let lit = match (&ff.language, &ff.datatype) {
                (Some(lang), _) => RdfLiteral::LangLiteral {
                    lang: lang.clone(),
                    literal: lexical,
                },
                (_, Some(dt)) => RdfLiteral::TypedLiteral {
                    type_iri: dt.clone(),
                    literal: lexical,
                },
                _ => RdfLiteral::LiteralString(lexical),
            };
            ds.add_literal_resource(lit)
        }
    };
    Some(id)
}

use std::path::Path;

use dag_rdf::{Datastore, GraphElementId, RdfLiteral, RdfResource, Triple};
use ingress::{GraphElement, IriReference};

use crate::RmlError;
use crate::ast::{ReferenceFormulation, TermType};
use crate::plan::{
    FormatFunction, GenerationLogic, LogicalPlan, LogicalProjection, OutputAttr, TermPattern,
};
use crate::sources::SourceRow;
use crate::sources::csv::CsvSource;
use crate::sources::json::JsonSource;
use crate::template::expand_template;

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
    let LogicalPlan::Scan(scan) = proj.input.as_ref() else {
        return Ok(());
    };

    let crate::ast::LogicalSourceRef::File(rel_path) = &scan.source;
    let path = base_dir.join(rel_path);

    match scan.reference_formulation {
        ReferenceFormulation::Csv => {
            let source = CsvSource::new(path);
            for row_result in source.rows() {
                let raw = row_result?;
                let row = crate::sources::CsvRow(raw);
                execute_row(proj, &row, ds)?;
            }
        }
        ReferenceFormulation::JsonPath => {
            let mut source = JsonSource::new(path);
            if let Some(iter) = &scan.iterator {
                source = source.with_iterator(iter.clone());
            }
            for row_result in source.rows() {
                let row = row_result?;
                execute_row(proj, &row, ds)?;
            }
        }
    }
    Ok(())
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

    match graph_id {
        Some(g) => {
            ds.add_named_graph_triple(g, Triple { subject: s, predicate: p, obj: o });
        }
        None => {
            ds.add_triple(Triple { subject: s, predicate: p, obj: o });
        }
    }
    Ok(())
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
            if encode { crate::template::percent_encode(&val) } else { val }
        }
    };

    let id = match ff.term_type {
        TermType::Iri => {
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

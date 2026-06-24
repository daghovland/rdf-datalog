use ingress::{GraphElement, IriReference, RdfResource};

use crate::plan::{GenerationLogic, LogicalPlan, LogicalProjection, TermPattern};

pub fn constant_fold(plans: Vec<LogicalPlan>) -> Vec<LogicalPlan> {
    plans.into_iter().map(fold_plan).collect()
}

fn fold_plan(plan: LogicalPlan) -> LogicalPlan {
    match plan {
        LogicalPlan::Projection(proj) => LogicalPlan::Projection(fold_projection(proj)),
        other => other,
    }
}

fn fold_projection(proj: LogicalProjection) -> LogicalProjection {
    let attrs = proj
        .attrs
        .into_iter()
        .map(|(attr, logic)| (attr, fold_logic(logic)))
        .collect();
    LogicalProjection {
        input: proj.input,
        attrs,
    }
}

fn fold_logic(logic: GenerationLogic) -> GenerationLogic {
    match logic {
        GenerationLogic::Dynamic(ref ff) => {
            match &ff.pattern {
                TermPattern::Template(t) if !t.contains('{') => {
                    // No placeholder → constant; materialise the GraphElement now
                    let elem = match ff.term_type {
                        crate::ast::TermType::Iri => {
                            GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(t.clone())))
                        }
                        _ => {
                            // For Literal/BlankNode no-placeholder templates keep as-is
                            return logic;
                        }
                    };
                    GenerationLogic::Constant(elem)
                }
                _ => logic,
            }
        }
        other => other,
    }
}

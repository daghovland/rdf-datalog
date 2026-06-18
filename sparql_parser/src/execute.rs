/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! SPARQL query execution against a [`Datastore`].
//!
//! Supports BGP, FILTER (comparison/regex/BOUND), OPTIONAL, UNION, MINUS,
//! DISTINCT, LIMIT, OFFSET.

use crate::ast::{
    BinaryOp, Expression, ProjectionElement, Query, QueryComponent, Term, TriplePattern, UnaryOp,
};
use dag_rdf::{
    Datastore, GraphElement, GraphElementId, RdfLiteral, Term as DagTerm, DEFAULT_GRAPH_ELEMENT_ID,
};
use ingress::{
    IriReference, XSD_BOOLEAN, XSD_DECIMAL, XSD_DOUBLE, XSD_FLOAT, XSD_INTEGER, XSD_STRING,
};
use num_bigint::BigInt;
use std::collections::HashMap;
use std::collections::HashSet;

/// A single bound solution: variable name → concrete graph element.
pub type SolutionRow = HashMap<String, GraphElement>;

/// The result of executing a SPARQL SELECT query.
pub struct SelectResult {
    /// Variable names in projection order.
    pub variables: Vec<String>,
    /// Each row maps projected variable names to their bound value.
    pub rows: Vec<SolutionRow>,
}

/// A single resolved (ground) triple from a CONSTRUCT result.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ResolvedTriple {
    pub subject: GraphElement,
    pub predicate: GraphElement,
    pub object: GraphElement,
}

/// The result of executing a SPARQL query (SELECT, ASK, or CONSTRUCT).
pub enum QueryResult {
    Select(SelectResult),
    Ask(bool),
    Construct(Vec<ResolvedTriple>),
}

/// Execute a parsed SPARQL query against `datastore`.
pub fn execute(query: &Query, datastore: &Datastore) -> Result<QueryResult, String> {
    match query {
        Query::Select {
            projection,
            where_clause,
            limit,
            offset,
            distinct,
            ..
        } => {
            let variables = projection_variables(projection, where_clause, datastore);
            let initial: Vec<PartialSub> = vec![HashMap::new()];
            let solutions = eval_components(
                where_clause,
                initial,
                datastore,
                ActiveGraph::Fixed(DEFAULT_GRAPH_ELEMENT_ID),
            );

            // Project
            let mut rows: Vec<SolutionRow> = solutions
                .iter()
                .map(|sub| project(sub, &variables))
                .collect();

            if *distinct {
                // `dedup()` only removes *consecutive* duplicates; use a proper
                // seen-set so non-adjacent identical rows are also removed.
                // SolutionRow = HashMap<String, GraphElement> — not Hash itself,
                // so we canonicalise each row as a sorted Vec of (var, element) pairs.
                let mut seen: std::collections::HashSet<Vec<(String, GraphElement)>> =
                    std::collections::HashSet::new();
                rows.retain(|row| {
                    let mut key: Vec<(String, GraphElement)> =
                        row.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                    key.sort_by(|a, b| a.0.cmp(&b.0));
                    seen.insert(key)
                });
            }

            if let Some(off) = offset {
                let off = *off as usize;
                if off < rows.len() {
                    rows = rows[off..].to_vec();
                } else {
                    rows.clear();
                }
            }
            if let Some(lim) = limit {
                rows.truncate(*lim as usize);
            }

            Ok(QueryResult::Select(SelectResult { variables, rows }))
        }
        Query::Ask { where_clause } => {
            let initial: Vec<PartialSub> = vec![HashMap::new()];
            let solutions = eval_components(
                where_clause,
                initial,
                datastore,
                ActiveGraph::Fixed(DEFAULT_GRAPH_ELEMENT_ID),
            );
            Ok(QueryResult::Ask(!solutions.is_empty()))
        }
        Query::Construct {
            template,
            where_clause,
        } => {
            let initial: Vec<PartialSub> = vec![HashMap::new()];
            let solutions = eval_components(
                where_clause,
                initial,
                datastore,
                ActiveGraph::Fixed(DEFAULT_GRAPH_ELEMENT_ID),
            );

            let effective_template: Vec<TriplePattern> = if template.is_empty() {
                collect_bgps_from_components(where_clause)
            } else {
                template.clone()
            };

            let mut output: HashSet<ResolvedTriple> = HashSet::new();
            let mut bnode_counter: u32 = 0;

            for sub in &solutions {
                let mut bnode_map: HashMap<u32, u32> = HashMap::new();
                for tp in &effective_template {
                    let s = bind_template_term(
                        &tp.subject,
                        sub,
                        datastore,
                        &mut bnode_map,
                        &mut bnode_counter,
                    );
                    let p = bind_template_term(
                        &tp.predicate,
                        sub,
                        datastore,
                        &mut bnode_map,
                        &mut bnode_counter,
                    );
                    let o = bind_template_term(
                        &tp.object,
                        sub,
                        datastore,
                        &mut bnode_map,
                        &mut bnode_counter,
                    );
                    if let (Some(s), Some(p), Some(o)) = (s, p, o) {
                        let subject_ok = !matches!(s, GraphElement::GraphLiteral(_));
                        let pred_ok =
                            matches!(p, GraphElement::NodeOrEdge(dag_rdf::RdfResource::Iri(_)));
                        if subject_ok && pred_ok {
                            output.insert(ResolvedTriple {
                                subject: s,
                                predicate: p,
                                object: o,
                            });
                        }
                    }
                }
            }

            Ok(QueryResult::Construct(output.into_iter().collect()))
        }
    }
}

// ── Projection ────────────────────────────────────────────────────────────────

fn projection_variables(
    proj: &[ProjectionElement],
    components: &[QueryComponent],
    _datastore: &Datastore,
) -> Vec<String> {
    // If star, collect all variables from the where clause
    if proj.iter().any(|p| matches!(p, ProjectionElement::Star)) {
        let mut vars: Vec<String> = Vec::new();
        collect_vars_from_components(components, &mut vars);
        vars.sort();
        vars.dedup();
        return vars;
    }
    proj.iter()
        .filter_map(|p| match p {
            ProjectionElement::Variable(v) => Some(v.clone()),
            ProjectionElement::Expression(_, alias) => Some(alias.clone()),
            ProjectionElement::Star => None,
        })
        .collect()
}

fn collect_vars_from_components(components: &[QueryComponent], vars: &mut Vec<String>) {
    for comp in components {
        match comp {
            QueryComponent::BGP(tps) => {
                for tp in tps {
                    collect_vars_from_term(&tp.subject, vars);
                    collect_vars_from_term(&tp.predicate, vars);
                    collect_vars_from_term(&tp.object, vars);
                }
            }
            QueryComponent::Optional(inner) | QueryComponent::Minus(inner) => {
                collect_vars_from_components(inner, vars);
            }
            QueryComponent::Union(left, right) => {
                collect_vars_from_components(left, vars);
                collect_vars_from_components(right, vars);
            }
            QueryComponent::Graph(graph_term, inner) => {
                collect_vars_from_term(graph_term, vars);
                collect_vars_from_components(inner, vars);
            }
            QueryComponent::Bind(_, alias) => {
                vars.push(alias.clone());
            }
            QueryComponent::Filter(_) | QueryComponent::Values(_, _) => {}
            QueryComponent::Service(_, inner, _) => {
                collect_vars_from_components(inner, vars);
            }
        }
    }
}

fn collect_vars_from_term(term: &Term, vars: &mut Vec<String>) {
    if let Term::Variable(v) = term {
        if !is_internal_variable(v) {
            vars.push(v.clone());
        }
    }
}

fn is_internal_variable(var: &str) -> bool {
    var.starts_with("__path_")
}

fn project(sub: &PartialSub, variables: &[String]) -> SolutionRow {
    variables
        .iter()
        .filter_map(|v| sub.get(v).map(|el| (v.clone(), el.clone())))
        .collect()
}

// ── Evaluation ────────────────────────────────────────────────────────────────

/// Internal solution mapping: variable → concrete graph element.
///
/// Uses `GraphElement` values directly (not interned IDs) so that computed
/// values from `BIND` expressions can be stored without requiring a mutable
/// reference to the datastore.
type PartialSub = HashMap<String, GraphElement>;

#[derive(Clone)]
enum ActiveGraph {
    Fixed(GraphElementId),
    Variable(String),
}

fn eval_components(
    components: &[QueryComponent],
    solutions: Vec<PartialSub>,
    datastore: &Datastore,
    active_graph: ActiveGraph,
) -> Vec<PartialSub> {
    let mut current = solutions;
    for comp in components {
        current = eval_component(comp, current, datastore, &active_graph);
        if current.is_empty() {
            break;
        }
    }
    current
}

fn eval_component(
    comp: &QueryComponent,
    solutions: Vec<PartialSub>,
    datastore: &Datastore,
    active_graph: &ActiveGraph,
) -> Vec<PartialSub> {
    match comp {
        QueryComponent::BGP(tps) => eval_bgp(tps, solutions, datastore, active_graph),

        QueryComponent::Filter(expr) => solutions
            .into_iter()
            .filter(|sub| eval_filter(expr, sub, datastore, active_graph))
            .collect(),

        QueryComponent::Optional(inner) => {
            let mut result = Vec::new();
            for sub in solutions {
                let extended =
                    eval_components(inner, vec![sub.clone()], datastore, (*active_graph).clone());
                if extended.is_empty() {
                    result.push(sub);
                } else {
                    result.extend(extended);
                }
            }
            result
        }

        QueryComponent::Union(left, right) => {
            let left_sols =
                eval_components(left, solutions.clone(), datastore, (*active_graph).clone());
            let right_sols = eval_components(right, solutions, datastore, (*active_graph).clone());
            let mut result = left_sols;
            result.extend(right_sols);
            result
        }

        QueryComponent::Minus(inner) => solutions
            .into_iter()
            .filter(|sub| {
                let minus_sols =
                    eval_components(inner, vec![sub.clone()], datastore, (*active_graph).clone());
                minus_sols.is_empty() || minus_sols.iter().all(|ms| !compatible(sub, ms))
            })
            .collect(),

        QueryComponent::Graph(graph_term, inner) => solutions
            .into_iter()
            .flat_map(|sub| {
                let scoped_graph = match graph_term {
                    Term::Constant(gel) => {
                        let Some(&graph_id) = datastore.resources.resource_map.get(gel) else {
                            return Vec::new();
                        };
                        ActiveGraph::Fixed(graph_id)
                    }
                    Term::Variable(var) => {
                        if let Some(gel) = sub.get(var) {
                            if let Some(&graph_id) = datastore.resources.resource_map.get(gel) {
                                ActiveGraph::Fixed(graph_id)
                            } else {
                                ActiveGraph::Variable(var.clone())
                            }
                        } else {
                            ActiveGraph::Variable(var.clone())
                        }
                    }
                };
                eval_components(inner, vec![sub], datastore, scoped_graph)
            })
            .collect(),

        QueryComponent::Bind(expr, alias) => solutions
            .into_iter()
            .filter_map(|mut sub| {
                let val = eval_bind_expr(expr, &sub, datastore)?;
                sub.insert(alias.clone(), val);
                Some(sub)
            })
            .collect(),

        QueryComponent::Values(vars, rows) => {
            let mut result = Vec::new();
            for sub in solutions {
                for row in rows {
                    if vars.len() != row.len() {
                        continue;
                    }
                    let mut new_sub = sub.clone();
                    let mut ok = true;
                    for (var, val_opt) in vars.iter().zip(row.iter()) {
                        if let Some(gel) = val_opt {
                            match new_sub.get(var) {
                                Some(existing) if existing != gel => {
                                    ok = false;
                                    break;
                                }
                                _ => {
                                    new_sub.insert(var.clone(), gel.clone());
                                }
                            }
                        } // UNDEF (None) — leave unbound
                    }
                    if ok {
                        result.push(new_sub);
                    }
                }
            }
            result
        }

        QueryComponent::Service(_, inner, _) => {
            // SERVICE not supported; return empty
            let _ = inner;
            Vec::new()
        }
    }
}

/// Two substitutions are compatible if they agree on all shared variables.
fn compatible(a: &PartialSub, b: &PartialSub) -> bool {
    for (var, gel_a) in a {
        if let Some(gel_b) = b.get(var) {
            if gel_a != gel_b {
                return false;
            }
        }
    }
    true
}

// ── BGP evaluation ────────────────────────────────────────────────────────────

fn eval_bgp(
    patterns: &[TriplePattern],
    solutions: Vec<PartialSub>,
    datastore: &Datastore,
    active_graph: &ActiveGraph,
) -> Vec<PartialSub> {
    let mut current = solutions;
    for pattern in patterns {
        current = current
            .into_iter()
            .flat_map(|sub| eval_triple_pattern(pattern, &sub, datastore, active_graph))
            .collect();
        if current.is_empty() {
            break;
        }
    }
    current
}

fn eval_triple_pattern(
    tp: &TriplePattern,
    sub: &PartialSub,
    datastore: &Datastore,
    active_graph: &ActiveGraph,
) -> Vec<PartialSub> {
    // If any constant in the pattern is absent from the store it can never match.
    for term in [&tp.subject, &tp.predicate, &tp.object] {
        if let Term::Constant(gel) = term {
            if !datastore.resources.resource_map.contains_key(gel) {
                return Vec::new();
            }
        }
    }

    let mut new_solutions = Vec::new();

    let g = match active_graph {
        ActiveGraph::Fixed(id) => Some(*id),
        ActiveGraph::Variable(v) => sub
            .get(v)
            .and_then(|gel| datastore.resources.resource_map.get(gel))
            .copied(),
    };
    let s = match ast_term_to_dag_term(&tp.subject, sub, datastore) {
        DagTerm::Resource(id) => Some(id),
        _ => None,
    };
    let p = match ast_term_to_dag_term(&tp.predicate, sub, datastore) {
        DagTerm::Resource(id) => Some(id),
        _ => None,
    };
    let o = match ast_term_to_dag_term(&tp.object, sub, datastore) {
        DagTerm::Resource(id) => Some(id),
        _ => None,
    };

    for quad in datastore.quads_matching(g, s, p, o) {
        let mut new_sub = sub.clone();
        let mut ok = true;

        // Bind a variable to the GraphElement resolved from a quad-field ID.
        macro_rules! bind {
            ($term:expr, $id:expr) => {
                if let Term::Variable(v) = $term {
                    let gel = datastore.resources.get_graph_element($id).clone();
                    match new_sub.get(v) {
                        Some(existing) if existing != &gel => {
                            ok = false;
                        }
                        _ => {
                            new_sub.insert(v.clone(), gel);
                        }
                    }
                }
            };
        }

        bind!(&tp.subject, quad.subject);
        bind!(&tp.predicate, quad.predicate);
        bind!(&tp.object, quad.obj);

        if let ActiveGraph::Variable(graph_var) = active_graph {
            let gel = datastore
                .resources
                .get_graph_element(quad.triple_id)
                .clone();
            match new_sub.get(graph_var) {
                Some(existing) if existing != &gel => {
                    ok = false;
                }
                _ => {
                    new_sub.insert(graph_var.clone(), gel);
                }
            }
        }

        if ok {
            new_solutions.push(new_sub);
        }
    }
    new_solutions
}

fn ast_term_to_dag_term(term: &Term, sub: &PartialSub, datastore: &Datastore) -> DagTerm {
    match term {
        Term::Variable(v) => match sub.get(v) {
            Some(gel) => match datastore.resources.resource_map.get(gel) {
                Some(&id) => DagTerm::Resource(id),
                // Computed value (e.g. BIND arithmetic result) not in the store —
                // treat as an unbound variable so no quads can match it.
                None => DagTerm::Variable(v.clone()),
            },
            None => DagTerm::Variable(v.clone()),
        },
        Term::Constant(gel) => match datastore.resources.resource_map.get(gel) {
            Some(&id) => DagTerm::Resource(id),
            None => DagTerm::Variable(format!("__unknown_{:?}", gel)),
        },
    }
}

// ── FILTER expression evaluation ──────────────────────────────────────────────

/// Evaluate a SPARQL expression as a boolean filter guard.
///
/// `sub` maps variable names to interned [`GraphElementId`]s — the same type as a
/// Datalog `Substitution`.  Returns `false` if the expression is unbound or errors.
///
/// Uses the default graph as the active graph (appropriate for Datalog rules,
/// which do not operate over named-graph scopes).
///
/// This is the bridge used by `datalog::RuleAtom::FilterAtom` to evaluate
/// SPARQL-style expression guards inside Datalog rule bodies.
pub fn eval_expr_as_filter(
    expr: &Expression,
    sub: &HashMap<String, GraphElementId>,
    datastore: &Datastore,
) -> bool {
    // Convert the Datalog ID-based substitution to the GraphElement-based
    // substitution expected by the SPARQL evaluator.
    let gel_sub: PartialSub = sub
        .iter()
        .map(|(var, &id)| {
            (
                var.clone(),
                datastore.resources.get_graph_element(id).clone(),
            )
        })
        .collect();
    eval_expression_bool(
        expr,
        &gel_sub,
        datastore,
        &ActiveGraph::Fixed(DEFAULT_GRAPH_ELEMENT_ID),
    )
    .unwrap_or(false)
}

fn eval_filter(
    expr: &Expression,
    sub: &PartialSub,
    datastore: &Datastore,
    active_graph: &ActiveGraph,
) -> bool {
    eval_expression_bool(expr, sub, datastore, active_graph).unwrap_or(false)
}

/// Evaluate an expression to a concrete GraphElement value.
///
/// Constants in the query (e.g. `"SPARQL"` in `regex(?x, "SPARQL")`) are
/// returned directly — they need not exist in the datastore's resource map.
fn eval_expression_value(
    expr: &Expression,
    sub: &PartialSub,
    datastore: &Datastore,
) -> Option<GraphElement> {
    match expr {
        Expression::Variable(v) => sub.get(v).cloned(),
        Expression::Constant(gel) => Some(gel.clone()),
        Expression::FunctionCall(name, args) => eval_function_value(name, args, sub, datastore),
        Expression::Binary(l, op, r) => eval_arithmetic(l, op, r, sub, datastore),
        Expression::Unary(UnaryOp::Plus, inner) => eval_expression_value(inner, sub, datastore),
        Expression::Unary(UnaryOp::Minus, inner) => {
            arithmetic_negate(eval_expression_value(inner, sub, datastore)?)
        }
        _ => None,
    }
}

/// Negate a numeric literal.
fn arithmetic_negate(el: GraphElement) -> Option<GraphElement> {
    match el {
        GraphElement::GraphLiteral(RdfLiteral::IntegerLiteral(n)) => {
            Some(GraphElement::GraphLiteral(RdfLiteral::IntegerLiteral(-n)))
        }
        GraphElement::GraphLiteral(RdfLiteral::DecimalLiteral(d)) => {
            Some(GraphElement::GraphLiteral(RdfLiteral::DecimalLiteral(-d)))
        }
        GraphElement::GraphLiteral(RdfLiteral::DoubleLiteral(d)) => Some(
            GraphElement::GraphLiteral(RdfLiteral::DoubleLiteral((-d.into_inner()).into())),
        ),
        _ => None,
    }
}

/// Evaluate a binary arithmetic expression (Add/Sub/Mul/Div).
/// Returns `None` if operands are not numeric or op is not arithmetic.
fn eval_arithmetic(
    left: &Expression,
    op: &BinaryOp,
    right: &Expression,
    sub: &PartialSub,
    datastore: &Datastore,
) -> Option<GraphElement> {
    let l = eval_expression_value(left, sub, datastore)?;
    let r = eval_expression_value(right, sub, datastore)?;
    match (&l, &r) {
        (
            GraphElement::GraphLiteral(RdfLiteral::IntegerLiteral(a)),
            GraphElement::GraphLiteral(RdfLiteral::IntegerLiteral(b)),
        ) => {
            let result = match op {
                BinaryOp::Add => a + b,
                BinaryOp::Sub => a - b,
                BinaryOp::Mul => a * b,
                BinaryOp::Div => {
                    if b == &BigInt::from(0) {
                        return None;
                    }
                    a / b
                }
                _ => return None,
            };
            Some(GraphElement::GraphLiteral(RdfLiteral::IntegerLiteral(
                result,
            )))
        }
        _ => {
            // Promote to f64 for mixed / floating-point arithmetic
            let af = literal_to_f64(match &l {
                GraphElement::GraphLiteral(lit) => lit,
                _ => return None,
            })?;
            let bf = literal_to_f64(match &r {
                GraphElement::GraphLiteral(lit) => lit,
                _ => return None,
            })?;
            let result = match op {
                BinaryOp::Add => af + bf,
                BinaryOp::Sub => af - bf,
                BinaryOp::Mul => af * bf,
                BinaryOp::Div => {
                    if bf == 0.0 {
                        return None;
                    }
                    af / bf
                }
                _ => return None,
            };
            Some(GraphElement::GraphLiteral(RdfLiteral::DoubleLiteral(
                result.into(),
            )))
        }
    }
}

fn eval_function_value(
    name: &str,
    args: &[Expression],
    sub: &PartialSub,
    datastore: &Datastore,
) -> Option<GraphElement> {
    let upper = name.to_ascii_uppercase();
    match upper.as_str() {
        "STR" => {
            let el = eval_expression_value(args.first()?, sub, datastore)?;
            let s = graph_element_to_string(&el)?;
            Some(GraphElement::GraphLiteral(RdfLiteral::LiteralString(s)))
        }
        "LANG" => {
            let el = eval_expression_value(args.first()?, sub, datastore)?;
            if let GraphElement::GraphLiteral(RdfLiteral::LangLiteral { lang, .. }) = el {
                Some(GraphElement::GraphLiteral(RdfLiteral::LiteralString(lang)))
            } else {
                Some(GraphElement::GraphLiteral(RdfLiteral::LiteralString(
                    String::new(),
                )))
            }
        }
        "STRLEN" => {
            let el = eval_expression_value(args.first()?, sub, datastore)?;
            let s = match &el {
                GraphElement::GraphLiteral(RdfLiteral::LiteralString(s)) => s.clone(),
                GraphElement::GraphLiteral(RdfLiteral::LangLiteral { literal, .. }) => {
                    literal.clone()
                }
                GraphElement::GraphLiteral(RdfLiteral::TypedLiteral { literal, .. }) => {
                    literal.clone()
                }
                _ => return None,
            };
            let len = s.chars().count();
            Some(GraphElement::GraphLiteral(RdfLiteral::TypedLiteral {
                type_iri: IriReference(XSD_INTEGER.to_string()),
                literal: len.to_string(),
            }))
        }
        "DATATYPE" => {
            let el = eval_expression_value(args.first()?, sub, datastore)?;
            let dt_iri = match &el {
                GraphElement::GraphLiteral(RdfLiteral::TypedLiteral { type_iri, .. }) => {
                    type_iri.0.clone()
                }
                GraphElement::GraphLiteral(RdfLiteral::LiteralString(_)) => XSD_STRING.to_string(),
                GraphElement::GraphLiteral(RdfLiteral::LangLiteral { .. }) => {
                    "http://www.w3.org/1999/02/22-rdf-syntax-ns#langString".to_string()
                }
                GraphElement::GraphLiteral(RdfLiteral::BooleanLiteral(_)) => {
                    XSD_BOOLEAN.to_string()
                }
                GraphElement::GraphLiteral(RdfLiteral::IntegerLiteral(_)) => {
                    XSD_INTEGER.to_string()
                }
                GraphElement::GraphLiteral(RdfLiteral::DecimalLiteral(_)) => {
                    XSD_DECIMAL.to_string()
                }
                GraphElement::GraphLiteral(RdfLiteral::FloatLiteral(_)) => XSD_FLOAT.to_string(),
                GraphElement::GraphLiteral(RdfLiteral::DoubleLiteral(_)) => XSD_DOUBLE.to_string(),
                _ => return None,
            };
            Some(GraphElement::NodeOrEdge(dag_rdf::RdfResource::Iri(
                IriReference(dt_iri),
            )))
        }
        _ => None,
    }
}

fn eval_expression_bool(
    expr: &Expression,
    sub: &PartialSub,
    datastore: &Datastore,
    active_graph: &ActiveGraph,
) -> Option<bool> {
    match expr {
        Expression::Binary(left, op, right) => match op {
            BinaryOp::And => {
                let l = eval_expression_bool(left, sub, datastore, active_graph).unwrap_or(false);
                let r = eval_expression_bool(right, sub, datastore, active_graph).unwrap_or(false);
                Some(l && r)
            }
            BinaryOp::Or => {
                let l = eval_expression_bool(left, sub, datastore, active_graph).unwrap_or(false);
                let r = eval_expression_bool(right, sub, datastore, active_graph).unwrap_or(false);
                Some(l || r)
            }
            BinaryOp::Eq => {
                let l = eval_expression_value(left, sub, datastore)?;
                let r = eval_expression_value(right, sub, datastore)?;
                Some(l == r)
            }
            BinaryOp::Ne => {
                let l = eval_expression_value(left, sub, datastore)?;
                let r = eval_expression_value(right, sub, datastore)?;
                Some(l != r)
            }
            BinaryOp::Lt | BinaryOp::Gt | BinaryOp::Le | BinaryOp::Ge => {
                let l = eval_expression_value(left, sub, datastore)?;
                let r = eval_expression_value(right, sub, datastore)?;
                let ord = compare_graph_elements(&l, &r)?;
                Some(match op {
                    BinaryOp::Lt => ord < 0,
                    BinaryOp::Gt => ord > 0,
                    BinaryOp::Le => ord <= 0,
                    BinaryOp::Ge => ord >= 0,
                    _ => unreachable!(),
                })
            }
            _ => None,
        },
        Expression::Unary(UnaryOp::Not, inner) => {
            Some(!eval_expression_bool(inner, sub, datastore, active_graph).unwrap_or(false))
        }
        Expression::FunctionCall(name, args) => eval_function_bool(name, args, sub, datastore),
        Expression::Exists(inner) => {
            let sols =
                eval_components(inner, vec![sub.clone()], datastore, (*active_graph).clone());
            Some(!sols.is_empty())
        }
        Expression::NotExists(inner) => {
            let sols =
                eval_components(inner, vec![sub.clone()], datastore, (*active_graph).clone());
            Some(sols.is_empty())
        }
        _ => {
            let el = eval_expression_value(expr, sub, datastore)?;
            match el {
                GraphElement::GraphLiteral(RdfLiteral::BooleanLiteral(b)) => Some(b),
                GraphElement::GraphLiteral(RdfLiteral::TypedLiteral {
                    ref type_iri,
                    ref literal,
                }) if type_iri.0 == XSD_BOOLEAN => Some(literal == "true"),
                _ => None,
            }
        }
    }
}

fn eval_function_bool(
    name: &str,
    args: &[Expression],
    sub: &PartialSub,
    datastore: &Datastore,
) -> Option<bool> {
    let upper = name.to_ascii_uppercase();
    match upper.as_str() {
        "BOUND" => {
            if let Some(Expression::Variable(v)) = args.first() {
                Some(sub.contains_key(v))
            } else {
                None
            }
        }
        "REGEX" => {
            let text_el = eval_expression_value(args.first()?, sub, datastore)?;
            let text = graph_element_to_string(&text_el)?;

            let pat_el = eval_expression_value(args.get(1)?, sub, datastore)?;
            let pattern = graph_element_to_string(&pat_el)?;

            // Flags (optional 3rd arg)
            let flags = if let Some(flag_expr) = args.get(2) {
                let fel = eval_expression_value(flag_expr, sub, datastore)?;
                graph_element_to_string(&fel).unwrap_or_default()
            } else {
                String::new()
            };

            let case_insensitive = flags.contains('i');
            let matches = if case_insensitive {
                text.to_lowercase().contains(&pattern.to_lowercase())
            } else {
                text.contains(pattern.as_str())
            };
            Some(matches)
        }
        "LANGMATCHES" => {
            let lang_el = eval_expression_value(args.first()?, sub, datastore)?;
            let lang = graph_element_to_string(&lang_el)?.to_lowercase();

            let range_el = eval_expression_value(args.get(1)?, sub, datastore)?;
            let range = graph_element_to_string(&range_el)?.to_lowercase();

            Some(if range == "*" {
                !lang.is_empty()
            } else {
                lang == range || lang.starts_with(&format!("{}-", range))
            })
        }
        "ISIRI" | "ISURI" => {
            let el = eval_expression_value(args.first()?, sub, datastore)?;
            Some(matches!(
                el,
                dag_rdf::GraphElement::NodeOrEdge(dag_rdf::RdfResource::Iri(_))
            ))
        }
        "ISBLANK" => {
            let el = eval_expression_value(args.first()?, sub, datastore)?;
            Some(matches!(
                el,
                dag_rdf::GraphElement::NodeOrEdge(dag_rdf::RdfResource::AnonymousBlankNode(_))
            ))
        }
        "ISLITERAL" => {
            let el = eval_expression_value(args.first()?, sub, datastore)?;
            Some(matches!(el, dag_rdf::GraphElement::GraphLiteral(_)))
        }
        _ => None,
    }
}

/// Evaluate an expression for use in `BIND`, returning its `GraphElement` value.
/// Supports variables, constants, arithmetic, and function calls.
fn eval_bind_expr(
    expr: &Expression,
    sub: &PartialSub,
    datastore: &Datastore,
) -> Option<GraphElement> {
    eval_expression_value(expr, sub, datastore)
}

fn graph_element_to_string(el: &GraphElement) -> Option<String> {
    match el {
        GraphElement::GraphLiteral(RdfLiteral::LiteralString(s)) => Some(s.clone()),
        GraphElement::GraphLiteral(RdfLiteral::LangLiteral { literal, .. }) => {
            Some(literal.clone())
        }
        GraphElement::GraphLiteral(RdfLiteral::TypedLiteral { literal, .. }) => {
            Some(literal.clone())
        }
        GraphElement::NodeOrEdge(dag_rdf::RdfResource::Iri(iri)) => Some(iri.0.clone()),
        _ => None,
    }
}

/// Extract a numeric f64 from a literal if it has a numeric datatype.
fn literal_to_f64(lit: &RdfLiteral) -> Option<f64> {
    match lit {
        RdfLiteral::IntegerLiteral(i) => i.to_string().parse().ok(),
        RdfLiteral::DoubleLiteral(d) => Some(d.into_inner()),
        RdfLiteral::DecimalLiteral(d) => Some(d.to_string().parse().ok()?),
        RdfLiteral::FloatLiteral(f) => Some(f.into_inner()),
        RdfLiteral::TypedLiteral { type_iri, literal } => {
            let iri = &type_iri.0;
            if iri == XSD_INTEGER || iri == XSD_DECIMAL || iri == XSD_DOUBLE || iri == XSD_FLOAT {
                literal.parse().ok()
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Compare graph elements for FILTER relational operators.
/// Returns negative, 0, positive, or None if not comparable.
fn compare_graph_elements(a: &GraphElement, b: &GraphElement) -> Option<i32> {
    use dag_rdf::GraphElement::GraphLiteral;
    use std::cmp::Ordering::*;
    if let (GraphLiteral(a_lit), GraphLiteral(b_lit)) = (a, b) {
        // Try numeric comparison first
        if let (Some(af), Some(bf)) = (literal_to_f64(a_lit), literal_to_f64(b_lit)) {
            return af.partial_cmp(&bf).map(|o| match o {
                Less => -1,
                Equal => 0,
                Greater => 1,
            });
        }
        // String literal comparison
        let a_str = match a_lit {
            RdfLiteral::LiteralString(s) => Some(s.as_str()),
            RdfLiteral::TypedLiteral { literal, .. } => Some(literal.as_str()),
            _ => None,
        };
        let b_str = match b_lit {
            RdfLiteral::LiteralString(s) => Some(s.as_str()),
            RdfLiteral::TypedLiteral { literal, .. } => Some(literal.as_str()),
            _ => None,
        };
        if let (Some(a_s), Some(b_s)) = (a_str, b_str) {
            return Some(match a_s.cmp(b_s) {
                Less => -1,
                Equal => 0,
                Greater => 1,
            });
        }
    }
    None
}

// ── CONSTRUCT helpers ─────────────────────────────────────────────────────────

/// Collect all BGP triple patterns from a component list (for CONSTRUCT WHERE short form).
fn collect_bgps_from_components(components: &[QueryComponent]) -> Vec<TriplePattern> {
    let mut out = Vec::new();
    for comp in components {
        match comp {
            QueryComponent::BGP(tps) => out.extend(tps.clone()),
            QueryComponent::Optional(inner) => {
                out.extend(collect_bgps_from_components(inner));
            }
            QueryComponent::Union(left, right) => {
                out.extend(collect_bgps_from_components(left));
                out.extend(collect_bgps_from_components(right));
            }
            QueryComponent::Graph(_, inner) => {
                out.extend(collect_bgps_from_components(inner));
            }
            _ => {}
        }
    }
    out
}

/// Resolve a template term to a concrete `GraphElement`, remapping blank nodes per solution.
///
/// Returns `None` if the term is an unbound variable (triple is silently skipped).
fn bind_template_term(
    term: &Term,
    sub: &PartialSub,
    _datastore: &Datastore,
    bnode_map: &mut HashMap<u32, u32>,
    bnode_counter: &mut u32,
) -> Option<GraphElement> {
    match term {
        Term::Variable(v) => sub.get(v).cloned(),
        Term::Constant(gel) => {
            if let GraphElement::NodeOrEdge(dag_rdf::RdfResource::AnonymousBlankNode(orig_id)) = gel
            {
                // Each solution gets a fresh blank node for each distinct label.
                let fresh_id = bnode_map.entry(*orig_id).or_insert_with(|| {
                    let id = *bnode_counter;
                    *bnode_counter += 1;
                    id
                });
                Some(GraphElement::NodeOrEdge(
                    dag_rdf::RdfResource::AnonymousBlankNode(*fresh_id),
                ))
            } else {
                Some(gel.clone())
            }
        }
    }
}

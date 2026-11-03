/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Minimal `QueryComponent` -> SPARQL text serializer, for forwarding a
//! `SERVICE <endpoint> { ... }` clause's body to a remote endpoint (issue
//! [#743](https://github.com/daghovland/rdf-datalog/issues/743), part of
//! epic [#741](https://github.com/daghovland/rdf-datalog/issues/741) — see
//! `docs/plans/SPARQL_FEDERATION_PLAN.md`).
//!
//! The only entry point, [`service_body_to_sparql_text`], projects a
//! `SERVICE` body as a plain `SELECT * WHERE { ... }` — sufficient for
//! `sparql_parser::parse_query` (or any standards-compliant remote endpoint)
//! to parse back. **Round-tripping, not textual fidelity, is the bar**: the
//! output need not resemble the original query's formatting, only be *some*
//! valid SPARQL text with the same meaning. A future bind-join/VALUES-pushdown
//! mode (injecting already-bound outer variables as a `VALUES` clause) is a
//! separate, deliberately out-of-scope follow-up per the plan doc.
//!
//! Every [`crate::ast::QueryComponent`], [`crate::ast::Expression`], and
//! [`crate::ast::PropertyPath`] variant is handled, rather than guessing at
//! the subset actually reachable inside a `SERVICE` body today — an
//! unhandled variant should never silently produce wrong/incomplete text.
//! Triple-pattern and term leaves reuse [`crate::explain::render_triple_pattern`]/
//! [`crate::explain::render_term`] (which this issue also fixed to render
//! `GraphElement` literals/blank nodes as parseable syntax, not just a debug
//! label — see that module's `render_graph_element` doc comment); there was
//! no existing `Expression`-to-text or `PropertyPath`-to-text renderer
//! anywhere in the crate, so both are written fresh here.
//!
//! Operator/path precedence is handled by parenthesizing liberally rather
//! than tracking precedence levels: every `Expression::Binary`/`Unary`
//! operand and every non-atomic `PropertyPath` operand is wrapped in `( )`.
//! This is always valid SPARQL syntax and sidesteps the need to reproduce
//! the grammar's precedence table exactly — the bar is a parseable,
//! meaning-preserving rendering, not a minimal one.

use crate::ast::{
    Aggregate, BinaryOp, DatasetClause, Expression, OrderCondition, ProjectionElement,
    PropertyPath, Query, QueryComponent, UnaryOp,
};
use crate::explain::{render_graph_element, render_term, render_triple_pattern};

/// Render a `SERVICE` clause's inner `WHERE`-body components as a complete,
/// parseable SPARQL query: `SELECT * WHERE { ... }`.
pub fn service_body_to_sparql_text(components: &[QueryComponent]) -> String {
    format!("SELECT * WHERE {{ {} }}", render_component_list(components))
}

/// Render a full `Query` (used for `QueryComponent::Subquery`'s nested
/// `{ SELECT ... }`, and reachable transitively from any `EXISTS`/
/// `NOT EXISTS` or component list that embeds one).
fn render_query(query: &Query) -> String {
    match query {
        Query::Select {
            projection,
            dataset,
            where_clause,
            group_by,
            having,
            order_by,
            limit,
            offset,
            distinct,
        } => {
            let mut s = String::from("SELECT ");
            if *distinct {
                s.push_str("DISTINCT ");
            }
            s.push_str(&render_projection(projection));
            s.push(' ');
            s.push_str(&render_dataset(dataset));
            s.push_str("WHERE { ");
            s.push_str(&render_component_list(where_clause));
            s.push_str(" }");
            if !group_by.is_empty() {
                s.push_str(" GROUP BY ");
                s.push_str(
                    &group_by
                        .iter()
                        .map(|gc| match &gc.alias {
                            Some(alias) => {
                                format!("({} AS ?{})", render_expression(&gc.expr), alias)
                            }
                            None => match &gc.expr {
                                Expression::Variable(v) => format!("?{v}"),
                                other => format!("({})", render_expression(other)),
                            },
                        })
                        .collect::<Vec<_>>()
                        .join(" "),
                );
            }
            if !having.is_empty() {
                s.push_str(" HAVING ");
                s.push_str(
                    &having
                        .iter()
                        .map(|e| format!("({})", render_expression(e)))
                        .collect::<Vec<_>>()
                        .join(" "),
                );
            }
            if !order_by.is_empty() {
                s.push_str(" ORDER BY ");
                s.push_str(
                    &order_by
                        .iter()
                        .map(render_order_condition)
                        .collect::<Vec<_>>()
                        .join(" "),
                );
            }
            if let Some(limit) = limit {
                s.push_str(&format!(" LIMIT {limit}"));
            }
            if let Some(offset) = offset {
                s.push_str(&format!(" OFFSET {offset}"));
            }
            s
        }
        Query::Ask {
            dataset,
            where_clause,
        } => format!(
            "ASK {}WHERE {{ {} }}",
            render_dataset(dataset),
            render_component_list(where_clause)
        ),
        Query::Construct {
            template,
            dataset,
            where_clause,
        } => {
            let tmpl = template
                .iter()
                .map(|tp| format!("{} .", render_triple_pattern(tp)))
                .collect::<Vec<_>>()
                .join(" ");
            format!(
                "CONSTRUCT {{ {tmpl} }} {}WHERE {{ {} }}",
                render_dataset(dataset),
                render_component_list(where_clause)
            )
        }
        Query::Describe {
            resources,
            dataset,
            where_clause,
        } => {
            let res = if resources.is_empty() {
                "*".to_string()
            } else {
                resources
                    .iter()
                    .map(render_term)
                    .collect::<Vec<_>>()
                    .join(" ")
            };
            let where_part = if where_clause.is_empty() {
                String::new()
            } else {
                format!("WHERE {{ {} }}", render_component_list(where_clause))
            };
            format!("DESCRIBE {res} {}{}", render_dataset(dataset), where_part)
        }
    }
}

fn render_projection(projection: &[ProjectionElement]) -> String {
    if projection
        .iter()
        .any(|p| matches!(p, ProjectionElement::Star))
    {
        return "*".to_string();
    }
    projection
        .iter()
        .map(|p| match p {
            ProjectionElement::Star => "*".to_string(),
            ProjectionElement::Variable(v) => format!("?{v}"),
            ProjectionElement::Expression(expr, alias) => {
                format!("({} AS ?{alias})", render_expression(expr))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Trailing-space-terminated `FROM`/`FROM NAMED` clause list, or empty.
fn render_dataset(dataset: &[DatasetClause]) -> String {
    if dataset.is_empty() {
        return String::new();
    }
    let parts: Vec<String> = dataset
        .iter()
        .map(|d| match d {
            DatasetClause::Default(g) => format!("FROM {}", render_graph_element(g)),
            DatasetClause::Named(g) => format!("FROM NAMED {}", render_graph_element(g)),
        })
        .collect();
    format!("{} ", parts.join(" "))
}

fn render_order_condition(oc: &OrderCondition) -> String {
    if oc.ascending {
        format!("ASC({})", render_expression(&oc.expression))
    } else {
        format!("DESC({})", render_expression(&oc.expression))
    }
}

fn render_component_list(components: &[QueryComponent]) -> String {
    components
        .iter()
        .map(render_query_component)
        .collect::<Vec<_>>()
        .join(" ")
}

fn render_query_component(component: &QueryComponent) -> String {
    match component {
        QueryComponent::BGP(patterns) => patterns
            .iter()
            .map(|tp| format!("{} .", render_triple_pattern(tp)))
            .collect::<Vec<_>>()
            .join(" "),
        QueryComponent::PathPattern(subject, path, object) => format!(
            "{} {} {} .",
            render_term(subject),
            render_property_path(path),
            render_term(object)
        ),
        QueryComponent::Subquery(inner) => format!("{{ {} }}", render_query(inner)),
        QueryComponent::Optional(inner) => {
            format!("OPTIONAL {{ {} }}", render_component_list(inner))
        }
        QueryComponent::Union(left, right) => format!(
            "{{ {} }} UNION {{ {} }}",
            render_component_list(left),
            render_component_list(right)
        ),
        QueryComponent::Filter(expr) => format!("FILTER ({})", render_expression(expr)),
        QueryComponent::Bind(expr, alias) => {
            format!("BIND (({}) AS ?{alias})", render_expression(expr))
        }
        QueryComponent::Values(vars, rows) => render_values(vars, rows),
        QueryComponent::Minus(inner) => format!("MINUS {{ {} }}", render_component_list(inner)),
        QueryComponent::Graph(graph_term, inner) => format!(
            "GRAPH {} {{ {} }}",
            render_term(graph_term),
            render_component_list(inner)
        ),
        QueryComponent::Service(endpoint, inner, silent) => format!(
            "SERVICE {}{} {{ {} }}",
            if *silent { "SILENT " } else { "" },
            render_term(endpoint),
            render_component_list(inner)
        ),
        QueryComponent::Group(inner) => format!("{{ {} }}", render_component_list(inner)),
    }
}

/// `VALUES (?v1 ?v2) { (t11 t12) (t21 t22) ... }` per SPARQL 1.1 §18.2.2.7,
/// using `UNDEF` for `None` entries. Always uses the general parenthesized/
/// tuple form (valid for any variable count, including one) rather than the
/// unparenthesized single-variable short form.
fn render_values(vars: &[String], rows: &[Vec<Option<dag_rdf::GraphElement>>]) -> String {
    let varlist = vars
        .iter()
        .map(|v| format!("?{v}"))
        .collect::<Vec<_>>()
        .join(" ");
    let rows_str = rows
        .iter()
        .map(|row| {
            let vals = row
                .iter()
                .map(|v| match v {
                    Some(gel) => render_graph_element(gel),
                    None => "UNDEF".to_string(),
                })
                .collect::<Vec<_>>()
                .join(" ");
            format!("({vals})")
        })
        .collect::<Vec<_>>()
        .join(" ");
    format!("VALUES ({varlist}) {{ {rows_str} }}")
}

fn render_expression(expr: &Expression) -> String {
    match expr {
        Expression::Variable(v) => format!("?{v}"),
        Expression::Constant(gel) => render_graph_element(gel),
        Expression::Binary(left, op, right) => format!(
            "({} {} {})",
            render_expression(left),
            render_binary_op(op),
            render_expression(right)
        ),
        Expression::Unary(op, inner) => match op {
            UnaryOp::Not => format!("(!{})", render_expression(inner)),
            UnaryOp::Plus => format!("(+{})", render_expression(inner)),
            UnaryOp::Minus => format!("(-{})", render_expression(inner)),
        },
        Expression::FunctionCall(name, args) => format!(
            "{}({})",
            render_function_name(name),
            args.iter()
                .map(render_expression)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Expression::Aggregate(agg) => render_aggregate(agg),
        Expression::Exists(inner) => format!("EXISTS {{ {} }}", render_component_list(inner)),
        Expression::NotExists(inner) => {
            format!("NOT EXISTS {{ {} }}", render_component_list(inner))
        }
        Expression::In(expr, list) => format!(
            "({} IN ({}))",
            render_expression(expr),
            list.iter()
                .map(render_expression)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Expression::NotIn(expr, list) => format!(
            "({} NOT IN ({}))",
            render_expression(expr),
            list.iter()
                .map(render_expression)
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn render_binary_op(op: &BinaryOp) -> &'static str {
    match op {
        BinaryOp::Add => "+",
        BinaryOp::Sub => "-",
        BinaryOp::Mul => "*",
        BinaryOp::Div => "/",
        BinaryOp::Eq => "=",
        BinaryOp::Ne => "!=",
        BinaryOp::Lt => "<",
        BinaryOp::Gt => ">",
        BinaryOp::Le => "<=",
        BinaryOp::Ge => ">=",
        BinaryOp::And => "&&",
        BinaryOp::Or => "||",
    }
}

/// A `FunctionCall`'s `name` is stored (by `parse_function_call`) as a bare
/// builtin keyword *or* as a full IRI string with no `<>`/prefix markers
/// left to tell them apart — `Expression::FunctionCall(String, Vec<Expression>)`
/// doesn't distinguish the two at the type level. Heuristic: a name
/// containing `://` is treated as an IRI extension function and
/// re-wrapped in `<>`; anything else is emitted as a bare builtin-call
/// keyword. This covers every name `parse_function_call` can actually
/// produce (builtin keywords never contain `://`; IRI/prefixed-name
/// extension functions always expand to one).
fn render_function_name(name: &str) -> String {
    if name.contains("://") {
        format!("<{name}>")
    } else {
        name.to_string()
    }
}

fn render_aggregate(agg: &Aggregate) -> String {
    match agg {
        Aggregate::CountStar => "COUNT(*)".to_string(),
        Aggregate::Count(expr, distinct) => render_agg_call("COUNT", expr, *distinct),
        Aggregate::Sum(expr, distinct) => render_agg_call("SUM", expr, *distinct),
        Aggregate::Avg(expr, distinct) => render_agg_call("AVG", expr, *distinct),
        Aggregate::Min(expr, distinct) => render_agg_call("MIN", expr, *distinct),
        Aggregate::Max(expr, distinct) => render_agg_call("MAX", expr, *distinct),
        Aggregate::Sample(expr, distinct) => render_agg_call("SAMPLE", expr, *distinct),
        Aggregate::GroupConcat(expr, sep, distinct) => format!(
            "GROUP_CONCAT({}{}; SEPARATOR=\"{}\")",
            if *distinct { "DISTINCT " } else { "" },
            render_expression(expr),
            sep.replace('\\', "\\\\").replace('"', "\\\"")
        ),
    }
}

fn render_agg_call(name: &str, expr: &Expression, distinct: bool) -> String {
    format!(
        "{name}({}{})",
        if distinct { "DISTINCT " } else { "" },
        render_expression(expr)
    )
}

/// Render a non-atomic `PropertyPath` operand, parenthesizing unless it is
/// an `Iri`/`NegatedSet` leaf (which is already unambiguous on its own).
fn render_path_operand(path: &PropertyPath) -> String {
    match path {
        PropertyPath::Iri(_) | PropertyPath::NegatedSet(_) => render_property_path(path),
        _ => format!("({})", render_property_path(path)),
    }
}

fn render_property_path(path: &PropertyPath) -> String {
    match path {
        PropertyPath::Iri(gel) => render_graph_element(gel),
        PropertyPath::Sequence(parts) => parts
            .iter()
            .map(render_path_operand)
            .collect::<Vec<_>>()
            .join("/"),
        PropertyPath::Alternative(left, right) => {
            format!(
                "{}|{}",
                render_path_operand(left),
                render_path_operand(right)
            )
        }
        PropertyPath::Inverse(inner) => format!("^{}", render_path_operand(inner)),
        PropertyPath::ZeroOrMore(inner) => format!("{}*", render_path_operand(inner)),
        PropertyPath::OneOrMore(inner) => format!("{}+", render_path_operand(inner)),
        PropertyPath::ZeroOrOne(inner) => format!("{}?", render_path_operand(inner)),
        PropertyPath::Repeat(inner, min, max) => {
            format!(
                "{}{}",
                render_path_operand(inner),
                render_repeat_bounds(*min, *max)
            )
        }
        PropertyPath::NegatedSet(elems) => format!(
            "!({})",
            elems
                .iter()
                .map(render_graph_element)
                .collect::<Vec<_>>()
                .join("|")
        ),
    }
}

fn render_repeat_bounds(min: usize, max: Option<usize>) -> String {
    match max {
        Some(max) if max == min => format!("{{{min}}}"),
        Some(max) => format!("{{{min},{max}}}"),
        None => format!("{{{min},}}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{Term, TriplePattern};
    use crate::ParserContext;
    use dag_rdf::{GraphElement, IriReference, RdfResource};

    fn iri_node(iri: &str) -> GraphElement {
        GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(iri.to_string())))
    }

    fn var(name: &str) -> Term {
        Term::Variable(name.to_string())
    }

    fn iri_const(iri: &str) -> Term {
        Term::Constant(iri_node(iri))
    }

    fn tp(s: Term, p: Term, o: Term) -> TriplePattern {
        TriplePattern {
            subject: s,
            predicate: p,
            object: o,
        }
    }

    const EX: &str = "http://example.org/";

    fn parses(text: &str) {
        let mut ctx = ParserContext {
            prefixes: std::collections::HashMap::new(),
            base: None,
        };
        match crate::parse_query(text, &mut ctx) {
            Ok((rest, _)) => assert!(
                rest.trim().is_empty(),
                "did not fully parse {text:?}: leftover {rest:?}"
            ),
            Err(e) => panic!("failed to parse {text:?}: {e:?}"),
        }
    }

    // 1. BGP with a couple of triple patterns, including a triple-term pattern.
    #[test]
    fn bgp_with_triple_term_pattern() {
        let inner = tp(
            iri_const(&format!("{EX}s")),
            iri_const(&format!("{EX}p")),
            iri_const(&format!("{EX}o")),
        );
        let components = vec![QueryComponent::BGP(vec![
            tp(var("x"), iri_const(&format!("{EX}p1")), var("y")),
            tp(
                Term::TripleTerm(Box::new(inner)),
                iri_const(&format!("{EX}p2")),
                var("z"),
            ),
        ])];
        let text = service_body_to_sparql_text(&components);
        assert!(text.starts_with("SELECT * WHERE {"));
        assert!(text.contains("<<("));
        parses(&text);
    }

    // 2. FILTER with a binary comparison and a function call.
    #[test]
    fn filter_binary_and_function_call() {
        let components = vec![
            QueryComponent::BGP(vec![tp(
                var("x"),
                iri_const(&format!("{EX}age")),
                var("age"),
            )]),
            QueryComponent::Filter(Expression::Binary(
                Box::new(Expression::Variable("age".to_string())),
                BinaryOp::Gt,
                Box::new(Expression::Constant(GraphElement::GraphLiteral(
                    ingress::RdfLiteral::IntegerLiteral(18.into()),
                ))),
            )),
            QueryComponent::Filter(Expression::FunctionCall(
                "BOUND".to_string(),
                vec![Expression::Variable("x".to_string())],
            )),
        ];
        let text = service_body_to_sparql_text(&components);
        assert!(text.contains("FILTER"));
        assert!(text.contains("BOUND(?x)"));
        parses(&text);
    }

    // 3. OPTIONAL and UNION, nested.
    #[test]
    fn optional_and_nested_union() {
        let components = vec![
            QueryComponent::BGP(vec![tp(var("x"), iri_const(&format!("{EX}p")), var("y"))]),
            QueryComponent::Optional(vec![QueryComponent::Union(
                vec![QueryComponent::BGP(vec![tp(
                    var("y"),
                    iri_const(&format!("{EX}q1")),
                    var("z"),
                )])],
                vec![QueryComponent::BGP(vec![tp(
                    var("y"),
                    iri_const(&format!("{EX}q2")),
                    var("z"),
                )])],
            )]),
        ];
        let text = service_body_to_sparql_text(&components);
        assert!(text.contains("OPTIONAL"));
        assert!(text.contains("UNION"));
        parses(&text);
    }

    // 4. Property path: sequence, alternative, inverse, and a repetition variant.
    #[test]
    fn property_paths_all_shapes() {
        let p1 = PropertyPath::Iri(iri_node(&format!("{EX}p1")));
        let p2 = PropertyPath::Iri(iri_node(&format!("{EX}p2")));
        let sequence = PropertyPath::Sequence(vec![p1.clone(), p2.clone()]);
        let alternative = PropertyPath::Alternative(Box::new(p1.clone()), Box::new(p2.clone()));
        let inverse = PropertyPath::Inverse(Box::new(p1.clone()));
        let repeat = PropertyPath::Repeat(Box::new(p2.clone()), 2, Some(4));
        let zero_or_more = PropertyPath::ZeroOrMore(Box::new(p1));
        let one_or_more = PropertyPath::OneOrMore(Box::new(p2));

        for (name, path) in [
            ("sequence", sequence),
            ("alternative", alternative),
            ("inverse", inverse),
            ("repeat", repeat),
            ("zero_or_more", zero_or_more),
            ("one_or_more", one_or_more),
        ] {
            let components = vec![QueryComponent::PathPattern(
                var("x"),
                Box::new(path),
                var("y"),
            )];
            let text = service_body_to_sparql_text(&components);
            parses(&text);
            assert!(!name.is_empty());
        }
    }

    // 5. BIND.
    #[test]
    fn bind_expression() {
        let components = vec![
            QueryComponent::BGP(vec![tp(var("x"), iri_const(&format!("{EX}p")), var("y"))]),
            QueryComponent::Bind(
                Expression::Binary(
                    Box::new(Expression::Variable("y".to_string())),
                    BinaryOp::Add,
                    Box::new(Expression::Constant(GraphElement::GraphLiteral(
                        ingress::RdfLiteral::IntegerLiteral(1.into()),
                    ))),
                ),
                "z".to_string(),
            ),
        ];
        let text = service_body_to_sparql_text(&components);
        assert!(text.contains("BIND"));
        parses(&text);
    }

    // 6. VALUES with multiple variables and at least one UNDEF.
    #[test]
    fn values_multi_var_with_undef() {
        let components = vec![QueryComponent::Values(
            vec!["a".to_string(), "b".to_string()],
            vec![
                vec![Some(iri_node(&format!("{EX}1"))), None],
                vec![
                    None,
                    Some(GraphElement::GraphLiteral(
                        ingress::RdfLiteral::LiteralString("hi \"there\"".to_string()),
                    )),
                ],
            ],
        )];
        let text = service_body_to_sparql_text(&components);
        assert!(text.contains("VALUES"));
        assert!(text.contains("UNDEF"));
        parses(&text);
    }

    // 7. GRAPH and MINUS.
    #[test]
    fn graph_and_minus() {
        let components = vec![
            QueryComponent::Graph(
                var("g"),
                vec![QueryComponent::BGP(vec![tp(
                    var("x"),
                    iri_const(&format!("{EX}p")),
                    var("y"),
                )])],
            ),
            QueryComponent::Minus(vec![QueryComponent::BGP(vec![tp(
                var("x"),
                iri_const(&format!("{EX}excluded")),
                var("y"),
            )])]),
        ];
        let text = service_body_to_sparql_text(&components);
        assert!(text.contains("GRAPH"));
        assert!(text.contains("MINUS"));
        parses(&text);
    }

    // 8. Round-trip bar: a combined body covering several constructs at once,
    // rendered and re-parsed.
    #[test]
    fn combined_body_round_trips() {
        let components = vec![
            QueryComponent::BGP(vec![tp(var("x"), iri_const(&format!("{EX}p")), var("y"))]),
            QueryComponent::Optional(vec![QueryComponent::BGP(vec![tp(
                var("y"),
                iri_const(&format!("{EX}q")),
                var("z"),
            )])]),
            QueryComponent::Filter(Expression::Binary(
                Box::new(Expression::Variable("z".to_string())),
                BinaryOp::Ne,
                Box::new(Expression::Constant(GraphElement::GraphLiteral(
                    ingress::RdfLiteral::LiteralString("excluded".to_string()),
                ))),
            )),
            QueryComponent::Union(
                vec![QueryComponent::BGP(vec![tp(
                    var("x"),
                    iri_const(&format!("{EX}r1")),
                    var("w"),
                )])],
                vec![QueryComponent::BGP(vec![tp(
                    var("x"),
                    iri_const(&format!("{EX}r2")),
                    var("w"),
                )])],
            ),
            QueryComponent::Bind(Expression::Variable("w".to_string()), "alias".to_string()),
            QueryComponent::Values(
                vec!["extra".to_string()],
                vec![vec![Some(iri_node(&format!("{EX}v")))], vec![None]],
            ),
        ];
        let text = service_body_to_sparql_text(&components);
        parses(&text);
    }

    // A nested `{ SELECT ... }` subquery inside a SERVICE body.
    #[test]
    fn subquery_round_trips() {
        let inner = Query::Select {
            projection: vec![ProjectionElement::Variable("x".to_string())],
            dataset: vec![],
            where_clause: vec![QueryComponent::BGP(vec![tp(
                var("x"),
                iri_const(&format!("{EX}p")),
                var("y"),
            )])],
            group_by: vec![],
            having: vec![],
            order_by: vec![],
            limit: Some(5),
            offset: None,
            distinct: false,
        };
        let components = vec![QueryComponent::Subquery(Box::new(inner))];
        let text = service_body_to_sparql_text(&components);
        assert!(text.contains("SELECT"));
        assert!(text.contains("LIMIT 5"));
        parses(&text);
    }
}

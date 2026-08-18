/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

use super::functions::{
    compare_graph_elements, eval_function_bool, eval_function_value, graph_element_to_string,
    values_equal,
};
use super::solutions::solution_row_to_partial;
use super::*;

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
    // The Datalog substitution already holds interned ids — carry them through
    // directly as `Interned` bindings (no `GraphElement` materialisation).
    let gel_sub: PartialSub = sub
        .iter()
        .map(|(var, &id)| (var.clone(), PartialSubValue::Interned(id)))
        .collect();
    eval_expression_bool(
        expr,
        &gel_sub,
        datastore,
        &ActiveGraph::Fixed(DEFAULT_GRAPH_ELEMENT_ID),
    )
    .unwrap_or(false)
}

/// Evaluate a SPARQL expression as a boolean filter against the default graph.
///
/// `sub` maps variable names to their bound `GraphElement` values.  This is
/// the public entry point for downstream crates (e.g. `datalog`, `shacl`) that
/// need to test a SPARQL `Expression` guard without access to the internal
/// `ActiveGraph` type.  EXISTS / NOT EXISTS expressions use the default graph.
///
/// Returns `false` on evaluation error or when the expression is unbound.
/// See: <https://github.com/daghovland/rdf-datalog/issues/60>
pub fn eval_expression_bool_filter(
    expr: &Expression,
    sub: &SolutionRow,
    datastore: &Datastore,
) -> bool {
    eval_expression_bool(
        expr,
        &solution_row_to_partial(sub),
        datastore,
        &ActiveGraph::Fixed(DEFAULT_GRAPH_ELEMENT_ID),
    )
    .unwrap_or(false)
}

pub(crate) fn eval_filter(
    expr: &Expression,
    sub: &PartialSub,
    datastore: &Datastore,
    active_graph: &ActiveGraph,
) -> bool {
    eval_expression_bool(expr, sub, datastore, active_graph).unwrap_or(false)
}

/// Evaluate an expression to a concrete GraphElement value.
///
/// `sub` maps variable names to their bound [`GraphElement`] values.  Constants
/// in the query (e.g. `"SPARQL"` in `regex(?x, "SPARQL")`) are returned directly
/// without touching the datastore.
///
/// Returns `None` when the expression is unbound or evaluation fails (e.g.
/// division by zero, type mismatch).
///
/// This is the public entry point for downstream crates; internally the
/// evaluator threads a `PartialSub` (which may hold interned ids) through
/// `eval_expression_value_inner`.
/// See: <https://github.com/daghovland/rdf-datalog/issues/60>
pub fn eval_expression_value(
    expr: &Expression,
    sub: &SolutionRow,
    datastore: &Datastore,
) -> Option<GraphElement> {
    eval_expression_value_inner(expr, &solution_row_to_partial(sub), datastore)
}

/// Evaluate an expression against an internal [`PartialSub`] solution.
///
/// `sub` maps variable names to their current bindings ([`PartialSubValue`]).
/// Returns `None` when the expression is unbound or evaluation fails.
pub(crate) fn eval_expression_value_inner(
    expr: &Expression,
    sub: &PartialSub,
    datastore: &Datastore,
) -> Option<GraphElement> {
    match expr {
        Expression::Variable(v) => sub.get(v).map(|val| val.resolve(datastore)),
        Expression::Constant(gel) => Some(gel.clone()),
        Expression::FunctionCall(name, args) => eval_function_value(name, args, sub, datastore),
        Expression::Binary(
            l,
            op @ (BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div),
            r,
        ) => eval_arithmetic(l, op, r, sub, datastore),
        // Comparison (`=`, `!=`, `<`, `>`, `<=`, `>=`) and logical (`&&`,
        // `||`) operators don't produce a value of their own in
        // `eval_arithmetic` — they're boolean-valued. In a value-producing
        // context (projection, `BIND`) that boolean must still surface as
        // an `xsd:boolean` literal rather than silently evaluating to
        // nothing; delegate to the boolean evaluator (which already
        // normalizes numeric equality/ordering across literal
        // representations, see `values_equal`/`compare_graph_elements`) and
        // wrap the result. `EXISTS`/`NOT EXISTS` don't appear directly under
        // these operators here (they're handled by `eval_expression_bool`
        // itself against the default graph, matching `eval_bind_expr`'s and
        // `eval_expression_bool_filter`'s existing convention for
        // value/BIND contexts that have no `ActiveGraph` in scope).
        // See https://github.com/daghovland/rdf-datalog/issues/207.
        Expression::Binary(
            _,
            BinaryOp::Eq
            | BinaryOp::Ne
            | BinaryOp::Lt
            | BinaryOp::Gt
            | BinaryOp::Le
            | BinaryOp::Ge
            | BinaryOp::And
            | BinaryOp::Or,
            _,
        )
        | Expression::Unary(UnaryOp::Not, _) => {
            let b = eval_expression_bool(
                expr,
                sub,
                datastore,
                &ActiveGraph::Fixed(DEFAULT_GRAPH_ELEMENT_ID),
            )?;
            Some(GraphElement::GraphLiteral(RdfLiteral::BooleanLiteral(b)))
        }
        Expression::Unary(UnaryOp::Plus, inner) => {
            eval_expression_value_inner(inner, sub, datastore)
        }
        Expression::Unary(UnaryOp::Minus, inner) => {
            arithmetic_negate(eval_expression_value_inner(inner, sub, datastore)?)
        }
        _ => None,
    }
}

/// Negate a numeric literal.
///
/// Uses `classify_numeric`/`numeric_lit_to_element` (rather than matching
/// each `RdfLiteral` variant by hand) so that: (1) a real `TypedLiteral{
/// xsd:decimal, .. }` input stays `xsd:decimal` after negation instead of
/// being promoted to `xsd:double` (the previous fallback for any non-integer
/// `TypedLiteral`), and (2) the result is emitted in the same `TypedLiteral`
/// shape real data uses, so it can join against already-interned data of
/// the same negated value. See <https://github.com/daghovland/rdf-datalog/issues/228>.
pub(crate) fn arithmetic_negate(el: GraphElement) -> Option<GraphElement> {
    let lit = match &el {
        GraphElement::GraphLiteral(lit) => lit,
        _ => return None,
    };
    let negated = match classify_numeric(lit)? {
        NumericLit::Integer(n) => NumericLit::Integer(-n),
        NumericLit::Decimal(d) => NumericLit::Decimal(-d),
        NumericLit::Float(f) => NumericLit::Float(-f),
        NumericLit::Double(f) => NumericLit::Double(-f),
    };
    Some(numeric_lit_to_element(negated))
}

/// A numeric literal normalised to one of SPARQL/XPath's four numeric type
/// ranks — `xsd:integer` ⊂ `xsd:decimal` ⊂ `xsd:float` ⊂ `xsd:double`
/// (SPARQL 1.1 §17.1, "Operand Data Types") — regardless of which
/// `RdfLiteral` shape it arrived in.
pub(crate) enum NumericLit {
    Integer(BigInt),
    Decimal(rust_decimal::Decimal),
    Float(f64),
    Double(f64),
}

/// Classify a literal's numeric type and value.
///
/// Both the Turtle parser (`turtle::convert_literal`) and the SPARQL
/// numeric-literal parser (`parse_numeric_literal`, deliberately mirroring
/// it) always produce the generic `RdfLiteral::TypedLiteral { type_iri,
/// literal }` shape for numeric data. The canonical `IntegerLiteral` /
/// `DecimalLiteral` / `FloatLiteral` / `DoubleLiteral` variants are only ever
/// produced by aggregates (`SUM`/`COUNT`/`AVG`/etc., which cannot appear
/// inside `BIND` so never hit the join-lookup bug below) — every scalar
/// producer (`eval_arithmetic`, `arithmetic_negate`, `ABS`/`CEIL`/`FLOOR`/
/// `ROUND`, the xsd casts) goes through `numeric_lit_to_element` below to
/// emit the same `TypedLiteral` shape. Recognising only the canonical
/// variants here would mean arithmetic on any real data silently falls
/// through to `xsd:double` promotion, corrupting `1 + 1` into `2.0e0`. See
/// <https://github.com/daghovland/rdf-datalog/issues/207> (and the sibling
/// gap in <https://github.com/daghovland/rdf-datalog/issues/198>).
pub(crate) fn classify_numeric(lit: &RdfLiteral) -> Option<NumericLit> {
    match lit {
        RdfLiteral::IntegerLiteral(n) => Some(NumericLit::Integer(n.clone())),
        RdfLiteral::DecimalLiteral(d) => Some(NumericLit::Decimal(*d)),
        RdfLiteral::FloatLiteral(f) => Some(NumericLit::Float(f.into_inner())),
        RdfLiteral::DoubleLiteral(d) => Some(NumericLit::Double(d.into_inner())),
        RdfLiteral::TypedLiteral { type_iri, literal } => match type_iri.0.as_str() {
            XSD_INTEGER => literal.parse::<BigInt>().ok().map(NumericLit::Integer),
            XSD_DECIMAL => literal
                .parse::<rust_decimal::Decimal>()
                .ok()
                .map(NumericLit::Decimal),
            XSD_FLOAT => literal.parse::<f64>().ok().map(NumericLit::Float),
            XSD_DOUBLE => literal.parse::<f64>().ok().map(NumericLit::Double),
            _ => None,
        },
        _ => None,
    }
}

/// Reconstruct a classified numeric value as the `TypedLiteral { type_iri,
/// literal }` shape real parsed data always uses (see `classify_numeric`'s
/// doc comment above), rather than a producer-specific native `RdfLiteral`
/// variant.
///
/// A single normalization point for every scalar numeric producer
/// (`eval_arithmetic`, `arithmetic_negate`, `ABS`/`CEIL`/`FLOOR`/`ROUND`) so a
/// `BIND`-computed numeric value used in a later triple-pattern join
/// position (e.g. `BIND(ABS(?o) AS ?z) . ?s1 ?p1 ?z`) is looked up in
/// `resource_map` by structural equality (`resolve_match_term`) and actually
/// finds the already-interned resource, regardless of which function
/// produced it. Generalizes the integer-only version of this fix in
/// `eval_arithmetic` (W3C `bind03`,
/// <https://github.com/daghovland/rdf-datalog/issues/198>) to every other
/// producer — see <https://github.com/daghovland/rdf-datalog/issues/228>.
pub(crate) fn numeric_lit_to_element(n: NumericLit) -> GraphElement {
    let (type_iri, literal) = match n {
        NumericLit::Integer(i) => (XSD_INTEGER, i.to_string()),
        NumericLit::Decimal(d) => (XSD_DECIMAL, d.to_string()),
        NumericLit::Float(f) => (XSD_FLOAT, f.to_string()),
        NumericLit::Double(f) => (XSD_DOUBLE, f.to_string()),
    };
    GraphElement::GraphLiteral(RdfLiteral::TypedLiteral {
        type_iri: IriReference(type_iri.to_string()),
        literal,
    })
}

/// Evaluate a binary arithmetic expression (Add/Sub/Mul/Div), applying the
/// SPARQL/XPath numeric type-promotion rules: the result takes the wider of
/// the two operand types (integer < decimal < float < double), so
/// `integer + integer` stays `integer`, `decimal + integer` becomes
/// `decimal`, and only an operand that is genuinely `xsd:float`/`xsd:double`
/// forces promotion to floating point.
/// Returns `None` if operands are not numeric or op is not arithmetic.
pub(crate) fn eval_arithmetic(
    left: &Expression,
    op: &BinaryOp,
    right: &Expression,
    sub: &PartialSub,
    datastore: &Datastore,
) -> Option<GraphElement> {
    let l = eval_expression_value_inner(left, sub, datastore)?;
    let r = eval_expression_value_inner(right, sub, datastore)?;
    let l_lit = match &l {
        GraphElement::GraphLiteral(lit) => lit,
        _ => return None,
    };
    let r_lit = match &r {
        GraphElement::GraphLiteral(lit) => lit,
        _ => return None,
    };
    let ln = classify_numeric(l_lit)?;
    let rn = classify_numeric(r_lit)?;

    // Exact fast path: integer op integer stays integer for Add/Sub/Mul.
    // `Div` is deliberately excluded: SPARQL/XPath's `op:numeric-divide`
    // always promotes an integer/integer division to `xsd:decimal`, even
    // when both operands are integers (e.g. `2/2` is `1.0`, not the
    // integer-division result `1`) — falls through to the decimal path
    // below instead. See W3C `coalesce01` (#205): a prior version used
    // truncating `BigInt` division and emitted `xsd:integer`, which both
    // computed the wrong value for non-exact quotients and used the wrong
    // datatype for exact ones.
    if let (NumericLit::Integer(a), NumericLit::Integer(b)) = (&ln, &rn) {
        if !matches!(op, BinaryOp::Div) {
            let result = match op {
                BinaryOp::Add => a + b,
                BinaryOp::Sub => a - b,
                BinaryOp::Mul => a * b,
                _ => return None,
            };
            // Emit the same `TypedLiteral { type_iri, literal }` shape the
            // Turtle and SPARQL numeric-literal parsers always produce for
            // real data (see `classify_numeric`'s doc comment above), via
            // `numeric_lit_to_element`, rather than the canonical
            // `IntegerLiteral` variant. A `BIND`-computed value used in a
            // later triple-pattern position (e.g. `BIND(?o+1 AS ?z) . ?s1
            // ?p1 ?z`) is looked up in `resource_map` by structural equality
            // (`resolve_match_term`); an `IntegerLiteral` never structurally
            // equals the `TypedLiteral` shape under which the same value was
            // actually interned, so the lookup silently failed and the join
            // produced zero rows regardless of whether the value was
            // genuinely present. See W3C `bind03` and
            // <https://github.com/daghovland/rdf-datalog/issues/198>.
            return Some(numeric_lit_to_element(NumericLit::Integer(result)));
        }
        // `Div`: fall through to the decimal path below (see comment above).
    }

    // A genuinely `xsd:double` operand forces double-precision arithmetic.
    // See #228: the result must use `numeric_lit_to_element`, not the
    // native `DoubleLiteral` variant, for the same join-lookup reason as the
    // integer fast path above.
    if matches!(ln, NumericLit::Double(_)) || matches!(rn, NumericLit::Double(_)) {
        let result = apply_f64_op(op, numeric_lit_to_f64(&ln), numeric_lit_to_f64(&rn))?;
        return Some(numeric_lit_to_element(NumericLit::Double(result)));
    }

    // A genuinely `xsd:float` operand (with no double present) forces
    // float-precision arithmetic. See #228, as above.
    if matches!(ln, NumericLit::Float(_)) || matches!(rn, NumericLit::Float(_)) {
        let result = apply_f64_op(op, numeric_lit_to_f64(&ln), numeric_lit_to_f64(&rn))?;
        return Some(numeric_lit_to_element(NumericLit::Float(result)));
    }

    // Remaining case: an integer/decimal mix with at least one decimal
    // operand — exact decimal arithmetic, result stays decimal. See #228,
    // as above.
    let ad = numeric_lit_to_decimal(&ln)?;
    let bd = numeric_lit_to_decimal(&rn)?;
    let result = match op {
        BinaryOp::Add => ad + bd,
        BinaryOp::Sub => ad - bd,
        BinaryOp::Mul => ad * bd,
        BinaryOp::Div => {
            if bd.is_zero() {
                return None;
            }
            ad / bd
        }
        _ => return None,
    };
    Some(numeric_lit_to_element(NumericLit::Decimal(result)))
}

/// Widen a classified numeric literal to `f64` for float/double arithmetic.
pub(crate) fn numeric_lit_to_f64(n: &NumericLit) -> f64 {
    match n {
        NumericLit::Integer(i) => i.to_string().parse().unwrap_or(f64::NAN),
        NumericLit::Decimal(d) => d.to_string().parse().unwrap_or(f64::NAN),
        NumericLit::Float(f) | NumericLit::Double(f) => *f,
    }
}

/// Widen a classified integer/decimal numeric literal to `Decimal` for exact
/// decimal arithmetic. Not meaningful for `Float`/`Double` — callers only
/// reach this after ruling both out.
pub(crate) fn numeric_lit_to_decimal(n: &NumericLit) -> Option<rust_decimal::Decimal> {
    match n {
        NumericLit::Integer(i) => i.to_string().parse().ok(),
        NumericLit::Decimal(d) => Some(*d),
        NumericLit::Float(_) | NumericLit::Double(_) => None,
    }
}

/// Apply an arithmetic `BinaryOp` to two `f64` operands.
pub(crate) fn apply_f64_op(op: &BinaryOp, a: f64, b: f64) -> Option<f64> {
    Some(match op {
        BinaryOp::Add => a + b,
        BinaryOp::Sub => a - b,
        BinaryOp::Mul => a * b,
        BinaryOp::Div => {
            if b == 0.0 {
                return None;
            }
            a / b
        }
        _ => return None,
    })
}

/// Shared implementation for the boolean-valued string predicates
/// `STRSTARTS`, `STRENDS`, and `CONTAINS` (SPARQL 1.1 §17.4.3).
///
/// Used by both `eval_function_value` (for `BIND`/projection contexts,
/// wrapped in a `BooleanLiteral`) and `eval_function_bool` (for direct
/// `FILTER` contexts) so the two dispatch paths cannot diverge.
pub(crate) fn eval_string_predicate(
    name: &str,
    args: &[Expression],
    sub: &PartialSub,
    datastore: &Datastore,
) -> Option<bool> {
    let text_el = eval_expression_value_inner(args.first()?, sub, datastore)?;
    let text = graph_element_to_string(&text_el)?;
    let arg_el = eval_expression_value_inner(args.get(1)?, sub, datastore)?;
    let arg = graph_element_to_string(&arg_el)?;
    match name {
        "STRSTARTS" => Some(text.starts_with(arg.as_str())),
        "STRENDS" => Some(text.ends_with(arg.as_str())),
        "CONTAINS" => Some(text.contains(arg.as_str())),
        _ => None,
    }
}

pub(crate) fn eval_expression_bool(
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
                let l = eval_expression_value_inner(left, sub, datastore)?;
                let r = eval_expression_value_inner(right, sub, datastore)?;
                Some(values_equal(&l, &r))
            }
            BinaryOp::Ne => {
                let l = eval_expression_value_inner(left, sub, datastore)?;
                let r = eval_expression_value_inner(right, sub, datastore)?;
                Some(!values_equal(&l, &r))
            }
            BinaryOp::Lt | BinaryOp::Gt | BinaryOp::Le | BinaryOp::Ge => {
                let l = eval_expression_value_inner(left, sub, datastore)?;
                let r = eval_expression_value_inner(right, sub, datastore)?;
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
        Expression::In(expr, list) => {
            let val = eval_expression_value_inner(expr, sub, datastore)?;
            Some(list.iter().any(|item| {
                eval_expression_value_inner(item, sub, datastore)
                    .map(|v| values_equal(&v, &val))
                    .unwrap_or(false)
            }))
        }
        Expression::NotIn(expr, list) => {
            let val = eval_expression_value_inner(expr, sub, datastore)?;
            Some(!list.iter().any(|item| {
                eval_expression_value_inner(item, sub, datastore)
                    .map(|v| values_equal(&v, &val))
                    .unwrap_or(false)
            }))
        }
        Expression::FunctionCall(name, args) => eval_function_bool(name, args, sub, datastore),
        Expression::Exists(inner) => {
            // EXISTS/NOT EXISTS sub-evaluation is intentionally outside the
            // deadline-threaded chain (#372): expression evaluation is
            // itself out of scope for cooperative-cancellation checks (no
            // per-row iteration to bound at this level), and threading a
            // `Deadline` into every expression-evaluation function just to
            // reach these two cases would balloon the diff for a case that
            // is bounded by the query's own solution-row size in practice.
            // `Deadline::none()` can never itself produce an `Err` (its
            // `check()` is always `Ok(())`), so the only way this call could
            // fail is if a *nested* deadline-bearing evaluation propagated an
            // error into it — which cannot happen here, since `None` never
            // installs an expiring deadline anywhere downstream either.
            //
            // Only whether `inner` has *any* solution matters here (never
            // which one, or how many) and the result is discarded except for
            // `is_empty()` — no bindings from `inner` leak into `sub`. So,
            // like `Query::Ask` (issue #536), budget the evaluation to a
            // single row: an unselective `inner` pattern stops scanning after
            // the first match instead of enumerating every match.
            let sols = eval_components_budgeted(
                inner,
                vec![sub.clone()],
                datastore,
                (*active_graph).clone(),
                Some(1),
                &Deadline::none(),
            )
            .unwrap_or_default();
            Some(!sols.is_empty())
        }
        Expression::NotExists(inner) => {
            // See the comment on the `Exists` arm above (including the
            // #536 budget-of-1 short-circuit — inverting the boolean does
            // not change how many solutions are needed to decide it).
            let sols = eval_components_budgeted(
                inner,
                vec![sub.clone()],
                datastore,
                (*active_graph).clone(),
                Some(1),
                &Deadline::none(),
            )
            .unwrap_or_default();
            Some(sols.is_empty())
        }
        _ => {
            let el = eval_expression_value_inner(expr, sub, datastore)?;
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

/// Evaluate an expression for use in `BIND`, returning its `GraphElement` value.
/// Supports variables, constants, arithmetic, and function calls.
pub(crate) fn eval_bind_expr(
    expr: &Expression,
    sub: &PartialSub,
    datastore: &Datastore,
) -> Option<GraphElement> {
    // Scope `BNODE(str)` memoization to this one BIND evaluation (one
    // solution row) — see `project_with_exprs_partial` and #346.
    let _bnode_guard = BnodeMemoGuard::install();
    eval_expression_value_inner(expr, sub, datastore)
}

/// Evaluate a binary operation between two already-resolved values, e.g. an
/// arithmetic expression combining two aggregate results in a SELECT/HAVING
/// clause (`(MIN(?p) + MAX(?p)) / 2`).
///
/// Applies the same SPARQL/XPath numeric type-promotion rules as
/// `eval_arithmetic` (integer < decimal < float < double, result takes the
/// widest operand type) via the shared `classify_numeric`/
/// `numeric_lit_to_element` machinery, rather than the previous
/// integer-fast-path-or-else-`f64` split, which silently forced every
/// non-integer/integer combination (including a plain integer/decimal mix)
/// to `xsd:double` and lost `xsd:decimal` precision/typing. One deliberate
/// divergence from `eval_arithmetic`: `Div` between two integers here
/// produces an exact `xsd:decimal` (per SPARQL/XPath `op:numeric-divide`,
/// which never returns integer), rather than `eval_arithmetic`'s truncating
/// integer division — this only affects aggregate-expression arithmetic
/// (matching the W3C `aggregates` suite's `agg-err-01` expectation), not the
/// general BIND/FILTER arithmetic path. See
/// <https://github.com/daghovland/rdf-datalog/issues/202>.
pub(crate) fn eval_binary_value(
    l: &GraphElement,
    op: &BinaryOp,
    r: &GraphElement,
) -> Option<GraphElement> {
    let l_lit = match l {
        GraphElement::GraphLiteral(lit) => lit,
        _ => return None,
    };
    let r_lit = match r {
        GraphElement::GraphLiteral(lit) => lit,
        _ => return None,
    };
    let ln = classify_numeric(l_lit)?;
    let rn = classify_numeric(r_lit)?;

    if matches!(ln, NumericLit::Double(_)) || matches!(rn, NumericLit::Double(_)) {
        let result = apply_f64_op(op, numeric_lit_to_f64(&ln), numeric_lit_to_f64(&rn))?;
        return Some(numeric_lit_to_element(NumericLit::Double(result)));
    }
    if matches!(ln, NumericLit::Float(_)) || matches!(rn, NumericLit::Float(_)) {
        let result = apply_f64_op(op, numeric_lit_to_f64(&ln), numeric_lit_to_f64(&rn))?;
        return Some(numeric_lit_to_element(NumericLit::Float(result)));
    }
    if let (NumericLit::Integer(a), NumericLit::Integer(b)) = (&ln, &rn) {
        return match op {
            BinaryOp::Add => Some(numeric_lit_to_element(NumericLit::Integer(a + b))),
            BinaryOp::Sub => Some(numeric_lit_to_element(NumericLit::Integer(a - b))),
            BinaryOp::Mul => Some(numeric_lit_to_element(NumericLit::Integer(a * b))),
            BinaryOp::Div => {
                if b == &BigInt::from(0) {
                    return None;
                }
                let ad = numeric_lit_to_decimal(&ln)?;
                let bd = numeric_lit_to_decimal(&rn)?;
                Some(numeric_lit_to_element(NumericLit::Decimal(ad / bd)))
            }
            _ => None,
        };
    }
    // Remaining case: an integer/decimal mix with at least one decimal
    // operand — exact decimal arithmetic, result stays decimal.
    let ad = numeric_lit_to_decimal(&ln)?;
    let bd = numeric_lit_to_decimal(&rn)?;
    let result = match op {
        BinaryOp::Add => ad + bd,
        BinaryOp::Sub => ad - bd,
        BinaryOp::Mul => ad * bd,
        BinaryOp::Div => {
            if bd.is_zero() {
                return None;
            }
            ad / bd
        }
        _ => return None,
    };
    Some(numeric_lit_to_element(NumericLit::Decimal(result)))
}

/// Resolve a template term to a concrete `GraphElement`, remapping blank nodes per solution.
///
/// Returns `None` if the term is an unbound variable (triple is silently skipped).
pub(crate) fn bind_template_term(
    term: &Term,
    sub: &PartialSub,
    datastore: &Datastore,
    bnode_map: &mut HashMap<u32, u32>,
    bnode_counter: &mut u32,
) -> Option<GraphElement> {
    match term {
        Term::Variable(v) => sub.get(v).map(|val| val.resolve(datastore)),
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
        // CONSTRUCT templates containing a triple term are out of scope for
        // phase R3 (#146); skip the triple rather than emit something wrong.
        Term::TripleTerm(_) => None,
    }
}

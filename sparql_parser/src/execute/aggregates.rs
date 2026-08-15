/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

use super::expressions::{
    classify_numeric, eval_binary_value, eval_expression_bool, eval_expression_value_inner,
    numeric_lit_to_decimal, numeric_lit_to_element, numeric_lit_to_f64, NumericLit,
};
use super::functions::{compare_graph_elements, graph_element_to_string};
use super::*;

/// True if a projection element contains an aggregate expression.
pub(crate) fn elem_has_aggregate(elem: &ProjectionElement) -> bool {
    match elem {
        ProjectionElement::Expression(expr, _) => expr_has_aggregate(expr),
        _ => false,
    }
}

pub(crate) fn expr_has_aggregate(expr: &Expression) -> bool {
    match expr {
        Expression::Aggregate(_) => true,
        Expression::Binary(l, _, r) => expr_has_aggregate(l) || expr_has_aggregate(r),
        Expression::Unary(_, inner) => expr_has_aggregate(inner),
        Expression::FunctionCall(_, args) => args.iter().any(expr_has_aggregate),
        _ => false,
    }
}

/// Partition solutions into groups keyed by GROUP BY expressions.
///
/// When `group_by` is empty all solutions fall into one implicit group.
///
/// A `GroupCondition` written `(expr AS ?var)` additionally binds the
/// computed grouping key to `?var` in every solution of the resulting group
/// (see [`bind_group_aliases`]), so it is available to the projection,
/// `HAVING`, and `ORDER BY` like any other bound variable — this is what lets
/// `GROUP BY (COALESCE(?w, ...) AS ?X)` project `?X` (W3C SPARQL 1.1
/// `grouping` suite `Group-4`,
/// <https://github.com/daghovland/rdf-datalog/issues/206>).
pub(crate) fn group_by_solutions(
    solutions: &[PartialSub],
    group_by: &[GroupCondition],
    datastore: &Datastore,
) -> Vec<Vec<PartialSub>> {
    if group_by.is_empty() {
        return vec![solutions.to_vec()];
    }
    // Special case per SPARQL 1.1 §11.4.1 (see the "agg empty group" / "Aggregate
    // over empty group resulting in a row with unbound variables" W3C test,
    // <http://answers.semanticweb.com/questions/17410/>, tracked in
    // <https://github.com/daghovland/rdf-datalog/issues/202>): when the WHERE
    // clause produces zero solutions, an explicit GROUP BY still yields exactly
    // one (empty) group rather than zero groups. Every GROUP BY key and
    // aggregate is then evaluated over that empty group, leaving them (and any
    // GROUP BY alias) unbound in the single output row.
    if solutions.is_empty() {
        return vec![vec![]];
    }
    let mut map: Vec<(Vec<Option<GraphElement>>, Vec<PartialSub>)> = Vec::new();
    'outer: for sub in solutions {
        let key: Vec<Option<GraphElement>> = group_by
            .iter()
            .map(|gc| eval_expression_value_inner(&gc.expr, sub, datastore))
            .collect();
        let bound_sub = bind_group_aliases(sub, group_by, &key);
        for (k, group) in &mut map {
            if *k == key {
                group.push(bound_sub);
                continue 'outer;
            }
        }
        map.push((key, vec![bound_sub]));
    }
    map.into_iter().map(|(_, g)| g).collect()
}

/// Bind each aliased `GroupCondition`'s computed key value to its alias
/// variable in `sub`. Conditions with no `AS var` (the common case) leave
/// `sub` untouched. An unbound key component (e.g. the grouping expression
/// errored for this solution) leaves the alias unbound too, rather than
/// binding it to some placeholder value.
pub(crate) fn bind_group_aliases(
    sub: &PartialSub,
    group_by: &[GroupCondition],
    key: &[Option<GraphElement>],
) -> PartialSub {
    if group_by.iter().all(|gc| gc.alias.is_none()) {
        return sub.clone();
    }
    let mut sub = sub.clone();
    for (gc, val) in group_by.iter().zip(key.iter()) {
        if let Some(alias) = &gc.alias {
            match val {
                Some(v) => {
                    sub.insert(alias.clone(), PartialSubValue::Computed(v.clone()));
                }
                None => {
                    sub.remove(alias);
                }
            }
        }
    }
    sub
}

/// Build the output row for one group in an aggregate query.
pub(crate) fn project_aggregate_row(
    projection: &[ProjectionElement],
    group: &[PartialSub],
    datastore: &Datastore,
) -> SolutionRow {
    // Same "one query solution" scoping as `project_with_exprs_partial` — an
    // aggregate row is itself one solution, so `BNODE(str)` calls across its
    // projection elements should share a memo, and different group rows must
    // not. See #346.
    let _bnode_guard = BnodeMemoGuard::install();
    let rep = group.first().cloned().unwrap_or_default();
    let mut row = SolutionRow::new();
    for elem in projection {
        match elem {
            ProjectionElement::Variable(v) => {
                if let Some(val) = rep.get(v) {
                    row.insert(v.clone(), val.resolve(datastore));
                }
            }
            ProjectionElement::Expression(expr, alias) => {
                if let Some(val) = eval_expr_in_group(expr, group, &rep, datastore) {
                    row.insert(alias.clone(), val);
                }
            }
            ProjectionElement::Star => {}
        }
    }
    row
}

/// Evaluate an expression in the context of a group (for SELECT and HAVING).
///
/// Aggregate sub-expressions are computed over the full group; non-aggregate
/// sub-expressions use the representative solution `rep`.
pub(crate) fn eval_expr_in_group(
    expr: &Expression,
    group: &[PartialSub],
    rep: &PartialSub,
    datastore: &Datastore,
) -> Option<GraphElement> {
    match expr {
        Expression::Aggregate(agg) => eval_aggregate_value(agg, group, datastore),
        Expression::Binary(l, op, r) => {
            // Arithmetic in HAVING (e.g. SUM(?x) > 5): eval both sides in group context
            let lv = eval_expr_in_group(l, group, rep, datastore)?;
            let rv = eval_expr_in_group(r, group, rep, datastore)?;
            // Reuse the arithmetic helper by creating single-element "groups" (for pure values)
            eval_binary_value(&lv, op, &rv)
        }
        _ => eval_expression_value_inner(expr, rep, datastore),
    }
}

/// Evaluate a HAVING expression as a boolean, with aggregates computed over the group.
pub(crate) fn eval_having_expr(
    expr: &Expression,
    group: &[PartialSub],
    datastore: &Datastore,
) -> bool {
    let rep = group.first().cloned().unwrap_or_default();
    eval_having_bool(expr, group, &rep, datastore).unwrap_or(false)
}

pub(crate) fn eval_having_bool(
    expr: &Expression,
    group: &[PartialSub],
    rep: &PartialSub,
    datastore: &Datastore,
) -> Option<bool> {
    match expr {
        Expression::Binary(left, op, right) => match op {
            BinaryOp::And => {
                let l = eval_having_bool(left, group, rep, datastore).unwrap_or(false);
                let r = eval_having_bool(right, group, rep, datastore).unwrap_or(false);
                Some(l && r)
            }
            BinaryOp::Or => {
                let l = eval_having_bool(left, group, rep, datastore).unwrap_or(false);
                let r = eval_having_bool(right, group, rep, datastore).unwrap_or(false);
                Some(l || r)
            }
            BinaryOp::Eq
            | BinaryOp::Ne
            | BinaryOp::Lt
            | BinaryOp::Gt
            | BinaryOp::Le
            | BinaryOp::Ge => {
                let l = eval_expr_in_group(left, group, rep, datastore)?;
                let r = eval_expr_in_group(right, group, rep, datastore)?;
                let ord = compare_graph_elements(&l, &r)?;
                Some(match op {
                    BinaryOp::Eq => ord == 0,
                    BinaryOp::Ne => ord != 0,
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
            Some(!eval_having_bool(inner, group, rep, datastore).unwrap_or(false))
        }
        _ => eval_expression_bool(
            expr,
            rep,
            datastore,
            &ActiveGraph::Fixed(DEFAULT_GRAPH_ELEMENT_ID),
        ),
    }
}

/// Compute an aggregate function over a group of solutions.
pub(crate) fn eval_aggregate_value(
    agg: &Aggregate,
    group: &[PartialSub],
    datastore: &Datastore,
) -> Option<GraphElement> {
    match agg {
        Aggregate::CountStar => Some(GraphElement::GraphLiteral(RdfLiteral::IntegerLiteral(
            BigInt::from(group.len()),
        ))),

        Aggregate::Count(expr, distinct) => {
            let mut values: Vec<GraphElement> = group
                .iter()
                .filter_map(|sub| eval_expression_value_inner(expr, sub, datastore))
                .collect();
            if *distinct {
                let set: HashSet<_> = values.drain(..).collect();
                values.extend(set);
            }
            Some(GraphElement::GraphLiteral(RdfLiteral::IntegerLiteral(
                BigInt::from(values.len()),
            )))
        }

        Aggregate::Sum(expr, distinct) => {
            let mut values: Vec<GraphElement> = group
                .iter()
                .filter_map(|sub| eval_expression_value_inner(expr, sub, datastore))
                .collect();
            if *distinct {
                let set: HashSet<_> = values.drain(..).collect();
                values.extend(set);
            }
            sum_values(&values)
        }

        Aggregate::Avg(expr, distinct) => {
            let mut values: Vec<GraphElement> = group
                .iter()
                .filter_map(|sub| eval_expression_value_inner(expr, sub, datastore))
                .collect();
            if *distinct {
                let set: HashSet<_> = values.drain(..).collect();
                values.extend(set);
            }
            if values.is_empty() {
                return None;
            }
            let sum = sum_values(&values)?;
            let sum_lit = match &sum {
                GraphElement::GraphLiteral(lit) => lit,
                _ => return None,
            };
            let sum_n = classify_numeric(sum_lit)?;
            let count = values.len();
            // Divide, preserving the same integer < decimal < float < double
            // type-promotion `sum_values` already applied to the sum: a
            // `double`/`float` sum stays floating-point, otherwise the
            // division is exact `xsd:decimal` (never plain `xsd:integer` —
            // SPARQL/XPath `op:numeric-divide` never returns integer,
            // matching AVG/AVG-with-GROUP-BY's expected `xsd:decimal` results
            // rather than the previous unconditional `xsd:double`). See
            // <https://github.com/daghovland/rdf-datalog/issues/202>.
            match sum_n {
                NumericLit::Double(f) => {
                    Some(numeric_lit_to_element(NumericLit::Double(f / count as f64)))
                }
                NumericLit::Float(f) => {
                    Some(numeric_lit_to_element(NumericLit::Float(f / count as f64)))
                }
                _ => {
                    let sum_d = numeric_lit_to_decimal(&sum_n)?;
                    let count_d = rust_decimal::Decimal::from(count);
                    Some(numeric_lit_to_element(NumericLit::Decimal(sum_d / count_d)))
                }
            }
        }

        // MIN/MAX use the `<` operator's comparison semantics
        // (`compare_graph_elements`, which returns `None` for operand pairs
        // with no defined ordering — e.g. a numeric literal against a blank
        // node), not `ORDER BY`'s total extended ordering
        // (`compare_graph_elements_total`). Per SPARQL 1.1, if `<` is
        // undefined for any pair of values in the group, the aggregate itself
        // errors and produces no binding, rather than silently falling back
        // to one of the two operands as the previous `reduce`-with-`_ => b`
        // fallback did. See the W3C `aggregates` suite's "Error in AVG"
        // (`agg-err-01`, mixed numeric-literal/blank-node group under `:y`)
        // and <https://github.com/daghovland/rdf-datalog/issues/202>.
        Aggregate::Min(expr, _) => {
            let mut values = group
                .iter()
                .filter_map(|sub| eval_expression_value_inner(expr, sub, datastore));
            let mut current = values.next()?;
            for v in values {
                match compare_graph_elements(&current, &v) {
                    Some(ord) => {
                        if ord > 0 {
                            current = v;
                        }
                    }
                    None => return None,
                }
            }
            Some(current)
        }

        Aggregate::Max(expr, _) => {
            let mut values = group
                .iter()
                .filter_map(|sub| eval_expression_value_inner(expr, sub, datastore));
            let mut current = values.next()?;
            for v in values {
                match compare_graph_elements(&current, &v) {
                    Some(ord) => {
                        if ord < 0 {
                            current = v;
                        }
                    }
                    None => return None,
                }
            }
            Some(current)
        }

        Aggregate::Sample(expr, _) => group
            .iter()
            .find_map(|sub| eval_expression_value_inner(expr, sub, datastore)),

        Aggregate::GroupConcat(expr, sep, distinct) => {
            let mut parts: Vec<String> = group
                .iter()
                .filter_map(|sub| {
                    let el = eval_expression_value_inner(expr, sub, datastore)?;
                    graph_element_to_string(&el)
                })
                .collect();
            if *distinct {
                let set: HashSet<_> = parts.drain(..).collect();
                parts.extend(set);
                parts.sort();
            }
            let result = parts.join(sep);
            Some(GraphElement::GraphLiteral(RdfLiteral::LiteralString(
                result,
            )))
        }
    }
}

/// Sum a list of numeric `GraphElement` values, applying the same
/// SPARQL/XPath numeric type-promotion rules `eval_arithmetic` uses:
/// `xsd:integer` stays exact if every value is an integer, an integer/decimal
/// mix stays exact `xsd:decimal`, and only a genuinely `xsd:double`/
/// `xsd:float` value forces floating-point.
///
/// Uses `classify_numeric` (rather than matching only the native
/// `IntegerLiteral` variant, as a prior version of this function did) so a
/// `TypedLiteral{xsd:integer, ..}` input — which is what every numeric BIND
/// function/cast now produces (#228) as well as what real parsed data always
/// uses — is recognized as an integer instead of silently falling through to
/// the floating-point path (e.g. `SUM(xsd:integer(...))` wrongly summing to
/// an `xsd:double`).
pub(crate) fn sum_values(values: &[GraphElement]) -> Option<GraphElement> {
    if values.is_empty() {
        return Some(numeric_lit_to_element(NumericLit::Integer(BigInt::from(0))));
    }
    let mut classified = Vec::with_capacity(values.len());
    for v in values {
        let lit = match v {
            GraphElement::GraphLiteral(lit) => lit,
            _ => return None,
        };
        classified.push(classify_numeric(lit)?);
    }
    if classified
        .iter()
        .any(|n| matches!(n, NumericLit::Double(_)))
    {
        let sum: f64 = classified.iter().map(numeric_lit_to_f64).sum();
        return Some(numeric_lit_to_element(NumericLit::Double(sum)));
    }
    if classified.iter().any(|n| matches!(n, NumericLit::Float(_))) {
        let sum: f64 = classified.iter().map(numeric_lit_to_f64).sum();
        return Some(numeric_lit_to_element(NumericLit::Float(sum)));
    }
    if classified
        .iter()
        .all(|n| matches!(n, NumericLit::Integer(_)))
    {
        let mut int_sum = BigInt::from(0);
        for n in &classified {
            if let NumericLit::Integer(i) = n {
                int_sum += i;
            }
        }
        return Some(numeric_lit_to_element(NumericLit::Integer(int_sum)));
    }
    // Remaining case: an integer/decimal mix with at least one decimal value.
    let mut dec_sum = rust_decimal::Decimal::from(0);
    for n in &classified {
        dec_sum += numeric_lit_to_decimal(n)?;
    }
    Some(numeric_lit_to_element(NumericLit::Decimal(dec_sum)))
}

/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

use super::aggregates::elem_has_aggregate;
use super::expressions::eval_expression_value_inner;
use super::*;

/// Project a solution row, evaluating any `(expr AS ?alias)` projection elements.
///
/// Thin wrapper around [`project_with_exprs_partial`] that resolves each
/// projected binding to a concrete [`GraphElement`] for the top-level
/// `SELECT` result shape (`SolutionRow`). See that function for the
/// alias-reuse semantics.
pub(crate) fn project_with_exprs(
    sub: &PartialSub,
    projection: &[ProjectionElement],
    datastore: &Datastore,
) -> SolutionRow {
    project_with_exprs_partial(sub, projection, datastore)
        .into_iter()
        .map(|(k, v)| (k, v.resolve(datastore)))
        .collect()
}

/// Project a solution, evaluating any `(expr AS ?alias)` projection elements,
/// keeping the result as a [`PartialSub`] (unresolved bindings) rather than a
/// fully-resolved [`SolutionRow`].
///
/// A later `(expr AS ?alias)` SELECT item may reference an alias bound by an
/// earlier one in the same projection list (e.g.
/// `SELECT (?a + 1 AS ?x) (?x * 2 AS ?y)`) — the W3C project-expression
/// conformance suite's "Reuse a project expression variable in select" case.
/// To support that, expressions are evaluated against a `sub`-derived
/// substitution that accumulates each computed alias as it goes, rather than
/// against the original WHERE-clause bindings alone. `Star`/`Variable`
/// projection elements are unaffected — they always read the original
/// WHERE-clause bindings, never a previously-projected alias.
///
/// Shared by the top-level `SELECT` path ([`project_with_exprs`]) and the
/// non-aggregate subquery projection path (`execute_select_inner`), so the
/// alias-reuse fix from issue 207 (linked below) applies uniformly to both —
/// see issue 223 (linked below) for the subquery-path gap this closes.
/// See <https://github.com/daghovland/rdf-datalog/issues/207> and
/// <https://github.com/daghovland/rdf-datalog/issues/223>.
pub(crate) fn project_with_exprs_partial(
    sub: &PartialSub,
    projection: &[ProjectionElement],
    datastore: &Datastore,
) -> PartialSub {
    // Every projection element for this one solution row is evaluated within
    // a single call, so installing the `BNODE(str)` memo here gives it
    // exactly the "one query solution" scope SPARQL 1.1 §17.4.2.7 requires:
    // shared across the whole row (e.g. `(BNODE(?s1) AS ?b1) (BNODE(?s2) AS
    // ?b2)` in the W3C `bnode01` fixture), cleared before the next row. See
    // #346.
    let _bnode_guard = BnodeMemoGuard::install();
    let mut row: PartialSub = HashMap::new();
    let mut extended: PartialSub = sub.clone();
    for elem in projection {
        match elem {
            ProjectionElement::Star => {
                for (k, v) in sub {
                    row.insert(k.clone(), v.clone());
                }
            }
            ProjectionElement::Variable(v) => {
                if let Some(val) = sub.get(v) {
                    row.insert(v.clone(), val.clone());
                }
            }
            ProjectionElement::Expression(expr, alias) => {
                if let Some(val) = eval_expression_value_inner(expr, &extended, datastore) {
                    extended.insert(alias.clone(), PartialSubValue::Computed(val.clone()));
                    row.insert(alias.clone(), PartialSubValue::Computed(val));
                }
            }
        }
    }
    row
}

// ── Evaluation ────────────────────────────────────────────────────────────────

/// Value-equality between two bindings, reproducing the pre-#141 semantics
/// where `PartialSub` held resolved [`GraphElement`]s compared with `==`.
pub(crate) fn psv_eq(a: &PartialSubValue, b: &PartialSubValue, datastore: &Datastore) -> bool {
    match (a, b) {
        // Interning is injective (`add_resource` dedups), so equal ids denote
        // equal elements — no datastore lookup needed on the hot path.
        (PartialSubValue::Interned(x), PartialSubValue::Interned(y)) => x == y,
        (PartialSubValue::Computed(x), PartialSubValue::Computed(y)) => x == y,
        // Mixed: an interned id and a computed value can still denote the same
        // element, so compare resolved forms.
        _ => a.resolve(datastore) == b.resolve(datastore),
    }
}

/// Whole-solution equality by resolved value, reproducing the pre-#141
/// `HashMap<String, GraphElement>` `==`/`.contains` semantics: same key set and
/// each shared variable's binding resolves to the same [`GraphElement`].
pub(crate) fn partial_subs_equal(a: &PartialSub, b: &PartialSub, datastore: &Datastore) -> bool {
    a.len() == b.len()
        && a.iter().all(|(k, va)| match b.get(k) {
            Some(vb) => psv_eq(va, vb, datastore),
            None => false,
        })
}

/// Wrap a resolved [`SolutionRow`] as a [`PartialSub`]. Used at the public API
/// boundary where callers hand us already-resolved [`GraphElement`] bindings;
/// they are carried as `Computed` (a `Computed` value that is in fact interned
/// is still resolved correctly by [`PartialSubValue::to_id`] / [`psv_eq`]).
pub(crate) fn solution_row_to_partial(row: &SolutionRow) -> PartialSub {
    row.iter()
        .map(|(k, v)| (k.clone(), PartialSubValue::Computed(v.clone())))
        .collect()
}

/// Natural join of a solution set against a `VALUES` data block: `vars` names
/// the columns, `rows` is each inline-data row (`None` for `UNDEF`).
///
/// For every existing solution, every VALUES row that doesn't conflict with
/// an already-bound variable produces one output solution (so a solution can
/// multiply into several rows when more than one VALUES row is compatible —
/// see the W3C bindings-suite `values04`/`values05` fixtures, which rely on
/// exactly this to produce more output rows than input solutions). A `None`
/// (`UNDEF`) entry in a row leaves that variable unconstrained by *that row*
/// — it neither introduces a new binding nor conflicts with an existing one
/// — per SPARQL 1.1 §10.2's inline-data-as-join semantics.
///
/// Backs [`QueryComponent::Values`] (evaluated in `eval_component`), which
/// is *also* how a trailing post-query / post-subquery `ValuesClause` is
/// represented: `sparql_parser::parse_query_body` appends the parsed
/// `ValuesClause` directly onto the query's (or subquery's) `where_clause`
/// rather than modelling it as a separate post-modifier field. That gets its
/// join-before-`Project` placement (SPARQL 1.1 §18.2.4.3 — a ValuesClause
/// variable can bind/restrict solutions even when it isn't in the SELECT
/// list, but is itself projected out only under `SELECT *`) and its
/// subquery-projection scoping for free from the same machinery that
/// already evaluates an inline `VALUES` block, with no separate code path
/// to keep in sync. See <https://github.com/daghovland/rdf-datalog/issues/200>.
pub(crate) fn join_solutions_with_values(
    solutions: Vec<PartialSub>,
    vars: &[String],
    rows: &[Vec<Option<GraphElement>>],
    datastore: &Datastore,
) -> Vec<PartialSub> {
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
                    let new_val = PartialSubValue::Computed(gel.clone());
                    match new_sub.get(var) {
                        Some(existing) if !psv_eq(existing, &new_val, datastore) => {
                            ok = false;
                            break;
                        }
                        _ => {
                            new_sub.insert(var.clone(), new_val);
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

/// Two substitutions are compatible if they agree on all shared variables.
pub(crate) fn compatible(a: &PartialSub, b: &PartialSub, datastore: &Datastore) -> bool {
    for (var, val_a) in a {
        if let Some(val_b) = b.get(var) {
            if !psv_eq(val_a, val_b, datastore) {
                return false;
            }
        }
    }
    true
}

// ── LIMIT short-circuit budget (issue #165) ─────────────────────────────────

/// The maximum number of solutions the top-level (or subquery) SELECT will
/// ever consume, i.e. `OFFSET + LIMIT`, or `None` when the full solution set
/// is required.
///
/// Returns `None` — disabling the short-circuit — whenever a solution-set
/// modifier must observe every row: no `LIMIT` at all (an `OFFSET` alone is
/// unbounded), `ORDER BY` (sorts the whole set), `GROUP BY` / aggregates
/// (folds every row), or `DISTINCT` (a conservative first pass; counting
/// distinct rows early is legal but not done here). Because SPARQL leaves row
/// order unspecified without `ORDER BY`, returning the first `OFFSET + LIMIT`
/// solutions is a legal — and here byte-identical — selection.
pub(crate) fn select_solution_budget(
    distinct: bool,
    order_by: &[OrderCondition],
    group_by: &[GroupCondition],
    projection: &[ProjectionElement],
    offset: Option<u64>,
    limit: Option<u64>,
) -> Option<usize> {
    let limit = limit? as usize;
    if distinct || !order_by.is_empty() || !group_by.is_empty() {
        return None;
    }
    if projection.iter().any(elem_has_aggregate) {
        return None;
    }
    let offset = offset.map(|o| o as usize).unwrap_or(0);
    Some(offset.saturating_add(limit))
}

#[cfg(test)]
mod partial_sub_value_tests {
    use super::*;
    use num_bigint::BigInt;

    /// #141: the whole reason [`PartialSubValue`] deliberately omits a derived
    /// `PartialEq` is that representation-level equality is wrong — an
    /// `Interned(id)` binding (from a triple-pattern match) and a
    /// `Computed(gel)` binding (from `BIND`/`VALUES`) can denote the *same*
    /// element. [`psv_eq`] must compare by resolved value, so a cross-variant
    /// pair pointing at one interned element compares equal, while distinct
    /// elements do not.
    #[test]
    fn psv_eq_compares_cross_variant_by_resolved_value() {
        let mut ds = Datastore::new(10);
        let resource = GraphElement::GraphLiteral(RdfLiteral::IntegerLiteral(BigInt::from(42)));
        let id = ds.add_resource(resource.clone());
        let other = GraphElement::GraphLiteral(RdfLiteral::IntegerLiteral(BigInt::from(43)));

        let interned = PartialSubValue::Interned(id);
        let computed_same = PartialSubValue::Computed(resource);
        let computed_other = PartialSubValue::Computed(other);

        assert!(
            psv_eq(&interned, &computed_same, &ds),
            "an Interned id and a Computed value denoting the same element must be equal"
        );
        assert!(
            psv_eq(&computed_same, &interned, &ds),
            "psv_eq must be symmetric across variants"
        );
        assert!(
            !psv_eq(&interned, &computed_other, &ds),
            "bindings denoting different elements must not be equal"
        );
    }

    /// #141: [`PartialSubValue::to_id`] must reproduce the pre-refactor
    /// `resource_map.get(gel)` lookup — an `Interned` binding already carries
    /// its id; a `Computed` binding yields an id only when that value happens
    /// to be interned, and `None` for a computed value (e.g. a `BIND`
    /// arithmetic result) that was never added to the store.
    #[test]
    fn to_id_resolves_interned_and_present_computed_but_not_absent() {
        let mut ds = Datastore::new(10);
        let resource = GraphElement::GraphLiteral(RdfLiteral::IntegerLiteral(BigInt::from(7)));
        let id = ds.add_resource(resource.clone());

        assert_eq!(
            PartialSubValue::Interned(id).to_id(&ds),
            Some(id),
            "an Interned binding must return its own id"
        );
        assert_eq!(
            PartialSubValue::Computed(resource).to_id(&ds),
            Some(id),
            "a Computed value that is interned must return the matching id"
        );

        let never_interned =
            GraphElement::GraphLiteral(RdfLiteral::IntegerLiteral(BigInt::from(1_000_001_i64)));
        assert_eq!(
            PartialSubValue::Computed(never_interned).to_id(&ds),
            None,
            "a Computed value never added to the store has no id"
        );
    }
}

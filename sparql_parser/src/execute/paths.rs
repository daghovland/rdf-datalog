/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

use super::bgp::{eval_triple_pattern, resolve_match_term, MatchTerm};
use super::solutions::{partial_subs_equal, psv_eq};
use super::*;

/// Evaluate a property path pattern against the datastore, extending one solution.
/// Zero-hop ("identity") solutions for a path pattern: subject and object
/// must denote the same node. Used by `?` (`ZeroOrOne`) and by the `k == 0`
/// case of bounded repetition (`{0}`, `{0,m}`).
///
/// Per SPARQL 1.1's arbitrary-length-path semantics, when both endpoints are
/// unbound variables the zero-length path connects every node `x` that
/// appears (as a subject or object) in the active graph to itself — see
/// [`graph_nodes`]. When the active graph is itself an unbound `GRAPH ?g`
/// variable, this also enumerates every named graph, binding `?g` per node
/// (mirroring the non-path-pattern `GRAPH ?g` binding behaviour in
/// [`eval_triple_pattern_core`]). See
/// <https://github.com/daghovland/rdf-datalog/issues/203>.
pub(crate) fn zero_hop_solutions(
    subject_term: &Term,
    object_term: &Term,
    sub: &PartialSub,
    datastore: &Datastore,
    active_graph: &ActiveGraph,
) -> Vec<PartialSub> {
    let s_gel = resolve_term_to_gel(subject_term, sub, datastore);
    let o_gel = resolve_term_to_gel(object_term, sub, datastore);
    match (s_gel, o_gel) {
        // Both bound: must be equal
        (Some(s), Some(o)) if s == o => vec![sub.clone()],
        // Both bound but distinct: no zero-hop solution
        (Some(_), Some(_)) => Vec::new(),
        // Subject bound, object unbound: bind object = subject
        (Some(s), None) => {
            if let Term::Variable(v) = object_term {
                let mut new_sub = sub.clone();
                new_sub.insert(v.clone(), PartialSubValue::Computed(s));
                vec![new_sub]
            } else {
                Vec::new()
            }
        }
        // Object bound, subject unbound: bind subject = object
        (None, Some(o)) => {
            if let Term::Variable(v) = subject_term {
                let mut new_sub = sub.clone();
                new_sub.insert(v.clone(), PartialSubValue::Computed(o));
                vec![new_sub]
            } else {
                Vec::new()
            }
        }
        // Both unbound: one solution per node in the active graph, with
        // subject = object = that node.
        (None, None) => match (subject_term, object_term) {
            (Term::Variable(sv), Term::Variable(ov)) => {
                zero_hop_all_nodes(sv, ov, sub, datastore, active_graph)
            }
            _ => Vec::new(),
        },
    }
}

/// All distinct RDF terms appearing as a subject or object of some quad in
/// graph `graph` (`None` = every graph, matching [`Datastore::quads_matching`]'s
/// wildcard convention).
pub(crate) fn graph_nodes(
    datastore: &Datastore,
    graph: Option<GraphElementId>,
) -> HashSet<GraphElement> {
    datastore
        .quads_matching(graph, None, None, None)
        .into_iter()
        .flat_map(|q| {
            [
                datastore.resources.get_graph_element(q.subject).clone(),
                datastore.resources.get_graph_element(q.obj).clone(),
            ]
        })
        .collect()
}

/// Every distinct graph id that owns at least one quad (used to enumerate an
/// unbound `GRAPH ?g` variable). Includes the default graph id if it holds
/// any quads, matching the existing (non-path-pattern) `GRAPH ?g` binding
/// behaviour in [`eval_triple_pattern_core`], which likewise scans with an
/// unconstrained graph argument.
pub(crate) fn distinct_graph_ids(datastore: &Datastore) -> HashSet<GraphElementId> {
    datastore
        .quads_matching(None, None, None, None)
        .into_iter()
        .map(|q| q.triple_id)
        .collect()
}

/// Zero-hop solutions when both path endpoints are unbound variables:
/// bind `subj_var = obj_var = x` for every node `x` in the active graph. If
/// `active_graph` is an unbound `GRAPH ?g` variable, iterate every named
/// graph and bind `?g` accordingly (see [`distinct_graph_ids`]).
pub(crate) fn zero_hop_all_nodes(
    subj_var: &str,
    obj_var: &str,
    sub: &PartialSub,
    datastore: &Datastore,
    active_graph: &ActiveGraph,
) -> Vec<PartialSub> {
    let push_for_graph = |results: &mut Vec<PartialSub>,
                          graph_binding: Option<(&str, GraphElementId)>,
                          gid: Option<GraphElementId>| {
        for gel in graph_nodes(datastore, gid) {
            let mut new_sub = sub.clone();
            if let Some((gvar, id)) = graph_binding {
                new_sub.insert(gvar.to_string(), PartialSubValue::Interned(id));
            }
            new_sub.insert(subj_var.to_string(), PartialSubValue::Computed(gel.clone()));
            new_sub.insert(obj_var.to_string(), PartialSubValue::Computed(gel));
            results.push(new_sub);
        }
    };

    let mut results = Vec::new();
    match active_graph {
        ActiveGraph::Fixed(id) => push_for_graph(&mut results, None, Some(*id)),
        ActiveGraph::Variable(gvar) => match sub.get(gvar).and_then(|val| val.to_id(datastore)) {
            Some(id) => push_for_graph(&mut results, None, Some(id)),
            None => {
                for gid in distinct_graph_ids(datastore) {
                    push_for_graph(&mut results, Some((gvar, gid)), Some(gid));
                }
            }
        },
    }
    results
}

/// Evaluate a bounded/unbounded repetition path (`p{n}`, `p{n,m}`, `p{n,}`,
/// `p{,m}`).
///
/// Unlike `ZeroOrMore`/`OneOrMore`, which use arbitrary-length-path
/// (fixed-point/BFS) semantics — one solution per reachable pair, regardless
/// of how many distinct walks connect it — bounded repetition uses ordinary
/// sequence (join) semantics: `p{k}` is evaluated as a `k`-fold sequence of
/// `p`, so distinct walks of the same length produce distinct (duplicate)
/// solutions. This matches the W3C property-path test expectations (e.g.
/// `data-diamond.ttl` has two distinct 2-hop walks from `:a` to `:z`, and
/// `:a :p{2} ?z` is expected to produce two solutions with `?z = :z`, not
/// one) — see
/// <https://github.com/daghovland/rdf-datalog/issues/203>.
///
/// For an unbounded lower-bounded range (`{n,}`, `max == None`), this is
/// evaluated as `p{n}` followed by `p*`: exactly `n` hops (preserving walk
/// multiplicity, and safe on cyclic data since it's a fixed number of
/// joins) followed by zero-or-more further hops (fixed-point reachability,
/// so cycles don't cause non-termination or multiplicity blow-up).
// The `&Deadline` parameter (#372) pushes this over clippy's default
// too-many-arguments threshold; splitting the existing (subject, path,
// object, sub, datastore, active_graph) tuple into a struct is a bigger,
// unrelated refactor this change intentionally doesn't take on.
#[allow(clippy::too_many_arguments)]
pub(crate) fn eval_repeat_path(
    subject_term: &Term,
    inner: &PropertyPath,
    object_term: &Term,
    sub: PartialSub,
    datastore: &Datastore,
    active_graph: &ActiveGraph,
    range: (usize, Option<usize>),
    deadline: &Deadline,
) -> Result<Vec<PartialSub>, String> {
    let (min, max) = range;
    match max {
        Some(max_n) => {
            if min > max_n {
                return Ok(Vec::new());
            }
            let mut results = Vec::new();
            for k in min..=max_n {
                deadline.check()?;
                results.extend(eval_exact_repeat(
                    subject_term,
                    inner,
                    object_term,
                    sub.clone(),
                    datastore,
                    active_graph,
                    k,
                    deadline,
                )?);
            }
            Ok(results)
        }
        None => {
            // {min,} == inner{min} / inner*
            let mut steps: Vec<PropertyPath> = (0..min).map(|_| inner.clone()).collect();
            steps.push(PropertyPath::ZeroOrMore(Box::new(inner.clone())));
            let seq = PropertyPath::Sequence(steps);
            eval_path_pattern(
                subject_term,
                &seq,
                object_term,
                sub,
                datastore,
                active_graph,
                deadline,
            )
        }
    }
}

/// Evaluate `inner{k}` for an exact, non-negative repeat count `k`.
#[allow(clippy::too_many_arguments)] // see `eval_repeat_path` above (#372)
pub(crate) fn eval_exact_repeat(
    subject_term: &Term,
    inner: &PropertyPath,
    object_term: &Term,
    sub: PartialSub,
    datastore: &Datastore,
    active_graph: &ActiveGraph,
    k: usize,
    deadline: &Deadline,
) -> Result<Vec<PartialSub>, String> {
    if k == 0 {
        Ok(zero_hop_solutions(
            subject_term,
            object_term,
            &sub,
            datastore,
            active_graph,
        ))
    } else {
        let steps: Vec<PropertyPath> = (0..k).map(|_| inner.clone()).collect();
        let seq = PropertyPath::Sequence(steps);
        eval_path_pattern(
            subject_term,
            &seq,
            object_term,
            sub,
            datastore,
            active_graph,
            deadline,
        )
    }
}

pub(crate) fn eval_path_pattern(
    subject_term: &Term,
    path: &PropertyPath,
    object_term: &Term,
    sub: PartialSub,
    datastore: &Datastore,
    active_graph: &ActiveGraph,
    deadline: &Deadline,
) -> Result<Vec<PartialSub>, String> {
    match path {
        PropertyPath::Iri(gel) => {
            let tp = TriplePattern {
                subject: subject_term.clone(),
                predicate: Term::Constant(gel.clone()),
                object: object_term.clone(),
            };
            eval_triple_pattern(&tp, &sub, datastore, active_graph, None, deadline)
        }

        PropertyPath::Sequence(steps) => {
            if steps.is_empty() {
                return Ok(vec![sub]);
            }
            // Chain: introduce fresh bridge variables for intermediate nodes.
            // Each bridge variable is freshly generated (see
            // `fresh_bridge_var`) rather than named positionally, so nested
            // `Sequence` evaluations (e.g. from `eval_repeat_path`'s `{n,}`
            // desugaring) can never collide with an outer `Sequence`'s own
            // bridge/target variables (issue #203, W3C `pp04`).
            let mut current_subject = subject_term.clone();
            let mut current_subs = vec![sub];
            let n = steps.len();
            let mut bridge_names: Vec<String> = Vec::new();
            for (i, step) in steps.iter().enumerate() {
                let current_object = if i + 1 == n {
                    object_term.clone()
                } else {
                    let name = fresh_bridge_var();
                    bridge_names.push(name.clone());
                    Term::Variable(name)
                };
                let mut next_subs = Vec::new();
                for s in current_subs {
                    deadline.check()?;
                    next_subs.extend(eval_path_pattern(
                        &current_subject,
                        step,
                        &current_object,
                        s,
                        datastore,
                        active_graph,
                        deadline,
                    )?);
                }
                current_subs = next_subs;
                current_subject = current_object;
            }
            // Remove internal bridge variables from each solution
            Ok(current_subs
                .into_iter()
                .map(|mut s| {
                    for name in &bridge_names {
                        s.remove(name);
                    }
                    s
                })
                .collect())
        }

        PropertyPath::Alternative(left, right) => {
            let mut left_subs = eval_path_pattern(
                subject_term,
                left,
                object_term,
                sub.clone(),
                datastore,
                active_graph,
                deadline,
            )?;
            let right_subs = eval_path_pattern(
                subject_term,
                right,
                object_term,
                sub,
                datastore,
                active_graph,
                deadline,
            )?;
            left_subs.extend(right_subs);
            Ok(left_subs)
        }

        PropertyPath::Inverse(inner) => {
            // Swap subject and object
            eval_path_pattern(
                object_term,
                inner,
                subject_term,
                sub,
                datastore,
                active_graph,
                deadline,
            )
        }

        PropertyPath::ZeroOrOne(inner) => {
            // Zero hops: subject == object
            let zero_hop =
                zero_hop_solutions(subject_term, object_term, &sub, datastore, active_graph);
            let one_hop = eval_path_pattern(
                subject_term,
                inner,
                object_term,
                sub,
                datastore,
                active_graph,
                deadline,
            )?;
            // Deduplicate (zero-hop and one-hop may produce the same solution).
            // Compare by resolved value: zero-hop bindings are `Computed` while
            // one-hop bindings from a BGP match are `Interned`, so the same
            // logical solution can appear in two representations (#141).
            let mut result = zero_hop;
            for s in one_hop {
                deadline.check()?;
                if !result.iter().any(|r| partial_subs_equal(r, &s, datastore)) {
                    result.push(s);
                }
            }
            Ok(result)
        }

        PropertyPath::OneOrMore(inner) => transitive_closure(
            subject_term,
            inner,
            object_term,
            sub,
            datastore,
            active_graph,
            false,
            deadline,
        ),

        PropertyPath::ZeroOrMore(inner) => transitive_closure(
            subject_term,
            inner,
            object_term,
            sub,
            datastore,
            active_graph,
            true,
            deadline,
        ),

        PropertyPath::Repeat(inner, min, max) => eval_repeat_path(
            subject_term,
            inner,
            object_term,
            sub,
            datastore,
            active_graph,
            (*min, *max),
            deadline,
        ),

        PropertyPath::NegatedSet(excluded) => {
            let g = match active_graph {
                ActiveGraph::Fixed(id) => Some(*id),
                ActiveGraph::Variable(v) => sub.get(v).and_then(|val| val.to_id(datastore)),
            };
            let s_match = resolve_match_term(subject_term, &sub, datastore);
            let o_match = resolve_match_term(object_term, &sub, datastore);
            // See `MatchTerm`: an unsupported endpoint (e.g. a triple term)
            // or a never-interned constant must not silently degrade to an
            // unconstrained wildcard.
            if matches!(s_match, MatchTerm::Never) || matches!(o_match, MatchTerm::Never) {
                return Ok(Vec::new());
            }
            let s = s_match.into_query_arg();
            let o = o_match.into_query_arg();
            let excluded_ids: HashSet<GraphElementId> = excluded
                .iter()
                .filter_map(|gel| datastore.resources.resource_map.get(gel).copied())
                .collect();

            let mut results = Vec::new();
            for quad in datastore.quads_matching(g, s, None, o) {
                deadline.check()?;
                if excluded_ids.contains(&quad.predicate) {
                    continue;
                }
                let mut new_sub = sub.clone();
                let mut ok = true;
                if let Term::Variable(v) = subject_term {
                    let new_val = PartialSubValue::Interned(quad.subject);
                    match new_sub.get(v) {
                        Some(existing) if !psv_eq(existing, &new_val, datastore) => {
                            ok = false;
                        }
                        _ => {
                            new_sub.insert(v.clone(), new_val);
                        }
                    }
                }
                if let Term::Variable(v) = object_term {
                    let new_val = PartialSubValue::Interned(quad.obj);
                    match new_sub.get(v) {
                        Some(existing) if !psv_eq(existing, &new_val, datastore) => {
                            ok = false;
                        }
                        _ => {
                            new_sub.insert(v.clone(), new_val);
                        }
                    }
                }
                if ok {
                    results.push(new_sub);
                }
            }
            Ok(results)
        }
    }
}

/// Resolve a `Term` to a `GraphElement` using the current solution.
pub(crate) fn resolve_term_to_gel(
    term: &Term,
    sub: &PartialSub,
    datastore: &Datastore,
) -> Option<GraphElement> {
    match term {
        Term::Variable(v) => sub.get(v).map(|val| val.resolve(datastore)),
        Term::Constant(gel) => Some(gel.clone()),
        // Property paths over triple-term endpoints are out of scope for
        // phase R3 (#146); treat as unbound so no path steps match.
        Term::TripleTerm(_) => None,
    }
}

/// Compute transitive closure of `path` from `subject_term` to `object_term`.
///
/// `include_zero` = true for `*` (include starting node), false for `+`.
///
/// Strategy: BFS from the subject if it is bound (forward traversal).
/// If the subject is unbound and the object is bound, reverse BFS using ^path.
#[allow(clippy::too_many_arguments)] // see `eval_repeat_path` above (#372)
pub(crate) fn transitive_closure(
    subject_term: &Term,
    path: &PropertyPath,
    object_term: &Term,
    sub: PartialSub,
    datastore: &Datastore,
    active_graph: &ActiveGraph,
    include_zero: bool,
    deadline: &Deadline,
) -> Result<Vec<PartialSub>, String> {
    let subject_gel = resolve_term_to_gel(subject_term, &sub, datastore);
    let object_gel = resolve_term_to_gel(object_term, &sub, datastore);

    // Enumerate all nodes reachable from each concrete starting point
    // by doing BFS with the inner path as a single-hop traversal.
    // Forward BFS: returns all nodes reachable from start_gel.
    // For `include_zero` (p*): includes start_gel itself.
    // For `!include_zero` (p+): excludes start_gel.
    //
    // Issue #372: this `while` loop is the classic "runaway SPARQL query"
    // vector — a transitive-closure property path with no useful bound can
    // do a large amount of work on a big/dense graph before the `visited`
    // cycle guard makes it terminate. Checked every iteration (not batched):
    // it is a whole BGP-shaped `eval_path_pattern` call per pop, so the
    // relative overhead of one extra `Instant::now()` per iteration is
    // negligible next to that.
    let reachable_from = |start_gel: GraphElement| -> Result<Vec<GraphElement>, String> {
        let mut visited: HashSet<GraphElement> = HashSet::new();
        let mut queue = vec![start_gel.clone()];
        while let Some(current) = queue.pop() {
            deadline.check()?;
            let current_term = Term::Constant(current.clone());
            let next_subs = eval_path_pattern(
                &current_term,
                path,
                &Term::Variable("__tc_next".to_string()),
                sub.clone(),
                datastore,
                active_graph,
                deadline,
            )?;
            for s in next_subs {
                if let Some(next_val) = s.get("__tc_next") {
                    let next_gel = next_val.resolve(datastore);
                    if visited.insert(next_gel.clone()) {
                        queue.push(next_gel);
                    }
                }
            }
        }
        if include_zero {
            visited.insert(start_gel);
        }
        Ok(visited.into_iter().collect())
    };

    match (subject_gel, object_gel) {
        (Some(s_gel), Some(o_gel)) => {
            // Both bound: check if object is reachable from subject
            let reachable = reachable_from(s_gel)?;
            if reachable.contains(&o_gel) {
                Ok(vec![sub])
            } else {
                Ok(Vec::new())
            }
        }
        (Some(s_gel), None) => {
            // Subject bound, object unbound: enumerate all reachable nodes
            let reachable = reachable_from(s_gel)?;
            if let Term::Variable(obj_var) = object_term {
                Ok(reachable
                    .into_iter()
                    .map(|gel| {
                        let mut new_sub = sub.clone();
                        new_sub.insert(obj_var.clone(), PartialSubValue::Computed(gel));
                        new_sub
                    })
                    .collect())
            } else {
                Ok(Vec::new())
            }
        }
        (None, Some(o_gel)) => {
            // Object bound, subject unbound: BFS backwards using inverse path.
            // `visited` collects nodes that can reach o_gel in ≥1 hops.
            // For `include_zero` (p*) also include o_gel itself (0 hops).
            let inverse_path = PropertyPath::Inverse(Box::new(path.clone()));
            let reachable = {
                let mut visited: HashSet<GraphElement> = HashSet::new();
                let mut queue = vec![o_gel.clone()];
                while let Some(current) = queue.pop() {
                    deadline.check()?;
                    let current_term = Term::Constant(current.clone());
                    let next_subs = eval_path_pattern(
                        &current_term,
                        &inverse_path,
                        &Term::Variable("__tc_prev".to_string()),
                        sub.clone(),
                        datastore,
                        active_graph,
                        deadline,
                    )?;
                    for s in next_subs {
                        if let Some(prev_val) = s.get("__tc_prev") {
                            let prev_gel = prev_val.resolve(datastore);
                            if visited.insert(prev_gel.clone()) {
                                queue.push(prev_gel);
                            }
                        }
                    }
                }
                if include_zero {
                    visited.insert(o_gel);
                }
                visited
            };
            if let Term::Variable(subj_var) = subject_term {
                Ok(reachable
                    .into_iter()
                    .map(|gel| {
                        let mut new_sub = sub.clone();
                        new_sub.insert(subj_var.clone(), PartialSubValue::Computed(gel));
                        new_sub
                    })
                    .collect())
            } else {
                Ok(Vec::new())
            }
        }
        (None, None) => {
            // Both unbound: enumerate all nodes reachable from any node in
            // each active graph. For each subject node, find all objects
            // reachable from it. This is expensive; for now use the
            // bound-subject BFS for each node.
            //
            // When `active_graph` is an unbound `GRAPH ?g` variable, this
            // must range over every named graph and bind `?g` per graph
            // (rather than collapsing to a single unscoped, un-bound scan,
            // which silently left `?g` unbound in every produced solution —
            // see https://github.com/daghovland/rdf-datalog/issues/203,
            // W3C `pp35`).
            let (subj_var, obj_var) = match (subject_term, object_term) {
                (Term::Variable(s), Term::Variable(o)) => (s, o),
                _ => return Ok(Vec::new()),
            };

            let reachable_from_in = |start_gel: GraphElement,
                                     base_sub: &PartialSub,
                                     ag: &ActiveGraph|
             -> Result<Vec<GraphElement>, String> {
                let mut visited: HashSet<GraphElement> = HashSet::new();
                let mut queue = vec![start_gel.clone()];
                while let Some(current) = queue.pop() {
                    deadline.check()?;
                    let current_term = Term::Constant(current.clone());
                    let next_subs = eval_path_pattern(
                        &current_term,
                        path,
                        &Term::Variable("__tc_next".to_string()),
                        base_sub.clone(),
                        datastore,
                        ag,
                        deadline,
                    )?;
                    for s in next_subs {
                        if let Some(next_val) = s.get("__tc_next") {
                            let next_gel = next_val.resolve(datastore);
                            if visited.insert(next_gel.clone()) {
                                queue.push(next_gel);
                            }
                        }
                    }
                }
                if include_zero {
                    visited.insert(start_gel);
                }
                Ok(visited.into_iter().collect())
            };

            // One (base_sub, resolved_active_graph, graph_id) tuple per
            // graph to scan.
            let graph_scopes: Vec<(PartialSub, ActiveGraph, Option<GraphElementId>)> =
                match active_graph {
                    ActiveGraph::Fixed(id) => vec![(sub.clone(), active_graph.clone(), Some(*id))],
                    ActiveGraph::Variable(v) => {
                        match sub.get(v).and_then(|val| val.to_id(datastore)) {
                            Some(id) => vec![(sub.clone(), active_graph.clone(), Some(id))],
                            None => distinct_graph_ids(datastore)
                                .into_iter()
                                .map(|id| {
                                    let mut s2 = sub.clone();
                                    s2.insert(v.clone(), PartialSubValue::Interned(id));
                                    (s2, ActiveGraph::Fixed(id), Some(id))
                                })
                                .collect(),
                        }
                    }
                };

            let mut results = Vec::new();
            for (base_sub, ag, gid) in graph_scopes {
                let all_subjects = graph_nodes(datastore, gid);
                for s_gel in all_subjects {
                    // The Cartesian-product-shaped outer loop this function
                    // description already flags as "expensive" — check here
                    // too, not just inside the inner BFS, so a query with
                    // many subject nodes but a cheap per-subject BFS still
                    // gets bounded (#372).
                    deadline.check()?;
                    let reachable = reachable_from_in(s_gel.clone(), &base_sub, &ag)?;
                    for o_gel in reachable {
                        let mut new_sub = base_sub.clone();
                        new_sub.insert(subj_var.clone(), PartialSubValue::Computed(s_gel.clone()));
                        new_sub.insert(obj_var.clone(), PartialSubValue::Computed(o_gel));
                        if !results
                            .iter()
                            .any(|r| partial_subs_equal(r, &new_sub, datastore))
                        {
                            results.push(new_sub);
                        }
                    }
                }
            }
            Ok(results)
        }
    }
}

// ── Aggregate helpers ─────────────────────────────────────────────────────────

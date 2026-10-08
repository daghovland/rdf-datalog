/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Runtime per-operator (per-`QueryComponent`, per-triple-pattern) ACTUAL
//! execution timing, collected only when explicitly requested via
//! [`crate::execute::execute_with_profile`] (issue
//! [#572](https://github.com/daghovland/rdf-datalog/issues/572), follow-up
//! to [#537](https://github.com/daghovland/rdf-datalog/issues/537)'s
//! static-only `explain` module).
//!
//! See `docs/plans/EXPLAIN_ENDPOINT_537_PLAN.md`'s "#572" section for the
//! full design. In short:
//!
//! - [`ProfileNode`] is a **separate tree** from [`crate::explain::PlanNode`]
//!   — it is built from real measured timings/row counts during actual
//!   query execution, so it has no fixed positional correspondence to
//!   `PlanNode`'s tree shape. An `OPTIONAL`/`GRAPH` body runs once per outer
//!   row; repeated invocations of the same static body are merged into ONE
//!   node (`invocations` counts how many; `total_time_ms`/`rows_in`/
//!   `rows_out` are sums), not emitted as N siblings.
//! - Collection is thread-local (mirrors the `CURRENT_BASE` pattern already
//!   in `execute/mod.rs`, #346) rather than a new `EvalCtx` field, so
//!   `eval_component`/`eval_components_budgeted`'s signatures are untouched.
//!   Every instrumented call site's cost when inactive (the overwhelmingly
//!   common case — every non-`explain` query) is one `RefCell::borrow()` +
//!   `Option::is_some()` check: no `Instant::now()`, no allocation.
//! - [`ScopeGuard`] is RAII: its `Drop` impl closes a scope even when the
//!   function that opened it returns early via `?` (a deadline timeout or
//!   any other [`crate::error::ExecError`]), so a partial, correctly-scoped
//!   profile survives a failing/timed-out query — mirroring #537's existing
//!   "the static plan survives a failing execution" contract for the
//!   runtime profile.
//! - Granularity is one [`std::time::Instant`] pair per operator
//!   *invocation*, never per solution row or per matched quad — see the
//!   plan doc's "Mechanism" subsection for why this is safe (confirmed by
//!   reading `bgp.rs`/`components.rs`'s existing loop structure before
//!   committing to it).
//! - **Single-slot discipline.** `pending_children` is one `Option`, not a
//!   stack-keyed map: only the function that most recently closed a scope
//!   may consume it, and must do so immediately, before any other profiled
//!   call can overwrite the slot. `QueryComponent` kinds that call their
//!   recursive evaluator more than once per `eval_component` invocation
//!   (`Optional`, `Graph` — once per outer row; `Union` — exactly twice)
//!   take-and-merge explicitly in `components.rs`; kinds that call it at
//!   most once (`Minus`, `Group`, `Subquery`) need no extra code, since the
//!   generic wrapper in `eval_components_budgeted` already consumes
//!   whatever was left pending. Leaf kinds (`Filter`/`Bind`/`Values`/
//!   `PathPattern`/`Service`) explicitly *discard* any pending children —
//!   the guard against a future nested profiled call inside expression
//!   evaluation (e.g. `EXISTS`, not instrumented here — see
//!   [#697](https://github.com/daghovland/rdf-datalog/issues/697)) leaking
//!   onto an unrelated sibling's node.
//!
//! `EXISTS`/`NOT EXISTS` and property-path step-level timing are
//! deliberately out of scope — see #697.

use std::cell::RefCell;
use std::time::{Duration, Instant};

/// One node of the runtime profile tree, as opposed to
/// [`crate::explain::PlanNode`] (the STATIC plan) — see the module doc for
/// why these are two separate trees rather than one merged structure.
#[derive(Debug, Clone, PartialEq)]
pub struct ProfileNode {
    /// Mirrors `PlanNode`'s `"kind"` strings (`"BGP"`, `"Optional"`, ...)
    /// plus `"Pattern"` for one BGP triple pattern and the synthetic
    /// `"UnionLeft"`/`"UnionRight"` wrapper nodes (see module doc).
    pub kind: &'static str,
    /// Rendered detail text, where applicable (mirrors `PlanNode`'s
    /// `detail`/pattern-text fields) — `None` for kinds whose information
    /// is fully captured by `children` (e.g. `"BGP"`, `"UnionLeft"`).
    pub label: Option<String>,
    /// Summed wall-clock time across every invocation, in milliseconds.
    pub total_time_ms: f64,
    /// How many times this exact (static) operator was actually evaluated
    /// and merged into this node. `1` for every kind except `Optional`/
    /// `Graph`, which run once per outer row.
    pub invocations: u32,
    /// Summed input-solution count across every invocation.
    pub rows_in: usize,
    /// Summed output-solution count across every invocation.
    pub rows_out: usize,
    pub children: Vec<ProfileNode>,
}

impl ProfileNode {
    fn new(
        kind: &'static str,
        label: Option<String>,
        rows_in: usize,
        rows_out: usize,
        elapsed: Duration,
        children: Vec<ProfileNode>,
    ) -> Self {
        Self {
            kind,
            label,
            total_time_ms: elapsed.as_secs_f64() * 1000.0,
            invocations: 1,
            rows_in,
            rows_out,
            children,
        }
    }

    /// A zero-timing wrapper node with no row counts of its own, used for
    /// `Union`'s synthetic `"UnionLeft"`/`"UnionRight"` children (see
    /// module doc) — the timing/rows of interest live on their own
    /// `children` subtrees.
    pub(crate) fn wrapper(kind: &'static str, children: Vec<ProfileNode>) -> Self {
        Self {
            kind,
            label: None,
            total_time_ms: 0.0,
            invocations: 1,
            rows_in: 0,
            rows_out: 0,
            children,
        }
    }

    /// Merge `other` into `self` in place: sums time/invocations/rows, and
    /// recursively merges `children` position-wise. Used to collapse
    /// repeated invocations of the same static body (e.g. an `OPTIONAL`
    /// evaluated once per outer row) into one aggregate node.
    fn merge(&mut self, other: ProfileNode) {
        self.total_time_ms += other.total_time_ms;
        self.invocations += other.invocations;
        self.rows_in += other.rows_in;
        self.rows_out += other.rows_out;
        merge_children(&mut self.children, other.children);
    }
}

/// Merge `new` into `acc` in place, position-wise (see [`ProfileNode::merge`]).
pub(crate) fn merge_children(acc: &mut Vec<ProfileNode>, new: Vec<ProfileNode>) {
    for (i, node) in new.into_iter().enumerate() {
        if i < acc.len() {
            acc[i].merge(node);
        } else {
            acc.push(node);
        }
    }
}

struct ProfilerState {
    /// One frame per currently-open [`ScopeGuard`]; each frame collects the
    /// [`ProfileNode`]s completed directly within that scope, in order.
    stack: Vec<Vec<ProfileNode>>,
    /// Single-slot handoff: the child list most recently produced by a
    /// scope closing (a [`ScopeGuard`] dropping). See module doc's
    /// "single-slot discipline".
    pending_children: Option<Vec<ProfileNode>>,
}

thread_local! {
    static PROFILER: RefCell<Option<ProfilerState>> = const { RefCell::new(None) };
}

/// True iff a profiler is active on this thread — i.e. this query is being
/// evaluated via [`crate::execute::execute_with_profile`]. Every
/// instrumentation call site checks this first so the non-`explain` path
/// pays only this one cheap check.
pub(crate) fn is_active() -> bool {
    PROFILER.with(|p| p.borrow().is_some())
}

/// Start collecting a fresh profile on this thread.
pub(crate) fn start() {
    PROFILER.with(|p| {
        let mut p = p.borrow_mut();
        debug_assert!(p.is_none(), "profiler already active on this thread");
        *p = Some(ProfilerState {
            stack: Vec::new(),
            pending_children: None,
        });
    });
}

/// Stop collecting and return the root-level node list — the whole query's
/// profile, including a partial result if the query errored/timed out
/// partway through (every still-open [`ScopeGuard`] already closed via
/// `Drop` during the `?`-propagated unwind, folding its partial frame into
/// `pending_children`; any frame that is somehow still open here — not
/// expected in practice, since this runs after the top-level call has
/// fully returned — is folded in defensively, innermost first).
pub(crate) fn stop_and_take() -> Vec<ProfileNode> {
    PROFILER.with(|p| {
        let mut p = p.borrow_mut();
        let Some(mut state) = p.take() else {
            return Vec::new();
        };
        let mut result = state.pending_children.take().unwrap_or_default();
        while let Some(frame) = state.stack.pop() {
            merge_children(&mut result, frame);
        }
        result
    })
}

/// RAII guard for one nested scope (a sibling list of [`ProfileNode`]s —
/// one `eval_components_budgeted` call's component list, or one
/// `eval_bgp` call's pattern list). See module doc for why closing happens
/// in `Drop` rather than a manual pop call.
pub(crate) struct ScopeGuard {
    active: bool,
}

/// Open a new scope: pushes an empty child-list frame. No-op (returns an
/// inactive guard) if no profiler is active.
pub(crate) fn enter_scope() -> ScopeGuard {
    PROFILER.with(|p| {
        if let Some(state) = p.borrow_mut().as_mut() {
            state.stack.push(Vec::new());
            ScopeGuard { active: true }
        } else {
            ScopeGuard { active: false }
        }
    })
}

impl Drop for ScopeGuard {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        PROFILER.with(|p| {
            if let Some(state) = p.borrow_mut().as_mut() {
                let frame = state.stack.pop().unwrap_or_default();
                state.pending_children = Some(frame);
            }
        });
    }
}

/// Record `node` as a completed child of the scope currently open at the
/// top of the stack. No-op if no profiler is active.
fn record(node: ProfileNode) {
    PROFILER.with(|p| {
        if let Some(state) = p.borrow_mut().as_mut() {
            if let Some(top) = state.stack.last_mut() {
                top.push(node);
            }
        }
    });
}

/// Build a node for one completed operator invocation and record it into
/// the enclosing scope. `has_children` selects whether to consume
/// (`take_pending_children`) or discard (`clear_pending_children`) whatever
/// is pending — see module doc's "single-slot discipline".
pub(crate) fn finish(
    kind: &'static str,
    label: Option<String>,
    rows_in: usize,
    rows_out: usize,
    elapsed: Duration,
    has_children: bool,
) {
    let children = if has_children {
        take_pending_children()
    } else {
        clear_pending_children();
        Vec::new()
    };
    record(ProfileNode::new(
        kind, label, rows_in, rows_out, elapsed, children,
    ));
}

/// Take (and clear) the single pending-children slot — read immediately
/// after the one call that produced it, before any other profiled call can
/// overwrite it.
pub(crate) fn take_pending_children() -> Vec<ProfileNode> {
    PROFILER.with(|p| {
        p.borrow_mut()
            .as_mut()
            .and_then(|s| s.pending_children.take())
            .unwrap_or_default()
    })
}

/// Explicitly discard whatever is pending, without consuming it into a
/// node — see module doc's "single-slot discipline".
pub(crate) fn clear_pending_children() {
    PROFILER.with(|p| {
        if let Some(state) = p.borrow_mut().as_mut() {
            state.pending_children = None;
        }
    });
}

/// Re-set the pending-children slot — used by call sites (`Optional`,
/// `Graph`, `Union` in `components.rs`) that invoke their recursive
/// evaluator more than once per `eval_component` call and must merge
/// across invocations themselves before the generic wrapper in
/// `eval_components_budgeted` consumes the result.
pub(crate) fn set_pending_children(children: Vec<ProfileNode>) {
    PROFILER.with(|p| {
        if let Some(state) = p.borrow_mut().as_mut() {
            state.pending_children = Some(children);
        }
    });
}

/// `Instant::now()`, re-exported so call sites need only `use
/// crate::profile;` rather than also importing `std::time::Instant`
/// themselves purely for this.
pub(crate) fn now() -> Instant {
    Instant::now()
}

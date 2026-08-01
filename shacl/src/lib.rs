/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! SHACL (Shapes Constraint Language) validation.
//!
//! Spec: <https://www.w3.org/TR/shacl/>
//!
//! # Architecture
//!
//! SHACL Core constraints are translated to stratified Datalog rules (same engine as
//! OWL-RL), then materialised over a **clone** of the data graph.  Violation triples
//! derived by the engine are collected into a `ValidationReport`.
//!
//! `sh:closed` is evaluated separately against the original data graph before Datalog
//! materialisation to avoid synthetic helper predicates being mistaken for real data.
//!
//! See `docs/plans/SHACL_PLAN.md` for the phased implementation roadmap.

pub mod evaluate;
pub mod graph;
pub mod path;
pub mod shapes;
pub mod translate;
pub mod vocab;

use dag_rdf::ingress::{DEFAULT_GRAPH_ELEMENT_ID, Triple};
use dag_rdf::{Datastore, GraphElementId};
use datalog::evaluate_rules;
use ingress::RDF_TYPE;
use std::collections::{HashSet, VecDeque};

// ── Public types ──────────────────────────────────────────────────────────────

/// Severity of a SHACL validation result (`sh:resultSeverity`).
///
/// Spec: <https://www.w3.org/TR/shacl/#severity>. A shape's own `sh:severity`
/// determines the severity of every result it produces; the default when
/// unset is `sh:Violation`. `sh:severity` may also be an arbitrary
/// user-defined IRI (SHACL does not restrict its range to the three built-in
/// terms) — `Custom` preserves that IRI verbatim rather than collapsing it to
/// `Violation`. See
/// [#312](https://github.com/daghovland/rdf-datalog/issues/312).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Severity {
    #[default]
    Violation,
    Warning,
    Info,
    /// A `sh:severity` value other than the three built-in terms, keyed by
    /// its full IRI.
    Custom(String),
}

impl Severity {
    /// Parse a `sh:severity` object IRI. The three built-in terms map to
    /// their named variants; any other IRI becomes `Severity::Custom` (SHACL
    /// does not restrict `sh:severity`'s range), so this always returns
    /// `Some`.
    pub fn from_iri(iri: &str) -> Option<Self> {
        Some(match iri {
            vocab::SH_VIOLATION => Severity::Violation,
            vocab::SH_WARNING => Severity::Warning,
            vocab::SH_INFO => Severity::Info,
            other => Severity::Custom(other.to_string()),
        })
    }

    /// The full `sh:resultSeverity` object IRI for this severity.
    pub fn iri(&self) -> &str {
        match self {
            Severity::Violation => vocab::SH_VIOLATION,
            Severity::Warning => vocab::SH_WARNING,
            Severity::Info => vocab::SH_INFO,
            Severity::Custom(iri) => iri.as_str(),
        }
    }

    /// The `sh:` term name (`"sh:Violation"`, …), or `<iri>` for a custom
    /// severity, used when serialising a report.
    pub fn turtle_term(&self) -> String {
        match self {
            Severity::Violation => "sh:Violation".to_string(),
            Severity::Warning => "sh:Warning".to_string(),
            Severity::Info => "sh:Info".to_string(),
            Severity::Custom(iri) => format!("<{iri}>"),
        }
    }
}

/// Per-violation-predicate metadata, threaded alongside a `Severity` from
/// rule/constraint-generation time (`translate.rs`/`evaluate.rs`) through to
/// `collect_violations`, mirroring the way `Severity` itself is threaded
/// through `shapes_to_rules`/`eval_all`/`pre_compute_violations`. One
/// `ViolMeta` describes every violation triple sharing a given synthetic
/// violation predicate — which is always exactly one shape/path/constraint
/// combination (see `vocab::viol_*`). See
/// [#264](https://github.com/daghovland/rdf-datalog/issues/264).
#[derive(Debug, Clone)]
pub struct ViolMeta {
    pub severity: Severity,
    /// Display form of the actual shape node that produced this violation
    /// (`sh:sourceShape`) — an IRI, or a blank-node label (`_:bN`) if the
    /// shape is anonymous. For a property-shape-scoped violation this is the
    /// **property shape's own node** (the object of `sh:property`, which is
    /// commonly a named IRI in real-world SHACL, not necessarily a blank
    /// node), never the enclosing node shape — see
    /// `shapes::ParsedPropShape::shapes_id`. Always resolvable (every shape
    /// node has a display form, IRI or blank node), so unlike the field this
    /// replaced, never `None`. See
    /// [#264](https://github.com/daghovland/rdf-datalog/issues/264).
    pub source_shape: String,
    /// `sh:path` of the property shape that produced this violation, or
    /// `None` for a node-level (pathless) constraint.
    pub path: Option<String>,
    /// `sh:sourceConstraintComponent` IRI for the constraint that produced
    /// this violation.
    pub component: &'static str,
    /// `sh:message` declared on the producing shape, if any.
    pub message: Option<String>,
}

impl ViolMeta {
    /// `source_shape_id` is the shapes-graph node that actually declares the
    /// constraint responsible for this violation: the property shape's own
    /// node for a property-shape-scoped violation, or `shape.shapes_id`
    /// itself for a node-level constraint. `shape` supplies `severity`/
    /// `message` (shape-level, node-shape concerns) unless overridden by a
    /// property shape's own `sh:severity` — see `new_with_severity_override`.
    fn new(
        shapes_store: &Datastore,
        shape: &shapes::ParsedShape,
        source_shape_id: GraphElementId,
        path: Option<&str>,
        component: &'static str,
    ) -> Self {
        Self::new_with_severity_override(
            shapes_store,
            shape,
            source_shape_id,
            path,
            component,
            None,
        )
    }

    /// As [`new`](Self::new), but `severity_override` — a property shape's
    /// own `sh:severity`, when declared — takes precedence over the parent
    /// node shape's severity. See
    /// [#312](https://github.com/daghovland/rdf-datalog/issues/312).
    fn new_with_severity_override(
        shapes_store: &Datastore,
        shape: &shapes::ParsedShape,
        source_shape_id: GraphElementId,
        path: Option<&str>,
        component: &'static str,
        severity_override: Option<Severity>,
    ) -> Self {
        ViolMeta {
            severity: severity_override.unwrap_or_else(|| shape.severity.clone()),
            source_shape: graph::element_display(shapes_store, source_shape_id),
            path: path.map(str::to_string),
            component,
            message: shape.message.clone(),
        }
    }
}

/// A single validation result entry (`sh:ValidationResult`).
#[derive(Debug, Clone)]
pub struct ValidationResult {
    pub focus_node: Option<String>,
    pub severity: Severity,
    pub message: Option<String>,
    pub result_path: Option<String>,
    pub source_shape: String,
    pub source_constraint: Option<String>,
    pub value: Option<String>,
}

/// The outcome of validating a data graph against a shapes graph (`sh:ValidationReport`).
#[derive(Debug, Clone)]
pub struct ValidationReport {
    pub conforms: bool,
    pub results: Vec<ValidationResult>,
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Validate `data` against the SHACL shapes in `shapes`.
///
/// The data graph is cloned; the caller's `data` is not mutated.
pub fn validate(data: &Datastore, shapes: &Datastore) -> Result<ValidationReport, String> {
    let parsed = shapes::parse_shapes(shapes);

    // Static cycle check over the shapes graph alone (sh:not/sh:and/sh:or/sh:xone/
    // sh:node/sh:qualifiedValueShape references), done once here rather than
    // guarded per-node at evaluation time — the cycle is a property of the
    // shapes graph, independent of what data gets validated against it. See
    // https://github.com/daghovland/rdf-datalog/issues/278.
    if let Some(cycle) = shapes::find_shape_reference_cycle(shapes, &parsed) {
        return Err(shapes::describe_shape_cycle(shapes, &cycle));
    }

    // A literal `sh:targetNode` value is a focus node regardless of whether
    // it independently occurs anywhere in the data graph — the shapes graph
    // and data graph are ordinarily different documents. IRI/blank-node
    // `sh:targetNode` values already resolved correctly before this fix
    // (`evaluate::lookup_elem_value` looked them up directly against `data`'s
    // existing resource map), so only a literal `Target::Node` needs the
    // augmented copy below; skip the clone entirely otherwise; this
    // validate() runs on a hot path and a `Datastore` clone is not free.
    //
    // Intern every literal `Target::Node` value into an augmented copy of
    // `data` up front so that `data_targets`'s call to
    // `evaluate::lookup_elem_value` below always finds it, even though it
    // appears only in the shapes graph.
    // `translate::intern_elem` is idempotent (backed by `add_resource`), and
    // Phase 1's `translate::shapes_to_rules` already interns the same values
    // into `work` independently — this keeps the read-only `data` view
    // consistent with it. See
    // [#310](https://github.com/daghovland/rdf-datalog/issues/310).
    let literal_node_targets: Vec<&shapes::ElemValue> = parsed
        .iter()
        .flat_map(|shape| &shape.targets)
        .filter_map(|target| match target {
            shapes::Target::Node(elem @ shapes::ElemValue::Literal { .. }) => Some(elem),
            _ => None,
        })
        .collect();
    let owned_data;
    let data: &Datastore = if literal_node_targets.is_empty() {
        data
    } else {
        let mut augmented = data.clone();
        for elem in literal_node_targets {
            translate::intern_elem(elem, &mut augmented);
        }
        owned_data = augmented;
        &owned_data
    };

    let mut work = data.clone();

    // Pre-compute violations for constraints that must see only the original data triples
    // (before any Datalog materialisation adds synthetic helper predicates).
    let mut all_viol_preds = pre_compute_violations(&parsed, data, shapes, &mut work);

    // Translate remaining constraints to Datalog rules and materialise.
    let (rules, rule_viols) = translate::shapes_to_rules(&parsed, shapes, &mut work);
    // A SHACL constraint should never compile to a Datalog Contradiction rule
    // (SHACL violations are represented as synthetic marker predicates, not
    // via RuleHead::Contradiction), so this should not fail in practice. See
    // https://github.com/daghovland/rdf-datalog/issues/301.
    evaluate_rules(rules, &mut work)
        .map_err(|e| format!("unexpected contradiction while validating SHACL shapes: {e}"))?;
    all_viol_preds.extend(rule_viols);

    let results = collect_violations(&work, &all_viol_preds);
    Ok(ValidationReport {
        conforms: results.is_empty(),
        results,
    })
}

/// Serialize a `ValidationReport` as a Turtle string (SHACL report graph).
///
/// Spec: <https://www.w3.org/TR/shacl/#validation-report>
pub fn report_to_turtle(report: &ValidationReport) -> String {
    let mut out = String::new();
    out.push_str("@prefix sh: <http://www.w3.org/ns/shacl#> .\n");
    out.push_str("@prefix xsd: <http://www.w3.org/2001/XMLSchema#> .\n\n");
    out.push_str("[] a sh:ValidationReport ;\n");
    if report.conforms {
        out.push_str("   sh:conforms true .\n");
    } else {
        out.push_str("   sh:conforms false");
        for result in &report.results {
            out.push_str(" ;\n   sh:result [\n");
            out.push_str("       a sh:ValidationResult ;\n");
            out.push_str("       sh:resultSeverity ");
            out.push_str(&result.severity.turtle_term());
            if let Some(focus) = &result.focus_node {
                out.push_str(" ;\n       sh:focusNode ");
                out.push_str(&turtle_term(focus));
            }
            if let Some(path) = &result.result_path {
                out.push_str(" ;\n       sh:resultPath ");
                out.push_str(&turtle_term(path));
            }
            if let Some(val) = &result.value {
                out.push_str(" ;\n       sh:value ");
                out.push_str(&turtle_term(val));
            }
            out.push_str(" ;\n       sh:sourceShape ");
            out.push_str(&turtle_term(&result.source_shape));
            if let Some(component) = &result.source_constraint {
                out.push_str(" ;\n       sh:sourceConstraintComponent ");
                out.push_str(&turtle_term(component));
            }
            if let Some(msg) = &result.message {
                out.push_str(" ;\n       sh:resultMessage ");
                out.push('"');
                out.push_str(&msg.replace('"', "\\\""));
                out.push('"');
            }
            out.push_str("\n   ]");
        }
        out.push_str(" .\n");
    }
    out
}

/// Format a value as a Turtle term: a literal already in genuine Turtle
/// syntax (as-is, no re-quoting — see below), IRI `<…>`, blank node `_:…`
/// (as-is, no quoting — needed since `sh:sourceShape` can now be a
/// blank-node display string, see `graph::element_display`), or a plain
/// string that still needs quoting (e.g. `sh:resultMessage`, which is never
/// routed through `element_display`).
///
/// The literal check must come first and is unambiguous: `element_display`
/// now renders every literal as real Turtle literal syntax starting with
/// `"` (via `turtle::format_literal`, [#337](https://github.com/daghovland/rdf-datalog/issues/337)),
/// and no IRI/blank-node display string can start with `"`. Passing such a
/// string through unchanged (rather than wrapping it in another layer of
/// quotes) is what keeps `sh:value`/`sh:focusNode`/etc. correctly typed
/// (`"5"^^<xsd:integer>`, not `"\"5\"^^<xsd:integer>\""`).
fn turtle_term(s: &str) -> String {
    if s.starts_with('"') {
        s.to_string()
    } else if s.starts_with('<')
        || s.starts_with("http://")
        || s.starts_with("https://")
        || s.starts_with("urn:")
    {
        let iri = s.trim_start_matches('<').trim_end_matches('>');
        format!("<{iri}>")
    } else if s.starts_with("_:") {
        s.to_string()
    } else {
        format!("\"{}\"", s.replace('"', "\\\""))
    }
}

/// Build a `ValidationReport` as a `Datastore` graph (SHACL validation-report
/// shape), as RDF quads directly — no stringify/reparse round trip through
/// [`report_to_turtle`]. Mirrors that function's field-by-field emission
/// exactly, including that no `sh:result` triples are emitted when
/// `report.conforms` is `true` (even if `report.results` is non-empty).
///
/// Blank-node labels embedded in `ValidationResult` string fields (e.g.
/// `_:b3`, produced by `graph::element_display`) are re-interned
/// consistently: two fields sharing the same label resolve to the same
/// `GraphElementId` in the returned store. See
/// [#314](https://github.com/daghovland/rdf-datalog/issues/314).
pub fn report_to_datastore(report: &ValidationReport) -> Datastore {
    let mut ds = Datastore::new(64);
    let rdf_type = graph::intern_iri(&mut ds, RDF_TYPE);
    let sh_validation_report = graph::intern_iri(&mut ds, vocab::SH_VALIDATION_REPORT);
    let sh_conforms = graph::intern_iri(&mut ds, vocab::SH_CONFORMS);

    let report_node = ds.new_anonymous_blank_node();
    ds.add_triple(Triple {
        subject: report_node,
        predicate: rdf_type,
        obj: sh_validation_report,
    });
    let conforms_val =
        ds.add_literal_resource(dag_rdf::RdfLiteral::BooleanLiteral(report.conforms));
    ds.add_triple(Triple {
        subject: report_node,
        predicate: sh_conforms,
        obj: conforms_val,
    });

    if !report.conforms {
        let sh_result = graph::intern_iri(&mut ds, vocab::SH_RESULT);
        let sh_validation_result = graph::intern_iri(&mut ds, vocab::SH_VALIDATION_RESULT);
        let sh_result_severity = graph::intern_iri(&mut ds, vocab::SH_RESULT_SEVERITY);
        let sh_focus_node = graph::intern_iri(&mut ds, vocab::SH_FOCUS_NODE);
        let sh_result_path = graph::intern_iri(&mut ds, vocab::SH_RESULT_PATH);
        let sh_value = graph::intern_iri(&mut ds, vocab::SH_VALUE);
        let sh_source_shape = graph::intern_iri(&mut ds, vocab::SH_SOURCE_SHAPE);
        let sh_source_constraint_component =
            graph::intern_iri(&mut ds, vocab::SH_SOURCE_CONSTRAINT_COMPONENT);
        let sh_result_message = graph::intern_iri(&mut ds, vocab::SH_RESULT_MESSAGE);

        for result in &report.results {
            let result_node = ds.new_anonymous_blank_node();
            ds.add_triple(Triple {
                subject: report_node,
                predicate: sh_result,
                obj: result_node,
            });
            ds.add_triple(Triple {
                subject: result_node,
                predicate: rdf_type,
                obj: sh_validation_result,
            });

            // sh:resultSeverity: always a full IRI (Severity::iri()), never a
            // string/blank-node label, unlike the generic term fields below —
            // interned directly rather than through `intern_result_term`.
            let severity_id = graph::intern_iri(&mut ds, result.severity.iri());
            ds.add_triple(Triple {
                subject: result_node,
                predicate: sh_result_severity,
                obj: severity_id,
            });

            if let Some(focus) = &result.focus_node {
                let id = intern_result_term(&mut ds, focus);
                ds.add_triple(Triple {
                    subject: result_node,
                    predicate: sh_focus_node,
                    obj: id,
                });
            }
            if let Some(path) = &result.result_path {
                let id = intern_result_term(&mut ds, path);
                ds.add_triple(Triple {
                    subject: result_node,
                    predicate: sh_result_path,
                    obj: id,
                });
            }
            if let Some(val) = &result.value {
                let id = intern_result_term(&mut ds, val);
                ds.add_triple(Triple {
                    subject: result_node,
                    predicate: sh_value,
                    obj: id,
                });
            }
            let source_shape_id = intern_result_term(&mut ds, &result.source_shape);
            ds.add_triple(Triple {
                subject: result_node,
                predicate: sh_source_shape,
                obj: source_shape_id,
            });

            if let Some(component) = &result.source_constraint {
                let id = intern_result_term(&mut ds, component);
                ds.add_triple(Triple {
                    subject: result_node,
                    predicate: sh_source_constraint_component,
                    obj: id,
                });
            }
            if let Some(msg) = &result.message {
                let id = ds.add_literal_resource(dag_rdf::RdfLiteral::LiteralString(msg.clone()));
                ds.add_triple(Triple {
                    subject: result_node,
                    predicate: sh_result_message,
                    obj: id,
                });
            }
        }
    }
    ds
}

/// Intern a `ValidationResult` string field (focus node / path / value /
/// source shape / source-constraint-component) into `ds`, classifying it
/// exactly as [`turtle_term`] does for the text serializer: genuine Turtle
/// literal syntax (`"…"^^<…>` / `"…"@…` / `"…"`, produced by
/// `graph::element_display` — see [#337](https://github.com/daghovland/rdf-datalog/issues/337)),
/// an IRI (`<…>`, `http://…`, `https://…`, `urn:…`), or a blank-node label
/// (`_:…`, re-interned consistently by label via
/// `get_or_create_named_anon_resource` so the same label always resolves to
/// the same node within one `report_to_datastore` call).
///
/// The literal branch actually parses the Turtle syntax back into a proper
/// `RdfLiteral::TypedLiteral`/`LangLiteral`/`LiteralString` (via
/// `turtle::parse_literal_term`) rather than wrapping the whole string as an
/// opaque `LiteralString` — so a typed/lang-tagged `sh:value` etc. survives
/// `report_to_datastore` with its datatype/language intact. Falls back to a
/// plain string literal if `s` starts with `"` but is not parseable Turtle
/// literal syntax (defensive; `element_display` never produces such a
/// string, so this should not happen in practice).
fn intern_result_term(ds: &mut Datastore, s: &str) -> GraphElementId {
    if s.starts_with('"') {
        let lit = turtle::parse_literal_term(s)
            .unwrap_or_else(|| dag_rdf::RdfLiteral::LiteralString(s.to_string()));
        ds.add_literal_resource(lit)
    } else if s.starts_with('<')
        || s.starts_with("http://")
        || s.starts_with("https://")
        || s.starts_with("urn:")
    {
        let iri = s.trim_start_matches('<').trim_end_matches('>');
        graph::intern_iri(ds, iri)
    } else if let Some(label) = s.strip_prefix("_:") {
        ds.resources
            .get_or_create_named_anon_resource(label.to_string())
    } else {
        ds.add_literal_resource(dag_rdf::RdfLiteral::LiteralString(s.to_string()))
    }
}

// ── Pre-compute violations ────────────────────────────────────────────────────

/// Evaluate constraints that need the original (un-materialised) data graph.
///
/// Handles `sh:closed` and all Phase 2 value-testing constraints (datatype,
/// nodeKind, range, string, property pair, sh:node, sh:qualifiedValueShape, sh:xone).
/// Returns the violation-predicate IDs added.
fn pre_compute_violations(
    parsed: &[shapes::ParsedShape],
    data: &Datastore,
    shapes_store: &Datastore,
    work: &mut Datastore,
) -> Vec<(GraphElementId, ViolMeta)> {
    let mut viol_preds = Vec::new();
    for shape in parsed {
        // sh:deactivated — a deactivated shape produces no results at all,
        // including from sh:closed. See #262.
        if shape.deactivated {
            continue;
        }
        if let Some(allowed_iris) = &shape.closed {
            viol_preds.extend(closed_violations(
                shape,
                allowed_iris,
                data,
                shapes_store,
                work,
            ));
        }
    }
    let phase2_viols = evaluate::eval_all(parsed, data, shapes_store, work);
    viol_preds.extend(phase2_viols);
    viol_preds
}

/// Compute `sh:closed` violations directly from the data graph.
///
/// Each `(focusNode, forbiddenPredicate, value)` triple that occurs in the
/// data becomes one violation triple, carrying the real value as the object
/// (so `sh:value` is correct) — the offending predicate itself is instead
/// encoded into a per-predicate synthetic violation predicate (see
/// `vocab::viol_closed`) so that `sh:resultPath`, threaded through this
/// entry's own `ViolMeta`, can vary per offending predicate even though a
/// single shape can have several. Because we query `data` (before any
/// Datalog derivation), synthetic helper predicates added to `work` are
/// never seen. See [#308](https://github.com/daghovland/rdf-datalog/issues/308).
fn closed_violations(
    shape: &shapes::ParsedShape,
    allowed_iris: &[String],
    data: &Datastore,
    shapes_store: &Datastore,
    work: &mut Datastore,
) -> Vec<(GraphElementId, ViolMeta)> {
    // IDs of allowed predicates in the DATA store.
    let allowed: HashSet<GraphElementId> = allowed_iris
        .iter()
        .filter_map(|iri| graph::lookup_iri(data, iri))
        .collect();

    // One violation predicate per distinct offending predicate encountered,
    // interned lazily as we scan the data.
    let mut viol_preds: std::collections::HashMap<GraphElementId, GraphElementId> =
        std::collections::HashMap::new();
    let mut metas: Vec<(GraphElementId, ViolMeta)> = Vec::new();

    for node_id in data_targets(shape, data) {
        for triple in data.get_triples_with_subject(node_id) {
            if allowed.contains(&triple.predicate) {
                continue;
            }
            let viol_pred = *viol_preds.entry(triple.predicate).or_insert_with(|| {
                let pred =
                    graph::intern_iri(work, &vocab::viol_closed(shape.idx, triple.predicate));
                let path = graph::element_display(data, triple.predicate);
                metas.push((
                    pred,
                    ViolMeta::new(
                        shapes_store,
                        shape,
                        shape.shapes_id,
                        Some(&path),
                        vocab::CC_CLOSED,
                    ),
                ));
                pred
            });
            // node_id and triple.obj are valid IDs in `work` because `work`
            // is a clone of `data` (same resource list, same IDs).
            work.add_triple(Triple {
                subject: node_id,
                predicate: viol_pred,
                obj: triple.obj,
            });
        }
    }
    metas
}

// ── Target computation from original data ─────────────────────────────────────

/// Compute the focus nodes for `shape` directly from the `data` store.
fn data_targets(shape: &shapes::ParsedShape, data: &Datastore) -> Vec<GraphElementId> {
    let mut nodes: Vec<GraphElementId> = Vec::new();

    for target in &shape.targets {
        match target {
            shapes::Target::Node(elem) => {
                // Literal-valued sh:targetNode (e.g. `sh:targetNode 32`) is
                // resolved the same way as any other literal shapes-graph
                // value referenced against `data` — see #312.
                if let Some(id) = evaluate::lookup_elem_value(data, elem) {
                    push_unique(&mut nodes, id);
                }
            }
            shapes::Target::Class(class_iri) | shapes::Target::ImplicitClass(class_iri) => {
                for id in class_target_instances(data, class_iri) {
                    push_unique(&mut nodes, id);
                }
            }
            shapes::Target::SubjectsOf(pred_iri) => {
                if let Some(pred_id) = graph::lookup_iri(data, pred_iri) {
                    for t in data.get_triples_with_predicate(pred_id) {
                        push_unique(&mut nodes, t.subject);
                    }
                }
            }
            shapes::Target::ObjectsOf(pred_iri) => {
                if let Some(pred_id) = graph::lookup_iri(data, pred_iri) {
                    for t in data.get_triples_with_predicate(pred_id) {
                        push_unique(&mut nodes, t.obj);
                    }
                }
            }
        }
    }
    nodes
}

/// Resolve a `sh:targetClass`/implicit-class-target IRI to its focus nodes in
/// `data`: every node whose `rdf:type` is `class_iri` or a (transitive)
/// `rdfs:subClassOf` subclass of it, per SHACL spec §2.1.3.1
/// (`?this rdf:type/rdfs:subClassOf* $class`). Shared between the direct
/// Phase-2 evaluation path (`data_targets`, above) and the Datalog
/// rule-generation path (`translate::target_rules`) — both need identical
/// target semantics. See [#312](https://github.com/daghovland/rdf-datalog/issues/312).
pub(crate) fn class_target_instances(data: &Datastore, class_iri: &str) -> Vec<GraphElementId> {
    let mut nodes = Vec::new();
    let (Some(rdf_type_id), Some(class_id)) = (
        graph::lookup_iri(data, RDF_TYPE),
        graph::lookup_iri(data, class_iri),
    ) else {
        return nodes;
    };
    for c in class_and_subclasses(data, class_id) {
        for t in data.get_triples_with_object_predicate(c, rdf_type_id) {
            push_unique(&mut nodes, t.subject);
        }
    }
    nodes
}

/// Return `class_id` together with every class transitively related to it by
/// `rdfs:subClassOf` (`{c}` if `data` has no such triples at all — the
/// no-hierarchy case degenerates to the direct-instances-only behaviour this
/// replaced). Used to resolve `sh:targetClass`/implicit class targets per
/// SHACL spec §2.1.3.1 (`?this rdf:type/rdfs:subClassOf* $class`). See #312.
fn class_and_subclasses(data: &Datastore, class_id: GraphElementId) -> HashSet<GraphElementId> {
    let mut classes = HashSet::new();
    classes.insert(class_id);
    let Some(sub_class_of_id) = graph::lookup_iri(data, ingress::RDFS_SUB_CLASS_OF) else {
        return classes;
    };
    let mut queue: VecDeque<GraphElementId> = VecDeque::new();
    queue.push_back(class_id);
    while let Some(c) = queue.pop_front() {
        // t.subject rdfs:subClassOf c  =>  t.subject is a (transitive) subclass of c
        for t in data.get_triples_with_object_predicate(c, sub_class_of_id) {
            if classes.insert(t.subject) {
                queue.push_back(t.subject);
            }
        }
    }
    classes
}

fn push_unique(vec: &mut Vec<GraphElementId>, id: GraphElementId) {
    if !vec.contains(&id) {
        vec.push(id);
    }
}

// ── Violation collection ──────────────────────────────────────────────────────

fn collect_violations(
    work: &Datastore,
    viol_preds: &[(GraphElementId, ViolMeta)],
) -> Vec<ValidationResult> {
    let pred_meta: std::collections::HashMap<GraphElementId, &ViolMeta> =
        viol_preds.iter().map(|(id, meta)| (*id, meta)).collect();
    // Only examine default-graph triples (triple_id = 0).
    work.named_graphs
        .get_graph(DEFAULT_GRAPH_ELEMENT_ID)
        .filter(|q| pred_meta.contains_key(&q.predicate))
        .map(|q| {
            let focus = graph::element_display(work, q.subject);
            let meta = pred_meta.get(&q.predicate).copied();
            // sh:MinCount/sh:MaxCount/sh:UniqueLang violations are about the
            // cardinality/shape of an entire value *set*, not any single
            // value — the SHACL spec's validator definitions for these three
            // components never populate sh:value, and no fixture in the W3C
            // SHACL test suite (`tests/testdata/w3c_shacl/core/`) ever
            // expects one. Suppress it here rather than reporting an
            // arbitrary witness value that would (falsely) imply a single
            // value caused the violation. See
            // [#313](https://github.com/daghovland/rdf-datalog/issues/313).
            let suppress_value = matches!(
                meta.map(|m| m.component),
                Some(vocab::CC_MIN_COUNT) | Some(vocab::CC_MAX_COUNT) | Some(vocab::CC_UNIQUE_LANG)
            );
            let val = if suppress_value {
                None
            } else {
                let s = graph::element_display(work, q.obj);
                if s == vocab::INT_NIL || s == vocab::INT_TRUE {
                    None
                } else {
                    Some(s)
                }
            };
            ValidationResult {
                focus_node: Some(focus),
                severity: meta.map(|m| m.severity.clone()).unwrap_or_default(),
                message: meta.and_then(|m| m.message.clone()),
                result_path: meta.and_then(|m| m.path.clone()),
                source_shape: meta.map(|m| m.source_shape.clone()).unwrap_or_default(),
                source_constraint: meta.map(|m| m.component.to_string()),
                value: val,
            }
        })
        .collect()
}

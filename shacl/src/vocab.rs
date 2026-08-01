/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! SHACL namespace IRIs and synthetic marker IRIs used by the validator.
//!
//! Spec: <https://www.w3.org/TR/shacl/>

// ── SHACL namespace ───────────────────────────────────────────────────────────

pub const SH: &str = "http://www.w3.org/ns/shacl#";

pub const SH_NODE_SHAPE: &str = "http://www.w3.org/ns/shacl#NodeShape";
pub const SH_PROPERTY_SHAPE: &str = "http://www.w3.org/ns/shacl#PropertyShape";

// §2 Targets
pub const SH_TARGET_CLASS: &str = "http://www.w3.org/ns/shacl#targetClass";
pub const SH_TARGET_NODE: &str = "http://www.w3.org/ns/shacl#targetNode";
pub const SH_TARGET_SUBJECTS_OF: &str = "http://www.w3.org/ns/shacl#targetSubjectsOf";
pub const SH_TARGET_OBJECTS_OF: &str = "http://www.w3.org/ns/shacl#targetObjectsOf";

// §3 Properties
pub const SH_PROPERTY: &str = "http://www.w3.org/ns/shacl#property";
pub const SH_PATH: &str = "http://www.w3.org/ns/shacl#path";

// §2.3.1 Property paths — https://www.w3.org/TR/shacl/#property-paths
pub const SH_INVERSE_PATH: &str = "http://www.w3.org/ns/shacl#inversePath";
pub const SH_ALTERNATIVE_PATH: &str = "http://www.w3.org/ns/shacl#alternativePath";
pub const SH_ZERO_OR_MORE_PATH: &str = "http://www.w3.org/ns/shacl#zeroOrMorePath";
pub const SH_ONE_OR_MORE_PATH: &str = "http://www.w3.org/ns/shacl#oneOrMorePath";
pub const SH_ZERO_OR_ONE_PATH: &str = "http://www.w3.org/ns/shacl#zeroOrOnePath";

// §4.1 Value type
pub const SH_CLASS: &str = "http://www.w3.org/ns/shacl#class";
pub const SH_DATATYPE: &str = "http://www.w3.org/ns/shacl#datatype";
pub const SH_NODE_KIND: &str = "http://www.w3.org/ns/shacl#nodeKind";
pub const SH_IRI: &str = "http://www.w3.org/ns/shacl#IRI";
pub const SH_LITERAL: &str = "http://www.w3.org/ns/shacl#Literal";
pub const SH_BLANK_NODE: &str = "http://www.w3.org/ns/shacl#BlankNode";
pub const SH_BLANK_NODE_OR_IRI: &str = "http://www.w3.org/ns/shacl#BlankNodeOrIRI";
pub const SH_BLANK_NODE_OR_LITERAL: &str = "http://www.w3.org/ns/shacl#BlankNodeOrLiteral";
pub const SH_IRI_OR_LITERAL: &str = "http://www.w3.org/ns/shacl#IRIOrLiteral";

// §4.2 Cardinality
pub const SH_MIN_COUNT: &str = "http://www.w3.org/ns/shacl#minCount";
pub const SH_MAX_COUNT: &str = "http://www.w3.org/ns/shacl#maxCount";

// §4.3 Value range
pub const SH_MIN_INCLUSIVE: &str = "http://www.w3.org/ns/shacl#minInclusive";
pub const SH_MAX_INCLUSIVE: &str = "http://www.w3.org/ns/shacl#maxInclusive";
pub const SH_MIN_EXCLUSIVE: &str = "http://www.w3.org/ns/shacl#minExclusive";
pub const SH_MAX_EXCLUSIVE: &str = "http://www.w3.org/ns/shacl#maxExclusive";

// §4.4 String-based
pub const SH_MIN_LENGTH: &str = "http://www.w3.org/ns/shacl#minLength";
pub const SH_MAX_LENGTH: &str = "http://www.w3.org/ns/shacl#maxLength";
pub const SH_PATTERN: &str = "http://www.w3.org/ns/shacl#pattern";
pub const SH_FLAGS: &str = "http://www.w3.org/ns/shacl#flags";
pub const SH_LANGUAGE_IN: &str = "http://www.w3.org/ns/shacl#languageIn";
pub const SH_UNIQUE_LANG: &str = "http://www.w3.org/ns/shacl#uniqueLang";

// §4.5 Property pair
pub const SH_EQUALS: &str = "http://www.w3.org/ns/shacl#equals";
pub const SH_DISJOINT: &str = "http://www.w3.org/ns/shacl#disjoint";
pub const SH_LESS_THAN: &str = "http://www.w3.org/ns/shacl#lessThan";
pub const SH_LESS_THAN_OR_EQUALS: &str = "http://www.w3.org/ns/shacl#lessThanOrEquals";

// §4.6 Logical
pub const SH_NOT: &str = "http://www.w3.org/ns/shacl#not";
pub const SH_AND: &str = "http://www.w3.org/ns/shacl#and";
pub const SH_OR: &str = "http://www.w3.org/ns/shacl#or";
pub const SH_XONE: &str = "http://www.w3.org/ns/shacl#xone";

// §4.7 Shape-based
pub const SH_NODE: &str = "http://www.w3.org/ns/shacl#node";
pub const SH_QUALIFIED_VALUE_SHAPE: &str = "http://www.w3.org/ns/shacl#qualifiedValueShape";
pub const SH_QUALIFIED_MIN_COUNT: &str = "http://www.w3.org/ns/shacl#qualifiedMinCount";
pub const SH_QUALIFIED_MAX_COUNT: &str = "http://www.w3.org/ns/shacl#qualifiedMaxCount";
pub const SH_QUALIFIED_VALUE_SHAPES_DISJOINT: &str =
    "http://www.w3.org/ns/shacl#qualifiedValueShapesDisjoint";

// §4.8 Other
pub const SH_CLOSED: &str = "http://www.w3.org/ns/shacl#closed";
pub const SH_IGNORED_PROPERTIES: &str = "http://www.w3.org/ns/shacl#ignoredProperties";
pub const SH_HAS_VALUE: &str = "http://www.w3.org/ns/shacl#hasValue";
pub const SH_IN: &str = "http://www.w3.org/ns/shacl#in";

// §3.6 sh:deactivated
pub const SH_DEACTIVATED: &str = "http://www.w3.org/ns/shacl#deactivated";

// §3.5 Severity
pub const SH_SEVERITY: &str = "http://www.w3.org/ns/shacl#severity";
pub const SH_VIOLATION: &str = "http://www.w3.org/ns/shacl#Violation";
pub const SH_WARNING: &str = "http://www.w3.org/ns/shacl#Warning";
pub const SH_INFO: &str = "http://www.w3.org/ns/shacl#Info";

// §3.4 sh:message — a human-readable explanation attached to a shape,
// surfaced on every `ValidationResult` it produces (`sh:resultMessage`).
// See [#264](https://github.com/daghovland/rdf-datalog/issues/264).
pub const SH_MESSAGE: &str = "http://www.w3.org/ns/shacl#message";

// §3.6 Validation report — https://www.w3.org/TR/shacl/#validation-report
//
// Predicates/classes for the `sh:ValidationReport` graph produced by
// `report_to_turtle`/`report_to_datastore`. See
// [#314](https://github.com/daghovland/rdf-datalog/issues/314).
pub const SH_VALIDATION_REPORT: &str = "http://www.w3.org/ns/shacl#ValidationReport";
pub const SH_VALIDATION_RESULT: &str = "http://www.w3.org/ns/shacl#ValidationResult";
pub const SH_CONFORMS: &str = "http://www.w3.org/ns/shacl#conforms";
pub const SH_RESULT: &str = "http://www.w3.org/ns/shacl#result";
pub const SH_FOCUS_NODE: &str = "http://www.w3.org/ns/shacl#focusNode";
/// `sh:resultSeverity` — the severity of a *result* in a validation report.
/// Distinct from `SH_SEVERITY` (`sh:severity`), which is a shape's own
/// severity declaration in the shapes graph.
pub const SH_RESULT_SEVERITY: &str = "http://www.w3.org/ns/shacl#resultSeverity";
pub const SH_RESULT_PATH: &str = "http://www.w3.org/ns/shacl#resultPath";
pub const SH_VALUE: &str = "http://www.w3.org/ns/shacl#value";
pub const SH_SOURCE_SHAPE: &str = "http://www.w3.org/ns/shacl#sourceShape";
pub const SH_SOURCE_CONSTRAINT_COMPONENT: &str =
    "http://www.w3.org/ns/shacl#sourceConstraintComponent";
pub const SH_RESULT_MESSAGE: &str = "http://www.w3.org/ns/shacl#resultMessage";

// ── Constraint component IRIs (`sh:sourceConstraintComponent`) ────────────────
//
// One IRI per constraint component, exactly as named in the W3C SHACL spec's
// component table: <https://www.w3.org/TR/shacl/#core-components>. Used to
// populate `ValidationResult::source_constraint`. See
// [#264](https://github.com/daghovland/rdf-datalog/issues/264).

pub const CC_CLASS: &str = "http://www.w3.org/ns/shacl#ClassConstraintComponent";
pub const CC_DATATYPE: &str = "http://www.w3.org/ns/shacl#DatatypeConstraintComponent";
pub const CC_NODE_KIND: &str = "http://www.w3.org/ns/shacl#NodeKindConstraintComponent";
pub const CC_MIN_COUNT: &str = "http://www.w3.org/ns/shacl#MinCountConstraintComponent";
pub const CC_MAX_COUNT: &str = "http://www.w3.org/ns/shacl#MaxCountConstraintComponent";
pub const CC_MIN_INCLUSIVE: &str = "http://www.w3.org/ns/shacl#MinInclusiveConstraintComponent";
pub const CC_MAX_INCLUSIVE: &str = "http://www.w3.org/ns/shacl#MaxInclusiveConstraintComponent";
pub const CC_MIN_EXCLUSIVE: &str = "http://www.w3.org/ns/shacl#MinExclusiveConstraintComponent";
pub const CC_MAX_EXCLUSIVE: &str = "http://www.w3.org/ns/shacl#MaxExclusiveConstraintComponent";
pub const CC_MIN_LENGTH: &str = "http://www.w3.org/ns/shacl#MinLengthConstraintComponent";
pub const CC_MAX_LENGTH: &str = "http://www.w3.org/ns/shacl#MaxLengthConstraintComponent";
pub const CC_PATTERN: &str = "http://www.w3.org/ns/shacl#PatternConstraintComponent";
pub const CC_LANGUAGE_IN: &str = "http://www.w3.org/ns/shacl#LanguageInConstraintComponent";
pub const CC_UNIQUE_LANG: &str = "http://www.w3.org/ns/shacl#UniqueLangConstraintComponent";
pub const CC_EQUALS: &str = "http://www.w3.org/ns/shacl#EqualsConstraintComponent";
pub const CC_DISJOINT: &str = "http://www.w3.org/ns/shacl#DisjointConstraintComponent";
pub const CC_LESS_THAN: &str = "http://www.w3.org/ns/shacl#LessThanConstraintComponent";
pub const CC_LESS_THAN_OR_EQUALS: &str =
    "http://www.w3.org/ns/shacl#LessThanOrEqualsConstraintComponent";
pub const CC_NOT: &str = "http://www.w3.org/ns/shacl#NotConstraintComponent";
pub const CC_AND: &str = "http://www.w3.org/ns/shacl#AndConstraintComponent";
pub const CC_OR: &str = "http://www.w3.org/ns/shacl#OrConstraintComponent";
pub const CC_XONE: &str = "http://www.w3.org/ns/shacl#XoneConstraintComponent";
pub const CC_NODE: &str = "http://www.w3.org/ns/shacl#NodeConstraintComponent";
// Note: there is no unified "QualifiedValueShapeConstraintComponent" in the
// spec vocabulary (verified against https://www.w3.org/ns/shacl.ttl) — only
// separate Min/Max components, since sh:qualifiedMinCount and
// sh:qualifiedMaxCount are independent parameters of the same shape-based
// constraint. See #264.
pub const CC_QUALIFIED_MIN_COUNT: &str =
    "http://www.w3.org/ns/shacl#QualifiedMinCountConstraintComponent";
pub const CC_QUALIFIED_MAX_COUNT: &str =
    "http://www.w3.org/ns/shacl#QualifiedMaxCountConstraintComponent";
pub const CC_CLOSED: &str = "http://www.w3.org/ns/shacl#ClosedConstraintComponent";
pub const CC_HAS_VALUE: &str = "http://www.w3.org/ns/shacl#HasValueConstraintComponent";
pub const CC_IN: &str = "http://www.w3.org/ns/shacl#InConstraintComponent";

// ── Synthetic marker IRIs (internal to this implementation) ───────────────────
//
// These are minted into the working Datastore as predicate IRIs during validation.
// They never appear in user data. Prefixed with `urn:dagalog:shacl:` to avoid
// any clash with real data.

/// Singleton true-marker object for binary marker triples.
pub const INT_TRUE: &str = "urn:dagalog:shacl:true";
/// Sentinel nil object when there is no meaningful offending value.
pub const INT_NIL: &str = "urn:dagalog:shacl:nil";

/// Unique target predicate for shape `shape_idx`.
/// Triples `(node, target(i), INT_TRUE)` mark that `node` is a target of shape `i`.
pub fn int_target(shape_idx: usize) -> String {
    format!("urn:dagalog:shacl:target:{shape_idx}")
}

/// Unique has-value helper predicate for (shape_idx, prop_idx).
/// Triples `(node, has_val(i,j), INT_TRUE)` mean node has ≥1 value for prop j of shape i.
pub fn int_has_val(shape_idx: usize, prop_idx: usize) -> String {
    format!("urn:dagalog:shacl:hasVal:{shape_idx}:{prop_idx}")
}

/// Unique "allowed predicate" helper for shape `shape_idx` (sh:closed).
/// Triples `(pred, allowed(i), INT_TRUE)` mean `pred` is allowed in shape `i`.
pub fn int_allowed_pred(shape_idx: usize) -> String {
    format!("urn:dagalog:shacl:allowedPred:{shape_idx}")
}

/// Unique "in-list" helper predicate for (shape_idx, prop_idx) sh:in constraint.
/// Triples `(value, in_list(i,j), INT_TRUE)` mean `value` is in the sh:in list.
pub fn int_in_list(shape_idx: usize, prop_idx: usize) -> String {
    format!("urn:dagalog:shacl:inList:{shape_idx}:{prop_idx}")
}

/// Synthetic "prop_idx" base used for node-level (pathless) constraints
/// (`ParsedShape::node_constraints`), so their violation-IRI `prop_idx` slot
/// never collides with a real `ParsedPropShape::idx` (which starts at 0) or
/// with the `sub_idx * 10_000 + pi` scheme used for `sh:and` inner shapes.
/// See [#260](https://github.com/daghovland/rdf-datalog/issues/260).
pub const NODE_LEVEL_PI_BASE: usize = usize::MAX / 2;

// ── Violation IRI builders ────────────────────────────────────────────────────
//
// One unique violation predicate per (shape, constraint). Each violation triple
// (focusNode, viol_pred, offendingValueOrNil) in the working store after
// evaluate_rules becomes one ValidationResult.

pub fn viol_min_count(shape_idx: usize, prop_idx: usize) -> String {
    format!("urn:dagalog:shacl:viol:{shape_idx}:{prop_idx}:minCount")
}

pub fn viol_max_count(shape_idx: usize, prop_idx: usize) -> String {
    format!("urn:dagalog:shacl:viol:{shape_idx}:{prop_idx}:maxCount")
}

pub fn viol_class(shape_idx: usize, prop_idx: usize) -> String {
    format!("urn:dagalog:shacl:viol:{shape_idx}:{prop_idx}:class")
}

pub fn viol_has_value(shape_idx: usize, prop_idx: usize) -> String {
    format!("urn:dagalog:shacl:viol:{shape_idx}:{prop_idx}:hasValue")
}

pub fn viol_in(shape_idx: usize, prop_idx: usize) -> String {
    format!("urn:dagalog:shacl:viol:{shape_idx}:{prop_idx}:in")
}

/// `sh:closed` is data-driven: the set of offending predicates for a shape
/// isn't known until the data graph is scanned (unlike every other
/// constraint, which is keyed by a fixed `prop_idx` from the shapes graph
/// alone). Each distinct offending predicate therefore gets its own
/// violation predicate, disambiguated by `pred_id` — the offending
/// predicate's own `GraphElementId` (stable within a single `validate()`
/// call since `work` starts as a clone of `data`, see `closed_violations`) —
/// so `sh:resultPath` can vary per offending predicate while `sh:value`
/// still carries the real triple object. See
/// [#308](https://github.com/daghovland/rdf-datalog/issues/308).
pub fn viol_closed(shape_idx: usize, pred_id: u32) -> String {
    format!("urn:dagalog:shacl:viol:{shape_idx}:closed:{pred_id}")
}

pub fn viol_not(shape_idx: usize) -> String {
    format!("urn:dagalog:shacl:viol:{shape_idx}:not")
}

pub fn viol_and(shape_idx: usize) -> String {
    format!("urn:dagalog:shacl:viol:{shape_idx}:and")
}

pub fn viol_or(shape_idx: usize) -> String {
    format!("urn:dagalog:shacl:viol:{shape_idx}:or")
}

pub fn viol_xone(shape_idx: usize) -> String {
    format!("urn:dagalog:shacl:viol:{shape_idx}:xone")
}

// sh:not/sh:and/sh:or/sh:xone declared directly inside a sh:property block —
// distinct predicate namespace from the node-shape-scoped viol_not/and/or/xone
// above, since both are keyed by shape_idx alone and would otherwise collide
// when the same node shape has both a node-level and a property-level
// combinator. See https://github.com/daghovland/rdf-datalog/issues/311.
pub fn viol_prop_not(shape_idx: usize, prop_idx: usize) -> String {
    format!("urn:dagalog:shacl:viol:{shape_idx}:{prop_idx}:prop-not")
}

pub fn viol_prop_and(shape_idx: usize, prop_idx: usize) -> String {
    format!("urn:dagalog:shacl:viol:{shape_idx}:{prop_idx}:prop-and")
}

pub fn viol_prop_or(shape_idx: usize, prop_idx: usize) -> String {
    format!("urn:dagalog:shacl:viol:{shape_idx}:{prop_idx}:prop-or")
}

pub fn viol_prop_xone(shape_idx: usize, prop_idx: usize) -> String {
    format!("urn:dagalog:shacl:viol:{shape_idx}:{prop_idx}:prop-xone")
}

// §4.1 value type (Phase 2)
pub fn viol_datatype(shape_idx: usize, prop_idx: usize) -> String {
    format!("urn:dagalog:shacl:viol:{shape_idx}:{prop_idx}:datatype")
}
pub fn viol_node_kind(shape_idx: usize, prop_idx: usize) -> String {
    format!("urn:dagalog:shacl:viol:{shape_idx}:{prop_idx}:nodeKind")
}

// §4.3 value range (Phase 2)
pub fn viol_min_inclusive(shape_idx: usize, prop_idx: usize) -> String {
    format!("urn:dagalog:shacl:viol:{shape_idx}:{prop_idx}:minInclusive")
}
pub fn viol_max_inclusive(shape_idx: usize, prop_idx: usize) -> String {
    format!("urn:dagalog:shacl:viol:{shape_idx}:{prop_idx}:maxInclusive")
}
pub fn viol_min_exclusive(shape_idx: usize, prop_idx: usize) -> String {
    format!("urn:dagalog:shacl:viol:{shape_idx}:{prop_idx}:minExclusive")
}
pub fn viol_max_exclusive(shape_idx: usize, prop_idx: usize) -> String {
    format!("urn:dagalog:shacl:viol:{shape_idx}:{prop_idx}:maxExclusive")
}

// §4.4 string-based (Phase 2)
pub fn viol_min_length(shape_idx: usize, prop_idx: usize) -> String {
    format!("urn:dagalog:shacl:viol:{shape_idx}:{prop_idx}:minLength")
}
pub fn viol_max_length(shape_idx: usize, prop_idx: usize) -> String {
    format!("urn:dagalog:shacl:viol:{shape_idx}:{prop_idx}:maxLength")
}
pub fn viol_pattern(shape_idx: usize, prop_idx: usize) -> String {
    format!("urn:dagalog:shacl:viol:{shape_idx}:{prop_idx}:pattern")
}
pub fn viol_language_in(shape_idx: usize, prop_idx: usize) -> String {
    format!("urn:dagalog:shacl:viol:{shape_idx}:{prop_idx}:languageIn")
}
pub fn viol_unique_lang(shape_idx: usize, prop_idx: usize) -> String {
    format!("urn:dagalog:shacl:viol:{shape_idx}:{prop_idx}:uniqueLang")
}

// §4.5 property pair (Phase 2)
pub fn viol_equals(shape_idx: usize, prop_idx: usize) -> String {
    format!("urn:dagalog:shacl:viol:{shape_idx}:{prop_idx}:equals")
}
pub fn viol_disjoint(shape_idx: usize, prop_idx: usize) -> String {
    format!("urn:dagalog:shacl:viol:{shape_idx}:{prop_idx}:disjoint")
}
pub fn viol_less_than(shape_idx: usize, prop_idx: usize) -> String {
    format!("urn:dagalog:shacl:viol:{shape_idx}:{prop_idx}:lessThan")
}
pub fn viol_less_than_or_equals(shape_idx: usize, prop_idx: usize) -> String {
    format!("urn:dagalog:shacl:viol:{shape_idx}:{prop_idx}:lessThanOrEquals")
}

// §4.7 shape-based (Phase 2)
pub fn viol_node_shape(shape_idx: usize, prop_idx: usize) -> String {
    format!("urn:dagalog:shacl:viol:{shape_idx}:{prop_idx}:node")
}
pub fn viol_qualified_min_count(shape_idx: usize, prop_idx: usize) -> String {
    format!("urn:dagalog:shacl:viol:{shape_idx}:{prop_idx}:qualifiedMinCount")
}
pub fn viol_qualified_max_count(shape_idx: usize, prop_idx: usize) -> String {
    format!("urn:dagalog:shacl:viol:{shape_idx}:{prop_idx}:qualifiedMaxCount")
}

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

// §4.8 Other
pub const SH_CLOSED: &str = "http://www.w3.org/ns/shacl#closed";
pub const SH_IGNORED_PROPERTIES: &str = "http://www.w3.org/ns/shacl#ignoredProperties";
pub const SH_HAS_VALUE: &str = "http://www.w3.org/ns/shacl#hasValue";
pub const SH_IN: &str = "http://www.w3.org/ns/shacl#in";

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

/// "Conforms to inner shape" helper for shape `shape_idx`, sub-shape `sub_idx`.
/// Used for sh:not / sh:or / sh:xone to derive that the node satisfies a sub-shape.
pub fn int_sub_ok(shape_idx: usize, sub_idx: usize) -> String {
    format!("urn:dagalog:shacl:subOk:{shape_idx}:{sub_idx}")
}

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

pub fn viol_node_class(shape_idx: usize) -> String {
    format!("urn:dagalog:shacl:viol:{shape_idx}:nodeClass")
}

pub fn viol_has_value(shape_idx: usize, prop_idx: usize) -> String {
    format!("urn:dagalog:shacl:viol:{shape_idx}:{prop_idx}:hasValue")
}

pub fn viol_in(shape_idx: usize, prop_idx: usize) -> String {
    format!("urn:dagalog:shacl:viol:{shape_idx}:{prop_idx}:in")
}

pub fn viol_closed(shape_idx: usize) -> String {
    format!("urn:dagalog:shacl:viol:{shape_idx}:closed")
}

pub fn viol_not(shape_idx: usize) -> String {
    format!("urn:dagalog:shacl:viol:{shape_idx}:not")
}

pub fn viol_and(shape_idx: usize, sub_idx: usize) -> String {
    format!("urn:dagalog:shacl:viol:{shape_idx}:and:{sub_idx}")
}

pub fn viol_or(shape_idx: usize) -> String {
    format!("urn:dagalog:shacl:viol:{shape_idx}:or")
}

pub fn viol_xone(shape_idx: usize) -> String {
    format!("urn:dagalog:shacl:viol:{shape_idx}:xone")
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
pub fn viol_qualified_value(shape_idx: usize, prop_idx: usize) -> String {
    format!("urn:dagalog:shacl:viol:{shape_idx}:{prop_idx}:qualifiedValue")
}

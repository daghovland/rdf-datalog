//! Closed built-in registry of FNML/GREL functions.
//!
//! A fully generic FnO dispatcher (resolving arbitrary `fno:Function` IRIs,
//! fetching remote function descriptions) is out of scope. This registry
//! covers the small, fixed set of unary GREL string functions that appear in
//! nearly every real-world FNML mapping. See
//! `docs/plans/RML_FNML_PLAN.md` and
//! [#27](https://github.com/daghovland/rdf-datalog/issues/27).
//!
//! Extension point: add a `BuiltinFunction` variant, an `apply` arm, and a
//! `resolve_builtin` IRI match arm — no architecture change required.

use ingress::IriReference;

/// The published GREL vocabulary namespace, verified against
/// <https://users.ugent.be/~bjdmeest/function/grel.ttl>.
pub const GREL_NS: &str = "https://users.ugent.be/~bjdmeest/function/grel.ttl#";

/// `fno:executes` — links a function-map node to the function it invokes.
pub const FNO_EXECUTES: &str = "https://w3id.org/function/ontology#executes";

/// `fnml:functionValue` — the FNML trigger property on a term map node.
pub const FNML_FUNCTION_VALUE: &str = "http://semweb.mmlab.be/ns/fnml#functionValue";

fn grel(local: &str) -> String {
    format!("{GREL_NS}{local}")
}

/// A resolved built-in FNML function. All variants in this pass are unary
/// (a single `grel:valueParam`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinFunction {
    ToUpperCase,
    ToLowerCase,
    Trim,
}

/// Resolve a function IRI (the object of `fno:executes`) to a built-in
/// implementation, or `None` if it isn't in the registry — callers must
/// treat that as a hard error (`RmlError::UnknownFunction`), not a
/// per-row skip.
pub fn resolve_builtin(iri: &IriReference) -> Option<BuiltinFunction> {
    match iri.0.as_str() {
        s if s == grel("toUpperCase") => Some(BuiltinFunction::ToUpperCase),
        s if s == grel("toLowerCase") => Some(BuiltinFunction::ToLowerCase),
        s if s == grel("string_trim") => Some(BuiltinFunction::Trim),
        _ => None,
    }
}

/// Apply a resolved built-in function to its single input value.
pub fn apply(f: BuiltinFunction, input: &str) -> String {
    match f {
        BuiltinFunction::ToUpperCase => input.to_uppercase(),
        BuiltinFunction::ToLowerCase => input.to_lowercase(),
        BuiltinFunction::Trim => input.trim().to_string(),
    }
}

/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

use super::expressions::{numeric_lit_to_element, NumericLit};
use super::functions::{literal_to_f64, parse_datetime_or_date_lexical};
use super::*;

/// Dispatch a resolved XSD datatype IRI to its cast implementation.
pub(crate) fn eval_xsd_cast(target_iri: &str, el: &GraphElement) -> Option<GraphElement> {
    match target_iri {
        XSD_STRING => cast_to_xsd_string(el),
        XSD_BOOLEAN => cast_to_xsd_boolean(el),
        XSD_INTEGER => cast_to_xsd_integer(el),
        XSD_DECIMAL => cast_to_xsd_decimal(el),
        XSD_DOUBLE => cast_to_xsd_double(el),
        XSD_FLOAT => cast_to_xsd_float(el),
        XSD_DATE_TIME => cast_to_xsd_datetime(el),
        _ => None,
    }
}

/// Parse an XSD `integer` lexical form (optional sign, digits only) into a `BigInt`.
pub(crate) fn parse_xsd_integer_lexical(s: &str) -> Option<BigInt> {
    let t = s.trim();
    let (sign, digits) = match t.strip_prefix('+') {
        Some(rest) => ("", rest),
        None => match t.strip_prefix('-') {
            Some(rest) => ("-", rest),
            None => ("", t),
        },
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    format!("{sign}{digits}").parse::<BigInt>().ok()
}

/// Parse an XSD `boolean` lexical form (`true`/`false`/`1`/`0`) into a `bool`.
pub(crate) fn parse_xsd_boolean_lexical(s: &str) -> Option<bool> {
    match s.trim() {
        "true" | "1" => Some(true),
        "false" | "0" => Some(false),
        _ => None,
    }
}

/// Parse an XSD `double`/`float` lexical form, including the special values
/// `INF`/`-INF`/`NaN`, into an `f64`.
pub(crate) fn parse_xsd_double_lexical(s: &str) -> Option<f64> {
    match s.trim() {
        "INF" | "+INF" => Some(f64::INFINITY),
        "-INF" => Some(f64::NEG_INFINITY),
        "NaN" => Some(f64::NAN),
        other => other.parse::<f64>().ok(),
    }
}

/// Cast to `xsd:string`: the lexical/string value of the source literal.
/// Kept separate from `graph_element_to_string` (shared by `CONCAT`/`STRLEN`/
/// etc.) so this cast's semantics — e.g. rendering native numeric/boolean
/// literals — can't change the behaviour of those unrelated builtins.
pub(crate) fn cast_to_xsd_string(el: &GraphElement) -> Option<GraphElement> {
    let s = match el {
        GraphElement::GraphLiteral(RdfLiteral::LiteralString(s)) => s.clone(),
        GraphElement::GraphLiteral(RdfLiteral::LangLiteral { literal, .. }) => literal.clone(),
        GraphElement::GraphLiteral(RdfLiteral::TypedLiteral { literal, .. }) => literal.clone(),
        GraphElement::GraphLiteral(RdfLiteral::BooleanLiteral(b)) => b.to_string(),
        GraphElement::GraphLiteral(RdfLiteral::IntegerLiteral(n)) => n.to_string(),
        GraphElement::GraphLiteral(RdfLiteral::DecimalLiteral(d)) => d.to_string(),
        GraphElement::GraphLiteral(RdfLiteral::DoubleLiteral(d)) => d.to_string(),
        GraphElement::GraphLiteral(RdfLiteral::FloatLiteral(f)) => f.to_string(),
        _ => return None,
    };
    Some(GraphElement::GraphLiteral(RdfLiteral::LiteralString(s)))
}

/// Cast to `xsd:boolean` per XPath casting rules: numeric zero/NaN is
/// `false`, any other numeric value is `true`; strings must match the
/// `xsd:boolean` lexical space (`true`/`false`/`1`/`0`).
pub(crate) fn cast_to_xsd_boolean(el: &GraphElement) -> Option<GraphElement> {
    let b = match el {
        GraphElement::GraphLiteral(RdfLiteral::BooleanLiteral(b)) => *b,
        GraphElement::GraphLiteral(RdfLiteral::LiteralString(s)) => parse_xsd_boolean_lexical(s)?,
        GraphElement::GraphLiteral(RdfLiteral::LangLiteral { literal, .. }) => {
            parse_xsd_boolean_lexical(literal)?
        }
        GraphElement::GraphLiteral(RdfLiteral::TypedLiteral { type_iri, literal })
            if type_iri.0 == XSD_BOOLEAN || type_iri.0 == XSD_STRING =>
        {
            parse_xsd_boolean_lexical(literal)?
        }
        GraphElement::GraphLiteral(lit) => {
            let f = literal_to_f64(lit)?;
            !f.is_nan() && f != 0.0
        }
        _ => return None,
    };
    // Emit `TypedLiteral{xsd:boolean, "true"/"false"}` — the shape
    // `parse_boolean_literal` produces for real `true`/`false` literals —
    // rather than the native `BooleanLiteral` variant, so a cast result can
    // join against real interned boolean data. See #228.
    Some(GraphElement::GraphLiteral(RdfLiteral::TypedLiteral {
        type_iri: IriReference(XSD_BOOLEAN.to_string()),
        literal: b.to_string(),
    }))
}

/// Cast to `xsd:integer` per XPath fn:integer casting rules: numeric sources
/// truncate toward zero (NOT floor/round — `xsd:integer(-3.7)` is `-3`).
pub(crate) fn cast_to_xsd_integer(el: &GraphElement) -> Option<GraphElement> {
    let n = match el {
        GraphElement::GraphLiteral(RdfLiteral::IntegerLiteral(n)) => n.clone(),
        GraphElement::GraphLiteral(RdfLiteral::BooleanLiteral(b)) => BigInt::from(u8::from(*b)),
        GraphElement::GraphLiteral(RdfLiteral::LiteralString(s)) => parse_xsd_integer_lexical(s)?,
        GraphElement::GraphLiteral(RdfLiteral::LangLiteral { literal, .. }) => {
            parse_xsd_integer_lexical(literal)?
        }
        GraphElement::GraphLiteral(RdfLiteral::TypedLiteral { type_iri, literal })
            if type_iri.0 == XSD_BOOLEAN =>
        {
            BigInt::from(u8::from(parse_xsd_boolean_lexical(literal)?))
        }
        GraphElement::GraphLiteral(RdfLiteral::TypedLiteral { type_iri, literal })
            if type_iri.0 == XSD_INTEGER || type_iri.0 == XSD_STRING =>
        {
            parse_xsd_integer_lexical(literal)?
        }
        GraphElement::GraphLiteral(lit) => {
            // xsd:decimal / xsd:double / xsd:float (native or typed): truncate.
            let f = literal_to_f64(lit)?;
            if !f.is_finite() {
                return None;
            }
            BigInt::from(f.trunc() as i64)
        }
        _ => return None,
    };
    // TypedLiteral, not the native IntegerLiteral variant — see #228.
    Some(numeric_lit_to_element(NumericLit::Integer(n)))
}

/// Cast to `xsd:decimal`. Converts via the source's string form (rather than
/// through `f64`) where possible, to avoid binary-float rounding noise in the
/// resulting decimal's lexical form.
pub(crate) fn cast_to_xsd_decimal(el: &GraphElement) -> Option<GraphElement> {
    let d = match el {
        GraphElement::GraphLiteral(RdfLiteral::DecimalLiteral(d)) => *d,
        GraphElement::GraphLiteral(RdfLiteral::IntegerLiteral(n)) => n.to_string().parse().ok()?,
        GraphElement::GraphLiteral(RdfLiteral::BooleanLiteral(b)) => {
            rust_decimal::Decimal::from(u8::from(*b))
        }
        GraphElement::GraphLiteral(RdfLiteral::LiteralString(s)) => s.trim().parse().ok()?,
        GraphElement::GraphLiteral(RdfLiteral::LangLiteral { literal, .. }) => {
            literal.trim().parse().ok()?
        }
        GraphElement::GraphLiteral(RdfLiteral::TypedLiteral { type_iri, literal })
            if type_iri.0 == XSD_BOOLEAN =>
        {
            rust_decimal::Decimal::from(u8::from(parse_xsd_boolean_lexical(literal)?))
        }
        GraphElement::GraphLiteral(RdfLiteral::TypedLiteral { type_iri, literal })
            if type_iri.0 == XSD_DECIMAL
                || type_iri.0 == XSD_INTEGER
                || type_iri.0 == XSD_STRING =>
        {
            literal.trim().parse().ok()?
        }
        GraphElement::GraphLiteral(lit) => {
            // xsd:double / xsd:float (native or typed): round-trip through the
            // decimal string form of the f64 to avoid binary-float noise.
            let f = literal_to_f64(lit)?;
            if !f.is_finite() {
                return None;
            }
            f.to_string().parse().ok()?
        }
        _ => return None,
    };
    // TypedLiteral, not the native DecimalLiteral variant — see #228.
    Some(numeric_lit_to_element(NumericLit::Decimal(d)))
}

/// Shared numeric extraction for `xsd:double`/`xsd:float` casts: handles
/// booleans and lexical strings (including `INF`/`NaN`) directly, and
/// delegates to `literal_to_f64` for the plain numeric literal kinds it
/// already covers.
pub(crate) fn extract_f64_for_cast(el: &GraphElement) -> Option<f64> {
    match el {
        GraphElement::GraphLiteral(RdfLiteral::BooleanLiteral(b)) => {
            Some(if *b { 1.0 } else { 0.0 })
        }
        GraphElement::GraphLiteral(RdfLiteral::LiteralString(s)) => parse_xsd_double_lexical(s),
        GraphElement::GraphLiteral(RdfLiteral::LangLiteral { literal, .. }) => {
            parse_xsd_double_lexical(literal)
        }
        GraphElement::GraphLiteral(RdfLiteral::TypedLiteral { type_iri, literal })
            if type_iri.0 == XSD_BOOLEAN =>
        {
            parse_xsd_boolean_lexical(literal).map(|b| if b { 1.0 } else { 0.0 })
        }
        GraphElement::GraphLiteral(RdfLiteral::TypedLiteral { literal, .. }) => {
            parse_xsd_double_lexical(literal)
        }
        GraphElement::GraphLiteral(lit) => literal_to_f64(lit),
        _ => None,
    }
}

/// Cast to `xsd:double`. Emits `TypedLiteral{xsd:double, ..}`, not the native
/// `DoubleLiteral` variant, so the result can join against real interned
/// `xsd:double` data — see #228.
pub(crate) fn cast_to_xsd_double(el: &GraphElement) -> Option<GraphElement> {
    let f = extract_f64_for_cast(el)?;
    Some(numeric_lit_to_element(NumericLit::Double(f)))
}

/// Cast to `xsd:float`. Emits `TypedLiteral{xsd:float, ..}`, not the native
/// `FloatLiteral` variant — see #228, as above.
pub(crate) fn cast_to_xsd_float(el: &GraphElement) -> Option<GraphElement> {
    let f = extract_f64_for_cast(el)?;
    Some(numeric_lit_to_element(NumericLit::Float(f)))
}

/// Cast to `xsd:dateTime` (#194): a native `DateTimeLiteral` passes through
/// unchanged; a string (or `xsd:dateTime`/`xsd:date`/`xsd:string`-typed
/// literal) is parsed via `parse_datetime_or_date_lexical`, which accepts the
/// full `xsd:dateTime` lexical space (with or without a timezone) and
/// normalizes `xsd:date` (`YYYY-MM-DD`) to midnight UTC. Deliberately does
/// NOT fall back to bare `xsd:gYear` the way `parse_xsd_datetime` (used by
/// `YEAR`/`MONTH`/`DAY`) does — a bare year is not a valid `xsd:dateTime`
/// cast source per the XPath casting rules, so it stays unbound.
///
/// The result is emitted as `TypedLiteral{xsd:dateTime, dt.to_rfc3339()}`,
/// not the native `DateTimeLiteral` variant — the Turtle parser always
/// produces `TypedLiteral` for `xsd:dateTime` data (only `xsd:string`
/// literals get a dedicated variant; see `turtle::convert_literal`), so a
/// native `DateTimeLiteral` cast result could never structurally match
/// already-interned `xsd:dateTime` data in a later triple-pattern join. See
/// #228. Note `chrono`'s `to_rfc3339()` always normalizes the UTC offset to
/// `+00:00` (never `Z`), so real data joined against a cast result must use
/// the same `+00:00` lexical form.
pub(crate) fn cast_to_xsd_datetime(el: &GraphElement) -> Option<GraphElement> {
    let dt = match el {
        GraphElement::GraphLiteral(RdfLiteral::DateTimeLiteral(dt)) => *dt,
        GraphElement::GraphLiteral(RdfLiteral::LiteralString(s)) => {
            parse_datetime_or_date_lexical(s)?
        }
        GraphElement::GraphLiteral(RdfLiteral::LangLiteral { literal, .. }) => {
            parse_datetime_or_date_lexical(literal)?
        }
        GraphElement::GraphLiteral(RdfLiteral::TypedLiteral { type_iri, literal })
            if type_iri.0 == XSD_DATE_TIME
                || type_iri.0 == XSD_DATE
                || type_iri.0 == XSD_STRING =>
        {
            parse_datetime_or_date_lexical(literal)?
        }
        _ => return None,
    };
    Some(GraphElement::GraphLiteral(RdfLiteral::TypedLiteral {
        type_iri: IriReference(XSD_DATE_TIME.to_string()),
        literal: dt.to_rfc3339(),
    }))
}

/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

use super::casts::{cast_to_xsd_boolean, eval_xsd_cast};
use super::expressions::{
    classify_numeric, eval_expression_value_inner, eval_string_predicate, numeric_lit_to_element,
    NumericLit,
};
use super::*;

pub(crate) fn eval_function_value(
    name: &str,
    args: &[Expression],
    sub: &PartialSub,
    datastore: &Datastore,
) -> Option<GraphElement> {
    // XSD datatype constructor/cast functions (SPARQL 1.1 §17.4.2). Unlike the
    // bare-keyword builtins below, these arrive as the function name's
    // *resolved* IRI: `xsd:integer(...)` is parsed via `parse_prefixed_name`
    // (or `<http://...#integer>(...)` via `parse_iri_ref`), never as the bare
    // word `xsd:integer` (see #186/PR #189), so dispatch matches the full IRI
    // rather than joining the uppercase-keyword match below.
    if matches!(
        name,
        XSD_STRING
            | XSD_BOOLEAN
            | XSD_INTEGER
            | XSD_DECIMAL
            | XSD_DOUBLE
            | XSD_FLOAT
            | XSD_DATE_TIME
    ) {
        let el = eval_expression_value_inner(args.first()?, sub, datastore)?;
        return eval_xsd_cast(name, &el);
    }
    let upper = name.to_ascii_uppercase();
    match upper.as_str() {
        "STRSTARTS" | "STRENDS" | "CONTAINS" => {
            eval_fn_strstarts_strends_contains(upper.as_str(), args, sub, datastore)
        }
        "STRBEFORE" => eval_fn_strbefore(args, sub, datastore),
        "STRAFTER" => eval_fn_strafter(args, sub, datastore),
        "STR" => eval_fn_str(args, sub, datastore),
        "LANG" => eval_fn_lang(args, sub, datastore),
        "STRLEN" => eval_fn_strlen(args, sub, datastore),
        "DATATYPE" => eval_fn_datatype(args, sub, datastore),
        "UCASE" => eval_fn_ucase(args, sub, datastore),
        "LCASE" => eval_fn_lcase(args, sub, datastore),
        "CONCAT" => eval_fn_concat(args, sub, datastore),
        "SUBSTR" => eval_fn_substr(args, sub, datastore),
        "IRI" | "URI" => eval_fn_iri_uri(args, sub, datastore),
        "STRDT" => eval_fn_strdt(args, sub, datastore),
        "STRLANG" => eval_fn_strlang(args, sub, datastore),
        "ISNUMERIC" => eval_fn_isnumeric(args, sub, datastore),
        "SAMETERM" => eval_fn_sameterm(args, sub, datastore),
        "ABS" => eval_fn_abs(args, sub, datastore),
        "ROUND" => eval_fn_round(args, sub, datastore),
        "CEIL" => eval_fn_ceil(args, sub, datastore),
        "FLOOR" => eval_fn_floor(args, sub, datastore),
        "COALESCE" => eval_fn_coalesce(args, sub, datastore),
        "IF" => eval_fn_if(args, sub, datastore),
        "BNODE" => eval_fn_bnode(args, sub, datastore),
        "ENCODE_FOR_URI" => eval_fn_encode_for_uri(args, sub, datastore),
        "REPLACE" => eval_fn_replace(args, sub, datastore),
        "RAND" => eval_fn_rand(args, sub, datastore),
        "NOW" => eval_fn_now(args, sub, datastore),
        "YEAR" => eval_fn_year(args, sub, datastore),
        "MONTH" => eval_fn_month(args, sub, datastore),
        "DAY" => eval_fn_day(args, sub, datastore),
        "HOURS" => eval_fn_hours(args, sub, datastore),
        "MINUTES" => eval_fn_minutes(args, sub, datastore),
        "SECONDS" => eval_fn_seconds(args, sub, datastore),
        "TZ" => eval_fn_tz(args, sub, datastore),
        "TIMEZONE" => eval_fn_timezone(args, sub, datastore),
        "MD5" => eval_fn_md5(args, sub, datastore),
        "SHA1" => eval_fn_sha1(args, sub, datastore),
        "SHA256" => eval_fn_sha256(args, sub, datastore),
        "SHA384" => eval_fn_sha384(args, sub, datastore),
        "SHA512" => eval_fn_sha512(args, sub, datastore),
        "UUID" => eval_fn_uuid(args, sub, datastore),
        "STRUUID" => eval_fn_struuid(args, sub, datastore),
        _ => None,
    }
}

pub(crate) fn eval_fn_strstarts_strends_contains(
    name: &str,
    args: &[Expression],
    sub: &PartialSub,
    datastore: &Datastore,
) -> Option<GraphElement> {
    let b = eval_string_predicate(name, args, sub, datastore)?;
    Some(GraphElement::GraphLiteral(RdfLiteral::BooleanLiteral(b)))
}

// STRBEFORE/STRAFTER (SPARQL 1.1 §17.4.3.13/14, `fn:substring-before`/
// `fn:substring-after`): the result carries arg1's simple/lang/
// xsd:string tag regardless of whether the separator is found, and
// the two operands must be "argument compatible" (§17.1) — arg2 may
// be a simple literal or `xsd:string` (compatible with anything), or
// must share arg1's exact language tag; any other combination
// (e.g. arg1 plain, arg2 language-tagged) is an error. A prior
// implementation always emitted a plain simple literal and never
// checked compatibility, failing the "datatyping" W3C fixtures (#205).
pub(crate) fn eval_fn_strbefore(
    args: &[Expression],
    sub: &PartialSub,
    datastore: &Datastore,
) -> Option<GraphElement> {
    let text_el = eval_expression_value_inner(args.first()?, sub, datastore)?;
    let (text, tag1) = literal_str_tag(&text_el)?;
    let sep_el = eval_expression_value_inner(args.get(1)?, sub, datastore)?;
    let (sep, tag2) = literal_str_tag(&sep_el)?;
    if !str_args_compatible(&tag1, &tag2) {
        return None;
    }
    // Per the W3C-approved `strbefore01a`/`strafter01a` revision: an
    // empty *separator* still yields arg1's tag (§17.4.3.13's
    // explicit empty-`B` case), but a separator that simply isn't
    // found in the text falls back to an untagged plain empty
    // literal, discarding arg1's tag — a distinct case from "found
    // an empty match". A prior version applied arg1's tag to both.
    if sep.is_empty() {
        return Some(str_tag_to_element(String::new(), tag1));
    }
    match text.find(sep.as_str()) {
        Some(idx) => Some(str_tag_to_element(text[..idx].to_string(), tag1)),
        None => Some(GraphElement::GraphLiteral(RdfLiteral::LiteralString(
            String::new(),
        ))),
    }
}

pub(crate) fn eval_fn_strafter(
    args: &[Expression],
    sub: &PartialSub,
    datastore: &Datastore,
) -> Option<GraphElement> {
    let text_el = eval_expression_value_inner(args.first()?, sub, datastore)?;
    let (text, tag1) = literal_str_tag(&text_el)?;
    let sep_el = eval_expression_value_inner(args.get(1)?, sub, datastore)?;
    let (sep, tag2) = literal_str_tag(&sep_el)?;
    if !str_args_compatible(&tag1, &tag2) {
        return None;
    }
    // See `STRBEFORE`'s comment above: empty separator preserves
    // arg1's tag (returns arg1 unchanged), but "not found" falls back
    // to an untagged plain empty literal.
    if sep.is_empty() {
        return Some(str_tag_to_element(text.clone(), tag1));
    }
    match text.find(sep.as_str()) {
        Some(idx) => Some(str_tag_to_element(
            text[idx + sep.len()..].to_string(),
            tag1,
        )),
        None => Some(GraphElement::GraphLiteral(RdfLiteral::LiteralString(
            String::new(),
        ))),
    }
}

pub(crate) fn eval_fn_str(
    args: &[Expression],
    sub: &PartialSub,
    datastore: &Datastore,
) -> Option<GraphElement> {
    let el = eval_expression_value_inner(args.first()?, sub, datastore)?;
    let s = graph_element_to_string(&el)?;
    Some(GraphElement::GraphLiteral(RdfLiteral::LiteralString(s)))
}

pub(crate) fn eval_fn_lang(
    args: &[Expression],
    sub: &PartialSub,
    datastore: &Datastore,
) -> Option<GraphElement> {
    let el = eval_expression_value_inner(args.first()?, sub, datastore)?;
    if let GraphElement::GraphLiteral(RdfLiteral::LangLiteral { lang, .. }) = el {
        Some(GraphElement::GraphLiteral(RdfLiteral::LiteralString(lang)))
    } else {
        Some(GraphElement::GraphLiteral(RdfLiteral::LiteralString(
            String::new(),
        )))
    }
}

pub(crate) fn eval_fn_strlen(
    args: &[Expression],
    sub: &PartialSub,
    datastore: &Datastore,
) -> Option<GraphElement> {
    let el = eval_expression_value_inner(args.first()?, sub, datastore)?;
    let s = match &el {
        GraphElement::GraphLiteral(RdfLiteral::LiteralString(s)) => s.clone(),
        GraphElement::GraphLiteral(RdfLiteral::LangLiteral { literal, .. }) => literal.clone(),
        GraphElement::GraphLiteral(RdfLiteral::TypedLiteral { literal, .. }) => literal.clone(),
        _ => return None,
    };
    let len = s.chars().count();
    Some(GraphElement::GraphLiteral(RdfLiteral::TypedLiteral {
        type_iri: IriReference(XSD_INTEGER.to_string()),
        literal: len.to_string(),
    }))
}

pub(crate) fn eval_fn_datatype(
    args: &[Expression],
    sub: &PartialSub,
    datastore: &Datastore,
) -> Option<GraphElement> {
    let el = eval_expression_value_inner(args.first()?, sub, datastore)?;
    let dt_iri = match &el {
        GraphElement::GraphLiteral(RdfLiteral::TypedLiteral { type_iri, .. }) => type_iri.0.clone(),
        GraphElement::GraphLiteral(RdfLiteral::LiteralString(_)) => XSD_STRING.to_string(),
        GraphElement::GraphLiteral(RdfLiteral::LangLiteral { .. }) => {
            "http://www.w3.org/1999/02/22-rdf-syntax-ns#langString".to_string()
        }
        GraphElement::GraphLiteral(RdfLiteral::BooleanLiteral(_)) => XSD_BOOLEAN.to_string(),
        GraphElement::GraphLiteral(RdfLiteral::IntegerLiteral(_)) => XSD_INTEGER.to_string(),
        GraphElement::GraphLiteral(RdfLiteral::DecimalLiteral(_)) => XSD_DECIMAL.to_string(),
        GraphElement::GraphLiteral(RdfLiteral::FloatLiteral(_)) => XSD_FLOAT.to_string(),
        GraphElement::GraphLiteral(RdfLiteral::DoubleLiteral(_)) => XSD_DOUBLE.to_string(),
        // `NOW()` produces a native `DateTimeLiteral` (see its own
        // comment below), so `DATATYPE(NOW())` must recognise it too
        // rather than falling through to `None` — otherwise
        // `FILTER(DATATYPE(?n) = xsd:dateTime)` always fails (W3C
        // `now01`, #205).
        GraphElement::GraphLiteral(RdfLiteral::DateTimeLiteral(_)) => {
            ingress::XSD_DATE_TIME.to_string()
        }
        GraphElement::GraphLiteral(RdfLiteral::DateLiteral(_)) => ingress::XSD_DATE.to_string(),
        GraphElement::GraphLiteral(RdfLiteral::TimeLiteral(_)) => ingress::XSD_TIME.to_string(),
        _ => return None,
    };
    Some(GraphElement::NodeOrEdge(dag_rdf::RdfResource::Iri(
        IriReference(dt_iri),
    )))
}

// ── String functions ──────────────────────────────────────────────────
// UCASE/LCASE/SUBSTR (SPARQL 1.1 §17.4.3.7/8/10) preserve the
// operand's simple/lang/xsd:string tag on output — a prior version
// always emitted a plain simple literal, dropping `@lang`/
// `^^xsd:string`, and failed every W3C fixture using a tagged
// operand (#205). Falls back to the untagged `graph_element_to_string`
// path for any other literal shape (numbers, IRIs, etc.) that isn't
// strictly a string literal per spec but which earlier callers may
// still rely on.
pub(crate) fn eval_fn_ucase(
    args: &[Expression],
    sub: &PartialSub,
    datastore: &Datastore,
) -> Option<GraphElement> {
    let el = eval_expression_value_inner(args.first()?, sub, datastore)?;
    if let Some((s, tag)) = literal_str_tag(&el) {
        Some(str_tag_to_element(s.to_uppercase(), tag))
    } else {
        let s = graph_element_to_string(&el)?;
        Some(GraphElement::GraphLiteral(RdfLiteral::LiteralString(
            s.to_uppercase(),
        )))
    }
}

pub(crate) fn eval_fn_lcase(
    args: &[Expression],
    sub: &PartialSub,
    datastore: &Datastore,
) -> Option<GraphElement> {
    let el = eval_expression_value_inner(args.first()?, sub, datastore)?;
    if let Some((s, tag)) = literal_str_tag(&el) {
        Some(str_tag_to_element(s.to_lowercase(), tag))
    } else {
        let s = graph_element_to_string(&el)?;
        Some(GraphElement::GraphLiteral(RdfLiteral::LiteralString(
            s.to_lowercase(),
        )))
    }
}

// CONCAT (SPARQL 1.1 §17.4.3.9, `fn:concat` with CONCAT's own
// datatyping addendum): the result is `xsd:string` if every argument
// is `xsd:string`; a shared language tag if every argument carries
// that same language tag; otherwise a plain simple literal. Any
// non-string-literal argument (e.g. an integer) is an error.
pub(crate) fn eval_fn_concat(
    args: &[Expression],
    sub: &PartialSub,
    datastore: &Datastore,
) -> Option<GraphElement> {
    let mut result = String::new();
    let mut tags = Vec::with_capacity(args.len());
    for arg in args {
        let el = eval_expression_value_inner(arg, sub, datastore)?;
        let (s, tag) = literal_str_tag(&el)?;
        result.push_str(&s);
        tags.push(tag);
    }
    let out_tag = if !tags.is_empty() && tags.iter().all(|t| *t == StrLitTag::XsdString) {
        StrLitTag::XsdString
    } else if let Some(StrLitTag::Lang(first_lang)) = tags.first() {
        if tags
            .iter()
            .all(|t| matches!(t, StrLitTag::Lang(l) if l == first_lang))
        {
            StrLitTag::Lang(first_lang.clone())
        } else {
            StrLitTag::Plain
        }
    } else {
        StrLitTag::Plain
    };
    Some(str_tag_to_element(result, out_tag))
}

pub(crate) fn eval_fn_substr(
    args: &[Expression],
    sub: &PartialSub,
    datastore: &Datastore,
) -> Option<GraphElement> {
    let el = eval_expression_value_inner(args.first()?, sub, datastore)?;
    let (text, tag) = literal_str_tag(&el)
        .or_else(|| graph_element_to_string(&el).map(|s| (s, StrLitTag::Plain)))?;
    let s: Vec<char> = text.chars().collect();
    let start_el = eval_expression_value_inner(args.get(1)?, sub, datastore)?;
    let start: usize = element_to_usize(&start_el)?.saturating_sub(1);
    let result: String = if let Some(len_expr) = args.get(2) {
        let len_el = eval_expression_value_inner(len_expr, sub, datastore)?;
        let len: usize = element_to_usize(&len_el)?;
        s.iter().skip(start).take(len).collect()
    } else {
        s.iter().skip(start).collect()
    };
    Some(str_tag_to_element(result, tag))
}

// ── Term construction ─────────────────────────────────────────────────
// IRI()/URI() (SPARQL 1.1 §17.4.2.6) must resolve a relative-IRI
// string argument against the query's effective base IRI at
// *evaluation* time — not just parse time, which is all
// `ParserContext::base` (#217) covers for IRIs written directly in
// query syntax. `current_base()` reads the base installed by
// `execute_with_base` (via the thread-local `CURRENT_BASE`, see
// `BaseGuard`). `crate::resolve_iri` is the same RFC 3986 resolver
// `BASE`/`PREFIX` parsing uses; with no base in effect it returns the
// raw string unresolved (verbatim), matching that existing no-base
// convention rather than erroring. See #346.
pub(crate) fn eval_fn_iri_uri(
    args: &[Expression],
    sub: &PartialSub,
    datastore: &Datastore,
) -> Option<GraphElement> {
    let el = eval_expression_value_inner(args.first()?, sub, datastore)?;
    let iri_str = graph_element_to_string(&el)?;
    let resolved = crate::resolve_iri(current_base().as_deref(), &iri_str).unwrap_or(iri_str);
    Some(GraphElement::NodeOrEdge(dag_rdf::RdfResource::Iri(
        IriReference(resolved),
    )))
}

// STRDT/STRLANG (SPARQL 1.1 §17.4.3.5/6, `fn:strdt`/`STRLANG`)
// require their first argument to be a *simple* literal — no
// language tag, no datatype (not even `xsd:string`) — and error
// otherwise. A prior implementation accepted any literal (or even an
// IRI) via `graph_element_to_string`, silently succeeding on
// already-typed/lang-tagged/non-literal input where the spec
// mandates an error (W3C `strdt03`/`strlang03`, #205).
pub(crate) fn eval_fn_strdt(
    args: &[Expression],
    sub: &PartialSub,
    datastore: &Datastore,
) -> Option<GraphElement> {
    let lex_el = eval_expression_value_inner(args.first()?, sub, datastore)?;
    let literal = match lex_el {
        GraphElement::GraphLiteral(RdfLiteral::LiteralString(s)) => s,
        _ => return None,
    };
    let dt_el = eval_expression_value_inner(args.get(1)?, sub, datastore)?;
    let type_iri = match dt_el {
        GraphElement::NodeOrEdge(dag_rdf::RdfResource::Iri(iri)) => iri,
        _ => return None,
    };
    Some(GraphElement::GraphLiteral(RdfLiteral::TypedLiteral {
        type_iri,
        literal,
    }))
}

pub(crate) fn eval_fn_strlang(
    args: &[Expression],
    sub: &PartialSub,
    datastore: &Datastore,
) -> Option<GraphElement> {
    let lex_el = eval_expression_value_inner(args.first()?, sub, datastore)?;
    let literal = match lex_el {
        GraphElement::GraphLiteral(RdfLiteral::LiteralString(s)) => s,
        _ => return None,
    };
    let lang_el = eval_expression_value_inner(args.get(1)?, sub, datastore)?;
    let lang = graph_element_to_string(&lang_el)?;
    Some(GraphElement::GraphLiteral(RdfLiteral::LangLiteral {
        lang,
        literal,
    }))
}

// ── Type testing ──────────────────────────────────────────────────────
pub(crate) fn eval_fn_isnumeric(
    args: &[Expression],
    sub: &PartialSub,
    datastore: &Datastore,
) -> Option<GraphElement> {
    let el = eval_expression_value_inner(args.first()?, sub, datastore)?;
    let is_numeric = match &el {
        GraphElement::GraphLiteral(
            RdfLiteral::IntegerLiteral(_)
            | RdfLiteral::DecimalLiteral(_)
            | RdfLiteral::DoubleLiteral(_)
            | RdfLiteral::FloatLiteral(_),
        ) => true,
        GraphElement::GraphLiteral(RdfLiteral::TypedLiteral { type_iri, .. }) => {
            matches!(
                type_iri.0.as_str(),
                XSD_INTEGER | XSD_DECIMAL | XSD_DOUBLE | XSD_FLOAT
            )
        }
        _ => false,
    };
    Some(GraphElement::GraphLiteral(RdfLiteral::BooleanLiteral(
        is_numeric,
    )))
}

pub(crate) fn eval_fn_sameterm(
    args: &[Expression],
    sub: &PartialSub,
    datastore: &Datastore,
) -> Option<GraphElement> {
    let a = eval_expression_value_inner(args.first()?, sub, datastore)?;
    let b = eval_expression_value_inner(args.get(1)?, sub, datastore)?;
    Some(GraphElement::GraphLiteral(RdfLiteral::BooleanLiteral(
        a == b,
    )))
}

// ── Numeric functions ─────────────────────────────────────────────────
//
// ABS/CEIL/FLOOR/ROUND all go through `classify_numeric` for their
// input (rather than matching `RdfLiteral` variants by hand) and
// `numeric_lit_to_element` for their output. This matters on both
// ends (#228): `classify_numeric` recognizes a real `TypedLiteral{
// xsd:decimal/xsd:float/xsd:double, .. }` input for what it actually
// is instead of falling through to an `xsd:double`-promoting
// catch-all (the bug `ABS` had — a real `xsd:decimal` input silently
// became `xsd:double` output), and `numeric_lit_to_element` emits the
// `TypedLiteral` shape real data uses so the result can join against
// already-interned data of the same value.
pub(crate) fn eval_fn_abs(
    args: &[Expression],
    sub: &PartialSub,
    datastore: &Datastore,
) -> Option<GraphElement> {
    let el = eval_expression_value_inner(args.first()?, sub, datastore)?;
    let lit = match &el {
        GraphElement::GraphLiteral(lit) => lit,
        _ => return None,
    };
    let abs = match classify_numeric(lit)? {
        NumericLit::Integer(n) => NumericLit::Integer(if n < BigInt::from(0) { -n } else { n }),
        NumericLit::Decimal(d) => NumericLit::Decimal(d.abs()),
        NumericLit::Float(f) => NumericLit::Float(f.abs()),
        NumericLit::Double(f) => NumericLit::Double(f.abs()),
    };
    Some(numeric_lit_to_element(abs))
}

// CEIL/FLOOR/ROUND preserve the operand's numeric type (SPARQL 1.1
// §17.4.5's `fn:round`/`fn:ceiling`/`fn:floor` semantics): an
// `xsd:integer` input passes through unchanged, an `xsd:decimal`
// input stays `xsd:decimal` (rounded to a whole-number *value*, not
// cast to `xsd:integer` — e.g. `ROUND("-1.6"^^xsd:decimal)` is
// `"-2"^^xsd:decimal`, not `"-2"^^xsd:integer`), and float/double
// stay float/double. An earlier version always promoted the result
// to `xsd:integer` regardless of input type, which failed the W3C
// `round01`/`ceil01`/`floor01` fixtures on exact-datatype comparison
// (#205). An already-integer input is passed through exactly via
// `classify_numeric` rather than round-tripping through `f64`
// (avoiding precision loss for values outside `f64`'s exact integer
// range).
pub(crate) fn eval_fn_round(
    args: &[Expression],
    sub: &PartialSub,
    datastore: &Datastore,
) -> Option<GraphElement> {
    let el = eval_expression_value_inner(args.first()?, sub, datastore)?;
    let lit = match &el {
        GraphElement::GraphLiteral(lit) => lit,
        _ => return None,
    };
    match classify_numeric(lit)? {
        NumericLit::Integer(n) => Some(numeric_lit_to_element(NumericLit::Integer(n))),
        // SPARQL/XPath `fn:round` breaks ties toward positive
        // infinity (`ROUND(2.5) = 3`, `ROUND(-2.5) = -2`, NOT -3) —
        // i.e. `floor(d + 0.5)`, not `rust_decimal`'s
        // `MidpointAwayFromZero` (which rounds -2.5 to -3). Computed
        // in exact `Decimal` arithmetic (add 1/2, then floor) rather
        // than round-tripping through `f64`, matching the spec test
        // already covering this exact case
        // (`spec_s17_round_negative_half_toward_positive_infinity`).
        NumericLit::Decimal(d) => Some(numeric_lit_to_element(NumericLit::Decimal(
            (d + rust_decimal::Decimal::new(5, 1)).floor(),
        ))),
        NumericLit::Float(f) => Some(numeric_lit_to_element(NumericLit::Float((f + 0.5).floor()))),
        NumericLit::Double(f) => Some(numeric_lit_to_element(NumericLit::Double(
            (f + 0.5).floor(),
        ))),
    }
}

pub(crate) fn eval_fn_ceil(
    args: &[Expression],
    sub: &PartialSub,
    datastore: &Datastore,
) -> Option<GraphElement> {
    let el = eval_expression_value_inner(args.first()?, sub, datastore)?;
    let lit = match &el {
        GraphElement::GraphLiteral(lit) => lit,
        _ => return None,
    };
    match classify_numeric(lit)? {
        NumericLit::Integer(n) => Some(numeric_lit_to_element(NumericLit::Integer(n))),
        NumericLit::Decimal(d) => Some(numeric_lit_to_element(NumericLit::Decimal(d.ceil()))),
        NumericLit::Float(f) => Some(numeric_lit_to_element(NumericLit::Float(f.ceil()))),
        NumericLit::Double(f) => Some(numeric_lit_to_element(NumericLit::Double(f.ceil()))),
    }
}

pub(crate) fn eval_fn_floor(
    args: &[Expression],
    sub: &PartialSub,
    datastore: &Datastore,
) -> Option<GraphElement> {
    let el = eval_expression_value_inner(args.first()?, sub, datastore)?;
    let lit = match &el {
        GraphElement::GraphLiteral(lit) => lit,
        _ => return None,
    };
    match classify_numeric(lit)? {
        NumericLit::Integer(n) => Some(numeric_lit_to_element(NumericLit::Integer(n))),
        NumericLit::Decimal(d) => Some(numeric_lit_to_element(NumericLit::Decimal(d.floor()))),
        NumericLit::Float(f) => Some(numeric_lit_to_element(NumericLit::Float(f.floor()))),
        NumericLit::Double(f) => Some(numeric_lit_to_element(NumericLit::Double(f.floor()))),
    }
}

// ── Logic / control ───────────────────────────────────────────────────
pub(crate) fn eval_fn_coalesce(
    args: &[Expression],
    sub: &PartialSub,
    datastore: &Datastore,
) -> Option<GraphElement> {
    args.iter()
        .find_map(|arg| eval_expression_value_inner(arg, sub, datastore))
}

pub(crate) fn eval_fn_if(
    args: &[Expression],
    sub: &PartialSub,
    datastore: &Datastore,
) -> Option<GraphElement> {
    let cond_el = eval_expression_value_inner(args.first()?, sub, datastore)?;
    let cond = element_to_bool(&cond_el)?;
    if cond {
        eval_expression_value_inner(args.get(1)?, sub, datastore)
    } else {
        eval_expression_value_inner(args.get(2)?, sub, datastore)
    }
}

// ── Blank nodes ───────────────────────────────────────────────────────
// BNODE() always mints a fresh blank node. BNODE(str) (SPARQL 1.1
// §17.4.2.7) must return the *same* blank node for repeated calls
// with the same simple-literal argument string within a single query
// solution, and a fresh one across solutions/no argument. The
// "within a single query solution" scoping is provided by
// `BNODE_MEMO` being cleared at each solution-row boundary
// (`BnodeMemoGuard`, installed in `project_with_exprs_partial`/
// `eval_bind_expr`) rather than being reasoned about here. Only a
// bare simple literal (no lang tag/datatype) counts as a valid
// memoization key — matching `STRDT`'s/`STRLANG`'s simple-literal
// check above — so e.g. `BNODE("1")` and `BNODE(1)` never collide.
// See #346.
pub(crate) fn eval_fn_bnode(
    args: &[Expression],
    sub: &PartialSub,
    datastore: &Datastore,
) -> Option<GraphElement> {
    use std::sync::atomic::{AtomicU32, Ordering};
    static BNODE_COUNTER: AtomicU32 = AtomicU32::new(0);
    let arg_key = match args.first() {
        Some(arg) => {
            let el = eval_expression_value_inner(arg, sub, datastore)?;
            match el {
                GraphElement::GraphLiteral(RdfLiteral::LiteralString(s)) => Some(s),
                _ => return None,
            }
        }
        None => None,
    };
    if let Some(key) = &arg_key {
        if let Some(existing) = BNODE_MEMO.with(|c| c.borrow().get(key).cloned()) {
            return Some(existing);
        }
    }
    let id = BNODE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let fresh = GraphElement::NodeOrEdge(dag_rdf::RdfResource::AnonymousBlankNode(id));
    if let Some(key) = arg_key {
        BNODE_MEMO.with(|c| c.borrow_mut().insert(key, fresh.clone()));
    }
    Some(fresh)
}

// ── String functions (continued) ──────────────────────────────────────
pub(crate) fn eval_fn_encode_for_uri(
    args: &[Expression],
    sub: &PartialSub,
    datastore: &Datastore,
) -> Option<GraphElement> {
    let el = eval_expression_value_inner(args.first()?, sub, datastore)?;
    let s = graph_element_to_string(&el)?;
    let mut out = String::new();
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    Some(GraphElement::GraphLiteral(RdfLiteral::LiteralString(out)))
}

// REPLACE (SPARQL 1.1 §17.4.3.15, `fn:replace`) requires its subject
// to be a genuine string literal (errors on e.g. a numeric operand —
// W3C `replace01`'s `:s7` case) and preserves that operand's
// simple/lang/xsd:string tag on output (#205).
pub(crate) fn eval_fn_replace(
    args: &[Expression],
    sub: &PartialSub,
    datastore: &Datastore,
) -> Option<GraphElement> {
    let el = eval_expression_value_inner(args.first()?, sub, datastore)?;
    let (s, tag) = literal_str_tag(&el)?;
    let pat_el = eval_expression_value_inner(args.get(1)?, sub, datastore)?;
    let pat = graph_element_to_string(&pat_el)?;
    let rep_el = eval_expression_value_inner(args.get(2)?, sub, datastore)?;
    let rep = graph_element_to_string(&rep_el)?;
    let flags = if let Some(flag_expr) = args.get(3) {
        if let Some(f_el) = eval_expression_value_inner(flag_expr, sub, datastore) {
            graph_element_to_string(&f_el).unwrap_or_default()
        } else {
            String::new()
        }
    } else {
        String::new()
    };
    let pattern = if flags.contains('i') {
        format!("(?i){pat}")
    } else {
        pat
    };
    let re = regex::Regex::new(&pattern).ok()?;
    Some(str_tag_to_element(
        re.replace_all(&s, rep.as_str()).into_owned(),
        tag,
    ))
}

// ── Numeric functions (random) ────────────────────────────────────────
pub(crate) fn eval_fn_rand(
    _args: &[Expression],
    _sub: &PartialSub,
    _datastore: &Datastore,
) -> Option<GraphElement> {
    use rand::Rng;
    let v: f64 = rand::thread_rng().gen();
    Some(GraphElement::GraphLiteral(RdfLiteral::DoubleLiteral(
        v.into(),
    )))
}

// ── Datetime functions ────────────────────────────────────────────────
//
// YEAR/MONTH/DAY/HOURS/MINUTES/SECONDS weren't in #228's enumerated
// scope, but are the exact same producer/lookup bug: extracting a
// date/time component and emitting a native `IntegerLiteral`/
// `DecimalLiteral` instead of `TypedLiteral` via
// `numeric_lit_to_element` would mean `BIND(YEAR(?d) AS ?z) . ?s :p
// ?z` fails to join for the same structural-inequality reason ABS
// did. Fixed here too rather than left to resurface as issue #4 of
// the same recurring pattern (see #228's "recurring pattern"
// section). `NOW()`'s native `DateTimeLiteral` is deliberately left
// as-is: its value is the current instant, which cannot coincide
// with already-interned data, so the join-lookup bug can't manifest.
pub(crate) fn eval_fn_now(
    _args: &[Expression],
    _sub: &PartialSub,
    _datastore: &Datastore,
) -> Option<GraphElement> {
    Some(GraphElement::GraphLiteral(RdfLiteral::DateTimeLiteral(
        chrono::Utc::now(),
    )))
}

pub(crate) fn eval_fn_year(
    args: &[Expression],
    sub: &PartialSub,
    datastore: &Datastore,
) -> Option<GraphElement> {
    let el = eval_expression_value_inner(args.first()?, sub, datastore)?;
    let dt = parse_xsd_datetime(&el)?;
    use chrono::Datelike;
    Some(numeric_lit_to_element(NumericLit::Integer(BigInt::from(
        dt.year(),
    ))))
}

pub(crate) fn eval_fn_month(
    args: &[Expression],
    sub: &PartialSub,
    datastore: &Datastore,
) -> Option<GraphElement> {
    let el = eval_expression_value_inner(args.first()?, sub, datastore)?;
    let dt = parse_xsd_datetime(&el)?;
    use chrono::Datelike;
    Some(numeric_lit_to_element(NumericLit::Integer(BigInt::from(
        dt.month(),
    ))))
}

pub(crate) fn eval_fn_day(
    args: &[Expression],
    sub: &PartialSub,
    datastore: &Datastore,
) -> Option<GraphElement> {
    let el = eval_expression_value_inner(args.first()?, sub, datastore)?;
    let dt = parse_xsd_datetime(&el)?;
    use chrono::Datelike;
    Some(numeric_lit_to_element(NumericLit::Integer(BigInt::from(
        dt.day(),
    ))))
}

pub(crate) fn eval_fn_hours(
    args: &[Expression],
    sub: &PartialSub,
    datastore: &Datastore,
) -> Option<GraphElement> {
    let el = eval_expression_value_inner(args.first()?, sub, datastore)?;
    let dt = parse_xsd_datetime_local(&el)?;
    use chrono::Timelike;
    Some(numeric_lit_to_element(NumericLit::Integer(BigInt::from(
        dt.hour(),
    ))))
}

pub(crate) fn eval_fn_minutes(
    args: &[Expression],
    sub: &PartialSub,
    datastore: &Datastore,
) -> Option<GraphElement> {
    let el = eval_expression_value_inner(args.first()?, sub, datastore)?;
    let dt = parse_xsd_datetime_local(&el)?;
    use chrono::Timelike;
    Some(numeric_lit_to_element(NumericLit::Integer(BigInt::from(
        dt.minute(),
    ))))
}

pub(crate) fn eval_fn_seconds(
    args: &[Expression],
    sub: &PartialSub,
    datastore: &Datastore,
) -> Option<GraphElement> {
    let el = eval_expression_value_inner(args.first()?, sub, datastore)?;
    let dt = parse_xsd_datetime_local(&el)?;
    use chrono::Timelike;
    Some(numeric_lit_to_element(NumericLit::Decimal(
        rust_decimal::Decimal::from(dt.second()),
    )))
}

pub(crate) fn eval_fn_tz(
    args: &[Expression],
    sub: &PartialSub,
    datastore: &Datastore,
) -> Option<GraphElement> {
    let el = eval_expression_value_inner(args.first()?, sub, datastore)?;
    let tz_str = extract_tz_string(&el)?;
    Some(GraphElement::GraphLiteral(RdfLiteral::LiteralString(
        tz_str,
    )))
}

// `TIMEZONE()` (SPARQL 1.1 §17.4.4, `fn:timezone-from-dateTime`)
// differs from `TZ()`: it returns an `xsd:dayTimeDuration` value
// (e.g. `"-PT8H"`, `"PT0S"`) and, per the spec, is an *error* (so the
// whole expression is unbound) when the operand has no timezone —
// whereas `TZ()` returns the empty string for that case. Genuinely
// missing prior to #205 (only `TZ` existed).
pub(crate) fn eval_fn_timezone(
    args: &[Expression],
    sub: &PartialSub,
    datastore: &Datastore,
) -> Option<GraphElement> {
    let el = eval_expression_value_inner(args.first()?, sub, datastore)?;
    let tz_str = extract_tz_string(&el)?;
    if tz_str.is_empty() {
        return None;
    }
    let (sign, hh, mm) = if tz_str == "Z" {
        ('+', 0i64, 0i64)
    } else {
        let sign = if tz_str.starts_with('-') { '-' } else { '+' };
        let rest = &tz_str[1..];
        let mut parts = rest.split(':');
        let hh: i64 = parts.next()?.parse().ok()?;
        let mm: i64 = parts.next().unwrap_or("0").parse().ok()?;
        (sign, hh, mm)
    };
    let duration = if hh == 0 && mm == 0 {
        "PT0S".to_string()
    } else if mm == 0 {
        format!("{sign}PT{hh}H")
    } else {
        format!("{sign}PT{hh}H{mm}M")
    };
    Some(GraphElement::GraphLiteral(RdfLiteral::TypedLiteral {
        type_iri: IriReference("http://www.w3.org/2001/XMLSchema#dayTimeDuration".to_string()),
        literal: duration,
    }))
}

// ── Hash functions ────────────────────────────────────────────────────
pub(crate) fn eval_fn_md5(
    args: &[Expression],
    sub: &PartialSub,
    datastore: &Datastore,
) -> Option<GraphElement> {
    let el = eval_expression_value_inner(args.first()?, sub, datastore)?;
    let s = graph_element_to_string(&el)?;
    let hash = md5::compute(s.as_bytes());
    Some(GraphElement::GraphLiteral(RdfLiteral::LiteralString(
        format!("{hash:x}"),
    )))
}

pub(crate) fn eval_fn_sha1(
    args: &[Expression],
    sub: &PartialSub,
    datastore: &Datastore,
) -> Option<GraphElement> {
    let el = eval_expression_value_inner(args.first()?, sub, datastore)?;
    let s = graph_element_to_string(&el)?;
    use sha1::Digest;
    Some(GraphElement::GraphLiteral(RdfLiteral::LiteralString(
        hex::encode(sha1::Sha1::digest(s.as_bytes())),
    )))
}

pub(crate) fn eval_fn_sha256(
    args: &[Expression],
    sub: &PartialSub,
    datastore: &Datastore,
) -> Option<GraphElement> {
    let el = eval_expression_value_inner(args.first()?, sub, datastore)?;
    let s = graph_element_to_string(&el)?;
    use sha2::Digest;
    Some(GraphElement::GraphLiteral(RdfLiteral::LiteralString(
        hex::encode(sha2::Sha256::digest(s.as_bytes())),
    )))
}

pub(crate) fn eval_fn_sha384(
    args: &[Expression],
    sub: &PartialSub,
    datastore: &Datastore,
) -> Option<GraphElement> {
    let el = eval_expression_value_inner(args.first()?, sub, datastore)?;
    let s = graph_element_to_string(&el)?;
    use sha2::Digest;
    Some(GraphElement::GraphLiteral(RdfLiteral::LiteralString(
        hex::encode(sha2::Sha384::digest(s.as_bytes())),
    )))
}

pub(crate) fn eval_fn_sha512(
    args: &[Expression],
    sub: &PartialSub,
    datastore: &Datastore,
) -> Option<GraphElement> {
    let el = eval_expression_value_inner(args.first()?, sub, datastore)?;
    let s = graph_element_to_string(&el)?;
    use sha2::Digest;
    Some(GraphElement::GraphLiteral(RdfLiteral::LiteralString(
        hex::encode(sha2::Sha512::digest(s.as_bytes())),
    )))
}

// ── UUID functions ────────────────────────────────────────────────────
pub(crate) fn eval_fn_uuid(
    _args: &[Expression],
    _sub: &PartialSub,
    _datastore: &Datastore,
) -> Option<GraphElement> {
    let id = uuid::Uuid::new_v4();
    Some(GraphElement::NodeOrEdge(dag_rdf::RdfResource::Iri(
        IriReference(format!("urn:uuid:{id}")),
    )))
}

pub(crate) fn eval_fn_struuid(
    _args: &[Expression],
    _sub: &PartialSub,
    _datastore: &Datastore,
) -> Option<GraphElement> {
    let id = uuid::Uuid::new_v4();
    Some(GraphElement::GraphLiteral(RdfLiteral::LiteralString(
        id.to_string(),
    )))
}

/// Parse an `xsd:dateTime` or `xsd:date` lexical form into a `chrono::DateTime<Utc>`.
/// Accepts full RFC 3339 (`xsd:dateTime` with a timezone/`Z`), a timezone-less
/// `xsd:dateTime` lexical form (naive datetime, assumed UTC — RFC 3339 alone
/// requires an offset but the XSD dateTime lexical space does not), and
/// `xsd:date` (`YYYY-MM-DD`, normalized to midnight UTC). Shared by
/// `parse_xsd_datetime` (which additionally falls back to bare `xsd:gYear`
/// for `YEAR`/`MONTH`/`DAY`) and the `xsd:dateTime` cast (`cast_to_xsd_datetime`),
/// which intentionally does NOT get the gYear fallback — a bare year is not a
/// valid cast source per the XPath casting rules (see #194).
pub(crate) fn parse_datetime_or_date_lexical(s: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    // Full RFC 3339 dateTime (requires a timezone offset or 'Z').
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return Some(dt.with_timezone(&chrono::Utc));
    }
    // xsd:dateTime lexical form without a timezone (naive; assumed UTC).
    if let Ok(ndt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f") {
        return Some(ndt.and_utc());
    }
    // xsd:date (YYYY-MM-DD)
    if let Ok(d) = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        return d.and_hms_opt(0, 0, 0).map(|ndt| ndt.and_utc());
    }
    None
}

/// Parse an XSD dateTime (or date/gYear) graph element into a `chrono::DateTime<Utc>`.
/// Handles `DateTimeLiteral`, RFC 3339 `xsd:dateTime` strings, `xsd:date` (YYYY-MM-DD),
/// and `xsd:gYear` (YYYY) so that YEAR/MONTH/DAY work on all common date types.
pub(crate) fn parse_xsd_datetime(el: &GraphElement) -> Option<chrono::DateTime<chrono::Utc>> {
    match el {
        GraphElement::GraphLiteral(RdfLiteral::DateTimeLiteral(dt)) => Some(*dt),
        GraphElement::GraphLiteral(RdfLiteral::TypedLiteral { literal, .. }) => {
            if let Some(dt) = parse_datetime_or_date_lexical(literal) {
                return Some(dt);
            }
            // xsd:gYear ("YYYY")
            if let Ok(y) = literal.parse::<i32>() {
                return chrono::NaiveDate::from_ymd_opt(y, 1, 1)
                    .and_then(|d| d.and_hms_opt(0, 0, 0))
                    .map(|ndt| ndt.and_utc());
            }
            None
        }
        _ => None,
    }
}

/// Parse an XSD dateTime graph element into a `chrono::DateTime<FixedOffset>`
/// that preserves the *lexical* timezone offset instead of normalising to
/// UTC. SPARQL 1.1 §17.4.4's `HOURS`/`MINUTES`/`SECONDS` (`fn:hours-from-dateTime`
/// etc.) report the time-of-day components as written in the source literal,
/// not shifted to UTC — e.g. `HOURS("2010-12-21T15:38:02-08:00"^^xsd:dateTime)`
/// is `15`, not `23`. `parse_xsd_datetime`'s `with_timezone(&Utc)` conversion
/// is correct for `YEAR`/`MONTH`/`DAY` in every W3C fixture (none of them
/// cross a date boundary under UTC normalisation) but silently breaks HOURS
/// whenever the offset is non-zero (W3C `hours-01`, #205). A native
/// `DateTimeLiteral` (produced only by `NOW()`) has no separate offset to
/// preserve, so it is treated as UTC (offset `+00:00`).
pub(crate) fn parse_xsd_datetime_local(
    el: &GraphElement,
) -> Option<chrono::DateTime<chrono::FixedOffset>> {
    match el {
        GraphElement::GraphLiteral(RdfLiteral::DateTimeLiteral(dt)) => Some(dt.fixed_offset()),
        GraphElement::GraphLiteral(RdfLiteral::TypedLiteral { literal, .. }) => {
            if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(literal) {
                return Some(dt);
            }
            // Timezone-less xsd:dateTime lexical form: treat as UTC.
            if let Ok(ndt) = chrono::NaiveDateTime::parse_from_str(literal, "%Y-%m-%dT%H:%M:%S%.f")
            {
                return Some(ndt.and_utc().fixed_offset());
            }
            None
        }
        _ => None,
    }
}

/// Extract the timezone string from an XSD dateTime graph element.
/// Returns `"Z"` for UTC, `"+HH:MM"` / `"-HH:MM"` for fixed offsets, and
/// `""` for naive (no-timezone) values.
pub(crate) fn extract_tz_string(el: &GraphElement) -> Option<String> {
    let raw = match el {
        GraphElement::GraphLiteral(RdfLiteral::DateTimeLiteral(_)) => return Some("Z".to_string()),
        GraphElement::GraphLiteral(RdfLiteral::TypedLiteral { literal, .. }) => literal.as_str(),
        _ => return None,
    };
    if raw.ends_with('Z') {
        return Some("Z".to_string());
    }
    // After the 'T' separator the time portion is HH:MM:SS[.frac].
    // A timezone offset ('+'/'-') can only appear after the seconds.
    if let Some(t_pos) = raw.find('T') {
        let after_t = &raw[t_pos + 1..];
        for (i, c) in after_t.char_indices() {
            if i >= 5 && (c == '+' || c == '-') {
                return Some(after_t[i..].to_string());
            }
        }
    }
    Some(String::new())
}

pub(crate) fn eval_function_bool(
    name: &str,
    args: &[Expression],
    sub: &PartialSub,
    datastore: &Datastore,
) -> Option<bool> {
    // `xsd:boolean(v)` used directly in a boolean context (e.g.
    // `FILTER(xsd:boolean(?x))`). The other XSD cast targets (integer,
    // decimal, double, float, string) don't produce a boolean value, so — per
    // this codebase's existing (narrow, non-EBV-coercing) boolean-context
    // conventions, see `element_to_bool` — they're intentionally left
    // unhandled here rather than inventing a general effective-boolean-value
    // coercion just for casts.
    if name == XSD_BOOLEAN {
        let el = eval_expression_value_inner(args.first()?, sub, datastore)?;
        let cast = cast_to_xsd_boolean(&el)?;
        return element_to_bool(&cast);
    }
    let upper = name.to_ascii_uppercase();
    match upper.as_str() {
        "STRSTARTS" | "STRENDS" | "CONTAINS" => {
            eval_string_predicate(upper.as_str(), args, sub, datastore)
        }
        "BOUND" => {
            if let Some(Expression::Variable(v)) = args.first() {
                Some(sub.contains_key(v))
            } else {
                None
            }
        }
        "REGEX" => {
            let text_el = eval_expression_value_inner(args.first()?, sub, datastore)?;
            let text = graph_element_to_string(&text_el)?;

            let pat_el = eval_expression_value_inner(args.get(1)?, sub, datastore)?;
            let pattern = graph_element_to_string(&pat_el)?;

            // Flags (optional 3rd arg)
            let flags = if let Some(flag_expr) = args.get(2) {
                let fel = eval_expression_value_inner(flag_expr, sub, datastore)?;
                graph_element_to_string(&fel).unwrap_or_default()
            } else {
                String::new()
            };

            // SPARQL 1.1 §17.4.3.14: REGEX performs a genuine XPath-style
            // regular-expression match (`fn:matches`), not a substring test.
            // A prior `text.contains(pattern)` implementation silently
            // treated every pattern as a literal substring, so anchors
            // (`^`/`$`), character classes (`[0-9A-F]`), and repetition
            // (`{8}`) never worked — e.g. UUID-shape validation in the W3C
            // `uuid01`/`struuid01` fixtures always failed. See #205.
            let mut pattern_str = pattern.clone();
            let mut inline_flags = String::new();
            for f in flags.chars() {
                match f {
                    'i' => inline_flags.push('i'),
                    's' => inline_flags.push('s'),
                    'm' => inline_flags.push('m'),
                    'x' => inline_flags.push('x'),
                    _ => {}
                }
            }
            if !inline_flags.is_empty() {
                pattern_str = format!("(?{inline_flags}){pattern_str}");
            }
            let re = regex::Regex::new(&pattern_str).ok()?;
            Some(re.is_match(&text))
        }
        "LANGMATCHES" => {
            let lang_el = eval_expression_value_inner(args.first()?, sub, datastore)?;
            let lang = graph_element_to_string(&lang_el)?.to_lowercase();

            let range_el = eval_expression_value_inner(args.get(1)?, sub, datastore)?;
            let range = graph_element_to_string(&range_el)?.to_lowercase();

            Some(if range == "*" {
                !lang.is_empty()
            } else {
                lang == range || lang.starts_with(&format!("{}-", range))
            })
        }
        "ISIRI" | "ISURI" => {
            let el = eval_expression_value_inner(args.first()?, sub, datastore)?;
            Some(matches!(
                el,
                dag_rdf::GraphElement::NodeOrEdge(dag_rdf::RdfResource::Iri(_))
            ))
        }
        "ISBLANK" => {
            let el = eval_expression_value_inner(args.first()?, sub, datastore)?;
            Some(matches!(
                el,
                dag_rdf::GraphElement::NodeOrEdge(dag_rdf::RdfResource::AnonymousBlankNode(_))
            ))
        }
        "ISLITERAL" => {
            let el = eval_expression_value_inner(args.first()?, sub, datastore)?;
            Some(matches!(el, dag_rdf::GraphElement::GraphLiteral(_)))
        }
        // Fallback: any function not given a dedicated boolean-context arm
        // above (e.g. `ISNUMERIC`, `SAMETERM`) may still be usable in a
        // boolean position (`FILTER isNumeric(?x)`) if `eval_function_value`
        // computes an `xsd:boolean`-typed result for it. Without this, such
        // functions silently evaluate to `None` in `FILTER`/boolean contexts
        // even though they work fine inside `BIND`/projections, which used to
        // make `FILTER isNumeric(?num)` reject every row (see #205).
        _ => {
            let el = eval_function_value(name, args, sub, datastore)?;
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

/// Extract an integer from either `IntegerLiteral` or `TypedLiteral(xsd:integer)`.
pub(crate) fn element_to_usize(el: &GraphElement) -> Option<usize> {
    match el {
        GraphElement::GraphLiteral(RdfLiteral::IntegerLiteral(n)) => n.to_string().parse().ok(),
        GraphElement::GraphLiteral(RdfLiteral::TypedLiteral { type_iri, literal })
            if type_iri.0 == XSD_INTEGER =>
        {
            literal.parse().ok()
        }
        _ => None,
    }
}

/// Coerce a boolean from either `BooleanLiteral` or `TypedLiteral(xsd:boolean)`.
pub(crate) fn element_to_bool(el: &GraphElement) -> Option<bool> {
    match el {
        GraphElement::GraphLiteral(RdfLiteral::BooleanLiteral(b)) => Some(*b),
        GraphElement::GraphLiteral(RdfLiteral::TypedLiteral { type_iri, literal })
            if type_iri.0 == XSD_BOOLEAN =>
        {
            Some(literal == "true")
        }
        _ => None,
    }
}

pub(crate) fn graph_element_to_string(el: &GraphElement) -> Option<String> {
    match el {
        GraphElement::GraphLiteral(RdfLiteral::LiteralString(s)) => Some(s.clone()),
        GraphElement::GraphLiteral(RdfLiteral::LangLiteral { literal, .. }) => {
            Some(literal.clone())
        }
        GraphElement::GraphLiteral(RdfLiteral::TypedLiteral { literal, .. }) => {
            Some(literal.clone())
        }
        GraphElement::NodeOrEdge(dag_rdf::RdfResource::Iri(iri)) => Some(iri.0.clone()),
        _ => None,
    }
}

/// A string-valued literal's "tag": whether it's a simple literal, has a
/// language tag, or is explicitly `xsd:string`-typed. SPARQL 1.1 §17.4.3's
/// string functions (`UCASE`, `LCASE`, `SUBSTR`, `STRBEFORE`, `STRAFTER`,
/// `REPLACE`, `CONCAT`) must propagate this tag from their input(s) to their
/// output rather than always emitting a plain simple literal — losing it
/// caused every W3C string-function fixture that used a language-tagged or
/// `xsd:string`-typed operand to fail on exact-datatype comparison (#205).
#[derive(Clone, PartialEq, Eq)]
pub(crate) enum StrLitTag {
    Plain,
    Lang(String),
    XsdString,
}

/// Extract a string literal's lexical value and `StrLitTag`. Returns `None`
/// for anything that isn't a simple/lang/xsd:string literal (IRIs, numbers,
/// booleans, dates, blank nodes, other typed literals) — per spec, the
/// string functions this feeds are only defined over string-valued operands
/// and must error (propagate `None`) on anything else.
pub(crate) fn literal_str_tag(el: &GraphElement) -> Option<(String, StrLitTag)> {
    match el {
        GraphElement::GraphLiteral(RdfLiteral::LiteralString(s)) => {
            Some((s.clone(), StrLitTag::Plain))
        }
        GraphElement::GraphLiteral(RdfLiteral::LangLiteral { literal, lang }) => {
            Some((literal.clone(), StrLitTag::Lang(lang.clone())))
        }
        GraphElement::GraphLiteral(RdfLiteral::TypedLiteral { literal, type_iri })
            if type_iri.0 == XSD_STRING =>
        {
            Some((literal.clone(), StrLitTag::XsdString))
        }
        _ => None,
    }
}

/// Reconstruct a `GraphElement` from a computed string value and the
/// `StrLitTag` it should carry.
pub(crate) fn str_tag_to_element(s: String, tag: StrLitTag) -> GraphElement {
    match tag {
        StrLitTag::Plain => GraphElement::GraphLiteral(RdfLiteral::LiteralString(s)),
        StrLitTag::Lang(lang) => {
            GraphElement::GraphLiteral(RdfLiteral::LangLiteral { lang, literal: s })
        }
        StrLitTag::XsdString => GraphElement::GraphLiteral(RdfLiteral::TypedLiteral {
            type_iri: IriReference(XSD_STRING.to_string()),
            literal: s,
        }),
    }
}

/// SPARQL 1.1 §17.1's "argument compatibility" rule for two string operands
/// (used by `STRBEFORE`/`STRAFTER`'s second argument, and by other
/// string-comparison builtins): compatible if `arg2` has no language tag (is
/// a simple literal or `xsd:string`), or if both share the exact same
/// language tag. Two literals with *different* language tags are not
/// compatible, and the containing function must error (`None`).
pub(crate) fn str_args_compatible(tag1: &StrLitTag, tag2: &StrLitTag) -> bool {
    match tag2 {
        StrLitTag::Plain | StrLitTag::XsdString => true,
        StrLitTag::Lang(l2) => matches!(tag1, StrLitTag::Lang(l1) if l1 == l2),
    }
}

/// Extract a numeric f64 from a literal if it has a numeric datatype.
pub(crate) fn literal_to_f64(lit: &RdfLiteral) -> Option<f64> {
    match lit {
        RdfLiteral::IntegerLiteral(i) => i.to_string().parse().ok(),
        RdfLiteral::DoubleLiteral(d) => Some(d.into_inner()),
        RdfLiteral::DecimalLiteral(d) => Some(d.to_string().parse().ok()?),
        RdfLiteral::FloatLiteral(f) => Some(f.into_inner()),
        RdfLiteral::TypedLiteral { type_iri, literal } => {
            let iri = &type_iri.0;
            if iri == XSD_INTEGER || iri == XSD_DECIMAL || iri == XSD_DOUBLE || iri == XSD_FLOAT {
                literal.parse().ok()
            } else {
                None
            }
        }
        _ => None,
    }
}

// ── XSD datatype constructor/cast functions (SPARQL 1.1 §17.4.2, #190) ────
//
// `xsd:integer(v)`, `xsd:decimal(v)`, `xsd:double(v)`, `xsd:float(v)`,
// `xsd:string(v)`, `xsd:boolean(v)`, `xsd:dateTime(v)` (#194): given an
// appropriately-typed input (numeric literal, boolean, string, or
// matching-lexical-form typed literal), produce a new literal of the target
// datatype. An invalid conversion returns `None` — per this file's
// established convention (see `ABS`/`ROUND`/etc. above), that leaves the
// enclosing expression's result unbound rather than erroring the whole
// query.

/// Equality for `=`/`!=`/`IN`/`NOT IN` (SPARQL 1.1 §17.3, §17.4.1.9).
///
/// As of #228, every scalar computed-value producer in this module (unary
/// minus, binary arithmetic, `ABS`/`CEIL`/`FLOOR`/`ROUND`, the xsd casts)
/// emits the same `TypedLiteral { type_iri, literal }` shape that literals
/// parsed directly from SPARQL query text or Turtle data use
/// (`parse_numeric_literal`/`parse_boolean_literal` in `lib.rs`,
/// `turtle::convert_literal`) — see `numeric_lit_to_element`. Only
/// aggregates (`SUM`/`COUNT`/`AVG`/etc., which cannot appear inside `BIND`)
/// still produce the native `RdfLiteral` variants (`IntegerLiteral`,
/// `DecimalLiteral`, `DoubleLiteral`, `FloatLiteral`, `BooleanLiteral`). A
/// raw Rust `==` sees these as different enum variants even when they denote
/// the same value, so e.g. `SUM(?x) = 2` could wrongly compare unequal
/// against a `TypedLiteral` (#208).
///
/// This normalizes numeric and boolean literals across both representations,
/// then falls back to plain equality for every other RDF term shape (IRIs,
/// blank nodes, plain strings, language-tagged literals) — where the
/// native/parsed split does not exist and `==` was already correct.
///
/// One more cross-representation split needs normalizing here (#360): a
/// simple/plain string literal (`RdfLiteral::LiteralString`) and an
/// explicitly `xsd:string`-typed literal (`RdfLiteral::TypedLiteral {
/// type_iri: xsd:string, .. }`) are, per RDF 1.1 §5.1, the *same value* — but
/// they are different Rust enum variants, so raw `==` (and `a == b` at the
/// end of this function) sees them as unequal. This split is reachable
/// without any Turtle ingestion at all: `sparql_parser`'s own query-text
/// literal grammar (`parse_string_literal`) already parses `"foo"^^xsd:string`
/// as a distinct `TypedLiteral` rather than collapsing it (unlike
/// `turtle::convert_literal`, which historically collapsed the Turtle-syntax
/// form — see the ingestion-side investigation in
/// `docs/plans/XSD_STRING_LITERAL_360_PLAN.md`), so `"foo" = "foo"^^xsd:string`
/// in a bare `FILTER` was wrong before any ingestion change. `sameTerm`
/// deliberately keeps using raw `==` (term identity, not value equality) and
/// is unaffected by this normalization.
pub(crate) fn values_equal(a: &GraphElement, b: &GraphElement) -> bool {
    if let (GraphElement::GraphLiteral(a_lit), GraphElement::GraphLiteral(b_lit)) = (a, b) {
        if let (Some(af), Some(bf)) = (literal_to_f64(a_lit), literal_to_f64(b_lit)) {
            return af.partial_cmp(&bf) == Some(std::cmp::Ordering::Equal);
        }
        if let (Some(a_s), Some(b_s)) = (
            simple_or_xsd_string_value(a_lit),
            simple_or_xsd_string_value(b_lit),
        ) {
            return a_s == b_s;
        }
    }
    if let (Some(a_bool), Some(b_bool)) = (element_to_bool(a), element_to_bool(b)) {
        return a_bool == b_bool;
    }
    a == b
}

/// If `lit` is a simple/plain literal or an explicitly `xsd:string`-typed
/// literal, return its lexical value — the two are value-equal per RDF 1.1
/// §5.1 (see `values_equal`). Returns `None` for every other literal shape
/// (language-tagged, numeric, boolean, other datatypes), so callers only
/// normalize this one specific split and never widen matching to e.g.
/// language-tagged literals.
pub(crate) fn simple_or_xsd_string_value(lit: &RdfLiteral) -> Option<&str> {
    match lit {
        RdfLiteral::LiteralString(s) => Some(s.as_str()),
        RdfLiteral::TypedLiteral { type_iri, literal } if type_iri.0 == XSD_STRING => {
            Some(literal.as_str())
        }
        _ => None,
    }
}

/// Compare graph elements for FILTER relational operators.
/// Returns negative, 0, positive, or None if not comparable.
pub(crate) fn compare_graph_elements(a: &GraphElement, b: &GraphElement) -> Option<i32> {
    use dag_rdf::GraphElement::GraphLiteral;
    use std::cmp::Ordering::*;
    if let (GraphLiteral(a_lit), GraphLiteral(b_lit)) = (a, b) {
        // Try numeric comparison first
        if let (Some(af), Some(bf)) = (literal_to_f64(a_lit), literal_to_f64(b_lit)) {
            return af.partial_cmp(&bf).map(|o| match o {
                Less => -1,
                Equal => 0,
                Greater => 1,
            });
        }
        // String literal comparison
        let a_str = match a_lit {
            RdfLiteral::LiteralString(s) => Some(s.as_str()),
            RdfLiteral::TypedLiteral { literal, .. } => Some(literal.as_str()),
            _ => None,
        };
        let b_str = match b_lit {
            RdfLiteral::LiteralString(s) => Some(s.as_str()),
            RdfLiteral::TypedLiteral { literal, .. } => Some(literal.as_str()),
            _ => None,
        };
        if let (Some(a_s), Some(b_s)) = (a_str, b_str) {
            return Some(match a_s.cmp(b_s) {
                Less => -1,
                Equal => 0,
                Greater => 1,
            });
        }
    }
    None
}

// ── CONSTRUCT helpers ─────────────────────────────────────────────────────────

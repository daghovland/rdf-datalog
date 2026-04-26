/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Datalog rule parser (nom-based).
//!
//! Parses a semi-positive Datalog language for RDF, translating
//! `DagSemTools.Datalog.Parser` from C#/ANTLR into Rust/nom.
//!
//! ## Syntax
//!
//! ```text
//! # Prefix declarations (SPARQL-style or Turtle-style)
//! PREFIX ex: <https://example.com/data#>
//! @prefix ex2: <https://example.com/data2#> .
//!
//! # Fact (rule with empty body)
//! [?s, ex:pred, ex:obj] .
//! ex:type[?s] .
//!
//! # Proper rule: head :- body .
//! [?x, a, ?c] :- [?x, ?p, ?y], [?p, rdfs:range, ?c] .
//!
//! # Predicate-first triple: p[s, o]
//! ex:prop[?s, ex:obj] :- ex:prop2[?s, ex:obj], ex:prop3[?s, ex:obj] .
//!
//! # Type atom: p[s] means  s rdf:type p
//! ex:Employee[?x] :- ex:Manager[?x] .
//!
//! # Negation
//! ex:prop[?s, ex:obj] :- ex:prop2[?s, ex:obj], NOT ex:prop3[?s, ex:obj] .
//!
//! # Contradiction (derive ⊥)
//! false :- [?X, a, <https://example.com/BadClass>] .
//!
//! # Named graph
//! [?s, ex:p, ex:o] ?graph :- ex:p[?s, ex:o] ?graph .
//! ```
//!
//! ## Built-in prefixes
//!
//! `rdf:`, `rdfs:`, `xsd:`, and `owl:` are pre-declared.

use dag_rdf::{Datastore, IriReference, QuadPattern, RdfResource, Term, DEFAULT_GRAPH_ELEMENT_ID};
use datalog::types::{Rule, RuleAtom, RuleHead};
use nom::{
    IResult,
    bytes::complete::{take_while, take_while1},
    character::complete::{char, multispace0, multispace1},
    combinator::opt,
    sequence::pair,
};
use std::collections::HashMap;
use std::path::Path;

const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";

// ── Intermediate AST ─────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
enum ParsedTerm {
    Variable(String),
    Iri(String),
}

#[derive(Debug, Clone)]
struct ParsedQuadPattern {
    graph: ParsedTerm,
    subject: ParsedTerm,
    predicate: ParsedTerm,
    object: ParsedTerm,
}

#[derive(Debug, Clone)]
enum ParsedRuleHead {
    NormalHead(ParsedQuadPattern),
    Contradiction,
}

#[derive(Debug, Clone)]
enum ParsedRuleAtom {
    PositivePattern(ParsedQuadPattern),
    NotPattern(ParsedQuadPattern),
}

#[derive(Debug, Clone)]
struct ParsedRule {
    head: ParsedRuleHead,
    body: Vec<ParsedRuleAtom>,
}

// ── Parser context ────────────────────────────────────────────────────────────

struct ParserContext {
    prefixes: HashMap<String, String>,
    base_iri: Option<String>,
}

impl Default for ParserContext {
    fn default() -> Self {
        let mut prefixes = HashMap::new();
        prefixes.insert("rdf".into(), "http://www.w3.org/1999/02/22-rdf-syntax-ns#".into());
        prefixes.insert("rdfs".into(), "http://www.w3.org/2000/01/rdf-schema#".into());
        prefixes.insert("xsd".into(), "http://www.w3.org/2001/XMLSchema#".into());
        prefixes.insert("owl".into(), "http://www.w3.org/2002/07/owl#".into());
        ParserContext { prefixes, base_iri: None }
    }
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Parse a Datalog program from a string, interning IRIs into `datastore`.
pub fn parse(input: &str, datastore: &mut Datastore) -> Result<Vec<Rule>, String> {
    let mut ctx = ParserContext::default();
    let mut remaining = input;
    let mut parsed_rules: Vec<ParsedRule> = Vec::new();

    loop {
        remaining = skip_ws_comments(remaining);
        if remaining.is_empty() {
            break;
        }

        // Call parse_directive with a mutable borrow; the borrow ends at the
        // semicolon, so the immutable borrow for parse_rule on the next line is fine.
        let dir = parse_directive(remaining, &mut ctx);
        if let Ok((rest, _)) = dir {
            remaining = rest;
            continue;
        }

        match parse_rule(remaining, &ctx) {
            Ok((rest, rule)) => {
                parsed_rules.push(rule);
                remaining = rest;
            }
            Err(e) => {
                let snippet = &remaining[..remaining.len().min(60)];
                return Err(format!("Datalog parse error near {:?}: {:?}", snippet, e));
            }
        }
    }

    parsed_rules.into_iter().map(|r| intern_rule(r, datastore)).collect()
}

/// Parse a Datalog program from a file.
pub fn parse_file(path: &Path, datastore: &mut Datastore) -> Result<Vec<Rule>, String> {
    let input = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {}", path.display(), e))?;
    parse(&input, datastore)
}

// ── Whitespace + comment skipping ─────────────────────────────────────────────

fn skip_ws_comments(mut s: &str) -> &str {
    loop {
        s = s.trim_start_matches(|c: char| c.is_ascii_whitespace());
        if s.starts_with('#') {
            let end = s.find(['\n', '\r']).unwrap_or(s.len());
            s = &s[end..];
        } else {
            return s;
        }
    }
}

// ── Directive parsing ─────────────────────────────────────────────────────────

// Note: 'i is the input lifetime; ctx has its own shorter lifetime.
fn parse_directive<'i>(input: &'i str, ctx: &mut ParserContext) -> IResult<&'i str, ()> {
    if let Ok((rest, (name, iri))) = parse_prefix_decl(input) {
        ctx.prefixes.insert(name, iri);
        return Ok((rest, ()));
    }
    if let Ok((rest, base)) = parse_base_decl(input) {
        ctx.base_iri = Some(base);
        return Ok((rest, ()));
    }
    if let Ok((rest, _)) = parse_version_decl(input) {
        return Ok((rest, ()));
    }
    Err(nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Tag)))
}

fn parse_prefix_decl(input: &str) -> IResult<&str, (String, String)> {
    let (input, _) = alt_keyword(input, &["@prefix", "PREFIX", "prefix"])?;
    let (input, _) = multispace1(input)?;
    let (input, name) = take_while(|c: char| c.is_alphanumeric() || c == '_')(input)?;
    let (input, _) = char(':')(input)?;
    let (input, _) = multispace0(input)?;
    let (input, iri) = parse_absolute_iri_str(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = opt(char('.'))(input)?;
    Ok((input, (name.to_string(), iri)))
}

fn parse_base_decl(input: &str) -> IResult<&str, String> {
    let (input, _) = alt_keyword(input, &["@base", "BASE", "base"])?;
    let (input, _) = multispace1(input)?;
    let (input, iri) = parse_absolute_iri_str(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = opt(char('.'))(input)?;
    Ok((input, iri))
}

fn parse_version_decl(input: &str) -> IResult<&str, ()> {
    let (input, _) = alt_keyword(input, &["@version", "VERSION", "version"])?;
    let (input, _) = take_while(|c| c != '.' && c != '\n')(input)?;
    let (input, _) = opt(char('.'))(input)?;
    Ok((input, ()))
}

/// Match one of `keywords` at a word boundary (not followed by alphanumeric / `_`).
fn alt_keyword<'i>(input: &'i str, keywords: &[&str]) -> IResult<&'i str, &'i str> {
    for kw in keywords {
        if let Some(rest) = input.strip_prefix(kw) {
            let at_word_boundary = kw.starts_with('@')
                || rest.chars().next().map(|c| !c.is_alphanumeric() && c != '_').unwrap_or(true);
            if at_word_boundary {
                return Ok((rest, &input[..kw.len()]));
            }
        }
    }
    Err(nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Tag)))
}

// ── IRI parsing ───────────────────────────────────────────────────────────────

fn parse_absolute_iri_str(input: &str) -> IResult<&str, String> {
    let (input, _) = char('<')(input)?;
    let (input, content) = take_while(|c| c != '>')(input)?;
    let (input, _) = char('>')(input)?;
    Ok((input, content.to_string()))
}

fn parse_iri<'i>(input: &'i str, ctx: &ParserContext) -> IResult<&'i str, String> {
    if input.starts_with('<') {
        return parse_absolute_iri_str(input);
    }
    parse_prefixed_iri(input, ctx)
}

fn parse_prefixed_iri<'i>(input: &'i str, ctx: &ParserContext) -> IResult<&'i str, String> {
    let (after_prefix, prefix) = take_while(|c: char| c.is_alphanumeric() || c == '_')(input)?;
    let (after_colon, _) = char(':')(after_prefix)?;

    // Reject bare keyword prefixes
    let lower = prefix.to_ascii_lowercase();
    if matches!(lower.as_str(), "not" | "false" | "prefix" | "base") {
        return Err(nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Tag)));
    }

    // Local name must start with alphanumeric or '_'
    if !after_colon.chars().next().map(|c| c.is_alphanumeric() || c == '_').unwrap_or(false) {
        return Err(nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::TakeWhile1)));
    }

    let (after_local, local) = take_while(|c: char| {
        c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | '#' | '%' | '+' | '~')
    })(after_colon)?;

    let base = ctx.prefixes.get(prefix).ok_or_else(|| {
        log::warn!("Undefined prefix {:?}", prefix);
        nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Tag))
    })?;

    Ok((after_local, format!("{}{}", base, local)))
}

// ── Term parsing ──────────────────────────────────────────────────────────────

fn parse_variable_term(input: &str) -> IResult<&str, ParsedTerm> {
    let (input, _) = char('?')(input)?;
    let (input, name) = take_while1(|c: char| c.is_alphanumeric() || c == '_')(input)?;
    Ok((input, ParsedTerm::Variable(name.to_string())))
}

fn parse_rdf_type_abbr(input: &str) -> IResult<&str, ParsedTerm> {
    if let Some(rest) = input.strip_prefix('a')
        && rest.chars().next().map(|c| !c.is_alphanumeric() && c != '_' && c != ':').unwrap_or(true)
    {
        return Ok((rest, ParsedTerm::Iri(RDF_TYPE.to_string())));
    }
    Err(nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Char)))
}

fn parse_term<'i>(input: &'i str, ctx: &ParserContext) -> IResult<&'i str, ParsedTerm> {
    if input.starts_with('?') {
        return parse_variable_term(input);
    }
    if let Ok(r) = parse_rdf_type_abbr(input) {
        return Ok(r);
    }
    let (rest, iri) = parse_iri(input, ctx)?;
    Ok((rest, ParsedTerm::Iri(iri)))
}

// ── Atom parsing ──────────────────────────────────────────────────────────────

fn default_graph_term() -> ParsedTerm {
    ParsedTerm::Iri("__default_graph__".to_string())
}

/// Parse `[subject, predicate, object]`.
fn parse_bracket_triple<'i>(input: &'i str, ctx: &ParserContext) -> IResult<&'i str, ParsedQuadPattern> {
    let (input, _) = char('[')(input)?;
    let (input, _) = multispace0(input)?;
    let (input, subject) = parse_term(input, ctx)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = char(',')(input)?;
    let (input, _) = multispace0(input)?;
    let (input, predicate) = parse_term(input, ctx)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = char(',')(input)?;
    let (input, _) = multispace0(input)?;
    let (input, object) = parse_term(input, ctx)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = char(']')(input)?;
    Ok((input, ParsedQuadPattern { graph: default_graph_term(), subject, predicate, object }))
}

/// Parse `relation[subject, object]` (triple) or `relation[subject]` (type atom).
///
/// Type atom `p[s]` is sugar for `s rdf:type p`.
fn parse_predicate_first<'i>(input: &'i str, ctx: &ParserContext) -> IResult<&'i str, ParsedQuadPattern> {
    let (input, predicate) = parse_term(input, ctx)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = char('[')(input)?;
    let (input, _) = multispace0(input)?;
    let (input, first_arg) = parse_term(input, ctx)?;
    let (input, _) = multispace0(input)?;

    if input.starts_with(',') {
        let (input, _) = pair(char(','), multispace0)(input)?;
        let (input, second_arg) = parse_term(input, ctx)?;
        let (input, _) = multispace0(input)?;
        let (input, _) = char(']')(input)?;
        Ok((input, ParsedQuadPattern { graph: default_graph_term(), subject: first_arg, predicate, object: second_arg }))
    } else {
        // Type atom p[s] → s rdf:type p
        let (input, _) = char(']')(input)?;
        Ok((input, ParsedQuadPattern {
            graph: default_graph_term(),
            subject: first_arg,
            predicate: ParsedTerm::Iri(RDF_TYPE.to_string()),
            object: predicate,
        }))
    }
}

/// Parse an atom with an optional graph-name suffix.
fn parse_positive_atom<'i>(input: &'i str, ctx: &ParserContext) -> IResult<&'i str, ParsedQuadPattern> {
    let (input, mut pattern) = if input.starts_with('[') {
        parse_bracket_triple(input, ctx)?
    } else {
        parse_predicate_first(input, ctx)?
    };

    // Optional graph name: term followed immediately by a rule separator
    let (input, _) = multispace0(input)?;
    let save = input;
    if let Ok((after_term, graph)) = parse_term(input, ctx) {
        let after_ws = skip_ws_comments(after_term);
        if after_ws.starts_with(":-") || after_ws.starts_with('.') || after_ws.starts_with(',') || after_ws.is_empty() {
            pattern.graph = graph;
            return Ok((after_term, pattern));
        }
    }
    Ok((save, pattern))
}

// ── Rule parsing ──────────────────────────────────────────────────────────────

fn parse_rule<'i>(input: &'i str, ctx: &ParserContext) -> IResult<&'i str, ParsedRule> {
    let (input, head) = parse_head(input, ctx)?;
    let (input, _) = multispace0(input)?;

    if input.starts_with(":-") {
        let (input, _) = pair(char(':'), char('-'))(input)?;
        let (input, _) = multispace0(input)?;
        let (input, body) = parse_body(input, ctx)?;
        let (input, _) = multispace0(input)?;
        let (input, _) = char('.')(input)?;
        Ok((input, ParsedRule { head, body }))
    } else {
        let (input, _) = char('.')(input)?;
        Ok((input, ParsedRule { head, body: Vec::new() }))
    }
}

fn parse_head<'i>(input: &'i str, ctx: &ParserContext) -> IResult<&'i str, ParsedRuleHead> {
    if let Some(rest) = input.strip_prefix("false") {
        let next = rest.chars().next();
        if next.map(|c| !c.is_alphanumeric() && c != '_' && c != ':').unwrap_or(true) {
            return Ok((rest, ParsedRuleHead::Contradiction));
        }
    }
    let (input, pattern) = parse_positive_atom(input, ctx)?;
    Ok((input, ParsedRuleHead::NormalHead(pattern)))
}

fn parse_body<'i>(input: &'i str, ctx: &ParserContext) -> IResult<&'i str, Vec<ParsedRuleAtom>> {
    let (input, first) = parse_rule_atom(input, ctx)?;
    let mut atoms = vec![first];
    let mut remaining = input;

    loop {
        let (r, _) = multispace0(remaining)?;
        if r.starts_with(',') {
            let (r, _) = char(',')(r)?;
            let r = skip_ws_comments(r);
            let (r, atom) = parse_rule_atom(r, ctx)?;
            atoms.push(atom);
            remaining = r;
        } else {
            remaining = r;
            break;
        }
    }

    Ok((remaining, atoms))
}

fn parse_rule_atom<'i>(input: &'i str, ctx: &ParserContext) -> IResult<&'i str, ParsedRuleAtom> {
    if keyword_not(input) {
        let rest = &input[3..];
        let (rest, _) = multispace1(rest)?;
        let (rest, pattern) = parse_positive_atom(rest, ctx)?;
        return Ok((rest, ParsedRuleAtom::NotPattern(pattern)));
    }
    let (input, pattern) = parse_positive_atom(input, ctx)?;
    Ok((input, ParsedRuleAtom::PositivePattern(pattern)))
}

fn keyword_not(input: &str) -> bool {
    input.len() >= 3 && {
        let b = input.as_bytes();
        matches!(b[0], b'N' | b'n')
            && matches!(b[1], b'O' | b'o')
            && matches!(b[2], b'T' | b't')
            && input[3..].chars().next().map(|c| !c.is_alphanumeric() && c != '_').unwrap_or(true)
    }
}

// ── IRI interning: ParsedRule → Rule ─────────────────────────────────────────

fn intern_rule(parsed: ParsedRule, ds: &mut Datastore) -> Result<Rule, String> {
    let head = match parsed.head {
        ParsedRuleHead::Contradiction => RuleHead::Contradiction,
        ParsedRuleHead::NormalHead(p) => RuleHead::NormalHead(intern_quad_pattern(p, ds)?),
    };
    let body = parsed.body.into_iter().map(|a| intern_rule_atom(a, ds)).collect::<Result<_, _>>()?;
    Ok(Rule { head, body })
}

fn intern_rule_atom(atom: ParsedRuleAtom, ds: &mut Datastore) -> Result<RuleAtom, String> {
    Ok(match atom {
        ParsedRuleAtom::PositivePattern(p) => RuleAtom::PositivePattern(intern_quad_pattern(p, ds)?),
        ParsedRuleAtom::NotPattern(p) => RuleAtom::NotPattern(intern_quad_pattern(p, ds)?),
    })
}

fn intern_quad_pattern(p: ParsedQuadPattern, ds: &mut Datastore) -> Result<QuadPattern, String> {
    Ok(QuadPattern {
        graph: intern_term(p.graph, ds)?,
        subject: intern_term(p.subject, ds)?,
        predicate: intern_term(p.predicate, ds)?,
        object: intern_term(p.object, ds)?,
    })
}

fn intern_term(term: ParsedTerm, ds: &mut Datastore) -> Result<Term, String> {
    Ok(match term {
        ParsedTerm::Variable(name) => Term::Variable(name),
        ParsedTerm::Iri(iri) if iri == "__default_graph__" => Term::Resource(DEFAULT_GRAPH_ELEMENT_ID),
        ParsedTerm::Iri(iri) => Term::Resource(ds.add_node_resource(RdfResource::Iri(IriReference(iri)))),
    })
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use dag_rdf::Datastore;
    use datalog::types::RuleAtom;

    fn ds() -> Datastore { Datastore::new(10_000) }

    fn ctx_with(prefix: &str, iri: &str) -> ParserContext {
        let mut ctx = ParserContext::default();
        ctx.prefixes.insert(prefix.to_string(), iri.to_string());
        ctx
    }

    // ── Directive tests ───────────────────────────────────────────────────────

    #[test]
    fn prefix_sparql_style() {
        let mut ctx = ParserContext::default();
        parse_directive("PREFIX ex: <https://example.com/data#>", &mut ctx).unwrap();
        assert_eq!(ctx.prefixes["ex"], "https://example.com/data#");
    }

    #[test]
    fn prefix_turtle_style() {
        let mut ctx = ParserContext::default();
        parse_directive("@prefix ex: <https://example.com/data#> .", &mut ctx).unwrap();
        assert_eq!(ctx.prefixes["ex"], "https://example.com/data#");
    }

    #[test]
    fn prefix_lowercase_style() {
        let mut ctx = ParserContext::default();
        parse_directive("prefix ex: <https://example.com/data#>", &mut ctx).unwrap();
        assert_eq!(ctx.prefixes["ex"], "https://example.com/data#");
    }

    // ── IRI / term tests ──────────────────────────────────────────────────────

    #[test]
    fn absolute_iri() {
        let ctx = ParserContext::default();
        let (rest, s) = parse_iri("<https://example.com/>", &ctx).unwrap();
        assert!(rest.is_empty());
        assert_eq!(s, "https://example.com/");
    }

    #[test]
    fn prefixed_iri_expansion() {
        let ctx = ctx_with("ex", "https://example.com/data#");
        let (_, s) = parse_iri("ex:Foo", &ctx).unwrap();
        assert_eq!(s, "https://example.com/data#Foo");
    }

    #[test]
    fn a_shorthand_as_rdf_type() {
        let ctx = ParserContext::default();
        let (_, term) = parse_term("a ", &ctx).unwrap();
        assert!(matches!(term, ParsedTerm::Iri(s) if s == RDF_TYPE));
    }

    #[test]
    fn a_not_confused_with_prefix_abc() {
        let ctx = ctx_with("abc", "https://example.com/");
        let (_, term) = parse_term("abc:foo", &ctx).unwrap();
        assert!(matches!(term, ParsedTerm::Iri(s) if s == "https://example.com/foo"));
    }

    #[test]
    fn variable_parsed() {
        let ctx = ParserContext::default();
        let (rest, t) = parse_term("?myVar rest", &ctx).unwrap();
        assert_eq!(rest, " rest");
        assert!(matches!(t, ParsedTerm::Variable(s) if s == "myVar"));
    }

    #[test]
    fn colon_minus_not_parsed_as_iri() {
        // ':- ' should NOT be parsed as a prefixed IRI
        let ctx = ParserContext::default();
        assert!(parse_term(":- something", &ctx).is_err());
    }

    // ── Atom tests ────────────────────────────────────────────────────────────

    #[test]
    fn bracket_triple() {
        let ctx = ctx_with("ex", "https://example.com/data#");
        let (rest, p) = parse_bracket_triple("[?s, ex:pred, ex:obj]", &ctx).unwrap();
        assert!(rest.is_empty());
        assert!(matches!(p.subject, ParsedTerm::Variable(v) if v == "s"));
        assert!(matches!(&p.predicate, ParsedTerm::Iri(s) if s == "https://example.com/data#pred"));
        assert!(matches!(&p.object,    ParsedTerm::Iri(s) if s == "https://example.com/data#obj"));
    }

    #[test]
    fn predicate_first_triple() {
        let ctx = ctx_with("ex", "https://example.com/data#");
        let (rest, p) = parse_predicate_first("ex:prop[?s, ex:obj]", &ctx).unwrap();
        assert!(rest.is_empty());
        assert!(matches!(&p.predicate, ParsedTerm::Iri(s) if s == "https://example.com/data#prop"));
        assert!(matches!(p.subject,    ParsedTerm::Variable(v) if v == "s"));
        assert!(matches!(&p.object,    ParsedTerm::Iri(s) if s == "https://example.com/data#obj"));
    }

    #[test]
    fn type_atom_sugar() {
        // ex:type[?s] ≡  ?s rdf:type ex:type
        let ctx = ctx_with("ex", "https://example.com/data#");
        let (_, p) = parse_predicate_first("ex:type[?s]", &ctx).unwrap();
        assert!(matches!(p.subject,    ParsedTerm::Variable(v) if v == "s"));
        assert!(matches!(&p.predicate, ParsedTerm::Iri(s) if s == RDF_TYPE));
        assert!(matches!(&p.object,    ParsedTerm::Iri(s) if s == "https://example.com/data#type"));
    }

    // ── Full-program tests (translating DagSemTools TestParser) ───────────────

    #[test]
    fn single_rule_bracket_syntax() {
        let src = "prefix ex: <https://example.com/data#>\n\
            [?s, <https://example.com/data#predicate>, ex:obj] :-\n\
            [?s, <https://example.com/data#predicate2>, ex:obj].";
        let rules = parse(src, &mut ds()).unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].body.len(), 1);
    }

    #[test]
    fn rule_with_two_body_atoms() {
        let src = "prefix ex: <https://example.com/data#>\n\
            ex:prop[?s, ex:obj] :- ex:prop2[?s, ex:obj],\n    ex:prop3[?s, ex:obj].";
        let rules = parse(src, &mut ds()).unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].body.len(), 2);
    }

    #[test]
    fn two_rules_in_one_file() {
        let src = "prefix ex: <https://example.com/data#>\n\
            ex:prop[?s, ex:obj] :- ex:prop2[?s, ex:obj], ex:prop3[?s, ex:obj].\n\
            ex:prop3[?s, ex:obj] :- ex:prop4[?s, ex:obj], ex:type3[?s].";
        let rules = parse(src, &mut ds()).unwrap();
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].body.len(), 2);
        assert_eq!(rules[1].body.len(), 2);
    }

    #[test]
    fn negation_in_body() {
        let src = "prefix ex: <https://example.com/data#>\n\
            ex:prop[?s, ex:obj] :- ex:prop2[?s, ex:obj], NOT ex:prop3[?s, ex:obj].";
        let rules = parse(src, &mut ds()).unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].body.len(), 2);
        assert!(matches!(rules[0].body[1], RuleAtom::NotPattern(_)));
    }

    #[test]
    fn negation_lowercase_not() {
        let src = "prefix ex: <https://example.com/data#>\n\
            ex:prop[?s, ex:obj] :- ex:prop2[?s, ex:obj], not ex:prop3[?s, ex:obj].";
        let rules = parse(src, &mut ds()).unwrap();
        assert!(matches!(rules[0].body[1], RuleAtom::NotPattern(_)));
    }

    #[test]
    fn type_atom_in_body() {
        let src = "prefix ex: <https://example.com/data#>\n\
            ex:type[?s] :- ex:prop[?s, ex:obj], ex:type2[?s].";
        let rules = parse(src, &mut ds()).unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].body.len(), 2);
    }

    #[test]
    fn multiple_prefix_styles() {
        let src = r#"prefix ex: <https://example.com/data#>
@prefix ex2: <https://example.com/data2#> .
@prefix ex3: <https://example.com/data3#> .
[?s, <https://example.com/data#predicate>, ex2:obj] :-
    [?s, <https://example.com/data#predicate2>, ex3:obj]."#;
        let rules = parse(src, &mut ds()).unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].body.len(), 1);
    }

    #[test]
    fn builtin_rdfs_prefix() {
        // rdfs:range should expand without explicit declaration
        let src = "[?x, a, ?c] :- [?x, ?p, ?y], [?p, rdfs:range, ?c] .";
        let mut ds = ds();
        let rules = parse(src, &mut ds).unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].body.len(), 2);
        // Head predicate should be rdf:type
        if let RuleHead::NormalHead(ref pat) = rules[0].head {
            let rdf_type_id = ds.add_node_resource(RdfResource::Iri(IriReference(RDF_TYPE.to_string())));
            assert_eq!(pat.predicate, Term::Resource(rdf_type_id));
        } else {
            panic!("expected NormalHead");
        }
    }

    #[test]
    fn contradiction_head() {
        let src = "false :- [?X, a, <https://example.com/AnotherInvalidClass>] .";
        let rules = parse(src, &mut ds()).unwrap();
        assert_eq!(rules.len(), 1);
        assert!(matches!(rules[0].head, RuleHead::Contradiction));
        assert_eq!(rules[0].body.len(), 1);
    }

    #[test]
    fn named_graph_variable() {
        let src = "prefix ex: <https://example.com/data#>\n\
            [?s, ex:predicate, ex:object2] ?graph :- ex:predicate[?s, ex:object] ?graph .";
        let mut ds = ds();
        let rules = parse(src, &mut ds).unwrap();
        assert_eq!(rules.len(), 1);
        if let RuleHead::NormalHead(ref pat) = rules[0].head {
            assert_eq!(pat.graph, Term::Variable("graph".to_string()));
        }
        if let RuleAtom::PositivePattern(ref pat) = rules[0].body[0] {
            assert_eq!(pat.graph, Term::Variable("graph".to_string()));
        }
    }

    #[test]
    fn type_atom_with_space_before_bracket() {
        // typeatom2.datalog: ex:type [?new_node] :- ex:type [?node] .
        let src = "prefix ex: <https://example.com/data#>\nex:type [?new_node] :- ex:type [?node] .";
        let rules = parse(src, &mut ds()).unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].body.len(), 1);
    }

    #[test]
    fn fact_empty_body() {
        let src = "prefix ex: <https://example.com/data#>\n[ex:Alice, a, ex:Person] .";
        let rules = parse(src, &mut ds()).unwrap();
        assert_eq!(rules.len(), 1);
        assert!(rules[0].body.is_empty());
    }

    #[test]
    fn comment_handling() {
        let src = "# comment\nprefix ex: <https://example.com/data#>\n\
            # another\n[?s, ex:pred, ex:obj] :- [?s, ex:pred2, ex:obj] .";
        let rules = parse(src, &mut ds()).unwrap();
        assert_eq!(rules.len(), 1);
    }

    #[test]
    fn invalid_input_is_error() {
        assert!(parse("this is not datalog !!!", &mut ds()).is_err());
    }

    #[test]
    fn large_file() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/testdata/large.datalog");
        if !path.exists() { return; }
        let rules = parse_file(&path, &mut ds()).unwrap();
        assert!(rules.len() > 100, "expected >100 rules, got {}", rules.len());
    }
}

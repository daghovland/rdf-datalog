/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! SPARQL 1.2 query parser (nom-based).
//!
//! Supports:
//! - SELECT (with DISTINCT, * projection, variable projection, LIMIT/OFFSET/ORDER BY)
//! - Basic Graph Patterns (triple patterns)
//! - FILTER (comparison operators, regex(), BOUND(), NOT EXISTS, EXISTS)
//! - OPTIONAL
//! - UNION
//! - MINUS
//! - PREFIX declarations
//! - Literals: string, language-tagged, typed, numeric (integer, decimal), boolean
//! - Blank node subjects/objects

pub mod ast;
pub mod execute;
pub use execute::{
    eval_expr_as_filter, execute, QueryResult, ResolvedTriple, SelectResult, SolutionRow,
};

use crate::ast::*;
use dag_rdf::{GraphElement, IriReference, RdfLiteral, RdfResource};
use ingress::{XSD_BOOLEAN, XSD_DECIMAL, XSD_INTEGER};
use nom::{
    branch::alt,
    bytes::complete::{tag, tag_no_case, take_until, take_while, take_while1},
    character::complete::{char, multispace0, multispace1},
    combinator::{map, opt},
    multi::{many0, separated_list0, separated_list1},
    sequence::{delimited, pair, preceded, terminated, tuple},
    IResult,
};
use std::collections::HashMap;

pub struct ParserContext {
    pub prefixes: HashMap<String, String>,
}

// ── Top-level entry point ────────────────────────────────────────────────────

pub fn parse_query<'a>(input: &'a str, ctx: &'a mut ParserContext) -> IResult<&'a str, Query> {
    let (input, _) = multispace0(input)?;
    // PREFIX declarations — may mutate ctx with new prefix bindings
    let (mut input, _) = many0(parse_prefix(ctx))(input)?;
    input = multispace0(input)?.0;
    parse_query_body(ctx)(input)
}

/// Parse a query body (SELECT / ASK / CONSTRUCT) without PREFIX declarations.
///
/// Used for both top-level queries (after prefix parsing) and subqueries inside
/// group graph patterns (which inherit the outer prefix context).
fn parse_query_body<'a>(
    ctx: &'a ParserContext,
) -> impl Fn(&'a str) -> IResult<&'a str, Query> + 'a {
    move |input| {
        let (input, _) = multispace0(input)?;

        // ASK query: ASK [WHERE] GroupGraphPattern
        if let Ok((rest, _)) = tag_no_case::<_, _, nom::error::Error<&str>>("ASK")(input) {
            let boundary = rest
                .chars()
                .next()
                .map(|c| c.is_whitespace() || c == '{')
                .unwrap_or(true);
            if boundary {
                let (rest, _) = multispace0(rest)?;
                let (rest, _) = opt(terminated(tag_no_case("WHERE"), multispace0))(rest)?;
                let (rest, where_clause) = parse_group_graph_pattern(ctx)(rest)?;
                let (rest, _) = multispace0(rest)?;
                return Ok((rest, Query::Ask { where_clause }));
            }
        }

        // CONSTRUCT query: CONSTRUCT [{ template }] [WHERE] { pattern }
        if let Ok((rest, _)) = tag_no_case::<_, _, nom::error::Error<&str>>("CONSTRUCT")(input) {
            let boundary = rest
                .chars()
                .next()
                .map(|c| c.is_whitespace() || c == '{')
                .unwrap_or(true);
            if boundary {
                let (rest, _) = multispace0(rest)?;

                // Parse optional explicit template block (full form) or leave empty (short form).
                let (rest, template) = if rest.starts_with('{') {
                    let (rest, components) = parse_group_graph_pattern(ctx)(rest)?;
                    let tps: Vec<TriplePattern> = components
                        .into_iter()
                        .flat_map(|c| {
                            if let QueryComponent::BGP(tps) = c {
                                tps
                            } else {
                                vec![]
                            }
                        })
                        .collect();
                    (rest, tps)
                } else {
                    (rest, vec![])
                };

                let (rest, _) = multispace0(rest)?;
                let (rest, _) = opt(terminated(tag_no_case("WHERE"), multispace0))(rest)?;
                let (rest, where_clause) = parse_group_graph_pattern(ctx)(rest)?;
                let (rest, _) = multispace0(rest)?;
                return Ok((
                    rest,
                    Query::Construct {
                        template,
                        where_clause,
                    },
                ));
            }
        }

        // SELECT query
        let (input, _) = tag_no_case("SELECT")(input)?;
        let (input, _) = multispace1(input)?;

        // DISTINCT keyword
        let (input, distinct_opt) = opt(terminated(tag_no_case("DISTINCT"), multispace1))(input)?;
        let distinct = distinct_opt.is_some();

        // Projection: * or list of ?var
        let (input, projection) = parse_projection(ctx)(input)?;
        let (input, _) = multispace0(input)?;

        // SPARQL syntax error: duplicate projection alias (e.g. SELECT (1 AS ?x) (1 AS ?x))
        {
            let mut seen_aliases: Vec<&str> = Vec::new();
            for elem in &projection {
                if let ProjectionElement::Expression(_, alias) = elem {
                    if seen_aliases.contains(&alias.as_str()) {
                        return Err(nom::Err::Failure(nom::error::Error::new(
                            input,
                            nom::error::ErrorKind::Verify,
                        )));
                    }
                    seen_aliases.push(alias);
                }
            }
        }

        // WHERE (optional keyword)
        let (input, _) = opt(terminated(tag_no_case("WHERE"), multispace0))(input)?;

        // Group graph pattern
        let (input, _) = multispace0(input)?;
        let (input, where_clause) = parse_group_graph_pattern(ctx)(input)?;
        let (input, _) = multispace0(input)?;

        // Optional modifiers: GROUP BY, HAVING, ORDER BY, LIMIT, OFFSET
        let (input, group_by) = parse_group_by(ctx)(input)?;
        let (input, _) = multispace0(input)?;
        let (input, having) = parse_having(ctx)(input)?;
        let (input, _) = multispace0(input)?;
        let (input, order_by) = parse_order_by(ctx)(input)?;
        let (input, _) = multispace0(input)?;
        let (input, limit) = parse_limit_offset("LIMIT")(input)?;
        let (input, _) = multispace0(input)?;
        let (input, offset) = parse_limit_offset("OFFSET")(input)?;
        let (input, _) = multispace0(input)?;

        Ok((
            input,
            Query::Select {
                projection,
                where_clause,
                group_by,
                having,
                order_by,
                limit,
                offset,
                distinct,
            },
        ))
    }
}

// ── Prefix ───────────────────────────────────────────────────────────────────

fn parse_prefix<'a>(ctx: &mut ParserContext) -> impl FnMut(&'a str) -> IResult<&'a str, ()> + '_ {
    move |input| {
        let (input, _) = multispace0(input)?;
        let (input, _) = tag_no_case("PREFIX")(input)?;
        let (input, _) = multispace1(input)?;
        // prefix name: optional alphanumeric + colon (e.g. "foaf:", ":")
        let (input, prefix_name) = take_while(|c: char| c.is_alphanumeric() || c == '_')(input)?;
        let (input, _) = char(':')(input)?;
        let (input, _) = multispace0(input)?;
        let (input, iri) = parse_iri_ref(input)?;
        let (input, _) = opt(char('.'))(input)?;
        ctx.prefixes.insert(prefix_name.to_string(), iri.0);
        Ok((input, ()))
    }
}

// ── Projection ───────────────────────────────────────────────────────────────

fn parse_projection<'a>(
    ctx: &'a ParserContext,
) -> impl Fn(&'a str) -> IResult<&'a str, Vec<ProjectionElement>> + 'a {
    move |input| {
        alt((
            // *
            map(terminated(char('*'), multispace0), |_| {
                vec![ProjectionElement::Star]
            }),
            // one or more ?var / (?expr AS ?alias)
            map(
                many0(terminated(
                    move |i| parse_projection_element(ctx)(i),
                    multispace0,
                )),
                |elems| elems,
            ),
        ))(input)
    }
}

fn parse_projection_element<'a>(
    ctx: &'a ParserContext,
) -> impl Fn(&'a str) -> IResult<&'a str, ProjectionElement> + 'a {
    move |input| {
        alt((
            // (?expr AS ?alias)
            map(
                delimited(
                    pair(char('('), multispace0),
                    pair(
                        |i| parse_expression(ctx)(i),
                        preceded(
                            // multispace0 because the expression parser eagerly consumes
                            // trailing whitespace; multispace1 would always fail here.
                            tuple((multispace0, tag_no_case("AS"), multispace1, char('?'))),
                            parse_varname,
                        ),
                    ),
                    pair(multispace0, char(')')),
                ),
                |(expr, alias)| ProjectionElement::Expression(expr, alias),
            ),
            // ?var
            map(
                preceded(char('?'), parse_varname),
                ProjectionElement::Variable,
            ),
        ))(input)
    }
}

// ── Group Graph Pattern ───────────────────────────────────────────────────────

fn parse_group_graph_pattern<'a>(
    ctx: &'a ParserContext,
) -> impl Fn(&'a str) -> IResult<&'a str, Vec<QueryComponent>> + 'a {
    move |input| {
        let (input, _) = multispace0(input)?;
        let (input, _) = char('{')(input)?;
        let (input, _) = multispace0(input)?;
        let (input, components) = parse_group_graph_pattern_contents(ctx)(input)?;
        let (input, _) = multispace0(input)?;
        let (input, _) = char('}')(input)?;
        Ok((input, components))
    }
}

fn parse_group_graph_pattern_contents<'a>(
    ctx: &'a ParserContext,
) -> impl Fn(&'a str) -> IResult<&'a str, Vec<QueryComponent>> + 'a {
    move |input| {
        let mut components: Vec<QueryComponent> = Vec::new();
        let mut current_triples: Vec<TriplePattern> = Vec::new();
        let mut remaining = input;
        let mut blank_node_counter: usize = 0;

        loop {
            remaining = multispace0(remaining)?.0;

            // Check for close brace — end of group
            if remaining.starts_with('}') {
                break;
            }

            // OPTIONAL
            if remaining.to_ascii_uppercase().starts_with("OPTIONAL") {
                flush_triples(&mut components, &mut current_triples);
                let (r, inner) = preceded(
                    pair(tag_no_case("OPTIONAL"), multispace0),
                    parse_group_graph_pattern(ctx),
                )(remaining)?;
                components.push(QueryComponent::Optional(inner));
                remaining = r;
                continue;
            }

            // FILTER
            if remaining.to_ascii_uppercase().starts_with("FILTER") {
                flush_triples(&mut components, &mut current_triples);
                let (r, expr) = parse_filter(ctx)(remaining)?;
                components.push(QueryComponent::Filter(expr));
                remaining = r;
                continue;
            }

            // GRAPH
            if remaining.to_ascii_uppercase().starts_with("GRAPH") {
                flush_triples(&mut components, &mut current_triples);
                let (r, _) = tag_no_case("GRAPH")(remaining)?;
                let (r, _) = multispace1(r)?;
                let (r, graph_term) = parse_term(ctx)(r)?;
                let (r, _) = multispace0(r)?;
                let (r, inner) = parse_group_graph_pattern(ctx)(r)?;
                components.push(QueryComponent::Graph(graph_term, inner));
                remaining = r;
                continue;
            }

            // MINUS
            if remaining.to_ascii_uppercase().starts_with("MINUS") {
                flush_triples(&mut components, &mut current_triples);
                let (r, inner) = preceded(
                    pair(tag_no_case("MINUS"), multispace0),
                    parse_group_graph_pattern(ctx),
                )(remaining)?;
                components.push(QueryComponent::Minus(inner));
                remaining = r;
                continue;
            }

            // BIND
            if remaining.to_ascii_uppercase().starts_with("BIND") {
                flush_triples(&mut components, &mut current_triples);
                let (r, (expr, var)) = parse_bind(ctx)(remaining)?;
                components.push(QueryComponent::Bind(expr, var));
                remaining = r;
                continue;
            }

            // VALUES
            if remaining.to_ascii_uppercase().starts_with("VALUES") {
                flush_triples(&mut components, &mut current_triples);
                let (r, vals) = parse_values(ctx)(remaining)?;
                components.push(vals);
                remaining = r;
                continue;
            }

            // SubSelect: SELECT appears directly in group pattern position (no extra braces).
            // Grammar: GroupGraphPattern ::= '{' ( SubSelect | GroupGraphPatternSub ) '}'
            {
                let upper = remaining.to_ascii_uppercase();
                let is_direct_select = upper.starts_with("SELECT")
                    && upper[6..]
                        .chars()
                        .next()
                        .map(|c| !c.is_alphanumeric() && c != '_')
                        .unwrap_or(true);
                if is_direct_select {
                    flush_triples(&mut components, &mut current_triples);
                    let (r, inner_query) = parse_query_body(ctx)(remaining)?;
                    components.push(QueryComponent::Subquery(Box::new(inner_query)));
                    remaining = r;
                    continue;
                }
            }

            // Sub-group { ... } — could contain a subquery, be a UNION arm, or an inline group.
            // Note: `parse_group_graph_pattern` already handles { SELECT ... } internally
            // (via the direct-SELECT detection in `parse_group_graph_pattern_contents`), so
            // we do NOT special-case it here. Routing everything through `parse_group_graph_pattern`
            // is what makes UNION-of-subqueries work: after parsing the left `{ SELECT ... }`
            // we can still detect UNION.
            if remaining.starts_with('{') {
                flush_triples(&mut components, &mut current_triples);

                let (r, left) = parse_group_graph_pattern(ctx)(remaining)?;
                let r = multispace0(r)?.0;
                // Check for UNION
                if r.to_ascii_uppercase().starts_with("UNION") {
                    let (r, _) = tag_no_case("UNION")(r)?;
                    let (r, _) = multispace0(r)?;
                    let (r, right) = parse_group_graph_pattern(ctx)(r)?;
                    components.push(QueryComponent::Union(left, right));
                    remaining = r;
                } else {
                    // Inline sub-group: flatten into current components
                    components.extend(left);
                    remaining = r;
                }
                continue;
            }

            // Blank node property list in subject position: [ pred obj ; pred obj ... ]
            // Equivalent to a fresh anonymous blank node subject with the given pred-obj pairs.
            if remaining.starts_with('[') && !remaining.starts_with("[]") {
                flush_triples(&mut components, &mut current_triples);
                let (r, _) = char('[')(remaining)?;
                let (r, _) = multispace0(r)?;
                // Generate a fresh internal variable for the blank node
                let bn_var = Term::Variable(format!("__bn_{}", blank_node_counter));
                blank_node_counter += 1;
                // Parse predicate-object pairs inside [ ... ] with the blank node as subject
                let (r, inner_comps) = parse_predobj_pairs(ctx, &bn_var, r)?;
                for comp in inner_comps {
                    match comp {
                        QueryComponent::BGP(tps) => current_triples.extend(tps),
                        other => {
                            flush_triples(&mut components, &mut current_triples);
                            components.push(other);
                        }
                    }
                }
                let (r, _) = multispace0(r)?;
                let (r, _) = char(']')(r)?;
                remaining = r;
                // Consume optional dot
                remaining = multispace0(remaining)?.0;
                if remaining.starts_with('.') && !remaining.starts_with("..") {
                    remaining = &remaining[1..];
                }
                continue;
            }

            // Triple / path pattern statement
            match parse_triple_pattern_statement(ctx, remaining) {
                Ok((r, comps)) => {
                    for comp in comps {
                        match comp {
                            QueryComponent::BGP(tps) => current_triples.extend(tps),
                            other => {
                                flush_triples(&mut components, &mut current_triples);
                                components.push(other);
                            }
                        }
                    }
                    remaining = r;
                    // Consume optional dot
                    remaining = multispace0(remaining)?.0;
                    if remaining.starts_with('.') && !remaining.starts_with("..") {
                        remaining = &remaining[1..];
                    }
                }
                Err(_) => break,
            }
        }

        flush_triples(&mut components, &mut current_triples);
        Ok((remaining, components))
    }
}

fn flush_triples(components: &mut Vec<QueryComponent>, triples: &mut Vec<TriplePattern>) {
    if !triples.is_empty() {
        components.push(QueryComponent::BGP(std::mem::take(triples)));
    }
}

// ── Triple / path pattern ─────────────────────────────────────────────────────

/// Parse predicate-object pairs with a fixed `subject`, returning components.
///
/// Used for blank-node property lists `[ pred obj ; pred obj ]` where the blank
/// node is already known.  Stops at `]` without consuming it.
fn parse_predobj_pairs<'a>(
    ctx: &'a ParserContext,
    subject: &Term,
    input: &'a str,
) -> IResult<&'a str, Vec<QueryComponent>> {
    let mut comps: Vec<QueryComponent> = Vec::new();
    let mut remaining = input;

    loop {
        remaining = multispace0(remaining)?.0;
        if remaining.starts_with(']') || remaining.is_empty() {
            break;
        }
        let (r, _) = multispace0(remaining)?;
        let (r, path) = match parse_path_alternative(ctx)(r) {
            Ok(x) => x,
            Err(_) => break,
        };
        let (r, _) = multispace1(r)?;
        let (mut r, first_obj) = parse_term(ctx)(r)?;

        let mut objects = vec![first_obj];
        loop {
            let (rws, _) = multispace0(r)?;
            if !rws.starts_with(',') {
                r = rws;
                break;
            }
            let (rc, _) = char(',')(rws)?;
            let (rc, _) = multispace0(rc)?;
            let (rn, obj) = parse_term(ctx)(rc)?;
            objects.push(obj);
            r = rn;
        }

        for object in objects {
            match &path {
                PropertyPath::Iri(gel) => {
                    comps.push(QueryComponent::BGP(vec![TriplePattern {
                        subject: subject.clone(),
                        predicate: Term::Constant(gel.clone()),
                        object: object.clone(),
                    }]));
                }
                _ => {
                    comps.push(QueryComponent::PathPattern(
                        subject.clone(),
                        Box::new(path.clone()),
                        object.clone(),
                    ));
                }
            }
        }

        let (rws, _) = multispace0(r)?;
        if !rws.starts_with(';') {
            remaining = rws;
            break;
        }
        let (rws, _) = char(';')(rws)?;
        let (rws, _) = multispace0(rws)?;
        if rws.starts_with(']') {
            remaining = rws;
            break;
        }
        remaining = rws;
    }
    Ok((remaining, comps))
}

/// Parse one triple or path statement (subject + one or more predicate-object pairs
/// separated by `;`, with `,` for multiple objects per predicate).
///
/// Returns a list of `QueryComponent`s: `BGP([tp])` for plain triple patterns and
/// `PathPattern(s, path, o)` for complex property paths.
fn parse_triple_pattern_statement<'a>(
    ctx: &'a ParserContext,
    input: &'a str,
) -> IResult<&'a str, Vec<QueryComponent>> {
    let (input, _) = multispace0(input)?;
    let (mut remaining, subject) = parse_term(ctx)(input)?;
    let mut components: Vec<QueryComponent> = Vec::new();
    let mut first_predicate = true;

    loop {
        let (r, _) = if first_predicate {
            multispace1(remaining)?
        } else {
            multispace0(remaining)?
        };

        // If the predicate is a variable (e.g. ?p), parse it directly as a Term.
        // Variables are not valid property path expressions but are valid BGP predicates.
        let predicate_is_var = r.starts_with('?') || r.starts_with('$');
        let (r, pred_var_opt) = if predicate_is_var {
            let (r, t) = parse_term(ctx)(r)?;
            (r, Some(t))
        } else {
            (r, None)
        };

        let (r, path_opt) = if pred_var_opt.is_none() {
            let (r, p) = parse_path_alternative(ctx)(r)?;
            (r, Some(p))
        } else {
            (r, None)
        };

        let (r, _) = multispace1(r)?;
        let (mut r, first_object) = parse_term(ctx)(r)?;

        let mut objects = vec![first_object];
        loop {
            let (r_ws, _) = multispace0(r)?;
            if !r_ws.starts_with(',') {
                r = r_ws;
                break;
            }
            let (r_after_comma, _) = char(',')(r_ws)?;
            let (r_after_comma, _) = multispace0(r_after_comma)?;
            let (r_next, obj) = parse_term(ctx)(r_after_comma)?;
            objects.push(obj);
            r = r_next;
        }

        for object in objects {
            if let Some(ref pred_var) = pred_var_opt {
                components.push(QueryComponent::BGP(vec![TriplePattern {
                    subject: subject.clone(),
                    predicate: pred_var.clone(),
                    object: object.clone(),
                }]));
            } else if let Some(ref path) = path_opt {
                match path {
                    PropertyPath::Iri(gel) => {
                        components.push(QueryComponent::BGP(vec![TriplePattern {
                            subject: subject.clone(),
                            predicate: Term::Constant(gel.clone()),
                            object: object.clone(),
                        }]));
                    }
                    _ => {
                        components.push(QueryComponent::PathPattern(
                            subject.clone(),
                            Box::new(path.clone()),
                            object.clone(),
                        ));
                    }
                }
            }
        }

        let (r_ws, _) = multispace0(r)?;
        if !r_ws.starts_with(';') {
            remaining = r_ws;
            break;
        }

        let (r_after_semi, _) = char(';')(r_ws)?;
        let (r_after_semi, _) = multispace0(r_after_semi)?;

        // Allow trailing semicolon before '.' or '}'.
        if r_after_semi.starts_with('.') || r_after_semi.starts_with('}') {
            remaining = r_after_semi;
            break;
        }

        remaining = r_after_semi;
        first_predicate = false;
    }

    Ok((remaining, components))
}

// ── Property path grammar ─────────────────────────────────────────────────────
//
// PathAlternative := PathSequence ( '|' PathSequence )*
// PathSequence    := PathEltOrInverse ( '/' PathEltOrInverse )*
// PathEltOrInverse:= '^' PathElt | PathElt
// PathElt         := PathPrimary PathMod?
// PathMod         := '*' | '+' | '?'
// PathPrimary     := IRI | 'a' | '!' NegSet | '(' PathAlternative ')'

fn parse_path_alternative<'a>(
    ctx: &'a ParserContext,
) -> impl Fn(&'a str) -> IResult<&'a str, PropertyPath> + 'a {
    move |input| {
        let (input, left) = parse_path_sequence(ctx)(input)?;
        // No trailing multispace0 here — the caller (parse_triple_pattern_statement)
        // uses multispace0 before checking for '|', so we don't want to eat the
        // space that belongs between the path and the object term.
        let (input, rest) = many0(preceded(
            tuple((multispace0, char('|'), multispace0)),
            parse_path_sequence(ctx),
        ))(input)?;
        Ok((
            input,
            rest.into_iter().fold(left, |acc, r| {
                PropertyPath::Alternative(Box::new(acc), Box::new(r))
            }),
        ))
    }
}

fn parse_path_sequence<'a>(
    ctx: &'a ParserContext,
) -> impl Fn(&'a str) -> IResult<&'a str, PropertyPath> + 'a {
    move |input| {
        let (input, first) = parse_path_elt_or_inverse(ctx)(input)?;
        let (input, rest) = many0(preceded(
            tuple((multispace0, char('/'), multispace0)),
            |i| parse_path_elt_or_inverse(ctx)(i),
        ))(input)?;
        if rest.is_empty() {
            Ok((input, first))
        } else {
            let mut all = vec![first];
            all.extend(rest);
            Ok((input, PropertyPath::Sequence(all)))
        }
    }
}

fn parse_path_elt_or_inverse<'a>(
    ctx: &'a ParserContext,
) -> impl Fn(&'a str) -> IResult<&'a str, PropertyPath> + 'a {
    move |input| {
        alt((
            map(
                preceded(pair(char('^'), multispace0), |i| parse_path_elt(ctx)(i)),
                |p| PropertyPath::Inverse(Box::new(p)),
            ),
            |i| parse_path_elt(ctx)(i),
        ))(input)
    }
}

fn parse_path_elt<'a>(
    ctx: &'a ParserContext,
) -> impl Fn(&'a str) -> IResult<&'a str, PropertyPath> + 'a {
    move |input| {
        let (input, primary) = parse_path_primary(ctx)(input)?;
        let (input, mod_char) = opt(alt((
            map(char('*'), |_| '*'),
            map(char('+'), |_| '+'),
            map(char('?'), |_| '?'),
        )))(input)?;
        Ok((
            input,
            match mod_char {
                Some('*') => PropertyPath::ZeroOrMore(Box::new(primary)),
                Some('+') => PropertyPath::OneOrMore(Box::new(primary)),
                Some('?') => PropertyPath::ZeroOrOne(Box::new(primary)),
                _ => primary,
            },
        ))
    }
}

fn parse_path_primary<'a>(
    ctx: &'a ParserContext,
) -> impl Fn(&'a str) -> IResult<&'a str, PropertyPath> + 'a {
    move |input| {
        alt((
            // '(' PathAlternative ')'
            delimited(
                pair(char('('), multispace0),
                |i| parse_path_alternative(ctx)(i),
                pair(multispace0, char(')')),
            ),
            // '!' NegatedSet
            map(
                preceded(pair(char('!'), multispace0), |i| {
                    parse_path_negated_set(ctx)(i)
                }),
                PropertyPath::NegatedSet,
            ),
            // IRI / 'a'
            map(|i| parse_path_iri(ctx)(i), PropertyPath::Iri),
        ))(input)
    }
}

/// Parse an IRI in path position: full `<IRI>`, prefixed name, or `a` shorthand.
fn parse_path_iri<'a>(
    ctx: &'a ParserContext,
) -> impl Fn(&'a str) -> IResult<&'a str, GraphElement> + 'a {
    move |input| {
        alt((
            // 'a' shorthand for rdf:type (same boundary check as in parse_term)
            |input: &'a str| {
                if let Some(rest) = input.strip_prefix('a') {
                    let next = rest.chars().next();
                    if next
                        .map(|c| !c.is_alphanumeric() && c != '_' && c != ':')
                        .unwrap_or(true)
                    {
                        let iri = IriReference(
                            "http://www.w3.org/1999/02/22-rdf-syntax-ns#type".to_string(),
                        );
                        return Ok((rest, GraphElement::NodeOrEdge(RdfResource::Iri(iri))));
                    }
                }
                Err(nom::Err::Error(nom::error::Error::new(
                    input,
                    nom::error::ErrorKind::Tag,
                )))
            },
            map(parse_iri_ref, |iri| {
                GraphElement::NodeOrEdge(RdfResource::Iri(iri))
            }),
            map(parse_prefixed_name(ctx), |iri| {
                GraphElement::NodeOrEdge(RdfResource::Iri(iri))
            }),
        ))(input)
    }
}

/// Parse the body of a negated property set: `(IRI ('|' IRI)*)` or a single IRI.
fn parse_path_negated_set<'a>(
    ctx: &'a ParserContext,
) -> impl Fn(&'a str) -> IResult<&'a str, Vec<GraphElement>> + 'a {
    move |input| {
        alt((
            delimited(
                pair(char('('), multispace0),
                separated_list0(tuple((multispace0, char('|'), multispace0)), |i| {
                    parse_path_iri(ctx)(i)
                }),
                pair(multispace0, char(')')),
            ),
            map(|i| parse_path_iri(ctx)(i), |gel| vec![gel]),
        ))(input)
    }
}

// ── Term / literal parsing ───────────────────────────────────────────────────

fn parse_term<'a>(ctx: &'a ParserContext) -> impl Fn(&'a str) -> IResult<&'a str, Term> + 'a {
    move |input| {
        alt((
            // Variable
            map(preceded(char('?'), parse_varname), Term::Variable),
            // IRI in angle brackets
            map(parse_iri_ref, |iri| {
                Term::Constant(GraphElement::NodeOrEdge(RdfResource::Iri(iri)))
            }),
            // 'a' shorthand for rdf:type (must come before prefixed-name parser)
            |input: &'a str| {
                if let Some(rest) = input.strip_prefix('a') {
                    let next = rest.chars().next();
                    if next
                        .map(|c| !c.is_alphanumeric() && c != '_' && c != ':')
                        .unwrap_or(true)
                    {
                        let iri = IriReference(
                            "http://www.w3.org/1999/02/22-rdf-syntax-ns#type".to_string(),
                        );
                        return Ok((
                            rest,
                            Term::Constant(GraphElement::NodeOrEdge(RdfResource::Iri(iri))),
                        ));
                    }
                }
                Err(nom::Err::Error(nom::error::Error::new(
                    input,
                    nom::error::ErrorKind::Char,
                )))
            },
            // Prefixed name (prefix:local) — must come before bare terms
            map(parse_prefixed_name(ctx), |iri| {
                Term::Constant(GraphElement::NodeOrEdge(RdfResource::Iri(iri)))
            }),
            // String literal with optional lang tag or datatype
            map(parse_string_literal(ctx), |lit| {
                Term::Constant(GraphElement::GraphLiteral(lit))
            }),
            // Numeric literal
            map(parse_numeric_literal, |lit| {
                Term::Constant(GraphElement::GraphLiteral(lit))
            }),
            // Boolean literal
            map(parse_boolean_literal, |lit| {
                Term::Constant(GraphElement::GraphLiteral(lit))
            }),
            // Blank node _:label
            map(parse_blank_node, |id| {
                Term::Constant(GraphElement::NodeOrEdge(RdfResource::AnonymousBlankNode(
                    id,
                )))
            }),
        ))(input)
    }
}

fn parse_varname(input: &str) -> IResult<&str, String> {
    map(
        take_while1(|c: char| c.is_alphanumeric() || c == '_'),
        |s: &str| s.to_string(),
    )(input)
}

fn parse_iri_ref(input: &str) -> IResult<&str, IriReference> {
    map(
        delimited(char('<'), take_while(|c: char| c != '>'), char('>')),
        |iri: &str| IriReference(iri.to_string()),
    )(input)
}

/// Parse the local part of a SPARQL 1.1 prefixed name (PN_LOCAL).
///
/// Handles:
/// - Alphanumeric, `_`, `-`, `.`, `:` as regular characters
/// - `\CHAR` backslash-escaped characters (→ literal `CHAR` in the IRI)
/// - `%HH` percent-encoded sequences (kept verbatim in the IRI)
/// - Trailing unescaped `.` is not part of the local name (triple terminator)
fn parse_pn_local_str(input: &str) -> (String, &str) {
    let mut local = String::new();
    let mut remaining = input;
    // Track whether the last appended char came from an escape/percent-encoding.
    // Only unescaped trailing '.' needs to be trimmed.
    let mut last_was_escape = false;

    loop {
        // Percent-encoded: %HH
        if remaining.starts_with('%') && remaining.len() >= 3 {
            let b1 = remaining.as_bytes().get(1).copied().unwrap_or(0);
            let b2 = remaining.as_bytes().get(2).copied().unwrap_or(0);
            if b1.is_ascii_hexdigit() && b2.is_ascii_hexdigit() {
                local.push_str(&remaining[..3]);
                remaining = &remaining[3..];
                last_was_escape = true;
                continue;
            }
        }
        // PN_LOCAL_ESC: \CHAR
        if remaining.starts_with('\\') && remaining.len() >= 2 {
            let ch = remaining[1..].chars().next().unwrap();
            if matches!(
                ch,
                '_' | '~'
                    | '.'
                    | '-'
                    | '!'
                    | '$'
                    | '&'
                    | '\''
                    | '('
                    | ')'
                    | '*'
                    | '+'
                    | ','
                    | ';'
                    | '='
                    | '/'
                    | '?'
                    | '#'
                    | '@'
                    | '%' // Note: ':' is NOT in PN_LOCAL_ESC — bare ':' is valid in local names
                          // but '\:' is a syntax error per SPARQL 1.1 grammar.
            ) {
                local.push(ch);
                remaining = &remaining[1 + ch.len_utf8()..];
                last_was_escape = true;
                continue;
            }
        }
        // Regular PN_LOCAL character
        let Some(ch) = remaining.chars().next() else {
            break;
        };
        if ch.is_alphanumeric() || matches!(ch, '_' | '-' | ':') {
            // Non-dot regular char: reset trailing-dot tracking

            local.push(ch);
            remaining = &remaining[ch.len_utf8()..];
            last_was_escape = false;
        } else if ch == '.' {
            // Unescaped dot: may or may not be trailing — defer judgment
            local.push('.');
            remaining = &remaining[1..];
            last_was_escape = false;
        } else {
            break;
        }
    }

    // Trim any run of trailing unescaped dots. These are triple terminators.
    // We do NOT trim dots that came from escape sequences (\.)
    // Strategy: walk back from end of `local`, counting unescaped trailing dots,
    // then restore `remaining` to just before those dots in `input`.
    if !last_was_escape {
        while local.ends_with('.') {
            // The trailing '.' is an unescaped literal dot — remove it.
            local.pop();
            // Restore remaining by one byte (unescaped '.' is 1 byte ASCII)
            let consumed = input.len() - remaining.len();
            let restore_to = consumed - 1;
            remaining = &input[restore_to..];
        }
    }

    (local, remaining)
}

fn parse_prefixed_name<'a>(
    ctx: &'a ParserContext,
) -> impl Fn(&'a str) -> IResult<&'a str, IriReference> + 'a {
    move |input| {
        // Match prefix_name : local_name (SPARQL 1.1 PN_PREFIX : PN_LOCAL)
        // Prefix can be empty (just ":")
        let (after_prefix, prefix) = take_while(|c: char| c.is_alphanumeric() || c == '_')(input)?;
        let (after_colon, _) = char(':')(after_prefix)?;
        // Local name: full SPARQL 1.1 PN_LOCAL (colons, escapes, percent-encoding)
        let (local, after_local) = parse_pn_local_str(after_colon);

        // Must not be an empty local + empty prefix (that would match nothing)
        if prefix.is_empty() && local.is_empty() {
            return Err(nom::Err::Error(nom::error::Error::new(
                input,
                nom::error::ErrorKind::TakeWhile1,
            )));
        }

        // Reject keyword-like prefixes (FILTER, OPTIONAL, etc.)
        let lower = prefix.to_ascii_lowercase();
        if matches!(
            lower.as_str(),
            // "_" is the reserved blank-node prefix (_:label); not a declared prefix.
            "_" | "filter"
                | "optional"
                | "union"
                | "minus"
                | "bind"
                | "values"
                | "select"
                | "where"
                | "prefix"
                | "distinct"
                | "limit"
                | "offset"
                | "group"
                | "order"
                | "having"
                | "construct"
                | "describe"
                | "ask"
                | "not"
                | "exists"
                | "service"
                | "graph"
                | "from"
                | "named"
                | "base"
        ) {
            return Err(nom::Err::Error(nom::error::Error::new(
                input,
                nom::error::ErrorKind::Tag,
            )));
        }

        let base = ctx
            .prefixes
            .get(prefix)
            .cloned()
            .unwrap_or_else(|| prefix.to_string() + ":");
        Ok((after_local, IriReference(base + &local)))
    }
}

fn parse_string_literal<'a>(
    ctx: &'a ParserContext,
) -> impl Fn(&'a str) -> IResult<&'a str, RdfLiteral> + 'a {
    move |input| {
        // Try all four SPARQL string literal forms (longest match first)
        let (input, value) = alt((
            parse_triple_quoted_string,
            parse_triple_single_quoted_string,
            parse_double_quoted_string,
            parse_single_quoted_string,
        ))(input)?;

        // Optional language tag or datatype
        if input.starts_with('@') {
            let (input, _) = char('@')(input)?;
            let (input, lang) = take_while1(|c: char| c.is_alphanumeric() || c == '-')(input)?;
            return Ok((
                input,
                RdfLiteral::LangLiteral {
                    literal: value,
                    lang: lang.to_string(),
                },
            ));
        }
        if input.starts_with("^^") {
            let (input, _) = tag("^^")(input)?;
            let (input, dt_iri) = alt((parse_iri_ref, parse_prefixed_name(ctx)))(input)?;
            return Ok((
                input,
                RdfLiteral::TypedLiteral {
                    type_iri: dt_iri,
                    literal: value,
                },
            ));
        }

        Ok((input, RdfLiteral::LiteralString(value)))
    }
}

fn parse_triple_quoted_string(input: &str) -> IResult<&str, String> {
    // """..."""
    let (input, _) = tag("\"\"\"")(input)?;
    let (input, content) = take_until("\"\"\"")(input)?;
    let (input, _) = tag("\"\"\"")(input)?;
    Ok((input, content.to_string()))
}

fn parse_triple_single_quoted_string(input: &str) -> IResult<&str, String> {
    // '''...'''
    let (input, _) = tag("'''")(input)?;
    let (input, content) = take_until("'''")(input)?;
    let (input, _) = tag("'''")(input)?;
    Ok((input, content.to_string()))
}

/// Parse a double-quoted string literal (`"..."`) with basic escape handling.
fn parse_double_quoted_string(input: &str) -> IResult<&str, String> {
    let (input, _) = char('"')(input)?;
    let mut result = String::new();
    let mut remaining = input;
    loop {
        let (r, chunk) = take_while(|c: char| c != '"' && c != '\\')(remaining)?;
        result.push_str(chunk);
        if r.starts_with('\\') {
            let (r, _) = char('\\')(r)?;
            let ch = match r.chars().next().unwrap_or('\\') {
                'n' => '\n',
                't' => '\t',
                'r' => '\r',
                '"' => '"',
                '\'' => '\'',
                '\\' => '\\',
                other => other,
            };
            result.push(ch);
            remaining = &r[ch.len_utf8()..];
        } else {
            remaining = r;
            break;
        }
    }
    let (remaining, _) = char('"')(remaining)?;
    Ok((remaining, result))
}

/// Parse a single-quoted string literal (`'...'`) with basic escape handling.
fn parse_single_quoted_string(input: &str) -> IResult<&str, String> {
    let (input, _) = char('\'')(input)?;
    let mut result = String::new();
    let mut remaining = input;
    loop {
        let (r, chunk) = take_while(|c: char| c != '\'' && c != '\\')(remaining)?;
        result.push_str(chunk);
        if r.starts_with('\\') {
            let (r, _) = char('\\')(r)?;
            let ch = match r.chars().next().unwrap_or('\\') {
                'n' => '\n',
                't' => '\t',
                'r' => '\r',
                '\'' => '\'',
                '"' => '"',
                '\\' => '\\',
                other => other,
            };
            result.push(ch);
            remaining = &r[ch.len_utf8()..];
        } else {
            remaining = r;
            break;
        }
    }
    let (remaining, _) = char('\'')(remaining)?;
    Ok((remaining, result))
}

fn parse_numeric_literal(input: &str) -> IResult<&str, RdfLiteral> {
    // Optional sign
    let (input, sign) = opt(alt((char('+'), char('-'))))(input)?;
    // Integer or decimal
    let (input, integer_part) = take_while1(|c: char| c.is_ascii_digit())(input)?;
    let (input, frac) = opt(pair(char('.'), take_while1(|c: char| c.is_ascii_digit())))(input)?;

    let sign_str = match sign {
        Some('-') => "-",
        _ => "",
    };

    // Produce TypedLiteral to match what the turtle crate produces from Turtle data
    if let Some((_, frac_digits)) = frac {
        let s = format!("{}{}.{}", sign_str, integer_part, frac_digits);
        Ok((
            input,
            RdfLiteral::TypedLiteral {
                type_iri: IriReference(XSD_DECIMAL.to_string()),
                literal: s,
            },
        ))
    } else {
        let s = format!("{}{}", sign_str, integer_part);
        Ok((
            input,
            RdfLiteral::TypedLiteral {
                type_iri: IriReference(XSD_INTEGER.to_string()),
                literal: s,
            },
        ))
    }
}

fn parse_boolean_literal(input: &str) -> IResult<&str, RdfLiteral> {
    alt((
        map(tag("true"), |_| RdfLiteral::TypedLiteral {
            type_iri: IriReference(XSD_BOOLEAN.to_string()),
            literal: "true".to_string(),
        }),
        map(tag("false"), |_| RdfLiteral::TypedLiteral {
            type_iri: IriReference(XSD_BOOLEAN.to_string()),
            literal: "false".to_string(),
        }),
    ))(input)
}

fn parse_blank_node(input: &str) -> IResult<&str, u32> {
    // _:label — we hash the label to a u32 (simple but deterministic within a parse)
    let (input, _) = tag("_:")(input)?;
    let (input, label) = take_while1(|c: char| c.is_alphanumeric() || c == '_')(input)?;
    // Simple stable hash
    let hash = label
        .bytes()
        .fold(0u32, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u32));
    Ok((input, hash | 0x8000_0000))
}

// ── FILTER ───────────────────────────────────────────────────────────────────

fn parse_filter<'a>(
    ctx: &'a ParserContext,
) -> impl Fn(&'a str) -> IResult<&'a str, Expression> + 'a {
    move |input| {
        let (input, _) = tag_no_case("FILTER")(input)?;
        let (input, _) = multispace0(input)?;
        parse_expression(ctx)(input)
    }
}

/// Parse `FILTER(expr)` from `input`, returning `(bytes_consumed, expression)`.
///
/// Used by `datalog_parser` to parse `FILTER(...)` atoms in Datalog rule bodies.
/// The `ctx` carries prefix mappings for prefixed IRIs inside expressions.
/// Returns `Err(message)` on parse failure.
pub fn parse_filter_expression(
    input: &str,
    ctx: &ParserContext,
) -> Result<(usize, ast::Expression), String> {
    match parse_filter(ctx)(input) {
        Ok((rest, expr)) => Ok((input.len() - rest.len(), expr)),
        Err(e) => Err(format!("{e:?}")),
    }
}

fn parse_expression<'a>(
    ctx: &'a ParserContext,
) -> impl Fn(&'a str) -> IResult<&'a str, Expression> + 'a {
    move |input| parse_or_expression(ctx)(input)
}

fn parse_or_expression<'a>(
    ctx: &'a ParserContext,
) -> impl Fn(&'a str) -> IResult<&'a str, Expression> + 'a {
    move |input| {
        let (input, left) = parse_and_expression(ctx)(input)?;
        let (input, _) = multispace0(input)?;
        let (input, rest) = many0(preceded(
            pair(tag("||"), multispace0),
            parse_and_expression(ctx),
        ))(input)?;
        Ok((
            input,
            rest.into_iter().fold(left, |acc, r| {
                Expression::Binary(Box::new(acc), BinaryOp::Or, Box::new(r))
            }),
        ))
    }
}

fn parse_and_expression<'a>(
    ctx: &'a ParserContext,
) -> impl Fn(&'a str) -> IResult<&'a str, Expression> + 'a {
    move |input| {
        let (input, left) = parse_relational_expression(ctx)(input)?;
        let (input, _) = multispace0(input)?;
        let (input, rest) = many0(preceded(
            pair(tag("&&"), multispace0),
            parse_relational_expression(ctx),
        ))(input)?;
        Ok((
            input,
            rest.into_iter().fold(left, |acc, r| {
                Expression::Binary(Box::new(acc), BinaryOp::And, Box::new(r))
            }),
        ))
    }
}

fn parse_relational_expression<'a>(
    ctx: &'a ParserContext,
) -> impl Fn(&'a str) -> IResult<&'a str, Expression> + 'a {
    move |input| {
        let (input, left) = parse_additive_expression(ctx)(input)?;
        let (input, _) = multispace0(input)?;

        // NOT IN — must check before comparison operators and before unary NOT
        if let Ok((rest, _)) = tag_no_case::<_, _, nom::error::Error<&str>>("NOT")(input) {
            let boundary = rest
                .chars()
                .next()
                .map(|c| c.is_whitespace())
                .unwrap_or(false);
            if boundary {
                if let Ok((rest2, _)) = preceded(
                    multispace1,
                    tag_no_case::<_, _, nom::error::Error<&str>>("IN"),
                )(rest)
                {
                    let kw_end = rest2
                        .chars()
                        .next()
                        .map(|c| !c.is_alphanumeric() && c != '_')
                        .unwrap_or(true);
                    if kw_end {
                        let (rest2, _) = multispace0(rest2)?;
                        let (rest2, list) = parse_expression_list(ctx)(rest2)?;
                        return Ok((rest2, Expression::NotIn(Box::new(left), list)));
                    }
                }
            }
        }

        // IN — check word boundary so we don't consume prefix of longer identifier
        if let Ok((rest, _)) = tag_no_case::<_, _, nom::error::Error<&str>>("IN")(input) {
            let kw_end = rest
                .chars()
                .next()
                .map(|c| !c.is_alphanumeric() && c != '_')
                .unwrap_or(true);
            if kw_end {
                let (rest, _) = multispace0(rest)?;
                if rest.starts_with('(') {
                    let (rest, list) = parse_expression_list(ctx)(rest)?;
                    return Ok((rest, Expression::In(Box::new(left), list)));
                }
            }
        }

        // Comparison operators
        let (input, op_right) = opt(pair(
            alt((
                map(tag("!="), |_| BinaryOp::Ne),
                map(tag("<="), |_| BinaryOp::Le),
                map(tag(">="), |_| BinaryOp::Ge),
                map(tag("<"), |_| BinaryOp::Lt),
                map(tag(">"), |_| BinaryOp::Gt),
                map(tag("="), |_| BinaryOp::Eq),
            )),
            preceded(multispace0, |i| parse_additive_expression(ctx)(i)),
        ))(input)?;
        Ok((
            input,
            match op_right {
                Some((op, right)) => Expression::Binary(Box::new(left), op, Box::new(right)),
                None => left,
            },
        ))
    }
}

fn parse_expression_list<'a>(
    ctx: &'a ParserContext,
) -> impl Fn(&'a str) -> IResult<&'a str, Vec<Expression>> + 'a {
    move |input| {
        let (input, _) = char('(')(input)?;
        let (input, _) = multispace0(input)?;
        let (input, list) = separated_list0(tuple((multispace0, char(','), multispace0)), |i| {
            parse_expression(ctx)(i)
        })(input)?;
        let (input, _) = multispace0(input)?;
        let (input, _) = char(')')(input)?;
        Ok((input, list))
    }
}

fn parse_additive_expression<'a>(
    ctx: &'a ParserContext,
) -> impl Fn(&'a str) -> IResult<&'a str, Expression> + 'a {
    move |input| {
        let (input, left) = parse_multiplicative_expression(ctx)(input)?;
        let (input, _) = multispace0(input)?;
        let (input, rest) = many0(pair(
            alt((
                map(char('+'), |_| BinaryOp::Add),
                map(char('-'), |_| BinaryOp::Sub),
            )),
            preceded(multispace0, |i| parse_multiplicative_expression(ctx)(i)),
        ))(input)?;
        Ok((
            input,
            rest.into_iter().fold(left, |acc, (op, r)| {
                Expression::Binary(Box::new(acc), op, Box::new(r))
            }),
        ))
    }
}

fn parse_multiplicative_expression<'a>(
    ctx: &'a ParserContext,
) -> impl Fn(&'a str) -> IResult<&'a str, Expression> + 'a {
    move |input| {
        let (input, left) = parse_unary_expression(ctx)(input)?;
        let (input, _) = multispace0(input)?;
        let (input, rest) = many0(pair(
            alt((
                map(char('*'), |_| BinaryOp::Mul),
                map(char('/'), |_| BinaryOp::Div),
            )),
            preceded(multispace0, |i| parse_unary_expression(ctx)(i)),
        ))(input)?;
        Ok((
            input,
            rest.into_iter().fold(left, |acc, (op, r)| {
                Expression::Binary(Box::new(acc), op, Box::new(r))
            }),
        ))
    }
}

fn parse_unary_expression<'a>(
    ctx: &'a ParserContext,
) -> impl Fn(&'a str) -> IResult<&'a str, Expression> + 'a {
    move |input| {
        alt((
            map(
                preceded(pair(char('!'), multispace0), |i| {
                    parse_primary_expression(ctx)(i)
                }),
                |e| Expression::Unary(UnaryOp::Not, Box::new(e)),
            ),
            map(
                preceded(pair(char('-'), multispace0), |i| {
                    parse_primary_expression(ctx)(i)
                }),
                |e| Expression::Unary(UnaryOp::Minus, Box::new(e)),
            ),
            |i| parse_primary_expression(ctx)(i),
        ))(input)
    }
}

fn parse_primary_expression<'a>(
    ctx: &'a ParserContext,
) -> impl Fn(&'a str) -> IResult<&'a str, Expression> + 'a {
    move |input| {
        alt((
            // Parenthesised expression
            map(
                delimited(
                    pair(char('('), multispace0),
                    |i| parse_expression(ctx)(i),
                    pair(multispace0, char(')')),
                ),
                |e| e,
            ),
            // NOT EXISTS
            map(
                preceded(
                    tuple((
                        tag_no_case("NOT"),
                        multispace1,
                        tag_no_case("EXISTS"),
                        multispace0,
                    )),
                    parse_group_graph_pattern(ctx),
                ),
                Expression::NotExists,
            ),
            // EXISTS
            map(
                preceded(
                    pair(tag_no_case("EXISTS"), multispace0),
                    parse_group_graph_pattern(ctx),
                ),
                Expression::Exists,
            ),
            // Function calls (regex, bound, str, lang, datatype, etc.)
            |i| parse_function_call(ctx)(i),
            // Variable
            map(preceded(char('?'), parse_varname), Expression::Variable),
            // Literal (constant)
            map(
                alt((
                    map(parse_string_literal(ctx), |lit| {
                        GraphElement::GraphLiteral(lit)
                    }),
                    map(parse_numeric_literal, GraphElement::GraphLiteral),
                    map(parse_boolean_literal, GraphElement::GraphLiteral),
                    map(parse_iri_ref, |iri| {
                        GraphElement::NodeOrEdge(RdfResource::Iri(iri))
                    }),
                    map(parse_prefixed_name(ctx), |iri| {
                        GraphElement::NodeOrEdge(RdfResource::Iri(iri))
                    }),
                )),
                Expression::Constant,
            ),
        ))(input)
    }
}

fn parse_function_call<'a>(
    ctx: &'a ParserContext,
) -> impl Fn(&'a str) -> IResult<&'a str, Expression> + 'a {
    move |input| {
        // Function name: bare word, prefixed IRI, or full <IRI>
        let (input, fname) = alt((
            map(
                take_while1(|c: char| c.is_alphanumeric() || c == '_'),
                |s: &str| s.to_string(),
            ),
            map(parse_iri_ref, |iri| iri.0),
            map(parse_prefixed_name(ctx), |iri| iri.0),
        ))(input)?;

        let (input, _) = multispace0(input)?;
        let (input, _) = char('(')(input)?;
        let (input, _) = multispace0(input)?;

        // Intercept aggregate keywords and produce Expression::Aggregate
        let fname_upper = fname.to_ascii_uppercase();
        if matches!(
            fname_upper.as_str(),
            "COUNT" | "SUM" | "AVG" | "MIN" | "MAX" | "SAMPLE" | "GROUP_CONCAT"
        ) {
            // Optional DISTINCT keyword
            let (input, distinct) =
                map(opt(terminated(tag_no_case("DISTINCT"), multispace1)), |d| {
                    d.is_some()
                })(input)?;

            // COUNT(*) special form
            if fname_upper == "COUNT" && input.starts_with('*') {
                let (input, _) = char('*')(input)?;
                let (input, _) = multispace0(input)?;
                let (input, _) = char(')')(input)?;
                return Ok((input, Expression::Aggregate(Aggregate::CountStar)));
            }

            let (input, expr) = parse_expression(ctx)(input)?;
            let (input, _) = multispace0(input)?;

            // GROUP_CONCAT optional separator: ; separator="sep"
            if fname_upper == "GROUP_CONCAT" {
                let (input, sep) = opt(parse_group_concat_separator(ctx))(input)?;
                let sep = sep.unwrap_or_else(|| " ".to_string());
                let (input, _) = multispace0(input)?;
                let (input, _) = char(')')(input)?;
                return Ok((
                    input,
                    Expression::Aggregate(Aggregate::GroupConcat(Box::new(expr), sep, distinct)),
                ));
            }

            let (input, _) = char(')')(input)?;
            let agg = match fname_upper.as_str() {
                "COUNT" => Aggregate::Count(Box::new(expr), distinct),
                "SUM" => Aggregate::Sum(Box::new(expr), distinct),
                "AVG" => Aggregate::Avg(Box::new(expr), distinct),
                "MIN" => Aggregate::Min(Box::new(expr), distinct),
                "MAX" => Aggregate::Max(Box::new(expr), distinct),
                "SAMPLE" => Aggregate::Sample(Box::new(expr), distinct),
                _ => unreachable!(),
            };
            return Ok((input, Expression::Aggregate(agg)));
        }

        // Regular function call
        let (input, args) =
            separated_list0(pair(multispace0, pair(char(','), multispace0)), |i| {
                parse_expression(ctx)(i)
            })(input)?;
        let (input, _) = multispace0(input)?;
        let (input, _) = char(')')(input)?;

        Ok((input, Expression::FunctionCall(fname, args)))
    }
}

fn parse_group_concat_separator<'a>(
    ctx: &'a ParserContext,
) -> impl Fn(&'a str) -> IResult<&'a str, String> + 'a {
    move |input| {
        let (input, _) = char(';')(input)?;
        let (input, _) = multispace0(input)?;
        let (input, _) = tag_no_case("separator")(input)?;
        let (input, _) = multispace0(input)?;
        let (input, _) = char('=')(input)?;
        let (input, _) = multispace0(input)?;
        let (input, lit) = parse_string_literal(ctx)(input)?;
        let sep = match lit {
            RdfLiteral::LiteralString(s) => s,
            _ => " ".to_string(),
        };
        Ok((input, sep))
    }
}

// ── BIND ─────────────────────────────────────────────────────────────────────

fn parse_bind<'a>(
    ctx: &'a ParserContext,
) -> impl Fn(&'a str) -> IResult<&'a str, (Expression, String)> + 'a {
    move |input| {
        let (input, _) = tag_no_case("BIND")(input)?;
        let (input, _) = multispace0(input)?;
        let (input, _) = char('(')(input)?;
        let (input, _) = multispace0(input)?;
        let (input, expr) = parse_expression(ctx)(input)?;
        let (input, _) = multispace0(input)?;
        let (input, _) = tag_no_case("AS")(input)?;
        let (input, _) = multispace1(input)?;
        let (input, var) = preceded(char('?'), parse_varname)(input)?;
        let (input, _) = multispace0(input)?;
        let (input, _) = char(')')(input)?;
        Ok((input, (expr, var)))
    }
}

// ── VALUES ───────────────────────────────────────────────────────────────────

fn parse_values<'a>(
    ctx: &'a ParserContext,
) -> impl Fn(&'a str) -> IResult<&'a str, QueryComponent> + 'a {
    move |input| {
        let (input, _) = tag_no_case("VALUES")(input)?;
        let (input, _) = multispace1(input)?;

        // Single variable: VALUES ?x { ... }
        // Multiple: VALUES (?x ?y) { ... }
        let (input, vars) = alt((
            // Single variable
            map(preceded(char('?'), parse_varname), |v| vec![v]),
            // Multiple in parens
            delimited(
                pair(char('('), multispace0),
                many0(terminated(preceded(char('?'), parse_varname), multispace0)),
                char(')'),
            ),
        ))(input)?;

        let (input, _) = multispace0(input)?;
        let (input, _) = char('{')(input)?;
        let (input, _) = multispace0(input)?;

        // Rows
        let (input, rows) = many0(parse_values_row(ctx, vars.len()))(input)?;

        let (input, _) = multispace0(input)?;
        let (input, _) = char('}')(input)?;

        Ok((input, QueryComponent::Values(vars, rows)))
    }
}

fn parse_values_row<'a>(
    ctx: &'a ParserContext,
    n_vars: usize,
) -> impl Fn(&'a str) -> IResult<&'a str, Vec<Option<GraphElement>>> + 'a {
    move |input| {
        let (input, _) = multispace0(input)?;
        if n_vars == 1 {
            // Without parens for single var
            let (input, val) = parse_values_value(ctx)(input)?;
            let (input, _) = multispace0(input)?;
            Ok((input, vec![val]))
        } else {
            let (input, _) = char('(')(input)?;
            let (input, _) = multispace0(input)?;
            let (input, vals) = separated_list0(multispace1, parse_values_value(ctx))(input)?;
            let (input, _) = multispace0(input)?;
            let (input, _) = char(')')(input)?;
            let (input, _) = multispace0(input)?;
            Ok((input, vals))
        }
    }
}

fn parse_values_value<'a>(
    ctx: &'a ParserContext,
) -> impl Fn(&'a str) -> IResult<&'a str, Option<GraphElement>> + 'a {
    move |input| {
        alt((
            map(tag_no_case("UNDEF"), |_| None),
            map(parse_term(ctx), |t| match t {
                Term::Constant(gel) => Some(gel),
                Term::Variable(_) => None,
            }),
        ))(input)
    }
}

// ── GROUP BY / HAVING / ORDER BY / LIMIT / OFFSET ───────────────────────────

fn parse_group_by<'a>(
    ctx: &'a ParserContext,
) -> impl Fn(&'a str) -> IResult<&'a str, Vec<Expression>> + 'a {
    move |input| {
        let (input, gb) = opt(preceded(
            tuple((
                tag_no_case("GROUP"),
                multispace1,
                tag_no_case("BY"),
                multispace1,
            )),
            separated_list1(multispace1, |i| parse_expression(ctx)(i)),
        ))(input)?;
        Ok((input, gb.unwrap_or_default()))
    }
}

fn parse_having<'a>(
    ctx: &'a ParserContext,
) -> impl Fn(&'a str) -> IResult<&'a str, Vec<Expression>> + 'a {
    move |input| {
        let (input, hv) = opt(preceded(pair(tag_no_case("HAVING"), multispace1), |i| {
            parse_expression(ctx)(i)
        }))(input)?;
        Ok((input, hv.map(|e| vec![e]).unwrap_or_default()))
    }
}

fn parse_order_by<'a>(
    ctx: &'a ParserContext,
) -> impl Fn(&'a str) -> IResult<&'a str, Vec<OrderCondition>> + 'a {
    move |input| {
        let (input, ob) = opt(preceded(
            tuple((
                tag_no_case("ORDER"),
                multispace1,
                tag_no_case("BY"),
                multispace1,
            )),
            separated_list1(multispace1, parse_order_condition(ctx)),
        ))(input)?;
        Ok((input, ob.unwrap_or_default()))
    }
}

fn parse_order_condition<'a>(
    ctx: &'a ParserContext,
) -> impl Fn(&'a str) -> IResult<&'a str, OrderCondition> + 'a {
    move |input| {
        alt((
            map(
                preceded(pair(tag_no_case("ASC"), multispace0), |i| {
                    parse_expression(ctx)(i)
                }),
                |e| OrderCondition {
                    expression: e,
                    ascending: true,
                },
            ),
            map(
                preceded(pair(tag_no_case("DESC"), multispace0), |i| {
                    parse_expression(ctx)(i)
                }),
                |e| OrderCondition {
                    expression: e,
                    ascending: false,
                },
            ),
            map(
                |i| parse_expression(ctx)(i),
                |e| OrderCondition {
                    expression: e,
                    ascending: true,
                },
            ),
        ))(input)
    }
}

fn parse_limit_offset(keyword: &str) -> impl Fn(&str) -> IResult<&str, Option<u64>> + '_ {
    move |input| {
        let (input, val) = opt(preceded(
            pair(tag_no_case(keyword), multispace1),
            map(take_while1(|c: char| c.is_ascii_digit()), |s: &str| {
                s.parse::<u64>().unwrap_or(0)
            }),
        ))(input)?;
        Ok((input, val))
    }
}

pub mod ast;

use nom::{
    branch::alt,
    bytes::complete::{tag, tag_no_case},
    character::complete::{char, multispace0, multispace1, alphanumeric1},
    combinator::{map, opt, recognize},
    multi::{many0, separated_list0},
    sequence::{delimited, pair, preceded, tuple},
    IResult,
};
use crate::ast::*;
use dag_rdf::{GraphElement, RdfResource, RdfLiteral, IriReference};

use std::collections::HashMap;

pub struct ParserContext {
    pub prefixes: HashMap<String, String>,
}

pub fn parse_query<'a>(input: &'a str, ctx: &mut ParserContext) -> IResult<&'a str, Query> {
    let (input, _) = multispace0(input)?;
    let (input, _) = many0(parse_prefix(ctx))(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = tag_no_case("SELECT")(input)?;
    let (input, _) = multispace1(input)?;
    
    let (input, projection) = separated_list0(
        multispace1,
        map(preceded(char('?'), alphanumeric1), |name: &str| ProjectionElement::Variable(name.to_string()))
    )(input)?;

    let (input, _) = multispace0(input)?;
    let (input, _) = tag_no_case("WHERE")(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = char('{')(input)?;
    let (input, _) = multispace0(input)?;
    
    let (input, patterns) = many0(parse_triple_pattern(ctx))(input)?;

    let (input, _) = multispace0(input)?;
    let (input, _) = char('}')(input)?;
    let (input, _) = multispace0(input)?;

    let bgp = QueryComponent::BGP(patterns);

    Ok((input, Query::Select {
        projection,
        where_clause: vec![bgp],
        group_by: vec![],
        having: vec![],
        order_by: vec![],
        limit: None,
        offset: None,
        distinct: false,
    }))
}

fn parse_prefix<'a>(ctx: &mut ParserContext) -> impl FnMut(&'a str) -> IResult<&'a str, ()> + '_ {
    move |input| {
        let (input, _) = multispace0(input)?;
        let (input, _) = tag_no_case("PREFIX")(input)?;
        let (input, _) = multispace1(input)?;
        let (input, prefix_name) = recognize(pair(alphanumeric1, char(':')))(input)?;
        let (input, _) = multispace1(input)?;
        let (input, iri) = parse_iri(input)?;
        let (input, _) = opt(char('.'))(input)?;
        
        ctx.prefixes.insert(prefix_name[..prefix_name.len()-1].to_string(), iri.0);
        Ok((input, ()))
    }
}

fn parse_triple_pattern<'a>(ctx: &ParserContext) -> impl FnMut(&'a str) -> IResult<&'a str, TriplePattern> + '_ {
    move |input| {
        let (input, _) = multispace0(input)?;
        let (input, subject) = parse_term(ctx)(input)?;
        let (input, _) = multispace1(input)?;
        let (input, predicate) = parse_term(ctx)(input)?;
        let (input, _) = multispace1(input)?;
        let (input, object) = parse_term(ctx)(input)?;
        let (input, _) = multispace0(input)?;
        let (input, _) = opt(char('.'))(input)?;
        let (input, _) = multispace0(input)?;
        Ok((input, TriplePattern { subject, predicate, object }))
    }
}

fn parse_term<'a>(ctx: &ParserContext) -> impl FnMut(&'a str) -> IResult<&'a str, Term> + '_ {
    move |input| {
        alt((
            map(preceded(char('?'), alphanumeric1), |name: &str| Term::Variable(name.to_string())),
            map(parse_iri, |iri| Term::Constant(GraphElement::NodeOrEdge(RdfResource::Iri(iri)))),
            map(recognize(pair(opt(alphanumeric1), pair(char(':'), alphanumeric1))), |prefixed: &str| {
                let parts: Vec<&str> = prefixed.split(':').collect();
                let prefix = parts[0];
                let local = parts[1];
                let expanded = ctx.prefixes.get(prefix).cloned().unwrap_or_else(|| prefix.to_string()) + local;
                Term::Constant(GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(expanded))))
            }),
        ))(input)
    }
}

fn parse_iri(input: &str) -> IResult<&str, IriReference> {
    map(
        delimited(char('<'), recognize(many0(alt((alphanumeric1, tag("/"), tag("."), tag(":"), tag("#"))))), char('>')),
        |iri: &str| IriReference(iri.to_string())
    )(input)
}

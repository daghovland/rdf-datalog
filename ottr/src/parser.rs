use crate::ast::{Argument, Instance, Parameter, StottrDocument, TemplateDef, Term};
use crate::error::OttrError;
use crate::types::OttrType;
use ingress::{IriReference, RdfLiteral};
use nom::{
    IResult,
    branch::alt,
    bytes::complete::{tag, take_while, take_while1},
    character::complete::{char, multispace0, multispace1},
    combinator::{map, opt},
    multi::{many0, separated_list0},
    sequence::{delimited, pair, preceded, terminated},
};
use std::collections::HashMap;

struct ParserContext {
    prefixes: HashMap<String, String>,
}

impl ParserContext {
    fn resolve(&self, prefix: &str, local: &str) -> IriReference {
        let base = self.prefixes.get(prefix).cloned().unwrap_or_default();
        IriReference(format!("{base}{local}"))
    }
}

fn is_name_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn parse_iri_ref(input: &str) -> IResult<&str, IriReference> {
    map(
        delimited(char('<'), take_while(|c: char| c != '>'), char('>')),
        |iri: &str| IriReference(iri.to_string()),
    )(input)
}

fn parse_prefix_decl(input: &str) -> IResult<&str, (String, String)> {
    let (input, _) = tag("@prefix")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, prefix) = take_while(is_name_char)(input)?;
    let (input, _) = char(':')(input)?;
    let (input, _) = multispace0(input)?;
    let (input, iri) = parse_iri_ref(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = char('.')(input)?;
    Ok((input, (prefix.to_string(), iri.0)))
}

fn parse_prefixed_name<'a>(
    ctx: &'a ParserContext,
) -> impl Fn(&'a str) -> IResult<&'a str, IriReference> + 'a {
    move |input: &'a str| {
        let (input, prefix) = take_while(is_name_char)(input)?;
        let (input, _) = char(':')(input)?;
        let (input, local) = take_while1(is_name_char)(input)?;
        Ok((input, ctx.resolve(prefix, local)))
    }
}

fn parse_variable(input: &str) -> IResult<&str, String> {
    let (input, _) = char('?')(input)?;
    let (input, name) = take_while1(is_name_char)(input)?;
    Ok((input, name.to_string()))
}

fn parse_blank_node_label(input: &str) -> IResult<&str, String> {
    let (input, _) = tag("_:")(input)?;
    let (input, label) = take_while1(is_name_char)(input)?;
    Ok((input, label.to_string()))
}

fn parse_quoted_string(input: &str) -> IResult<&str, String> {
    map(
        delimited(char('"'), take_while(|c: char| c != '"'), char('"')),
        |s: &str| s.to_string(),
    )(input)
}

/// Parse a literal: a quoted string, optionally followed by `^^datatype`
/// (typed literal) or `@lang` (language-tagged string).
fn parse_literal<'a>(
    ctx: &'a ParserContext,
) -> impl Fn(&'a str) -> IResult<&'a str, RdfLiteral> + 'a {
    move |input: &'a str| {
        let (input, lexical) = parse_quoted_string(input)?;
        let (input, type_iri) = opt(preceded(tag("^^"), parse_prefixed_name(ctx)))(input)?;
        if let Some(type_iri) = type_iri {
            return Ok((
                input,
                RdfLiteral::TypedLiteral {
                    type_iri,
                    literal: lexical,
                },
            ));
        }
        let (input, lang) = opt(preceded(
            char('@'),
            take_while1(|c: char| c.is_alphanumeric() || c == '-'),
        ))(input)?;
        if let Some(lang) = lang {
            return Ok((
                input,
                RdfLiteral::LangLiteral {
                    lang: lang.to_string(),
                    literal: lexical,
                },
            ));
        }
        Ok((input, RdfLiteral::LiteralString(lexical)))
    }
}

/// Parse a parameter type: `ottr:IRI`, `ottr:BlankNode`, `ottr:Literal`, or a
/// datatype IRI such as `xsd:string` (the latter becomes `Literal(Some(iri))`).
/// List/NEList types are deferred to the list-expander phase.
fn parse_type<'a>(ctx: &'a ParserContext) -> impl Fn(&'a str) -> IResult<&'a str, OttrType> + 'a {
    move |input: &'a str| {
        let (rest, prefix) = take_while(is_name_char)(input)?;
        let (rest, _) = char(':')(rest)?;
        let (rest, local) = take_while1(is_name_char)(rest)?;
        let ottr_type = match (prefix, local) {
            ("ottr", "IRI") => OttrType::Iri,
            ("ottr", "BlankNode") => OttrType::BlankNode,
            ("ottr", "Literal") => OttrType::Literal(None),
            _ => OttrType::Literal(Some(ctx.resolve(prefix, local))),
        };
        Ok((rest, ottr_type))
    }
}

fn parse_parameter<'a>(
    ctx: &'a ParserContext,
) -> impl Fn(&'a str) -> IResult<&'a str, Parameter> + 'a {
    move |input: &'a str| {
        let (input, maybe_type) = opt(parse_type(ctx))(input)?;
        let (input, _) = multispace0(input)?;
        let (input, variable) = parse_variable(input)?;
        Ok((
            input,
            Parameter {
                variable,
                ottr_type: maybe_type.unwrap_or(OttrType::Iri),
                optional: false,
                default: None,
            },
        ))
    }
}

fn comma_separated0<'a, O>(
    item: impl Fn(&'a str) -> IResult<&'a str, O> + 'a,
) -> impl Fn(&'a str) -> IResult<&'a str, Vec<O>> + 'a {
    move |input: &'a str| {
        separated_list0(delimited(multispace0, char(','), multispace0), &item)(input)
    }
}

fn parse_term<'a>(ctx: &'a ParserContext) -> impl Fn(&'a str) -> IResult<&'a str, Term> + 'a {
    move |input: &'a str| {
        alt((
            map(parse_variable, Term::Variable),
            map(parse_literal(ctx), Term::Literal),
            // Blank node labels (`_:b1`) must be tried before prefixed names,
            // since `_` is itself a valid (if unusual) prefix name character.
            map(parse_blank_node_label, Term::BlankNode),
            map(parse_prefixed_name(ctx), Term::Iri),
            map(parse_iri_ref, Term::Iri),
        ))(input)
    }
}

fn parse_argument<'a>(
    ctx: &'a ParserContext,
) -> impl Fn(&'a str) -> IResult<&'a str, Argument> + 'a {
    move |input: &'a str| map(parse_term(ctx), Argument::Term)(input)
}

fn parse_instance<'a>(
    ctx: &'a ParserContext,
) -> impl Fn(&'a str) -> IResult<&'a str, Instance> + 'a {
    move |input: &'a str| {
        let (input, template) = parse_prefixed_name(ctx)(input)?;
        let (input, _) = multispace0(input)?;
        let (input, arguments) = delimited(
            pair(char('('), multispace0),
            comma_separated0(parse_argument(ctx)),
            pair(multispace0, char(')')),
        )(input)?;
        Ok((
            input,
            Instance {
                template,
                arguments,
                expander: None,
            },
        ))
    }
}

fn parse_instance_list<'a>(
    ctx: &'a ParserContext,
) -> impl Fn(&'a str) -> IResult<&'a str, Vec<Instance>> + 'a {
    move |input: &'a str| {
        delimited(
            pair(char('{'), multispace0),
            comma_separated0(parse_instance(ctx)),
            pair(multispace0, char('}')),
        )(input)
    }
}

fn parse_template_def<'a>(
    ctx: &'a ParserContext,
) -> impl Fn(&'a str) -> IResult<&'a str, TemplateDef> + 'a {
    move |input: &'a str| {
        let (input, id) = parse_prefixed_name(ctx)(input)?;
        let (input, _) = multispace0(input)?;
        let (input, parameters) = delimited(
            pair(char('['), multispace0),
            comma_separated0(parse_parameter(ctx)),
            pair(multispace0, char(']')),
        )(input)?;
        let (input, _) = multispace0(input)?;
        let (input, _) = tag("::")(input)?;
        let (input, _) = multispace0(input)?;
        let (input, body) = parse_instance_list(ctx)(input)?;
        let (input, _) = multispace0(input)?;
        let (input, _) = char('.')(input)?;
        Ok((
            input,
            TemplateDef {
                id,
                parameters,
                body,
            },
        ))
    }
}

enum Statement {
    Template(TemplateDef),
    Instance(Instance),
}

/// A top-level statement is either a template definition (`id [...] :: {...} .`)
/// or a bare instance call (`id (...) .`), as found in an instance/data file.
fn parse_statement<'a>(
    ctx: &'a ParserContext,
) -> impl Fn(&'a str) -> IResult<&'a str, Statement> + 'a {
    move |input: &'a str| {
        alt((
            map(parse_template_def(ctx), Statement::Template),
            map(
                terminated(parse_instance(ctx), pair(multispace0, char('.'))),
                Statement::Instance,
            ),
        ))(input)
    }
}

/// Parse a stOTTR source file (template definitions and/or instances).
pub fn parse_stottr(input: &str) -> Result<StottrDocument, OttrError> {
    let (rest, decls) = many0(delimited(multispace0, parse_prefix_decl, multispace0))(input)
        .map_err(|e: nom::Err<nom::error::Error<&str>>| OttrError::Parse(format!("{e:?}")))?;
    let prefixes = decls.into_iter().collect();
    let ctx = ParserContext { prefixes };

    let (rest, statements) =
        many0(delimited(multispace0, parse_statement(&ctx), multispace0))(rest)
            .map_err(|e: nom::Err<nom::error::Error<&str>>| OttrError::Parse(format!("{e:?}")))?;

    if !rest.trim().is_empty() {
        return Err(OttrError::Parse(format!(
            "unconsumed input at: {:.60}",
            rest
        )));
    }

    let mut templates = Vec::new();
    let mut instances = Vec::new();
    for statement in statements {
        match statement {
            Statement::Template(t) => templates.push(t),
            Statement::Instance(i) => instances.push(i),
        }
    }

    Ok(StottrDocument {
        templates,
        instances,
    })
}

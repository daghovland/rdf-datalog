/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! OWL 2 Functional-Style Syntax parser.
//!
//! Parses [OWL 2 Functional-Style Syntax](https://www.w3.org/TR/owl2-syntax/)
//! (`.ofn`) documents into an [`owl_ontology::Ontology`].
//!
//! See `docs/plans/OWL_FUNCTIONAL_SYNTAX_PARSER_PLAN.md` for the grammar
//! subset this parser covers and the module layout. Issue
//! [#180](https://github.com/daghovland/rdf-datalog/issues/180) tracks this
//! feature; SWRL `Rule(...)` parsing is deferred to
//! [#625](https://github.com/daghovland/rdf-datalog/issues/625).

mod annotation;
mod axiom;
mod class_expr;
mod data_range;
mod individual;
mod iri;
mod literal;
mod property_expr;
mod serialize;
mod tokens;

use iri::ParserContext;
use nom::Parser;
use owl_ontology::Ontology;

pub use serialize::serialize;

/// `prefixDeclaration ::= 'Prefix' '(' prefixName '=' fullIRI ')'`. Unlike
/// `abbreviatedIRI`'s `prefix:local` shape, the prefix *name* here is
/// terminated by `=`, not `:` -- so this reads the name directly rather than
/// reusing `iri::abbreviated_iri`'s prefixed-name parser.
fn prefix_declaration<'a>(
    ctx: &'a ParserContext,
) -> impl FnMut(&'a str) -> nom::IResult<&'a str, ()> {
    move |input: &'a str| {
        let (input, _) = tokens::keyword("Prefix")(input)?;
        let (input, _) = tokens::punct('(')(input)?;
        let name_end = input
            .find(|c: char| !tokens::is_ident_char(c))
            .unwrap_or(input.len());
        let name = input[..name_end].to_string();
        let rest = &input[name_end..];
        let (rest, _) = tokens::punct(':')(rest)?;
        let (rest, _) = tokens::punct('=')(rest)?;
        let (rest, iri_str) = iri::full_iri(rest)?;
        let (rest, _) = tokens::punct(')')(rest)?;
        ctx.declare_prefix(&name, &iri_str);
        Ok((rest, ()))
    }
}

fn import_decl<'a>(
    ctx: &'a ParserContext,
) -> impl FnMut(&'a str) -> nom::IResult<&'a str, ingress::IriReference> {
    move |input: &'a str| {
        let (input, resolved) = tokens::paren_form("Import", iri::iri(ctx)).parse(input)?;
        Ok((input, resolved.0))
    }
}

/// Parse an OWL 2 Functional-Style Syntax `ontologyDocument` and produce an
/// [`owl_ontology::Ontology`].
pub fn parse(input: &str) -> Result<Ontology, String> {
    let ctx = ParserContext::new();

    let (input, ()) = tokens::sp(input).map_err(fail)?;
    let (input, _prefixes) = nom::multi::many0(prefix_declaration(&ctx))
        .parse(input)
        .map_err(fail)?;

    let (input, _) = tokens::keyword("Ontology")(input).map_err(fail)?;
    let (input, _) = tokens::punct('(')(input).map_err(fail)?;

    let (input, ontology_iri) = nom::combinator::opt(iri::full_iri)
        .parse(input)
        .map_err(fail)?;
    let (input, version_iri) = if ontology_iri.is_some() {
        nom::combinator::opt(iri::full_iri)
            .parse(input)
            .map_err(fail)?
    } else {
        (input, None)
    };

    let (input, imports) = nom::multi::many0(import_decl(&ctx))
        .parse(input)
        .map_err(fail)?;

    let (input, ontology_annotations) = nom::multi::many0(annotation::annotation(&ctx))
        .parse(input)
        .map_err(fail)?;

    let (input, axioms) = nom::multi::many0(axiom::axiom(&ctx))
        .parse(input)
        .map_err(fail)?;

    let (input, _) = tokens::punct(')')(input).map_err(fail)?;

    let (input, ()) = tokens::sp(input).map_err(fail)?;
    if !input.is_empty() {
        let preview: String = input.chars().take(80).collect();
        return Err(format!(
            "OWL 2 Functional-Style Syntax parse error: unrecognized input at: {preview:?}"
        ));
    }

    let version = match (ontology_iri, version_iri) {
        (Some(o), Some(v)) => ingress::OntologyVersion::VersionedOntology {
            ontology_iri: ingress::IriReference(o),
            version_iri: ingress::IriReference(v),
        },
        (Some(o), None) => ingress::OntologyVersion::NamedOntology(ingress::IriReference(o)),
        (None, _) => ingress::OntologyVersion::UnNamedOntology,
    };

    Ok(Ontology::new(
        imports,
        version,
        ontology_annotations,
        axioms,
    ))
}

fn fail(e: nom::Err<nom::error::Error<&str>>) -> String {
    format!("OWL 2 Functional-Style Syntax parse error: {e:?}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_unnamed_empty_ontology() {
        let onto = parse("Ontology()").unwrap();
        assert_eq!(onto.version, ingress::OntologyVersion::UnNamedOntology);
        assert!(onto.axioms.is_empty());
    }

    #[test]
    fn parses_named_ontology_with_version_iri() {
        let onto =
            parse("Ontology(<http://example.org/pizza> <http://example.org/pizza/1.0>)").unwrap();
        assert_eq!(
            onto.version,
            ingress::OntologyVersion::VersionedOntology {
                ontology_iri: ingress::IriReference("http://example.org/pizza".to_string()),
                version_iri: ingress::IriReference("http://example.org/pizza/1.0".to_string()),
            }
        );
    }

    #[test]
    fn parses_prefix_declarations_and_import() {
        let onto = parse(
            "Prefix(:=<http://example.org/pizza#>)\n\
             Prefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
             Ontology(<http://example.org/pizza>\n\
                 Import(<http://example.org/imported>)\n\
                 Declaration(Class(:Pizza))\n\
             )",
        )
        .unwrap();
        assert_eq!(
            onto.directly_imports_documents,
            vec![ingress::IriReference(
                "http://example.org/imported".to_string()
            )]
        );
        assert_eq!(onto.axioms.len(), 1);
    }

    #[test]
    fn parses_ontology_level_annotation() {
        let onto = parse(
            "Prefix(rdfs:=<http://www.w3.org/2000/01/rdf-schema#>)\n\
             Ontology(Annotation(rdfs:label \"My Ontology\"))",
        )
        .unwrap();
        assert_eq!(onto.annotations.len(), 1);
    }

    #[test]
    fn parses_pizza_style_multi_axiom_ontology() {
        let src = "\
Prefix(:=<http://example.org/pizza#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Ontology(<http://example.org/pizza>
    Declaration(Class(:Pizza))
    Declaration(Class(:Food))
    Declaration(ObjectProperty(:hasTopping))
    SubClassOf(:Pizza :Food)
    EquivalentClasses(:Pizza ObjectIntersectionOf(:Food ObjectSomeValuesFrom(:hasTopping :Topping)))
    ObjectPropertyDomain(:hasTopping :Pizza)
    ObjectPropertyRange(:hasTopping :Topping)
    InverseFunctionalObjectProperty(:hasTopping)
)";
        let onto = parse(src).unwrap();
        assert_eq!(
            onto.try_get_ontology_iri(),
            Some(&ingress::IriReference(
                "http://example.org/pizza".to_string()
            ))
        );
        assert_eq!(onto.axioms.len(), 8);
    }
}

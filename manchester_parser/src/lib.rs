/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! OWL Manchester Syntax parser.
//!
//! Parses [OWL 2 Manchester Syntax](https://www.w3.org/TR/owl2-manchester-syntax/)
//! (`.omn`) documents into an [`owl_ontology::Ontology`].
//!
//! See `docs/plans/MANCHESTER_SYNTAX_PLAN.md` for the grammar subset this
//! parser covers, the module layout, and what's deferred (tracked in
//! [#157](https://github.com/daghovland/rdf-datalog/issues/157)). Issue
//! [#139](https://github.com/daghovland/rdf-datalog/issues/139) tracks this
//! feature.

mod annotation;
mod class_expr;
mod data_range;
mod frame;
mod individual;
mod iri;
mod literal;
mod property_expr;
mod serialize;
mod tokens;

use iri::ParserContext;
use nom::multi::many0;
use owl_ontology::Ontology;

pub use serialize::serialize;

fn prefix_declaration<'a>(
    ctx: &'a ParserContext,
) -> impl FnMut(&'a str) -> nom::IResult<&'a str, ()> {
    move |input: &'a str| {
        let (input, _) = tokens::keyword("Prefix:")(input)?;
        let prefix_end = input
            .find(|c: char| !tokens::is_ident_char(c))
            .unwrap_or(input.len());
        let name = input[..prefix_end].to_string();
        let rest = &input[prefix_end..];
        let (rest, _) = tokens::punct(':')(rest)?;
        let (rest, iri_str) = iri::full_iri(rest)?;
        ctx.declare_prefix(&name, &iri_str);
        Ok((rest, ()))
    }
}

fn import_decl<'a>(
    ctx: &'a ParserContext,
) -> impl FnMut(&'a str) -> nom::IResult<&'a str, ingress::IriReference> {
    move |input: &'a str| {
        let (input, _) = tokens::keyword("Import:")(input)?;
        let (input, resolved) = iri::iri(ctx)(input)?;
        Ok((input, resolved.0))
    }
}

/// Pre-scan the (post-prefix) document body for `DataProperty:` frame
/// headers, recording each one's resolved IRI in `ctx`. This lets
/// `class_expr::restriction` (and the `EquivalentProperties:`/
/// `DisjointProperties:` misc axioms) disambiguate an object- vs
/// data-property restriction/list *before* encountering the property's own
/// frame, regardless of frame order in the document. See `class_expr.rs`'s
/// module docs for why this disambiguation is needed at all.
///
/// This is a plain substring scan, not a structural parse: a `DataProperty:`
/// occurring inside a quoted string or IRI (never valid in practice) would
/// be misread. Acceptable given this parser's documented scope.
fn prescan_data_properties(ctx: &ParserContext, input: &str) {
    const KW: &str = "DataProperty:";
    for (idx, _) in input.match_indices(KW) {
        let after = &input[idx + KW.len()..];
        // Token parsers in this crate assume the *previous* token already
        // consumed trailing whitespace (see `tokens::tok`); this ad-hoc scan
        // bypasses that chain, so skip leading whitespace/comments here.
        if let Ok((after, ())) = tokens::sp(after)
            && let Ok((_, resolved)) = iri::iri(ctx)(after)
        {
            ctx.mark_data_property(&(resolved.0).0);
        }
    }
}

/// Parse a Manchester Syntax `ontologyDocument` and produce an
/// [`owl_ontology::Ontology`].
pub fn parse(input: &str) -> Result<Ontology, String> {
    let ctx = ParserContext::new();

    let (input, ()) = tokens::sp(input).map_err(fail)?;
    let (input, _prefixes) = many0(prefix_declaration(&ctx))(input).map_err(fail)?;

    prescan_data_properties(&ctx, input);

    let (input, _) = tokens::keyword("Ontology:")(input).map_err(fail)?;
    let (input, ontology_iri) = nom::combinator::opt(iri::full_iri)(input).map_err(fail)?;
    let (input, version_iri) = if ontology_iri.is_some() {
        nom::combinator::opt(iri::full_iri)(input).map_err(fail)?
    } else {
        (input, None)
    };

    let (input, imports) = many0(import_decl(&ctx))(input).map_err(fail)?;

    let (input, annotation_sections) =
        many0(annotation::annotations_section(&ctx))(input).map_err(fail)?;
    let ontology_annotations = annotation_sections.into_iter().flatten().collect();

    let (input, frame_axioms) = many0(frame::any_frame(&ctx))(input).map_err(fail)?;
    let axioms = frame_axioms.into_iter().flatten().collect();

    let (input, ()) = tokens::sp(input).map_err(fail)?;
    if !input.is_empty() {
        let preview: String = input.chars().take(80).collect();
        return Err(format!(
            "Manchester syntax parse error: unrecognized input at: {preview:?}"
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
    format!("Manchester syntax parse error: {e:?}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_unnamed_empty_ontology() {
        let onto = parse("Ontology:").unwrap();
        assert_eq!(onto.version, ingress::OntologyVersion::UnNamedOntology);
        assert!(onto.axioms.is_empty());
    }
}

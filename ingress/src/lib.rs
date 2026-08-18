/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/
//! Core RDF type hierarchy (`IriReference`, `RdfResource`, `RdfLiteral`, `GraphElement`)
//! and vocabulary constants, shared by all crates in the workspace.
#![warn(missing_docs)]
use chrono::{DateTime, Duration, NaiveDate, NaiveTime, Utc};
use num_bigint::BigInt;
use ordered_float::OrderedFloat;
use rust_decimal::Decimal;
use std::fmt;

/// A resolved (or unresolved) IRI, stored as a plain string.
#[derive(Debug, Clone, PartialEq, PartialOrd, Eq, Ord, Hash)]
pub struct IriReference(pub String);

impl fmt::Display for IriReference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// RDF/RDFS/OWL/XSD namespace IRI constants.
mod namespaces;
pub use namespaces::*;

/// Gating policy for operations that require an outbound network fetch.
mod network_policy;
pub use network_policy::NetworkPolicy;

/// An RDF resource: a named IRI node or an anonymous blank node.
#[derive(Debug, Clone, PartialEq, PartialOrd, Eq, Ord, Hash)]
pub enum RdfResource {
    /// A named resource identified by an IRI.
    Iri(IriReference),
    /// A blank node identified by its assigned numeric ID.
    AnonymousBlankNode(u32),
}

impl fmt::Display for RdfResource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RdfResource::Iri(iri) => {
                if iri.0 == RDF_TYPE {
                    write!(f, "a")
                } else {
                    write!(f, "<{}>", iri)
                }
            }
            RdfResource::AnonymousBlankNode(id) => write!(f, "_:({})", id),
        }
    }
}

/// An RDF literal value, one variant per supported datatype.
#[derive(Debug, Clone, PartialEq, PartialOrd, Eq, Ord, Hash)]
pub enum RdfLiteral {
    /// A plain string with no datatype or language tag.
    LiteralString(String),
    /// An `xsd:boolean` value.
    BooleanLiteral(bool),
    /// An `xsd:decimal` value.
    DecimalLiteral(Decimal),
    /// An `xsd:float` value.
    FloatLiteral(OrderedFloat<f64>),
    /// An `xsd:double` value.
    DoubleLiteral(OrderedFloat<f64>),
    /// An `xsd:duration` value.
    DurationLiteral(Duration),
    /// An `xsd:integer` value.
    IntegerLiteral(BigInt),
    /// An `xsd:dateTime` value.
    DateTimeLiteral(DateTime<Utc>),
    /// An `xsd:time` value.
    TimeLiteral(NaiveTime),
    /// An `xsd:date` value.
    DateLiteral(NaiveDate),
    /// A language-tagged string (`rdf:langString`).
    LangLiteral {
        /// The BCP 47 language tag.
        lang: String,
        /// The literal's lexical value.
        literal: String,
    },
    /// A literal with an explicit, non-built-in datatype IRI.
    TypedLiteral {
        /// The literal's datatype IRI.
        type_iri: IriReference,
        /// The literal's lexical value.
        literal: String,
    },
}

impl fmt::Display for RdfLiteral {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RdfLiteral::LiteralString(s) => write!(f, "({})", s),
            RdfLiteral::BooleanLiteral(b) => write!(f, "({})", b),
            RdfLiteral::DecimalLiteral(d) => write!(f, "DecimalLiteral({})", d),
            RdfLiteral::FloatLiteral(fl) => write!(f, "FloatLiteral({})", fl),
            RdfLiteral::DoubleLiteral(d) => write!(f, "DoubleLiteral({})", d),
            RdfLiteral::DurationLiteral(dur) => write!(f, "DurationLiteral({:?})", dur),
            RdfLiteral::IntegerLiteral(i) => write!(f, "IntegerLiteral({})", i),
            RdfLiteral::DateTimeLiteral(dt) => write!(f, "DateTimeLiteral({:?})", dt),
            RdfLiteral::TimeLiteral(t) => write!(f, "TimeLiteral({:?})", t),
            RdfLiteral::DateLiteral(d) => write!(f, "DateLiteral({:?})", d),
            RdfLiteral::LangLiteral { lang, literal } => write!(f, "{}@{}", lang, literal),
            RdfLiteral::TypedLiteral { type_iri, literal } => {
                write!(f, "{}^^{}", literal, type_iri)
            }
        }
    }
}

/// Identifies an RDF 1.2 embedded triple ("triple term") by its three interned
/// component IDs.  Each field is a `GraphElementId` (= `u32`) assigned by the
/// `GraphElementManager` in the `dag_rdf` crate.
///
/// Defined here rather than in `dag_rdf` so that `GraphElement::TripleTerm` can
/// carry it without introducing a circular dependency.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct TripleTermKey {
    /// Interned `GraphElementId` of the embedded triple's subject.
    pub subject: u32,
    /// Interned `GraphElementId` of the embedded triple's predicate.
    pub predicate: u32,
    /// Interned `GraphElementId` of the embedded triple's object.
    pub obj: u32,
}

/// A value that can be interned and assigned a `GraphElementId`: a resource, a literal, or a triple term.
#[derive(Debug, Clone, PartialEq, PartialOrd, Eq, Ord, Hash)]
pub enum GraphElement {
    /// A named or blank-node RDF resource.
    NodeOrEdge(RdfResource),
    /// An RDF literal.
    GraphLiteral(RdfLiteral),
    /// RDF 1.2 embedded triple (triple term): `<<( subject predicate object )>>`.
    ///
    /// The payload is a [`TripleTermKey`] whose fields are interned
    /// `GraphElementId` values.  Use `Datastore::add_triple_term` in `dag_rdf`
    /// to intern a triple term and obtain its `GraphElementId`.
    ///
    /// Serialisation and reasoning support is tracked in
    /// [#143](https://github.com/daghovland/rdf-datalog/issues/143).
    TripleTerm(TripleTermKey),
}

impl fmt::Display for GraphElement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GraphElement::NodeOrEdge(r) => write!(f, "{}", r),
            GraphElement::GraphLiteral(l) => write!(f, "{}", l),
            // Display the interned IDs; a richer representation requires access
            // to the Datastore and is left for full RDF 1.2 support (#143).
            GraphElement::TripleTerm(k) => {
                write!(f, "<<( {} {} {} )>>", k.subject, k.predicate, k.obj)
            }
        }
    }
}

/// A namespace prefix declaration, e.g. `PREFIX ex: <http://example.org/>`.
#[derive(Debug, Clone, PartialEq, PartialOrd, Eq, Ord, Hash)]
pub enum PrefixDeclaration {
    /// Maps a short prefix `name` to a full `iri`.
    PrefixDefinition {
        /// The prefix's short name (without the trailing colon).
        name: String,
        /// The IRI the prefix expands to.
        iri: IriReference,
    },
}

impl PrefixDeclaration {
    /// Returns the prefix's short name and its expanded IRI.
    pub fn try_get_prefix_name(&self) -> (&str, &IriReference) {
        match self {
            PrefixDeclaration::PrefixDefinition { name, iri } => (name, iri),
        }
    }
}

/// The identity of an ontology: unnamed, named, or named with a version IRI.
#[derive(Debug, Clone, PartialEq, PartialOrd, Eq, Ord, Hash)]
pub enum OntologyVersion {
    /// No ontology IRI was declared.
    UnNamedOntology,
    /// An ontology IRI with no separate version IRI.
    NamedOntology(IriReference),
    /// An ontology IRI paired with a distinct version IRI.
    VersionedOntology {
        /// The ontology's IRI.
        ontology_iri: IriReference,
        /// The IRI of this specific version of the ontology.
        version_iri: IriReference,
    },
}

impl OntologyVersion {
    /// Returns the version IRI, if this ontology declares one.
    pub fn try_get_ontology_version_iri(&self) -> Option<&IriReference> {
        match self {
            OntologyVersion::NamedOntology(_) => None,
            OntologyVersion::VersionedOntology { version_iri, .. } => Some(version_iri),
            _ => None,
        }
    }

    /// Returns the ontology IRI, if this ontology is named.
    pub fn try_get_ontology_iri(&self) -> Option<&IriReference> {
        match self {
            OntologyVersion::NamedOntology(iri) => Some(iri),
            OntologyVersion::VersionedOntology { ontology_iri, .. } => Some(ontology_iri),
            _ => None,
        }
    }
}

/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/
use chrono::{DateTime, Duration, NaiveDate, NaiveTime, Utc};
use num_bigint::BigInt;
use ordered_float::OrderedFloat;
use rust_decimal::Decimal;
use std::fmt;

#[derive(Debug, Clone, PartialEq, PartialOrd, Eq, Ord, Hash)]
pub struct IriReference(pub String);

impl fmt::Display for IriReference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

mod namespaces;
pub use namespaces::*;

mod network_policy;
pub use network_policy::NetworkPolicy;

#[derive(Debug, Clone, PartialEq, PartialOrd, Eq, Ord, Hash)]
pub enum RdfResource {
    Iri(IriReference),
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

#[derive(Debug, Clone, PartialEq, PartialOrd, Eq, Ord, Hash)]
pub enum RdfLiteral {
    LiteralString(String),
    BooleanLiteral(bool),
    DecimalLiteral(Decimal),
    FloatLiteral(OrderedFloat<f64>),
    DoubleLiteral(OrderedFloat<f64>),
    DurationLiteral(Duration),
    IntegerLiteral(BigInt),
    DateTimeLiteral(DateTime<Utc>),
    TimeLiteral(NaiveTime),
    DateLiteral(NaiveDate),
    LangLiteral {
        lang: String,
        literal: String,
    },
    TypedLiteral {
        type_iri: IriReference,
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
    pub subject: u32,
    pub predicate: u32,
    pub obj: u32,
}

#[derive(Debug, Clone, PartialEq, PartialOrd, Eq, Ord, Hash)]
pub enum GraphElement {
    NodeOrEdge(RdfResource),
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

#[derive(Debug, Clone, PartialEq, PartialOrd, Eq, Ord, Hash)]
pub enum PrefixDeclaration {
    PrefixDefinition { name: String, iri: IriReference },
}

impl PrefixDeclaration {
    pub fn try_get_prefix_name(&self) -> (&str, &IriReference) {
        match self {
            PrefixDeclaration::PrefixDefinition { name, iri } => (name, iri),
        }
    }
}

#[derive(Debug, Clone, PartialEq, PartialOrd, Eq, Ord, Hash)]
pub enum OntologyVersion {
    UnNamedOntology,
    NamedOntology(IriReference),
    VersionedOntology {
        ontology_iri: IriReference,
        version_iri: IriReference,
    },
}

impl OntologyVersion {
    pub fn try_get_ontology_version_iri(&self) -> Option<&IriReference> {
        match self {
            OntologyVersion::NamedOntology(_) => None,
            OntologyVersion::VersionedOntology { version_iri, .. } => Some(version_iri),
            _ => None,
        }
    }

    pub fn try_get_ontology_iri(&self) -> Option<&IriReference> {
        match self {
            OntologyVersion::NamedOntology(iri) => Some(iri),
            OntologyVersion::VersionedOntology { ontology_iri, .. } => Some(ontology_iri),
            _ => None,
        }
    }
}

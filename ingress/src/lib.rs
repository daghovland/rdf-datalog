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

#[derive(Debug, Clone, PartialEq, PartialOrd, Eq, Ord, Hash)]
pub enum GraphElement {
    NodeOrEdge(RdfResource),
    GraphLiteral(RdfLiteral),
}

impl fmt::Display for GraphElement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GraphElement::NodeOrEdge(r) => write!(f, "{}", r),
            GraphElement::GraphLiteral(l) => write!(f, "{}", l),
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

/// Reconstruct an `RdfLiteral` from a lexical value and a datatype IRI.
///
/// Returns the *specific* variant that the Turtle parser would produce for
/// well-known XSD types (integer, boolean, decimal, float, double, date, time,
/// dateTime).  Unknown datatypes fall back to `TypedLiteral`.
///
/// This is used by the persistence replay path so that replayed quads are
/// structurally equal to freshly-parsed quads and remain queryable.
pub fn rdf_literal_from_typed(lexical: &str, datatype: &str) -> RdfLiteral {
    use crate::namespaces::{
        XSD_BOOLEAN, XSD_DATE, XSD_DATE_TIME, XSD_DECIMAL, XSD_DOUBLE, XSD_FLOAT, XSD_INT,
        XSD_INTEGER, XSD_NON_NEGATIVE_INTEGER, XSD_TIME,
    };
    match datatype {
        XSD_INTEGER | XSD_INT | XSD_NON_NEGATIVE_INTEGER => {
            if let Ok(n) = lexical.parse::<BigInt>() {
                return RdfLiteral::IntegerLiteral(n);
            }
        }
        XSD_BOOLEAN => match lexical {
            "true" | "1" => return RdfLiteral::BooleanLiteral(true),
            "false" | "0" => return RdfLiteral::BooleanLiteral(false),
            _ => {}
        },
        XSD_DOUBLE => {
            if let Ok(f) = lexical.parse::<f64>() {
                return RdfLiteral::DoubleLiteral(OrderedFloat(f));
            }
        }
        XSD_FLOAT => {
            if let Ok(f) = lexical.parse::<f64>() {
                return RdfLiteral::FloatLiteral(OrderedFloat(f));
            }
        }
        XSD_DECIMAL => {
            if let Ok(d) = lexical.parse::<Decimal>() {
                return RdfLiteral::DecimalLiteral(d);
            }
        }
        XSD_DATE_TIME => {
            if let Ok(dt) = lexical.parse::<DateTime<Utc>>() {
                return RdfLiteral::DateTimeLiteral(dt);
            }
        }
        XSD_DATE => {
            if let Ok(d) = lexical.parse::<NaiveDate>() {
                return RdfLiteral::DateLiteral(d);
            }
        }
        XSD_TIME => {
            if let Ok(t) = lexical.parse::<NaiveTime>() {
                return RdfLiteral::TimeLiteral(t);
            }
        }
        _ => {}
    }
    RdfLiteral::TypedLiteral {
        literal: lexical.to_owned(),
        type_iri: IriReference(datatype.to_owned()),
    }
}

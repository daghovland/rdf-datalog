/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/
pub(crate) use ::ingress::{
    GraphElement, RdfLiteral, XSD_BOOLEAN, XSD_INT, XSD_INTEGER, XSD_NON_NEGATIVE_INTEGER,
};
pub use ::ingress::{IriReference, RdfResource};
use num_bigint::BigInt;
use std::fmt;

pub type GraphElementId = u32;
pub type TripleListIndex = usize;
pub type QuadListIndex = usize;

/// The IRI used for the default (unnamed) graph, matching DagSemTools convention.
pub const DEFAULT_GRAPH_IRI: &str = "urn:x-arq:DefaultGraph";

/// The element ID reserved for the default graph. Always 0, pre-populated in GraphElementManager.
pub const DEFAULT_GRAPH_ELEMENT_ID: GraphElementId = 0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Triple {
    pub subject: GraphElementId,
    pub predicate: GraphElementId,
    pub obj: GraphElementId,
}

impl fmt::Display for Triple {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({}, {}, {})", self.subject, self.predicate, self.obj)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Quad {
    pub triple_id: GraphElementId,
    pub subject: GraphElementId,
    pub predicate: GraphElementId,
    pub obj: GraphElementId,
}

impl Quad {
    pub fn get_triple(&self) -> Triple {
        Triple {
            subject: self.subject,
            predicate: self.predicate,
            obj: self.obj,
        }
    }
}

impl fmt::Display for Quad {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: ({}, {}, {})",
            self.triple_id, self.subject, self.predicate, self.obj
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TripleResource {
    pub subject: GraphElement,
    pub predicate: GraphElement,
    pub obj: GraphElement,
}

impl fmt::Display for TripleResource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({}, {}, {})", self.subject, self.predicate, self.obj)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct QuadResource {
    pub triple_id: GraphElement,
    pub subject: GraphElement,
    pub predicate: GraphElement,
    pub obj: GraphElement,
}

impl QuadResource {
    pub fn get_triple_resource(&self) -> TripleResource {
        TripleResource {
            subject: self.subject.clone(),
            predicate: self.predicate.clone(),
            obj: self.obj.clone(),
        }
    }
}

impl fmt::Display for QuadResource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: ({}, {}, {})",
            self.triple_id, self.subject, self.predicate, self.obj
        )
    }
}

/// Assumes the resource is some integer literal which fits in a BigInt, and extracts it if that is the case.
pub fn try_get_non_negative_integer_literal(gel: &GraphElement) -> Option<BigInt> {
    match gel {
        GraphElement::NodeOrEdge(_) => None,
        GraphElement::GraphLiteral(res) => match res {
            RdfLiteral::IntegerLiteral(nn) => Some(nn.clone()),
            RdfLiteral::TypedLiteral { type_iri, literal } => {
                let tp = type_iri.to_string();
                if [
                    XSD_INT.to_string(),
                    XSD_INTEGER.to_string(),
                    XSD_NON_NEGATIVE_INTEGER.to_string(),
                ]
                .contains(&tp)
                {
                    literal.parse::<BigInt>().ok()
                } else {
                    None
                }
            }
            _ => None,
        },
    }
}

/// Assumes the resource is some boolean literal, and extracts it if that is the case.
pub fn try_get_bool_literal(gel: &GraphElement) -> Option<bool> {
    match gel {
        GraphElement::NodeOrEdge(_) => None,
        GraphElement::GraphLiteral(res) => match res {
            RdfLiteral::BooleanLiteral(nn) => Some(*nn),
            RdfLiteral::TypedLiteral { type_iri, literal } => {
                if type_iri.to_string() == XSD_BOOLEAN {
                    match literal.as_str() {
                        "true" => Some(true),
                        "false" => Some(false),
                        _ => panic!("Invalid use of xsd:boolean on value {}", literal),
                    }
                } else {
                    None
                }
            }
            _ => None,
        },
    }
}

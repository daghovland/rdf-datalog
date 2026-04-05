/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! ELI axiom types.
//!
//! See <https://www.emse.fr/~zimmermann/Teaching/KRR/el.html> and
//! <https://arxiv.org/abs/2008.02232>.

use owl_ontology::{Class, Individual, ObjectPropertyExpression};

/// The EL(I) fragment of class expressions — used in sub-concept positions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComplexConcept {
    Top,
    AtomicConcept(Class),
    Intersection(Vec<ComplexConcept>),
    SomeValuesFrom(ObjectPropertyExpression, Box<ComplexConcept>),
}

/// A normalized concept used in super-concept positions in NormalizedConceptInclusion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NormalizedConcept {
    Bottom,
    AtomicNamedConcept(Class),
    AtomicAnonymousConcept,
    ObjectHasValue(ObjectPropertyExpression, Individual),
    AllValuesFrom(ObjectPropertyExpression, Class),
    AtMostOneValueFrom(ObjectPropertyExpression),
}

/// A normalized ELI formula used by the RL translation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Formula {
    /// `U C_i <= /\ A_i` — a disjunction of ELI-concepts is a subclass of a
    /// conjunction of atomic concepts.
    DirectlyTranslatableConceptInclusion {
        subclass_disjunction: Vec<ComplexConcept>,
        superclass_conjunction: Vec<Class>,
    },
    /// `/\ A_i <= C` — a conjunction of atomic concepts is a subclass of a
    /// (possibly complex) normalized concept.
    NormalizedConceptInclusion {
        subclass_conjunction: Vec<Class>,
        superclass: NormalizedConcept,
    },
}

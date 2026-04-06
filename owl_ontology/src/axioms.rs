/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Rust representation of OWL 2, following <https://www.w3.org/TR/2012/REC-owl2-syntax-20121211>.

use ingress::{GraphElement, IriReference};
use num_bigint::BigInt;

/// A fully-qualified IRI (the only IRI form used in OWL 2 functional syntax).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FullIri(pub IriReference);

pub type Iri = FullIri;
pub type AnnotationProperty = Iri;
pub type ObjectProperty = Iri;
pub type DataProperty = Iri;
pub type Datatype = Iri;
pub type Class = Iri;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Individual {
    NamedIndividual(Iri),
    AnonymousIndividual(u32),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AnnotationValue {
    IndividualAnnotation(Individual),
    LiteralAnnotation(GraphElement),
    IriAnnotation(Iri),
}

pub type Annotation = (AnnotationProperty, AnnotationValue);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AnnotationAxiom {
    AnnotationAssertion(
        Vec<Annotation>,
        AnnotationProperty,
        GraphElement,
        GraphElement,
    ),
    SubAnnotationPropertyOf(Vec<Annotation>, AnnotationProperty, AnnotationProperty),
    AnnotationPropertyDomain(Vec<Annotation>, AnnotationProperty, Iri),
    AnnotationPropertyRange(Vec<Annotation>, AnnotationProperty, Iri),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DataRange {
    NamedDataRange(Datatype),
    DataIntersectionOf(Vec<DataRange>),
    DataUnionOf(Vec<DataRange>),
    DataComplementOf(Box<DataRange>),
    DataOneOf(Vec<GraphElement>),
    DatatypeRestriction(Datatype, Vec<(DataProperty, GraphElement)>),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ObjectPropertyExpression {
    NamedObjectProperty(ObjectProperty),
    AnonymousObjectProperty(u32),
    InverseObjectProperty(Box<ObjectPropertyExpression>),
    ObjectPropertyChain(Vec<ObjectPropertyExpression>),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SubPropertyExpression {
    SubObjectPropertyExpression(ObjectPropertyExpression),
    PropertyExpressionChain(Vec<ObjectPropertyExpression>),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ClassExpression {
    ClassName(Class),
    AnonymousClass(u32),
    ObjectUnionOf(Vec<ClassExpression>),
    ObjectIntersectionOf(Vec<ClassExpression>),
    ObjectComplementOf(Box<ClassExpression>),
    ObjectOneOf(Vec<Individual>),
    ObjectSomeValuesFrom(ObjectPropertyExpression, Box<ClassExpression>),
    ObjectAllValuesFrom(ObjectPropertyExpression, Box<ClassExpression>),
    ObjectHasValue(ObjectPropertyExpression, Individual),
    ObjectHasSelf(ObjectPropertyExpression),
    ObjectMinQualifiedCardinality(BigInt, ObjectPropertyExpression, Box<ClassExpression>),
    ObjectMaxQualifiedCardinality(BigInt, ObjectPropertyExpression, Box<ClassExpression>),
    ObjectExactQualifiedCardinality(BigInt, ObjectPropertyExpression, Box<ClassExpression>),
    ObjectExactCardinality(BigInt, ObjectPropertyExpression),
    ObjectMinCardinality(BigInt, ObjectPropertyExpression),
    ObjectMaxCardinality(BigInt, ObjectPropertyExpression),
    DataSomeValuesFrom(Vec<DataProperty>, DataRange),
    DataAllValuesFrom(Vec<DataProperty>, DataRange),
    DataHasValue(DataProperty, GraphElement),
    DataMinQualifiedCardinality(BigInt, DataProperty, DataRange),
    DataMaxQualifiedCardinality(BigInt, DataProperty, DataRange),
    DataExactQualifiedCardinality(BigInt, DataProperty, DataRange),
    DataMinCardinality(BigInt, DataProperty),
    DataMaxCardinality(BigInt, DataProperty),
    DataExactCardinality(BigInt, DataProperty),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ObjectPropertyAxiom {
    ObjectPropertyDomain(ObjectPropertyExpression, ClassExpression),
    ObjectPropertyRange(ObjectPropertyExpression, ClassExpression),
    SubObjectPropertyOf(
        Vec<Annotation>,
        SubPropertyExpression,
        ObjectPropertyExpression,
    ),
    EquivalentObjectProperties(Vec<Annotation>, Vec<ObjectPropertyExpression>),
    DisjointObjectProperties(Vec<Annotation>, Vec<ObjectPropertyExpression>),
    InverseObjectProperties(
        Vec<Annotation>,
        ObjectPropertyExpression,
        ObjectPropertyExpression,
    ),
    FunctionalObjectProperty(Vec<Annotation>, ObjectPropertyExpression),
    InverseFunctionalObjectProperty(Vec<Annotation>, ObjectPropertyExpression),
    ReflexiveObjectProperty(Vec<Annotation>, ObjectPropertyExpression),
    IrreflexiveObjectProperty(Vec<Annotation>, ObjectPropertyExpression),
    SymmetricObjectProperty(Vec<Annotation>, ObjectPropertyExpression),
    AsymmetricObjectProperty(Vec<Annotation>, ObjectPropertyExpression),
    TransitiveObjectProperty(Vec<Annotation>, ObjectPropertyExpression),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DataPropertyAxiom {
    SubDataPropertyOf(Vec<Annotation>, DataProperty, DataProperty),
    EquivalentDataProperties(Vec<Annotation>, Vec<DataProperty>),
    DisjointDataProperties(Vec<Annotation>, Vec<DataProperty>),
    DataPropertyDomain(Vec<Annotation>, DataProperty, ClassExpression),
    DataPropertyRange(Vec<Annotation>, DataProperty, DataRange),
    FunctionalDataProperty(Vec<Annotation>, DataProperty),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ClassAxiom {
    SubClassOf(Vec<Annotation>, ClassExpression, ClassExpression),
    EquivalentClasses(Vec<Annotation>, Vec<ClassExpression>),
    DisjointClasses(Vec<Annotation>, Vec<ClassExpression>),
    DisjointUnion(Vec<Annotation>, Class, Vec<ClassExpression>),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Assertion {
    SameIndividual(Vec<Annotation>, Vec<Individual>),
    DifferentIndividuals(Vec<Annotation>, Vec<Individual>),
    ClassAssertion(Vec<Annotation>, ClassExpression, Individual),
    ObjectPropertyAssertion(
        Vec<Annotation>,
        ObjectPropertyExpression,
        Individual,
        Individual,
    ),
    NegativeObjectPropertyAssertion(
        Vec<Annotation>,
        ObjectPropertyExpression,
        Individual,
        Individual,
    ),
    DataPropertyAssertion(Vec<Annotation>, DataProperty, Individual, GraphElement),
    NegativeDataPropertyAssertion(Vec<Annotation>, DataProperty, Individual, GraphElement),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Entity {
    ClassDeclaration(Class),
    ObjectPropertyDeclaration(ObjectProperty),
    DataPropertyDeclaration(DataProperty),
    DatatypeDeclaration(Datatype),
    AnnotationPropertyDeclaration(AnnotationProperty),
    NamedIndividualDeclaration(Individual),
}

pub type Declaration = (Vec<Annotation>, Entity);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Axiom {
    AxiomDeclaration(Declaration),
    AxiomClassAxiom(ClassAxiom),
    AxiomObjectPropertyAxiom(ObjectPropertyAxiom),
    AxiomDataPropertyAxiom(DataPropertyAxiom),
    AxiomDatatypeDefinition(Vec<Annotation>, Datatype, DataRange),
    AxiomHasKey(
        Vec<Annotation>,
        ClassExpression,
        Vec<ObjectPropertyExpression>,
        Vec<DataProperty>,
    ),
    AxiomAssertion(Assertion),
    AxiomAnnotationAxiom(AnnotationAxiom),
}

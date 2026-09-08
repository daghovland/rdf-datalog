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

/// Alias for `FullIri`, matching OWL 2 functional-syntax terminology.
pub type Iri = FullIri;
/// An IRI used as an annotation property.
pub type AnnotationProperty = Iri;
/// An IRI used as an object property.
pub type ObjectProperty = Iri;
/// An IRI used as a data property.
pub type DataProperty = Iri;
/// An IRI used as a datatype.
pub type Datatype = Iri;
/// An IRI used as a class.
pub type Class = Iri;

/// A named or anonymous OWL individual.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Individual {
    /// An individual identified by an IRI.
    NamedIndividual(Iri),
    /// An individual identified only by an anonymous numeric ID (blank node).
    AnonymousIndividual(u32),
}

/// The value side of an `Annotation`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AnnotationValue {
    /// The annotation value is an individual.
    IndividualAnnotation(Individual),
    /// The annotation value is a literal.
    LiteralAnnotation(GraphElement),
    /// The annotation value is an IRI.
    IriAnnotation(Iri),
}

/// An annotation: a property paired with its value.
pub type Annotation = (AnnotationProperty, AnnotationValue);

/// Axioms about annotation properties and assertions.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AnnotationAxiom {
    /// Asserts an annotation `(subject, property, value)`, with its own annotations.
    AnnotationAssertion(
        Vec<Annotation>,
        AnnotationProperty,
        GraphElement,
        GraphElement,
    ),
    /// One annotation property is a sub-property of another.
    SubAnnotationPropertyOf(Vec<Annotation>, AnnotationProperty, AnnotationProperty),
    /// Declares the domain of an annotation property.
    AnnotationPropertyDomain(Vec<Annotation>, AnnotationProperty, Iri),
    /// Declares the range of an annotation property.
    AnnotationPropertyRange(Vec<Annotation>, AnnotationProperty, Iri),
}

/// An OWL 2 data range expression.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DataRange {
    /// A named datatype.
    NamedDataRange(Datatype),
    /// Intersection of data ranges.
    DataIntersectionOf(Vec<DataRange>),
    /// Union of data ranges.
    DataUnionOf(Vec<DataRange>),
    /// Complement of a data range.
    DataComplementOf(Box<DataRange>),
    /// An enumeration of literal values.
    DataOneOf(Vec<GraphElement>),
    /// A datatype restricted by facet/value pairs.
    DatatypeRestriction(Datatype, Vec<(DataProperty, GraphElement)>),
}

/// An OWL 2 object property expression.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ObjectPropertyExpression {
    /// A named object property.
    NamedObjectProperty(ObjectProperty),
    /// An anonymous object property, identified by a numeric ID.
    AnonymousObjectProperty(u32),
    /// The inverse of an object property expression.
    InverseObjectProperty(Box<ObjectPropertyExpression>),
    /// A chain of object property expressions (property chain axiom).
    ObjectPropertyChain(Vec<ObjectPropertyExpression>),
}

/// The left-hand side of a `SubObjectPropertyOf` axiom: a single expression or a chain.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SubPropertyExpression {
    /// A single sub-property expression.
    SubObjectPropertyExpression(ObjectPropertyExpression),
    /// A property expression chain.
    PropertyExpressionChain(Vec<ObjectPropertyExpression>),
}

/// An OWL 2 class expression.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ClassExpression {
    /// A named class.
    ClassName(Class),
    /// An anonymous class, identified by a numeric ID.
    AnonymousClass(u32),
    /// Union of class expressions.
    ObjectUnionOf(Vec<ClassExpression>),
    /// Intersection of class expressions.
    ObjectIntersectionOf(Vec<ClassExpression>),
    /// Complement of a class expression.
    ObjectComplementOf(Box<ClassExpression>),
    /// An enumeration of individuals.
    ObjectOneOf(Vec<Individual>),
    /// Existential restriction on an object property.
    ObjectSomeValuesFrom(ObjectPropertyExpression, Box<ClassExpression>),
    /// Universal restriction on an object property.
    ObjectAllValuesFrom(ObjectPropertyExpression, Box<ClassExpression>),
    /// Has-value restriction on an object property.
    ObjectHasValue(ObjectPropertyExpression, Individual),
    /// Self restriction on an object property.
    ObjectHasSelf(ObjectPropertyExpression),
    /// Qualified minimum cardinality restriction on an object property.
    ObjectMinQualifiedCardinality(BigInt, ObjectPropertyExpression, Box<ClassExpression>),
    /// Qualified maximum cardinality restriction on an object property.
    ObjectMaxQualifiedCardinality(BigInt, ObjectPropertyExpression, Box<ClassExpression>),
    /// Qualified exact cardinality restriction on an object property.
    ObjectExactQualifiedCardinality(BigInt, ObjectPropertyExpression, Box<ClassExpression>),
    /// Unqualified exact cardinality restriction on an object property.
    ObjectExactCardinality(BigInt, ObjectPropertyExpression),
    /// Unqualified minimum cardinality restriction on an object property.
    ObjectMinCardinality(BigInt, ObjectPropertyExpression),
    /// Unqualified maximum cardinality restriction on an object property.
    ObjectMaxCardinality(BigInt, ObjectPropertyExpression),
    /// Existential restriction on data properties.
    DataSomeValuesFrom(Vec<DataProperty>, DataRange),
    /// Universal restriction on data properties.
    DataAllValuesFrom(Vec<DataProperty>, DataRange),
    /// Has-value restriction on a data property.
    DataHasValue(DataProperty, GraphElement),
    /// Qualified minimum cardinality restriction on a data property.
    DataMinQualifiedCardinality(BigInt, DataProperty, DataRange),
    /// Qualified maximum cardinality restriction on a data property.
    DataMaxQualifiedCardinality(BigInt, DataProperty, DataRange),
    /// Qualified exact cardinality restriction on a data property.
    DataExactQualifiedCardinality(BigInt, DataProperty, DataRange),
    /// Unqualified minimum cardinality restriction on a data property.
    DataMinCardinality(BigInt, DataProperty),
    /// Unqualified maximum cardinality restriction on a data property.
    DataMaxCardinality(BigInt, DataProperty),
    /// Unqualified exact cardinality restriction on a data property.
    DataExactCardinality(BigInt, DataProperty),
}

/// Axioms about object properties.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ObjectPropertyAxiom {
    /// Declares the domain of an object property.
    ObjectPropertyDomain(ObjectPropertyExpression, ClassExpression),
    /// Declares the range of an object property.
    ObjectPropertyRange(ObjectPropertyExpression, ClassExpression),
    /// One object property expression is a sub-property of another.
    SubObjectPropertyOf(
        Vec<Annotation>,
        SubPropertyExpression,
        ObjectPropertyExpression,
    ),
    /// A set of object properties are pairwise equivalent.
    EquivalentObjectProperties(Vec<Annotation>, Vec<ObjectPropertyExpression>),
    /// A set of object properties are pairwise disjoint.
    DisjointObjectProperties(Vec<Annotation>, Vec<ObjectPropertyExpression>),
    /// Two object properties are inverses of each other.
    InverseObjectProperties(
        Vec<Annotation>,
        ObjectPropertyExpression,
        ObjectPropertyExpression,
    ),
    /// The object property is functional.
    FunctionalObjectProperty(Vec<Annotation>, ObjectPropertyExpression),
    /// The object property is inverse-functional.
    InverseFunctionalObjectProperty(Vec<Annotation>, ObjectPropertyExpression),
    /// The object property is reflexive.
    ReflexiveObjectProperty(Vec<Annotation>, ObjectPropertyExpression),
    /// The object property is irreflexive.
    IrreflexiveObjectProperty(Vec<Annotation>, ObjectPropertyExpression),
    /// The object property is symmetric.
    SymmetricObjectProperty(Vec<Annotation>, ObjectPropertyExpression),
    /// The object property is asymmetric.
    AsymmetricObjectProperty(Vec<Annotation>, ObjectPropertyExpression),
    /// The object property is transitive.
    TransitiveObjectProperty(Vec<Annotation>, ObjectPropertyExpression),
}

/// Axioms about data properties.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DataPropertyAxiom {
    /// One data property is a sub-property of another.
    SubDataPropertyOf(Vec<Annotation>, DataProperty, DataProperty),
    /// A set of data properties are pairwise equivalent.
    EquivalentDataProperties(Vec<Annotation>, Vec<DataProperty>),
    /// A set of data properties are pairwise disjoint.
    DisjointDataProperties(Vec<Annotation>, Vec<DataProperty>),
    /// Declares the domain of a data property.
    DataPropertyDomain(Vec<Annotation>, DataProperty, ClassExpression),
    /// Declares the range of a data property.
    DataPropertyRange(Vec<Annotation>, DataProperty, DataRange),
    /// The data property is functional.
    FunctionalDataProperty(Vec<Annotation>, DataProperty),
}

/// Axioms about classes.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ClassAxiom {
    /// One class expression is a subclass of another.
    SubClassOf(Vec<Annotation>, ClassExpression, ClassExpression),
    /// A set of class expressions are pairwise equivalent.
    EquivalentClasses(Vec<Annotation>, Vec<ClassExpression>),
    /// A set of class expressions are pairwise disjoint.
    DisjointClasses(Vec<Annotation>, Vec<ClassExpression>),
    /// A class is the disjoint union of a set of class expressions.
    DisjointUnion(Vec<Annotation>, Class, Vec<ClassExpression>),
}

/// ABox assertions about individuals.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Assertion {
    /// A set of individuals denote the same individual.
    SameIndividual(Vec<Annotation>, Vec<Individual>),
    /// A set of individuals are pairwise distinct.
    DifferentIndividuals(Vec<Annotation>, Vec<Individual>),
    /// An individual is an instance of a class expression.
    ClassAssertion(Vec<Annotation>, ClassExpression, Individual),
    /// An object property relates two individuals.
    ObjectPropertyAssertion(
        Vec<Annotation>,
        ObjectPropertyExpression,
        Individual,
        Individual,
    ),
    /// An object property does not relate two individuals.
    NegativeObjectPropertyAssertion(
        Vec<Annotation>,
        ObjectPropertyExpression,
        Individual,
        Individual,
    ),
    /// A data property relates an individual to a literal.
    DataPropertyAssertion(Vec<Annotation>, DataProperty, Individual, GraphElement),
    /// A data property does not relate an individual to a literal.
    NegativeDataPropertyAssertion(Vec<Annotation>, DataProperty, Individual, GraphElement),
}

/// Declares the type of a named OWL entity.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Entity {
    /// Declares an IRI as a class.
    ClassDeclaration(Class),
    /// Declares an IRI as an object property.
    ObjectPropertyDeclaration(ObjectProperty),
    /// Declares an IRI as a data property.
    DataPropertyDeclaration(DataProperty),
    /// Declares an IRI as a datatype.
    DatatypeDeclaration(Datatype),
    /// Declares an IRI as an annotation property.
    AnnotationPropertyDeclaration(AnnotationProperty),
    /// Declares an individual as named.
    NamedIndividualDeclaration(Individual),
}

/// An entity declaration together with its annotations.
pub type Declaration = (Vec<Annotation>, Entity);

/// An argument to a SWRL [`Atom`]: a variable, a literal, or an individual
/// (named, anonymous, or a datatype/class-name reference used positionally).
///
/// Unlike OWL 2 functional-syntax SWRL atoms, the Manchester-syntax concrete
/// form this crate parses (`predicate(arg, arg, ...)`) can't distinguish an
/// object-property argument from a data-property one without resolving the
/// predicate's declared type, so `Individual` and `Literal` are both always
/// syntactically reachable regardless of the atom's semantic kind.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AtomArg {
    /// A SWRL variable, named without its leading `?` (e.g. `?p` is `"p"`).
    Variable(String),
    /// A literal value.
    Literal(GraphElement),
    /// A named or anonymous individual.
    Individual(Individual),
}

/// A single atom in a SWRL rule's body or head.
///
/// See the [`docs/plans/MANCHESTER_SYNTAX_PLAN.md`](https://github.com/daghovland/rdf-datalog/blob/main/docs/plans/MANCHESTER_SYNTAX_PLAN.md)
/// "`Rule:` SWRL frames" addendum for why there is no arity-1
/// `BuiltInAtom`/data-range-atom variant here: every single-argument atom is
/// parsed as a [`Atom::ClassAtom`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Atom {
    /// `description(arg)` — `arg` is asserted to be an instance of the class
    /// expression.
    ClassAtom(ClassExpression, AtomArg),
    /// `iri(arg1, arg2)` — an object-property atom, a data-property atom, or
    /// a two-argument built-in, indistinguishable without resolving `iri`'s
    /// declared entity type.
    PropertyAtom(Iri, AtomArg, AtomArg),
    /// `iri(arg, ...)` with any arity other than 1 or 2 — a built-in atom
    /// (e.g. a 0- or 3+-argument `swrlb:` predicate).
    BuiltInAtom(Iri, Vec<AtomArg>),
}

/// A SWRL rule: `body -> head`, both conjunctions of [`Atom`]s.
///
/// Held separately from [`Axiom`] on [`crate::Ontology::rules`] rather than
/// as an `Axiom` variant: SWRL is a distinct W3C submission from OWL 2, and
/// `Axiom` is matched exhaustively (without a wildcard) by
/// `owl2rl2datalog::owl_to_rdf`'s RDF-translation walk, which has no
/// corresponding RDF form for a rule to translate into.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SwrlRule {
    /// Annotations on the rule itself (from an optional leading
    /// `Annotations:` section).
    pub annotations: Vec<Annotation>,
    /// The rule's antecedent (left of `->`): a conjunction of atoms.
    pub body: Vec<Atom>,
    /// The rule's consequent (right of `->`): a conjunction of atoms.
    pub head: Vec<Atom>,
}

/// The top-level OWL 2 axiom type, wrapping every axiom category.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Axiom {
    /// An entity declaration.
    AxiomDeclaration(Declaration),
    /// A class axiom.
    AxiomClassAxiom(ClassAxiom),
    /// An object property axiom.
    AxiomObjectPropertyAxiom(ObjectPropertyAxiom),
    /// A data property axiom.
    AxiomDataPropertyAxiom(DataPropertyAxiom),
    /// Defines a datatype in terms of a data range.
    AxiomDatatypeDefinition(Vec<Annotation>, Datatype, DataRange),
    /// A `HasKey` axiom: a class expression keyed by a set of object/data properties.
    AxiomHasKey(
        Vec<Annotation>,
        ClassExpression,
        Vec<ObjectPropertyExpression>,
        Vec<DataProperty>,
    ),
    /// An ABox assertion.
    AxiomAssertion(Assertion),
    /// An annotation axiom.
    AxiomAnnotationAxiom(AnnotationAxiom),
}

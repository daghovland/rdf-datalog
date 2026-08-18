/*
Copyright (C) 2024 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

/// Namespaces and IRIs used in the Turtle language.
/// The rdf namespace
pub const RDF: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";
/// The rdfs namespace.
pub const RDFS: &str = "http://www.w3.org/2000/01/rdf-schema#";
/// The owl namespace.
pub const OWL: &str = "http://www.w3.org/2002/07/owl#";
/// The XML Schema namespace
pub const XSD: &str = "http://www.w3.org/2001/XMLSchema#";

/// The IRI for rdf:type, also abbreviated 'a' in turtle
pub const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
/// The IRI for nil.
pub const RDF_NIL: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#nil";
/// The IRI for first.
pub const RDF_FIRST: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#first";
/// The IRI for rest.
pub const RDF_REST: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#rest";

/// The IRI for reifies.
pub const RDF_REIFIES: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#reifies";
/// The IRI for rdf:langString, the datatype of language-tagged literals (RDF 1.1 §5.5).
pub const RDF_LANG_STRING: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#langString";
/// The IRI for Literal.
pub const RDFS_LITERAL: &str = "http://www.w3.org/2000/01/rdf-schema#Literal";

/// The IRI for subClassOf.
pub const RDFS_SUB_CLASS_OF: &str = "http://www.w3.org/2000/01/rdf-schema#subClassOf";
/// The IRI for subPropertyOf.
pub const RDFS_SUB_PROPERTY_OF: &str = "http://www.w3.org/2000/01/rdf-schema#subPropertyOf";
/// The IRI for Datatype.
pub const RDFS_DATATYPE: &str = "http://www.w3.org/2000/01/rdf-schema#Datatype";
/// The IRI for domain.
pub const RDFS_DOMAIN: &str = "http://www.w3.org/2000/01/rdf-schema#domain";
/// The IRI for range.
pub const RDFS_RANGE: &str = "http://www.w3.org/2000/01/rdf-schema#range";

/// The IRI for sameAs.
pub const OWL_SAME_AS: &str = "http://www.w3.org/2002/07/owl#sameAs";
/// The IRI for differentFrom.
pub const OWL_DIFFERENT_FROM: &str = "http://www.w3.org/2002/07/owl#differentFrom";
/// The IRI for Ontology.
pub const OWL_ONTOLOGY: &str = "http://www.w3.org/2002/07/owl#Ontology";
/// The IRI for imports.
pub const OWL_IMPORT: &str = "http://www.w3.org/2002/07/owl#imports";
/// The IRI for versionIri.
pub const OWL_VERSION_IRI: &str = "http://www.w3.org/2002/07/owl#versionIri";
/// The IRI for OntologyProperty.
pub const OWL_ONTOLOGY_PROPERTY: &str = "http://www.w3.org/2002/07/owl#OntologyProperty";
/// The IRI for AnnotationProperty.
pub const OWL_ANNOTATION_PROPERTY: &str = "http://www.w3.org/2002/07/owl#AnnotationProperty";
/// The IRI for onProperty.
pub const OWL_ON_PROPERTY: &str = "http://www.w3.org/2002/07/owl#onProperty";
/// The IRI for onProperties.
pub const OWL_ON_PROPERTIES: &str = "http://www.w3.org/2002/07/owl#onProperties";
/// The IRI for onDataRange.
pub const OWL_ON_DATA_RANGE: &str = "http://www.w3.org/2002/07/owl#onDataRange";
/// The IRI for DatatypeProperty.
pub const OWL_DATATYPE_PROPERTY: &str = "http://www.w3.org/2002/07/owl#DatatypeProperty";
/// The IRI for ObjectProperty.
pub const OWL_OBJECT_PROPERTY: &str = "http://www.w3.org/2002/07/owl#ObjectProperty";
/// The IRI for Class.
pub const OWL_CLASS: &str = "http://www.w3.org/2002/07/owl#Class";
/// The IRI for NamedIndividual.
pub const OWL_NAMED_INDIVIDUAL: &str = "http://www.w3.org/2002/07/owl#NamedIndividual";
/// The IRI for Axiom.
pub const OWL_AXIOM: &str = "http://www.w3.org/2002/07/owl#Axiom";
/// The IRI for Thing.
pub const OWL_THING: &str = "http://www.w3.org/2002/07/owl#Thing";
/// The IRI for Nothing.
pub const OWL_NOTHING: &str = "http://www.w3.org/2002/07/owl#Nothing";
/// The IRI for Annotation.
pub const OWL_ANNOTATION: &str = "http://www.w3.org/2002/07/owl#Annotation";
/// The IRI for annotatedSource.
pub const OWL_ANNOTATED_SOURCE: &str = "http://www.w3.org/2002/07/owl#annotatedSource";
/// The IRI for annotatedProperty.
pub const OWL_ANNOTATED_PROPERTY: &str = "http://www.w3.org/2002/07/owl#annotatedProperty";
/// The IRI for annotatedTarget.
pub const OWL_ANNOTATED_TARGET: &str = "http://www.w3.org/2002/07/owl#annotatedTarget";
/// The IRI for AllDisjointClasses.
pub const OWL_ALL_DISJOINT_CLASSES: &str = "http://www.w3.org/2002/07/owl#AllDisjointClasses";
/// The IRI for AllDisjointProperties.
pub const OWL_ALL_DISJOINT_PROPERTIES: &str = "http://www.w3.org/2002/07/owl#AllDisjointProperties";
/// The IRI for AllDifferent.
pub const OWL_ALL_DIFFERENT: &str = "http://www.w3.org/2002/07/owl#AllDifferent";
/// The IRI for equivalentClass.
pub const OWL_EQUIVALENT_CLASS: &str = "http://www.w3.org/2002/07/owl#equivalentClass";
/// The IRI for members.
pub const OWL_MEMBERS: &str = "http://www.w3.org/2002/07/owl#members";
/// The IRI for equivalentProperty.
pub const OWL_EQUIVALENT_PROPERTY: &str = "http://www.w3.org/2002/07/owl#equivalentProperty";
/// The IRI for propertyDisjointWith.
pub const OWL_PROPERTY_DISJOINT_WITH: &str = "http://www.w3.org/2002/07/owl#propertyDisjointWith";
/// The IRI for FunctionalProperty.
pub const OWL_FUNCTIONAL_PROPERTY: &str = "http://www.w3.org/2002/07/owl#FunctionalProperty";
/// The IRI for InverseFunctionalProperty.
pub const OWL_INVERSE_FUNCTIONAL_PROPERTY: &str =
    "http://www.w3.org/2002/07/owl#InverseFunctionalProperty";
/// The IRI for ReflexiveProperty.
pub const OWL_REFLEXIVE_PROPERTY: &str = "http://www.w3.org/2002/07/owl#ReflexiveProperty";
/// The IRI for IrreflexiveProperty.
pub const OWL_IRREFLEXIVE_PROPERTY: &str = "http://www.w3.org/2002/07/owl#IrreflexiveProperty";
/// The IRI for SymmetricProperty.
pub const OWL_SYMMETRIC_PROPERTY: &str = "http://www.w3.org/2002/07/owl#SymmetricProperty";
/// The IRI for AsymmetricProperty.
pub const OWL_ASYMMETRIC_PROPERTY: &str = "http://www.w3.org/2002/07/owl#AsymmetricProperty";
/// The IRI for TransitiveProperty.
pub const OWL_TRANSITIVE_PROPERTY: &str = "http://www.w3.org/2002/07/owl#TransitiveProperty";
/// The IRI for disjointWith.
pub const OWL_DISJOINT_WITH: &str = "http://www.w3.org/2002/07/owl#disjointWith";
/// The IRI for disjointUnionOf.
pub const OWL_DISJOINT_UNION_OF: &str = "http://www.w3.org/2002/07/owl#disjointUnionOf";
/// The IRI for hasKey.
pub const OWL_HAS_KEY: &str = "http://www.w3.org/2002/07/owl#hasKey";
/// The IRI for NegativePropertyAssertion.
pub const OWL_NEGATIVE_PROPERTY_ASSERTION: &str =
    "http://www.w3.org/2002/07/owl#NegativePropertyAssertion";
/// The IRI for inverseOf.
pub const OWL_OBJECT_INVERSE_OF: &str = "http://www.w3.org/2002/07/owl#inverseOf";
/// The IRI for propertyChainAxiom.
pub const OWL_PROPERTY_CHAIN_AXIOM: &str = "http://www.w3.org/2002/07/owl#propertyChainAxiom";
/// The IRI for Restriction.
pub const OWL_RESTRICTION: &str = "http://www.w3.org/2002/07/owl#Restriction";
/// The IRI for intersectionOf.
pub const OWL_INTERSECTION_OF: &str = "http://www.w3.org/2002/07/owl#intersectionOf";
/// The IRI for unionOf.
pub const OWL_UNION_OF: &str = "http://www.w3.org/2002/07/owl#unionOf";
/// The IRI for complementOf.
pub const OWL_COMPLEMENT_OF: &str = "http://www.w3.org/2002/07/owl#complementOf";
/// The IRI for oneOf.
pub const OWL_ONE_OF: &str = "http://www.w3.org/2002/07/owl#oneOf";
/// The IRI for someValuesFrom.
pub const OWL_SOME_VALUES_FROM: &str = "http://www.w3.org/2002/07/owl#someValuesFrom";
/// The IRI for allValuesFrom.
pub const OWL_ALL_VALUES_FROM: &str = "http://www.w3.org/2002/07/owl#allValuesFrom";
/// The IRI for hasValue.
pub const OWL_HAS_VALUE: &str = "http://www.w3.org/2002/07/owl#hasValue";
/// The IRI for minQualifiedCardinality.
pub const OWL_MIN_QUALIFIED_CARDINALITY: &str =
    "http://www.w3.org/2002/07/owl#minQualifiedCardinality";
/// The IRI for maxQualifiedCardinality.
pub const OWL_MAX_QUALIFIED_CARDINALITY: &str =
    "http://www.w3.org/2002/07/owl#maxQualifiedCardinality";
/// The IRI for qualifiedCardinality.
pub const OWL_QUALIFIED_CARDINALITY: &str = "http://www.w3.org/2002/07/owl#qualifiedCardinality";
/// The IRI for cardinality.
pub const OWL_CARDINALITY: &str = "http://www.w3.org/2002/07/owl#cardinality";
/// The IRI for minCardinality.
pub const OWL_MIN_CARDINALITY: &str = "http://www.w3.org/2002/07/owl#minCardinality";
/// The IRI for maxCardinality.
pub const OWL_MAX_CARDINALITY: &str = "http://www.w3.org/2002/07/owl#maxCardinality";
/// The IRI for onClass.
pub const OWL_ON_CLASS: &str = "http://www.w3.org/2002/07/owl#onClass";
/// The IRI for hasSelf.
pub const OWL_HAS_SELF: &str = "http://www.w3.org/2002/07/owl#hasSelf";

/// The IRI for string.
pub const XSD_STRING: &str = "http://www.w3.org/2001/XMLSchema#string";
/// The IRI for boolean.
pub const XSD_BOOLEAN: &str = "http://www.w3.org/2001/XMLSchema#boolean";
/// The IRI for decimal.
pub const XSD_DECIMAL: &str = "http://www.w3.org/2001/XMLSchema#decimal";
/// The IRI for float.
pub const XSD_FLOAT: &str = "http://www.w3.org/2001/XMLSchema#float";
/// The IRI for double.
pub const XSD_DOUBLE: &str = "http://www.w3.org/2001/XMLSchema#double";
/// The IRI for duration.
pub const XSD_DURATION: &str = "http://www.w3.org/2001/XMLSchema#duration";
/// The IRI for dateTime.
pub const XSD_DATE_TIME: &str = "http://www.w3.org/2001/XMLSchema#dateTime";
/// The IRI for time.
pub const XSD_TIME: &str = "http://www.w3.org/2001/XMLSchema#time";
/// The IRI for date.
pub const XSD_DATE: &str = "http://www.w3.org/2001/XMLSchema#date";
/// The IRI for int.
pub const XSD_INT: &str = "http://www.w3.org/2001/XMLSchema#int";
/// The IRI for integer.
pub const XSD_INTEGER: &str = "http://www.w3.org/2001/XMLSchema#integer";
/// The IRI for nonNegativeInteger.
pub const XSD_NON_NEGATIVE_INTEGER: &str = "http://www.w3.org/2001/XMLSchema#nonNegativeInteger";
/// The IRI for hexBinary.
pub const XSD_HEX_BINARY: &str = "http://www.w3.org/2001/XMLSchema#hexBinary";
/// The IRI for base64Binary.
pub const XSD_BASE64_BINARY: &str = "http://www.w3.org/2001/XMLSchema#base64Binary";
/// The IRI for anyURI.
pub const XSD_ANY_URI: &str = "http://www.w3.org/2001/XMLSchema#anyURI";
/// The IRI for minLength.
pub const XSD_MIN_LENGTH: &str = "http://www.w3.org/2001/XMLSchema#minLength";
/// The IRI for maxLength.
pub const XSD_MAX_LENGTH: &str = "http://www.w3.org/2001/XMLSchema#maxLength";
/// The IRI for minInclusive.
pub const XSD_MIN_INCLUSIVE: &str = "http://www.w3.org/2001/XMLSchema#minInclusive";
/// The IRI for maxInclusive.
pub const XSD_MAX_INCLUSIVE: &str = "http://www.w3.org/2001/XMLSchema#maxInclusive";
/// The IRI for minExclusive.
pub const XSD_MIN_EXCLUSIVE: &str = "http://www.w3.org/2001/XMLSchema#minExclusive";
/// The IRI for maxExclusive.
pub const XSD_MAX_EXCLUSIVE: &str = "http://www.w3.org/2001/XMLSchema#maxExclusive";
/// The IRI for length.
pub const XSD_LENGTH: &str = "http://www.w3.org/2001/XMLSchema#length";
/// The IRI for pattern.
pub const XSD_PATTERN: &str = "http://www.w3.org/2001/XMLSchema#pattern";
/// The IRI for langRange.
pub const XSD_LANG_RANGE: &str = "http://www.w3.org/2001/XMLSchema#langRange";

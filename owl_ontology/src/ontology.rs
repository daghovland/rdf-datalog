/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

use ingress::{IriReference, OntologyVersion, PrefixDeclaration};
use crate::axioms::{Annotation, Axiom, Class, Declaration, Entity, FullIri, Individual};

/// An OWL 2 ontology.
pub struct Ontology {
    pub directly_imports_documents: Vec<IriReference>,
    pub version: OntologyVersion,
    pub annotations: Vec<Annotation>,
    pub axioms: Vec<Axiom>,
}

impl Ontology {
    pub fn new(
        directly_imports_documents: Vec<IriReference>,
        version: OntologyVersion,
        annotations: Vec<Annotation>,
        axioms: Vec<Axiom>,
    ) -> Self {
        Ontology { directly_imports_documents, version, annotations, axioms }
    }

    /// All axioms including built-in OWL 2 declarations.
    pub fn all_axioms(&self) -> impl Iterator<Item = Axiom> + '_ {
        let user: Vec<Axiom> = self.axioms.clone();
        let built_in = Self::built_in_declarations();
        user.into_iter().chain(built_in.into_iter())
    }

    pub fn try_get_ontology_iri(&self) -> Option<&IriReference> {
        self.version.try_get_ontology_iri()
    }

    pub fn try_get_version_iri(&self) -> Option<&IriReference> {
        self.version.try_get_ontology_version_iri()
    }

    fn built_in_declarations() -> Vec<Axiom> {
        let static_iris = [
            "http://www.w3.org/2002/07/owl#Thing",
            "http://www.w3.org/2002/07/owl#Nothing",
        ];
        let obj_prop_iris = [
            "http://www.w3.org/2002/07/owl#topObjectProperty",
            "http://www.w3.org/2002/07/owl#bottomObjectProperty",
        ];
        let data_prop_iris = [
            "http://www.w3.org/2002/07/owl#topDataProperty",
            "http://www.w3.org/2002/07/owl#bottomDataProperty",
        ];
        let datatype_iris = [
            "http://www.w3.org/2000/01/rdf-schema#Literal",
            "http://www.w3.org/2002/07/owl#real",
            "http://www.w3.org/2002/07/owl#rational",
            "http://www.w3.org/2001/XMLSchema#decimal",
            "http://www.w3.org/2001/XMLSchema#integer",
            "http://www.w3.org/2001/XMLSchema#nonNegativeInteger",
            "http://www.w3.org/2001/XMLSchema#nonPositiveInteger",
            "http://www.w3.org/2001/XMLSchema#positiveInteger",
            "http://www.w3.org/2001/XMLSchema#negativeInteger",
            "http://www.w3.org/2001/XMLSchema#long",
            "http://www.w3.org/2001/XMLSchema#int",
            "http://www.w3.org/2001/XMLSchema#short",
            "http://www.w3.org/2001/XMLSchema#byte",
            "http://www.w3.org/2001/XMLSchema#unsignedLong",
            "http://www.w3.org/2001/XMLSchema#unsignedInt",
            "http://www.w3.org/2001/XMLSchema#unsignedShort",
            "http://www.w3.org/2001/XMLSchema#unsignedByte",
        ];
        let annot_prop_iris = [
            "http://www.w3.org/2000/01/rdf-schema#label",
            "http://www.w3.org/2000/01/rdf-schema#comment",
            "http://www.w3.org/2000/01/rdf-schema#seeAlso",
            "http://www.w3.org/2000/01/rdf-schema#isDefinedBy",
            "http://www.w3.org/2002/07/owl#deprecated",
            "http://www.w3.org/2002/07/owl#versionInfo",
            "http://www.w3.org/2002/07/owl#priorVersion",
            "http://www.w3.org/2002/07/owl#backwardCompatibleWith",
            "http://www.w3.org/2002/07/owl#incompatibleWith",
        ];

        let mut decls: Vec<Axiom> = Vec::new();
        for iri in &static_iris {
            decls.push(Axiom::AxiomDeclaration((
                vec![],
                Entity::ClassDeclaration(FullIri(IriReference(iri.to_string()))),
            )));
        }
        for iri in &obj_prop_iris {
            decls.push(Axiom::AxiomDeclaration((
                vec![],
                Entity::ObjectPropertyDeclaration(FullIri(IriReference(iri.to_string()))),
            )));
        }
        for iri in &data_prop_iris {
            decls.push(Axiom::AxiomDeclaration((
                vec![],
                Entity::DataPropertyDeclaration(FullIri(IriReference(iri.to_string()))),
            )));
        }
        for iri in &datatype_iris {
            decls.push(Axiom::AxiomDeclaration((
                vec![],
                Entity::DatatypeDeclaration(FullIri(IriReference(iri.to_string()))),
            )));
        }
        for iri in &annot_prop_iris {
            decls.push(Axiom::AxiomDeclaration((
                vec![],
                Entity::AnnotationPropertyDeclaration(FullIri(IriReference(iri.to_string()))),
            )));
        }
        decls
    }
}

/// An OWL 2 ontology document (ontology + prefix declarations).
pub struct OntologyDocument {
    pub prefixes: Vec<PrefixDeclaration>,
    pub ontology: Ontology,
}

impl OntologyDocument {
    pub fn new(prefixes: Vec<PrefixDeclaration>, ontology: Ontology) -> Self {
        OntologyDocument { prefixes, ontology }
    }

    pub fn try_get_ontology_iri(&self) -> Option<&IriReference> {
        self.ontology.try_get_ontology_iri()
    }

    pub fn try_get_version_iri(&self) -> Option<&IriReference> {
        self.ontology.try_get_version_iri()
    }
}

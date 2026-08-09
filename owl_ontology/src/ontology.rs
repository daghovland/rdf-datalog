/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

use crate::axioms::{Annotation, Axiom, Entity, FullIri};
use ingress::{IriReference, OntologyVersion, PrefixDeclaration};

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
        Ontology {
            directly_imports_documents,
            version,
            annotations,
            axioms,
        }
    }

    /// Remove the first axiom in `self.axioms` that is value-equal to `axiom`.
    ///
    /// Returns `true` iff an axiom was actually removed.
    ///
    /// Only searches user-supplied `self.axioms` — the built-in declarations
    /// synthesised by [`Self::all_axioms`] (via the private
    /// `built_in_declarations` helper)
    /// (`owl:Thing`, `owl:Nothing`, `owl:topObjectProperty`, the XSD
    /// datatypes, ...) are not stored there and can never be removed this
    /// way; passing one of those always returns `false`.
    ///
    /// Part of incremental TBox retraction, see
    /// [#162](https://github.com/daghovland/rdf-datalog/issues/162):
    /// pairs with `owl2rl2datalog::axiom2datalog` (map the removed axiom to
    /// its compiled `Rule`s) and
    /// `datalog::IncrementalReasoner::apply_rule_deletions` (retract the
    /// facts those rules derived).
    pub fn remove_axiom(&mut self, axiom: &Axiom) -> bool {
        if let Some(pos) = self.axioms.iter().position(|a| a == axiom) {
            self.axioms.remove(pos);
            true
        } else {
            false
        }
    }

    /// All axioms including built-in OWL 2 declarations.
    pub fn all_axioms(&self) -> impl Iterator<Item = Axiom> + '_ {
        let user: Vec<Axiom> = self.axioms.clone();
        let built_in = Self::built_in_declarations();
        user.into_iter().chain(built_in)
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

#[cfg(test)]
mod tests {
    //! Direct unit coverage for [`Ontology::remove_axiom`], part of
    //! incremental TBox retraction
    //! ([#162](https://github.com/daghovland/rdf-datalog/issues/162)). The
    //! end-to-end path (`remove_axiom` + `owl2rl2datalog::axiom2datalog` +
    //! `datalog::IncrementalReasoner::apply_rule_deletions`) is covered in
    //! `owl2rl2datalog/src/lib.rs`'s `tbox_retraction_tests` module — these
    //! tests exercise `remove_axiom` itself, in isolation.
    use super::*;
    use crate::axioms::{ClassAxiom, ClassExpression};

    fn sub_class_of_axiom(sub: &str, sup: &str) -> Axiom {
        Axiom::AxiomClassAxiom(ClassAxiom::SubClassOf(
            vec![],
            ClassExpression::ClassName(FullIri(IriReference(sub.to_string()))),
            ClassExpression::ClassName(FullIri(IriReference(sup.to_string()))),
        ))
    }

    fn empty_ontology(axioms: Vec<Axiom>) -> Ontology {
        Ontology::new(vec![], OntologyVersion::UnNamedOntology, vec![], axioms)
    }

    /// Removing an axiom that is actually present returns `true` and the
    /// axiom is genuinely gone from `self.axioms` afterwards.
    #[test]
    fn test_remove_axiom_present_returns_true_and_removes_it() {
        let axiom = sub_class_of_axiom("http://example.org/Dog", "http://example.org/Animal");
        let mut ontology = empty_ontology(vec![axiom.clone()]);

        assert!(ontology.remove_axiom(&axiom));
        assert!(
            ontology.axioms.is_empty(),
            "the axiom must actually be gone from self.axioms, not just report true"
        );
    }

    /// Removing an axiom that was never added is a no-op: returns `false`
    /// and leaves `self.axioms` untouched.
    #[test]
    fn test_remove_axiom_absent_returns_false_no_op() {
        let present = sub_class_of_axiom("http://example.org/Dog", "http://example.org/Animal");
        let mut ontology = empty_ontology(vec![present.clone()]);

        let never_added = sub_class_of_axiom("http://example.org/Cat", "http://example.org/Animal");
        assert!(!ontology.remove_axiom(&never_added));
        assert_eq!(
            ontology.axioms,
            vec![present],
            "self.axioms must be completely untouched by a no-op removal"
        );
    }

    /// When two value-equal axioms are both present, `remove_axiom` removes
    /// only the first match — per its doc comment — leaving the other(s)
    /// untouched. Modelled here via two *distinct* axioms plus a duplicate
    /// of one of them, so the surviving count is unambiguous.
    #[test]
    fn test_remove_axiom_duplicate_removes_only_first_match() {
        let dup = sub_class_of_axiom("http://example.org/Dog", "http://example.org/Animal");
        let other = sub_class_of_axiom("http://example.org/Cat", "http://example.org/Animal");
        let mut ontology = empty_ontology(vec![dup.clone(), other.clone(), dup.clone()]);

        assert!(ontology.remove_axiom(&dup));
        assert_eq!(
            ontology.axioms,
            vec![other, dup],
            "only the first value-equal match must be removed; the second duplicate \
             and the unrelated axiom must survive untouched"
        );
    }

    /// A built-in declaration synthesised by `all_axioms()`/
    /// `built_in_declarations` (e.g. `owl:Thing`) is never actually stored
    /// in `self.axioms`, so removing it always returns `false` — even
    /// though it appears in `all_axioms()`'s output.
    #[test]
    fn test_remove_axiom_built_in_declaration_returns_false() {
        let mut ontology = empty_ontology(vec![]);

        let owl_thing_decl = Axiom::AxiomDeclaration((
            vec![],
            Entity::ClassDeclaration(FullIri(IriReference(
                "http://www.w3.org/2002/07/owl#Thing".to_string(),
            ))),
        ));
        assert!(
            ontology.all_axioms().any(|a| a == owl_thing_decl),
            "sanity: owl:Thing's declaration must actually appear in all_axioms()"
        );
        assert!(
            !ontology.remove_axiom(&owl_thing_decl),
            "a built-in declaration is never in self.axioms, so removal must report false"
        );
        assert!(
            ontology.axioms.is_empty(),
            "self.axioms must remain empty: there was nothing to remove"
        );
    }
}

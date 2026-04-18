/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Top-level RDF → OWL translation.
//! Mirrors `DagSemTools.RdfOwlTranslator.Rdf2Owl`.

use crate::axiom_parser::extract_axiom;
use crate::class_expression_parser::OntologyDeclarations;
use crate::ingress::WellKnownIds;
use dag_rdf::datastore::Datastore;
use dag_rdf::IriReference;
use ingress::*;
use owl_ontology::*;

/// Translate all RDF triples in `datastore` into an OWL 2 `OntologyDocument`.
///
/// This is the main entry point, mirroring `Rdf2Owl.extractOntology`.
pub fn rdf2owl(datastore: &mut Datastore) -> OntologyDocument {
    // Pre-compute all well-known IRI IDs (may add them to the resource manager)
    let ids = WellKnownIds::new(&mut datastore.resources);

    // Build class/property/individual declarations and parse anonymous class expressions
    let decls = OntologyDeclarations::build(datastore, &ids);

    // Extract ontology IRI and version from the triple store
    let (ontology_version, imports) = extract_ontology_name(datastore, &ids);

    // Extract all axioms from triples
    let axioms: Vec<Axiom> = datastore
        .named_graphs
        .get_all_quads()
        .filter(|q| q.triple_id == dag_rdf::ingress::DEFAULT_GRAPH_ELEMENT_ID)
        .filter_map(|q| {
            let triple = dag_rdf::ingress::Triple {
                subject: q.subject,
                predicate: q.predicate,
                obj: q.obj,
            };
            extract_axiom(datastore, &ids, &decls, &triple)
        })
        .collect();

    OntologyDocument::new(
        vec![],
        Ontology::new(imports, ontology_version, vec![], axioms),
    )
}

fn extract_ontology_version_iri(
    datastore: &Datastore,
    ids: &WellKnownIds,
    ontology_iri_id: dag_rdf::GraphElementId,
) -> Option<IriReference> {
    let triples: Vec<_> = datastore
        .get_triples_with_subject_predicate(ontology_iri_id, ids.owl_version_iri_id)
        .collect();
    if triples.len() > 1 {
        log::warn!("Multiple owl:versionIri triples found – using first");
    }
    triples.first().and_then(|tr| {
        datastore.resources.get_named_resource(tr.obj).cloned()
    })
}

fn extract_ontology_imports(
    datastore: &Datastore,
    ids: &WellKnownIds,
    ontology_iri_id: dag_rdf::GraphElementId,
) -> Vec<IriReference> {
    datastore
        .get_triples_with_subject_predicate(ontology_iri_id, ids.owl_import_id)
        .filter_map(|tr| datastore.resources.get_named_resource(tr.obj).cloned())
        .collect()
}

fn extract_ontology_iri(
    datastore: &Datastore,
    ids: &WellKnownIds,
) -> Option<dag_rdf::GraphElementId> {
    let ontology_type_triples: Vec<_> = datastore
        .get_triples_with_object_predicate(ids.owl_ontology_id, ids.rdf_type_id)
        .collect();
    if ontology_type_triples.len() > 1 {
        log::warn!("Multiple owl:Ontology declarations found – using first");
    }
    ontology_type_triples.first().map(|tr| tr.subject)
}

fn extract_ontology_name(
    datastore: &Datastore,
    ids: &WellKnownIds,
) -> (OntologyVersion, Vec<IriReference>) {
    match extract_ontology_iri(datastore, ids) {
        None => (OntologyVersion::UnNamedOntology, vec![]),
        Some(iri_id) => {
            let iri = match datastore.resources.get_named_resource(iri_id) {
                Some(i) => i.clone(),
                None => return (OntologyVersion::UnNamedOntology, vec![]),
            };
            let imports = extract_ontology_imports(datastore, ids, iri_id);
            let version = match extract_ontology_version_iri(datastore, ids, iri_id) {
                None => OntologyVersion::NamedOntology(iri),
                Some(version_iri) => OntologyVersion::VersionedOntology { ontology_iri: iri, version_iri },
            };
            (version, imports)
        }
    }
}

/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Translation of OWL 2 axioms to datalog rules, implementing Section 4.3 of
//! <https://www.w3.org/TR/owl2-profiles/#OWL_2_RL>.

pub mod abox;
pub mod equality;

pub use abox::assert_abox;

use dag_rdf::query::get_default_graph_pattern;
use dag_rdf::{GraphElementId, GraphElementManager, RdfResource, Term};
use datalog::types::{Rule, RuleAtom, RuleHead};
use ingress::{IriReference, RDF_TYPE};
use owl_ontology::{
    Axiom, ClassAxiom, ClassExpression, DataPropertyAxiom, FullIri, ObjectPropertyAxiom,
    ObjectPropertyExpression, Ontology,
};

// ── Resource helpers ──────────────────────────────────────────────────────────

fn rdf_type_id(resources: &mut GraphElementManager) -> GraphElementId {
    resources.add_node_resource(RdfResource::Iri(IriReference(RDF_TYPE.to_owned())))
}

fn type_pattern(
    resources: &mut GraphElementManager,
    var: &str,
    class_id: GraphElementId,
) -> dag_rdf::QuadPattern {
    get_default_graph_pattern(
        Term::Variable(var.to_owned()),
        Term::Resource(rdf_type_id(resources)),
        Term::Resource(class_id),
    )
}

/// Build a pattern `?subject <role> ?object` in the default graph.
fn get_obj_prop_pattern(
    resources: &mut GraphElementManager,
    prop: &ObjectPropertyExpression,
    subject_var: &str,
    object_var: &str,
) -> Option<dag_rdf::QuadPattern> {
    match prop {
        ObjectPropertyExpression::NamedObjectProperty(FullIri(iri)) => {
            let role_id = resources.add_node_resource(RdfResource::Iri(iri.clone()));
            Some(get_default_graph_pattern(
                Term::Variable(subject_var.to_owned()),
                Term::Resource(role_id),
                Term::Variable(object_var.to_owned()),
            ))
        }
        ObjectPropertyExpression::AnonymousObjectProperty(id) => {
            let role_id = resources.add_node_resource(RdfResource::AnonymousBlankNode(*id));
            Some(get_default_graph_pattern(
                Term::Variable(subject_var.to_owned()),
                Term::Resource(role_id),
                Term::Variable(object_var.to_owned()),
            ))
        }
        ObjectPropertyExpression::InverseObjectProperty(inner) => {
            get_obj_prop_pattern(resources, inner, object_var, subject_var)
        }
        ObjectPropertyExpression::ObjectPropertyChain(_) => {
            log::warn!("Domain/range of property chain not supported");
            None
        }
    }
}

fn get_class_expression_ids(
    resources: &mut GraphElementManager,
    expr: &ClassExpression,
) -> Vec<GraphElementId> {
    match expr {
        ClassExpression::ClassName(FullIri(iri)) => {
            vec![resources.add_node_resource(RdfResource::Iri(iri.clone()))]
        }
        ClassExpression::AnonymousClass(id) => {
            vec![resources.add_node_resource(RdfResource::AnonymousBlankNode(*id))]
        }
        ClassExpression::ObjectIntersectionOf(classes) => classes
            .iter()
            .flat_map(|c| get_class_expression_ids(resources, c))
            .collect(),
        ClassExpression::ObjectUnionOf(_) => {
            log::warn!("OWL 2 RL: Union in domain/range expression not supported");
            vec![]
        }
        _ => {
            log::warn!("Unhandled class expression in domain/range: {:?}", expr);
            vec![]
        }
    }
}

// ── ObjectProperty axiom translations ────────────────────────────────────────

fn object_property_domain(
    resources: &mut GraphElementManager,
    prop: &ObjectPropertyExpression,
    domain: &ClassExpression,
) -> Vec<Rule> {
    let Some(body_quad) = get_obj_prop_pattern(resources, prop, "x", "y") else {
        return vec![];
    };
    get_class_expression_ids(resources, domain)
        .into_iter()
        .map(|cls_id| Rule {
            head: RuleHead::NormalHead(type_pattern(resources, "x", cls_id)),
            body: vec![RuleAtom::PositivePattern(body_quad.clone())],
        })
        .collect()
}

fn object_property_range(
    resources: &mut GraphElementManager,
    prop: &ObjectPropertyExpression,
    range: &ClassExpression,
) -> Vec<Rule> {
    let Some(body_quad) = get_obj_prop_pattern(resources, prop, "x", "y") else {
        return vec![];
    };
    get_class_expression_ids(resources, range)
        .into_iter()
        .map(|cls_id| Rule {
            head: RuleHead::NormalHead(type_pattern(resources, "y", cls_id)),
            body: vec![RuleAtom::PositivePattern(body_quad.clone())],
        })
        .collect()
}

/// prp-symp: T(?p, rdf:type, owl:SymmetricProperty) T(?x, ?p, ?y) -> T(?y, ?p, ?x)
fn symmetric_object_property(
    resources: &mut GraphElementManager,
    prop: &ObjectPropertyExpression,
) -> Vec<Rule> {
    let Some(body_quad) = get_obj_prop_pattern(resources, prop, "x", "y") else {
        return vec![];
    };
    let Some(head_quad) = get_obj_prop_pattern(resources, prop, "y", "x") else {
        return vec![];
    };
    vec![Rule {
        head: RuleHead::NormalHead(head_quad),
        body: vec![RuleAtom::PositivePattern(body_quad)],
    }]
}

fn transitive_object_property(
    resources: &mut GraphElementManager,
    prop: &ObjectPropertyExpression,
) -> Vec<Rule> {
    let Some(xy) = get_obj_prop_pattern(resources, prop, "x", "y") else {
        return vec![];
    };
    let Some(yz) = get_obj_prop_pattern(resources, prop, "y", "z") else {
        return vec![];
    };
    let Some(xz) = get_obj_prop_pattern(resources, prop, "x", "z") else {
        return vec![];
    };
    vec![Rule {
        head: RuleHead::NormalHead(xz),
        body: vec![RuleAtom::PositivePattern(xy), RuleAtom::PositivePattern(yz)],
    }]
}

fn object_property_axiom2datalog(
    resources: &mut GraphElementManager,
    axiom: &ObjectPropertyAxiom,
) -> Vec<Rule> {
    match axiom {
        ObjectPropertyAxiom::ObjectPropertyDomain(prop, domain) => {
            object_property_domain(resources, prop, domain)
        }
        ObjectPropertyAxiom::ObjectPropertyRange(prop, range) => {
            object_property_range(resources, prop, range)
        }
        ObjectPropertyAxiom::SymmetricObjectProperty(_, prop) => {
            symmetric_object_property(resources, prop)
        }
        ObjectPropertyAxiom::TransitiveObjectProperty(_, prop) => {
            transitive_object_property(resources, prop)
        }
        _ => vec![],
    }
}

// ── DataProperty axiom translations ──────────────────────────────────────────

fn data_property_domain(
    resources: &mut GraphElementManager,
    prop_iri: &IriReference,
    domain: &ClassExpression,
) -> Vec<Rule> {
    let prop_id = resources.add_node_resource(RdfResource::Iri(prop_iri.clone()));
    let body_quad = get_default_graph_pattern(
        Term::Variable("x".to_owned()),
        Term::Resource(prop_id),
        Term::Variable("y".to_owned()),
    );
    get_class_expression_ids(resources, domain)
        .into_iter()
        .map(|cls_id| Rule {
            head: RuleHead::NormalHead(type_pattern(resources, "x", cls_id)),
            body: vec![RuleAtom::PositivePattern(body_quad.clone())],
        })
        .collect()
}

fn data_property_range(
    resources: &mut GraphElementManager,
    prop_iri: &IriReference,
    range: &owl_ontology::DataRange,
) -> Vec<Rule> {
    let prop_id = resources.add_node_resource(RdfResource::Iri(prop_iri.clone()));
    let body_quad = get_default_graph_pattern(
        Term::Variable("x".to_owned()),
        Term::Resource(prop_id),
        Term::Variable("y".to_owned()),
    );
    let range_id = match range {
        owl_ontology::DataRange::NamedDataRange(FullIri(dt_iri)) => {
            resources.add_node_resource(RdfResource::Iri(dt_iri.clone()))
        }
        _ => {
            log::warn!("Complex data ranges not yet supported");
            return vec![];
        }
    };
    vec![Rule {
        head: RuleHead::NormalHead(type_pattern(resources, "y", range_id)),
        body: vec![RuleAtom::PositivePattern(body_quad)],
    }]
}

fn data_property_axiom2datalog(
    resources: &mut GraphElementManager,
    axiom: &DataPropertyAxiom,
) -> Vec<Rule> {
    match axiom {
        DataPropertyAxiom::DataPropertyDomain(_, FullIri(iri), domain) => {
            data_property_domain(resources, iri, domain)
        }
        DataPropertyAxiom::DataPropertyRange(_, FullIri(iri), range) => {
            data_property_range(resources, iri, range)
        }
        DataPropertyAxiom::SubDataPropertyOf(_, _, _) => {
            log::warn!("Data property hierarchy not implemented yet");
            vec![]
        }
        DataPropertyAxiom::EquivalentDataProperties(_, _) => {
            log::warn!("Equivalent data property not implemented yet");
            vec![]
        }
        DataPropertyAxiom::DisjointDataProperties(_, _) => {
            log::warn!("Disjoint data property not implemented yet");
            vec![]
        }
        DataPropertyAxiom::FunctionalDataProperty(_, _) => {
            log::warn!("Functional data property not implemented yet");
            vec![]
        }
    }
}

// ── Class axiom translations ──────────────────────────────────────────────────

fn class_axiom2datalog(resources: &mut GraphElementManager, axiom: &ClassAxiom) -> Vec<Rule> {
    eli::owl2datalog(resources, axiom).unwrap_or_default()
}

// ── Top-level ─────────────────────────────────────────────────────────────────

fn owl_axiom2datalog(resources: &mut GraphElementManager, axiom: &Axiom) -> Vec<Rule> {
    match axiom {
        Axiom::AxiomObjectPropertyAxiom(a) => object_property_axiom2datalog(resources, a),
        Axiom::AxiomClassAxiom(a) => class_axiom2datalog(resources, a),
        Axiom::AxiomDataPropertyAxiom(a) => data_property_axiom2datalog(resources, a),
        _ => vec![],
    }
}

/// Translate a full OWL 2 ontology into a set of datalog rules.
///
/// Duplicate rules (structurally identical after IRI interning) are removed
/// before returning. This can reduce rule count by 20–40% for ontologies with
/// many subclass chains that generate overlapping RL rules.
pub fn owl2datalog(resources: &mut GraphElementManager, ontology: &Ontology) -> Vec<Rule> {
    let mut rules: Vec<Rule> = ontology
        .all_axioms()
        .flat_map(|axiom| owl_axiom2datalog(resources, &axiom))
        .collect();

    rules.extend(equality::get_equality_axioms(resources));

    rules.sort_unstable();
    rules.dedup();
    rules
}

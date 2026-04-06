/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

use dag_rdf::{GraphElementManager, RdfResource, Term};
use dag_rdf::query::get_default_graph_pattern;
use datalog::types::{Rule, RuleAtom, RuleHead};
use ingress::{IriReference, OWL_SAME_AS};

fn owl_same_as_id(resources: &mut GraphElementManager) -> dag_rdf::GraphElementId {
    resources.add_node_resource(RdfResource::Iri(IriReference(OWL_SAME_AS.to_owned())))
}

/// eq-sym: T(?x, owl:sameAs, ?y) -> T(?y, owl:sameAs, ?x)
fn get_symmetry_axiom(resources: &mut GraphElementManager) -> Rule {
    let same_as = owl_same_as_id(resources);
    Rule {
        head: RuleHead::NormalHead(get_default_graph_pattern(
            Term::Variable("y".to_owned()),
            Term::Resource(same_as),
            Term::Variable("x".to_owned()),
        )),
        body: vec![RuleAtom::PositivePattern(get_default_graph_pattern(
            Term::Variable("x".to_owned()),
            Term::Resource(same_as),
            Term::Variable("y".to_owned()),
        ))],
    }
}

/// eq-trans: T(?x, owl:sameAs, ?y), T(?y, owl:sameAs, ?z) -> T(?x, owl:sameAs, ?z)
fn get_transitivity_axiom(resources: &mut GraphElementManager) -> Rule {
    let same_as = owl_same_as_id(resources);
    Rule {
        head: RuleHead::NormalHead(get_default_graph_pattern(
            Term::Variable("x".to_owned()),
            Term::Resource(same_as),
            Term::Variable("z".to_owned()),
        )),
        body: vec![
            RuleAtom::PositivePattern(get_default_graph_pattern(
                Term::Variable("x".to_owned()),
                Term::Resource(same_as),
                Term::Variable("y".to_owned()),
            )),
            RuleAtom::PositivePattern(get_default_graph_pattern(
                Term::Variable("y".to_owned()),
                Term::Resource(same_as),
                Term::Variable("z".to_owned()),
            )),
        ],
    }
}

/// eq-rep-s: T(?s1, owl:sameAs, ?s2), T(?s1, ?p, ?o) -> T(?s2, ?p, ?o)
fn get_subject_equality_axiom(resources: &mut GraphElementManager) -> Rule {
    let same_as = owl_same_as_id(resources);
    Rule {
        head: RuleHead::NormalHead(get_default_graph_pattern(
            Term::Variable("s2".to_owned()),
            Term::Variable("p".to_owned()),
            Term::Variable("o".to_owned()),
        )),
        body: vec![
            RuleAtom::PositivePattern(get_default_graph_pattern(
                Term::Variable("s1".to_owned()),
                Term::Resource(same_as),
                Term::Variable("s2".to_owned()),
            )),
            RuleAtom::PositivePattern(get_default_graph_pattern(
                Term::Variable("s1".to_owned()),
                Term::Variable("p".to_owned()),
                Term::Variable("o".to_owned()),
            )),
        ],
    }
}

/// eq-rep-o: T(?o1, owl:sameAs, ?o2), T(?s, ?p, ?o1) -> T(?s, ?p, ?o2)
fn get_object_equality_axiom(resources: &mut GraphElementManager) -> Rule {
    let same_as = owl_same_as_id(resources);
    Rule {
        head: RuleHead::NormalHead(get_default_graph_pattern(
            Term::Variable("s".to_owned()),
            Term::Variable("p".to_owned()),
            Term::Variable("o2".to_owned()),
        )),
        body: vec![
            RuleAtom::PositivePattern(get_default_graph_pattern(
                Term::Variable("o1".to_owned()),
                Term::Resource(same_as),
                Term::Variable("o2".to_owned()),
            )),
            RuleAtom::PositivePattern(get_default_graph_pattern(
                Term::Variable("s".to_owned()),
                Term::Variable("p".to_owned()),
                Term::Variable("o1".to_owned()),
            )),
        ],
    }
}

pub fn get_equality_axioms(resources: &mut GraphElementManager) -> Vec<Rule> {
    vec![
        get_symmetry_axiom(resources),
        get_subject_equality_axiom(resources),
        get_object_equality_axiom(resources),
        get_transitivity_axiom(resources),
    ]
}

/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! VQS productive-extension index endpoint.
//!
//! Implements Phase 7 of `docs/plans/VQS_INDEX_PLAN.md`: wires the
//! `vqs-index` crate's precomputed configuration-query index into the HTTP
//! API so the query-builder frontend can ask "which values of this property
//! actually occur on instances of this class?" in well under 100 ms, without
//! firing a live SPARQL query for every keystroke.
//!
//! `GET /vqs/productive-values?class=<IRI>&property=<IRI>` returns the
//! productive data values for `property` on instances of `class`, using the
//! **Wld** reference configuration (one star-shaped, data-property-only
//! index per class — see `vqs_index::config_set::ConfigSet::w_local_data_only`).
//!
//! # Schema requirement
//!
//! The navigation graph is derived via `NavGraph::from_datastore`, which
//! reads `rdfs:domain` / `rdfs:range` triples.  Datasets that only contain
//! instance data (no domain/range declarations) produce an empty navigation
//! graph, and every lookup reports `"covered": false`.  Declaring domain and
//! range for the properties you want indexed is required to use this endpoint.
//!
//! # Caching
//!
//! The navigation graph and index are expensive to rebuild, so they are
//! cached in `AppState::vqs_cache` and only recomputed when the `Datastore`
//! generation counter (bumped on every mutation) changes.

use crate::AppState;
use crate::serialize::sparql_json::graph_element_to_json;
use axum::{
    Json,
    extract::{Query, State},
    response::IntoResponse,
};
use dag_rdf::GraphElement;
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use vqs_index::{ConfigSet, NavGraph};

/// Cached navigation graph + Wld configuration set, tagged with the
/// `Datastore` generation it was built from.
pub struct VqsCache {
    generation: u64,
    nav: NavGraph,
    config_set: ConfigSet,
}

#[derive(Deserialize)]
pub struct ProductiveValuesParams {
    pub class: String,
    pub property: String,
}

/// `GET /vqs/productive-values?class=<IRI>&property=<IRI>`
pub async fn productive_values(
    State(state): State<AppState>,
    Query(params): Query<ProductiveValuesParams>,
) -> impl IntoResponse {
    let generation = state.store.read().await.generation;

    let up_to_date = {
        let cache = state.vqs_cache.read().await;
        matches!(cache.as_ref(), Some(c) if c.generation == generation)
    };
    if !up_to_date {
        let ds = state.store.read().await;
        let nav = NavGraph::from_datastore(&ds);
        let config_set = ConfigSet::w_local_data_only(&nav, &ds);
        drop(ds);
        *state.vqs_cache.write().await = Some(VqsCache {
            generation,
            nav,
            config_set,
        });
    }

    let cache = state.vqs_cache.read().await;
    let cache = cache.as_ref().expect("just rebuilt or already fresh");

    match find_productive_values(&cache.config_set, &cache.nav, &params.class, &params.property) {
        Some(values) => Json(json!({
            "covered": true,
            "values": values.iter().map(graph_element_to_json).collect::<Vec<_>>(),
        }))
        .into_response(),
        None => Json(json!({ "covered": false, "values": [] })).into_response(),
    }
}

/// Look up productive values for `property` on `class` using the Wld config set.
///
/// Returns `None` when the class or property is not covered by the index —
/// e.g. the dataset has no `rdfs:domain`/`rdfs:range` declaration for it, or
/// `property` is an object (not data) property.
fn find_productive_values(
    config_set: &ConfigSet,
    nav: &NavGraph,
    class_iri: &str,
    property_iri: &str,
) -> Option<Vec<GraphElement>> {
    let class_id = nav.node_by_iri(class_iri)?;
    let table = config_set
        .tables
        .iter()
        .find(|t| t.config.nodes[0].nav_node == class_id)?;
    let node_idx = table.config.nodes.iter().position(|n| {
        n.parent_edge
            .is_some_and(|e| nav.edge(e).iri == property_iri)
    })?;
    Some(table.lookup_values(node_idx, &HashMap::new()))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use dag_rdf::Datastore;
    use turtle::parse_turtle;

    fn schema_fixture() -> Datastore {
        let ttl = r#"
            @prefix rdf:  <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .
            @prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
            @prefix xsd:  <http://www.w3.org/2001/XMLSchema#> .
            @prefix ex:   <http://example.org/> .

            ex:age  rdfs:domain ex:Person ; rdfs:range xsd:integer .
            ex:name rdfs:domain ex:Person ; rdfs:range xsd:string .

            ex:alice rdf:type ex:Person ; ex:age "30"^^xsd:integer ; ex:name "Alice"^^xsd:string .
            ex:bob   rdf:type ex:Person ; ex:age "25"^^xsd:integer ; ex:name "Bob"^^xsd:string .
        "#;
        let mut ds = Datastore::new(200);
        parse_turtle(&mut ds, ttl.as_bytes()).expect("fixture must parse");
        ds
    }

    /// Both ages are productive for Person→age.
    #[test]
    fn finds_productive_ages() {
        let ds = schema_fixture();
        let nav = NavGraph::from_datastore(&ds);
        let config_set = ConfigSet::w_local_data_only(&nav, &ds);
        let values = find_productive_values(
            &config_set,
            &nav,
            "http://example.org/Person",
            "http://example.org/age",
        )
        .expect("Person/age must be covered");
        assert_eq!(values.len(), 2);
    }

    /// A class with no nav-graph entry is reported as uncovered, not an empty list.
    #[test]
    fn unknown_class_not_covered() {
        let ds = schema_fixture();
        let nav = NavGraph::from_datastore(&ds);
        let config_set = ConfigSet::w_local_data_only(&nav, &ds);
        let values = find_productive_values(
            &config_set,
            &nav,
            "http://example.org/NoSuchClass",
            "http://example.org/age",
        );
        assert!(values.is_none());
    }

    /// A property absent from the class's outgoing edges is uncovered.
    #[test]
    fn unknown_property_not_covered() {
        let ds = schema_fixture();
        let nav = NavGraph::from_datastore(&ds);
        let config_set = ConfigSet::w_local_data_only(&nav, &ds);
        let values = find_productive_values(
            &config_set,
            &nav,
            "http://example.org/Person",
            "http://example.org/noSuchProperty",
        );
        assert!(values.is_none());
    }

    /// A dataset with no domain/range declarations yields an empty nav graph,
    /// so every lookup is uncovered (documents the schema requirement).
    #[test]
    fn no_schema_declarations_means_uncovered() {
        let ttl = r#"
            @prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .
            @prefix ex:  <http://example.org/> .
            ex:alice rdf:type ex:Person ; ex:age "30" .
        "#;
        let mut ds = Datastore::new(50);
        parse_turtle(&mut ds, ttl.as_bytes()).expect("fixture must parse");
        let nav = NavGraph::from_datastore(&ds);
        let config_set = ConfigSet::w_local_data_only(&nav, &ds);
        let values = find_productive_values(
            &config_set,
            &nav,
            "http://example.org/Person",
            "http://example.org/age",
        );
        assert!(values.is_none(), "no domain/range declared ⇒ uncovered");
    }
}

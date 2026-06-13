/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! In-memory dataset registry.
//!
//! Maps dataset names (e.g. `"ds"`) to their `Arc<RwLock<Datastore>>`.
//! The default dataset is always `"ds"`.

use dag_rdf::Datastore;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::RwLock;

pub struct DatasetRegistry {
    datasets: HashMap<String, Arc<RwLock<Datastore>>>,
}

impl DatasetRegistry {
    /// Create a registry with a single default `"ds"` dataset.
    pub fn new_with_default(store: Arc<RwLock<Datastore>>) -> Self {
        let mut datasets = HashMap::new();
        datasets.insert("ds".to_string(), store);
        Self { datasets }
    }

    fn canonical(name: &str) -> &str {
        name.trim_start_matches('/')
    }

    pub fn get(&self, name: &str) -> Option<Arc<RwLock<Datastore>>> {
        self.datasets.get(Self::canonical(name)).cloned()
    }

    pub fn exists(&self, name: &str) -> bool {
        self.datasets.contains_key(Self::canonical(name))
    }

    pub fn insert(&mut self, name: &str, store: Arc<RwLock<Datastore>>) {
        self.datasets
            .insert(Self::canonical(name).to_string(), store);
    }

    /// Returns `true` if the dataset was present.
    pub fn remove(&mut self, name: &str) -> bool {
        self.datasets.remove(Self::canonical(name)).is_some()
    }

    pub fn names(&self) -> Vec<&str> {
        self.datasets.keys().map(String::as_str).collect()
    }

    /// JSON body for `GET /$/datasets`.
    pub fn all_datasets_json(&self) -> serde_json::Value {
        let datasets: Vec<serde_json::Value> =
            self.datasets.keys().map(|n| dataset_info_json(n)).collect();
        serde_json::json!({ "datasets": datasets })
    }

    /// JSON body for `GET /$/datasets/{name}`.  `None` if dataset not found.
    pub fn dataset_info_json(&self, name: &str) -> Option<serde_json::Value> {
        let name = Self::canonical(name);
        self.datasets
            .contains_key(name)
            .then(|| dataset_info_json(name))
    }
}

fn dataset_info_json(name: &str) -> serde_json::Value {
    serde_json::json!({
        "ds.name": format!("/{name}"),
        "ds.state": "active",
        "ds.services": [
            { "srv.type": "query",  "srv.description": "SPARQL 1.1 Query",
              "srv.endpoints": ["query", "sparql"] },
            { "srv.type": "update", "srv.description": "SPARQL 1.1 Update",
              "srv.endpoints": ["update"] },
            { "srv.type": "gsp-rw", "srv.description": "Graph Store Protocol (Read-Write)",
              "srv.endpoints": ["data"] },
            { "srv.type": "gsp-r",  "srv.description": "Graph Store Protocol (Read only)",
              "srv.endpoints": ["get"] }
        ]
    })
}

/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Transaction registry and open-transaction state for the proprietary
//! BEGIN / COMMIT / ROLLBACK HTTP transaction API.
//!
//! Each open transaction stores the store generation it started with (for
//! optimistic-concurrency conflict detection), lists of quads to insert/delete
//! at commit time, and a last-activity timestamp for stale-transaction eviction.
//!
//! Related: [#125](https://github.com/daghovland/rdf-datalog/issues/125)

use dag_rdf::ingress::Quad;
use std::collections::HashMap;
use std::time::Instant;

/// State of a single open (uncommitted) transaction.
pub struct OpenTransaction {
    /// Generation of the store at `POST /transaction/begin` time.
    ///
    /// At commit time the live store's generation must equal this value,
    /// otherwise the commit is rejected with HTTP 409 Conflict.
    pub snapshot_generation: u64,
    /// Quads to insert into the store when the transaction is committed.
    ///
    /// All IDs are valid in the live store's `GraphElementManager` because
    /// resources were interned into the live store at write time.
    pub pending_inserts: Vec<Quad>,
    /// Quads to delete from the store when the transaction is committed.
    pub pending_deletes: Vec<Quad>,
    /// Wall-clock time of the most recent activity on this transaction.
    ///
    /// Used by [`TransactionRegistry::purge_stale`] to evict idle transactions.
    pub last_activity: Instant,
}

/// Registry of all open transactions, keyed by their UUID string.
#[derive(Default)]
pub struct TransactionRegistry {
    transactions: HashMap<String, OpenTransaction>,
}

impl TransactionRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Open a new transaction for the given store `generation`.
    ///
    /// Returns the UUID string that identifies the new transaction.
    pub fn begin(&mut self, generation: u64) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        self.transactions.insert(
            id.clone(),
            OpenTransaction {
                snapshot_generation: generation,
                pending_inserts: Vec::new(),
                pending_deletes: Vec::new(),
                last_activity: Instant::now(),
            },
        );
        id
    }

    /// Look up an open transaction by ID (immutable).
    pub fn get(&self, id: &str) -> Option<&OpenTransaction> {
        self.transactions.get(id)
    }

    /// Look up an open transaction by ID (mutable).
    pub fn get_mut(&mut self, id: &str) -> Option<&mut OpenTransaction> {
        self.transactions.get_mut(id)
    }

    /// Remove and return an open transaction.  Returns `None` if not found.
    pub fn remove(&mut self, id: &str) -> Option<OpenTransaction> {
        self.transactions.remove(id)
    }

    /// Evict transactions whose last activity is older than `timeout_secs`.
    pub fn purge_stale(&mut self, timeout_secs: u64) {
        let now = Instant::now();
        self.transactions
            .retain(|_, tx| now.duration_since(tx.last_activity).as_secs() < timeout_secs);
    }
}

/*
Copyright (C) 2026 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Durable changelog for quad mutations backed by a `redb` embedded database.
//!
//! ## Design
//!
//! The in-memory `Datastore` remains the query engine; `redb` is used only as
//! a durable write-ahead log of quad mutations.
//!
//! ### Write path (per mutating HTTP request)
//!
//! 1. Collect the quad delta (inserts / deletes / graph-clear operations).
//! 2. Serialize the delta as `LogEntry` values and append to the redb log
//!    within a single write transaction.
//! 3. `commit()` — redb fsyncs at this point.
//! 4. On `Ok`, apply the same delta to the in-memory `Datastore`, then return
//!    200/204 to the client.
//! 5. On `Err`, return 500 — the in-memory store is unchanged.
//!
//! ### Read path
//!
//! Reads go entirely through the in-memory `Datastore` — unchanged.
//!
//! ### Startup / recovery
//!
//! 1. Open (or create) the redb file at `<data-dir>/<db-file>`.
//! 2. `redb` automatically replays its own WAL for any committed-but-not-
//!    checkpointed transactions from a previous crash.
//! 3. Call `QuadChangelog::replay()` to iterate all log entries and apply them
//!    to a fresh in-memory `Datastore`.
//!
//! ### Compaction note
//!
//! The log grows unbounded in this first implementation.  Checkpoint / snapshot
//! compaction (replace the log with a full-dataset snapshot) is a follow-up.

use dag_rdf::{
    Datastore, GraphElement, IriReference, RdfLiteral, RdfResource,
    ingress::{DEFAULT_GRAPH_ELEMENT_ID, Quad},
};
use ingress::{
    XSD_BOOLEAN, XSD_DATE, XSD_DATE_TIME, XSD_DECIMAL, XSD_DOUBLE, XSD_DURATION, XSD_FLOAT,
    XSD_INTEGER, XSD_TIME,
};
use redb::{Database, ReadableDatabase, ReadableTable, ReadableTableMetadata, TableDefinition};
use serde::{Deserialize, Serialize};
use std::path::Path;

// ── redb table schema ─────────────────────────────────────────────────────────

/// Sequential log of quad-change operations.
/// Key: monotonically increasing u64 sequence number.
/// Value: JSON-encoded `LogEntry`.
const QUAD_LOG: TableDefinition<u64, &[u8]> = TableDefinition::new("quad_log");

// ── Serialisable types ────────────────────────────────────────────────────────

/// Serialisable representation of one RDF term (subject, predicate, or object).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum ElementRepr {
    Iri(String),
    Blank(u32),
    /// Plain string literal (`xsd:string`).
    LiteralPlain(String),
    /// Language-tagged literal (`rdf:langString`).
    LiteralLang {
        lexical: String,
        lang: String,
    },
    /// Any other typed literal — the datatype IRI is stored alongside the lexical form.
    LiteralTyped {
        lexical: String,
        datatype: String,
    },
}

/// The dataset name every quad-mutation `LogEntry` predates having a
/// `dataset` field at all implicitly belonged to: the changelog has only
/// ever been written for the single default dataset until
/// [#670](https://github.com/daghovland/rdf-datalog/issues/670), so this is
/// the historically-correct interpretation of an old on-disk entry, not just
/// an arbitrary fallback.
fn default_dataset_name() -> String {
    "ds".to_string()
}

/// A single entry in the durable changelog.
#[derive(Serialize, Deserialize, Debug)]
pub enum LogEntry {
    /// All quads in the named (or default) graph were removed.
    ClearGraph {
        /// Which dataset this mutation applies to. `#[serde(default)]` so
        /// pre-#670 on-disk entries (written before this field existed, back
        /// when the changelog only ever covered `"ds"`) still deserialize.
        #[serde(default = "default_dataset_name")]
        dataset: String,
        /// `None` = default graph; `Some(iri)` = named graph.
        graph: Option<String>,
    },
    /// A quad was inserted.
    InsertQuad {
        #[serde(default = "default_dataset_name")]
        dataset: String,
        graph: Option<String>,
        s: ElementRepr,
        p: ElementRepr,
        o: ElementRepr,
    },
    /// A quad was deleted.
    DeleteQuad {
        #[serde(default = "default_dataset_name")]
        dataset: String,
        graph: Option<String>,
        s: ElementRepr,
        p: ElementRepr,
        o: ElementRepr,
    },
    /// `POST /{dataset}/rules` (#390/#568) — full ruleset replace.
    ///
    /// Recorded as raw Datalog source text, not parsed `Rule`s: a `Rule`'s
    /// `QuadPattern`s hold interned `GraphElementId`s that are not stable
    /// across a restart (a fresh `GraphElementManager` assigns ids in
    /// whatever order data happens to be re-loaded), exactly why the quad
    /// entries above use the portable `ElementRepr` rather than raw ids.
    /// `datalog_parser::parse` already interns IRIs into whatever store it's
    /// given as it parses, so replaying is simply "parse this text again
    /// against the replayed store". See
    /// [#475](https://github.com/daghovland/rdf-datalog/issues/475).
    ReplaceRuleset { dataset: String, rules_text: String },
    /// `POST /{dataset}/rules/{ruleset-id}` (#473) — named/scoped ruleset
    /// load or replace.
    SetNamedRuleset {
        dataset: String,
        ruleset_id: String,
        rules_text: String,
    },
    /// `DELETE /{dataset}/rules/{ruleset-id}` (#473) — named ruleset
    /// retraction.
    DeleteNamedRuleset { dataset: String, ruleset_id: String },
    /// `POST /$/datasets` (#469) — a new dataset was registered.
    ///
    /// Logged so an *empty* dataset (no quads ever written to it) still
    /// exists after a restart, and so [`QuadChangelog::replay_dataset`] and
    /// [`QuadChangelog::ruleset_entries`] can tell a name's mutation entries
    /// from *before* a delete+recreate cycle apart from entries from
    /// *after* it (see [#670](https://github.com/daghovland/rdf-datalog/issues/670)).
    CreateDataset { name: String },
    /// `DELETE /$/datasets/{name}` (#469) — a dataset was removed.
    DeleteDataset { name: String },
}

/// Returns `true` for a [`LogEntry`] variant that records a rules-endpoint
/// mutation (as opposed to a quad mutation) — see [`QuadChangelog::ruleset_entries`].
fn is_ruleset_entry(entry: &LogEntry) -> bool {
    matches!(
        entry,
        LogEntry::ReplaceRuleset { .. }
            | LogEntry::SetNamedRuleset { .. }
            | LogEntry::DeleteNamedRuleset { .. }
    )
}

/// The dataset name a quad-mutation or ruleset-mutation `LogEntry` applies
/// to. Returns `None` for [`LogEntry::CreateDataset`]/[`LogEntry::DeleteDataset`]
/// (which carry a `name`, not a `dataset`, since they're about dataset
/// *registration* rather than a mutation scoped to an existing one).
fn entry_dataset(entry: &LogEntry) -> Option<&str> {
    match entry {
        LogEntry::ClearGraph { dataset, .. }
        | LogEntry::InsertQuad { dataset, .. }
        | LogEntry::DeleteQuad { dataset, .. }
        | LogEntry::ReplaceRuleset { dataset, .. }
        | LogEntry::SetNamedRuleset { dataset, .. }
        | LogEntry::DeleteNamedRuleset { dataset, .. } => Some(dataset),
        LogEntry::CreateDataset { .. } | LogEntry::DeleteDataset { .. } => None,
    }
}

// ── QuadChangelog ─────────────────────────────────────────────────────────────

/// A durable append-only log of quad mutations backed by `redb`.
pub struct QuadChangelog {
    db: Database,
    next_seq: u64,
}

impl QuadChangelog {
    /// Open or create a changelog database at `path`.
    ///
    /// If the file exists, it is opened and its WAL replayed automatically by
    /// `redb`; the sequence counter is advanced past the last persisted entry.
    pub fn open(path: &Path) -> Result<Self, String> {
        let db = Database::create(path).map_err(|e| format!("redb open failed: {e}"))?;

        // Create the table if it doesn't exist yet.
        let setup_txn = db.begin_write().map_err(|e| e.to_string())?;
        setup_txn.open_table(QUAD_LOG).map_err(|e| e.to_string())?;
        setup_txn.commit().map_err(|e| e.to_string())?;

        // Find the sequence number of the last written entry.
        let next_seq = {
            let read_txn = db.begin_read().map_err(|e| e.to_string())?;
            let table = read_txn.open_table(QUAD_LOG).map_err(|e| e.to_string())?;
            match table.last().map_err(|e| e.to_string())? {
                Some((k, _)) => k.value() + 1,
                None => 0,
            }
        };

        Ok(Self { db, next_seq })
    }

    // ── Internal append ───────────────────────────────────────────────────────

    fn append_entry(&mut self, entry: &LogEntry) -> Result<(), String> {
        let bytes = serde_json::to_vec(entry).map_err(|e| e.to_string())?;
        let write_txn = self.db.begin_write().map_err(|e| e.to_string())?;
        {
            let mut table = write_txn.open_table(QUAD_LOG).map_err(|e| e.to_string())?;
            table
                .insert(self.next_seq, bytes.as_slice())
                .map_err(|e| e.to_string())?;
        }
        // commit() calls fsync — this is the durability boundary.
        write_txn.commit().map_err(|e| e.to_string())?;
        self.next_seq += 1;
        Ok(())
    }

    /// Append a batch of entries in a single write transaction (single fsync).
    ///
    /// All entries either all succeed or all fail (atomically).
    pub fn append_batch(&mut self, entries: &[LogEntry]) -> Result<(), String> {
        if entries.is_empty() {
            return Ok(());
        }
        let write_txn = self.db.begin_write().map_err(|e| e.to_string())?;
        {
            let mut table = write_txn.open_table(QUAD_LOG).map_err(|e| e.to_string())?;
            for entry in entries {
                let bytes = serde_json::to_vec(entry).map_err(|e| e.to_string())?;
                table
                    .insert(self.next_seq, bytes.as_slice())
                    .map_err(|e| e.to_string())?;
                self.next_seq += 1;
            }
        }
        write_txn.commit().map_err(|e| e.to_string())?;
        Ok(())
    }

    // ── Public mutation log operations ────────────────────────────────────────

    /// Durably record that a graph was cleared, in `dataset`.
    pub fn log_clear_graph(&mut self, dataset: &str, graph_iri: Option<&str>) -> Result<(), String> {
        self.append_entry(&LogEntry::ClearGraph {
            dataset: dataset.to_owned(),
            graph: graph_iri.map(str::to_owned),
        })
    }

    /// Durably record a single quad insertion into `dataset`.
    ///
    /// Production code uses `append_batch` for efficiency (one fsync per request).
    /// These single-entry helpers exist for unit tests where per-quad control matters.
    #[cfg(test)]
    pub fn log_insert_quad(
        &mut self,
        dataset: &str,
        graph_iri: Option<&str>,
        s: &GraphElement,
        p: &GraphElement,
        o: &GraphElement,
    ) -> Result<(), String> {
        self.append_entry(&LogEntry::InsertQuad {
            dataset: dataset.to_owned(),
            graph: graph_iri.map(str::to_owned),
            s: to_repr(s),
            p: to_repr(p),
            o: to_repr(o),
        })
    }

    /// Durably record a single quad deletion from `dataset`.
    ///
    /// Production code uses `append_batch`. This helper exists for unit tests.
    #[cfg(test)]
    pub fn log_delete_quad(
        &mut self,
        dataset: &str,
        graph_iri: Option<&str>,
        s: &GraphElement,
        p: &GraphElement,
        o: &GraphElement,
    ) -> Result<(), String> {
        self.append_entry(&LogEntry::DeleteQuad {
            dataset: dataset.to_owned(),
            graph: graph_iri.map(str::to_owned),
            s: to_repr(s),
            p: to_repr(p),
            o: to_repr(o),
        })
    }

    // ── Dataset registration log operations (#670) ─────────────────────────────

    /// Durably record `POST /$/datasets` creating dataset `name`.
    pub fn log_create_dataset(&mut self, name: &str) -> Result<(), String> {
        self.append_entry(&LogEntry::CreateDataset {
            name: name.to_owned(),
        })
    }

    /// Durably record `DELETE /$/datasets/{name}` removing dataset `name`.
    pub fn log_delete_dataset(&mut self, name: &str) -> Result<(), String> {
        self.append_entry(&LogEntry::DeleteDataset {
            name: name.to_owned(),
        })
    }

    // ── Ruleset mutation log operations (#475) ────────────────────────────────

    /// Durably record a `POST /{dataset}/rules` full ruleset replace.
    pub fn log_replace_ruleset(&mut self, dataset: &str, rules_text: &str) -> Result<(), String> {
        self.append_entry(&LogEntry::ReplaceRuleset {
            dataset: dataset.to_owned(),
            rules_text: rules_text.to_owned(),
        })
    }

    /// Durably record a `POST /{dataset}/rules/{ruleset-id}` named ruleset
    /// load/replace.
    pub fn log_set_named_ruleset(
        &mut self,
        dataset: &str,
        ruleset_id: &str,
        rules_text: &str,
    ) -> Result<(), String> {
        self.append_entry(&LogEntry::SetNamedRuleset {
            dataset: dataset.to_owned(),
            ruleset_id: ruleset_id.to_owned(),
            rules_text: rules_text.to_owned(),
        })
    }

    /// Durably record a `DELETE /{dataset}/rules/{ruleset-id}` named ruleset
    /// retraction.
    pub fn log_delete_named_ruleset(
        &mut self,
        dataset: &str,
        ruleset_id: &str,
    ) -> Result<(), String> {
        self.append_entry(&LogEntry::DeleteNamedRuleset {
            dataset: dataset.to_owned(),
            ruleset_id: ruleset_id.to_owned(),
        })
    }

    /// Read back every entry in the log, alongside its sequence key, in
    /// original append order.
    fn all_entries(&self) -> Result<Vec<(u64, LogEntry)>, String> {
        let read_txn = self.db.begin_read().map_err(|e| e.to_string())?;
        let table = read_txn.open_table(QUAD_LOG).map_err(|e| e.to_string())?;

        let mut result = Vec::new();
        for row in table.iter().map_err(|e| e.to_string())? {
            let (key, bytes) = row.map_err(|e| e.to_string())?;
            let entry: LogEntry =
                serde_json::from_slice(bytes.value()).map_err(|e| e.to_string())?;
            result.push((key.value(), entry));
        }
        Ok(result)
    }

    /// Read back every ruleset-mutation log entry, in original append order,
    /// filtering out the quad-mutation entries. For a non-`"ds"` dataset,
    /// also drops any entry from *before* that dataset's most recent
    /// `CreateDataset` marker, so a delete+recreate cycle (#670) doesn't
    /// resurrect a stale ruleset from the deleted incarnation.
    ///
    /// Called once at startup (after quad-entry replay and initial-reasoner
    /// construction) to reconstruct any runtime-loaded rulesets. See
    /// [`docs/plans/PERSIST_RULESETS_475_PLAN.md`](https://github.com/daghovland/rdf-datalog/blob/main/docs/plans/PERSIST_RULESETS_475_PLAN.md).
    pub fn ruleset_entries(&self) -> Result<Vec<LogEntry>, String> {
        let entries = self.all_entries()?;
        let cutoffs = create_cutoffs(&entries);
        Ok(entries
            .into_iter()
            .filter(|(seq, entry)| {
                is_ruleset_entry(entry)
                    && entry_dataset(entry)
                        .is_some_and(|d| *seq >= cutoffs.get(d).copied().unwrap_or(0))
            })
            .map(|(_, entry)| entry)
            .collect())
    }

    /// The set of dataset names (other than the always-present `"ds"`) that
    /// are currently live: created via a `CreateDataset` entry and not
    /// subsequently removed by a later `DeleteDataset` entry for the same
    /// name. See [#670](https://github.com/daghovland/rdf-datalog/issues/670).
    pub fn discover_dataset_names(&self) -> Result<Vec<String>, String> {
        let entries = self.all_entries()?;
        let mut live: std::collections::BTreeMap<String, bool> = std::collections::BTreeMap::new();
        for (_, entry) in &entries {
            match entry {
                LogEntry::CreateDataset { name } => {
                    live.insert(name.clone(), true);
                }
                LogEntry::DeleteDataset { name } => {
                    live.insert(name.clone(), false);
                }
                _ => {}
            }
        }
        Ok(live
            .into_iter()
            .filter_map(|(name, is_live)| is_live.then_some(name))
            .collect())
    }

    // ── Compaction ────────────────────────────────────────────────────────────

    /// Atomically rewrite the log to contain only the current live quads for
    /// every dataset passed in `datasets` (name, its `Datastore`), plus every
    /// preserved ruleset-mutation entry still relevant to one of those
    /// datasets.
    ///
    /// Returns `(entries_before, entries_after)`. Every non-`"ds"` dataset in
    /// `datasets` also gets a fresh `CreateDataset` marker written, so an
    /// empty dataset's existence survives compaction even though it
    /// contributes no `InsertQuad` entries. Any dataset *not* in `datasets`
    /// (e.g. one that was deleted) has its residual log entries dropped by
    /// this rewrite, which is the correct behavior, not a side effect to
    /// work around: a deleted dataset's history has no live consumer left.
    pub fn compact_multi(&mut self, datasets: &[(&str, &Datastore)]) -> Result<(u64, u64), String> {
        let entries_before = {
            let read_txn = self.db.begin_read().map_err(|e| e.to_string())?;
            let table = read_txn.open_table(QUAD_LOG).map_err(|e| e.to_string())?;
            table.len().map_err(|e| e.to_string())?
        };

        let live_names: std::collections::HashSet<&str> =
            datasets.iter().map(|(name, _)| *name).collect();

        // Ruleset-mutation entries (#475) are not quad state and have no
        // "current snapshot" equivalent -- they must survive compaction
        // unchanged (in original order) rather than being discarded along
        // with the superseded quad-mutation history. Only entries for a
        // dataset still present in `datasets` are kept.
        let preserved_ruleset_entries: Vec<LogEntry> = self
            .ruleset_entries()?
            .into_iter()
            .filter(|e| entry_dataset(e).is_some_and(|d| live_names.contains(d)))
            .collect();

        let mut new_entries: Vec<LogEntry> = Vec::new();
        for (name, ds) in datasets {
            if *name != "ds" {
                new_entries.push(LogEntry::CreateDataset {
                    name: (*name).to_string(),
                });
            }
            for quad in ds.named_graphs.get_all_quads() {
                let graph = if quad.triple_id == DEFAULT_GRAPH_ELEMENT_ID {
                    None
                } else {
                    match ds.resources.get_graph_element(quad.triple_id) {
                        GraphElement::NodeOrEdge(RdfResource::Iri(iri)) => Some(iri.0.clone()),
                        _ => None,
                    }
                };
                new_entries.push(LogEntry::InsertQuad {
                    dataset: (*name).to_string(),
                    graph,
                    s: to_repr(ds.resources.get_graph_element(quad.subject)),
                    p: to_repr(ds.resources.get_graph_element(quad.predicate)),
                    o: to_repr(ds.resources.get_graph_element(quad.obj)),
                });
            }
        }
        new_entries.extend(preserved_ruleset_entries);

        let entries_after = new_entries.len() as u64;

        let write_txn = self.db.begin_write().map_err(|e| e.to_string())?;
        {
            let mut table = write_txn.open_table(QUAD_LOG).map_err(|e| e.to_string())?;
            let old_keys: Vec<u64> = table
                .iter()
                .map_err(|e| e.to_string())?
                .map(|r| r.map(|(k, _)| k.value()).map_err(|e| e.to_string()))
                .collect::<Result<_, _>>()?;
            for key in old_keys {
                table.remove(key).map_err(|e| e.to_string())?;
            }
            for (i, entry) in new_entries.iter().enumerate() {
                let bytes = serde_json::to_vec(entry).map_err(|e| e.to_string())?;
                table
                    .insert(i as u64, bytes.as_slice())
                    .map_err(|e| e.to_string())?;
            }
        }
        write_txn.commit().map_err(|e| e.to_string())?;
        self.next_seq = entries_after;

        Ok((entries_before, entries_after))
    }

    // ── Replay ────────────────────────────────────────────────────────────────

    /// Replay all log entries into a fresh `Datastore`.
    ///
    /// Called once at startup to reconstruct the in-memory store from the durable log.
    /// The log entry count is used as a size hint to pre-allocate the Datastore.
    #[cfg(test)]
    pub fn replay(&self) -> Result<Datastore, String> {
        let entries = self.all_entries()?;
        let size_hint = (entries.len() as u32).max(1024);
        let mut ds = Datastore::new(size_hint);
        for (_, entry) in &entries {
            apply_entry(&mut ds, entry);
        }
        Ok(ds)
    }

    /// Replay all log entries tagged for dataset `"ds"` INTO an existing
    /// `Datastore`, layering changelog mutations on top of any data already
    /// in `ds` (e.g., loaded from files).
    ///
    /// This is used at startup when `--data` files are pre-loaded before enabling
    /// persistence: the changelog records HTTP-driven mutations that happened during
    /// previous runs and must be applied on top of the file-based base data.
    /// See: <https://github.com/daghovland/rdf-datalog/issues/66>
    pub fn replay_into(&self, ds: &mut Datastore) -> Result<(), String> {
        for (_, entry) in self.all_entries()? {
            if entry_dataset(&entry) == Some("ds") {
                apply_entry(ds, &entry);
            }
        }
        Ok(())
    }

    /// Replay all quad-mutation log entries tagged for `name` (a non-`"ds"`
    /// dataset registered via `POST /$/datasets`) into a fresh `Datastore`.
    ///
    /// Only entries at or after `name`'s most recent `CreateDataset` marker
    /// are applied, so a delete+recreate cycle for the same name (#670)
    /// starts empty rather than resurrecting quads from the deleted
    /// incarnation. Callers are expected to only call this for names
    /// returned by [`Self::discover_dataset_names`].
    pub fn replay_dataset(&self, name: &str) -> Result<Datastore, String> {
        let entries = self.all_entries()?;
        let cutoffs = create_cutoffs(&entries);
        let cutoff = cutoffs.get(name).copied().unwrap_or(0);
        let mut ds = Datastore::new(1024);
        for (seq, entry) in &entries {
            if *seq >= cutoff && entry_dataset(entry) == Some(name) {
                apply_entry(&mut ds, entry);
            }
        }
        Ok(ds)
    }
}

/// For every dataset name that has at least one `CreateDataset` entry,
/// the sequence key of its *most recent* such entry — the cutoff below
/// which entries for that name belong to a since-deleted-and-recreated
/// incarnation and must be ignored. A name with no `CreateDataset` entry at
/// all (in practice, only `"ds"`, which exists implicitly from server
/// startup rather than via `POST /$/datasets`) has no cutoff, i.e. every
/// entry for it is considered.
fn create_cutoffs(entries: &[(u64, LogEntry)]) -> std::collections::HashMap<String, u64> {
    let mut cutoffs = std::collections::HashMap::new();
    for (seq, entry) in entries {
        if let LogEntry::CreateDataset { name } = entry {
            cutoffs.insert(name.clone(), *seq);
        }
    }
    cutoffs
}

// ── Entry application (replay logic) ─────────────────────────────────────────

fn apply_entry(ds: &mut Datastore, entry: &LogEntry) {
    match entry {
        LogEntry::ClearGraph { graph, .. } => {
            let graph_id = graph_id_for(ds, graph.as_deref());
            ds.remove_graph(graph_id);
        }
        LogEntry::InsertQuad { graph, s, p, o, .. } => {
            let graph_id = graph_id_for(ds, graph.as_deref());
            let s_id = ds.add_resource(from_repr(s));
            let p_id = ds.add_resource(from_repr(p));
            let o_id = ds.add_resource(from_repr(o));
            ds.named_graphs.add_quad(Quad {
                triple_id: graph_id,
                subject: s_id,
                predicate: p_id,
                obj: o_id,
            });
        }
        LogEntry::DeleteQuad { graph, s, p, o, .. } => {
            let graph_id = match graph {
                None => DEFAULT_GRAPH_ELEMENT_ID,
                Some(iri) => match ds.lookup_named_graph_id(iri) {
                    Some(id) => id,
                    None => return, // graph never existed
                },
            };
            let s_el = from_repr(s);
            let p_el = from_repr(p);
            let o_el = from_repr(o);
            let s_id = match ds.resources.resource_map.get(&s_el) {
                Some(&id) => id,
                None => return,
            };
            let p_id = match ds.resources.resource_map.get(&p_el) {
                Some(&id) => id,
                None => return,
            };
            let o_id = match ds.resources.resource_map.get(&o_el) {
                Some(&id) => id,
                None => return,
            };
            ds.remove_quad(Quad {
                triple_id: graph_id,
                subject: s_id,
                predicate: p_id,
                obj: o_id,
            });
        }
        // Ruleset-mutation entries (#475) don't apply to a bare `Datastore`
        // -- they need registry-level state (a dataset's reasoner + named
        // ruleset bookkeeping) and are replayed separately at startup via
        // `QuadChangelog::ruleset_entries`, after quad replay completes.
        //
        // Dataset-registration entries (#670) likewise don't apply to a bare
        // `Datastore` -- they're consumed by `QuadChangelog::discover_dataset_names`
        // at the `DatasetRegistry` level, before any per-dataset `Datastore`
        // (this one included) is even created.
        LogEntry::ReplaceRuleset { .. }
        | LogEntry::SetNamedRuleset { .. }
        | LogEntry::DeleteNamedRuleset { .. }
        | LogEntry::CreateDataset { .. }
        | LogEntry::DeleteDataset { .. } => {}
    }
}

fn graph_id_for(ds: &mut Datastore, graph_iri: Option<&str>) -> dag_rdf::GraphElementId {
    match graph_iri {
        None => DEFAULT_GRAPH_ELEMENT_ID,
        Some(iri) => ds.add_resource(GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(
            iri.to_owned(),
        )))),
    }
}

// ── Conversion helpers ────────────────────────────────────────────────────────

pub fn to_repr(el: &GraphElement) -> ElementRepr {
    match el {
        GraphElement::NodeOrEdge(RdfResource::Iri(iri)) => ElementRepr::Iri(iri.0.clone()),
        GraphElement::NodeOrEdge(RdfResource::AnonymousBlankNode(n)) => ElementRepr::Blank(*n),
        GraphElement::GraphLiteral(lit) => lit_to_repr(lit),
        // Triple terms: persistence support tracked in #143; emit as blank node placeholder.
        GraphElement::TripleTerm(k) => ElementRepr::Blank(k.subject),
    }
}

fn lit_to_repr(lit: &RdfLiteral) -> ElementRepr {
    match lit {
        RdfLiteral::LiteralString(s) => ElementRepr::LiteralPlain(s.clone()),
        RdfLiteral::LangLiteral { literal, lang } => ElementRepr::LiteralLang {
            lexical: literal.clone(),
            lang: lang.clone(),
        },
        RdfLiteral::TypedLiteral { literal, type_iri } => ElementRepr::LiteralTyped {
            lexical: literal.clone(),
            datatype: type_iri.0.clone(),
        },
        RdfLiteral::BooleanLiteral(b) => ElementRepr::LiteralTyped {
            lexical: b.to_string(),
            datatype: XSD_BOOLEAN.to_string(),
        },
        RdfLiteral::DecimalLiteral(d) => ElementRepr::LiteralTyped {
            lexical: d.to_string(),
            datatype: XSD_DECIMAL.to_string(),
        },
        RdfLiteral::FloatLiteral(f) => ElementRepr::LiteralTyped {
            lexical: f.to_string(),
            datatype: XSD_FLOAT.to_string(),
        },
        RdfLiteral::DoubleLiteral(d) => ElementRepr::LiteralTyped {
            lexical: d.to_string(),
            datatype: XSD_DOUBLE.to_string(),
        },
        RdfLiteral::DurationLiteral(dur) => ElementRepr::LiteralTyped {
            lexical: format!("{:?}", dur),
            datatype: XSD_DURATION.to_string(),
        },
        RdfLiteral::IntegerLiteral(n) => ElementRepr::LiteralTyped {
            lexical: n.to_string(),
            datatype: XSD_INTEGER.to_string(),
        },
        RdfLiteral::DateTimeLiteral(dt) => ElementRepr::LiteralTyped {
            lexical: dt.to_rfc3339(),
            datatype: XSD_DATE_TIME.to_string(),
        },
        RdfLiteral::TimeLiteral(t) => ElementRepr::LiteralTyped {
            lexical: t.to_string(),
            datatype: XSD_TIME.to_string(),
        },
        RdfLiteral::DateLiteral(d) => ElementRepr::LiteralTyped {
            lexical: d.to_string(),
            datatype: XSD_DATE.to_string(),
        },
    }
}

fn from_repr(repr: &ElementRepr) -> GraphElement {
    match repr {
        ElementRepr::Iri(iri) => {
            GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(iri.clone())))
        }
        ElementRepr::Blank(n) => GraphElement::NodeOrEdge(RdfResource::AnonymousBlankNode(*n)),
        ElementRepr::LiteralPlain(s) => {
            GraphElement::GraphLiteral(RdfLiteral::LiteralString(s.clone()))
        }
        ElementRepr::LiteralLang { lexical, lang } => {
            GraphElement::GraphLiteral(RdfLiteral::LangLiteral {
                literal: lexical.clone(),
                lang: lang.clone(),
            })
        }
        ElementRepr::LiteralTyped { lexical, datatype } => {
            GraphElement::GraphLiteral(RdfLiteral::TypedLiteral {
                literal: lexical.clone(),
                type_iri: IriReference(datatype.clone()),
            })
        }
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use dag_rdf::{IriReference, RdfResource};
    use tempfile::tempdir;

    fn iri(s: &str) -> GraphElement {
        GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(s.to_owned())))
    }

    fn lit(s: &str) -> GraphElement {
        GraphElement::GraphLiteral(RdfLiteral::LiteralString(s.to_owned()))
    }

    #[test]
    fn changelog_roundtrip_insert() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.redb");

        // Write a quad.
        {
            let mut cl = QuadChangelog::open(&db_path).unwrap();
            cl.log_insert_quad(
                "ds",
            None,
                &iri("http://example.org/s"),
                &iri("http://example.org/p"),
                &lit("hello"),
            )
            .unwrap();
        }

        // Reopen and replay.
        let cl = QuadChangelog::open(&db_path).unwrap();
        let ds = cl.replay().unwrap();

        let s_id = ds
            .resources
            .resource_map
            .get(&iri("http://example.org/s"))
            .copied()
            .expect("subject not interned");
        let triples: Vec<_> = ds.get_triples_with_subject(s_id).collect();
        assert_eq!(triples.len(), 1, "expected exactly one triple");
    }

    #[test]
    fn changelog_roundtrip_clear() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.redb");

        {
            let mut cl = QuadChangelog::open(&db_path).unwrap();
            // Insert then clear.
            cl.log_insert_quad(
                "ds",
            None,
                &iri("http://example.org/s"),
                &iri("http://example.org/p"),
                &lit("hello"),
            )
            .unwrap();
            cl.log_clear_graph("ds", None).unwrap();
        }

        let cl = QuadChangelog::open(&db_path).unwrap();
        let ds = cl.replay().unwrap();

        // After clear, no quads should remain.
        let all: Vec<_> = ds.named_graphs.get_all_quads().collect();
        assert!(all.is_empty(), "all quads should be gone after clear");
    }

    #[test]
    fn changelog_batch_append() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.redb");

        {
            let mut cl = QuadChangelog::open(&db_path).unwrap();
            let entries = vec![
                LogEntry::InsertQuad {
                    dataset: "ds".to_string(),
                    graph: None,
                    s: to_repr(&iri("http://example.org/a")),
                    p: to_repr(&iri("http://example.org/p")),
                    o: to_repr(&lit("one")),
                },
                LogEntry::InsertQuad {
                    dataset: "ds".to_string(),
                    graph: None,
                    s: to_repr(&iri("http://example.org/b")),
                    p: to_repr(&iri("http://example.org/p")),
                    o: to_repr(&lit("two")),
                },
            ];
            cl.append_batch(&entries).unwrap();
        }

        let cl = QuadChangelog::open(&db_path).unwrap();
        let ds = cl.replay().unwrap();
        let all: Vec<_> = ds.named_graphs.get_all_quads().collect();
        assert_eq!(all.len(), 2);
    }

    // ── replay_into ───────────────────────────────────────────────────────────

    #[test]
    fn replay_into_layers_on_existing_datastore() {
        use dag_rdf::ingress::DEFAULT_GRAPH_ELEMENT_ID;

        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.redb");

        {
            let mut cl = QuadChangelog::open(&db_path).unwrap();
            cl.log_insert_quad(
                "ds",
            None,
                &iri("http://example.org/changelog"),
                &iri("http://example.org/p"),
                &lit("from changelog"),
            )
            .unwrap();
        }

        // Build an existing store with a different triple already in it.
        let cl = QuadChangelog::open(&db_path).unwrap();
        let mut ds = Datastore::new(64);
        let s_id = ds.add_resource(iri("http://example.org/preloaded"));
        let p_id = ds.add_resource(iri("http://example.org/p"));
        let o_id = ds.add_resource(GraphElement::GraphLiteral(RdfLiteral::LiteralString(
            "from preload".to_owned(),
        )));
        ds.named_graphs.add_quad(dag_rdf::ingress::Quad {
            triple_id: DEFAULT_GRAPH_ELEMENT_ID,
            subject: s_id,
            predicate: p_id,
            obj: o_id,
        });

        cl.replay_into(&mut ds).unwrap();

        // Both the preloaded triple and the changelog triple must be present.
        let all: Vec<_> = ds.named_graphs.get_all_quads().collect();
        assert_eq!(all.len(), 2, "preloaded + changelog quad both present");

        let cl_s = ds
            .resources
            .resource_map
            .get(&iri("http://example.org/changelog"))
            .copied()
            .expect("changelog subject should be interned");
        let triples: Vec<_> = ds.get_triples_with_subject(cl_s).collect();
        assert_eq!(triples.len(), 1, "changelog quad accessible by subject");
    }

    // ── Named-graph operations ────────────────────────────────────────────────

    #[test]
    fn named_graph_insert_roundtrip() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.redb");
        let graph_iri = "http://example.org/g1";

        {
            let mut cl = QuadChangelog::open(&db_path).unwrap();
            cl.log_insert_quad(
                "ds",
            Some(graph_iri),
                &iri("http://example.org/s"),
                &iri("http://example.org/p"),
                &lit("named"),
            )
            .unwrap();
        }

        let cl = QuadChangelog::open(&db_path).unwrap();
        let ds = cl.replay().unwrap();
        let all: Vec<_> = ds.named_graphs.get_all_quads().collect();
        assert_eq!(all.len(), 1, "one quad in named graph");

        // The graph element must be interned as the named-graph IRI.
        let g_el = GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(graph_iri.to_owned())));
        assert!(
            ds.resources.resource_map.contains_key(&g_el),
            "named graph IRI should be interned"
        );
    }

    #[test]
    fn named_graph_clear_removes_only_that_graph() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.redb");

        {
            let mut cl = QuadChangelog::open(&db_path).unwrap();
            // Insert one quad in the default graph and one in a named graph.
            cl.log_insert_quad(
                "ds",
            None,
                &iri("http://example.org/default_s"),
                &iri("http://example.org/p"),
                &lit("default"),
            )
            .unwrap();
            cl.log_insert_quad(
                "ds",
            Some("http://example.org/g1"),
                &iri("http://example.org/named_s"),
                &iri("http://example.org/p"),
                &lit("named"),
            )
            .unwrap();
            // Clear only the named graph.
            cl.log_clear_graph("ds", Some("http://example.org/g1")).unwrap();
        }

        let cl = QuadChangelog::open(&db_path).unwrap();
        let ds = cl.replay().unwrap();
        let all: Vec<_> = ds.named_graphs.get_all_quads().collect();
        assert_eq!(
            all.len(),
            1,
            "only default-graph quad should remain after clearing named graph"
        );
    }

    // ── delete edge cases ─────────────────────────────────────────────────────

    #[test]
    fn delete_nonexistent_quad_is_noop() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.redb");

        {
            let mut cl = QuadChangelog::open(&db_path).unwrap();
            // Delete a quad that was never inserted — must not panic.
            cl.log_delete_quad(
                "ds",
            None,
                &iri("http://example.org/ghost_s"),
                &iri("http://example.org/p"),
                &lit("ghost"),
            )
            .unwrap();
        }

        let cl = QuadChangelog::open(&db_path).unwrap();
        let ds = cl.replay().unwrap();
        let all: Vec<_> = ds.named_graphs.get_all_quads().collect();
        assert!(all.is_empty(), "nothing to delete: store stays empty");
    }

    #[test]
    fn delete_quad_in_nonexistent_named_graph_is_noop() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.redb");

        {
            let mut cl = QuadChangelog::open(&db_path).unwrap();
            // Delete from a named graph that was never created.
            cl.log_delete_quad(
                "ds",
                Some("http://example.org/no_such_graph"),
                &iri("http://example.org/s"),
                &iri("http://example.org/p"),
                &lit("v"),
            )
            .unwrap();
        }

        let cl = QuadChangelog::open(&db_path).unwrap();
        let ds = cl.replay().unwrap();
        let all: Vec<_> = ds.named_graphs.get_all_quads().collect();
        assert!(all.is_empty(), "delete in missing graph must be a no-op");
    }

    #[test]
    fn insert_delete_reinsert_same_quad_is_present() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.redb");

        let s = iri("http://example.org/s");
        let p = iri("http://example.org/p");
        let o = lit("value");

        {
            let mut cl = QuadChangelog::open(&db_path).unwrap();
            cl.log_insert_quad("ds", None, &s, &p, &o).unwrap();
            cl.log_delete_quad("ds", None, &s, &p, &o).unwrap();
            cl.log_insert_quad("ds", None, &s, &p, &o).unwrap();
        }

        let cl = QuadChangelog::open(&db_path).unwrap();
        let ds = cl.replay().unwrap();
        let all: Vec<_> = ds.named_graphs.get_all_quads().collect();
        assert_eq!(
            all.len(),
            1,
            "quad must be present after insert-delete-insert"
        );
    }

    // ── element representation round-trips ───────────────────────────────────

    #[test]
    fn blank_node_repr_roundtrip() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.redb");

        let blank = GraphElement::NodeOrEdge(RdfResource::AnonymousBlankNode(42));

        {
            let mut cl = QuadChangelog::open(&db_path).unwrap();
            cl.log_insert_quad("ds", None, &blank, &iri("http://example.org/p"), &lit("v"))
                .unwrap();
        }

        let cl = QuadChangelog::open(&db_path).unwrap();
        let ds = cl.replay().unwrap();
        let all: Vec<_> = ds.named_graphs.get_all_quads().collect();
        assert_eq!(all.len(), 1, "blank-node quad must survive replay");

        // The blank node must be interned with the same ID.
        assert!(
            ds.resources.resource_map.contains_key(&blank),
            "blank node b42 must be interned"
        );
    }

    #[test]
    fn lang_literal_repr_roundtrip() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.redb");

        let lang_lit = GraphElement::GraphLiteral(RdfLiteral::LangLiteral {
            literal: "Bonjour".to_owned(),
            lang: "fr".to_owned(),
        });

        {
            let mut cl = QuadChangelog::open(&db_path).unwrap();
            cl.log_insert_quad(
                "ds",
            None,
                &iri("http://example.org/s"),
                &iri("http://example.org/label"),
                &lang_lit,
            )
            .unwrap();
        }

        let cl = QuadChangelog::open(&db_path).unwrap();
        let ds = cl.replay().unwrap();
        let all: Vec<_> = ds.named_graphs.get_all_quads().collect();
        assert_eq!(all.len(), 1, "lang-literal quad must survive replay");
        assert!(
            ds.resources.resource_map.contains_key(&lang_lit),
            "lang literal 'Bonjour'@fr must be interned after replay"
        );
    }

    // ── sequence counter continuity ───────────────────────────────────────────

    #[test]
    fn sequence_counter_resumes_after_reopen() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.redb");

        // First session: write 2 entries.
        {
            let mut cl = QuadChangelog::open(&db_path).unwrap();
            cl.log_insert_quad(
                "ds",
            None,
                &iri("http://example.org/a"),
                &iri("http://example.org/p"),
                &lit("1"),
            )
            .unwrap();
            cl.log_insert_quad(
                "ds",
            None,
                &iri("http://example.org/b"),
                &iri("http://example.org/p"),
                &lit("2"),
            )
            .unwrap();
        }

        // Second session: write 2 more entries.
        {
            let mut cl = QuadChangelog::open(&db_path).unwrap();
            cl.log_insert_quad(
                "ds",
            None,
                &iri("http://example.org/c"),
                &iri("http://example.org/p"),
                &lit("3"),
            )
            .unwrap();
            cl.log_insert_quad(
                "ds",
            None,
                &iri("http://example.org/d"),
                &iri("http://example.org/p"),
                &lit("4"),
            )
            .unwrap();
        }

        // All 4 entries must be present (no overwrites from seq-counter restart).
        let cl = QuadChangelog::open(&db_path).unwrap();
        let ds = cl.replay().unwrap();
        let all: Vec<_> = ds.named_graphs.get_all_quads().collect();
        assert_eq!(
            all.len(),
            4,
            "all 4 quads across two sessions must survive; seq counter must not restart"
        );
    }

    // ── empty batch ───────────────────────────────────────────────────────────

    #[test]
    fn empty_batch_is_ok_and_does_not_advance_sequence() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.redb");

        let mut cl = QuadChangelog::open(&db_path).unwrap();
        let seq_before = cl.next_seq;
        cl.append_batch(&[]).unwrap();
        assert_eq!(
            cl.next_seq, seq_before,
            "empty batch must not advance the sequence counter"
        );
    }

    // ── multi-dataset (#670) ─────────────────────────────────────────────────

    /// A pre-#670 on-disk entry has no `dataset` field at all. It must still
    /// deserialize (as dataset `"ds"`) rather than refusing to start.
    #[test]
    fn old_format_entry_without_dataset_field_deserializes_as_ds() {
        let old_json = r#"{"InsertQuad":{"graph":null,"s":{"Iri":"http://example.org/s"},"p":{"Iri":"http://example.org/p"},"o":{"LiteralPlain":"hello"}}}"#;
        let entry: LogEntry = serde_json::from_str(old_json).unwrap();
        match &entry {
            LogEntry::InsertQuad { dataset, .. } => assert_eq!(dataset, "ds"),
            other => panic!("expected InsertQuad, got {other:?}"),
        }
        assert_eq!(entry_dataset(&entry), Some("ds"));
    }

    /// A whole redb table written by a pre-#670 build (rows with no `dataset`
    /// field) must still replay correctly into `"ds"` via `replay_into`.
    #[test]
    fn old_format_table_replays_into_ds() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.redb");

        {
            let mut cl = QuadChangelog::open(&db_path).unwrap();
            let old_json = br#"{"InsertQuad":{"graph":null,"s":{"Iri":"http://example.org/s"},"p":{"Iri":"http://example.org/p"},"o":{"LiteralPlain":"hello"}}}"#;
            let write_txn = cl.db.begin_write().unwrap();
            {
                let mut table = write_txn.open_table(QUAD_LOG).unwrap();
                table.insert(0u64, old_json.as_slice()).unwrap();
            }
            write_txn.commit().unwrap();
            cl.next_seq = 1;
        }

        let cl = QuadChangelog::open(&db_path).unwrap();
        let mut ds = Datastore::new(64);
        cl.replay_into(&mut ds).unwrap();
        let all: Vec<_> = ds.named_graphs.get_all_quads().collect();
        assert_eq!(all.len(), 1, "old-format entry must replay into 'ds'");
    }

    #[test]
    fn writes_to_different_datasets_do_not_cross_contaminate_on_replay() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.redb");

        {
            let mut cl = QuadChangelog::open(&db_path).unwrap();
            cl.log_create_dataset("foo").unwrap();
            cl.log_insert_quad(
                "ds",
                None,
                &iri("http://example.org/ds-s"),
                &iri("http://example.org/p"),
                &lit("in-ds"),
            )
            .unwrap();
            cl.log_insert_quad(
                "foo",
                None,
                &iri("http://example.org/foo-s"),
                &iri("http://example.org/p"),
                &lit("in-foo"),
            )
            .unwrap();
        }

        let cl = QuadChangelog::open(&db_path).unwrap();
        let mut ds_store = Datastore::new(64);
        cl.replay_into(&mut ds_store).unwrap();
        let ds_quads: Vec<_> = ds_store.named_graphs.get_all_quads().collect();
        assert_eq!(ds_quads.len(), 1, "'ds' must only see its own quad");

        let foo_store = cl.replay_dataset("foo").unwrap();
        let foo_quads: Vec<_> = foo_store.named_graphs.get_all_quads().collect();
        assert_eq!(foo_quads.len(), 1, "'foo' must only see its own quad");
        assert!(
            foo_store
                .resources
                .resource_map
                .contains_key(&iri("http://example.org/foo-s")),
            "'foo' quad must be the one tagged for it"
        );
    }

    #[test]
    fn discover_dataset_names_reflects_create_and_delete() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.redb");

        let mut cl = QuadChangelog::open(&db_path).unwrap();
        cl.log_create_dataset("alive").unwrap();
        cl.log_create_dataset("dead").unwrap();
        cl.log_delete_dataset("dead").unwrap();

        let names = cl.discover_dataset_names().unwrap();
        assert_eq!(names, vec!["alive".to_string()]);
    }

    #[test]
    fn delete_then_recreate_does_not_resurrect_old_quads() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.redb");

        let mut cl = QuadChangelog::open(&db_path).unwrap();
        cl.log_create_dataset("cycle").unwrap();
        cl.log_insert_quad(
            "cycle",
            None,
            &iri("http://example.org/old"),
            &iri("http://example.org/p"),
            &lit("stale"),
        )
        .unwrap();
        cl.log_delete_dataset("cycle").unwrap();
        cl.log_create_dataset("cycle").unwrap();

        let names = cl.discover_dataset_names().unwrap();
        assert_eq!(names, vec!["cycle".to_string()]);

        let store = cl.replay_dataset("cycle").unwrap();
        let all: Vec<_> = store.named_graphs.get_all_quads().collect();
        assert!(
            all.is_empty(),
            "recreated dataset must not resurrect quads from its deleted incarnation"
        );
    }

    #[test]
    fn compact_multi_preserves_every_dataset() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.redb");

        let mut ds_store = Datastore::new(64);
        let s = ds_store.add_resource(iri("http://example.org/ds-s"));
        let p = ds_store.add_resource(iri("http://example.org/p"));
        let o = ds_store.add_resource(lit("in-ds"));
        ds_store.named_graphs.add_quad(dag_rdf::ingress::Quad {
            triple_id: DEFAULT_GRAPH_ELEMENT_ID,
            subject: s,
            predicate: p,
            obj: o,
        });

        let mut foo_store = Datastore::new(64);
        let s2 = foo_store.add_resource(iri("http://example.org/foo-s"));
        let p2 = foo_store.add_resource(iri("http://example.org/p"));
        let o2 = foo_store.add_resource(lit("in-foo"));
        foo_store.named_graphs.add_quad(dag_rdf::ingress::Quad {
            triple_id: DEFAULT_GRAPH_ELEMENT_ID,
            subject: s2,
            predicate: p2,
            obj: o2,
        });

        let empty_store = Datastore::new(64);

        {
            let mut cl = QuadChangelog::open(&db_path).unwrap();
            cl.log_create_dataset("foo").unwrap();
            cl.log_create_dataset("empty1").unwrap();
            cl.compact_multi(&[
                ("ds", &ds_store),
                ("foo", &foo_store),
                ("empty1", &empty_store),
            ])
            .unwrap();
        }

        let cl = QuadChangelog::open(&db_path).unwrap();
        let names = cl.discover_dataset_names().unwrap();
        assert_eq!(names.len(), 2, "both non-ds datasets must survive compaction");
        assert!(names.contains(&"foo".to_string()));
        assert!(names.contains(&"empty1".to_string()));

        let mut replayed_ds = Datastore::new(64);
        cl.replay_into(&mut replayed_ds).unwrap();
        assert_eq!(replayed_ds.named_graphs.get_all_quads().count(), 1);

        let replayed_foo = cl.replay_dataset("foo").unwrap();
        assert_eq!(replayed_foo.named_graphs.get_all_quads().count(), 1);

        let replayed_empty = cl.replay_dataset("empty1").unwrap();
        assert_eq!(replayed_empty.named_graphs.get_all_quads().count(), 0);
    }
}

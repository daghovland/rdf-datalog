use dag_rdf::Datastore;

/// Per-kernel-session state: one `Datastore` shared across all cells.
pub struct KernelSession {
    pub datastore: Datastore,
    pub execution_count: u64,
}

impl KernelSession {
    pub fn new() -> Self {
        Self {
            datastore: Datastore::new(1_000_000),
            execution_count: 0,
        }
    }
}

impl Default for KernelSession {
    fn default() -> Self {
        Self::new()
    }
}

//! The salsa writer.
//!
//! [`WriterHandle`] owns the master [`SalsaDb`](crate::salsa::SalsaDb). Today it
//! is a thin pass-through wrapper still living inside [`GlobalState`], mutated on
//! the main event loop exactly as before. It exists to concentrate every
//! salsa-touching access behind one type so a later phase can relocate the
//! database onto a dedicated writer thread without re-threading call sites: the
//! main loop will keep a read-only clone for minting snapshots while the owned
//! handle moves off-thread.
//!
//! [`GlobalState`]: crate::lsp::global_state::GlobalState

use crate::salsa::{Analysis, SalsaDb};

/// Owns the master salsa database handle.
///
/// Reads clone the handle (`Analysis`, a cheap `Arc` bump over the shared
/// `salsa::Storage`); writes go through [`db_mut`](Self::db_mut). The wrapper
/// carries no other state yet — the salsa-input side of the document map and the
/// settle machinery migrate here in later phases.
pub(crate) struct WriterHandle {
    db: SalsaDb,
}

impl WriterHandle {
    /// A fresh writer over a default (empty) database.
    pub(crate) fn new() -> Self {
        Self {
            db: SalsaDb::default(),
        }
    }

    /// Shared read access to the database.
    pub(crate) fn db(&self) -> &SalsaDb {
        &self.db
    }

    /// Exclusive write access to the database. The `&mut` borrow is what salsa
    /// uses to cancel any in-flight reads on cloned handles.
    pub(crate) fn db_mut(&mut self) -> &mut SalsaDb {
        &mut self.db
    }

    /// Mint a cheap read-only snapshot of the database for a worker thread.
    pub(crate) fn analysis(&self) -> Analysis {
        Analysis::new(self.db.clone())
    }
}

impl Default for WriterHandle {
    fn default() -> Self {
        Self::new()
    }
}

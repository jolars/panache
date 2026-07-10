//! The salsa writer.
//!
//! [`WriterHandle`] owns the master [`SalsaDb`](crate::salsa::SalsaDb). Today it
//! is a thin pass-through wrapper still living inside [`GlobalState`], mutated on
//! the main event loop exactly as before. It exists to concentrate every
//! salsa-touching access behind one type so a later phase can relocate the
//! database onto a dedicated writer thread without re-threading call sites.
//!
//! **Concurrency invariant (validated by the test below).** A salsa write
//! (`db_mut()` → `zalsa_mut` → `cancel_others`) blocks until the live-clone count
//! is exactly 1. So when the database moves off-thread, the owning thread must be
//! the sole holder of a live handle at write time: read snapshots have to be
//! transient — minted by the owner and dropped when the read finishes or
//! cancels. A persistent clone retained on the main loop would keep the count
//! `>= 2` and deadlock the writer, so reads are routed to the owner to mint an
//! ephemeral snapshot (fatou's model, and panache's current main-loop model),
//! not served from a long-lived main-loop clone.
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

#[cfg(test)]
mod tests {
    use salsa::Durability;

    use super::WriterHandle;
    use crate::lsp::helpers::catch_cancelled;

    /// Pins down the salsa concurrency invariant the writer-thread port must
    /// obey. `zalsa_mut` (every write) runs `cancel_others`, which **blocks until
    /// the live-clone count is exactly 1** (see salsa `storage.rs`:
    /// `while *clones != 1 { cvar.wait() }`). Two consequences shape the design:
    ///
    /// 1. A *persistent* database clone held anywhere keeps the count `>= 2`
    ///    forever, so the owning thread's next write **deadlocks**. The main loop
    ///    therefore must NOT retain a long-lived read clone (`read_db`); read
    ///    snapshots must be transient — minted by the owning thread and dropped
    ///    when the read finishes or cancels. This is exactly fatou's model (and
    ///    panache's current main-loop model): the db owner mints an ephemeral
    ///    snapshot per read.
    /// 2. Given transient snapshots, cross-thread reads are safe: a reader on
    ///    another thread holds its snapshot only for the duration of one read,
    ///    then drops it, so the owner's write proceeds once the count returns to
    ///    1. A read racing a write either observes a valid revision or unwinds to
    ///    a cancellation that `catch_cancelled` maps to `None`.
    ///
    /// The test hands the reader a *fresh* snapshot per round and waits for it to
    /// drop before the next write, mirroring the "owner mints, reader drops"
    /// contract. It asserts liveness (no deadlock) and visibility (the reader
    /// observes each committed revision).
    #[test]
    fn transient_cross_thread_snapshots_stay_live_and_visible() {
        const ROUNDS: u64 = 200;
        let path = std::path::PathBuf::from("/spike/doc.qmd");

        let mut writer = WriterHandle::new();
        let file = writer.db_mut().update_file_text_with_durability(
            path.clone(),
            "v0".to_string(),
            Durability::LOW,
        );
        let config = crate::salsa::FileConfig::new(writer.db(), crate::Config::default());

        // Owner (this thread) hands a transient snapshot to the reader thread each
        // round and receives back the value the reader observed (after it dropped
        // the snapshot). A rendezvous channel keeps at most one snapshot in the
        // reader's hands at a time.
        let (snap_tx, snap_rx) = crossbeam_channel::bounded::<crate::salsa::Analysis>(0);
        let (ack_tx, ack_rx) = crossbeam_channel::bounded::<Option<String>>(0);

        let reader = std::thread::spawn(move || {
            while let Ok(snap) = snap_rx.recv() {
                let value = catch_cancelled(|| {
                    crate::salsa::parsed_tree_root(snap.db(), file, config)
                        .text()
                        .to_string()
                });
                // Drop the snapshot BEFORE acking so the owner's next write sees
                // the clone count return to 1 and never blocks.
                drop(snap);
                if ack_tx.send(value).is_err() {
                    break;
                }
            }
        });

        for i in 1..=ROUNDS {
            writer.db_mut().update_file_text_with_durability(
                path.clone(),
                format!("v{i}"),
                Durability::LOW,
            );
            // Mint a transient snapshot on the owning thread and hand it off.
            snap_tx.send(writer.analysis()).expect("reader alive");
            let observed = ack_rx.recv().expect("reader acked");
            if let Some(text) = observed {
                let text = text.trim_end();
                assert!(
                    text.strip_prefix('v')
                        .is_some_and(|n| n.parse::<u64>().is_ok()),
                    "reader observed a torn/invalid parse: {text:?}"
                );
            }
        }

        drop(snap_tx);
        drop(ack_rx);
        reader.join().expect("reader thread panicked");

        // Visibility: after all rounds, the owner observes the final revision.
        let final_owned = crate::salsa::parsed_tree_root(writer.db(), file, config)
            .text()
            .to_string();
        assert_eq!(final_owned.trim_end(), format!("v{ROUNDS}"));
    }
}

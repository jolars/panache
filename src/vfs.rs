//! File identity and text inputs shared by the LSP and Salsa.
//!
//! Owns the single source of truth for file *identity*: the path<->[`FileId`]
//! bimap and the id->[`FileText`] input table. Only the writer mutates it
//! (`register`/`remove_path`); cloned worker handles share the same
//! `Arc<Mutex<_>>` and only read. The salsa [`crate::salsa::SalsaDb`] holds one
//! [`Vfs`] and delegates all path/id lookups to it.
//!
//! Ids are dense and append-only: a fresh id is `files.len()` and slots are
//! never recycled. Eviction tombstones a slot (`None`) and drops its path/input
//! reverse entries, so re-interning an evicted path mints a *fresh* id (matching
//! the pre-consolidation `HashMap` behavior) while every id a stale worker
//! snapshot still names stays index-addressable.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock, RwLock};

use crate::salsa::{Db, FileSet, FileText};

/// Opaque, process-stable identity for a file. This is a plain newtype rather
/// than a Salsa interned struct because
/// the LSP boundary must convert URI -> `FileId` synchronously on the main
/// thread, outside any salsa query. Intra-query path interning still goes
/// through [`crate::salsa::InternedPath`].
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct FileId(u32);

/// Per-id metadata: the (immutable) backing path and its salsa text input.
/// `path` is `None` for an in-memory buffer with no file on disk (retires the
/// `<memory>` sentinel).
#[derive(Clone)]
struct FileMeta {
    path: Option<PathBuf>,
    input: FileText,
}

/// The interior of [`Vfs`]: the dense id->meta table plus the two reverse
/// indices. Held behind a copy-on-write `Arc`: the writer rebuilds a fresh
/// `VfsInner` and swaps the `Arc`, so a reader that has cloned the current `Arc`
/// keeps reading a consistent snapshot with no further locking.
#[derive(Clone, Default)]
struct VfsInner {
    /// Dense id->meta table indexed by `FileId.0`. A `None` slot is a tombstone
    /// (an evicted id); the vector only grows, so ids are never recycled.
    files: Vec<Option<FileMeta>>,
    /// Live path -> id. Eviction removes the entry, so a re-interned path mints
    /// a fresh id.
    path_to_id: HashMap<PathBuf, FileId>,
    /// Reverse map: a [`FileText`] input back to its id, so path-keyed queries
    /// can resolve a document's path from its `FileText` identity.
    input_to_id: HashMap<FileText, FileId>,
}

impl VfsInner {
    fn meta(&self, id: FileId) -> Option<&FileMeta> {
        self.files.get(id.0 as usize).and_then(|slot| slot.as_ref())
    }
}

/// Owns the path-to-id-to-input table and the structural [`FileSet`] input.
/// Owned by
/// [`crate::salsa::SalsaDb`]; both fields are shared behind `Arc`, so cloned
/// worker handles observe the same table and the same `FileSet` input.
///
/// Reads are near-lock-free: a reader takes the read lock only long enough to
/// clone the current snapshot `Arc`, then does its lookups against that owned
/// snapshot with no lock held. The single writer rebuilds a fresh [`VfsInner`]
/// and swaps the `Arc` under the write lock, so an in-flight reader keeps
/// observing a consistent prior snapshot. This is a dependency-free stand-in
/// for `arc-swap` (the critical section is a pointer clone, not a lookup).
#[derive(Clone, Default)]
pub(crate) struct Vfs {
    inner: Arc<RwLock<Arc<VfsInner>>>,
    /// The single [`FileSet`] input, minted lazily on the writer and shared
    /// across cloned handles. Co-located with the interner because the set's
    /// membership *is* the interner's live id set: interning adds an id and
    /// eviction removes it. The actual `Arc<HashSet<FileId>>` value lives in
    /// salsa (mutated via `&mut SalsaDb` setters); this holds only the handle.
    file_set: Arc<OnceLock<FileSet>>,
}

impl Vfs {
    /// Clone the current snapshot `Arc` (read lock held only for the clone).
    fn snapshot(&self) -> Arc<VfsInner> {
        self.inner.read().expect("vfs lock poisoned").clone()
    }

    /// Copy-on-write mutation: clone the current snapshot, apply `f`, and swap
    /// the fresh `Arc` in under the write lock. Writer-only; readers holding an
    /// older snapshot are unaffected.
    fn with_mut<R>(&self, f: impl FnOnce(&mut VfsInner) -> R) -> R {
        let mut guard = self.inner.write().expect("vfs lock poisoned");
        let mut next = (**guard).clone();
        let out = f(&mut next);
        *guard = Arc::new(next);
        out
    }

    /// The shared [`FileSet`] input handle, minted once on the writer. Cloned
    /// worker handles share the same `OnceLock` and only ever read it back, so
    /// they observe the writer's set, never mint their own.
    pub(crate) fn file_set(&self, db: &dyn Db) -> FileSet {
        *self
            .file_set
            .get_or_init(|| FileSet::new(db, Arc::new(HashSet::new())))
    }

    pub(crate) fn id_for_path(&self, path: &Path) -> Option<FileId> {
        self.snapshot().path_to_id.get(path).copied()
    }

    pub(crate) fn input_for_id(&self, id: FileId) -> Option<FileText> {
        self.snapshot().meta(id).map(|meta| meta.input)
    }

    /// The [`FileId`] backing a [`FileText`] input, or `None` if the input is
    /// unregistered or evicted. Lets the LSP key a document on its stable id.
    pub(crate) fn id_for_input(&self, input: FileText) -> Option<FileId> {
        self.snapshot().input_to_id.get(&input).copied()
    }

    pub(crate) fn input_for_path(&self, path: &Path) -> Option<FileText> {
        let snap = self.snapshot();
        let id = *snap.path_to_id.get(path)?;
        snap.meta(id).map(|meta| meta.input)
    }

    pub(crate) fn path_for_id(&self, id: FileId) -> Option<PathBuf> {
        self.snapshot().meta(id).and_then(|meta| meta.path.clone())
    }

    /// The immutable backing path for a [`FileText`] input, or `None` for an
    /// in-memory buffer / unregistered / evicted input.
    pub(crate) fn path_for_input(&self, input: FileText) -> Option<PathBuf> {
        let snap = self.snapshot();
        let id = *snap.input_to_id.get(&input)?;
        snap.meta(id).and_then(|meta| meta.path.clone())
    }

    pub(crate) fn cached_paths(&self) -> Vec<PathBuf> {
        self.snapshot().path_to_id.keys().cloned().collect()
    }

    /// How many id slots the table holds, tombstones included. Only eviction
    /// and interning move this number, so it is the direct measure of whether
    /// a repeated event churns ids instead of reusing them.
    pub(crate) fn slot_count(&self) -> usize {
        self.snapshot().files.len()
    }

    /// Register a fresh id for `path`/`input` and return it. Called only by the
    /// single writer. The id is `files.len()` (dense, append-only).
    pub(crate) fn register(&self, path: Option<PathBuf>, input: FileText) -> FileId {
        self.with_mut(|inner| {
            let id = FileId(inner.files.len() as u32);
            if let Some(path) = path.clone() {
                inner.path_to_id.insert(path, id);
            }
            inner.input_to_id.insert(input, id);
            inner.files.push(Some(FileMeta { path, input }));
            id
        })
    }

    /// Forget a path's id/input mapping, tombstoning its slot. Returns the
    /// removed [`FileId`], if any. The id is not recycled, so a later
    /// re-intern of the same path mints a fresh id.
    pub(crate) fn remove_path(&self, path: &Path) -> Option<FileId> {
        self.with_mut(|inner| {
            let id = inner.path_to_id.remove(path)?;
            let removed = inner
                .files
                .get_mut(id.0 as usize)
                .and_then(|slot| slot.take());
            if let Some(meta) = removed {
                inner.input_to_id.remove(&meta.input);
            }
            Some(id)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::salsa::SalsaDb;

    #[test]
    fn register_assigns_distinct_ids_and_round_trips_lookups() {
        let db = SalsaDb::default();
        let vfs = Vfs::default();
        let a = FileText::new(&db, None);
        let b = FileText::new(&db, None);

        let id_a = vfs.register(Some(PathBuf::from("/a.qmd")), a);
        let id_b = vfs.register(Some(PathBuf::from("/b.qmd")), b);
        assert_ne!(id_a, id_b);

        assert_eq!(vfs.id_for_path(Path::new("/a.qmd")), Some(id_a));
        assert!(vfs.input_for_id(id_a) == Some(a));
        assert!(vfs.input_for_path(Path::new("/a.qmd")) == Some(a));
        assert_eq!(vfs.path_for_id(id_a), Some(PathBuf::from("/a.qmd")));
        assert_eq!(vfs.path_for_input(a), Some(PathBuf::from("/a.qmd")));
    }

    #[test]
    fn evict_clears_lookups_and_reintern_mints_fresh_id() {
        let db = SalsaDb::default();
        let vfs = Vfs::default();
        let a = FileText::new(&db, None);

        let id1 = vfs.register(Some(PathBuf::from("/a.qmd")), a);
        assert_eq!(vfs.remove_path(Path::new("/a.qmd")), Some(id1));

        // Every view of the evicted id clears (matches the pre-consolidation
        // `HashMap` removal behavior).
        assert_eq!(vfs.id_for_path(Path::new("/a.qmd")), None);
        assert!(vfs.input_for_id(id1).is_none());
        assert_eq!(vfs.path_for_id(id1), None);
        assert_eq!(vfs.path_for_input(a), None);
        assert!(vfs.cached_paths().is_empty());

        // Re-interning the same path mints a fresh, distinct id: tombstoned
        // slots are never recycled.
        let b = FileText::new(&db, None);
        let id2 = vfs.register(Some(PathBuf::from("/a.qmd")), b);
        assert_ne!(id1, id2);
        assert_eq!(vfs.id_for_path(Path::new("/a.qmd")), Some(id2));
    }

    #[test]
    fn in_memory_buffers_get_distinct_pathless_ids() {
        let db = SalsaDb::default();
        let vfs = Vfs::default();
        let a = FileText::new(&db, None);
        let b = FileText::new(&db, None);

        let id_a = vfs.register(None, a);
        let id_b = vfs.register(None, b);
        assert_ne!(id_a, id_b);
        assert_eq!(vfs.path_for_id(id_a), None);
        assert_eq!(vfs.path_for_input(a), None);
        // A pathless buffer is still input-addressable by its id.
        assert!(vfs.input_for_id(id_a) == Some(a));
    }
}

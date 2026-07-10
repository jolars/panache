# LSP writer-thread port --- work-in-progress handoff

> Branch-scoped scratch note. It travels with the `lsp-writer-thread` branch so
> the work can be resumed on another machine. **Delete before merging.**

## Goal

Fix format-on-save latency on larger projects (e.g. a Bookdown book). The stall
is not "salsa on the main thread" per se (that's the deliberate rust-analyzer
single-writer model); it's that the diagnostics settle runs a **disk-I/O
write-phase synchronously on the main event loop** --- `dispatch_due_lints`
(`src/lsp/dispatch.rs`) calls `reload_open_documents_referenced_files`
(`src/lsp/documents.rs`), which reads includes/bibliographies off disk and
`resync_cached_file_from_disk` over the whole referenced set. A
`textDocument/formatting` request landing in that window waits for the main loop
to return to `select!`.

Chosen fix (user-approved): the full **dedicated-writer-thread port**, mirroring
the sibling servers (fatou/arity/badness). Move the salsa database off the main
loop onto a `panache-lsp-writer` thread so the main loop never blocks on salsa
writes or referenced-file disk I/O.

## THE load-bearing finding (read this first)

A spike proved the original plan's read-path design **deadlocks** and had to be
corrected:

- Every salsa write (`db_mut()` → `zalsa_mut` → `cancel_others`) blocks
  `while *clones != 1` --- see salsa 0.27 source
  `~/.cargo/registry/src/*/salsa-0.27.0/src/storage.rs:160-188` (the fn even
  warns *"This could deadlock if there is a single worker with two handles to
  the same database!"*).
- Therefore a **persistent** db clone held anywhere (e.g. a long-lived main-loop
  `read_db` clone) keeps the count `>= 2` forever and the writer thread's first
  write hangs.
- Correct model: read snapshots must be **transient** --- minted by the
  db-owning thread and dropped when the read finishes or cancels. This forces
  fatou's `ReadJob` routing: reads go to the writer, which mints an ephemeral
  snapshot and hands it to the read pool. (This is also how panache works today:
  the main loop owns the db and mints ephemeral `Analysis` clones per pooled
  read.)

Guard: the test `transient_cross_thread_snapshots_stay_live_and_visible` in
`src/lsp/writer.rs` pins this. If it fails, or anyone reintroduces a persistent
`read_db` clone on the main loop, that's the regression.

## Landed so far (4 commits on `lsp-writer-thread`, oldest first)

1. `07e5ea24` refactor(lsp): route salsa access through `WriterHandle` ---
   `WriterHandle` owns the `SalsaDb`; all `gs.salsa` sites go through
   `db()`/`db_mut()`/`analysis()`.
2. `cf0d5f76` test(lsp): pin salsa cross-thread snapshot invariant --- the spike
   above; corrected the design.
3. `4e8e1af2` refactor(lsp): funnel write notifications through `apply_write`
   --- every DB-mutating notification flows through one
   `GlobalState::apply_write(WriteCommand)` chokepoint (`writer_command.rs`).
4. `bec5d348` refactor(lsp): move config ownership into the writer ---
   `workspace_folders`, extend-chain watch set, config-error toast dedup, and
   `load_config_notifying` moved onto `WriterHandle` (holds a `ClientSender`
   clone). **Writer now owns db + config.**

Every commit is green: LSP suite (296 tests), clippy `-D warnings`, rustfmt.

## Next edits (mapped, not started)

**1. Document map → writer.** Add `document_map()` / `document_map_mut()` (via
`Arc::make_mut`) / `document_map_arc()` to `WriterHandle`; remove the
`Arc<DocumentMap>` field from `GlobalState`. `StateSnapshot`'s OWN
`document_map` field STAYS (it is assembled at snapshot time). GlobalState-side
sites (\~15, some multi-line `gs\n.document_map…`):

- `src/lsp/documents.rs`: 101, 108, 151, 201, 239, 328, 363, 378, 388
- `src/lsp/dispatch.rs`: 798, 893
- `src/lsp/handlers/file_watcher.rs`: 101
- `src/lsp/testing.rs`: 548, 559, 709, 729

Rule: `gs.document_map` → `gs.writer.document_map()`; `gs.document_map_mut()` →
`gs.writer.document_map_mut()`; `snapshot()`'s `Arc::clone(&self.document_map)`
→ `self.writer.document_map_arc()`.

**2. Diagnostics store → writer** --- defer; entangled with `on_task`/settle
application, so it lands with the settle-on-writer phase.

**3. Thread spawn** (`WriterHandle::spawn()` + `ReadJob` channel + `Outbound`
channel), then **settle on the writer**, then the **harvester thread** for the
referenced-file disk I/O (the actual latency win), then **version-gating** of
publishes.

## End-state design (for orientation)

- **Writer thread owns:** `SalsaDb`, the salsa-input side of documents (or the
  whole document map --- see below), config state, diagnostics store, settle
  machinery. Mints transient `StateSnapshot`s per `ReadJob`.
- **Main loop owns:** the LSP transport (`sender`, in-flight/cancelled request
  ids), the `settle_deadline` timer, `external_pending`. Forwards
  `WriteCommand`s and `ReadJob`s to the writer; drains `Outbound` (diagnostics
  publishes, toasts) to the client.
- **`LspTester` harness** (`src/lsp/testing.rs`) drives handlers synchronously
  over `&mut GlobalState` and 28 `tests/lsp/*` files depend on it. Preserve this
  as the writer's `inline()` mode (owns the db in-thread, mints snapshots
  synchronously, no channel round-trip). Never delete the synchronous path.
- **Keep panache's single all-docs settle pass** (do NOT adopt fatou's per-URI
  `decide()` for diagnostics): panache has cross-document diagnostics (a shared
  manifest error deduped across docs) that a per-URI model would break.

Design decision in flight: whether the writer owns the *entire* document map
(salsa handles + trees + text) or only the salsa-input side. The whole-map
option is simpler for read-snapshot assembly (the writer mints a complete
snapshot with no bounce back to the main loop) and keeps did_change's
`set_text → refdef_set → parse` colocated with the db; the current lean is
toward the whole map on the writer, with the main loop as thin transport.

## How to resume

```text
cd <repo> && git checkout lsp-writer-thread
# read this file top-to-bottom, then:
cargo test --features lsp lsp
cargo clippy --features lsp --all-targets -- -D warnings
# continue at "Next edits -> 1. Document map -> writer"
```

Keep each field-group move a separate green commit. Verify per step with the two
commands above (and `cargo fmt -- --check`).

## Cross-machine note

For this to reach another computer the branch must be pushed:
`git push -u origin lsp-writer-thread` (needs your authorization --- not done
automatically).

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

## Landed so far (7 commits on `lsp-writer-thread`, oldest first)

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
5. `962babf5` refactor(lsp): move document map into the writer --- the writer
   owns the whole `Arc<DocumentMap>` (per the design decision below: whole map,
   not just the salsa-input side). `StateSnapshot`'s own `document_map` field is
   unchanged. **Writer now owns db + config + document map.**
6. `98751ff0` refactor(lsp): decouple write handlers from `GlobalState` ---
   write handlers run against writer-owned state (which gains
   `runtime_settings`) and request main-loop side effects (settle arming,
   external-lint marking, diagnostics drops) via a `WriteEffects` accumulator
   applied by `GlobalState::apply_write_effects`. The tester funnels
   notifications through `apply_write` like production.
7. refactor(lsp): spawn the dedicated writer thread --- `WriterState` (the owned
   state) split from `WriterHandle` (a mode wrapper: `Inline(Box<WriterState>)`
   or `Threaded { tx }`). Production calls `gs.spawn_writer()` after the
   handshake; `LspTester` stays inline forever. In threaded mode: writes forward
   as `WriteCommand`s (effects return as `Task::WriteEffects` on the existing
   task channel --- no separate `Outbound` channel needed); pooled reads forward
   as `ReadJob`s (main loop captures `SnapshotBits` + the handler closure, the
   writer mints the `StateSnapshot` and hands the job to the pools via
   `TaskSpawner` clones); the due settle forwards as a `SettleJob`, so the
   **referenced-file reload (disk-I/O write phase) runs on the writer thread,
   off the main event loop**. The main loop now stays responsive during the
   reload, but note the win is partial: a read forwarded *mid-reload* still
   queues behind it on the writer channel --- releasing that wait is the
   harvester phase below. `settle_task` (the all-docs read pass) is a free fn in
   `dispatch.rs` shared by inline and threaded paths. Smoke test:
   `threaded_writer_serves_writes_reads_and_settles`.

Every commit is green: full suite w/ `--features lsp` (296 LSP integration tests
included), clippy `-D warnings`, rustfmt.

## Next edits

**1. Diagnostics store + settle machinery → writer** --- move
`DiagnosticCollection`, `lint_generation`/`last_applied_lint_generation`,
`external_pending`, and possibly the `settle_deadline` timer onto the writer so
effects stop round-tripping. Entangled with `on_task`; the pull-diagnostics
handlers read the store on the main loop, so the store may need to stay shared
(`Arc`) or the pull path re-routed.

**2. Harvester thread** --- move the referenced-file disk I/O off the writer
thread too (it currently holds up queued writes/reads behind a slow reload; see
the timing warn in `writer_thread`'s `Settle` arm).

**3. Version-gating** of publishes.

Watch out for: in threaded mode nothing on the main loop may call
`writer.state()`/`state_mut()` or the inline delegates (`db()`,
`document_map()`, ...) --- they panic after `spawn()`. The 296-test suite runs
inline, so a threaded-only regression needs the writer unit tests (or a future
threaded harness) to catch it.

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
# continue at "Next edits -> 1. Diagnostics store + settle machinery -> writer"
```

Keep each field-group move a separate green commit. Verify per step with the two
commands above (and `cargo fmt -- --check`).

## Cross-machine note

For this to reach another computer the branch must be pushed:
`git push -u origin lsp-writer-thread` (needs your authorization --- not done
automatically).

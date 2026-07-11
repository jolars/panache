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

8. refactor(lsp): move diagnostics store + settle machinery into the writer ---
   `WriterState` now owns `DiagnosticCollection`, both lint generations,
   `external_pending`, `settle_deadline`, and copies of the pull-capability
   flags (set pre-spawn at `initialize`; `GlobalState` keeps only
   `supports_diagnostic_refresh` --- the refresh is an outgoing request with
   main-loop-tracked ids). Consequences: `Task::WriteEffects` and `SnapshotBits`
   are gone (write effects apply writer-side; the writer mints complete
   snapshots from its own state); the writer **self-times the settle** in
   threaded mode (`recv_timeout` against its own deadline), so
   `SettleJob`/`submit_settle` are gone and the main loop's
   `dispatch_due_lints`/`next_lint_timeout` are inline-mode-only no-ops after
   spawn; settle results still ride the task channel (`Task::Diagnostics`) and
   `on_task` forwards them back via `WriterHandle::forward_settle_result`
   (routing through the main loop avoids a self-referential writer sender that
   would keep its channel from disconnecting on shutdown); the refresh nudge
   returns as `Task::RefreshDiagnostics`; `workspace/diagnostic` moved from the
   inline-streaming path to a pooled `spawn_streaming_request` over the
   snapshot's store view (the `inline_streaming` macro is deleted), which also
   makes it FIFO-ordered after forwarded writes and properly cancellable.

9. feat(lsp): harvest settle disk I/O on a dedicated thread --- the settle write
   phase in threaded mode is now a **harvest cycle**: the writer discovers and
   interns the referenced set per round (`WriterState::harvest_round` over
   `SalsaDb::discover_referenced_files`, the split-out discovery half of
   `load_referenced_files`), the `panache-lsp-harvester` thread reads the paths
   from disk, and the contents come back as `Task::Harvested` →
   `WriterMsg::Harvested` (same main-loop-forwarded routing as settle results
   --- a direct harvester→writer channel would deadlock shutdown circularly) for
   a compare-and-set apply (`SalsaDb::apply_harvested_file_text`: `None`→`Some`
   at HIGH durability like `load_file_from_disk`, changed-content refresh at
   MEDIUM like `resync_cached_file_from_disk`; open-buffer paths skipped at
   request AND apply time --- buffer-authoritative). Rounds repeat until
   discovery finds nothing new, then `complete_settle` bumps the generation,
   takes externals, mints the snapshot, and spawns the read pass --- so writes
   served mid-cycle are covered by the very pass the cycle feeds, and
   `complete_settle` clears any deadline they armed (no redundant re-lint). A
   deadline firing mid-cycle is swallowed for the same reason. At most one cycle
   is in flight. Inline mode keeps the synchronous
   `reload_open_documents_referenced_files` (`begin_due_settle`), so the tester
   stays synchronous. Note: `did_open`/`did_save`/file-op/watcher handlers still
   call the synchronous load/reload on the writer thread --- event-driven and
   mostly cached steady-state (loads skip `Some` inputs), but not
   settle-frequency; could route through the harvester later if profiling says
   so. Tests: `harvest_rounds_resync_referenced_files` (primitives) and
   `threaded_settle_harvests_referenced_files_off_thread` (end-to-end
   out-of-band bibliography resync through the real threads).

10. feat(lsp): version-gate diagnostics publishes --- `DocumentState` gains
    `version` (set at `didOpen`, updated at `didChange`); the settle read pass
    tags each publish with the version the snapshot holds (non-open targets like
    manifests/config files stay `None`). The tag flows through
    `DiagnosticCollection` into both push (`publishDiagnostics.version`) and
    pull (`WorkspaceDocumentDiagnosticReport.version`), so a client discards a
    report computed against a buffer it has since edited. Note: an
    items-unchanged settle keeps the stored entry (old version tag) --- no
    redundant push, and the items still describe the buffer.

Every commit is green: full suite w/ `--features lsp` (296+ LSP integration
tests included), clippy `-D warnings`, rustfmt.

## Next edits

None --- the port and the follow-ups listed at kickoff (writer thread,
diagnostics/settle ownership, harvester, version-gating) have all landed.
Remaining before merge: real-editor smoke test on a large project (the Bookdown
book that motivated this), then **delete this file**.

Watch out for: in threaded mode nothing on the main loop may call
`writer.state()`/`state_mut()` or the inline delegates (`db()`,
`document_map()`, ...) --- they panic after `spawn()`. The 296-test suite runs
inline, so a threaded-only regression needs the writer unit tests (or a future
threaded harness) to catch it.

## End-state design (for orientation)

- **Writer thread owns (all landed):** `SalsaDb`, the whole document map, config
  state, the diagnostics store, and the settle machinery (deadline, generations,
  pending externals; self-timed via `recv_timeout`). Mints transient
  `StateSnapshot`s per `ReadJob`.
- **Main loop owns:** the LSP transport (`sender`, in-flight/cancelled request
  ids, outgoing-request ids + `supports_diagnostic_refresh`) and the task pools.
  Forwards `WriteCommand`s, `ReadJob`s, and settle results to the writer; turns
  `Task`s into client messages.
- **`LspTester` harness** (`src/lsp/testing.rs`) drives handlers synchronously
  over `&mut GlobalState` and 28 `tests/lsp/*` files depend on it. Preserve this
  as the writer's `inline()` mode (owns the db in-thread, mints snapshots
  synchronously, no channel round-trip). Never delete the synchronous path.
- **Keep panache's single all-docs settle pass** (do NOT adopt fatou's per-URI
  `decide()` for diagnostics): panache has cross-document diagnostics (a shared
  manifest error deduped across docs) that a per-URI model would break.

## How to resume

```text
cd <repo> && git checkout lsp-writer-thread
# read this file top-to-bottom, then:
cargo test --features lsp lsp
cargo clippy --features lsp --all-targets -- -D warnings
# see "Next edits" (all planned work has landed)
```

Keep each field-group move a separate green commit. Verify per step with the two
commands above (and `cargo fmt -- --check`).

## Cross-machine note

For this to reach another computer the branch must be pushed:
`git push -u origin lsp-writer-thread` (needs your authorization --- not done
automatically).

# LSP Async + Rowan Safety Pattern

This note documents the preferred handler structure for LSP requests that touch
both async state (document map/Salsa) and non-`Send` CST nodes (`rowan`).

## Why this exists

`rowan::SyntaxNode` and related cursor internals are not `Send`. LSP request
futures in `tower_lsp_server` must be `Send`, so handlers can fail to compile
if a syntax node is held across an `.await`.

## Required pattern

Use a two-phase structure in handlers:

1. **Async fetch phase** (may await)
   - Load open-document inputs via `crate::lsp::context::get_open_document_context`.
   - Load other async dependencies (config, project graph/index data, etc.).
   - Compute byte offsets from positions.

2. **Sync syntax phase** (no await)
   - Build CST root via `ctx.syntax_root()`.
   - Run all CST traversal and symbol extraction.

After syntax analysis is done, it is safe to do more async work, but avoid
carrying CST nodes into those awaits.

## Practical checklist

- Never keep `SyntaxNode`, `SyntaxToken`, or iterators alive across `.await`.
- Prefer storing only plain data (labels, ranges, enums, IDs) before awaiting.
- If needed, create a small `Pending*` enum/struct to capture extracted intent,
  then do async lookups.
- Prefer `OpenDocumentContext` over ad-hoc map/db lock patterns.

## Current examples

- `src/lsp/handlers/goto_definition.rs`
- `src/lsp/handlers/hover.rs`
- `src/lsp/handlers/references.rs`
- `src/lsp/handlers/document_links.rs`

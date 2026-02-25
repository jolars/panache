### Long-term refactor plan: inline parsing during block parsing (Pandoc-style)

We have decided to incrementally move from the current “block parse → inline
pass” pipeline to a Pandoc-like approach where blocks emit inline structure
directly.

#### Goals

- Preserve losslessness and stable `SyntaxKind` structure (formatter/LSP depend
  on it).
- Avoid backtracking on `GreenNodeBuilder` by using “detect/collect first, emit
  once.”
- Keep changes incremental: maintain an A/B-able path until golden tests and
  formatter are updated.

#### Incremental phases

1. **Shared inline emission helper**
   - Introduce a small helper (e.g. `emit_inlines(builder, text, config)`) that
     calls the existing inline parsing core.
   - Block modules should call this helper instead of emitting a single `TEXT`
     token where appropriate.

2. **Convert low-risk blocks first (stringy content)**
   - Headings (`HEADING_CONTENT`), table captions/cells, definition list terms,
     figure captions, etc.
   - These already have the relevant text slice available at emission time.

3. **Convert paragraphs/plain via buffering-at-close**
   - While scanning lines, accumulate raw paragraph/plain bytes (including
     newlines) in the block parser container state.
   - When the paragraph/plain closes, emit `PARAGRAPH`/`PLAIN` and run inline
     parsing once on the buffered chunk.
   - This supports multi-line inline constructs (e.g. display math) without
     needing rollback.

4. **Transition plumbing and compatibility**
   - Add a temporary flag/config or internal switch to skip the separate
     `InlineParser` pass when blocks already emit inline structure.
   - Keep both paths temporarily to A/B in tests and isolate formatter/linter
     assumptions.

5. **Finalize and delete the legacy pass**
   - Once golden cases, formatter, and linter all match expectations, remove the
     separate inline traversal.
   - Prefer small, feature-by-feature migrations over snapshot-wide updates.

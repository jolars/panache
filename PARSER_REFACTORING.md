# Parser Refactoring: Inline Parsing During Block Parsing (Pandoc-style)

**Status**: Phase 4 In Progress (PLAIN blocks ✅, PARAGRAPH blocks pending) | Phase 5 Future Work  
**Date**: 2026-02-25

---

## Overview

We are incrementally migrating from the current "block parse → inline pass" pipeline 
to a Pandoc-like approach where blocks emit inline structure directly during parsing.

### Progress Document

This document tracks the rationale, goals, implementation progress, and testing strategy.
It should be updated as we complete each phase and serves as a reference.

### Why This Refactoring?

**Performance**: Current two-pass architecture has overhead:

- Full CST re-traversal in InlineParser
- Complete tree rebuild (new GreenNodes allocated)
- String concatenation for paragraphs
- For 100+ documents, this overhead multiplies significantly

**Pandoc alignment**: Matching Pandoc's architecture makes it easier to conform to 
their behavior and reference their implementation.

**Incremental parsing**: Long-term goal. Current architecture makes this nearly 
impossible because inline pass rebuilds the entire tree.

### Goals

- ✅ Preserve losslessness and stable `SyntaxKind` structure (formatter/LSP depend on it)
- ✅ Avoid backtracking on `GreenNodeBuilder` by using "detect/collect first, emit once"
- ✅ Keep changes incremental: maintain A/B-able path until golden tests and formatter are updated
- 🔄 Achieve 40-60% performance improvement when fully migrated

---

## Implementation Progress

### Phase 1: Shared Inline Emission Helper ✅ COMPLETE (2026-02-25)

**Goal**: Create infrastructure to call inline parsing from block parser.

**Completed**:

- ✅ Created `src/parser/block_parser/inline_emission.rs`
- ✅ `emit_inlines(builder, text, config)` helper function
- ✅ Calls existing `parse_inline_text_recursive()` from inline_parser/core.rs
- ✅ Made `inline_parser::core` module public
- ✅ Unit tests verify identical output to current inline parser
- ✅ All 829 tests passing, clippy clean

**Key files**:

- `src/parser/block_parser/inline_emission.rs` (135 lines)

---

### Phase 2: Config and Testing Infrastructure ✅ COMPLETE (2026-02-25)

**Goal**: Add A/B testing to verify migration doesn't change behavior.

**Completed**:

- ✅ Added `ParserConfig` to `src/config.rs`
  - `use_integrated_inline_parsing: bool` (default: false)
  - Serde support (TOML serialization)
  - ConfigBuilder method
- ✅ Created A/B testing harness: `tests/ab_testing.rs`
  - `run_ab_test(case_name)` function
  - Verifies CST and output equivalence between old/new parser
  - 2 tests passing
- ✅ All 834 tests passing, clippy clean

**Usage**:

```toml
# panache.toml
[parser]
use-integrated-inline-parsing = true  # Enable new parser (default: false)
```

**Key files**:

- `src/config.rs` - Added ParserConfig struct
- `tests/ab_testing.rs` - A/B testing harness (171 lines)

---

### Phase 3: Migrate Individual Blocks ✅ COMPLETE (2026-02-25)

**Goal**: Migrate low-risk blocks with simple inline content.

#### ✅ Headings (COMPLETE - 2026-02-25)

**Changes**:

- Modified `src/parser/block_parser/headings.rs::emit_atx_heading()`
  - Added `config: &Config` parameter
  - Conditional: if flag enabled, call `emit_inlines()`, else emit TEXT
- Updated call site in `block_parser.rs`
- Modified `src/parser/inline_parser.rs`:
  - Added `should_skip_already_parsed()` - checks if HEADING_CONTENT should skip parsing
  - Added `copy_subtree_verbatim()` - copies already-parsed nodes without re-parsing
  - Modified `copy_node_to_builder()` to use skip logic

**Verification**:

- ✅ All 836 tests pass (2 A/B tests)
- ✅ Clippy clean
- ✅ CST structure identical between old/new parser
- ✅ Formatted output identical

#### ✅ Table Captions (COMPLETE - 2026-02-25)

**Changes**:

- Modified `src/parser/block_parser/tables.rs::emit_table_caption()`
  - Added `config: &Config` parameter  
  - Conditional inline parsing for caption text
- Added `config` parameter to all table parsing functions:
  - `try_parse_simple_table()`
  - `try_parse_pipe_table()`
  - `try_parse_grid_table()`
  - `try_parse_multiline_table()`
- Updated all call sites in `block_parser.rs`
- Added `TABLE_CAPTION` to `should_skip_already_parsed()` in inline_parser.rs

**Verification**:

- ✅ All tests pass (3 A/B tests including `table_with_caption`)
- ✅ CST structure identical
- ✅ Formatted output identical

#### ✅ Definition List Terms (COMPLETE - 2026-02-25)

**Changes**:

- Modified `src/parser/block_parser/definition_lists.rs::emit_term()`
  - Added `config: &Config` parameter
  - Conditional inline parsing for term text
- Updated call site in `block_parser.rs`
- Added `TERM` to `should_skip_already_parsed()` in inline_parser.rs

**Verification**:

- ✅ All tests pass (4 A/B tests including `definition_list`)
- ✅ CST structure identical
- ✅ Formatted output identical

#### ✅ Line Block Lines (COMPLETE - 2026-02-25)

**Changes**:

- Modified `src/parser/block_parser/line_blocks.rs::parse_line_block()`
  - Added `config: &Config` parameter
  - Conditional inline parsing for line content (both regular and continuation lines)
- Updated call site in `block_parser.rs`
- Added `LINE_BLOCK_LINE` to `should_skip_already_parsed()` in inline_parser.rs
- Added `LINE_BLOCK_MARKER` to `is_structural_token()` (similar to BLOCKQUOTE_MARKER)

**Note**: Line blocks did not previously support inline formatting, so this is new functionality enabled by the integrated parsing approach.

**Verification**:

- ✅ All 837+ tests pass (5 A/B tests including `line_blocks`)
- ✅ CST structure identical
- ✅ Formatted output identical

#### ❌ Table Cells (SKIPPED)

**Reason**: Current table structure doesn't have cell-level nodes (TABLE_CELL exists in SyntaxKind but is not used). Tables emit entire row text as TEXT tokens. Migrating table cells would require:
1. Architectural changes to create TABLE_CELL nodes in the parser
2. Significant changes to table formatting logic
3. Out of scope for Phase 3

**Decision**: Skip table cells migration. This is a future enhancement that would require broader table architecture refactoring.

**Migration pattern** (established and used):

```rust
// In block parser emission function:
builder.start_node(SyntaxKind::HEADING_CONTENT.into());
if config.parser.use_integrated_inline_parsing {
    inline_emission::emit_inlines(builder, text_content, config);
} else {
    builder.token(SyntaxKind::TEXT.into(), text_content);
}
builder.finish_node();

// In inline_parser.rs::should_skip_already_parsed():
matches!(
    node.kind(),
    SyntaxKind::HEADING_CONTENT
    | SyntaxKind::TABLE_CAPTION
    | SyntaxKind::TERM
    | SyntaxKind::LINE_BLOCK_LINE
)
```

**Summary**:

- ✅ 4/4 feasible blocks migrated (headings, table captions, definition list terms, line block lines)
- ✅ 1 block skipped (table cells - requires architectural changes)
- ✅ 5 A/B tests passing
- ✅ Flag remains disabled by default (opt-in)

---

### Phase 4: Paragraphs and Plain Text 🔄 IN PROGRESS (2026-02-25)

**Goal**: Handle multi-line inline content with buffering.

**Status**: PLAIN blocks ✅ complete, PARAGRAPH blocks pending.

#### ✅ PLAIN Blocks (COMPLETE - 2026-02-25)

**Approach**:

PLAIN blocks (used in definition lists and tight list items) required a fundamentally different approach than Phase 3 blocks:

1. **Buffering**: Content is accumulated in `TextBuffer` during block parsing
2. **Delayed emission**: PLAIN node with inline elements is emitted when container closes
3. **Container state**: Added `plain_buffer: TextBuffer` field to `Container::Definition`

**Key architectural decision**: Unlike Phase 3's "emit immediately with inline parsing", PLAIN requires buffering because:
- PLAIN can span multiple continuation lines
- Container must remain open while accumulating lines
- Inline parsing must happen on the complete multi-line content

**Implementation details**:

**TextBuffer utility** (`src/parser/block_parser/text_buffer.rs`):
```rust
pub(crate) struct TextBuffer {
    lines: Vec<String>,  // Stores lines WITH newlines for losslessness
}

impl TextBuffer {
    pub(crate) fn push_line(&mut self, text: impl Into<String>)
    pub(crate) fn get_accumulated_text(&self) -> String  // concat(), not join()
}
```

**Container state changes** (`src/parser/block_parser/container_stack.rs`):
```rust
Container::Definition {
    content_col: usize,
    plain_open: bool,
    plain_buffer: TextBuffer,  // NEW: Accumulate PLAIN content
}
```

**Core emission logic** (`src/parser/block_parser.rs`):

- `close_containers_to()`: When closing Definition, emit buffered PLAIN with inline parsing
- `emit_buffered_plain_if_needed()`: Emit PLAIN without closing Definition (for nested lists/blank lines)

**Bug fixes discovered**:

1. **Definition marker spacing**: Fixed `try_parse_definition_marker()` to not count newlines as whitespace
2. **Empty definition lines**: Handle `:   \n` where content starts on continuation line
3. **Nested lists**: Emit buffered PLAIN before starting lists inside definitions
4. **Blank lines**: Emit buffered PLAIN before processing blank lines to maintain byte ordering

**Modified files**:

- `src/parser/block_parser.rs` (~150 lines changed):
  - `close_containers_to()` method (lines 88-146)
  - `emit_buffered_plain_if_needed()` helper (lines 148-178)
  - Definition creation with conditional buffering (lines 1804-1843)
  - Continuation logic for Definition (lines 866-929)
  - Blank line handling (lines 223-248)
  - List marker detection (lines 1540-1543)
- `src/parser/block_parser/text_buffer.rs` (created, 142 lines)
- `src/parser/block_parser/definition_lists.rs` (1 line changed - newline counting fix)
- `src/parser/inline_parser.rs`:
  - Added PLAIN to `should_skip_already_parsed()` (line 263)
  - Modified `should_concatenate_for_parsing()` to exclude PLAIN when using integrated parsing

**Test coverage**:

- Created 3 comprehensive test cases:
  - `tests/cases/paragraph_continuation/` - multi-line paragraphs
  - `tests/cases/plain_continuation_edge_cases/` - PLAIN with inline markup, nested structures
  - `tests/cases/paragraph_plain_mixed/` - mixed paragraph and definition content
- Added `tests/ab_testing.rs::ab_test_plain_continuation`

**Verification**:

- ✅ All 840 tests passing
- ✅ A/B test confirms new parser produces identical output to old parser
- ✅ CST structure identical (verified via parse tree comparison)
- ✅ Losslessness verified (CST.text() equals input)
- ✅ Clippy clean

**Critical insight**: Second-pass inline parser must NOT re-parse PLAIN nodes when integrated parsing is enabled. The `should_concatenate_for_parsing()` function now checks the flag and excludes PLAIN, using `copy_subtree_verbatim()` instead.

#### ⏳ PARAGRAPH Blocks (PENDING)

**Status**: Not started

**Planned approach**: Apply identical buffering pattern to PARAGRAPH:

1. Add `buffer: TextBuffer` to `Container::Paragraph`
2. Modify `append_paragraph_line()` to buffer instead of emit
3. Create `close_paragraph()` helper to emit buffered content with inline parsing
4. Update all paragraph close sites to use the helper

**Expected complexity**: Similar to PLAIN but simpler (no nested list edge cases)

---

### Phase 5: Finalize and Delete Legacy Pass (FUTURE)

**Goal**: Make integrated parsing the default and remove legacy code.

**Steps**:

1. Flip default: `use_integrated_inline_parsing = true`
2. Extensive real-world testing
3. Remove flag and legacy code path
4. Delete separate InlineParser pass
5. Simplify architecture

---

## Performance Expectations

**Current (Phase 1-3, flag=false)**: Baseline performance (two-pass)

**Phase 3 complete (flag=true)**: 

- Estimated 10-30% improvement (depends on document structure)
- Headings, tables, captions parsed in one pass

**Phases 4-5 complete**:

- Estimated 40-60% improvement
- Single pass through content
- No tree rebuild

---

## Testing Strategy

### A/B Testing

Every migrated block must pass A/B tests:

```bash
cargo test --test ab_testing
```

Verifies:

- ✅ CST structure identical (old vs new parser)
- ✅ Formatted output identical
- ✅ Losslessness preserved (both paths)
- ✅ Idempotency maintained (both paths)

### Current Test Status

- **Total tests**: 840+ (all passing)
- **A/B tests**: 6 (`blockquotes`, `headings`, `table_with_caption`, `definition_list`, `line_blocks`, `plain_continuation`)

### Coverage

If there are not adequate tests for what we are migrating, we will add new
tests to cover those cases before migration.

### New Unit Tests

Also add new unit tests as we go for `emit_inlines()` to verify it produces identical
output to the old inline parser. Not every aspect is covered in golden tests,
for instance not lsp behavior, so these unit tests are critical for confidence.

---

## References

- **Pandoc source**: `pandoc/src/Text/Pandoc/Readers/Markdown.hs`
- **Session plan**: `.copilot/session-state/.../plan.md`
- **A/B testing guide**: `.copilot/session-state/.../files/ab-testing-guide.md`

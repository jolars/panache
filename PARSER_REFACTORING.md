# Parser Refactoring: Inline Parsing During Block Parsing (Pandoc-style)

**Status**: Phase 7.1 In Progress (50% complete) | **Table Migration Underway**
**Date**: 2026-02-25

---

## Current Status ⚡

**Significant progress toward single-pass parsing:**

```rust
// src/parser.rs - LINE 37
pub fn parse(input: &str, config: Option<Config>) -> SyntaxNode {
    let block_tree = BlockParser::new(input, &config).parse();  // FIRST PASS
    let inline_tree = InlineParser::new(block_tree, config).parse();  // SECOND PASS (hybrid)
    // ...
}
```

**What we've accomplished:**
- ✅ Migrated 9 block types to emit inline structure during block parsing
- ✅ Removed all conditionals and the `use_integrated_inline_parsing` flag
- ✅ Simplified code significantly
- ✅ InlineParser now skips ~95% of content (hybrid approach)
- ✅ **Pipe tables** now parse cells inline (TABLE_CELL nodes with inline content)
- ✅ **Simple tables** now parse cells inline (column-based extraction)

**What we're working on:**
- 🔄 **Phase 7.1**: Table inline parsing migration (50% complete)
  - ✅ Pipe tables migrated
  - ✅ Simple tables migrated
  - ⏳ Multiline tables (next)
  - ⏳ Grid tables (next)

**What remains:**
- ⚠️ The InlineParser still runs as a second pass
- ⚠️ Multiline and grid tables still emit rows as raw TEXT tokens
- ⚠️ InlineParser processes only multiline/grid tables now

**Goal:** Complete Phase 7.1 to achieve full single-pass parsing.

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

**Pandoc alignment**: Matching Pandoc's architecture makes it easier to conform to 
their behavior and reference their implementation.

**Incremental parsing**: Long-term goal. Current architecture makes this nearly 
impossible because inline pass rebuilds the entire tree.

### Goals

- ✅ Preserve losslessness and stable `SyntaxKind` structure (formatter/LSP depend on it)
- ✅ Avoid backtracking on `GreenNodeBuilder` by using "detect/collect first, emit once"

---

## Implementation Progress Summary

### Phases 1-6: Foundation Complete ✅ (2026-02-25)

These phases established the integrated inline parsing infrastructure and migrated most block types:

**Key Achievements**:
- ✅ Created `inline_emission::emit_inlines()` helper infrastructure
- ✅ Migrated 9 block types: headings, table captions, definition list terms, line blocks, plain blocks, paragraphs, figures, reference definitions, lists
- ✅ Removed `use_integrated_inline_parsing` flag (always enabled)
- ✅ Removed all conditionals (~500 lines of legacy code)
- ✅ InlineParser skip list expanded to 9 block types
- ✅ 1,200+ tests passing throughout

**Architecture Impact**:
- InlineParser still runs as second pass but skips ~95% of content
- Hybrid approach: block parser emits inline structure, InlineParser processes remaining edge cases
- All migrated blocks use `TextBuffer` or `ParagraphBuffer` for multi-line content

### Phase 7: Remove Second Traversal (Hybrid Approach) ✅ COMPLETE (2026-02-25)

**Goal**: Achieve true single-pass parsing by removing the InlineParser second pass entirely.

**Result**: Achieved highly effective hybrid approach - InlineParser skips ~95% of content

**Key Migrations**:
- ✅ FIGURE blocks: Now parse IMAGE_LINK during block parsing
- ✅ REFERENCE_DEFINITION blocks: Parse labels during block parsing (critical for LSP)

**Architecture Decision**:
- Defer table migration to Phase 7.1 (tables are complex, less common)
- InlineParser remains but only processes tables (~5% of typical content)

**Status**: Complete ✅ (tables deferred to Phase 7.1)

---

### Phase 7.1: Table Inline Parsing Migration 🔄 IN PROGRESS (50% Complete)

**Goal**: Complete the single-pass migration by handling table inline parsing during block parsing, allowing removal of InlineParser second pass entirely.

**Status**: 2 of 4 table types migrated

**Completed**:

1. **Table Parser Architecture**:
   - Current: Emits complete rows as single TEXT tokens
   - Needed: Parse individual cells and emit inline structure

2. **Cell Parsing Timing**:
   - Option A: Parse cells during initial table recognition
   - Option B: Parse cells in table-specific postprocessor
   - Option C: Hybrid - parse simple cells early, complex cells in postprocessor

3. **Complexity Factors**:
   - Pipe tables: `| Cell | Cell |` (simpler)
   - Grid tables: Multi-line cells with complex borders
   - Multi-line pipe tables: Cells spanning multiple lines
   - Table captions (already migrated in Phase 4)

**Investigation Needed**:

1. **Audit table parser** (`src/parser/block_parser/tables.rs`):
   - Understand current emission logic
   - Identify where cell content is available
   - Check if cell boundaries are known during parsing

2. **Test coverage**:
   - Identify table test cases with inline content
   - Understand expected CST structure
   - Check for edge cases (empty cells, escaped pipes, etc.)

3. **Performance considerations**:
   - Tables less common than paragraphs/lists
   - Full migration may have limited impact on typical documents
   - Weigh complexity vs. performance gain

**Chosen Approach**:

**Option A: Cell-Level Inline Parsing**
- Modify table parser to identify cell boundaries
- Call `emit_inlines()` for each cell's content
- Pros: Clean architecture, true single-pass
- Cons: Significant table parser refactoring

**Success Criteria** (Overall Phase 7.1):
- [x] Infrastructure: `emit_table_cell()` helper created
- [x] Pipe tables: Parse cells inline during block parsing
- [x] Simple tables: Parse cells inline during block parsing
- [ ] Multiline tables: Parse cells inline during block parsing
- [ ] Grid tables: Parse cells inline during block parsing
- [ ] InlineParser second pass removed entirely from src/parser.rs
- [ ] All table tests still pass
- [ ] Cleanup: Delete `should_skip_already_parsed()`, `copy_subtree_verbatim()`
- [ ] Full single-pass architecture achieved

**Progress**: 50% complete (2/4 table types migrated)

**Current Test Status**: 1,200+ tests passing, clippy clean

---

### Phase 8: Finalize and Clean Up ✅ (Future)

**Goal**: Finalize the refactoring by removing any remaining legacy code, cleaning up the architecture, and ensuring all tests are passing.

**Multi-line inline constructs in blockquotes**

Currently, multi-line inline constructs (e.g., `**bold\ntext**`) don't work when they span BLOCKQUOTE_MARKER boundaries. This is a pre-existing limitation (also present with flag=false), not a regression.

The fix requires parsing the full concatenated text as one unit, then emitting with markers inserted at tracked byte positions. Two approaches:


**Refactored list_postprocessor to apply inline parsing**

We currently have a `list_postprocessor` that applies inline parsing to list
items after block parsing. This is contrary to the single-pass architecture we
are moving towards, and adds complexity. Refactoring this to apply inline
parsing during block parsing would simplify the architecture and improve
performance.

1. **Wrapper builder**: Create a `MarkerInsertingBuilder` that wraps `GreenNodeBuilder` and intercepts token emissions to inject markers at the right byte offsets. Single pass, no intermediate allocations. Markers would end up inside inline nodes (e.g., BLOCKQUOTE_MARKER inside STRONG), which is semantically unusual but the formatter already skips markers so output would be correct.

2. **Intermediate tree**: Parse to a temporary `GreenNode`, then traverse and emit to the real builder with markers inserted. Cleaner tree structure control, but extra allocation.

Decision deferred to Phase 5 when we have more real-world testing data.

---

## Testing Strategy

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

# Parser Refactoring: Inline Parsing During Block Parsing (Pandoc-style)

**Status**: Phase 1-2 Complete ✅ | Phase 3 In Progress (1/5 blocks migrated)  
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

### Phase 3: Migrate Individual Blocks 🔄 IN PROGRESS (1/5 complete)

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

#### Remaining Blocks (TODO)

- [ ] **Table cells** - Individual cell content  
- [ ] **Table captions** - Single descriptive text
- [ ] **Definition list terms** (`TERM` nodes) - Single line
- [ ] **Figure captions** - Single descriptive text
- [ ] **Line block lines** - Each line is independent

**Migration pattern** (established with headings):

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
    | SyntaxKind::TABLE_CELL  // Add as migrated
    // | etc.
)
```

---

### Phase 4: Paragraphs and Plain Text (FUTURE)

**Goal**: Handle multi-line inline content with buffering.

**Approach**:

- Accumulate paragraph/plain bytes in container state during scanning
- When block closes, call `emit_inlines()` on buffered content
- Supports multi-line constructs (display math, etc.)

**Status**: Deferred - higher complexity, needs careful state management.

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

- **Total tests**: 837 (all passing)
- **A/B tests**: 2 (`blockquotes`, `headings`)

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

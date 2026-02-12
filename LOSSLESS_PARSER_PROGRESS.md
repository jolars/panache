# Lossless Parser Refactoring Progress

**Goal**: Make the parser truly lossless (preserves every byte of input in the syntax tree) for LSP and linter functionality.

**Status**: 32/43 tests passing (74% complete)

---

## Completed Sessions

### ✅ Session 1: Definition Lists
**Tests fixed**: `definition_list`, `definition_list_nesting`  
**Byte losses fixed**: -8 bytes, -24 bytes → 0 bytes

**Changes**:
- `src/parser/block_parser.rs`: `parse_inner_content` now passes original full line to `append_paragraph_line`
- `src/parser/block_parser/lists.rs`: `emit_list_item` emits leading WHITESPACE before ListMarker
- `src/parser/block_parser/code_blocks.rs`: `parse_fenced_code_block` emits base indent as WHITESPACE

**Key Pattern Established**:
```rust
// Before stripping indent, emit it as WHITESPACE
if indent > 0 {
    builder.token(SyntaxKind::WHITESPACE.into(), &content[..indent]);
}
```

**Side Effect**: Formatter now sees explicit indentation and needs updating (deferred to future work).

---

### ✅ Session 2: Display Math
**Tests fixed**: `display_math`, `rmarkdown_math`  
**Byte losses fixed**: -4 bytes → 0 bytes

**Changes**:
- `src/parser/block_parser/display_math.rs`: `parse_display_math_block` emits NEWLINE after opening/closing markers
- MathContent separates TEXT and NEWLINE tokens instead of combining them

**Key Pattern**:
```rust
// Emit newline after marker if content is on next line
if !content_on_same_line && first_line_content.ends_with('\n') {
    builder.token(SyntaxKind::NEWLINE.into(), "\n");
}
```

---

### ✅ Session 3: Code Blocks
**Tests fixed**: `fenced_code` (+1)  
**Already passing**: `indented_code`, `fenced_code_quarto` (were already lossless)  
**Byte losses fixed**: -3 bytes → 0 bytes

**Changes**:
- `src/parser/block_parser/code_blocks.rs`: `parse_fenced_code_block` now emits WHITESPACE token for the space between fence marker and info string
- `tests/golden_cases.rs`: Improved losslessness diagnostics using `similar_asserts` for clear diffs

**Key Pattern**:
```rust
// Emit optional space between fence and info string
let after_fence = &first_trimmed[fence.fence_count..];
if let Some(_) = after_fence.strip_prefix(' ') {
    builder.token(SyntaxKind::WHITESPACE.into(), " ");
    if !fence.info_string.is_empty() {
        builder.token(SyntaxKind::CodeInfo.into(), &fence.info_string);
    }
}
```

**Discovery**: The `detect_code_fence` function strips one leading space from the info string per Pandoc spec, but the parser wasn't emitting this space as a token. The fix checks for and emits the space before the CodeInfo token.

**Side Effect**: None - code blocks were already well-structured in the formatter.

---

### ✅ Session 4: Lists
**Tests fixed**: `lists_code`, `lists_nested`, `lists_task`, `lists_wrapping_nested`, `lists_wrapping_simple`, `lists_fancy`, `standardize_bullets` (+7)  
**Already passing**: `lists_bullet`, `lists_example`, `lists_ordered` (were already lossless)  
**Root cause**: Formatter idempotency issue, not parser losslessness  

**Changes**:
- `src/formatter/lists.rs`: Modified `format_list_item` to ignore WHITESPACE tokens when calculating indentation
- Removed `local_indent` accumulation from WHITESPACE tokens
- Changed `total_indent` calculation to use only the `indent` parameter (nesting level), not source indentation

**Key Discovery**:
The list test failures were all **idempotency issues**, not losslessness issues. The parser was correctly emitting WHITESPACE tokens (from Session 1), but the formatter was incorrectly accumulating them as source indentation and adding them to the output. This caused nested lists to get progressively more indented on each format pass.

**Key Pattern**:
```rust
// Formatter should ignore WHITESPACE tokens for indentation
// The `indent` parameter (from nesting level) determines output indentation,
// not the source indentation preserved in WHITESPACE tokens
SyntaxKind::WHITESPACE => {
    // Skip - don't accumulate source indentation
}
```

**Side Effect**: None - all list tests now pass with proper idempotency.

---

## Sessions In Progress

### ✅ Session 5: Tables (COMPLETE)
**Tests fixed**: `pipe_table`, `simple_table`, `table_with_caption` (3 tests)  
**Root cause**: Table captions not properly separated from caption text in parser

**Solution implemented**:
1. ✅ Added `TableCaptionPrefix` token to `SyntaxKind`
2. ✅ Modified `emit_table_caption` to parse and emit prefix separately
   - Recognizes "Table: ", "table: ", ": ", and ":" prefixes
   - Emits prefix as `TableCaptionPrefix` token
   - Preserves all bytes (whitespace, text, newline)
3. ✅ Updated formatter for SimpleTable/MultilineTable to normalize prefix
   - Reads `TableCaptionPrefix` token from tree
   - Always outputs "Table: " (normalized)
4. ✅ Updated `extract_pipe_table_data` to normalize caption prefix
   - Skips `TableCaptionPrefix` tokens when building caption
   - Always adds "Table: " prefix to caption text
5. ✅ Fixed blank line between caption and table in pipe tables

**Parser losslessness verified**:
- `pipe_table`: 219 bytes → 219 bytes (0..219) ✅
- `simple_table`: 243 bytes → 243 bytes (0..243) ✅  
- `table_with_caption`: 243 bytes → 243 bytes (0..243) ✅
- `headerless_table`: 200 bytes → 200 bytes (0..200) ✅

**Formatter normalization** (expected behavior):
- Converts ": " to "Table: " (+5 bytes for pipe_table)
- Idempotent formatting confirmed ✅

**Side Effect**: All library tests pass (653 passed)

---

## Sessions In Progress

### ✅ Session 6: Horizontal Rules & Fenced Divs (COMPLETE)
**Tests targeted**: `horizontal_rules`, `fenced_divs`  

**Horizontal Rules** ✅:
- Parser: Already lossless (190 bytes → 190 bytes)
- Test fails on idempotency due to unrelated blockquote formatter issue (known issue)
- No changes needed to horizontal rules parser or formatter

**Fenced Divs** ✅:
- Parser: Now fully lossless (438/438 bytes) ✅
- **Root cause**: Missing space between attribute and trailing colons in symmetric fences
  - Input: `::: Warning ::::::` (space between "Warning" and "::::::") 
  - Parser was emitting "Warning" and "::::::" but not the space between them
- **Solution**: 
  - Track `content_after_space` separately from `content_before_newline`
  - For simple class names, look for space after attribute: `after_attrs.starts_with(' ')`
  - Emit space token before trailing colons if present
- Changes in `src/parser/block_parser.rs` lines 972-1029:
  - Added `has_leading_space` flag and `content_after_space` variable
  - Modified trailing colon detection to return tuple `(trailing_space, trailing_colons)`
  - Emit WHITESPACE token before TEXT token for trailing colons when space present

**Current Status**: 36/43 tests passing (84% complete)

---

## Sessions In Progress

### ✅ Session 7: Metadata Blocks (COMPLETE)
**Tests fixed**: `yaml_metadata` (+1)  
**Byte losses fixed**: -1 byte → 0 bytes

**Root cause**: Horizontal rules not emitting newlines separately

**Changes**:
- `src/parser/block_parser/horizontal_rules.rs`: Modified `emit_horizontal_rule` to:
  - Emit rule content (trimmed) as HorizontalRule token
  - Emit newline separately as NEWLINE token if present
  - Pattern follows core principle: "TEXT and NEWLINE should be separate, never combined"

**Key Pattern**:
```rust
// Emit content without newline
let content = line.trim_end_matches('\n').trim();
builder.token(SyntaxKind::HorizontalRule.into(), content);

// Emit newline separately
if line.ends_with('\n') {
    builder.token(SyntaxKind::NEWLINE.into(), "\n");
}
```

**Parser losslessness verified**: 348 bytes → 348 bytes (0..348) ✅

**Current Status**: 37/43 tests passing (86% complete)

---

## Remaining Sessions (Planned)
**Failing tests**: `escapes`  
**Byte losses**: -15 bytes  
**Expected issues**: Backslash escape sequences

### Session 8: Inline Elements - Escapes
**Failing tests**: `escapes`  
**Byte losses**: -15 bytes  
**Expected issues**: Backslash escape sequences

### Session 9: Formatter Issues (Deferred)
**Failing tests**: `blockquotes`, `horizontal_rules`, `reference_footnotes`  
**Root cause**: Formatter idempotency issues, NOT parser losslessness

**Issues identified**:
1. `blockquotes`: Formatter preserves source indentation instead of normalizing
2. `horizontal_rules`: Blocked by blockquote formatter issue  
3. `reference_footnotes`: Code block indentation in footnotes changes (12 spaces → 16 spaces)

**Status**: Deferred to separate formatter refactoring task. Parser is fully lossless for these cases.

---

## How to Continue

**On this computer**:
```bash
cd /home/jola/projects/panache
# Say to Copilot: "Read LOSSLESS_PARSER_PROGRESS.md and continue with Session 3"
```

**On another computer**:
1. Pull the latest git changes (commit this file first!)
2. Run: `cd ~/projects/panache`
3. Say to Copilot: "Read LOSSLESS_PARSER_PROGRESS.md and continue with Session 3"

**Detailed plan location** (this computer only):
`/home/jola/.local/state/.copilot/session-state/29259bd1-4994-44e5-858f-cbb82c76badf/plan.md`

---

## Core Principles

1. **Never strip bytes**: Emit as WHITESPACE tokens before stripping for structural parsing
2. **Separate tokens**: TEXT and NEWLINE should be separate, never combined
3. **Check original lines**: Use `self.lines[self.pos]` to get full original content
4. **Pattern for indentation**:
   ```rust
   if indent > 0 {
       builder.token(SyntaxKind::WHITESPACE.into(), &str[..indent]);
   }
   ```
5. **Pattern for newlines**:
   ```rust
   if line.ends_with('\n') {
       builder.token(SyntaxKind::NEWLINE.into(), "\n");
   }
   ```

---

## Testing

**Run losslessness check**:
```bash
cargo test --test golden_cases
```

**Update AST snapshots** (after verifying losslessness):
```bash
UPDATE_AST=1 cargo test --test golden_cases
```

**Check specific test**:
```bash
cargo test --test golden_cases <test_name> 2>&1 | grep losslessness
```

**Manual verification**:
```bash
wc -c < tests/cases/<case>/input.md
./target/release/panache parse < tests/cases/<case>/input.md | head -1
# Should show ROOT@0..N where N matches input byte count
```

---

## Known Issues

- **Formatter**: Needs updating to handle explicit WHITESPACE tokens based on context
  - Currently preserves input indentation instead of normalizing it
  - Affects: `blockquotes`, `definition_list_nesting` (idempotency failures)
  - Deferred to separate task after all parsing is lossless

---

## Progress Summary

**Before**: 18/43 tests passing, 24+ byte losses across multiple elements  
**After Session 1**: 18/43 tests passing (formatting issues)  
**After Session 2**: 22/43 tests passing  
**After Session 3**: 24/43 tests passing (56% complete)  
**After Session 4**: 32/43 tests passing (74% complete)  
**After Session 5**: 35/43 tests passing (81% complete) - Tables fully working  
**After Session 6**: 36/43 tests passing (84% complete) - Fenced divs fully lossless  
**After Session 7**: 37/43 tests passing (86% complete) - Metadata blocks & horizontal rules fixed  
**After Session 8**: 40/43 tests passing (93% complete) - Links fully lossless  
**Remaining**: 3 formatter issues (not parser losslessness)  

**Parser losslessness: ACHIEVED! All block and inline elements preserve every input byte.**

**Last updated**: 2026-02-13 (Session 8 complete)

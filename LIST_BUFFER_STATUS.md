# List Tight/Loose Detection - Implementation Status

**Date**: 2026-02-26  
**Status**: Phase 2 in progress - buffer approach with losslessness bugs

## The Problem

Markdown lists can be "tight" (compact) or "loose" depending on blank lines:

```markdown
Tight list (no blank lines):
- Item 1
- Item 2

Loose list (blank line between items):
- Item 1

- Item 2
```

In our CST, tight items wrap content in `PLAIN`, loose items wrap in `PARAGRAPH`.

**The challenge**: We don't know if a list is loose until we've seen all items. But `GreenNodeBuilder` is append-only - once we emit a node, we can't change it.

Pandoc solves this with `compactify` - a post-processing pass that transforms the AST after parsing. We need an equivalent solution for our CST.

## Current State (commit cc83003)

The commit attempted to:
1. Buffer list item content in `ListItemBuffer`
2. Track `has_blank_between_items` on the List container
3. Emit PLAIN or PARAGRAPH when each item closes

**Problem**: The first item closes BEFORE we see the blank line, so it gets the wrong wrapper.

**Failing tests**: 3 tests fail due to this ordering issue.

---

## Option 1: Item-Level Buffering with Pre-Detection

Buffer item content, but mark the list as loose BEFORE closing items.

**Approach**:
- When blank line is seen between items, mark list loose immediately
- Close first item with correct PLAIN/PARAGRAPH

**Challenges**:
- Need to distinguish "blank line between items" from "blank line after list"
- Requires look-ahead to know if another item follows
- Complex edge cases with nested structures

**Pros**: Single emission, no tree rebuild  
**Cons**: Complex detection logic, fragile

---

## Option 2: List-Level Buffering (Emit Once)

Don't emit LIST_ITEM nodes during parsing. Buffer everything, emit when List closes.

**Approach**:
- When List starts, push container (don't emit LIST node yet)
- Collect all item data in container
- When List closes, determine tight/loose, emit everything

**Challenges**:
- Nested content (code blocks, nested lists) inside items
- Need to track what's inside each item without emitting
- May require sub-builders for nested block content

**Pros**: Purest "emit once" approach, correct by construction  
**Cons**: Most complex to implement, nested content handling

---

## Option 3: Sub-Builder + Transform

Use a separate GreenNodeBuilder for each list. Transform when done.

**Approach**:
1. When List starts, push new `GreenNodeBuilder` to a stack
2. Emit all list content to the sub-builder
3. When List closes:
   - Finish sub-builder → get `GreenNode`
   - Walk tree, transform PLAIN↔PARAGRAPH based on `has_blank_between_items`
   - Re-emit transformed tree to parent builder
4. Pop builder stack

**Challenges**:
- Need to walk and re-emit the tree (similar to old postprocessor)
- Stack management for nested lists
- Performance overhead of tree walk + re-emission

**Pros**: Clean separation, localized rebuild (only list content)  
**Cons**: Involves tree transformation, not pure single-emission

---

## Option 4: Global Post-Processing (Old Approach)

Parse entire document, then walk and transform lists.

**Approach**:
- Parse everything with PLAIN wrappers
- Post-process: walk tree, find lists with blank lines, transform PLAIN→PARAGRAPH
- This was the deleted `list_postprocessor.rs`

**Challenges**:
- Full tree rebuild
- Not truly single-pass

**Pros**: Simple to implement, proven to work  
**Cons**: Rebuilds entire tree, not localized

---

## Option 5: Hybrid - Transform at List Close

Keep current parsing, but when List container closes, transform its children.

**Approach**:
- Emit LIST and LIST_ITEM nodes during parsing (as now)
- Use PLAIN by default
- When List closes, if `has_blank_between_items`:
  - Get the LIST subtree somehow
  - Transform PLAIN→PARAGRAPH
  - Replace in builder

**Challenges**:
- GreenNodeBuilder doesn't support "get last node" or "replace node"
- Would need to track byte positions or use a different builder strategy

**Pros**: Minimal change to current approach  
**Cons**: May not be possible with current rowan API

---

## Pandoc Reference

Pandoc's `compactify` function (in `Shared.hs`):

```haskell
compactify :: [Blocks] -> [Blocks]
compactify items =
  let (others, final) = (init items, last items)
  in  case reverse (B.toList final) of
         (Para a:xs)
           | null [Para x | Para x <- xs ++ concatMap B.toList others]
           -> others ++ [B.fromList (reverse (Plain a : xs))]
         _ | null [Para x | Para x <- concatMap B.toList items]
           -> items
         _ -> map (fmap plainToPara) items
```

Key insight: Pandoc's rule is:
- If ANY item has Para (except possibly at the end of the last item), ALL become Para
- If only the last item ends in Para, convert that Para→Plain
- Otherwise, keep as-is

---

---

## CURRENT STATUS (2026-02-26 Evening Session)

### ✅ Phase 1: COMPLETE
**Fixed the original bug** - first list item content now emits properly.

**What we did**:
- Identified that `continue_list_at_level()` was calling `containers.close_to()` which bypassed buffer emission
- Replaced all calls with direct `close_containers_to()` in core.rs
- Removed `continue_list_at_level()` function entirely
- Result: **All 3 originally failing lib tests now pass** ✅

**Key insight**: The container stack's `close_to()` is simple (just pop + finish_node), but `Parser::close_containers_to()` handles buffer emission. Must use the latter.

### 🚧 Phase 2: IN PROGRESS
**Simplified to per-item decision** - removed list-level loose tracking.

**What we did**:
- Removed `has_blank_between_items` field from `Container::List`
- Changed buffer emission to only check item-level: `buffer.has_blank_lines_between_content()`
- Added logic to emit buffer BEFORE:
  - Starting nested lists (line ~1818 in core.rs)
  - Starting code blocks (line ~1380 in core.rs)

**Current result**: 
- ✅ CST now has mixed PLAIN/PARAGRAPH as designed (per-item decision working)
- ❌ **14 golden tests failing** - losslessness violations

### ❌ Current Problems

**Problem 1: Blank lines emitted as nodes instead of buffered**

Example CST (WRONG):
```
LIST_ITEM
  LIST_MARKER "-"
  WHITESPACE " "
  BLANK_LINE <- Should NOT be here
  PLAIN <- text here
```

Should be:
```
LIST_ITEM
  LIST_MARKER "-"
  WHITESPACE " "
  PLAIN <- text with blank lines buffered inside
```

**Root cause**: Lines 482-485 in `core.rs` ALWAYS emit BLANK_LINE nodes, even after we buffer them at line 431.

**The logic flow**:
1. Line 431: We buffer the blank line with `buffer.push_blank_line()`
2. Line 438: We `break` to avoid closing the ListItem
3. Lines 482-485: We STILL emit a BLANK_LINE node ← BUG!

**Attempted fix**: Added `buffered_in_list_item` flag to track when we buffer, skip emission if true. But tests still show BLANK_LINE nodes appearing.

**Problem 2: Content appearing after blocks instead of before**

Example from input:
```markdown
* List item text
  
  Continuation paragraph
  
  ```python
  code
  ```
```

Current CST (WRONG order):
```
LIST_ITEM
  MARKER
  CODE_BLOCK <- appears first
  PLAIN <- text appears second
```

Should be:
```
LIST_ITEM
  MARKER  
  PLAIN <- text appears first
  CODE_BLOCK <- block appears second
```

**Root cause**: We buffer text as we parse, but when we encounter a block structure, we emit the block immediately. Then when the list item closes, we emit the buffered text AFTER all the blocks.

**Attempted fix**: Emit buffer BEFORE starting blocks (nested lists, code). But blank lines are still causing ordering issues.

### Why Buffer Approach SHOULD Work

**Key insight from discussion**: 
- Pandoc collects raw text, then re-parses it (`parseFromString`)
- We can't do that with rowan's append-only builder
- BUT paragraphs buffer successfully!

**Why paragraphs work**: Paragraphs can't contain blocks - only inline content. So buffering is simple.

**Why list items are harder**: List items CAN contain blocks (code, nested lists, etc.). We need to:
1. Buffer text content
2. When we see a block, emit buffered text FIRST, then the block
3. Continue buffering after the block
4. Don't emit blank lines as nodes - they should only affect buffer content

### Next Steps to Fix

**Immediate fixes needed**:

1. **Fix blank line emission** (highest priority):
   - The `buffered_in_list_item` flag isn't working
   - Debug why BLANK_LINE nodes still appear
   - Ensure blank lines in list items are ONLY in buffer, never as nodes

2. **Emit buffer before ALL block structures**, not just lists/code:
   - Horizontal rules
   - Blockquotes
   - Tables
   - Fenced divs
   - LaTeX environments
   - HTML blocks
   - etc.

3. **Handle continuation after blocks**:
   - After emitting a block, continue buffering subsequent text
   - Don't lose blank lines between blocks

4. **Test edge cases**:
   - Multiple paragraphs in one item
   - Multiple code blocks in one item
   - Nested lists with text before and after

### Files Modified So Far

- `src/parser/core.rs` - Main parsing logic, buffer emission
- `src/parser/utils/container_stack.rs` - Removed `has_blank_between_items` 
- `src/parser/utils/list_item_buffer.rs` - Made `clear()` pub(crate)
- `src/parser/blocks/lists.rs` - Removed `continue_list_at_level()`, simplified List container

### Test Results

- ✅ Lib tests: 827 passed (all)
- ❌ Golden tests: 79 passed, **14 failed** (losslessness)
- ❌ Format tests: Some failures expected (formatter not updated yet)

**Failing golden tests** (all losslessness):
- `blockquote_depth_change`
- `blockquote_list_blanks`
- `blockquote_list_blockquote`
- `blockquotes`
- `definition_list_nesting`
- `lazy_continuation_deep`
- `lists_bullet`
- `lists_code`
- `lists_nested`
- `lists_task`
- `lists_wrapping_nested`
- `lists_wrapping_simple`
- `paragraph_continuation`
- `standardize_bullets`

### Decision: Continue with Buffer Approach

**Rationale**:
- The buffer approach is architecturally sound (proven by paragraphs)
- We're close - just have implementation bugs
- The per-item decision (Phase 2 goal) is working correctly
- Switching strategies now would waste progress

**Confidence level**: High - the bugs are identifiable and fixable.

---

## Questions for Discussion

1. ~~Which approach best balances correctness, performance, and maintainability?~~ **DECIDED: Buffer approach (Option 6 from original plan)**
2. ~~Is tree transformation acceptable if localized to lists?~~ **NOT NEEDED: Buffer works**
3. Should we match Pandoc's `compactify` exactly, or use simpler "blank between items" heuristic? **Using per-item decision**
4. ~~How important is "single emission" vs correctness?~~ **Correctness first - we emit buffer before blocks**

---

## Files Involved

- `src/parser/core.rs` - Main parser, container handling, buffer emission logic
- `src/parser/utils/container_stack.rs` - Container definitions (List simplified)
- `src/parser/utils/list_item_buffer.rs` - Item content buffering
- `src/parser/blocks/lists.rs` - List marker parsing, item emission (simplified)

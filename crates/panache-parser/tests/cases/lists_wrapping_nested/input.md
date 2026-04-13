---
title: "Nested List Wrapping Tests"
---

## Deep Nesting with Long Lines

- Top level item with a very long description that needs wrapping to demonstrate how the formatter handles text that exceeds the line width limit at the outermost level.
  - Second level nested item that also has a very long description which should be wrapped while maintaining the correct indentation relative to its parent item.
    - Third level item with another long line that tests whether the hanging indentation works correctly even at deeper nesting levels.
      - Fourth level with more text that is quite long and should wrap properly
  - Back to second level with a long line that contains multiple sentences. The first sentence is quite lengthy. The second sentence adds more content. The third sentence ensures we test wrapping thoroughly.
- Another top level item
  1. Nested ordered list with a very long description that should maintain proper indentation
  2. Second ordered item at this level
     - Bullet inside ordered list with a long description that needs wrapping
     - Another bullet with even more text that goes on and on requiring the formatter to wrap it properly
  3. Third ordered item returning to numbered format

## Task Lists with Deep Nesting

- [ ] Top level unchecked task with a very long description that should be wrapped while preserving the checkbox syntax and maintaining readability.
  - [ ] Nested unchecked task that also has a long description requiring wrapping
    - [x] Deeply nested completed task with text that is long enough to require wrapping at this indentation level
  - [x] Back to second level completed task with a very long line that tests the formatter's ability to handle wrapped text in completed tasks.
- [x] Another top level completed task

## Roman Numerals with Wrapping

  i. First Roman numeral item with a very long description that tests right-alignment and wrapping together
 ii. Second item
iii. Third item with another long description that should wrap properly while maintaining the right-aligned Roman numeral formatting
 iv. Fourth item with a moderately long description
  v. Fifth item with the longest description of all which goes on and on and should definitely require wrapping to multiple lines

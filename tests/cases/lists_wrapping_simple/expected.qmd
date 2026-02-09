---
title: "List Wrapping Tests"
---

## Bullet Lists with Long Lines

- This is a very long line that should be wrapped because it exceeds the default
  line width of 80 characters.
- Short item
- Another extremely long line that contains a lot of text and should be wrapped
  to multiple lines with proper hanging indentation maintained throughout.
- Final item with `inline code` that should be preserved and a very long
  sentence that continues beyond the line width limit.

## Ordered Lists with Long Lines

1. This is the first item with a very long description that exceeds 80
   characters and needs to be wrapped properly.
2. Second item
3. The third item has an even longer description that goes on and on, containing
   multiple clauses and phrases that make it necessary to wrap the text across
   several lines while maintaining proper indentation.

## Task Lists with Long Lines

- [ ] This unchecked task has a very long description that should wrap properly
      while maintaining the checkbox format.
- [x] Completed task
- [ ] Another unchecked task with an extremely long description that includes
      multiple sentences. The first sentence is long. The second sentence is
      also quite long and helps test the wrapping behavior.

## Mixed List Types

- Bullet point with reasonable length
  1. Nested ordered item that is short
  2. Nested ordered item with a very long description that needs to be wrapped
     to maintain readability
- Another bullet with a very long line that should be wrapped while the nested
  items below should maintain their proper indentation
  - Nested bullet one
  - Nested bullet two with a longer description that might also need wrapping
    eventually

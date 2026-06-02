# Empty List Items

## Bare bullet markers (should be flagged)

- Item one
-
- Item three

## Bare ordered markers (should be flagged)

1.
2. Second
3.

## Empty bullet that became a Setext H2 underline (should be flagged)

- Setext bullet trap
  -

## Setext H1 underline inside a list item (should NOT be flagged)

- Heading one
  ===

## Items with only whitespace after marker (should be flagged)

- Item
-   
- Item three

## Valid non-empty items (should NOT be flagged)

- One
- Two
- Three

1. First
2. Second

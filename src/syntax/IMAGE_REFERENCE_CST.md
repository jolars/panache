# Image Reference CST Notes

This note records the parser/CST normalization for reference-style images (for
example, `![alt][img]`) and the resulting invariant for typed consumers.

## Current Invariant

- `Link` nodes expose reference labels through a child `LINK_REF` node.
- `ImageLink` nodes expose reference labels through a child `LINK_REF` node for
  explicit and collapsed reference forms.

Typed consumers can treat links and image links consistently for reference
target extraction.

## Why This Matters

- Wrapper ergonomics are aligned (`Link::reference()` / `ImageLink::reference()`).
- LSP features can share reference-target extraction paths.
- AST coverage/refactors no longer depend on token-shape fallbacks.

## Implemented CST Shape

For reference-style images, parser emission includes a `LINK_REF` child under
`IMAGE_LINK`.

Target shape sketch:

```text
IMAGE_LINK
  IMAGE_LINK_START "!["
  IMAGE_ALT ...
  TEXT "]"
  TEXT "["
  LINK_REF
    TEXT "img"
  TEXT "]"
```

For collapsed and shortcut forms:

- `![alt][]` -> `LINK_REF` with empty label text between brackets
- `![alt]` -> shortcut form remains without the second bracket pair (`LINK_REF`
  is not emitted)

## Non-Negotiables

- Preserve byte-for-byte losslessness (all markers, whitespace, ordering).
- Do not move formatter policy into parser logic.
- Keep existing single-pass inline parsing model.
- Keep user-visible formatting behavior unchanged.

## Verification Done

1. Parser/AST tests assert `LINK_REF` presence for explicit and collapsed image
   references.
2. Wrapper methods use `ImageLink::reference()` as the primary source.
3. LSP helpers/symbol resolution now use shared reference-target extraction.
4. Token scanning fallback for image reference labels is removed.

This invariant should be maintained for future parser and wrapper changes.

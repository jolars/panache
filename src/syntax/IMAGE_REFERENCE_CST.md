# Image Reference CST Notes

This note captures a parser/CST follow-up discovered while improving LSP
wrapper coverage for reference-style images (for example, `![alt][img]`).

## Current State

- `Link` nodes expose reference labels through a child `LINK_REF` node.
- `ImageLink` nodes currently encode reference syntax as plain `TEXT` tokens
  after `IMAGE_ALT` (`]`, `[`, label text, `]`), without a `LINK_REF` child.

That means typed consumers can treat links and image links differently even
when the source syntax concept is the same (reference label target).

## Why This Matters

- Wrapper ergonomics diverge (`Link::reference()` works directly; image
  reference handling needs token-shape-specific logic).
- LSP features that should be syntax-agnostic (goto/rename/prepare) need extra
  image-specific branches.
- Future AST coverage and refactors are harder than necessary.

## Proposed CST Shape Adjustment (Future)

For reference-style images, emit a `LINK_REF` child under `IMAGE_LINK` with the
same internal token structure used for links.

Target shape sketch:

```text
IMAGE_LINK
  IMAGE_LINK_START "!["
  IMAGE_ALT ...
  LINK_REF
    TEXT "["
    TEXT "img"
    TEXT "]"
```

For collapsed and shortcut forms:

- `![alt][]` -> `LINK_REF` with empty label text between brackets
- `![alt]` -> may remain explicit shortcut encoding or be normalized to an
  empty/implicit `LINK_REF` form, as long as losslessness is preserved

## Non-Negotiables

- Preserve byte-for-byte losslessness (all markers, whitespace, ordering).
- Do not move formatter policy into parser logic.
- Keep existing single-pass inline parsing model.
- Do not change user-visible formatting behavior as part of CST reshaping.

## Migration Plan

1. Add parser tests that assert the new CST node presence for image references.
2. Add/adjust wrapper tests so `ImageLink::reference()` is the primary path.
3. Keep temporary compatibility in LSP helpers while landing parser changes.
4. Remove fallback token scanning once parser shape is stable.

This is a structural cleanup item, not a blocker for current LSP behavior.

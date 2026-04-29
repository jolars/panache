# CommonMark conformance — running session recap

This file is the rolling, terse handoff between sessions of the
`commonmark-conformance-expand` skill. Read it at the start of a session for
suggested next targets and known follow-ups; rewrite the **Latest session**
entry at the end with what changed and what to look at next. Remove and replace
the "Latest session" entry with a new one at the end of each session, but 
check if there is something from the prior session that should be
carried forward.

Keep entries short. The full triage data lives in
`crates/panache-parser/tests/commonmark/report.txt` and
`docs/development/commonmark-report.json`; this file is for the *judgment calls*
a fresh session can't reconstruct from those artifacts (why a target was picked,
what was deliberately skipped, which fix unlocked which group).

--------------------------------------------------------------------------------

## Latest session — 2026-04-29 (iv)

**Pass count: 572 → 574 / 652 (88.0%, +2)**

Two Lists-section wins from independent root causes: #320
(renderer-only, loose-list mis-detection) and #301 (dialect
divergence on bullet-marker matching). The recap's prior "don't
redo: #320 needs a parser fix" was wrong — under the current
CommonMark flavor defaults, the parser already produces a clean
`LIST_ITEM > BLOCK_QUOTE` shape for `* a\n  > b\n  >\n* c\n`; the
remaining gap was purely a renderer bug.

### Root cause #1: `list_item_ends_with_blank` walked into BLOCK_QUOTEs

The previous-session helper used `node.descendants()` to find a
BLANK_LINE whose end byte coincides with the LIST_ITEM's end. That
walk crosses into BLOCK_QUOTE children, so for #320 the blockquote-
internal blank (`  >\n`) — which is *blockquote* content, not
document-level whitespace — was treated as if the outer item ended
with a blank. That made the list loose, the renderer added
`<p>` wrappers around `a` and `c`, and the spec output (tight
list) did not match.

Fix: replace `descendants()` with a custom walk that only descends
into LIST and LIST_ITEM. Blank lines inside BLOCK_QUOTE (or any
other strict container that owns its own blank lines) are no
longer seen by the outer-list looseness check. The #326 pattern
still works because its BLANK_LINE lives one level deep inside an
inner LIST, not behind a BLOCK_QUOTE.

### Root cause #2: bullet markers always matched across dialects

`markers_match` returned `true` for any two `Bullet(_)` markers
regardless of character. CommonMark §5.3 makes `-`, `+`, `*`
*distinct* bullet types — switching characters at the same indent
starts a new list (verified with pandoc: `pandoc -f commonmark -t
native` splits, `pandoc -f markdown -t native` joins). So the
parser was producing one LIST for `- foo\n- bar\n+ baz\n` instead
of two.

Fix: thread `Dialect` into `markers_match` and
`find_matching_list_level`, and gate the bullet-character check on
`Dialect::CommonMark`. Pandoc dialect keeps the existing "any
bullet matches any bullet" behavior.

### Formatter idempotency follow-on

The parser fix exposed a formatter idempotency bug under
CommonMark: the formatter unconditionally normalized `*`/`+` to
`-`, which silently merged the two CommonMark lists into one and
required two format passes to stabilize (every-pair blanks vs.
single-blank). Fix: in
`crates/panache-formatter/src/formatter/lists.rs`, route LIST_MARKER
output through `normalize_bullet_for_output(&self, raw)`, which
preserves the source character when
`Dialect::for_flavor(flavor) == Dialect::CommonMark` and keeps the
existing standardize-to-`-` behavior under Pandoc/Quarto/etc.
(Pandoc's `standardize_bullets` golden case is unaffected.)

### Files changed

- **Renderer gap**:
  - `crates/panache-parser/tests/commonmark/html_renderer.rs`:
    replaced `descendants()`-based scan in
    `list_item_ends_with_blank` with `descendant_blank_at_end`
    that walks only LIST/LIST_ITEM children.
- **Dialect divergence (parser)**:
  - `crates/panache-parser/src/parser/blocks/lists.rs`:
    `markers_match` and `find_matching_list_level` take
    `dialect: Dialect`; bullet pair matching gated on it.
  - `crates/panache-parser/src/parser/core.rs`,
    `.../parser/utils/continuation.rs`: pass `self.config.dialect`
    at the three call sites.
- **Formatter idempotency**:
  - `crates/panache-formatter/src/formatter/lists.rs`: new
    `normalize_bullet_for_output` method; replaces inline bullet
    standardization at the LIST_MARKER output site. Static
    `extract_list_marker` left as-is (used for width/indent
    only — bullets are 1 byte each, normalization is harmless
    there).
- **New parser fixtures + snapshots**:
  - `list_item_blockquote_internal_blank_commonmark`: pins the
    `LIST_ITEM > BLOCK_QUOTE > BLANK_LINE` CST that the renderer
    fix now leans on (so the invariant doesn't rot silently in
    `html_renderer.rs`).
  - `list_mixed_bullets_commonmark` /
    `list_mixed_bullets_pandoc`: paired dialect-divergence
    fixtures pinning the two-LIST vs one-LIST CST split.
- **Formatter golden case**:
  - `tests/fixtures/cases/list_mixed_bullets_commonmark/` with
    `flavor = "commonmark"` — pins formatted output and exercises
    idempotency for the CommonMark-only "two adjacent lists"
    block sequence.
- **Allowlist additions** (Lists): #301, #320.

### Don't redo

- Don't merge `extract_list_marker` and
  `normalize_bullet_for_output`. The static helper's normalization
  is fine for width/indent math (all bullets are 1 byte) and
  changing it would force every static caller to take a `Config`
  or `Dialect` parameter. The output site is the only place where
  the character actually matters.
- Don't add a paired Pandoc formatter golden for
  `list_mixed_bullets_*`. Pandoc's behavior here (single
  collapsed list) is already covered by the existing
  `standardize_bullets` fixture; duplicating is the churn the
  rule explicitly warns against.
- Don't reintroduce the `descendants()`-based scan in
  `list_item_ends_with_blank`. The walk-only-LIST/LIST_ITEM
  helper is what spec §5.3 actually requires; BLOCK_QUOTEs and
  CODE_BLOCKs own their own blank lines.
- Recap (iii) said "#320 needs a parser fix (stray PARAGRAPH for
  `  > b`)". That was true under the *prior* CommonMark defaults.
  Current defaults already produce the clean BLOCK_QUOTE shape
  under `Flavor::CommonMark`, so #320 is now renderer-only. Don't
  go hunting for the parser-side blockquote-continuation fix
  again — re-verify with `printf '...' | cargo run -- --config
  /tmp/cm.toml parse` first.

### Suggested next targets, ranked

1. **Empty list item closes the list when followed by blank line
   (#280)** — `-\n\n  foo\n` should produce
   `<ul><li></li></ul><p>foo</p>`. Parser-shape gap: parser keeps
   `  foo` inside the same LIST_ITEM as a second PLAIN child
   instead of closing the list. Touches list-item continuation
   when the item starts with bare-marker + blank.
2. **List with non-uniform marker indentation (#312)** —
   `- a\n - b\n  - c\n   - d\n    - e\n` should keep all five at
   the same list level (last "- e" is lazy continuation of "d"
   per CommonMark indent rules). Currently splits at "- e"
   because the parser interprets 4-space indent as starting a
   nested list. Parser-shape gap; touches list-marker indent
   tracking.
3. **Tabs (#2, #5, #6, #7)** — column-aware tab expansion for
   indented-code inside containers. Substantial; touches
   `leading_indent` and tab-stop logic.
4. **HTML block #148** — raw HTML `<pre>`-block contains a blank
   line that should be emitted verbatim, but our parser/renderer
   reformats `_world_` as inline emphasis inside the `<pre>`. May
   be a renderer bug (HTML block content should be byte-perfect).
5. **Reference link followed by another bracket pair (#569, #571)**
   — requires CMark "left-bracket scanner" stack model. Large.
6. **Nested LINKs in link text (#518, #519, #520, #532, #533)** —
   CommonMark §6.4 forbids real nesting; outer must un-link
   itself when inner resolves. Same scanner-stack work as #569.
7. **HTML-tag/autolink interaction with link brackets (#524, #526,
   #536, #538)** — bracket scanner must skip past raw HTML and
   autolinks too.
8. **Block quotes lazy-continuation #235, #251** — last two
   blockquote failures.
9. **Fence inside blockquote inside list item (#321)**.
10. **Lazy / nested marker continuation (#298, #299)**.
11. **Multi-block content in `1.     code` items (#273, #274)**.
12. **Setext-in-list-item (#300)**.
13. **Emphasis and strong emphasis (47 fails)** — flanking-rule
    edge cases. #352 (`a*"foo"*`), #354 (`*$*alpha`),
    #366/#367/#368/#369, #372–376 (underscore intra-word). Need
    proper CommonMark flanking-rule gating; current emphasis
    parser leans on Pandoc's looser semantics.
14. **Ref-def dialect divergence #201** — `[foo]: <bar>(baz)`. Low
    priority.

# Math parser/formatter — running session recap

Concise handoff between sessions of the `math-parser-formatter` skill. Read it
for the latest result and suggested next sub-targets. At the end of a session,
rewrite those sections instead of accumulating history.

--------------------------------------------------------------------------------

## Persistent implementation details

- **Read math content via `syntax::math::math_content_text()`**, never
  `MATH_CONTENT.text()`. The block machinery interleaves container prefixes
  (`LINE_PREFIX`, and sometimes host `NEWLINE`) into `MATH_CONTENT` on
  continuation lines; the helper strips them by whitelisting `MATH_*` tokens.
  Reading `.text()` directly leaks the `>` and re-accumulates it every format
  pass (a real idempotency bug that was fixed in Phase 1).
- **`MATH_SPACE`/`MATH_NEWLINE` are intentionally distinct** from host
  `WHITESPACE`/`NEWLINE` — that distinction is what makes the helper above work.
  `MATH_SPACE` is load-bearing (collides with blockquote-prefix `WHITESPACE`
  otherwise); `MATH_NEWLINE` is kept for symmetry.
- Operator class and precedence are semantic interpretation, not CST shape;
  macros can override them. Keep the lexical CST neutral and share the
  Panache-owned interpretation between consumers.
- **Scripts are native CST structure.** `MATH_SCRIPTED` owns one base atom and
  `MATH_SUBSCRIPT`/`MATH_SUPERSCRIPT` children. Unbraced text bases and
  arguments split at Unicode-scalar boundaries; comments and blank lines stop
  attachment. Formatter interpretation must inherit the base atom's class
  across the script—especially for scripted relation and assignment breaks.
- **Contextual coercion follows Badness's role model, not full Appendix G.** A
  `Bin` becomes `Ord` at list start, after an effective binary or relation, or
  after an atom with `DelimiterRole::Open`. `Punct`, `Op`, and an `Open` class
  without a genuine delimiter role remain operands for this purpose.
- **Authored `\\` breaks layout rows but not the semantic atom stream.** Lower
  each row separately while deriving every row's atoms from one source-ordered
  stream; otherwise, a sign after `\\` is incorrectly coerced as though it
  started a new math list.
- **Some scripted composite relations expose a pinned Badness defect.** Badness
  still splits a non-colon relation head from its CST-separated scripted tail
  (`<=_i` → `< =_i`, likewise `>=_i` and `==_i`). Panache preserves those
  relations through the compatibility path; exclude them from mandatory byte
  parity until the pinned oracle is corrected. Definition relations, including
  `:=_i`, now have byte parity.

--------------------------------------------------------------------------------

## Latest session

**Comments in final environment-grid cells.** Routed one supported nested
comment-bearing construct in the final cell of a top-level environment row
through the shared typed document IR.

- Mandatory byte parity with Badness now covers groups, signature-proven math
  arguments, braced scripts, and paired delimiters in final grid cells,
  including Unicode content and rows terminated by authored `\\` markers.
- Continuation indentation composes the environment's two-column indent, the
  aligned cell start, and the nested construct's hanging offset. Column widths
  continue to use Unicode-scalar counts, and every migrated case is explicitly
  idempotent.
- Multiline non-final cells remain on the compatibility path. Badness does not
  apply its ordinary grid spacing to the separator after such a cell, so that
  behavior needs its own bounded analysis before migration.
- The committed 107-case report is unchanged because this slice adds focused
  controlled bodies rather than new corpus fixtures.

### Suggested next sub-targets

1. Analyze and migrate comment-bearing non-final grid cells, including their
   separator and width semantics, as a distinct bounded slice.
2. Extend typed comments into nested environments.
3. Move display and environment authored-break layout onto the document IR,
   including modifier and adjacent-comment behavior.

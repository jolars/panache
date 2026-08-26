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

**Comments in nested environments.** Extended typed comment lowering and its
eligibility check recursively through standalone nested-environment rows, with
byte parity against Badness and explicit idempotency coverage.

- The safety gate now mirrors the renderer's row structure: a standalone nested
  environment recurses into its body, while mixed or otherwise unsupported
  comment-bearing cells still take the verbatim compatibility path.
- Mandatory parity covers free, aligned final-cell, aligned first-cell, and
  two-level nested cases; each case is explicitly idempotent.
- The pinned Badness formatter's first-column ordinary-group inconsistency also
  applies to single-cell nested-environment rows: it resets operator context
  after the comment and adds one continuation column. Panache reproduces this
  behavior for parity, but it should be raised and corrected in Badness.
- The committed 107-case report is unchanged because this slice adds focused
  controlled bodies rather than new corpus fixtures.

### Suggested next sub-targets

1. Move display and environment authored-break layout onto the document IR,
   including modifier and adjacent-comment behavior.
2. Expand grid-comment parity to rows combining multiple multiline cells if a
   motivating corpus case appears.
3. Extend nested-environment comment parity to mixed surrounding expressions
   only if that mixed layout is migrated as its own bounded slice.

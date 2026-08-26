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

**Environment nested inside structured delimiters.** Composed the existing
environment-row document inside the typed `MATH_DELIMITED` lowering for the
shared `environments/nested/delimited_matrix.tex` case.

- Mandatory byte parity and second-pass idempotency now cover the case in
  inline, display, and environment contexts.
- The matrix body hangs from the `\left` delimiter's opening width while its
  `\begin` remains on the opening line and `\end` returns to the same column.
- Selection remains narrow: the delimiter must be closed, its body must contain
  exactly one well-formed environment plus layout trivia, and mixed or malformed
  shapes remain on the compatibility paths.
- Display and environment contexts retain an authored final math newline;
  inline context flattens it, matching the controlled Badness wrappers.
- The regenerated 107-case report improves from 95 to 98 parity runs and from
  211 to 208 divergences; all three changes promote the delimited matrix case.

### Suggested next sub-targets

1. Add typed relation-chain continuation alignment, then retire the display
   authored-break compatibility path for supported chains.
2. Expand structured-delimiter environment lowering to mixed bodies only when a
   representative oracle case can pin the spacing and break policy.
3. Expand grid-comment parity to rows combining multiple multiline cells if a
   motivating corpus case appears.

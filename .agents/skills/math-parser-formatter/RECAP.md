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

**Authored-break alignment cells.** Routed supported `\\` rows containing
top-level `&` separators through the typed document IR, with byte parity and
idempotency in inline, display, and environment contexts.

- `&` remains layout rather than a semantic atom. Each cell is lowered against
  its slice of the source-ordered semantic stream, so contextual operator roles
  survive the separator while authored and operator-required separator spacing
  matches Badness.
- Environment grids lower supported ordinary cells to documents only when the
  body contains an authored break. Existing column widths and trailing-marker
  alignment therefore operate on typed flat widths without changing unrelated
  compatibility cases.
- Mandatory parity now covers ordinary and unequal-width grids plus a nested
  multiline comment in the final cell. All cases run through every controlled
  context and an explicit second formatting pass.
- Display nested-comment safety now accepts an authored break when complete
  typed lowering proves the body is supported. Regenerating the 107-case report
  leaves both its contents and classification counts unchanged.

### Suggested next sub-targets

1. Replace the remaining embedded-environment compatibility path for supported
   standalone and mixed rows.
2. Add typed relation-chain continuation alignment, then retire the display
   authored-break compatibility path for supported chains.
3. Expand grid-comment parity to rows combining multiple multiline cells if a
   motivating corpus case appears.

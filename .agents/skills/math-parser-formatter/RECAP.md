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

**Authored breaks in display and free environment bodies.** Routed supported
non-chain `\\` rows through the typed document IR in display math and non-grid
environment bodies, with byte parity against Badness and explicit idempotency
coverage in all three controlled contexts. Display relation chains retain the
legacy path until their Panache-specific continuation alignment exists in IR.

- Display bodies retain source-ordered semantic context across `\\`; environment
  rows reset it, matching Badness's treatment of a leading unary operator.
- Environment lowering aligns trailing row markers from typed flat widths,
  retains closed `*`/bracket modifiers, and keeps an adjacent `%` comment on the
  marker line. Nested groups and delimiters retain inline/display break style.
- Mandatory parity now covers bare and modified markers, unequal row widths,
  operators, nested groups, proven command arguments, paired delimiters, and
  adjacent comments; every controlled context is checked for idempotency.
- Regenerating the 107-case report leaves its classification counts unchanged.
  It records the newly migrated environment comment layout and refreshes one
  stale malformed-environment divergence left by the preceding nested-comment
  slice.

### Suggested next sub-targets

1. Move authored-break rows containing `&` alignment cells onto the document
   IR, preserving trailing-marker alignment and comment behavior.
2. Replace the remaining embedded-environment compatibility path for supported
   standalone and mixed rows.
3. Expand grid-comment parity to rows combining multiple multiline cells if a
   motivating corpus case appears.

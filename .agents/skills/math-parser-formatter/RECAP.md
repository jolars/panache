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
- **Composite relations expose a known Badness formatter defect.** It
  splits `:=` into punctuation plus relation (`x:=y` → `x: = y`) and splits a
  CST-severed head from its scripted tail (`:=_i` → `: =_i`, `<=_i` →
  `< =_i`). Panache preserves authored composite relations through the
  compatibility path; exclude these cases from mandatory byte parity until the
  pinned oracle is corrected.

--------------------------------------------------------------------------------

## Latest session

**Paired-delimiter body comments.** Extended typed comment lowering into
`\left`/`\right` bodies, completing the inline comment slice for every
bracketing construct already on the typed path.

- `lower_delimited` now routes its body through the shared `lower_body`
  dispatch, and `delimited_is_supported` accepts `MATH_COMMENT` body elements.
  A comment outside the body, such as one between `\left` and its delimiter,
  stays on the conservative fallback.
- The existing opening-width alignment and body padding already reproduce
  Badness's layout: continuation lines land at the opening width plus one
  column, and a trailing comment pushes `\right` one column further out.
- Added focused unit coverage, byte-exact parity for leading, trailing, mid,
  empty, nested, scripted, and argument-embedded pairs, and extended the
  idempotency guard.
- Added two shared-corpus cases and regenerated the 107-case baseline; inline
  parity increased from 74 to 76 cases.

### Suggested next sub-targets

1. Lower authored `\\` breaks, the last inline shape still on the fallback.
2. Extend the comment and break slices beyond inline math into the display and
   environment contexts, where `can_lower_nested_comments` still declines.

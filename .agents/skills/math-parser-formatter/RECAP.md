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
- **Composite relations expose a known Badness formatter defect.** It
  splits `:=` into punctuation plus relation (`x:=y` → `x: = y`) and splits a
  CST-severed head from its scripted tail (`:=_i` → `: =_i`, `<=_i` →
  `< =_i`). Panache preserves authored composite relations through the
  compatibility path; exclude these cases from mandatory byte parity until the
  pinned oracle is corrected.

--------------------------------------------------------------------------------

## Latest session

**Authored inline row breaks.** Lowered typed `MATH_LINE_BREAK` nodes through
the document IR, completing the inline comment-and-break shapes already
represented in the shared corpus.

- Plain breaks, tight `*` and bracket modifiers, nested group/argument/pair
  breaks, and comments after breaks now take the typed path. Unclosed bracket
  modifiers remain on the conservative fallback.
- Break layout preserves whether the author placed whitespace before `\\`,
  reproducing Badness's distinction between `a\\b` and `a \\ b`.
- Row lowering shares one semantic atom stream across every break. This
  preserves Badness's contextual operator role after `\\`, while each row is
  still formatted and indented independently.
- Added focused lowering, malformed-input, idempotency, and byte-exact oracle
  coverage. Regenerated the 107-case baseline; inline parity increased from 76
  to 78 cases.

### Suggested next sub-targets

1. Extend typed comments into display and environment contexts, where
   `can_lower_nested_comments` still declines.
2. Move display and environment authored-break layout onto the document IR,
   including modifier and adjacent-comment behavior.

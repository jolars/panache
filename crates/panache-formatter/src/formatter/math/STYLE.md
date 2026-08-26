# Math content formatting --- canonical style rules

The experimental math formatter (`Config::experimental_format_math`, default
off) reformats the **content** of math spans. It does structurally-safe layout
(whitespace collapse, `&`-column alignment, environment indentation, `\\`
normalization) plus **precedence-aware operator spacing** (see Rule 6) and
**semantic line-breaking of over-width display rows** (see Rule 7). It stays
conservative beyond that: never macro rewriting, `\frac`/`\dfrac`
canonicalization, or auto-`&` insertion. There is no pandoc oracle for math
*formatting* (pandoc passes math through); the reference for alignment behavior
is `latexindent`, and operator spacing is meaning-validated against a dev-only
MathML oracle (`tests/math_cross_validation.rs`).

The formatter **re-parses the clean content string** (delimiters excluded) into
a `MATH_CONTENT` CST and re-emits it. Re-parsing the already-prefix-stripped
string (from `math_content_text`) avoids the host container-prefix problem that
a direct subtree walk would hit.

## Bail-to-verbatim guards

Returned unchanged, never reflowed:

1. The gate is off (`enabled == false`).
2. The content has an unescaped lone `$` (matches the existing
   `has_unescaped_single_dollar_in_content` preservation guard).
3. The structural parse reports any diagnostic (unclosed/mismatched braces or
   environments). Malformed math has an untrustworthy row/column structure.

## Rules

1. **Inline whitespace collapse.** In inline context (`$...$`, `\(...\)`), the
   content is rendered on one line with every whitespace run collapsed to a
   single space and the ends trimmed. Spaces are never *removed* (a
   command-terminating space survives: `\alpha   x` → `\alpha x`). A leading
   top-level `%` comment remains on its own line, and a same-line trailing `%`
   comment retains the newline that terminates it. Safe mid-expression comments
   retain the preceding atom's semantic context across their hard newline, so a
   following sign remains binary or unary as authored. The same rule applies in
   signature-proven math arguments, ordinary groups, braced script arguments,
   and `\left`/`\right` bodies. Every bracket level enclosing a comment-broken
   body adds one column of hanging indentation, which also positions the closing
   delimiter after a trailing comment; a `\left`/`\right` pair contributes its
   opening width plus one column instead, and its padded body puts the closing
   `\right` one further column out.

2. **Display free rows.** Non-environment display content (`$$...$$`) is laid
   out one row per line. Rows split on a top-level `\\` (hard break, kept) or a
   top-level newline (soft, dropped); blank lines collapse. Each row's
   whitespace is collapsed and trimmed, then indented by `math_indent` (default
   0). Free content is **never** column-aligned --- a bare `&` outside an
   environment is not a separator.

3. **Environment layout.** A standalone `\begin{name}` and `\end{name}` each go
   on their own line at the environment's indent. The body is indented **one
   level (2 spaces) deeper**, accumulating for nested environments.
   `math_indent` does **not** apply inside standalone environments (hardcoded
   2-space, opinionated --- may become configurable later under the experimental
   clause).

   A free comment-bearing body without `&`, an authored `\\`, or a nested
   environment follows the typed comment rules from Rule 1 at the environment's
   one-level indent. Its operator context survives comment newlines, and nested
   brackets contribute their normal hanging indentation.

   An environment embedded as an operand inside a balanced ordinary delimiter
   pair (`(...)` or `[...]`) remains in the surrounding expression. If it makes
   the delimiter body multiline, the body breaks after the opening delimiter, at
   top-level commas/semicolons, and before the closing delimiter. The
   environment starts after its preceding expression; its body hangs one level
   beyond the `\begin` column, and `\end` returns to that column. Following
   punctuation stays attached to `\end`, never detached onto its own line. This
   is formatter-side delimiter interpretation; ordinary delimiters remain flat
   tokens in the lossless CST because they do not create TeX scope.

   In display math, a single top-level environment with surrounding free content
   uses the same hanging layout. Its `\begin` stays after the preceding
   expression, and its body and `\end` align relative to the environment's
   starting column. Inline math follows Badness's distinct layout: the
   environment's continuation lines return to the math body's base column.

   A lone environment inside a closed `\left`/`\right` body composes the same
   environment layout with the structured delimiter's hanging column. The
   `\begin` remains beside the opening delimiter; body rows indent one level
   beyond it, and `\end` returns beneath it before the closing `\right`.

   Mixed shapes this layout does not yet model safely --- multiple environments
   in one segment, unbalanced ordinary delimiters, or a segment containing a
   comment or explicit `\\` --- stay verbatim. The surrounding display-math
   formatter owns delimiter-adjacent line breaks, so the verbatim fallback
   removes only leading and trailing newline characters. It preserves
   indentation and all internal whitespace. The math-local Wadler-style document
   model (`ir.rs`) preserves multiline fragments compositionally; it never uses
   string sentinels.

4. **`\\` normalization.** Display and environment row layout emits a trailing
   hard break as `\\` with one preceding space. Typed inline lowering follows
   Badness and preserves whether the author placed whitespace before the break.
   A trailing `\\` on the final row is **preserved if present, never
   synthesized**.

5. **`&`-column alignment.** Within an environment body, rows split into cells
   on **top-level** `&` (a `&` inside a group `{...}` or a nested environment is
   opaque content, not a separator). Each cell is rendered inline and trimmed.
   The per-column width is the max trimmed width over **every** cell of
   multi-cell rows (the last column included, so trailing `\\` align too). Cells
   join with the canonical `&` separator and are right-padded to their column
   width. The **last** cell is padded only when the row carries a trailing `\\`
   (so the `\\` line up); a final or soft-break row's last cell is left unpadded
   to avoid trailing whitespace. Single-cell rows never participate. Widths are
   **source character counts**, so alignment is cosmetic source-tidiness, not
   rendered-glyph alignment (`\alpha` counts as 6).

   A grid cell containing one comment-bearing group, signature-proven argument,
   braced script, or `\left`/`\right` body uses the typed comment layout from
   Rule 1. A final cell's continuation indent composes the environment indent,
   the aligned cell's starting column, and the enclosing construct's hanging
   indent.

   For Badness parity, a multiline non-final cell switches the entire
   environment to tight separators: `&` has no surrounding grid space, columns
   are not padded, and a trailing `\\` is not preceded by a synthesized space.
   Cell contents still receive ordinary operator formatting. The pinned oracle
   has one construct-sensitive inconsistency: after a comment in an ordinary
   first-column group---including a single-cell row---the next operator receives
   line-local context, and the continuation gains one column, while commands,
   scripts, paired delimiters, and later columns preserve semantic context
   across the comment. Panache reproduces this behavior until the oracle is
   corrected.

   Ragged rows are fine: a column's width is the max over only the rows that
   have a non-last cell there; a short row contributes to and is padded for only
   the columns it has.

   A row whose sole content is a single nested environment (no `&`, no `\\`) is
   block-laid-out at the body indent rather than inlined. Comment-bearing cells
   inside such an environment recurse through the same typed layout and safety
   checks at every nesting depth.

6. **Operator spacing.** The char operators `+ - * = < >` (the parser's neutral
   `MATH_OPERATOR` tokens) are spaced by *interpretation*, not by CST shape ---
   the class/precedence logic lives in `operators.rs`, the math analog of YAML
   scalar cooking, keyed on operator text + command name. A run of adjacent
   operator chars splits into atoms: adjacent **relation** chars (`= < >`) merge
   into one composite relation (`<=`, `==` stay one unit), while each **sign**
   char (`+ - *`) is its own atom --- so `=-` is a relation `=` then a sign `-`,
   giving `x = -y`, and `a--b` is binary-then-unary `a - -b`. A sign atom in a
   *unary* position --- list start, or after another Bin/Rel/Open/Punct/large-op ---
   is coerced to ordinary (TeX's unary-minus rule). Binary/relation atoms get
   one space on each side; unary atoms are **tight**, *stripping* adjacent
   author spaces (`- x` → `-x`, `f( - x)` → `f(-x)`), except a space demanded by
   a neighboring spaced operator still wins (`x = - y` → `x = -y`). The
   preceding atom's class comes from the last significant token: a `MATH_TEXT`
   run by its last char (`(`/`[` → open, `)`/`]` → close, `,`/`;` → punct), a
   command via the `operators.rs` table (`\leq` → Rel, `\cdot` → Bin, `\sum` →
   large op, else ordinary), `{`/`^`/`_`/`&` as unary-inducing, `\\` resetting
   to start. Author whitespace between two ordinary atoms is preserved, so a
   command-terminating space (`\alpha x`) and a `\text{ a }` interior survive.
   Command operators (`\leq`, `\cdot`) are re-spaced the same way: a binary or
   relation command gets one space on each side (`a\cdot b` → `a \cdot b`,
   `a\leq b` → `a \leq b`), classed via the `operators.rs` table. They are
   **never** made tight, though --- a command's terminating space is mandatory
   (stripping `\leq b` to `\leqb` would name a different control word), so a
   unary-position command op, a large operator (`\sum`), and ordinary commands
   all keep their author space verbatim. The structural `MATH_DELIMITED` node,
   rather than the command table, identifies `\left`/`\right` framing.

   **The definition `:=`.** A `:` is an ordinary atom whose spacing is the
   author's (`x:y` and `f: A` are left alone), *except* when an `=` follows it
   immediately: then the two are one composite relation, spaced as a unit
   (`x:=y` → `x := y`, never `x : = y`). The parser gives the `:` its own
   `MATH_TEXT` token precisely so the pair has an element boundary --- the
   line-breaker anchors and breaks on the `:`, so a chain can never be split
   between a colon and its `=`. The selector is
   `operators::is_definition_colon`; only the leading form fuses, so `=:` stays
   an `=` relation followed by an ordinary `:`.

7. **Display line-breaking.** A free display row (`$$…$$`, non-environment)
   wider than `line-width` is broken at its **top-level** operators in a
   two-level hierarchy keyed on `operators::break_priority` (**relations** >
   **binary** > everything else). The first relation stays on the opening line;
   every later relation starts a continuation aligned under the **first
   relation** (`linebreak::relation_column`) --- the classic stacked-`=` layout
   for an equality/comparison chain. Then any relation segment that is still
   over-width splits before each top-level **binary** operator, with each
   `+ term` sitting **flush** under that segment's own right-hand side. The
   relation/RHS offset alone supplies the visual nesting; binary continuations
   never pick up an extra step. The width budget charges the flat `math-indent`
   against `line-width`, so a broken line plus its leading indent still stays
   within `line-width`. It is source-cosmetic only --- math ignores whitespace,
   so the rendered equation is unchanged:

   ```
   A = aaaaaaaaaa
       + bbbbbbbbbb
     = cccccccccc
       + dddddddddd
   ```

   (At a width where each relation segment fits, no binary breaking happens and
   only the relation split shows: `A = aaaa + bbbb` / `= cccc + dddd`.)

   **Assignment exception.** When the leading relation is an *assignment* arrow
   (`\gets`, `\leftarrow`, `\mapsto`, `\coloneqq`, or the composite `:=`), the
   arrow defines its LHS rather than equating it, so it is **not** part of an
   equality chain it introduces. An equality or comparison continuation anchors
   under the assignment's *right-hand side* (`linebreak::rhs_start_column`)
   instead of under the arrow, so a wide arrow (`\gets` is 5 cols) does not drag
   it left. A repeated assignment, however, aligns its operator under the first
   assignment operator. The continuation's relation kind selects the anchor via
   `linebreak::continuation_anchor_for` and `relation_is_assignment`. `\to` and
   `\rightarrow` are intentionally *not* assignments (they are usually limits or
   mappings).

   ```
   \beta_0 \gets \beta_0 + \frac{4}{n} …
                 = \beta_0 - \frac{1}{L_0} …
                 = 1/4

   A :=_i bbbbbbbbbb
     :=_j cccccccccc
   ```

   This is **fully deterministic**: the layout is a pure function of the
   content, `line-width`, and `math-indent` --- the author's own line breaks and
   indentation are never preserved, only recomputed.

   - **Top-level only.** An operator at delimiter depth > 0 --- inside the flat
     token runs `(…)` or `[…]` --- is never a break candidate. Structural
     `MATH_DELIMITED` (`\left…\right`), `MATH_GROUP`, and `MATH_ENVIRONMENT`
     nodes are opaque operands that the break scan never descends into, so their
     interior operators are likewise excluded.
   - **Spaced operators only.** A candidate is a *spaced* operator
     (`operators::is_spaced` after `coerce`); a unary `+`/`-` is `Ord` and never
     a break site. A relation continuation re-spaces correctly in isolation
     (relations never coerce); a binary continuation is rendered with a seeded
     closing-operand class (`render_inline_seeded`) so its leading `+`/`-` stays
     binary instead of coercing to a sign.
   - **A logical row is one equation.** Free rows split into logical rows only
     on a top-level hard `\\`; a soft newline is insignificant whitespace and
     does **not** start a new row, so a multi-line authored equation (and the
     breaker's own continuations) collapse to one unit and are re-laid-out.
     (Contrast environment-body rows, which keep soft-newline boundaries.) The
     exception: a soft newline terminating a `%` comment stays a boundary, or
     the next line is absorbed into the comment.
   - **`\\` relation chains align like an implicit `aligned`.** A genuine hard
     `\\` *does* split logical rows. When ≥ 2 such `\\`-joined rows form a
     relation chain --- the head ends in `\\` and every following row
     `begins_with_top_level_relation` (a continuation like `= b`) --- each
     continuation hangs at the corresponding anchor in the head row: an equality
     or comparison under an assignment's RHS, but a repeated assignment under
     its operator. This is exactly the within-row policy, so a `\\`-broken chain
     in bare `$$` reads like an `aligned` even without one
     (`relation_chain_alignment`). This fires regardless of width (the `\\` are
     forced breaks). A group containing a top-level `&` is left to the existing
     free-row path (a bare `&` is not a column separator), and `\\` rows that
     are not a relation chain stay flush at the bare `math-indent`.
   - **Scope:** every over-width free row with a top-level relation **or**
     binary operator is broken. A **relation chain** (≥ 2 relations) splits at
     its relations, then nests binary terms inside each over-width segment (as
     above). A **single-relation** row splits its over-width binary RHS, each
     `+ term` flush under the right-hand side. A **standalone binary chain** (no
     relation) splits with the first term as the head and each `+ term` flush
     under it. The unifying rule: a binary continuation aligns flush under the
     **first term of its operand sequence** (for a relation segment that is its
     RHS; for a bare chain it is the chain itself). The relation/RHS offset is
     the only nesting; `math-indent` shifts the whole block but never the
     internal alignment, so the equation's shape is identical at any indent. A
     row with **no** top-level relation or binary operator (e.g. a single wide
     `\frac{…}{…}`) is left on one over-width line --- like an unbreakable long
     word in prose reflow. Inline and environment-body math are not line-broken.

8. **Tight scripts and group interiors.** Whitespace that TeX ignores is
   removed:
   - **Sub/superscript markers** (`_`, `^`) bind tightly, so author whitespace
     on either side is stripped: `H _{ 00}` → `H_{00}`, `x ^ 2` → `x^2`,
     `{a} _ b` → `{a}_b`. The marker still presents an opening class, so a
     directly following `+`/`-` coerces to unary (`x^{-1}` keeps its minus
     tight).
   - **Signature-proven math arguments** recurse through the normal math spacing
     path, so their leading and trailing interior whitespace is trimmed
     (`\frac{ 1 }{ 2 }` → `\frac{1}{2}`). Text-domain, unknown, unmatched, and
     redefined-command arguments are emitted as one opaque byte string. This
     preserves `\text{ a }`, custom text macros, and any argument whose
     whitespace semantics Panache cannot prove. Configured signatures replace
     built-ins; raw-TeX definitions shadow both.

## Idempotency

`format(format(x)) == format(x)` for every well-formed input. The alignment
engine guarantees it by construction:

- **Trim before measure.** Each cell is trimmed before its width is measured, so
  the trailing padding emitted on pass 1 is stripped before pass 2 measures ---
  pass 2 computes identical column widths.
- **Padding is trailing only.** Never inserted before a separator in a way that
  would re-grow on the next pass.
- **Indentation is derived from tree depth, never measured from source**, so a
  line's leading indent is discarded on re-parse (it becomes a leading
  `MATH_SPACE` in the first cell, trimmed away) and regenerated identically.
- The canonical `&` separator re-tokenizes to
  `MATH_SPACE MATH_ALIGN   MATH_SPACE`; pass 2 splits on the same `&` and trims
  the same surrounding spaces, so cell boundaries are stable.
- **Operator spacing is a fixed point.** A spaced operator re-tokenizes to the
  same `MATH_OPERATOR`(+`MATH_SPACE`) shape, and its class depends only on the
  token stream --- which round-trips --- so pass 2 makes the identical decision.
  Inserting at most one space per gap (then `collapse_spaces` + cell trim) and
  stripping spaces only beside *tight* runs both converge in one pass.
- **Tight scripts and trimmed group interiors are fixed points.** Once a script
  is tight (`H_{00}`) or a math-mode group interior is trimmed (`{00}`), the
  re-parse has no adjacent whitespace to strip, so pass 2 emits the same bytes.
  The text-mode exemption keys on the command name, which round-trips, so the
  same groups are spared each pass.
- **Line-breaking is a fixed point.** The breaker emits continuations on soft
  newlines with leading alignment spaces. On pass 2 those soft newlines and
  spaces are insignificant whitespace that re-joins into the single logical row
  (Rule 7), and the continuation indent is recomputed from that row's structure
  (never measured from the source), so the identical break points and alignment
  column are reproduced.
- **Embedded environments are a fixed point.** Their hanging column is the
  canonical flat width of the formatted segment prefix, never the source
  indentation. The delimiter group and environment hard lines therefore choose
  the same broken layout on every pass, while punctuation remains in the same
  document concatenation as the environment close.

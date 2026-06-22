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
   command-terminating space survives: `\alpha   x` → `\alpha x`).

2. **Display free rows.** Non-environment display content (`$$...$$`) is laid
   out one row per line. Rows split on a top-level `\\` (hard break, kept) or a
   top-level newline (soft, dropped); blank lines collapse. Each row's
   whitespace is collapsed and trimmed, then indented by `math_indent` (default
   0). Free content is **never** column-aligned --- a bare `&` outside an
   environment is not a separator.

3. **Environment layout.** `\begin{name}` and `\end{name}` each go on their own
   line at the environment's indent. The body is indented **one level (2 spaces)
   deeper**, accumulating for nested environments. `math_indent` does **not**
   apply inside environments (hardcoded 2-space, opinionated --- may become
   configurable later under the experimental clause).

4. **`\\` normalization.** A row's trailing hard break is emitted as `\\` (one
   space before). A trailing `\\` on the final row is **preserved if present,
   never synthesized**.

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

   Ragged rows are fine: a column's width is the max over only the rows that
   have a non-last cell there; a short row contributes to and is padded for only
   the columns it has.

   A row whose sole content is a single nested environment (no `&`, no `\\`) is
   block-laid-out at the body indent rather than inlined.

6. **Operator spacing.** The char operators `+ - * = < >` (the parser's neutral
   `MATH_OPERATOR` tokens) are spaced by *interpretation*, not by CST shape ---
   the class/precedence logic lives in `operators.rs`, the math analog of YAML
   scalar cooking, keyed on operator text + command name. A run of adjacent
   operator chars splits into atoms: adjacent **relation** chars (`= < >`) merge
   into one composite relation (`<=`, `==` stay one unit), while each **sign**
   char (`+ - *`) is its own atom --- so `=-` is a relation `=` then a sign `-`,
   giving `x = -y`, and `a--b` is binary-then-unary `a - -b`. A sign atom in a
   *unary* position --- list start, or after another Bin/Rel/Open/Punct/large-op
   --- is coerced to ordinary (TeX's unary-minus rule). Binary/relation atoms
   get one space on each side; unary atoms are **tight**, *stripping* adjacent
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
   unary-position command op, a large operator (`\sum`), a delimiter command
   (`\left`/`\right`), and ordinary commands all keep their author space
   verbatim. A break-priority column for line-breaking is a later phase.

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
   (`\gets`, `\leftarrow`, `\mapsto`, `\coloneqq`, or `:=`), the arrow defines
   its LHS rather than equating it, so it is **not** part of the equality chain
   it introduces. The equality continuations then anchor under the assignment's
   *right-hand side* (`linebreak::rhs_start_column`) instead of under the arrow,
   so a wide arrow (`\gets` is 5 cols) does not drag them left. The selector is
   `linebreak::continuation_anchor` / `first_relation_is_assignment`. `\to` and
   `\rightarrow` are intentionally *not* assignments (they are usually limits or
   mappings).

   ```
   \beta_0 \gets \beta_0 + \frac{4}{n} …
                 = \beta_0 - \frac{1}{L_0} …
                 = 1/4
   ```

   This is **fully deterministic**: the layout is a pure function of the
   content, `line-width`, and `math-indent` --- the author's own line breaks and
   indentation are never preserved, only recomputed.

   - **Top-level only.** An operator at delimiter depth > 0 --- inside `(…)`,
     `[…]`, or `\left…\right` (tracked by an open/close counter, since those are
     *flat token runs*, not nesting nodes), or anywhere inside a `{…}` brace
     group (a node we never descend, so `\frac{…}{…}` arguments are opaque) ---
     is never a break candidate.
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
     `begins_with_top_level_relation` (a continuation like `= b`) --- the
     continuations hang at the head's `continuation_anchor` (under the first
     relation, or the assignment's RHS), exactly as the within-row relation
     breaks do, so a `\\`-broken chain in bare `$$` reads like an `aligned` even
     without one (`relation_chain_alignment`). This fires regardless of width
     (the `\\` are forced breaks). A group containing a top-level `&` is left to
     the existing free-row path (a bare `&` is not a column separator), and `\\`
     rows that are not a relation chain stay flush at the bare `math-indent`.
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
   - **Math-mode brace groups** have their *leading and trailing* interior
     whitespace trimmed (`{ 00 }` → `{00}`, `{-1 }` → `{-1}`), since math mode
     ignores it. The space *before* `{` and *after* `}` (between atoms) is left
     alone (`{x} y` stays `{x} y`), and inter-atom spaces inside the group keep
     the Rule 1/6 collapse, not removal. **Text-mode groups are exempt:** the
     argument of a text-switching command (`\text`, `\mbox`, the `\text*` family
     --- see `operators::is_text_mode_command`) keeps its interior spaces
     verbatim (`\text{ a }` survives), and the exemption nests, so a group
     inside a text argument (`\text{a {b} c}`) is also preserved. Whether a
     group is text mode is tracked with a brace-mode stack in
     `render::space_operators`. Math-mode font commands (`\mathrm`, `\mathbf`)
     are **not** text mode --- spaces are already insignificant inside them ---
     so their interiors are trimmed like any other math group.

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

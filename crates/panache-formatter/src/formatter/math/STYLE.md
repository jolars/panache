# Math content formatting --- canonical style rules

The math formatter (`Config::math`, default `reflow`) reformats the **content**
of math spans. Every mode except `verbatim` applies structurally safe layout
(whitespace collapse, `&`-column alignment, environment indentation, `\\`
normalization) plus **precedence-aware operator spacing** (see Rule 6). The mode
controls only the handling of soft line boundaries in free display math:
`preserve` retains them, `single-line` removes them without width wrapping, and
`reflow` removes them and **semantically breaks over-width display rows** (see
Rule 7). The formatter stays conservative beyond that: never macro rewriting,
`\frac`/`\dfrac` canonicalization, or auto-`&` insertion. There is no pandoc
oracle for math *formatting* (pandoc passes math through). The pinned
`badness-formatter` crate is the byte-layout oracle, while independent MathML
and TeX/PDF checks validate meaning preservation. Both oracles are
development-only dependencies.

Pandoc represents standalone TeX math environments as raw TeX rather than
Markdown display-math nodes. Panache retains that parser shape, but treats the
typed raw environment as a math-formatting host in every non-`verbatim` mode.
The same environment-body rules therefore apply both to standalone environments
and to TeX environments nested inside `$$...$$` or `\[...\]`.

The formatter **re-parses the clean content string** (delimiters excluded) into
a `MATH_CONTENT` CST and re-emits it. Re-parsing the already-prefix-stripped
string (from `math_content_text`) avoids the host container-prefix problem that
a direct subtree walk would hit.

## Bail-to-verbatim guards

Returned without a math-content rewrite, never reflowed:

1. The mode is `verbatim`.
2. The content has an unescaped lone `$` (matches the existing
   `has_unescaped_single_dollar_in_content` preservation guard).
3. The structural parse reports any diagnostic (unclosed/mismatched braces or
   environments). Malformed math has an untrustworthy row/column structure.
4. A dangling script or structurally malformed environment makes attachment or
   row boundaries untrustworthy.
5. A command argument's math domain is unproven and the surrounding structure
   cannot preserve that argument as an opaque fragment.
6. Typed lowering does not yet cover the exact shape. The current named cases
   are a comment between a command and its argument, an otherwise
   whitespace-only proven argument containing a comment, multiple environments
   in one punctuation segment, unbalanced ordinary delimiters, and mixed free
   segments containing a comment or authored `\\`.

Text-domain, unknown, unmatched, over-attached, and document-redefined command
arguments are opaque preservation islands when the surrounding expression can
still be lowered safely; they do not automatically preserve the whole span. The
host's preserved-body path removes ASCII spaces and tabs immediately before a
line ending, except for the semantic ASCII space in the TeX control symbol `\ `.
This line hygiene does not otherwise normalize or reflow the preserved content.

## Intentional Badness differences

- Markdown inline hosts join layout lines unless a TeX comment pins the break.
- A soft newline before a signature-proven argument collapses as insignificant
  whitespace.
- Panache applies both sides of the TeXbook Bin-to-Ord rule: it tightens
  authored whitespace after unary signs and keeps postfix signs tight before
  relations, closing atoms, and punctuation. The pinned formatter omits the
  punctuation and right-context cases.
- Panache keeps signs attached to unbraced TeX dimensions; the pinned formatter
  incorrectly spells them as binary operators.
- Panache preserves scripted composite relations such as `<=_i`, `>=_i`, and
  `==_i`; the pinned formatter incorrectly separates the relation head from its
  CST-separated script.
- Panache retains an authored environment-row boundary after a standalone TeX
  control space. The pinned formatter moves the control space onto the next row;
  Panache keeps the semantic space at the physical line end.
- Panache wraps independent equations at top-level punctuation. The pinned
  formatter treats their relations as one chain and can strand the second
  equation's left-hand side at the end of the preceding line.

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
   out according to `Config::math`. A top-level `\\` is a structural hard break
   and remains a row boundary in every non-verbatim mode. In `preserve` mode,
   each authored top-level newline is also retained as a soft row boundary. In
   `single-line` and `reflow` modes, top-level soft newlines are insignificant
   whitespace and are removed before layout; blank lines collapse. Each
   resulting row's whitespace is collapsed and trimmed, then indented by
   `math_indent` (default 2), in addition to any list or blockquote indentation.
   Free content is **never** column-aligned---a bare `&` outside an environment
   is not a separator.

3. **Environment layout.** A standalone `\begin{name}` and `\end{name}` each go
   on their own line at the environment's indent. The body is indented **one
   level (2 spaces) deeper**, accumulating for nested environments. An
   environment inside a Markdown display-math host inherits `math_indent` around
   its markers, then adds the fixed two-space body indent. A raw standalone TeX
   environment has no Markdown host indent; its body still uses the same fixed
   two-space step.

   Signature-declared environment arguments remain attached to the opening
   marker. In particular, `array`'s optional position and required column
   specification form one header (`\begin{array}[t]{cc}`); insignificant
   whitespace before the required specification is removed rather than turning
   the specification into the first body row.

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
   starting column. An ordinary operand, closed brace-group operand, or
   structured `\left…\right` operand immediately before the environment stays
   tight to `\begin`; scripts remain attached to each operand, and the body
   hangs from the resulting source column. When a nested `%` comment pins the
   display breaks and the preceding expression ends in a binary operator, the
   expression breaks before that operator; the environment stays on the
   operator's continuation line and hangs from its resulting `\begin` column. A
   preceding relation instead stays in the flat head with the environment when
   it fits; the body hangs from that later `\begin` column. A unary `+` or `-`
   after either break head remains tight to the environment, and the body hangs
   from the resulting `\begin` column. In `preserve` and `single-line` modes,
   safe trailing content with no top-level binary or relation stays on the
   environment's `\end` line. In `reflow` mode, content after a multiline
   environment starts on a new line at the display indent; a following relation
   begins its own chain rather than inheriting the preceding relation's column.
   Punctuation and closing delimiters remain attached to the environment.
   Scripts on that operand stay attached to the closing marker
   (`\end{matrix}^T`); their width determines where any same-line suffix begins,
   while a following operator takes its display break. Inline math follows
   Badness's distinct layout: the environment's continuation lines return to the
   math body's base column when the body prints flat. If a nested `%` comment
   pins the inline breaks, the environment instead hangs from its actual
   `\begin` source column, including the host `$` opener's column.

   A single environment inside a closed `\left`/`\right` body composes the same
   environment layout with the structured delimiter's hanging column, whether it
   is alone or surrounded by free expression content. The `\begin` remains
   beside the opening delimiter or preceding expression; body rows indent one
   level beyond it, and `\end` returns beneath it. Following expression content
   stays on the `\end` line before the closing `\right`; the structured
   delimiter does not add the ordinary-delimiter breaks described above.

   Multiple environments compose only when top-level punctuation puts each one
   in a separate segment. The punctuation stays attached to the preceding
   `\end`, and the next environment begins immediately afterward; its body and
   closing delimiter align from that actual starting column. Authored rows keep
   the normal environment policy. Comment-bearing cells use the typed grid
   policy in every context. Inline comment-pinned continuations include the host
   `$` opener's column when deriving their hanging indentation.

   Mixed shapes this layout does not yet model safely --- multiple environments
   in one segment, unbalanced ordinary delimiters, or free segments containing a
   comment or explicit `\\` --- stay verbatim. The surrounding display-math
   formatter owns delimiter-adjacent line breaks, so the verbatim fallback
   removes only leading and trailing newline characters. It preserves
   indentation and all internal whitespace except ASCII line-end padding. The
   math-local Wadler-style document model (`ir.rs`) preserves multiline
   fragments compositionally; it never uses string sentinels.

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

   Every math-body line ends without layout-generated ASCII spaces or tabs. The
   printer and preserved-body host paths enforce this invariant; alignment
   padding may appear before a `\\` marker, but never at the physical line end.
   The semantic ASCII space in the TeX control symbol `\ ` is the sole exception
   and survives even when that token ends a physical line.

   A grid cell containing one comment-bearing group, signature-proven argument,
   braced script, or `\left`/`\right` body uses the typed comment layout from
   Rule 1. A final cell's continuation indent composes the environment indent,
   the aligned cell's starting column, and the enclosing construct's hanging
   indent.

   For Badness parity, a multiline non-final cell switches the entire
   environment to tight separators: `&` has no surrounding grid space, columns
   are not padded, and a trailing `\\` is not preceded by a synthesized space.
   Cell contents still receive ordinary operator formatting, but every multiline
   cell's continuation resets to the environment body indent instead of
   inheriting its cell column or preceding atom offset. The pinned oracle has
   one construct-sensitive inconsistency: after a comment in an ordinary
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

6. **Operator spacing.** The char operators `+ - * = < >` in the parser's
   neutral `MATH_WORD` tokens are spaced by *interpretation*, not by CST shape.
   The parser-owned semantic math stream classifies operator text and command
   names, and supplies precedence to the formatter. A run of adjacent operator
   chars splits into atoms: adjacent **relation** chars (`= < >`) merge into one
   composite relation (`<=`, `==` stay one unit), while each **sign** char
   (`+ - *`) is its own atom---so `=-` is a relation `=` then a sign `-`, giving
   `x = -y`, and `a--b` is binary-then-unary `a - -b`. A sign atom in a *unary*
   position --- list start, or after another Bin/Rel/Open/Punct/large-op --- is
   coerced to ordinary (TeX's unary-minus rule). A binary atom is likewise
   ordinary before a Rel/Close/Punct atom, so a postfix left-limit sign stays
   tight (`N(t - )` → `N(t-)`). Binary/relation atoms get one space on each
   side; coerced atoms are **tight**, *stripping* adjacent author spaces (`- x`
   → `-x`, `f( - x)` → `f(-x)`), except a space demanded by a neighboring spaced
   operator still wins (`x = - y` → `x = -y`). The preceding atom's class comes
   from the last significant token: a `MATH_WORD` run by its last char (`(`/`[`
   → open, `)`/`]` → close, `,`/`;` → punct), a command via the parser semantic
   table (`\leq` → Rel, `\cdot` → Bin, `\sum` → large op, else ordinary),
   `{`/`^`/`_`/`&` as unary-inducing, `\\` preserving context across authored
   rows. Author whitespace between two ordinary atoms is preserved, so a
   command-terminating space (`\alpha x`) and a `\text{ a }` interior survive.
   Command operators (`\leq`, `\cdot`) are re-spaced the same way: a binary or
   relation command gets one space on each side (`a\cdot b` → `a \cdot b`,
   `a\leq b` → `a \leq b`), classed by the parser semantic table. They are
   **never** made tight, though --- a command's terminating space is mandatory
   (stripping `\leq b` to `\leqb` would name a different control word), so a
   unary-position command op, a large operator (`\sum`), and ordinary commands
   all keep their author space verbatim. The structural `MATH_DELIMITED` node,
   rather than the command table, identifies `\left`/`\right` framing.

   At the start of an environment row or alignment cell, a sign can continue an
   earlier expression. Preserve the authored gap after that leading sign (`1 \\`
   followed by `+ 2` keeps `+ 2`, and `+2` stays `+2`), collapsing a whitespace
   run to one space. Cell-local unary coercion alone does not prove that
   tightening is appropriate. Nested groups and arguments still apply ordinary
   unary spacing (`+ {- x}` → `+ {-x}`).

   A leading ASCII sign scanned by an unbraced TeX dimension command (`\hskip`,
   `\vskip`, `\kern`, `\mkern`, or `\mskip`) is not a binary operator. Its
   authored gap after the command is collapsed normally, while the sign stays
   tight to the dimension (`\hskip - 1cm` → `\hskip -1cm`). This asymmetric
   attachment also prevents the sign from becoming a display break point.

   **The definition `:=`.** A `:` is an ordinary atom whose spacing is the
   author's (`x:y` and `f: A` are left alone), *except* when an `=` follows it
   immediately: then the two are one composite relation, spaced as a unit
   (`x:=y` → `x := y`, never `x : = y`). The parser gives the `:` its own
   `MATH_WORD` token precisely so the pair has an element boundary---the typed
   display layout anchors and breaks on the `:`, so a chain can never be split
   between a colon and its `=`. `coalesce_scripted_relations` repairs the
   semantic boundary when a script splits the relation in the CST; only the
   leading form fuses, so `=:` stays an `=` relation followed by an ordinary
   `:`.

7. **Display line-breaking.** In `reflow` mode, a free display row (`$$…$$`,
   non-environment) wider than `line-width` is broken after **top-level**
   punctuation, around `\qquad`, or before top-level operators. Operator
   candidates follow parser `MathBreakPriority` (**relations** > **binary** >
   everything else). It chooses a subset of those candidates instead of breaking
   at every operator. The deterministic layout score charges squared overflow
   more heavily than a continuation, prefers punctuation and later relation
   breaks to binary breaks, and prefers a binary break to separating the first
   relation from its left-hand side. Ties favor the smaller maximum line width,
   then the smaller sum of squared line widths. This packs complete operator
   segments together when they fit instead of stranding `\pm`, `\cdot`, or a
   short relation on its own line.

   Top-level punctuation, such as commas and semicolons, and `\qquad` separate
   independent expressions. A selected break after punctuation starts at the
   display indent and leaves the punctuation attached to the preceding
   expression. When a break selects `\qquad`, the command occupies its own
   source line at the display indent, followed by the next expression at that
   same indent. Its width does not affect that expression's wrapping or relation
   alignment. Expressions that fit together keep `\qquad` inline.

   A selected later relation starts a continuation aligned under the **first
   relation in its expression**---the classic stacked-`=` layout for an
   equality/comparison chain. The anchor is the relation's printed column; if
   the first relation moves to a new line, later relations follow that column. A
   selected binary continuation sits **flush** under that relation segment's
   right-hand side. The relation/RHS offset alone supplies the visual nesting;
   binary continuations never pick up an extra step. A top-level conditioning
   bar (`\mid`) identifies neighboring relations as separate predicates rather
   than one relation chain, so relation breaks in that row receive a strong
   penalty. A row that exceeds the budget by only a source column can therefore
   stay intact when every available relation break would create operator
   islands. The width budget otherwise charges the flat `math-indent` against
   `line-width`. This is source-cosmetic only---math ignores whitespace, so the
   rendered equation is unchanged:

   ```
   A = aaaaaaaaaa
       + bbbbbbbbbb
     = cccccccccc
       + dddddddddd
   ```

   (At a width where more terms fit, the selector may keep several binary or
   relation segments on one line.)

   **Assignment exception.** When the leading relation is an *assignment* arrow
   (`\gets`, `\leftarrow`, `\mapsto`, or `\coloneqq`), the arrow defines its LHS
   rather than equating it, so it is **not** part of an equality chain it
   introduces. An equality or comparison continuation anchors under the
   assignment's *right-hand side* instead of under the arrow, so a wide arrow
   (`\gets` is 5 cols) does not drag it left. A repeated assignment, however,
   aligns its operator under the first assignment operator. Typed relation
   layout selects the anchor from `RelationLayout` and `relation_is_assignment`.
   `:=`, `\to`, and `\rightarrow` are intentionally *not* assignments for
   automatic layout; they participate in the relation chain.

   ```
   \beta_0 \gets \beta_0 + \frac{4}{n} …
                 = \beta_0 - \frac{1}{L_0} …
                 = 1/4

   A := bbbbbbbbbb
     = cccccccccc
   ```

   This is **fully deterministic**: the reflow layout is a pure function of the
   content, `line-width`, and `math-indent`---the author's own line breaks and
   indentation are never preserved, only recomputed.

   - **Top-level only.** Punctuation or an operator at delimiter depth > 0 ---
     inside the flat token runs `(…)` or `[…]` --- is never a break candidate.
     Structural `MATH_DELIMITED` (`\left…\right`), `MATH_GROUP`, and
     `MATH_ENVIRONMENT` nodes are opaque operands that the break scan never
     descends into, so their interior operators are likewise excluded.
   - **Spaced operators only.** An operator candidate has parser
     `MathBreakPriority::Binary` or `MathBreakPriority::Relation`; a unary
     `+`/`-` is `Ord` and never a break site. Typed lowering keeps the semantic
     atom stream intact across continuation segments, so a leading binary
     operator stays binary instead of coercing to a sign.
   - **Semantic control spaces survive breaks.** The math IR represents `\ `
     separately from layout padding. An authored or selected line ending after
     that token retains its ASCII space, while any formatter-generated padding
     after it is removed.
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
     are not a relation chain stay flush at the bare `math-indent`. Badness
     treats a definition-led authored chain differently from automatic wrapping:
     rows beginning with `:=`, `:=_i`, or a later ordinary relation remain flush
     at `math-indent`.
   - **Scope:** every over-width free row with a top-level relation **or**
     binary operator enters break selection. A **relation chain** can split at
     later relations and nest selected binary continuations inside those
     segments. A **single-relation** row can split its over-width binary RHS or,
     when that produces a materially better layout, break before the relation. A
     **standalone binary chain** (no relation) packs as many complete terms as
     fit and aligns each selected continuation under the first term. The
     unifying rule: a binary continuation aligns flush under the **first term of
     its operand sequence** (for a relation segment that is its RHS; for a bare
     chain it is the chain itself). The relation/RHS offset is the only nesting;
     `math-indent` shifts the whole block but never the internal alignment. A
     row with **no** top-level relation or binary operator (e.g. a single wide
     `\frac{…}{…}`) is left on one over-width line---like an unbreakable long
     word in prose reflow. `single-line` uses the same normalized flat row but
     never applies these width breaks, and `preserve` retains the authored soft
     row boundaries. Inline and environment-body math are not width-broken.

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
- **Display line policy is a fixed point.** In `preserve`, each retained soft
  row is normalized independently and remains a row on the next pass. In
  `single-line`, soft rows flatten once and no new width breaks are introduced.
  In `reflow`, the breaker emits continuations on soft newlines with leading
  alignment spaces; pass 2 rejoins those insignificant boundaries into one
  logical row and recomputes the same break points and alignment column from
  structure rather than source indentation.
- **Embedded environments are a fixed point.** Their hanging column is the
  canonical flat width of the formatted segment prefix, never the source
  indentation. The delimiter group and environment hard lines therefore choose
  the same broken layout on every pass, while punctuation remains in the same
  document concatenation as the environment close.

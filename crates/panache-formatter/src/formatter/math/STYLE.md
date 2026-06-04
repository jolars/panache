# Math content formatting --- canonical style rules

The experimental math formatter (`Config::experimental_format_math`, default
off) reformats the **content** of math spans structurally. It is deliberately
conservative: **only structurally-safe transforms**, never operator spacing,
macro rewriting, `\frac`/`\dfrac` canonicalization, or auto-`&` insertion. There
is no pandoc oracle for math *formatting* (pandoc passes math through); the
reference for alignment behavior is `latexindent`.

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

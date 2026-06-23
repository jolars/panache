# YAML formatter style spec

Canonical reference for the deterministic style rules that govern the in-tree
YAML formatter under `crates/panache-formatter/src/formatter/yaml/`.

These rules are deterministic (same input → same output) and small enough to fit
in one table. Rules 1--12 + 14 were cross-validated against pretty_yaml 0.6.0
and Prettier 3.6.2 on a 15-case battery of representative frontmatter --- both
agree on the spec; rule 6's bracket placement is the one point where they
differ, and the rule pins pretty_yaml's choice. Rule 13 (trailing newline) and
rule 14 (block-structural spacing) were cross-validated against pretty_yaml
later, during the Phase 1 corpus harness rollout. Rule 6's plain-scalar overflow
analog (block-map values) was cross-validated against pretty_yaml during Phase
1.15b on the `plain_wrap/` corpus.

pretty_yaml is the cross-validation reference because it implements the same
rules. It is not the source of truth: this document is. If the formatter
diverges from pretty_yaml on a case, one of the two is wrong relative to this
spec --- fix it; do not enumerate it. See `.claude/rules/yaml-formatter.md` for
the load-bearing invariants.

## Rules

1. **Indent.** 2 spaces, fully canonicalized regardless of input shape. Each
   content line's indent = `2 * (entry/item nesting depth − 1)` spaces, counting
   the line's containing `YAML_BLOCK_MAP_ENTRY` + `YAML_BLOCK_SEQUENCE_ITEM`
   ancestors. Root-level entries/items get 0 spaces. Tab-indented input is
   rejected by the in-tree parser outright, so the formatter never sees it.
   Multi-line plain / single-quoted / double-quoted scalar continuation lines
   indent at `2 * entry/item nesting depth` (one level deeper than the default
   --- the value column, not the key column), since the continuation belongs to
   the value side of the entry. Block-scalar (`|`/`>`) interior lines are
   currently preserved verbatim --- the indent sits inside one multi-line
   `YAML_SCALAR` token and full canonicalization needs a real block-scalar
   renderer (tracked separately; keeps pretty_yaml parity on already-canonical
   cases, diverges on non-canonical block-scalar indent).

2. **Sequence items** indented +2 from the parent key (`categories:\n  - foo`,
   never `- foo` at parent column).

3. **Quote style preference:** plain → double-quoted → single-quoted only when
   content contains characters that would need backslash-escaping in
   double-quoted form (e.g. `'C:\Users\test'`). Operationally, the formatter
   never adds or removes quoting from a scalar the user wrote plain or
   double-quoted --- those carry semantic intent (`true` the bool vs `"true"`
   the string). Single-quoted scalars are converted to double-quoted UNLESS the
   de-escaped content contains any of `\`, `'`, `"`, or an ASCII control
   character (0x00--0x1F or 0x7F). The control-char guard is conservative:
   pretty_yaml additionally generates `\t` / `\n` / etc. escapes when converting
   single → double, but the in-tree formatter keeps those as single-quoted
   instead --- frontmatter rarely has literal tabs or newlines in quoted
   scalars, and adding escape generation buys little. Single is preserved when
   content has `'` because that's the one case where converting (`'don''t'` →
   `"don't"`) would change the user's explicit choice of escape character
   without simplifying anything; pretty_yaml does the same.

4. **Block scalar style** (literal `|` vs folded `>`): preserved from input.
   They carry different YAML semantics and are not interchangeable.

5. **Flow spacing:** `{ key: value }` with one space inside braces; `[a, b, c]`
   with no space inside brackets, one space after each comma, one space after
   each `:`. Multi-line flow containers and flow containers with embedded
   `YAML_COMMENT` tokens are preserved verbatim (rule 6 owns multi-line wrap;
   in-flow comments are too rare to warrant their own canonicalization path). If
   the parser couldn't structure a flow map's contents into entries (e.g.
   `{key:value}`, no space to disambiguate `:`), the inner bytes are emitted
   verbatim between `{` and `}` --- matches pretty_yaml's "normalize spacing
   around structure, don't re-parse content" behavior.

6. **Flow wrap on line-width overflow:** each item on its own line, trailing
   comma, **opening bracket stays on the key line**
   (`keywords: [\n  first,\n  ...\n]`). This is the one point of disagreement
   between pretty_yaml and Prettier --- we follow pretty_yaml. Wrap fires when
   the canonical single-line form would push the line strictly past
   `line_width`; lines exactly at `line_width` stay single-line. Items indent at
   `parent_content_column + 2`; the closing bracket aligns at
   `parent_content_column`. For a flow in a block-map value, the parent content
   column is `2 * (entry/item depth − 1)`; for a flow in a block sequence item,
   the `-` prefix shifts the content column right by two. Nested flow containers
   inside a wrapped item stay in their canonical single-line form (rule 5)
   unless they themselves overflow on the wrapped line. Multi-line flow input (a
   flow container with `\n` between its brackets) currently passes through
   verbatim because the in-tree parser rejects it; the "multi-line input is
   sticky" behavior pretty_yaml shows lands when the parser learns to accept
   those inputs. **Plain-scalar overflow** in a block-map value follows the
   analogous wrap: when a single-line plain scalar pushes its line past
   `line_width`, greedy word-wrap onto continuation lines indented at
   `entry/item depth * 2` (the value column, matching rule 1's multi-line scalar
   continuation indent so wrap output round-trips). Quoted (`'…'`, `"…"`) and
   block (`|`/`>`) scalars are never reflowed *as plain scalars* (a long
   double-quoted one may instead convert to a folded scalar --- rule 17).
   Already-multi-line scalars are left to rule 1's continuation rule. Scalars
   decorated with tags (`!!str`), anchors (`&name`), aliases (`*name`), or
   trailed by inline comments are skipped (the rare-shape escape valve; matches
   pretty_yaml on the cases that appear in the corpus). Plain scalars inside
   block sequences are also skipped: pretty_yaml's wrap-continuation column
   there (`parent_content_col + 2`) disagrees with rule 1's multi-line
   continuation column (`depth * 2`), so pretty_yaml itself isn't idempotent on
   that shape --- deferred until the parser/formatter can pick one column
   without losing pass-2 stability. Line breaks are taken only at *single*-space
   separators (a fold restores exactly one space there, so the break
   round-trips); a run of >=2 spaces is glued into its chunk and never split, so
   it is preserved verbatim --- the cost is that a chunk wider than `line_width`
   overflows its line, like a long URL. (This corrects an earlier behavior that
   consumed a run sitting at the break point, which silently dropped the run's
   spaces on the fold; preserving them is value-correct and shared with the rule
   15 / rule 17 folded paths.)

7. **Blank lines:** runs of multiple interior blank lines collapse to one max.
   Leading blank lines (before the first content line) are stripped entirely ---
   mirrors rule 13's no-trailing-blanks invariant; preamble whitespace at the
   top of a frontmatter document is never meaningful. Cross-validated against
   pretty_yaml on the `tests/fixtures/yaml_corpus/blank_lines/` cases.

8. **Inline comments:** exactly one space before `#`. Applies only to inline
   comments (comments with non-whitespace content earlier on the same line);
   standalone comments (preceded by `NEWLINE` or at file start) keep their
   original surrounding whitespace. Implemented inside the token walk because
   line-level passes can't reliably distinguish `#` inside quoted scalars from a
   comment indicator.

9. **Comment positions** (above key, inline, between keys): preserved. Comments
   are user-authored content.

10. **Trailing whitespace** on every line: stripped. ASCII space and tab only
    (CRLF round-trips because `\r` is preserved). Applies uniformly, including
    inside `|`/`>` block scalars --- pretty_yaml does the same; this trades the
    "trailing space carries semantics inside `|`" YAML-spec quirk for the "no
    trailing whitespace anywhere" invariant.

11. **Empty scalars:** `key:` stays `key:`, never canonicalized to `key: null`
    or `key: ""`.

12. **Key order:** preserved. Frontmatter is content the user wrote; reordering
    would surprise.

13. **Trailing document newline:** always exactly one `\n` at EOF. Missing
    trailing newline → add one; multiple trailing newlines → collapse to one.
    Cross-validated against pretty_yaml on the standard zero/one/many cases
    (`tests/fixtures/yaml_corpus/document/empty.yaml`,
    `missing_trailing_newline.yaml`, `multiple_trailing_newlines.yaml`).
    Whitespace-only inputs (e.g. `"   "`) are out of scope for rule 13 alone ---
    pretty_yaml canonicalizes those more aggressively, and the divergence
    resolves once the trailing-whitespace rule (#10) lands.

14. **Block-structural spacing.** A whitespace run sitting between a block
    structural indicator (`:` after a block-map key, `-` after a block-sequence
    item marker) and inline content on the same line collapses to exactly one
    space. `key:    value` → `key: value`; `-    item` → `- item`. Trailing-only
    whitespace (`key:   \n  value`) is left to rule 10 to strip; the value's own
    indent line is governed by rule 1. Flow containers normalize `:` / `,`
    spacing through the canonical-emission path (rule 5), so this rule only
    governs block-level structural runs. Added in Phase 1.13 after the real-
    frontmatter harvest surfaced inputs (e.g. `echo:    false`) that rules 1, 5,
    and 8 didn't reach.

15. **Folded (`>`) block-scalar wrapping.** A *folded* block scalar (`>`, `>-`,
    `>+`) reflows its prose per the active wrap mode, exactly like a markdown
    paragraph. This is loss-free: within a folded scalar a single line break
    between two equally-indented non-empty lines folds to one space, so the body
    can be rejoined and re-broken anywhere without changing the parsed value.
    The body is grouped into paragraphs of contiguous base-indent prose lines;
    folding-significant lines act as separators and pass through verbatim ---
    blank lines (which fold to a newline) and more-indented lines (which are
    literal within a folded scalar). Each paragraph is then re-laid-out:
    - **`reflow`** (default): join the paragraph and greedy-fill to
      `line_width`. Short lines *are* joined --- the whole paragraph is
      rewrapped.
    - **`sentence`**: join the paragraph, then emit one sentence per line.
    - **`semantic`**: keep the author's existing line breaks and additionally
      split each on sentence boundaries (semantic linefeeds).
    - **`preserve`**: leave the body's line breaks exactly as written.

    The sentence/semantic modes reuse the same sentence-boundary engine
    (`sentence_wrap`) as markdown prose, so YAML frontmatter prose wraps the
    same way as the document body. Bails (leaves verbatim) on an explicit
    indentation indicator (`>2`), a header trailing comment, or an empty body.
    **Literal (`|`) block scalars never wrap** --- their newlines are
    significant content. A quoted scalar is never *reflowed in place*, but a
    long double-quoted one may be *converted* to a folded scalar first and then
    reflowed (rule 17); single-quoted scalars are left untouched.

    **Deliberate divergence from pretty_yaml**, which preserves every block
    scalar verbatim: rule 15 is the sanctioned point where the in-tree formatter
    reflows a block scalar. (This revises the earlier "only break overlong
    lines, never join" calibration --- joining is what makes reflow actually
    reflow.) Because of that, folded scalars are *exempt from the pretty_yaml
    parity check* in the cross-validation corpus (idempotency is still
    asserted); behavioral coverage lives in
    `crates/panache-formatter/tests/format/yaml_folded_wrap.rs`. Single-line
    *plain* scalars wrap under the same modes (see rule 6's "plain-scalar
    overflow"). Added after the `fig-cap: >` Quarto hashpipe caption stayed
    unwrapped, then revised so a hand-wrapped folded `description:` reflows
    cleanly instead of stranding orphan words.

16. **Verbatim frontmatter fields.** A small allowlist of *top-level*
    frontmatter keys hold code/directives that a downstream **non-YAML**
    consumer reads line-by-line, so their block-scalar values are exempt from
    rule 15's folded reflow (and from rule 6's plain wrap): the author's line
    breaks pass through untouched. The allowlist (`VERBATIM_FRONTMATTER_FIELDS`
    in `document.rs`) is deliberately narrow and content-agnostic --- it matches
    on the key, not the value --- and only at depth 1 (a nested `vignette:`
    under some unrelated map is not exempt).

    Current members:
    - `vignette`: R/knitr vignette magic (`%\VignetteIndexEntry{…}`,
      `%\VignetteEngine{…}`, `%\VignetteEncoding{…}`). R's `tools::vignetteInfo`
      greps the *raw* frontmatter lines, so folding two directives onto one line
      hides the engine and trips `R CMD check` (issue #366). Folding is
      loss-free as YAML, so there is no formatting upside to reflowing these ---
      only the risk of breaking the consumer.

    **Not a divergence from pretty_yaml's rules** --- it is an additional safety
    exemption on top of rule 15. Because these values are folded scalars, the
    cross-validation corpus already skips them for *parity* (still asserts
    idempotency); behavioral coverage lives in
    `crates/panache-formatter/tests/format/yaml_verbatim_fields.rs`. Add a field
    only with a reproducing case --- candidates like Pandoc's `header-includes`
    / `include-in-header` are deferred until one surfaces.

17. **Folding long double-quoted scalars.** When a wrap mode is active (not
    `preserve`), a single-line **double-quoted** scalar whose key/value line
    overflows `line_width` is rewritten as a folded `>-` block scalar so its
    prose reflows under rule 15. Strip chomping (`-`) is used because a quoted
    scalar carries no trailing newline. The conversion fires only when it is
    **completely value-preserving**; otherwise the scalar stays quoted (and may
    overflow). Guards (leave quoted on any):
    - the scalar spans multiple lines (a multi-line `"…"`);
    - it contains any backslash escape other than `\\` or `\"` (`\n`, `\t`,
      `\uXXXX`, the `\<newline>` continuation, ... --- all introduce
      newlines/tabs/control chars/whitespace folding can't reproduce);
    - the decoded value has leading or trailing whitespace (folding strips it; a
      leading space also makes the line more-indented = literal);
    - the decoded value contains a control character (`cp < 0x20`, incl. TAB, or
      `0x7F`).

    Runs of >=2 spaces are **not** a guard: they round-trip because
    `wrap_plain_scalar_text` only breaks at a *single*-space separator and keeps
    longer runs verbatim mid-line (a fold collapses only the break). The
    conversion reuses rule 15's machinery: it builds a one-line `>-` candidate
    from the decoded value and runs it through `reflow_folded_scalar`, so it is
    idempotent by construction (pass 2 re-runs that same path) and applies only
    when reflow actually splits the value (>=2 body lines). The rule keys off
    the `"` prefix, so it does not match single-quoted scalars directly --- but
    a *simple* single-quoted scalar is first rewritten to double-quoted by rule
    3 and then folds here; a single-quoted scalar rule 3 *preserves* (its
    content holds a `'`) is left untouched.

    **Deliberate divergence from pretty_yaml**, which preserves the
    double-quoted scalar verbatim --- the same sanctioned-divergence class as
    rule 15. The cross-validation corpus detects the folded *output* and skips
    it for *parity* (idempotency still asserted); behavioral coverage lives in
    `crates/panache-formatter/tests/format/yaml_double_to_folded.rs`. Added for
    issue #388 (long `description:`/`title:` frontmatter overflowing the cap).

## Notes

Rules 4, 9, and 12 are "preserve" rules: they don't add a new behavior, they
explicitly decline to canonicalize a semantically-meaningful user choice.
They're still deterministic.

Rule 3 is the only spec rule with semantic-content awareness. The
escape-required test is decidable from the scalar's bytes alone (no context
dependence), so it remains rule-based.

## Plain-scalar wrapping (config, not spec)

Plain-scalar wrapping is a config option, not a spec rule. It is controlled by
Panache's `wrap` setting, which `yaml_engine.rs` maps onto pretty_yaml's
`ProseWrap`:

- `wrap: preserve` → `ProseWrap::Preserve` --- nothing wraps.
- `wrap: reflow` (default) / `sentence` / `semantic` → `ProseWrap::Always` ---
  plain scalars wrap with +2 indent continuation lines; folded (`>`) block
  scalars wrap their overlong body lines per rule 15; quoted (`"…"`, `'…'`) and
  literal (`|`) block scalars never wrap regardless of mode.

The in-tree formatter inherits this mapping at cutover. The spec-adjacent
invariant worth pinning: **plain scalars and folded (`>`) block scalars wrap;
quoted and literal (`|`) block scalars are preserved verbatim regardless of wrap
mode**. Wrapping a quoted scalar would change escape behavior (double-quoted) or
require backslash handling not present in single-quoted; wrapping a `|` literal
scalar would change its significant newlines. Folding (`>`) is the one block
style whose line breaks are *defined* to collapse to spaces, so reflowing its
body is loss-free (rule 15) --- this is a deliberate divergence from
pretty_yaml, which preserves all block scalars.

Edge case worth knowing about: a plain scalar containing `key: value`-shaped
text (colon followed by space, mid-content) is already ambiguous to strict YAML
parsers; wrapping it surfaces the breakage. The in-tree parser will likely
reject this input outright, making the wrap question moot. If we ever silently
accept it, the formatter must avoid wrapping at that boundary.

## Adding a new rule

Adding a new rule is a deliberate act. If development surfaces an edge case
neither the spec nor pretty_yaml currently covers, the resolution is a new rule
here (with a one-line rationale and a fixture under
`crates/panache-formatter/tests/fixtures/yaml_corpus/`) --- not a special-case
branch in the formatter.

New rules need cross-validation against pretty_yaml before landing. If they
conflict, decide explicitly which is right and document the decision. Most rules
become the spec *because* pretty_yaml agrees; rule 15 (folded-scalar wrapping)
is the one case so far where we decided to diverge --- its coverage lives in
in-tree-only tests rather than the parity corpus. See
`.claude/rules/yaml-formatter.md` for the process context.

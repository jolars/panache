# CommonMark conformance — running session recap

This file is the rolling, terse handoff between sessions of the
`commonmark-conformance-expand` skill. Read it at the start of a session for
suggested next targets and known follow-ups; rewrite the **Latest session**
entry at the end with what changed and what to look at next. Remove and replace
the "Latest session" entry with a new one at the end of each session, but 
check if there is something from the prior session that should be
carried forward.

Keep entries short. The full triage data lives in
`crates/panache-parser/tests/commonmark/report.txt` and
`docs/development/commonmark-report.json`; this file is for the *judgment calls*
a fresh session can't reconstruct from those artifacts (why a target was picked,
what was deliberately skipped, which fix unlocked which group).

--------------------------------------------------------------------------------

## Latest session — 2026-04-30 (xxviii)

**Pass count: 648 / 652 (99.4%, no change — triage-only,
no fix landed)**

Probed all four remaining failures (#523, #533, #569,
#571 — all in **Links**) to confirm session (xxvii)'s
classification ("all need inline IR migration") and to
look for a smaller-scope path. The classification stands;
one *refinement* for #523 is recorded below for the next
session that opens this work.

### Targets unlocked

None.

### Refinement: #523 has a narrower path than full IR

The carried-forward "Inline IR migration is prerequisite
for all four" is mostly correct, but **#523 specifically
can be unlocked with a refdef-aware scan-time check
without doing the full IR**. Root cause confirmed:
`parser/inlines/delimiter_stack.rs::scan_delim_runs`
calls
`try_parse_reference_link(..., allow_shortcut=true,
inline_link_attempted=false, ctx)` on every `[`. The
shortcut path matches *shape only* — it has no refdef
map to consult — so `[bar* baz]` (no refdef anywhere) is
treated as opaque and the inner `*` is hidden from the
delimiter scanner. CMark wants the brackets to fall
through to literal text in this case, exposing the inner
`*` as a closer for the outer `*foo`. (Confirmed via
`pandoc -f commonmark` — emphasis fires.)

Sketch of the minimal fix:

1. Pre-collect refdef labels from the input with a
   linear walk using
   `blocks::reference_links::try_parse_reference_definition`.
   Output: `HashSet<String>` of normalized labels (the
   normalization in
   `crates/panache-parser/tests/commonmark/html_renderer.rs::normalize_label`
   is the reference shape).
2. Thread the set through `Parser::parse` →
   `parse_inline_text_recursive` /
   `parse_inline_text` → `build_plan` →
   `scan_delim_runs`. (Non-Copy, so probably as a
   `&'a HashSet<String>` parameter rather than on
   `ParserOptions`.)
3. In `scan_delim_runs`, gate the shortcut /
   implicit-empty (`[text][]`) reference-link skip on
   `labels.contains(&normalized)`. The explicit
   `[text][label]` and inline `[text](url)` skips stay
   unconditional.

Verified by hand against the existing fixture
`emphasis_skips_shortcut_reference_link` (`*[foo*]` with
`[foo*]: /url` refdef): the fix preserves its CST shape
because the refdef label IS in the prescan set, so the
shortcut bracket stays opaque and the surrounding `*`
runs stay literal.

**Why I didn't land it this session:** the architectural
churn — a refdef pre-pass on `Parser`, threading a
non-`Copy` `HashSet` argument through ~5 module-function
signatures across `inlines/core.rs` and
`inlines/delimiter_stack.rs`, and updating public-API
entry points — pushes past the "single focused fix" bar
even though the unblock is one spec example. A future
session that tackles the IR migration will need this
refdef map *anyway* to drive the bracket scanner's
link/no-link decision per CMark §6.3, so building it as
part of the IR work avoids the same plumbing twice.

#533/#569/#571 still need the full IR — no refinement
there. #533 is the inner-`[baz][ref]` → outer-`[…][ref]`
suppression rule for *reference* links (the inline
variant is already handled via
`disallow_inner_links` per session xviii). #569/#571 are
multi-pair `[foo][bar][baz]` resolution where the
parser must scan with refdef awareness, not just by
shape.

### Files touched

None. The probe test added during triage was deleted
before finishing per the skill's "delete it before
finishing" rule.

### Don't redo

- **Don't pursue a non-refdef-aware "never skip
  shortcut" simplification.** Verified by hand: setting
  `allow_shortcut=false` (or otherwise unconditionally
  refusing to skip shortcut form) in `scan_delim_runs`
  would regress
  `emphasis_skips_shortcut_reference_link`. CMark wants
  shortcut brackets opaque to outer emphasis *when the
  refdef resolves*; the distinction can't be made
  without the refdef map.
- **Don't try to fix #523 without #533/#569/#571 in
  mind.** They share the same scan-time mismatch — the
  scanner is blind to the refdef map — but #523 is the
  only one solvable with just refdef *labels*. The
  others additionally need full link-resolution
  semantics (inner-link suppression, multi-pair scan
  precedence). A refdef-label pre-pass that gets dropped
  later when the IR lands is wasted plumbing.

### Suggested next targets, ranked

(Carries forward session xxvii's ranking — no changes
warranted; this session only sharpened the #523 picture.)

1. **Inline IR migration** — turn
   `delimiter_stack`'s `Vec<DelimRun>` into a real
   linked-list IR with `Text`, `Construct(GreenNode)`,
   `DelimRun`, `EmphasisGroup`, `BracketMarker` events.
   Move the byte walk in `parse_inline_range_impl`
   (CommonMark path) into an IR-builder pass, and
   emission into an IR-walker. Bundle the refdef-label
   pre-pass (see *Refinement* above) into this work so
   the bracket-marker side has resolution context from
   day one.
2. **#523 / #533** — emphasis closing inside link
   bracket text, and the inner-link → outer-`[…][ref]`
   suppression rule. #523 falls out of the IR + refdef
   pre-pass directly; #533 needs the IR's bracket
   marker plus a reference-aware variant of
   `link_text_contains_inner_link`.
3. **#569 / #571** — `[foo][bar][baz]` reference-link
   nesting. Needs the bracket scanner plus a
   refdef-aware resolution pass that scans
   right-to-left (or with three-pair lookahead).
4. **Pandoc dialect migration onto the unified
   algorithm.** Once the IR is in place, parameterize
   `process_emphasis`'s flanking predicates by
   `Dialect` and run Pandoc through it too.
5. **Formatter fix for nested-only outer LIST_ITEM** —
   carried prerequisite for lifting the same-line
   nested-LIST and blockquote-in-list-item dialect
   gates.
6. **Multi-line setext inside list items** (CommonMark
   only) — paired parser + formatter fixtures, dialect-
   gated. Strictly cosmetic; no spec example exercises
   it in the conformance harness today.

### Carry-forward from prior sessions

(Carrying forward from session xxvii unless noted.)

- Session (xxvii)'s
  `Parser::try_fold_list_item_buffer_into_setext` (in
  `parser/core.rs`) is load-bearing for #300; the
  helper *must* run before `dispatcher_match` is
  computed in `parse_inner_content`, and it must bail
  on `segment_count() != 1` (multi-line setext inside
  list items is dialect-divergent and out of scope).
  The indent guard
  `underline_indent_cols < content_col → return false`
  is also load-bearing — without it, #94/#99/#281/#282
  flip to spurious setext headings.
- Session (xxvii)'s formatter list-loose detection
  (`crates/panache-formatter/src/formatter/lists.rs`)
  uses the combined predicate
  `(has_blank_within_item || has_structural_multi_block)
  && !has_nested_lists`. **Don't include `HTML_BLOCK`
  in `is_loose_trigger_block`** — the
  `ignore_directives` fixture relies on
  PLAIN+HTML_BLOCK+PLAIN+HTML_BLOCK staying tight
  (those HTML blocks are panache's
  `<!-- panache-ignore-* -->` directives).
- Session (xxvi)'s emphasis carry-forward (delimiter
  stack, `EmphasisPlan`, `Vec<DelimRun>` representation,
  the four "don't redo" notes about
  `coalesce-on-Literal`, `openers_bottom`, byte-keyed
  plan, plan threading through nested recursion) is all
  still load-bearing on the CommonMark inline path.
  Don't extend that module to bracket markers without
  first migrating to the linked-list IR (see *Suggested
  next targets* #1).
- Session (xxv)'s setext list-item indent guard and
  `in_marker_only_list_item` flag in
  `block_dispatcher.rs` are load-bearing for #278;
  session (xxvii)'s fold helper is *additive* (different
  code path, different state) and does not interact with
  them. Don't try to merge the two.
- Session (ix)'s "tail-end only" emphasis heuristic and
  Pandoc dialect gate still apply to the Pandoc inline
  path only; session (xxvi) replaced the CommonMark
  path entirely.
- Session (x)/(xi)'s link-scanner skip pattern (autolink
  / raw-HTML opacity for emphasis closer + link bracket
  close) is load-bearing for #524/#526/#536/#538. Don't
  unify the autolink and raw-HTML skip flags — Pandoc
  treats them differently. The bracket scanner work for
  #523/#533/#569/#571 will need to interoperate with
  these flags, not replace them.
- Session (xii)'s lazy paragraph continuation across
  reduced blockquote depth, session (xiii)'s
  `try_lazy_list_continuation` for OpenList only at
  `indent_cols ≥ 4`, session (xvii)'s HTML block #148
  fix (`</pre>` rejection in VERBATIM_TAGS), session
  (xviii)'s `disallow_inner_links` flag scope (inline
  links only — reference-link nesting #569/#571 needs a
  different pass), session (xix)/(xxi)/(xxii)'s
  column-aware indented-code logic (list-item
  `marker_spaces_after` and `virtual_marker_space`
  separate from blockquote `virtual_absorbed`), and
  session (xxiv)'s same-line blockquote-in-list-item
  branch dialect gate are all unchanged and unaffected
  by this session.

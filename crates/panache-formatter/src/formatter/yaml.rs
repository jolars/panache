//! In-tree YAML formatter (shadow, Phase 1 of the cutover plan).
//!
//! Consumes the in-tree parser CST
//! ([`panache_parser::parser::yaml::parse_yaml_tree`]) and emits
//! deterministically-styled YAML text per the 14 style rules in
//! `STYLE.md` (next to this file) — the canonical spec, relocated
//! from `.claude/skills/yaml-formatter-cutover/plan.md` in Phase 1.2.
//!
//! **Not wired into the live formatting pipeline.** Until the joint
//! cutover lands (Phase 2), live YAML output still routes through
//! [`crate::yaml_engine`] → `pretty_yaml`. This module is
//! cross-validated against pretty_yaml on a corpus by
//! `crates/panache-formatter/tests/yaml_cross_validation.rs`. See
//! `.claude/skills/yaml-formatter-cutover/SKILL.md` for scope.
//!
//! Phase 1.13 status: cross-validation harness live; rules 1
//! (canonical 2-space indent driven by entry/item nesting depth),
//! 2 (sequence items indent +2 from parent key — carried by rule 1's
//! depth math, no separate code), 3 (prefer double-quoted over
//! single-quoted unless the de-escaped content has `\`, `'`, `"`, or
//! a control char that would require backslash-escaping in
//! double-quoted form; plain stays plain; double stays double),
//! 5 (canonical flow spacing for single-line, comment-free
//! `YAML_FLOW_SEQUENCE` / `YAML_FLOW_MAP` subtrees), 6 (overflow wrap:
//! when a flow container's single-line form pushes its enclosing line
//! past `line_width`, rewrite each item onto its own line at the
//! parent entry/item's content column + 2, with trailing comma and a
//! standalone closing bracket; opening bracket stays on the key
//! line), 7 (collapse blank-line runs; strip leading blanks
//! entirely), 8 (one space before inline `#` comments), 10 (strip
//! trailing whitespace per line), 13 (exactly one trailing `\n` at
//! EOF), and 14 (collapse runs of whitespace between block
//! structural indicators — `:` after a block-map key, `-` after a
//! block-sequence marker — and inline content on the same line to a
//! single space; trailing-only runs are left for rule 10)
//! implemented in [`document`]. All behavior-changing rules are
//! live; preserve rules 4 (block-scalar style), 9 (comment
//! positions), 11 (empty scalars), and 12 (key order) are locked in
//! by corpus + unit tests with no formatter code (they cross-validate
//! against pretty_yaml because both implementations leave these
//! shapes alone). Corpus covers trivially-canonical inputs,
//! trailing-newline + trailing-WS shape rules, rule-1 indent
//! stressors, rule-2 parent-column sequences, rule-3 quote-style
//! cases (single→double when safe; single kept for `\` / `'` / `"`
//! content; conversion on keys + flow items), rule-5 flow-spacing
//! cases, rule-6 overflow-wrap cases (depths 0/1/2, block-sequence
//! parent shifts items +4, sequence-of-maps keeps nested flow
//! canonical, just-at-80 boundary), rule-7 blank-line cases, rule-8
//! inline-comment cases, rule-4 literal/folded + chomping indicator
//! cases, rule-9 between-keys / between-seq-items / trailing-comment
//! cases, rule-11 empty-scalar cases (bare, multiple, with inline
//! comment, in sequence), rule-12 key-order cases (reverse-alpha,
//! deep nesting), rule-14 cases (multi-space after `:` / `-`, tab
//! after `:`), and six harvested real-frontmatter cases
//! (`real/quarto_frontmatter_keywords`, `whitespace_normalization`,
//! `non_ascii_scalar`, `quarto_landing_page`,
//! `folded_description_performance`, `folded_description_short`).
//! Block scalar (`|`/`>`) interior lines are left verbatim — rule 1
//! needs a real block-scalar renderer to canonicalize them.
//! Multi-line flow input (containing `\n` between brackets) is still
//! rejected by the in-tree parser, so the "multi-line input is
//! sticky" behavior pretty_yaml shows is parked until the parser
//! supports it.

#[path = "yaml/block_map.rs"]
mod block_map;
#[path = "yaml/block_sequence.rs"]
mod block_sequence;
#[path = "yaml/document.rs"]
mod document;
#[path = "yaml/flow.rs"]
mod flow;
#[path = "yaml/options.rs"]
mod options;
#[path = "yaml/scalar.rs"]
mod scalar;

pub use options::{WrapMode, YamlFormatOptions};

/// Format the given YAML source under the in-tree formatter.
///
/// On a parse error (input the in-tree parser rejects outright),
/// returns the input verbatim. The cross-validation harness treats
/// that as a "skip" case — pretty_yaml also passes its input through
/// on its own rejection path, and a shadow formatter shouldn't be
/// the thing that surfaces parse errors.
pub fn format_yaml(input: &str, opts: &YamlFormatOptions) -> String {
    match panache_parser::parser::yaml::parse_yaml_tree(input) {
        Some(tree) => document::render(&tree, opts),
        None => input.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_gets_one_trailing_newline() {
        // STYLE.md rule 13: every successfully-formatted document
        // ends with exactly one `\n`. The parser accepts `""` as an
        // empty document, so the formatter emits `"\n"`.
        let opts = YamlFormatOptions::default();
        assert_eq!(format_yaml("", &opts), "\n");
    }

    #[test]
    fn already_canonical_mapping_is_byte_stable() {
        // Token bodies are still emitted verbatim (no per-container
        // restyling yet); inputs already in canonical form round-trip
        // unchanged. Cross-validation lives in
        // `tests/yaml_cross_validation.rs`.
        let opts = YamlFormatOptions::default();
        let input = "title: My Title\nauthor: Me\n";
        assert_eq!(format_yaml(input, &opts), input);
    }

    #[test]
    fn multiple_trailing_newlines_collapse_to_one() {
        let opts = YamlFormatOptions::default();
        assert_eq!(format_yaml("key: value\n\n\n", &opts), "key: value\n");
    }

    #[test]
    fn missing_trailing_newline_gets_one() {
        let opts = YamlFormatOptions::default();
        assert_eq!(format_yaml("key: value", &opts), "key: value\n");
    }

    #[test]
    fn nested_indent_canonicalizes_to_two_spaces() {
        // STYLE.md rule 1: every content line is indented by
        // 2 * (entry/item nesting depth - 1). 4-space indent
        // collapses to 2-space; triply-nested 8-space collapses to
        // 4-space; sequence-in-mapping dashes land at parent-key + 2.
        let opts = YamlFormatOptions::default();
        assert_eq!(
            format_yaml("outer:\n    inner: value\n", &opts),
            "outer:\n  inner: value\n"
        );
        assert_eq!(
            format_yaml("a:\n    b:\n        c: value\n", &opts),
            "a:\n  b:\n    c: value\n"
        );
        assert_eq!(
            format_yaml("items:\n    - foo\n    - bar\n", &opts),
            "items:\n  - foo\n  - bar\n"
        );
    }

    #[test]
    fn block_scalar_interior_lines_preserved() {
        // Rule 1 skips block-scalar (`|`/`>`) interior lines: their
        // indent is baked into a single multi-line YAML_SCALAR token
        // and proper canonicalization needs a real block-scalar
        // renderer. The indicator line itself (`key: |`) still gets
        // standard rewriting.
        let opts = YamlFormatOptions::default();
        let input = "key: |\n  line one\n  line two\n";
        assert_eq!(format_yaml(input, &opts), input);
    }

    #[test]
    fn rule_5_flow_spacing_canonicalized() {
        // Rule 5: flow sequence — no space inside `[]`, one space after
        // each `,`. Flow map — one space inside `{}`, one space after
        // each `,`, one space after each `:`.
        let opts = YamlFormatOptions::default();
        assert_eq!(format_yaml("tags: [a,b,c]\n", &opts), "tags: [a, b, c]\n");
        assert_eq!(
            format_yaml("tags: [ a , b , c ]\n", &opts),
            "tags: [a, b, c]\n"
        );
        assert_eq!(
            format_yaml("obj: {key: value}\n", &opts),
            "obj: { key: value }\n"
        );
        assert_eq!(
            format_yaml("obj: {  key: value  }\n", &opts),
            "obj: { key: value }\n"
        );
        assert_eq!(
            format_yaml("a: {x: 1,y: 2}\n", &opts),
            "a: { x: 1, y: 2 }\n"
        );
        // Pathological: parser can't structure `{key:value}` into
        // entries; emit `{ inner }` with spacing normalized around
        // the unparseable content.
        assert_eq!(
            format_yaml("obj: {key:value}\n", &opts),
            "obj: { key:value }\n"
        );
        // Empty containers stay empty (no inner space).
        assert_eq!(format_yaml("e: []\n", &opts), "e: []\n");
        assert_eq!(format_yaml("e: {}\n", &opts), "e: {}\n");
    }

    #[test]
    fn rule_5_multiline_flow_preserved_verbatim() {
        // Multi-line flow containers stay verbatim — rule 6 will own
        // multi-line wrap. The boundary check (`can_canonicalize_flow`
        // returns false when the container's text contains `\n`) keeps
        // the canonical emitter out of the way.
        let opts = YamlFormatOptions::default();
        let input = "tags: [\n  a,\n  b,\n]\n";
        assert_eq!(format_yaml(input, &opts), input);
    }

    #[test]
    fn rule_8_inline_comment_spacing_normalized() {
        // Rule 8: exactly one space before `#` for inline comments.
        // Standalone comments (line-start) keep their original
        // surrounding whitespace (no inline context to normalize).
        let opts = YamlFormatOptions::default();
        assert_eq!(
            format_yaml("key: value   # loose\n", &opts),
            "key: value # loose\n"
        );
        assert_eq!(
            format_yaml("key: value #tight\n", &opts),
            "key: value #tight\n" // tight body preserved; only WS before `#` is normalized (already one)
        );
        assert_eq!(
            format_yaml("a: 1  # first\nb: 2     # second\n", &opts),
            "a: 1 # first\nb: 2 # second\n"
        );
        // Standalone comment at line start: pass through.
        assert_eq!(
            format_yaml("# standalone\nkey: value\n", &opts),
            "# standalone\nkey: value\n"
        );
    }

    #[test]
    fn rule_7_collapses_blank_line_runs() {
        // Rule 7: interior runs collapse to one blank line; leading
        // blank lines are stripped entirely.
        let opts = YamlFormatOptions::default();
        assert_eq!(format_yaml("a: 1\n\n\n\nb: 2\n", &opts), "a: 1\n\nb: 2\n");
        assert_eq!(format_yaml("a: 1\n\nb: 2\n", &opts), "a: 1\n\nb: 2\n");
        assert_eq!(format_yaml("\n\n\nkey: value\n", &opts), "key: value\n");
        // Whitespace-only "blank" lines (stripped to empty by rule 10
        // first) participate in the collapse.
        assert_eq!(
            format_yaml("a: 1\n   \n   \n   \nb: 2\n", &opts),
            "a: 1\n\nb: 2\n"
        );
    }

    #[test]
    fn rule_2_sequence_indents_under_parent_key() {
        // Rule 2: sequence items indent +2 from the parent key, never at
        // the parent column. Carried by rule 1's depth formula (entry/item
        // ancestors − 1) — verified here so the behavior is locked even if
        // rule 1's implementation moves.
        let opts = YamlFormatOptions::default();
        assert_eq!(
            format_yaml("categories:\n- foo\n- bar\n", &opts),
            "categories:\n  - foo\n  - bar\n"
        );
        assert_eq!(
            format_yaml("people:\n- name: Alice\n  age: 30\n- name: Bob\n", &opts),
            "people:\n  - name: Alice\n    age: 30\n  - name: Bob\n"
        );
    }

    #[test]
    fn rule_6_overflowing_flow_wraps() {
        // Rule 6: when a flow container's single-line form would push its
        // enclosing line past the 80-char default, wrap each item onto its
        // own line at parent's content column + 2, with trailing comma and
        // a standalone closing bracket aligned at the parent's content
        // column. The opening bracket stays on the key line.
        let opts = YamlFormatOptions::default();
        // Block-map entry at depth 0: items at col 2, `]` at col 0.
        let input =
            "k: [itm00, itm01, itm02, itm03, itm04, itm05, itm06, itm07, itm08, itm09, itm10x]\n";
        let expected = "k: [\n  itm00,\n  itm01,\n  itm02,\n  itm03,\n  itm04,\n  itm05,\n  itm06,\n  itm07,\n  itm08,\n  itm09,\n  itm10x,\n]\n";
        assert_eq!(format_yaml(input, &opts), expected);
        // Just-at-80 doesn't wrap; just-over does.
        let no_overflow =
            "k: [itm00, itm01, itm02, itm03, itm04, itm05, itm06, itm07, itm08, itm09, itm10]\n";
        assert_eq!(format_yaml(no_overflow, &opts), no_overflow);
    }

    #[test]
    fn rule_6_wrap_aligns_to_parent_content_column() {
        // Depth-1 block-map entry: items at col 4 (parent indent 2 + 2),
        // `]` at col 2.
        let opts = YamlFormatOptions::default();
        let input = "deep:\n  inner: [aaaaaaaa, bbbbbbbb, cccccccc, dddddddd, eeeeeeee, ffffffff, gggggggg, hhhhhhhh]\n";
        let expected = "deep:\n  inner: [\n    aaaaaaaa,\n    bbbbbbbb,\n    cccccccc,\n    dddddddd,\n    eeeeeeee,\n    ffffffff,\n    gggggggg,\n    hhhhhhhh,\n  ]\n";
        assert_eq!(format_yaml(input, &opts), expected);
    }

    #[test]
    fn rule_6_wrap_in_block_sequence_shifts_two_extra() {
        // Block-sequence item adds a `- ` prefix to its line; `]` aligns
        // with the column after `- ` (block-seq depth indent + 2), items at
        // +2 from there.
        let opts = YamlFormatOptions::default();
        let input = "items:\n  - [aaaaaaaa, bbbbbbbb, cccccccc, dddddddd, eeeeeeee, ffffffff, gggggggg, hhhhhhhh, iiiiiiii]\n";
        let expected = "items:\n  - [\n      aaaaaaaa,\n      bbbbbbbb,\n      cccccccc,\n      dddddddd,\n      eeeeeeee,\n      ffffffff,\n      gggggggg,\n      hhhhhhhh,\n      iiiiiiii,\n    ]\n";
        assert_eq!(format_yaml(input, &opts), expected);
    }

    #[test]
    fn rule_6_wrap_round_trips_multiline_input() {
        // Multi-line flow input (the parser now accepts the closing-`]`
        // at parent indent that rule 6 emits) must round-trip through
        // the formatter unchanged — items keep their canonical indent
        // and the closing bracket stays at parent_content_col.
        let opts = YamlFormatOptions::default();
        let input = "k: [\n  itm00,\n  itm01,\n  itm02,\n  itm03,\n  itm04,\n  itm05,\n  itm06,\n  itm07,\n  itm08,\n  itm09,\n  itm10x,\n]\n";
        let formatted = format_yaml(input, &opts);
        assert_eq!(
            formatted, input,
            "multi-line flow input should be sticky\n got: {formatted:?}\nwant: {input:?}",
        );
        let pass2 = format_yaml(&formatted, &opts);
        assert_eq!(pass2, formatted, "idempotency");
    }

    #[test]
    fn rule_6_wrap_preserves_nested_flow_canonical() {
        // When the outer wraps, inner flow items that fit stay in their
        // canonical single-line form (rule 5). pretty_yaml does the same.
        let opts = YamlFormatOptions::default();
        let input = "k: [{a: 1, b: 2}, {c: 3, d: 4}, {e: 5, f: 6}, {g: 7, h: 8}, {i: 9, j: 10}, {k: 11, l: 12}]\n";
        let expected = "k: [\n  { a: 1, b: 2 },\n  { c: 3, d: 4 },\n  { e: 5, f: 6 },\n  { g: 7, h: 8 },\n  { i: 9, j: 10 },\n  { k: 11, l: 12 },\n]\n";
        assert_eq!(format_yaml(input, &opts), expected);
    }

    #[test]
    fn rule_3_single_to_double_when_safe() {
        // Rule 3: single-quoted scalars whose de-escaped content has none
        // of `\`, `'`, `"`, or control chars convert to double-quoted.
        // Plain stays plain; double-quoted stays double-quoted.
        let opts = YamlFormatOptions::default();
        assert_eq!(format_yaml("k: 'hello'\n", &opts), "k: \"hello\"\n");
        assert_eq!(
            format_yaml("k: 'hello world'\n", &opts),
            "k: \"hello world\"\n"
        );
        // Content that would force quoting if plain (has `:`, `-`, `[`...)
        // still converts single → double — the conversion only cares
        // about double-form escape needs, not plain-form ambiguity.
        assert_eq!(format_yaml("k: 'foo: bar'\n", &opts), "k: \"foo: bar\"\n");
        assert_eq!(format_yaml("k: '-value'\n", &opts), "k: \"-value\"\n");
        assert_eq!(format_yaml("k: '[1,2,3]'\n", &opts), "k: \"[1,2,3]\"\n");
        // Type-ambiguous strings stay quoted (single → double); the
        // quote preserves the string semantics that distinguish `true`
        // the bool from `"true"` the string.
        assert_eq!(format_yaml("k: 'true'\n", &opts), "k: \"true\"\n");
        assert_eq!(format_yaml("k: '42'\n", &opts), "k: \"42\"\n");
        // Empty single becomes empty double.
        assert_eq!(format_yaml("k: ''\n", &opts), "k: \"\"\n");
        // Plain stays plain; double stays double.
        assert_eq!(format_yaml("k: hello\n", &opts), "k: hello\n");
        assert_eq!(format_yaml("k: \"hello\"\n", &opts), "k: \"hello\"\n");
    }

    #[test]
    fn rule_3_single_kept_when_escape_needed() {
        // Single is preserved when the de-escaped content has `\`, `'`,
        // or `"` — i.e., anything where double would require backslash
        // escaping or change escape character usage.
        let opts = YamlFormatOptions::default();
        assert_eq!(
            format_yaml("k: 'C:\\Users\\test'\n", &opts),
            "k: 'C:\\Users\\test'\n"
        );
        assert_eq!(
            format_yaml("k: 'he said \"hi\"'\n", &opts),
            "k: 'he said \"hi\"'\n"
        );
        // Doubled apostrophe (de-escapes to `'`) keeps single.
        assert_eq!(format_yaml("k: 'don''t'\n", &opts), "k: 'don''t'\n");
    }

    #[test]
    fn rule_3_applies_to_keys_and_flow_items() {
        // Single → double conversion fires on quoted KEYS and on quoted
        // scalars inside flow containers (which themselves carry through
        // rule 5's canonical spacing).
        let opts = YamlFormatOptions::default();
        assert_eq!(format_yaml("'hello': world\n", &opts), "\"hello\": world\n");
        assert_eq!(
            format_yaml("tags: ['foo', 'bar', baz]\n", &opts),
            "tags: [\"foo\", \"bar\", baz]\n"
        );
    }

    #[test]
    fn rule_4_block_scalar_style_preserved() {
        // Rule 4: literal `|` and folded `>` carry different YAML semantics
        // and are not interchangeable. Chomping indicators (`-` / `+`) and
        // indent indicators ride along with the header.
        let opts = YamlFormatOptions::default();
        let literal = "msg: |\n  line one\n  line two\n";
        assert_eq!(format_yaml(literal, &opts), literal);
        let folded = "msg: >\n  line one\n  line two\n";
        assert_eq!(format_yaml(folded, &opts), folded);
        let literal_strip = "msg: |-\n  line one\n  line two\n";
        assert_eq!(format_yaml(literal_strip, &opts), literal_strip);
        let folded_keep = "msg: >+\n  line one\n  line two\n";
        assert_eq!(format_yaml(folded_keep, &opts), folded_keep);
    }

    #[test]
    fn rule_9_comment_positions_preserved() {
        // Rule 9: comments above keys, between items, and at document end
        // are preserved at their original positions. Standalone-comment
        // surrounding whitespace passes through (rule 8 only normalizes the
        // single space before an inline `#`).
        let opts = YamlFormatOptions::default();
        let between_keys = "a: 1\n# between\nb: 2\n";
        assert_eq!(format_yaml(between_keys, &opts), between_keys);
        let between_seq_items = "items:\n  - foo\n  # mid\n  - bar\n";
        assert_eq!(format_yaml(between_seq_items, &opts), between_seq_items);
        let trailing = "key: value\n# trailing comment\n";
        assert_eq!(format_yaml(trailing, &opts), trailing);
        let blank_separated = "a: 1\n\n# section\nb: 2\n";
        assert_eq!(format_yaml(blank_separated, &opts), blank_separated);
    }

    #[test]
    fn rule_11_empty_scalars_preserved() {
        // Rule 11: `key:` stays `key:`; never canonicalized to `key: null`
        // or `key: ""`. An empty value in a sequence stays as a bare `-`.
        let opts = YamlFormatOptions::default();
        assert_eq!(format_yaml("key:\n", &opts), "key:\n");
        assert_eq!(format_yaml("a:\nb:\nc: 1\n", &opts), "a:\nb:\nc: 1\n");
        // An inline comment after an empty value keeps the empty.
        assert_eq!(format_yaml("key: # comment\n", &opts), "key: # comment\n");
    }

    #[test]
    fn rule_12_key_order_preserved() {
        // Rule 12: frontmatter is user-written; reordering would surprise.
        // Reverse-alphabetic input stays reverse-alphabetic; deep nesting
        // doesn't re-sort either level.
        let opts = YamlFormatOptions::default();
        let reverse = "zebra: 1\nyak: 2\nape: 3\n";
        assert_eq!(format_yaml(reverse, &opts), reverse);
        let deep =
            "root:\n  z: 1\n  a: 2\n  m: 3\nnested:\n  outer:\n    second: 2\n    first: 1\n";
        assert_eq!(format_yaml(deep, &opts), deep);
    }

    #[test]
    fn rule_14_collapses_run_after_colon() {
        // Rule 14: whitespace between `:` (block-map structural indicator)
        // and inline value collapses to one space.
        let opts = YamlFormatOptions::default();
        assert_eq!(format_yaml("k:    v\n", &opts), "k: v\n");
        assert_eq!(format_yaml("k:\tv\n", &opts), "k: v\n");
        assert_eq!(format_yaml("k:   v # c\n", &opts), "k: v # c\n");
        // No content after `:` on the same line — left for rule 10 to strip.
        assert_eq!(
            format_yaml("k:   \n  inner: v\n", &opts),
            "k:\n  inner: v\n"
        );
    }

    #[test]
    fn rule_14_collapses_run_after_dash() {
        // Rule 14: whitespace between `-` (block-sequence indicator) and
        // inline item collapses to one space.
        let opts = YamlFormatOptions::default();
        assert_eq!(
            format_yaml("items:\n  -    foo\n  -      bar\n", &opts),
            "items:\n  - foo\n  - bar\n"
        );
        // Bare `-` with only trailing whitespace stays empty (rule 10 strips).
        assert_eq!(
            format_yaml("items:\n  -   \n  - foo\n", &opts),
            "items:\n  -\n  - foo\n"
        );
    }

    #[test]
    fn trailing_whitespace_stripped_per_line() {
        // STYLE.md rule 10: trailing space + tab stripped from every
        // line; CRLF preserved (rule applies only to space/tab, not
        // `\r`); whitespace-only lines collapse to empty.
        let opts = YamlFormatOptions::default();
        assert_eq!(format_yaml("key: value   \n", &opts), "key: value\n");
        assert_eq!(format_yaml("key: value\t\n", &opts), "key: value\n");
        assert_eq!(format_yaml("a: 1\n   \nb: 2\n", &opts), "a: 1\n\nb: 2\n");
        assert_eq!(format_yaml("   ", &opts), "\n");
    }
}

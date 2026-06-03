//! Parity harness for the prefix-aware YAML scanner+builder
//! (`parse_stream_with_prefix` / `validate_yaml_with_prefix`,
//! Phase 2c step 2 of the yaml-formatter cutover).
//!
//! For a corpus of hashpipe (`#|`)-prefixed YAML payloads each case asserts:
//!
//! 1. **Losslessness** — the prefix-aware CST reproduces the raw prefixed
//!    bytes (`tree.text() == input`), so the YAML token ranges are host
//!    ranges directly (prefix bytes carried as `YAML_LINE_PREFIX` trivia,
//!    no offset remapping).
//! 2. **Structural parity** — projecting the prefix-aware tree yields the
//!    same yaml-test-suite event stream as projecting a plain parse of
//!    the prefix-stripped baseline (the projector already skips
//!    `YAML_LINE_PREFIX` leaves). A divergence means the prefix-excluded
//!    column/indent accounting drifted from the stripped baseline.
//! 3. **Validator agreement** — these payloads are all valid YAML once
//!    stripped, so `validate_yaml_with_prefix` reports no error.

use panache_parser::parser::yaml::{
    parse_stream, parse_stream_with_prefix, project_events_from_tree, validate_yaml_with_prefix,
};

const PREFIX: &str = "#|";

/// Replicates the production `normalize_hashpipe_input` baseline: strip
/// `#|` plus at most one following space from each line, join with `\n`.
fn strip_baseline(input: &str) -> String {
    input
        .lines()
        .map(|line| match line.strip_prefix(PREFIX) {
            Some(rest) => rest.strip_prefix(' ').unwrap_or(rest),
            None => line,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Each case: a label and a `#|`-prefixed payload. The label names the
/// scanner accounting path the shape stresses.
const CORPUS: &[(&str, &str)] = &[
    ("single_line_map", "#| echo: true\n#| warning: false\n"),
    ("no_space_after_marker", "#|echo: true\n#|warning: false\n"),
    ("nested_map", "#| a:\n#|   b: 1\n#|   c: 2\n"),
    (
        "dotted_key_nested_seq",
        "#| cache.extra: true\n#| cache-vars:\n#|   - x\n#|   - y\n",
    ),
    (
        "block_sequence",
        "#| fig-subcap:\n#|   - ROC\n#|   - PR Curve\n",
    ),
    ("tag_value", "#| transform: !expr 1 + 1\n"),
    ("flow_collection", "#| layout: [[1, 1], [1]]\n"),
    ("blank_line_between_keys", "#| a: 1\n#|\n#| b: 2\n"),
    // Multi-line plain scalar continuation: stresses the plain-scalar
    // continuation column check.
    (
        "plain_multiline_value",
        "#| fig-cap: Comparing ROC\n#|   and PR curve\n",
    ),
    // Single-quoted multi-line scalar: stresses the flow-scalar
    // continuation skip.
    (
        "quoted_multiline_value",
        "#| fig-cap: 'Comparing ROC (left) and PR curve (right) for a random forest\n#|   trained on a task.'\n",
    ),
    // Literal block scalar with an interior blank line: stresses the
    // block-scalar content loop and the blank `#|` line.
    (
        "literal_block_scalar_blank_line",
        "#| fig-alt: |\n#|   First paragraph.\n#|\n#|   Second paragraph.\n",
    ),
    // Folded block scalar with auto-detected indent: stresses
    // `auto_detect_block_scalar_indent` under a prefix.
    (
        "folded_block_scalar",
        "#| desc: >\n#|   long folded\n#|   text here\n",
    ),
];

#[test]
fn prefix_aware_parse_is_lossless() {
    for (label, input) in CORPUS {
        let tree = parse_stream_with_prefix(input, PREFIX);
        assert_eq!(
            tree.text().to_string(),
            *input,
            "losslessness failed for `{label}`",
        );
    }
}

#[test]
fn prefix_aware_structure_matches_stripped_baseline() {
    for (label, input) in CORPUS {
        let prefixed = parse_stream_with_prefix(input, PREFIX);
        let baseline = parse_stream(&strip_baseline(input));
        assert_eq!(
            project_events_from_tree(&prefixed),
            project_events_from_tree(&baseline),
            "structural parity failed for `{label}`",
        );
    }
}

#[test]
fn prefix_aware_validation_agrees_with_baseline() {
    for (label, input) in CORPUS {
        assert!(
            validate_yaml_with_prefix(input, PREFIX).is_none(),
            "expected `{label}` to validate (it is valid once stripped)",
        );
    }
}

/// The empty-prefix path must behave exactly like the plain parse so the
/// frontmatter (no-prefix) callers are unaffected.
#[test]
fn empty_prefix_matches_plain_parse() {
    let input = "title: Test\nauthors:\n  - A\n  - B\n";
    let with_empty = parse_stream_with_prefix(input, "");
    let plain = parse_stream(input);
    assert_eq!(with_empty.text().to_string(), plain.text().to_string());
    assert_eq!(
        project_events_from_tree(&with_empty),
        project_events_from_tree(&plain),
    );
    assert!(validate_yaml_with_prefix(input, "").is_none());
}

use panache_formatter::config::{Extensions, Flavor};
use panache_formatter::{Config, ConfigBuilder, HorizontalRuleStyle, format};

fn compact_config() -> Config {
    ConfigBuilder::default()
        .horizontal_rule_style(HorizontalRuleStyle::Compact)
        .build()
}

fn compact_gfm_config() -> Config {
    let flavor = Flavor::Gfm;
    Config {
        flavor,
        parser_extensions: Extensions::for_flavor(flavor),
        horizontal_rule_style: HorizontalRuleStyle::Compact,
        ..Default::default()
    }
}

#[test]
fn compact_rule_emits_three_dashes() {
    let input = "***\n";
    let expected = "---\n";
    let out = format(input, Some(compact_config()), None);
    assert_eq!(out, expected);
    assert_eq!(format(&out, Some(compact_config()), None), expected);
}

#[test]
fn compact_rule_in_blockquote() {
    let input = "> ***\n";
    let expected = "> ---\n";
    let out = format(input, Some(compact_config()), None);
    assert_eq!(out, expected);
    assert_eq!(format(&out, Some(compact_config()), None), expected);
}

#[test]
fn default_style_still_expands_to_line_width() {
    let cfg = ConfigBuilder::default().line_width(12).build();
    let out = format("***\n", Some(cfg), None);
    assert_eq!(out, "------------\n");
}

// `---` doubles as a YAML metadata delimiter: a *tight* `---\nkey: value\n---`
// is consumed as a metadata block (matching pandoc), even mid-document. The
// compact style is only safe because the formatter guarantees blank lines
// around rules, which breaks that reading. These tests pin the invariant.

#[test]
fn compact_rule_before_yaml_shaped_paragraph_is_idempotent() {
    let input = "a\n\n***\n\nkey: value\n";
    let expected = "a\n\n---\n\nkey: value\n";
    let out = format(input, Some(compact_config()), None);
    assert_eq!(out, expected);
    assert_eq!(format(&out, Some(compact_config()), None), expected);
}

#[test]
fn compact_rules_bracketing_yaml_shaped_paragraph_stay_rules() {
    let input = "a\n\n***\n\nkey: value\n\n***\n";
    let expected = "a\n\n---\n\nkey: value\n\n---\n";
    let out = format(input, Some(compact_config()), None);
    assert_eq!(out, expected);
    assert_eq!(format(&out, Some(compact_config()), None), expected);
}

#[test]
fn compact_rule_at_top_of_file_does_not_become_frontmatter() {
    let input = "***\n\nkey: value\n\n***\n";
    let expected = "---\n\nkey: value\n\n---\n";
    let out = format(input, Some(compact_config()), None);
    assert_eq!(out, expected);
    assert_eq!(format(&out, Some(compact_config()), None), expected);
}

#[test]
fn compact_rule_adjacent_to_text_in_blockquote_is_idempotent() {
    // Blockquotes don't get a blank line after the rule; the output must
    // still be a fixed point (no YAML/setext reinterpretation inside).
    let input = "> a\n>\n> ***\n> key: value\n";
    let out = format(input, Some(compact_config()), None);
    assert!(out.contains("> ---\n"), "compact rule missing: {out:?}");
    assert_eq!(format(&out, Some(compact_config()), None), out);
}

#[test]
fn compact_rule_interrupting_paragraph_gains_blank_lines_under_gfm() {
    // Under GFM a thematic break interrupts a paragraph, and a `---` directly
    // under text would be a setext underline; blank-line separation must
    // prevent both readings on the second pass.
    let input = "a\n***\nkey: value\n";
    let expected = "a\n\n---\n\nkey: value\n";
    let out = format(input, Some(compact_gfm_config()), None);
    assert_eq!(out, expected);
    assert_eq!(format(&out, Some(compact_gfm_config()), None), expected);
}

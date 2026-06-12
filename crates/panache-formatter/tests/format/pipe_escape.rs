use panache_formatter::Config;
use panache_formatter::config::{Extensions, Flavor};
use panache_formatter::format;

fn config_for(flavor: Flavor) -> Config {
    Config {
        flavor,
        parser_extensions: Extensions::for_flavor(flavor),
        ..Default::default()
    }
}

// Under CommonMark there are no pipe tables, so a literal `|` carries no
// special meaning and must not be escaped — matching pandoc's commonmark
// writer (`-t commonmark`). Regression for issue #367: table-shaped text
// reflowed into a paragraph was emitting `\|`.
#[test]
fn commonmark_does_not_escape_pipes() {
    let input = "| Title 1 | Title 2 |\n|-----------|-----------|\n| content 1 | content 2 |\n";
    let output = format(input, Some(config_for(Flavor::CommonMark)), None);
    assert!(
        !output.contains(r"\|"),
        "pipes must not be escaped under commonmark: {output:?}"
    );
    assert!(output.contains("| Title 1 | Title 2 |"), "{output:?}");
}

#[test]
fn commonmark_stray_pipe_in_paragraph_is_unescaped() {
    let input = "Pipe a | b here\n";
    let output = format(input, Some(config_for(Flavor::CommonMark)), None);
    similar_asserts::assert_eq!(output, "Pipe a | b here\n");
}

// Guard: under a flavor with pipe tables enabled (pandoc default) a stray `|`
// in a plain paragraph is still escaped so it cannot round-trip into a table —
// matching pandoc's markdown writer (`-t markdown`).
#[test]
fn pandoc_escapes_stray_pipe_in_paragraph() {
    let input = "Pipe a | b here\n";
    let output = format(input, Some(config_for(Flavor::Pandoc)), None);
    similar_asserts::assert_eq!(output, "Pipe a \\| b here\n");
}

// `|` also opens line blocks and forms grid-table cell rows, so a flavor that
// enables either extension (even without pipe tables) must keep escaping a bare
// `|`, otherwise a reflowed leading pipe could reparse as a line block / table
// row and break idempotency.
#[test]
fn line_blocks_extension_keeps_pipe_escaped() {
    let mut config = config_for(Flavor::CommonMark);
    config.parser_extensions.line_blocks = true;
    let output = format("Pipe a | b here\n", Some(config), None);
    similar_asserts::assert_eq!(output, "Pipe a \\| b here\n");
}

#[test]
fn grid_tables_extension_keeps_pipe_escaped() {
    let mut config = config_for(Flavor::CommonMark);
    config.parser_extensions.grid_tables = true;
    let output = format("Pipe a | b here\n", Some(config), None);
    similar_asserts::assert_eq!(output, "Pipe a \\| b here\n");
}

//! Raw block parsing (LaTeX commands, etc.)
//!
//! This module handles block-level raw content, primarily LaTeX commands
//! that appear at the start of lines. Examples:
//! - `\DeclareMathOperator{\E}{E{}}`
//! - `\newcommand{\foo}{bar}`
//! - `\usepackage{amsmath}`
//!
//! Per Pandoc behavior:
//! - Consecutive LaTeX command lines are grouped into a single RAW_BLOCK
//! - Blank lines or non-LaTeX content terminate the block
//! - Only enabled when `raw_tex` extension is active

use crate::config::Config;
use crate::syntax::{SyntaxKind, SyntaxNode};
use rowan::GreenNodeBuilder;

/// Check if content could start a raw TeX block.
///
/// Requirements:
/// - `raw_tex` extension must be enabled
/// - Line must start with backslash followed by a letter
pub fn can_start_raw_block(content: &str, config: &Config) -> bool {
    // Must have raw_tex extension enabled
    if !config.extensions.raw_tex {
        return false;
    }

    // Check if we're at the start of a line with a LaTeX command
    is_latex_command_line(content)
}

/// Check if a line starts with a LaTeX command (backslash + letter).
fn is_latex_command_line(line: &str) -> bool {
    let trimmed = line.trim_start();

    if !trimmed.starts_with('\\') {
        return false;
    }

    // After backslash, must have at least one letter
    let after_backslash = &trimmed[1..];

    // Exclude display math delimiters \[ and \]
    if after_backslash.starts_with('[') || after_backslash.starts_with(']') {
        return false;
    }

    after_backslash
        .chars()
        .next()
        .map(|c| c.is_ascii_alphabetic())
        .unwrap_or(false)
}

/// Parse a raw TeX block from lines array.
///
/// Collects one or more consecutive lines of LaTeX commands into a single
/// TEX_BLOCK node, stopping at blank lines or non-LaTeX content.
///
/// Returns the number of lines consumed.
pub fn parse_raw_tex_block(
    builder: &mut GreenNodeBuilder<'static>,
    lines: &[&str],
    start_pos: usize,
    _blockquote_depth: usize,
) -> usize {
    log::debug!("Starting raw TeX block at line {}", start_pos);

    builder.start_node(SyntaxKind::TEX_BLOCK.into());

    let mut lines_consumed = 0;
    let mut first_line = true;

    for line in &lines[start_pos..] {
        // Stop at blank lines
        if line.trim().is_empty() {
            break;
        }

        // Stop if not a LaTeX command line
        if !is_latex_command_line(line) {
            break;
        }

        log::trace!("  Raw block line: {:?}", line);

        if !first_line {
            builder.token(SyntaxKind::NEWLINE.into(), "\n");
        }
        first_line = false;

        // Emit the line content (strip newline)
        let content = line.trim_end_matches(&['\r', '\n'][..]);
        builder.token(SyntaxKind::TEXT.into(), content);

        lines_consumed += 1;
    }

    // Emit final newline if there were any lines
    if lines_consumed > 0 && !lines[start_pos + lines_consumed - 1].trim_end().is_empty() {
        builder.token(SyntaxKind::NEWLINE.into(), "\n");
    }

    builder.finish_node(); // TEX_BLOCK

    log::debug!("Finished raw TeX block, consumed {} lines", lines_consumed);
    lines_consumed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn test_is_latex_command_line() {
        assert!(is_latex_command_line("\\newcommand{foo}{bar}"));
        assert!(is_latex_command_line("\\DeclareMathOperator{\\E}{E{}}"));
        assert!(is_latex_command_line("  \\section{Title}"));
        assert!(is_latex_command_line("\\usepackage{amsmath}"));

        assert!(!is_latex_command_line("Regular text"));
        assert!(!is_latex_command_line("\\123 numbers"));
        assert!(!is_latex_command_line("\\  space"));
        assert!(!is_latex_command_line(""));
    }

    #[test]
    fn test_can_start_raw_block() {
        let config = Config::default();
        assert!(can_start_raw_block("\\newcommand{foo}{bar}", &config));
        assert!(!can_start_raw_block("Regular text", &config));

        let mut config_disabled = Config::default();
        config_disabled.extensions.raw_tex = false;
        assert!(!can_start_raw_block(
            "\\newcommand{foo}{bar}",
            &config_disabled
        ));
    }

    #[test]
    fn test_parse_single_command() {
        let lines = vec!["\\DeclareMathOperator{\\E}{E{}}\n"];
        let mut builder = GreenNodeBuilder::new();

        let consumed = parse_raw_tex_block(&mut builder, &lines, 0, 0);
        assert_eq!(consumed, 1);

        let green = builder.finish();
        let node = SyntaxNode::new_root(green);
        // The node's text should be the lossless input
        let text = node.text().to_string();
        assert!(
            text.contains("DeclareMathOperator"),
            "Should contain command text: {}",
            text
        );
    }

    #[test]
    fn test_parse_multiple_commands() {
        let lines = vec![
            "\\newcommand{\\foo}{bar}\n",
            "\\DeclareMathOperator{\\E}{E{}}\n",
        ];
        let mut builder = GreenNodeBuilder::new();

        let consumed = parse_raw_tex_block(&mut builder, &lines, 0, 0);
        assert_eq!(consumed, 2);

        let green = builder.finish();
        let node = SyntaxNode::new_root(green);
        let text = node.text().to_string();
        assert!(
            text.contains("newcommand"),
            "Should contain newcommand: {}",
            text
        );
        assert!(
            text.contains("DeclareMathOperator"),
            "Should contain DeclareMathOperator: {}",
            text
        );
    }

    #[test]
    fn test_stops_at_blank_line() {
        let lines = vec!["\\newcommand{\\foo}{bar}\n", "\n", "Regular paragraph\n"];
        let mut builder = GreenNodeBuilder::new();

        let consumed = parse_raw_tex_block(&mut builder, &lines, 0, 0);
        assert_eq!(consumed, 1);

        let green = builder.finish();
        let node = SyntaxNode::new_root(green);
        let text = node.text().to_string();
        assert!(text.contains("newcommand"));
        assert!(!text.contains("Regular paragraph"));
    }

    #[test]
    fn test_stops_at_non_latex() {
        let lines = vec!["\\newcommand{\\foo}{bar}\n", "Regular text\n"];
        let mut builder = GreenNodeBuilder::new();

        let consumed = parse_raw_tex_block(&mut builder, &lines, 0, 0);
        assert_eq!(consumed, 1);
    }
}

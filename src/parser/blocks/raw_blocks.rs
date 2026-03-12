//! Raw TeX block parsing (LaTeX commands and non-math environments)
//!
//! This module handles block-level raw TeX content:
//! 1. LaTeX commands: `\DeclareMathOperator`, `\newcommand`, etc.
//! 2. Non-math environments: `\begin{tabular}`, `\begin{figure}`, etc.
//!
//! Math environments (equation, align, etc.) are handled as INLINE content
//! in paragraphs, not as blocks. See INLINE_MATH_ENVIRONMENTS list below.
//!
//! Per Pandoc behavior:
//! - Consecutive LaTeX command lines are grouped into a single TEX_BLOCK
//! - Non-math environments become TEX_BLOCK
//! - Math environments are parsed inline (in paragraphs)
//! - Blank lines or non-LaTeX content terminate the block
//! - Only enabled when `raw_tex` extension is active

use crate::config::Config;
use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;

/// Inline math environments from Pandoc (parsed as RawInline in Para).
/// These should NOT be parsed as block-level environments.
///
/// Source: pandoc/src/Text/Pandoc/Readers/LaTeX/Math.hs:L97-L123
const INLINE_MATH_ENVIRONMENTS: &[&str] = &[
    "displaymath",
    "math",
    "equation",
    "equation*",
    "gather",
    "gather*",
    "multline",
    "multline*",
    "eqnarray",
    "eqnarray*",
    "align",
    "align*",
    "alignat",
    "alignat*",
    "flalign",
    "flalign*",
    "dmath",
    "dmath*",
    "dgroup",
    "dgroup*",
    "darray",
    "darray*",
    "subequations",
];

/// Check if an environment name is an inline math environment.
pub fn is_inline_math_environment(name: &str) -> bool {
    INLINE_MATH_ENVIRONMENTS.contains(&name)
}

/// Extract environment name from `\begin{name}` line.
/// Returns None if not a valid \begin{...} line.
pub fn extract_environment_name(line: &str) -> Option<String> {
    let trimmed = line.trim_start();

    if !trimmed.starts_with("\\begin{") {
        return None;
    }

    let after_begin = &trimmed[7..]; // Skip "\begin{"
    let close_brace = after_begin.find('}')?;
    let env_name = &after_begin[..close_brace];

    if env_name.is_empty() {
        return None;
    }

    Some(env_name.to_string())
}

/// Check if content could start a raw TeX block.
///
/// Requirements:
/// - `raw_tex` extension must be enabled
/// - Line must start with backslash followed by a letter
/// - If it's a `\begin{env}`, the environment must NOT be an inline math env
pub fn can_start_raw_block(content: &str, config: &Config) -> bool {
    // Must have raw_tex extension enabled
    if !config.extensions.raw_tex {
        return false;
    }

    // Check if it's a \begin{env} line
    if let Some(env_name) = extract_environment_name(content) {
        // Skip inline math environments - they should be parsed inline in paragraphs
        if is_inline_math_environment(&env_name) {
            return false;
        }
        // Non-math environment: parse as block
        return true;
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
    blockquote_depth: usize,
) -> usize {
    log::debug!("Starting raw TeX block at line {}", start_pos);

    builder.start_node(SyntaxKind::TEX_BLOCK.into());

    let first_line = lines[start_pos];
    let first_line_inner = crate::parser::blocks::blockquotes::strip_n_blockquote_markers(
        first_line,
        blockquote_depth,
    );
    if !is_latex_command_line(first_line_inner)
        && extract_environment_name(first_line_inner).is_none()
    {
        builder.finish_node();
        log::debug!("Finished raw TeX block, consumed 0 lines");
        return 0;
    }

    // Check if this is an environment
    let lines_consumed = if let Some(env_name) = extract_environment_name(first_line_inner) {
        // Parse environment: \begin{env}...content...\end{env}
        parse_tex_environment_lines(builder, lines, start_pos, &env_name, blockquote_depth)
    } else {
        // Parse consecutive LaTeX command lines
        parse_tex_command_lines(builder, lines, start_pos, blockquote_depth)
    };

    builder.finish_node(); // TEX_BLOCK

    log::debug!("Finished raw TeX block, consumed {} lines", lines_consumed);
    lines_consumed
}

/// Parse consecutive LaTeX command lines.
fn parse_tex_command_lines(
    builder: &mut GreenNodeBuilder<'static>,
    lines: &[&str],
    start_pos: usize,
    blockquote_depth: usize,
) -> usize {
    let mut lines_consumed = 0;
    let mut first_line = true;
    let mut brace_depth: i32 = 0;
    let mut started_braced_command = false;

    for line in &lines[start_pos..] {
        let inner =
            crate::parser::blocks::blockquotes::strip_n_blockquote_markers(line, blockquote_depth);
        if !first_line && brace_depth == 0 {
            // Stop at blank lines
            if inner.trim().is_empty() {
                break;
            }

            // Stop if not a LaTeX command line
            if !is_latex_command_line(inner) {
                break;
            }

            // Inside blockquotes, consume one command line at a time so outer parsing
            // can preserve each line's blockquote markers losslessly.
            if blockquote_depth > 0 {
                break;
            }
        }

        log::trace!("  Raw block line: {:?}", inner);

        if !first_line {
            builder.token(SyntaxKind::NEWLINE.into(), "\n");
        }
        first_line = false;

        // Emit the line content (strip newline)
        let content = inner.trim_end_matches(&['\r', '\n'][..]);
        builder.token(SyntaxKind::TEXT.into(), content);

        lines_consumed += 1;
        brace_depth += brace_delta(content);
        if brace_depth < 0 {
            brace_depth = 0;
        }
        if first_line && brace_depth > 0 {
            started_braced_command = true;
        }
        if started_braced_command && brace_depth == 0 {
            break;
        }
        first_line = false;
    }

    // Emit final newline if there were any lines
    if lines_consumed > 0 && !lines[start_pos + lines_consumed - 1].trim_end().is_empty() {
        builder.token(SyntaxKind::NEWLINE.into(), "\n");
    }

    lines_consumed
}

fn brace_delta(text: &str) -> i32 {
    let mut delta = 0i32;
    let mut backslashes = 0usize;

    for ch in text.chars() {
        if ch == '\\' {
            backslashes += 1;
            continue;
        }

        let escaped = backslashes % 2 == 1;
        backslashes = 0;

        if escaped {
            continue;
        }

        match ch {
            '{' => delta += 1,
            '}' => delta -= 1,
            _ => {}
        }
    }

    delta
}

/// Parse a LaTeX environment from \begin{env} to \end{env}.
fn parse_tex_environment_lines(
    builder: &mut GreenNodeBuilder<'static>,
    lines: &[&str],
    start_pos: usize,
    env_name: &str,
    blockquote_depth: usize,
) -> usize {
    let mut lines_consumed = 0;
    let mut first_line = true;
    let end_marker = format!("\\end{{{}}}", env_name);

    for line in &lines[start_pos..] {
        let inner =
            crate::parser::blocks::blockquotes::strip_n_blockquote_markers(line, blockquote_depth);
        log::trace!("  Environment line: {:?}", inner);

        if !first_line {
            builder.token(SyntaxKind::NEWLINE.into(), "\n");
        }
        first_line = false;

        // Emit the line content (strip newline)
        let content = inner.trim_end_matches(&['\r', '\n'][..]);
        builder.token(SyntaxKind::TEXT.into(), content);

        lines_consumed += 1;

        // Check if this line contains the end marker
        if inner.trim_start().starts_with(&end_marker) {
            break;
        }
    }

    // Emit final newline
    if lines_consumed > 0 {
        builder.token(SyntaxKind::NEWLINE.into(), "\n");
    }

    lines_consumed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::syntax::SyntaxNode;

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

    #[test]
    fn test_blockquote_line_does_not_loop() {
        let lines = vec!["> \\medskip\n"];
        let mut builder = GreenNodeBuilder::new();

        let consumed = parse_raw_tex_block(&mut builder, &lines, 0, 0);
        assert_eq!(consumed, 0);
    }

    #[test]
    fn test_blockquote_line_parses_tex_command() {
        let lines = vec!["> \\medskip\n"];
        let mut builder = GreenNodeBuilder::new();

        let consumed = parse_raw_tex_block(&mut builder, &lines, 0, 1);
        assert_eq!(consumed, 1);
    }

    #[test]
    fn test_blockquote_multiple_tex_commands_consumes_one_line() {
        let lines = vec!["> \\medskip\n", "> \\hfill---Joe Armstrong\n"];
        let mut builder = GreenNodeBuilder::new();

        let consumed = parse_raw_tex_block(&mut builder, &lines, 0, 1);
        assert_eq!(consumed, 1);
    }

    #[test]
    fn test_parse_braced_command_block_until_closing_brace() {
        let lines = vec!["\\pdfpcnote{\n", "  - blabla\n", "}\n"];
        let mut builder = GreenNodeBuilder::new();

        let consumed = parse_raw_tex_block(&mut builder, &lines, 0, 0);
        assert_eq!(consumed, 3);

        let green = builder.finish();
        let node = SyntaxNode::new_root(green);
        assert_eq!(node.text().to_string(), "\\pdfpcnote{\n  - blabla\n}\n");
    }
}

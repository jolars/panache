//! LaTeX command and environment parsing.
//!
//! Supports the `raw_tex` extension which preserves LaTeX commands and environments.
//!
//! Inline LaTeX commands: \cite{ref}, \textbf{text}, etc.
//! Block LaTeX environments: \begin{tabular}...\end{tabular}

use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;

/// Try to parse an inline LaTeX command starting at the given position.
/// Returns the number of **bytes** consumed if successful, or None.
///
/// LaTeX command pattern: \commandname[optional]{required}
/// - Starts with backslash
/// - Command name: letters only (a-zA-Z)
/// - Optional arguments in square brackets: [...]
/// - Required arguments in curly braces: {...}
pub(crate) fn try_parse_latex_command(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();

    if bytes.is_empty() || bytes[0] != b'\\' {
        return None;
    }

    // Check for escaped backslash (\\) - this is a hard line break, not LaTeX
    if bytes.len() > 1 && bytes[1] == b'\\' {
        return None;
    }

    let mut pos = 1; // Skip initial backslash

    // Parse command name (letters only)
    let command_start = pos;
    while pos < bytes.len() && bytes[pos].is_ascii_alphabetic() {
        pos += 1;
    }

    // Must have at least one letter in command name
    if pos == command_start {
        return None;
    }

    // Parse optional and required arguments
    while pos < bytes.len() {
        match bytes[pos] {
            b'[' => {
                // Optional argument in square brackets
                pos = skip_bracketed_arg(text, pos)?;
            }
            b'{' => {
                // Required argument in curly braces
                pos = skip_braced_arg(text, pos)?;
            }
            _ => {
                // End of command
                break;
            }
        }
    }

    // Must have consumed at least the backslash and command name
    if pos > 1 { Some(pos) } else { None }
}

/// Skip a bracketed argument [...], handling nested brackets.
/// Returns the position after the closing bracket, or None if invalid.
fn skip_bracketed_arg(text: &str, start: usize) -> Option<usize> {
    let bytes = text.as_bytes();

    if bytes.get(start)? != &b'[' {
        return None;
    }

    let mut pos = start + 1;
    let mut depth = 1;

    while pos < bytes.len() && depth > 0 {
        match bytes[pos] {
            b'[' => depth += 1,
            b']' => depth -= 1,
            b'\\' if pos + 1 < bytes.len() => {
                // Skip escaped character (both the backslash and the next byte)
                pos += 2;
                continue;
            }
            _ => {}
        }
        pos += 1;
    }

    if depth == 0 { Some(pos) } else { None }
}

/// Skip a braced argument {...}, handling nested braces.
/// Returns the position after the closing brace, or None if invalid.
fn skip_braced_arg(text: &str, start: usize) -> Option<usize> {
    let bytes = text.as_bytes();

    if bytes.get(start)? != &b'{' {
        return None;
    }

    let mut pos = start + 1;
    let mut depth = 1;

    while pos < bytes.len() && depth > 0 {
        match bytes[pos] {
            b'{' => depth += 1,
            b'}' => depth -= 1,
            b'\\' if pos + 1 < bytes.len() => {
                // Skip escaped character (both the backslash and the next byte)
                pos += 2;
                continue;
            }
            _ => {}
        }
        pos += 1;
    }

    if depth == 0 { Some(pos) } else { None }
}

/// Parse a LaTeX command and add it to the builder.
pub(crate) fn parse_latex_command(builder: &mut GreenNodeBuilder, text: &str, len: usize) {
    builder.start_node(SyntaxKind::LATEX_COMMAND.into());
    builder.token(SyntaxKind::TEXT.into(), &text[..len]);
    builder.finish_node();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_command() {
        assert_eq!(try_parse_latex_command(r"\cite{ref}"), Some(10));
        assert_eq!(try_parse_latex_command(r"\textbf{bold}"), Some(13));
    }

    #[test]
    fn test_command_with_optional_arg() {
        assert_eq!(
            try_parse_latex_command(r"\includegraphics[width=5cm]{file.png}"),
            Some(37)
        );
    }

    #[test]
    fn test_multiple_arguments() {
        assert_eq!(try_parse_latex_command(r"\newcommand{\foo}{bar}"), Some(22));
    }

    #[test]
    fn test_nested_braces() {
        assert_eq!(
            try_parse_latex_command(r"\command{text with {nested} braces}"),
            Some(35)
        );
    }

    #[test]
    fn test_no_arguments() {
        // Command without arguments - valid
        assert_eq!(try_parse_latex_command(r"\LaTeX "), Some(6));
    }

    #[test]
    fn test_escaped_backslash() {
        // \\ is a line break, not LaTeX
        assert_eq!(try_parse_latex_command(r"\\"), None);
    }

    #[test]
    fn test_not_latex() {
        assert_eq!(try_parse_latex_command(r"\123"), None); // Numbers not allowed
        assert_eq!(try_parse_latex_command(r"\ "), None); // No command name
        assert_eq!(try_parse_latex_command("no backslash"), None);
    }

    #[test]
    fn test_unclosed_braces() {
        assert_eq!(try_parse_latex_command(r"\cite{ref"), None);
    }

    #[test]
    fn test_unclosed_brackets() {
        assert_eq!(try_parse_latex_command(r"\command[opt"), None);
    }
}

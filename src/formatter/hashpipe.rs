//! Hashpipe-style YAML option formatting for Quarto executable chunks.
//!
//! Converts inline chunk options to Quarto's new hashpipe format with proper
//! line wrapping and language-specific comment prefixes.

use crate::parser::block_parser::chunk_options::ChunkOptionValue;
use crate::syntax::{AstNode, ChunkLabel, ChunkOption, SyntaxKind, SyntaxNode};

/// A chunk option with a classified value (simple or expression).
type ClassifiedOption = (String, ChunkOptionValue);

/// An option extracted from CST: (key, value, is_quoted).
/// For labels, key is None.
type CstOption = (Option<String>, Option<String>, bool);

/// Value types that can appear in chunk options.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ValueType {
    Boolean,
    Numeric,
    String,           // Accepts quoted strings and simple barewords
    QuotedStringOnly, // Only accepts quoted strings, not barewords
}

/// Allowlist of chunk options safe for hashpipe conversion.
///
/// Each entry maps an option name to the value types it accepts in hashpipe format.
/// Options not in this list will stay inline to avoid unknown type restrictions.
///
/// Based on knitr documentation: https://yihui.org/knitr/options/
const HASHPIPE_SAFE_OPTIONS: &[(&str, &[ValueType])] = &[
    // Chunk identification
    ("label", &[ValueType::QuotedStringOnly]), // Chunk label/name - only quoted to be safe
    // Code evaluation
    ("eval", &[ValueType::Boolean]), // Can also be numeric vector, but keep those inline
    // Text output
    ("echo", &[ValueType::Boolean]), // Can also be numeric vector, but keep those inline
    ("results", &[ValueType::String]), // "markup", "asis", "hold", "hide"
    ("collapse", &[ValueType::Boolean]),
    ("warning", &[ValueType::Boolean]), // Can also be NA or numeric, but keep those inline
    ("message", &[ValueType::Boolean]),
    ("error", &[ValueType::Boolean]), // Can also be numeric 0/1/2, but keep those inline
    ("include", &[ValueType::Boolean]),
    ("strip-white", &[ValueType::Boolean]),
    // Code decoration
    ("comment", &[ValueType::String]),
    ("highlight", &[ValueType::Boolean]),
    ("prompt", &[ValueType::Boolean]),
    ("size", &[ValueType::String]),       // LaTeX font sizes
    ("background", &[ValueType::String]), // Color values
    // Cache
    ("cache", &[ValueType::Boolean]), // Can also be path, but keep those inline
    ("cache-path", &[ValueType::String]),
    ("cache-lazy", &[ValueType::Boolean]),
    ("cache-comments", &[ValueType::Boolean]),
    ("cache-rebuild", &[ValueType::Boolean]),
    ("autodep", &[ValueType::Boolean]),
    // Plots
    ("fig-path", &[ValueType::String]),
    ("fig-keep", &[ValueType::String]), // "high", "none", "all", "first", "last"
    ("fig-show", &[ValueType::String]), // "asis", "hold", "animate", "hide"
    ("dev", &[ValueType::String]),      // "png", "pdf", "svg", etc.
    // Figure dimensions and layout
    ("fig-width", &[ValueType::Numeric]),
    ("fig-height", &[ValueType::Numeric]),
    ("fig-asp", &[ValueType::Numeric]),  // Aspect ratio
    ("fig-dim", &[ValueType::Numeric]),  // Can be vector, but single numeric values work
    ("out-width", &[ValueType::String]), // "50%", "300px", etc.
    ("out-height", &[ValueType::String]),
    ("fig-align", &[ValueType::String]), // "left", "center", "right", "default"
    ("fig-env", &[ValueType::String]),
    ("fig-pos", &[ValueType::String]),
    ("fig-scap", &[ValueType::String]),
    // Figure captions and alt text
    ("fig-cap", &[ValueType::String]),
    ("fig-alt", &[ValueType::String]),
    ("fig-subcap", &[ValueType::String]),
    // Plot parameters
    ("dpi", &[ValueType::Numeric]),
    // Animation
    ("aniopts", &[ValueType::String]),
    ("ffmpeg-format", &[ValueType::String]),
    // Quarto-specific code display
    ("code-fold", &[ValueType::Boolean, ValueType::String]), // true/false or "show"/"hide"
    ("code-summary", &[ValueType::String]),
    ("code-overflow", &[ValueType::String]), // "wrap" or "scroll"
    ("code-line-numbers", &[ValueType::Boolean]),
    // Output classes/attributes
    ("classes", &[ValueType::String]),
];

/// Mapping of common chunk option names from R dot-notation to YAML dash-notation.
///
/// Explicit overrides for common options. If not in this table, the default rule
/// is to replace dots with dashes (e.g., `fig.width` → `fig-width`).
const OPTION_NAME_OVERRIDES: &[(&str, &str)] = &[
    ("fig.cap", "fig-cap"),
    ("fig.alt", "fig-alt"),
    ("fig.width", "fig-width"),
    ("fig.height", "fig-height"),
    ("fig.align", "fig-align"),
    ("fig.pos", "fig-pos"),
    ("fig.env", "fig-env"),
    ("fig.scap", "fig-scap"),
    ("fig.lp", "fig-lp"),
    ("fig.subcap", "fig-subcap"),
    ("fig.ncol", "fig-ncol"),
    ("fig.sep", "fig-sep"),
    ("fig.process", "fig-process"),
    ("fig.show", "fig-show"),
    ("fig.keep", "fig-keep"),
    ("out.width", "out-width"),
    ("out.height", "out-height"),
    ("out.extra", "out-extra"),
];

/// Get the comment prefix for hashpipe options based on the chunk language.
///
/// Returns None for unknown languages to avoid using incorrect comment syntax.
///
/// Different languages use different comment syntax:
/// - R, Python, Julia, Bash, Ruby, Perl: `#|`
/// - C, C++, Java, JavaScript, Rust, Go, etc.: `//|`
/// - SQL dialects: `--|`
pub fn get_comment_prefix(language: &str) -> Option<&'static str> {
    match language.to_lowercase().as_str() {
        "r" | "python" | "julia" | "bash" | "shell" | "sh" | "ruby" | "perl" => Some("#|"),

        "c" | "cpp" | "c++" | "java" | "javascript" | "js" | "typescript" | "ts" | "rust"
        | "go" | "swift" | "kotlin" | "scala" | "csharp" | "c#" | "php" | "ojs" | "dot" => {
            Some("//|")
        }

        "sql" | "mysql" | "postgres" | "postgresql" | "sqlite" => Some("--|"),

        "mermaid" => Some("%%|"),

        // Unknown language - don't convert to hashpipe
        _ => None,
    }
}

/// Normalize a chunk option name from R dot-notation to YAML dash-notation.
///
/// First checks the override table, then falls back to replacing dots with dashes.
pub fn normalize_option_name(name: &str) -> String {
    // Check override table first
    for (old, new) in OPTION_NAME_OVERRIDES {
        if name == *old {
            return (*new).to_string();
        }
    }

    // Default: replace dots with dashes
    name.replace('.', "-")
}

/// Normalize a chunk option value for YAML syntax.
///
/// Converts R boolean literals to lowercase YAML booleans.
/// For quoted strings, preserves the quotes.
pub fn normalize_value(value: &str) -> String {
    match value {
        "TRUE" | "T" => "true".to_string(),
        "FALSE" | "F" => "false".to_string(),
        _ => value.to_string(),
    }
}

/// Extract chunk options from CST and classify into hashpipe-safe vs inline-only.
///
/// Returns (simple_options, complex_options) where simple_options can be converted
/// to hashpipe format and complex_options must stay inline.
pub fn split_options_from_cst(info_node: &SyntaxNode) -> (Vec<ClassifiedOption>, Vec<CstOption>) {
    let mut simple = Vec::new();
    let mut complex = Vec::new();

    // Find CHUNK_OPTIONS node
    for child in info_node.children() {
        if child.kind() == SyntaxKind::CHUNK_OPTIONS {
            // Iterate through options and labels
            for opt_or_label in child.children() {
                if let Some(label) = ChunkLabel::cast(opt_or_label.clone()) {
                    // Label converts to #| label: value
                    simple.push(("label".to_string(), ChunkOptionValue::Simple(label.text())));
                } else if let Some(opt) = ChunkOption::cast(opt_or_label) {
                    // Regular option with key=value
                    if let (Some(key), Some(value)) = (opt.key(), opt.value()) {
                        let is_quoted = opt.is_quoted();
                        let normalized_key = normalize_option_name(&key);

                        // Classify the option
                        if let Some(classified_value) =
                            classify_option_for_hashpipe(&normalized_key, &value, is_quoted)
                        {
                            simple.push((normalized_key, classified_value));
                        } else {
                            // Keep inline with original key
                            complex.push((Some(key), Some(value), is_quoted));
                        }
                    }
                }
            }
            break;
        }
    }

    (simple, complex)
}

/// Classify an option value for hashpipe conversion.
///
/// Returns Some(ClassifiedValue) if the option is safe for hashpipe, None otherwise.
fn classify_option_for_hashpipe(
    key: &str,
    value: &str,
    is_quoted: bool,
) -> Option<ChunkOptionValue> {
    use crate::parser::block_parser::chunk_options::{is_boolean_literal, is_numeric_literal};

    // Find allowed value types for this option
    let allowed_types = HASHPIPE_SAFE_OPTIONS
        .iter()
        .find(|(name, _)| *name == key)
        .map(|(_, types)| *types)?;

    // Check if value type matches allowed types
    if is_quoted {
        // Quoted string - safe if String or QuotedStringOnly is allowed
        if allowed_types.contains(&ValueType::String)
            || allowed_types.contains(&ValueType::QuotedStringOnly)
        {
            return Some(ChunkOptionValue::Simple(format!("\"{}\"", value)));
        }
    } else {
        // Unquoted - check boolean, numeric, or simple bareword
        if is_boolean_literal(value) && allowed_types.contains(&ValueType::Boolean) {
            return Some(ChunkOptionValue::Simple(value.to_lowercase()));
        }
        if is_numeric_literal(value) && allowed_types.contains(&ValueType::Numeric) {
            return Some(ChunkOptionValue::Simple(value.to_string()));
        }
        // Unquoted string (bareword) - only if String is allowed (not QuotedStringOnly)
        if allowed_types.contains(&ValueType::String) && is_simple_bareword(value) {
            return Some(ChunkOptionValue::Simple(value.to_string()));
        }
    }

    // Doesn't match allowed types or is complex expression - keep inline
    None
}

/// Check if a value is a simple bareword (identifier), not an expression.
///
/// Always returns false - we don't convert barewords to be safe.
/// Inline format requires quotes for string values (e.g., results="asis" not results=asis).
fn is_simple_bareword(_s: &str) -> bool {
    false
}

/// Format a single hashpipe option line with wrapping support.
///
/// If the option line exceeds `line_width`, wraps at word boundaries with
/// proper continuation indentation (2 spaces after the comment prefix).
pub fn format_hashpipe_option_with_wrap(
    prefix: &str,
    key: &str,
    value: &str,
    line_width: usize,
) -> Vec<String> {
    let first_line = format!("{} {}: {}", prefix, key, value);

    // Check if wrapping is needed
    if first_line.len() <= line_width {
        return vec![first_line];
    }

    // Calculate available space
    let first_prefix = format!("{} {}: ", prefix, key);
    let available_first = line_width.saturating_sub(first_prefix.len());

    // If even the prefix is too long, return as-is (don't break mid-word)
    if available_first < 10 {
        return vec![first_line];
    }

    let continuation_prefix = format!("{}  ", prefix); // 2 spaces after prefix
    let available_continuation = line_width.saturating_sub(continuation_prefix.len());

    let mut lines = Vec::new();
    let mut remaining = value;
    let mut is_first = true;

    while !remaining.is_empty() {
        let available = if is_first {
            available_first
        } else {
            available_continuation
        };

        // Find word boundary at or before available length
        let break_point = if remaining.len() <= available {
            remaining.len()
        } else {
            // Find last space before or at available
            remaining[..=available]
                .rfind(' ')
                .map(|i| i + 1) // Include the space
                .unwrap_or(available) // No space found, break at available
        };

        let chunk = &remaining[..break_point].trim_end();
        if is_first {
            lines.push(format!("{}{}", first_prefix, chunk));
            is_first = false;
        } else {
            lines.push(format!("{}{}", continuation_prefix, chunk));
        }

        remaining = remaining[break_point..].trim_start();
    }

    lines
}

/// Format chunk options as hashpipe lines.
///
/// Only formats simple values; complex expressions are filtered out.
/// Returns None if the language's comment syntax is unknown.
/// Returns Some(Vec<String>) with formatted hashpipe lines for known languages.
pub fn format_as_hashpipe(
    language: &str,
    options: &[ClassifiedOption],
    line_width: usize,
) -> Option<Vec<String>> {
    let prefix = get_comment_prefix(language)?; // Return None if unknown language
    let mut output = Vec::new();

    for (key, value) in options {
        // Only format simple values
        if let ChunkOptionValue::Simple(v) = value {
            let norm_key = normalize_option_name(key);
            let norm_val = normalize_value(v);

            // Handle bare options (no value)
            let value_str = if norm_val.is_empty() {
                "true".to_string() // Bare option means true
            } else {
                norm_val
            };

            let lines = format_hashpipe_option_with_wrap(prefix, &norm_key, &value_str, line_width);
            output.extend(lines);
        }
    }

    Some(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::block_parser::chunk_options::ChunkOptionValue;

    #[test]
    fn test_get_comment_prefix_r() {
        assert_eq!(get_comment_prefix("r"), Some("#|"));
        assert_eq!(get_comment_prefix("R"), Some("#|"));
    }

    #[test]
    fn test_get_comment_prefix_python() {
        assert_eq!(get_comment_prefix("python"), Some("#|"));
        assert_eq!(get_comment_prefix("Python"), Some("#|"));
    }

    #[test]
    fn test_get_comment_prefix_cpp() {
        assert_eq!(get_comment_prefix("cpp"), Some("//|"));
        assert_eq!(get_comment_prefix("c++"), Some("//|"));
        assert_eq!(get_comment_prefix("C++"), Some("//|"));
    }

    #[test]
    fn test_get_comment_prefix_sql() {
        assert_eq!(get_comment_prefix("sql"), Some("--|"));
        assert_eq!(get_comment_prefix("SQL"), Some("--|"));
    }

    #[test]
    fn test_get_comment_prefix_unknown() {
        assert_eq!(get_comment_prefix("unknown"), None);
        assert_eq!(get_comment_prefix("fortran"), None);
        assert_eq!(get_comment_prefix("matlab"), None);
    }

    #[test]
    fn test_normalize_option_name_override() {
        assert_eq!(normalize_option_name("fig.cap"), "fig-cap");
        assert_eq!(normalize_option_name("fig.width"), "fig-width");
    }

    #[test]
    fn test_normalize_option_name_default() {
        assert_eq!(normalize_option_name("my.option"), "my-option");
        assert_eq!(normalize_option_name("some.long.name"), "some-long-name");
    }

    #[test]
    fn test_normalize_option_name_no_dots() {
        assert_eq!(normalize_option_name("echo"), "echo");
        assert_eq!(normalize_option_name("warning"), "warning");
    }

    #[test]
    fn test_normalize_value_booleans() {
        assert_eq!(normalize_value("TRUE"), "true");
        assert_eq!(normalize_value("FALSE"), "false");
        assert_eq!(normalize_value("T"), "true");
        assert_eq!(normalize_value("F"), "false");
    }

    #[test]
    fn test_normalize_value_other() {
        assert_eq!(normalize_value("7"), "7");
        assert_eq!(normalize_value("\"hello\""), "\"hello\"");
        assert_eq!(normalize_value("3.14"), "3.14");
    }

    #[test]
    fn test_format_hashpipe_option_short() {
        let lines = format_hashpipe_option_with_wrap("#|", "echo", "true", 80);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0], "#| echo: true");
    }

    #[test]
    fn test_format_hashpipe_option_wrap() {
        let long_caption =
            "This is a very long caption that definitely exceeds the line width and needs to wrap";
        let lines = format_hashpipe_option_with_wrap("#|", "fig-cap", long_caption, 80);

        assert!(lines.len() > 1, "Should wrap into multiple lines");
        assert!(lines[0].starts_with("#| fig-cap:"));
        assert!(lines[1].starts_with("#|  ")); // 2-space indent
        assert!(lines[0].len() <= 80);
        // Continuation lines might be slightly over due to word boundaries
    }

    #[test]
    fn test_format_as_hashpipe_simple() {
        let options = vec![
            (
                "echo".to_string(),
                ChunkOptionValue::Simple("TRUE".to_string()),
            ),
            (
                "fig.width".to_string(),
                ChunkOptionValue::Simple("7".to_string()),
            ),
        ];

        let lines = format_as_hashpipe("r", &options, 80).unwrap();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "#| echo: true");
        assert_eq!(lines[1], "#| fig-width: 7");
    }

    #[test]
    fn test_format_as_hashpipe_skips_expressions() {
        let options = vec![
            (
                "echo".to_string(),
                ChunkOptionValue::Simple("TRUE".to_string()),
            ),
            (
                "label".to_string(),
                ChunkOptionValue::Expression("my_var".to_string()),
            ),
        ];

        let lines = format_as_hashpipe("r", &options, 80).unwrap();
        assert_eq!(lines.len(), 1); // Only echo, label skipped
        assert_eq!(lines[0], "#| echo: true");
    }

    #[test]
    fn test_format_as_hashpipe_unknown_language() {
        let options = vec![(
            "echo".to_string(),
            ChunkOptionValue::Simple("TRUE".to_string()),
        )];

        // Unknown language should return None
        let result = format_as_hashpipe("fortran", &options, 80);
        assert!(result.is_none());
    }
}

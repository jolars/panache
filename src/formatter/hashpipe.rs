//! Hashpipe-style YAML option formatting for Quarto executable chunks.
//!
//! Converts inline chunk options to Quarto's new hashpipe format with proper
//! line wrapping and language-specific comment prefixes.

use crate::parser::block_parser::chunk_options::ChunkOptionValue;

/// A chunk option with a classified value (simple or expression).
type ClassifiedOption = (String, ChunkOptionValue);

/// A raw chunk option from the parser (key with optional string value).
type RawOption = (String, Option<String>);

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
/// Different languages use different comment syntax:
/// - R, Python, Julia, Bash: `#|`
/// - C, C++, Java, JavaScript, Rust, Go: `//|`
/// - SQL: `--|`
pub fn get_comment_prefix(language: &str) -> &'static str {
    match language.to_lowercase().as_str() {
        "r" | "python" | "julia" | "bash" | "shell" | "sh" => "#|",
        "c" | "cpp" | "c++" | "java" | "javascript" | "js" | "typescript" | "ts" | "rust"
        | "go" | "swift" | "kotlin" => "//|",
        "sql" | "mysql" | "postgres" | "postgresql" | "sqlite" => "--|",
        _ => "#|", // Default to #| for unknown languages
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
/// Adds quotes around string values that need them (contain spaces, special chars).
pub fn normalize_value(value: &str) -> String {
    match value {
        "TRUE" | "T" => "true".to_string(),
        "FALSE" | "F" => "false".to_string(),
        _ => {
            // Check if value is numeric (doesn't need quotes)
            use crate::parser::block_parser::chunk_options::is_numeric_literal;
            if is_numeric_literal(value) {
                value.to_string()
            } else if needs_yaml_quotes(value) {
                // Add quotes for strings with spaces or special chars
                format!("\"{}\"", value.replace('\"', "\\\""))
            } else {
                value.to_string()
            }
        }
    }
}

/// Check if a value needs quotes in YAML.
///
/// Returns true for values with spaces or special YAML characters.
fn needs_yaml_quotes(s: &str) -> bool {
    s.is_empty()
        || s.contains(' ')
        || s.contains(':')
        || s.contains('#')
        || s.contains('[')
        || s.contains(']')
        || s.contains('{')
        || s.contains('}')
        || s.contains(',')
        || s.starts_with('-')
        || s.starts_with('>')
        || s.starts_with('|')
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
/// Returns a vector of formatted hashpipe lines.
pub fn format_as_hashpipe(
    language: &str,
    options: &[ClassifiedOption],
    line_width: usize,
) -> Vec<String> {
    let prefix = get_comment_prefix(language);
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

    output
}

/// Classify options and split into simple (convertible) and complex (inline-only).
///
/// Returns (simple_options, complex_options) where simple options can be
/// converted to hashpipe and complex options must stay inline.
///
/// Special case: `label` option is always treated as simple, even for barewords,
/// because labels are identifiers, not R expressions.
pub fn split_options(options: &[RawOption]) -> (Vec<ClassifiedOption>, Vec<RawOption>) {
    use crate::parser::block_parser::chunk_options::classify_value;

    let mut simple = Vec::new();
    let mut complex = Vec::new();

    for (key, value) in options {
        // Special case: label is always simple (it's an identifier, not an expression)
        if key == "label" {
            if let Some(v) = value {
                simple.push((key.clone(), ChunkOptionValue::Simple(v.clone())));
            } else {
                simple.push((key.clone(), ChunkOptionValue::Simple(String::new())));
            }
            continue;
        }

        let classified = classify_value(value);
        log::debug!("Classifying {key}={value:?} -> {classified:?}");
        match classified {
            ChunkOptionValue::Simple(_) => {
                simple.push((key.clone(), classified));
            }
            ChunkOptionValue::Expression(_) => {
                complex.push((key.clone(), value.clone()));
            }
        }
    }

    log::debug!("Split: {} simple, {} complex", simple.len(), complex.len());
    (simple, complex)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::block_parser::chunk_options::ChunkOptionValue;

    #[test]
    fn test_get_comment_prefix_r() {
        assert_eq!(get_comment_prefix("r"), "#|");
        assert_eq!(get_comment_prefix("R"), "#|");
    }

    #[test]
    fn test_get_comment_prefix_python() {
        assert_eq!(get_comment_prefix("python"), "#|");
        assert_eq!(get_comment_prefix("Python"), "#|");
    }

    #[test]
    fn test_get_comment_prefix_cpp() {
        assert_eq!(get_comment_prefix("cpp"), "//|");
        assert_eq!(get_comment_prefix("c++"), "//|");
        assert_eq!(get_comment_prefix("C++"), "//|");
    }

    #[test]
    fn test_get_comment_prefix_sql() {
        assert_eq!(get_comment_prefix("sql"), "--|");
        assert_eq!(get_comment_prefix("SQL"), "--|");
    }

    #[test]
    fn test_get_comment_prefix_unknown() {
        assert_eq!(get_comment_prefix("unknown"), "#|");
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

        let lines = format_as_hashpipe("r", &options, 80);
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

        let lines = format_as_hashpipe("r", &options, 80);
        assert_eq!(lines.len(), 1); // Only echo, label skipped
        assert_eq!(lines[0], "#| echo: true");
    }

    #[test]
    fn test_split_options() {
        let options = vec![
            ("echo".to_string(), Some("TRUE".to_string())),
            ("fig.width".to_string(), Some("7".to_string())),
            ("label".to_string(), Some("my_var".to_string())), // label is always simple
        ];

        let (simple, complex) = split_options(&options);

        // All three should be simple now (label gets special treatment)
        assert_eq!(simple.len(), 3);
        assert_eq!(complex.len(), 0);
    }
}

#[test]
fn test_split_with_quoted_string_containing_spaces() {
    let options = vec![
        ("label".to_string(), Some("\"my chunk\"".to_string())),
        ("echo".to_string(), Some("FALSE".to_string())),
    ];

    let (simple, complex) = split_options(&options);

    // Both should be simple
    assert_eq!(simple.len(), 2, "Both options should be simple");
    assert_eq!(complex.len(), 0, "No complex options");
}

//! Hashpipe-style YAML option formatting for Quarto executable chunks.
//!
//! Converts inline chunk options to Quarto's new hashpipe format with proper
//! line wrapping and language-specific comment prefixes.

use crate::config::WrapMode;
use crate::parser::utils::chunk_options::ChunkOptionValue;
use crate::parser::utils::chunk_options::hashpipe_comment_prefix;
use crate::syntax::{AstNode, ChunkInfoItem, CodeInfo, SyntaxNode};
use crate::yaml_engine;

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
    hashpipe_comment_prefix(language)
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

/// Extract chunk options from inline info-string and optional parsed leading hashpipe lines.
///
/// Returns ((simple_options, complex_options), had_leading_hashpipe_options).
/// When both sources provide the same normalized key, inline info-string options win.
pub fn split_options_from_cst_with_content(
    info_node: &SyntaxNode,
    content: &str,
    prefix: &str,
) -> ((Vec<ClassifiedOption>, Vec<CstOption>), bool) {
    #[derive(Clone)]
    enum Entry {
        Simple(ClassifiedOption),
        Complex(CstOption),
    }

    fn upsert(entries: &mut Vec<(String, Entry)>, normalized_key: String, entry: Entry) {
        if let Some(pos) = entries.iter().position(|(k, _)| *k == normalized_key) {
            entries[pos] = (normalized_key, entry);
        } else {
            entries.push((normalized_key, entry));
        }
    }

    fn insert_if_absent(entries: &mut Vec<(String, Entry)>, normalized_key: String, entry: Entry) {
        if entries.iter().any(|(k, _)| *k == normalized_key) {
            return;
        }
        entries.push((normalized_key, entry));
    }

    fn push_inline_option(
        entries: &mut Vec<(String, Entry)>,
        key: String,
        value: String,
        is_quoted: bool,
    ) {
        let normalized_key = normalize_option_name(&key);
        if let Some(classified_value) =
            classify_option_for_hashpipe(&normalized_key, &value, is_quoted)
        {
            upsert(
                entries,
                normalized_key.clone(),
                Entry::Simple((normalized_key, classified_value)),
            );
        } else {
            upsert(
                entries,
                normalized_key,
                Entry::Complex((Some(key), Some(value), is_quoted)),
            );
        }
    }

    fn push_content_option(
        entries: &mut Vec<(String, Entry)>,
        key: String,
        value: String,
        is_quoted: bool,
    ) {
        let normalized_key = normalize_option_name(&key);
        let rendered = if is_quoted {
            format!("\"{}\"", value)
        } else {
            value
        };
        insert_if_absent(
            entries,
            normalized_key.clone(),
            Entry::Simple((normalized_key, ChunkOptionValue::Simple(rendered))),
        );
    }

    let mut entries: Vec<(String, Entry)> = Vec::new();
    let mut had_content_hashpipe = false;
    let mut pending_label_parts: Vec<String> = Vec::new();

    // 1) Inline options from CODE_INFO CHUNK_OPTIONS (highest precedence)
    let Some(info) = CodeInfo::cast(info_node.clone()) else {
        return ((Vec::new(), Vec::new()), false);
    };

    for item in info.chunk_items() {
        match item {
            ChunkInfoItem::Label(label) => {
                let label_value = label.text();
                if !label_value.is_empty() {
                    pending_label_parts.push(label_value);
                }
            }
            ChunkInfoItem::Option(opt) => {
                if !pending_label_parts.is_empty() {
                    upsert(
                        &mut entries,
                        "label".to_string(),
                        Entry::Simple((
                            "label".to_string(),
                            ChunkOptionValue::Simple(pending_label_parts.join(" ")),
                        )),
                    );
                    pending_label_parts.clear();
                }
                if let (Some(key), Some(value)) = (opt.key(), opt.value()) {
                    push_inline_option(&mut entries, key, value, opt.is_quoted());
                }
            }
        }
    }

    if !pending_label_parts.is_empty() {
        upsert(
            &mut entries,
            "label".to_string(),
            Entry::Simple((
                "label".to_string(),
                ChunkOptionValue::Simple(pending_label_parts.join(" ")),
            )),
        );
    }

    // 2) Existing leading hashpipe options from CODE_CONTENT text (lower precedence).
    // Parse multiline quoted values so rewrapping can normalize them.
    for (key, value) in extract_leading_hashpipe_options(content, prefix) {
        had_content_hashpipe = true;
        push_content_option(&mut entries, key, value, false);
    }

    let mut simple = Vec::new();
    let mut complex = Vec::new();
    for (_, entry) in entries {
        match entry {
            Entry::Simple(s) => simple.push(s),
            Entry::Complex(c) => complex.push(c),
        }
    }

    ((simple, complex), had_content_hashpipe)
}

fn extract_leading_hashpipe_options(content: &str, prefix: &str) -> Vec<(String, String)> {
    let mut options = Vec::new();
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0usize;

    while i < lines.len() {
        let trimmed = lines[i].trim_start();
        if !trimmed.starts_with(prefix) {
            break;
        }
        let after_prefix = &trimmed[prefix.len()..];
        let rest = after_prefix.trim_start_matches([' ', '\t']);
        let Some(colon_idx) = rest.find(':') else {
            break;
        };
        let key = rest[..colon_idx].trim_end_matches([' ', '\t']);
        if key.is_empty() {
            break;
        }
        let value = rest[colon_idx + 1..]
            .trim_start_matches([' ', '\t'])
            .trim_end_matches([' ', '\t']);

        let mut merged_value = value.to_string();
        i += 1;

        if is_unclosed_double_quoted(&merged_value) {
            while i < lines.len() {
                let next_trimmed = lines[i].trim_start();
                if !next_trimmed.starts_with(prefix) {
                    break;
                }
                let next_after_prefix = &next_trimmed[prefix.len()..];
                if !next_after_prefix.starts_with([' ', '\t']) {
                    break;
                }
                let continuation = next_after_prefix.trim_start_matches([' ', '\t']);
                if continuation.is_empty() {
                    break;
                }
                if !merged_value.ends_with(' ') {
                    merged_value.push(' ');
                }
                merged_value.push_str(continuation);
                i += 1;
                if !is_unclosed_double_quoted(&merged_value) {
                    break;
                }
            }
        } else if is_yaml_block_scalar_indicator(&merged_value) {
            while i < lines.len() {
                let next_trimmed = lines[i].trim_start();
                if !next_trimmed.starts_with(prefix) {
                    break;
                }
                let next_after_prefix = &next_trimmed[prefix.len()..];
                if !is_block_scalar_continuation_line(next_after_prefix) {
                    break;
                }
                merged_value.push('\n');
                merged_value.push_str(next_after_prefix);
                i += 1;
            }
        } else if merged_value.is_empty() {
            while i < lines.len() {
                let next_trimmed = lines[i].trim_start();
                if !next_trimmed.starts_with(prefix) {
                    break;
                }
                let next_after_prefix = &next_trimmed[prefix.len()..];
                if !is_block_scalar_continuation_line(next_after_prefix) {
                    break;
                }
                merged_value.push('\n');
                merged_value.push_str(strip_single_yaml_prefix_space(next_after_prefix));
                i += 1;
            }
        }

        options.push((key.to_string(), merged_value));
    }

    options
}

fn strip_single_yaml_prefix_space(after_prefix: &str) -> &str {
    if let Some(rest) = after_prefix.strip_prefix(' ') {
        rest
    } else if let Some(rest) = after_prefix.strip_prefix('\t') {
        rest
    } else {
        after_prefix
    }
}

fn is_yaml_block_scalar_indicator(value: &str) -> bool {
    let s = value.trim();
    if s.is_empty() {
        return false;
    }
    let mut chars = s.chars();
    let Some(style) = chars.next() else {
        return false;
    };
    if style != '|' && style != '>' {
        return false;
    }
    chars.all(|ch| ch == '+' || ch == '-' || ch.is_ascii_digit())
}

fn leading_ws_count(text: &str) -> usize {
    text.chars().take_while(|c| matches!(c, ' ' | '\t')).count()
}

fn is_block_scalar_continuation_line(after_prefix: &str) -> bool {
    let text = after_prefix.trim_end_matches(['\n', '\r']);
    if text.trim().is_empty() {
        return true;
    }
    leading_ws_count(text) >= 2
}

fn is_unclosed_double_quoted(value: &str) -> bool {
    if !value.starts_with('"') {
        return false;
    }
    let mut escaped = false;
    let mut quote_count = 0usize;
    for ch in value.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == '"' {
            quote_count += 1;
        }
    }
    quote_count % 2 == 1
}

/// Classify an option value for hashpipe conversion.
///
/// Returns Some(ClassifiedValue) if the option is safe for hashpipe, None otherwise.
fn classify_option_for_hashpipe(
    key: &str,
    value: &str,
    is_quoted: bool,
) -> Option<ChunkOptionValue> {
    use crate::parser::utils::chunk_options::{is_boolean_literal, is_numeric_literal};

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
        if allowed_types.contains(&ValueType::Boolean)
            && (is_boolean_literal(value) || matches!(value, "true" | "false"))
        {
            return Some(ChunkOptionValue::Simple(value.to_ascii_lowercase()));
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
    if let Some((first, rest)) = value.split_once('\n')
        && is_yaml_block_scalar_indicator(first)
    {
        let mut lines = vec![format!("{} {}: {}", prefix, key, first)];
        lines.extend(rest.split('\n').map(|line| format!("{}{}", prefix, line)));
        return lines;
    }
    if let Some((first, rest)) = value.split_once('\n') {
        let mut lines = vec![if first.is_empty() {
            format!("{} {}:", prefix, key)
        } else {
            format!("{} {}: {}", prefix, key, first)
        }];
        lines.extend(rest.split('\n').map(|line| format!("{} {}", prefix, line)));
        return lines;
    }

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

    let continuation_prefix = format!("{}   ", prefix); // 3 spaces after prefix
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
    wrap: Option<&WrapMode>,
) -> Option<Vec<String>> {
    let prefix = get_comment_prefix(language)?; // Return None if unknown language
    let mut output = Vec::new();
    let mut yaml_entries: Vec<(String, String)> = Vec::new();

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

            yaml_entries.push((norm_key.clone(), value_str.clone()));
            let lines = format_hashpipe_option_with_wrap(prefix, &norm_key, &value_str, line_width);
            output.extend(lines);
        }
    }

    if !yaml_entries.is_empty() {
        let yaml_text = yaml_entries
            .iter()
            .map(|(key, value)| format!("{}: {}\n", key, value))
            .collect::<String>();
        // pretty_yaml wraps to the width of raw YAML text. Hashpipe output adds
        // a comment prefix (`#| `, `//| `, `--| `) before every rendered line,
        // so subtract that width to keep final emitted lines within line_width.
        let yaml_print_width = line_width.saturating_sub(prefix.len() + 1);
        let yaml_config = crate::config::Config {
            line_width: yaml_print_width,
            wrap: wrap.cloned(),
            ..Default::default()
        };
        if let Ok(formatted_yaml) = yaml_engine::format_yaml_with_config(&yaml_text, &yaml_config) {
            let lines = formatted_yaml
                .lines()
                .map(|line| {
                    if line.is_empty() {
                        prefix.to_string()
                    } else {
                        format!("{} {}", prefix, line)
                    }
                })
                .collect::<Vec<_>>();
            return Some(lines);
        }
    }

    Some(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::utils::chunk_options::ChunkOptionValue;

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
        assert!(lines[1].starts_with("#|   ")); // 3-space indent
        assert!(lines[0].len() <= 80);
        // Continuation lines might be slightly over due to word boundaries
    }

    #[test]
    fn test_format_hashpipe_option_block_scalar() {
        let value = "|\n   A caption\n   spanning lines";
        let lines = format_hashpipe_option_with_wrap("#|", "fig-cap", value, 80);
        assert_eq!(
            lines,
            vec!["#| fig-cap: |", "#|   A caption", "#|   spanning lines"]
        );
    }

    #[test]
    fn test_format_hashpipe_option_indented_yaml_multiline() {
        let value = "\n  - a\n  - b";
        let lines = format_hashpipe_option_with_wrap("#|", "list", value, 80);
        assert_eq!(lines, vec!["#| list:", "#|   - a", "#|   - b"]);
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

        let lines = format_as_hashpipe("r", &options, 80, None).unwrap();
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

        let lines = format_as_hashpipe("r", &options, 80, None).unwrap();
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
        let result = format_as_hashpipe("fortran", &options, 80, None);
        assert!(result.is_none());
    }
}

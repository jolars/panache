//! Hashpipe-style YAML option formatting for Quarto executable chunks.
//!
//! Converts inline chunk options to Quarto's new hashpipe format with proper
//! line wrapping and language-specific comment prefixes.

use crate::config::WrapMode;
use crate::syntax::{AstNode, ChunkInfoItem, CodeInfo, SyntaxNode};
use crate::yaml_engine;
use panache_parser::parser::utils::chunk_options::ChunkOptionValue;
use panache_parser::parser::utils::chunk_options::hashpipe_comment_prefix;
use panache_parser::parser::utils::hashpipe_normalizer::normalize_hashpipe_header;

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
    if let Some(normalized) = normalize_hashpipe_header(content, prefix)
        && let Some(options) = extract_options_from_normalized_yaml(&normalized.normalized_yaml)
    {
        had_content_hashpipe = true;
        for (key, value) in options {
            push_content_option(&mut entries, key, value, false);
        }
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

fn extract_options_from_normalized_yaml(normalized_yaml: &str) -> Option<Vec<(String, String)>> {
    let yaml_syntax = yaml_parser::parse(normalized_yaml).ok()?;
    let root = yaml_parser::ast::Root::cast(yaml_syntax)?;
    let map = root
        .documents()
        .next()
        .and_then(|doc| doc.block())
        .and_then(|block| block.block_map())?;

    let mut options = Vec::new();
    for entry in map.entries() {
        let key = hashpipe_map_entry_key(&entry)?;
        let value = hashpipe_map_entry_value_text(normalized_yaml, &entry);
        options.push((key, value));
    }
    Some(options)
}

fn hashpipe_map_entry_key(entry: &yaml_parser::ast::BlockMapEntry) -> Option<String> {
    let key = entry.key()?;
    if let Some(flow) = key.flow() {
        return hashpipe_flow_scalar_text(&flow);
    }
    let block = key.block()?;
    let flow = hashpipe_block_to_flow_scalar(&block)?;
    hashpipe_flow_scalar_text(&flow)
}

fn hashpipe_map_entry_value_text(
    normalized_yaml: &str,
    entry: &yaml_parser::ast::BlockMapEntry,
) -> String {
    let Some(value) = entry.value() else {
        return String::new();
    };

    if let Some(flow) = value.flow() {
        return hashpipe_flow_value_text(&flow).unwrap_or_else(|| {
            let range = flow.syntax().text_range();
            let start: usize = range.start().into();
            let end: usize = range.end().into();
            normalized_yaml[start..end].trim().to_string()
        });
    }

    if let Some(block) = value.block() {
        let range = block.syntax().text_range();
        let start: usize = range.start().into();
        let end: usize = range.end().into();
        let raw =
            restore_omitted_first_line_indent(normalized_yaml, start, &normalized_yaml[start..end]);
        // Preserve block values as block values when round-tripping through
        // hashpipe YAML normalization. Without this, sequence/map values can be
        // re-emitted as `key: - item` on a second pass (issue #172).
        if raw.starts_with('\n') {
            return raw;
        }
        let trimmed = raw.trim_start();
        if trimmed.starts_with('|') || trimmed.starts_with('>') {
            return raw;
        }
        return format!("\n{raw}");
    }

    let range = value.syntax().text_range();
    let start: usize = range.start().into();
    let end: usize = range.end().into();
    normalized_yaml[start..end].to_string()
}

fn hashpipe_block_to_flow_scalar(
    block: &yaml_parser::ast::Block,
) -> Option<yaml_parser::ast::Flow> {
    block
        .syntax()
        .children()
        .find_map(yaml_parser::ast::Flow::cast)
}

fn hashpipe_flow_scalar_text(flow: &yaml_parser::ast::Flow) -> Option<String> {
    let token = if let Some(token) = flow.plain_scalar() {
        token
    } else if let Some(token) = flow.single_quoted_scalar() {
        token
    } else if let Some(token) = flow.double_qouted_scalar() {
        token
    } else {
        return None;
    };
    let mut value = token.text().to_string();
    if token.kind() == yaml_parser::SyntaxKind::SINGLE_QUOTED_SCALAR {
        value = value.trim_matches('\'').to_string();
    } else if token.kind() == yaml_parser::SyntaxKind::DOUBLE_QUOTED_SCALAR {
        value = value.trim_matches('"').to_string();
    }
    Some(value)
}

fn hashpipe_flow_value_text(flow: &yaml_parser::ast::Flow) -> Option<String> {
    if let Some(token) = flow.plain_scalar() {
        let text = token.text().to_string();
        if text.contains('\n') {
            // Normalize wrapped plain scalars to a single logical line so
            // re-emission uses deterministic hashpipe wrapping.
            return Some(fold_multiline_plain_scalar(&text));
        }
        return Some(text);
    }
    if let Some(token) = flow.single_quoted_scalar() {
        return Some(token.text().to_string());
    }
    if let Some(token) = flow.double_qouted_scalar() {
        return Some(token.text().to_string());
    }
    None
}

fn fold_multiline_plain_scalar(text: &str) -> String {
    let mut lines = text
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line));
    let Some(first) = lines.next() else {
        return String::new();
    };

    let mut out = first.trim_end_matches([' ', '\t']).to_string();
    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        out.push(' ');
        out.push_str(line.trim_start_matches([' ', '\t']));
    }
    out
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

fn trim_minimum_indentation(text: &str) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();
    let min_indent = lines
        .iter()
        .filter_map(|line| {
            if line.trim().is_empty() {
                return None;
            }
            Some(line.chars().take_while(|c| matches!(c, ' ' | '\t')).count())
        })
        .min()
        .unwrap_or(0);

    lines
        .into_iter()
        .map(|line| {
            if line.trim().is_empty() {
                String::new()
            } else {
                line.chars().skip(min_indent).collect()
            }
        })
        .collect()
}

fn restore_omitted_first_line_indent(source: &str, value_start: usize, raw: &str) -> String {
    let line_start = source[..value_start].rfind('\n').map_or(0, |idx| idx + 1);
    let omitted_indent = &source[line_start..value_start];
    if omitted_indent.is_empty() || !omitted_indent.chars().all(|ch| matches!(ch, ' ' | '\t')) {
        return raw.to_string();
    }

    let mut out = String::with_capacity(raw.len() + omitted_indent.len());
    let mut applied = false;
    for line in raw.split_inclusive('\n') {
        let (line_content, has_newline) = if let Some(content) = line.strip_suffix('\n') {
            (content, true)
        } else {
            (line, false)
        };

        if !applied && !line_content.is_empty() {
            out.push_str(omitted_indent);
            applied = true;
        }
        out.push_str(line_content);
        if has_newline {
            out.push('\n');
        }
    }

    if !applied && !raw.is_empty() {
        return raw.to_string();
    }
    out
}

/// Classify an option value for hashpipe conversion.
///
/// Returns Some(ClassifiedValue) if the option is safe for hashpipe, None otherwise.
fn classify_option_for_hashpipe(
    key: &str,
    value: &str,
    is_quoted: bool,
) -> Option<ChunkOptionValue> {
    use panache_parser::parser::utils::chunk_options::{is_boolean_literal, is_numeric_literal};

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
    fn floor_char_boundary(s: &str, max: usize) -> usize {
        let mut idx = max.min(s.len());
        while idx > 0 && !s.is_char_boundary(idx) {
            idx -= 1;
        }
        idx
    }

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
            let upper = floor_char_boundary(remaining, available);
            if upper == 0 {
                remaining
                    .char_indices()
                    .nth(1)
                    .map(|(i, _)| i)
                    .unwrap_or(remaining.len())
            } else {
                remaining[..upper]
                    .rfind(' ')
                    .map(|i| i + 1) // Include the space
                    .unwrap_or(upper) // No space found, break at safe boundary
            }
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

            // Handle bare options (no value).
            // `label` is a string field; keep empty as an empty string instead of
            // coercing to boolean true.
            let mut value_str = if norm_val.is_empty() {
                if norm_key == "label" {
                    String::new()
                } else {
                    "true".to_string() // Bare option means true
                }
            } else {
                norm_val
            };

            // Quote labels only when needed to keep YAML parsing stable across
            // passes (e.g., `label: warning: false ...`), while preserving
            // existing plain-label output for normal labels.
            if norm_key == "label" && label_needs_yaml_quotes(&value_str) {
                value_str = canonicalize_label_yaml_value(&value_str);
            }

            yaml_entries.push((norm_key.clone(), value_str.clone()));
            let lines = format_hashpipe_option_with_wrap(prefix, &norm_key, &value_str, line_width);
            output.extend(lines);
        }
    }

    if !yaml_entries.is_empty() {
        let yaml_text = yaml_entries
            .iter()
            .map(|(key, value)| {
                if value.starts_with('\n')
                    && value
                        .lines()
                        .nth(1)
                        .is_some_and(|line| line.trim_start_matches([' ', '\t']).starts_with("- "))
                {
                    let mut rendered = String::new();
                    rendered.push_str(&format!("{key}:\n"));
                    let dedented = trim_minimum_indentation(value.trim_start_matches('\n'));
                    for line in dedented {
                        if line.is_empty() {
                            rendered.push('\n');
                        } else {
                            rendered.push_str("  ");
                            rendered.push_str(&line);
                            rendered.push('\n');
                        }
                    }
                    rendered
                } else if value.starts_with('\n') {
                    if value.ends_with('\n') {
                        format!("{key}:{value}")
                    } else {
                        format!("{key}:{value}\n")
                    }
                } else if value.contains('\n') && value.trim_start().starts_with("- ") {
                    let mut rendered = String::new();
                    rendered.push_str(&format!("{key}:\n"));
                    for line in value.lines() {
                        rendered.push_str("  ");
                        rendered.push_str(line);
                        rendered.push('\n');
                    }
                    rendered
                } else {
                    format!("{key}: {value}\n")
                }
            })
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

fn escape_for_double_quotes(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn canonicalize_label_yaml_value(value: &str) -> String {
    if (value.starts_with('"') && value.ends_with('"'))
        || (value.starts_with('\'') && value.ends_with('\''))
    {
        return value.to_string();
    }
    format!("\"{}\"", escape_for_double_quotes(value))
}

fn label_needs_yaml_quotes(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return true;
    }
    if (trimmed.starts_with('"') && trimmed.ends_with('"'))
        || (trimmed.starts_with('\'') && trimmed.ends_with('\''))
    {
        return false;
    }
    // YAML plain scalars become ambiguous for this case.
    trimmed.contains(':')
}

#[cfg(test)]
mod tests {
    use super::*;
    use panache_parser::parser::utils::chunk_options::ChunkOptionValue;

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
    fn test_format_hashpipe_option_wrap_handles_utf8_boundaries() {
        let value = "comparison data for three methods:- Student’s t, Bayes factor, and Welch’s t.";
        let lines = format_hashpipe_option_with_wrap("#|", "fig-cap", value, 60);

        assert!(lines.len() > 1, "Should wrap into multiple lines");
        assert!(lines[0].starts_with("#| fig-cap:"));
        assert!(lines[1].starts_with("#|   "));
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
    fn trim_minimum_indentation_preserves_relative_structure() {
        let lines = trim_minimum_indentation("   - ROC\n     - PR Curve");
        assert_eq!(lines, vec!["- ROC", "  - PR Curve"]);
    }

    #[test]
    fn restore_omitted_first_line_indent_for_block_sequence_values() {
        let source = "fig-cap:\n  - A\n  - B\n";
        let start = source.find("- A").expect("expected list item");
        let raw = &source[start..];
        let restored = restore_omitted_first_line_indent(source, start, raw);
        assert_eq!(restored, "  - A\n  - B\n");
    }

    #[test]
    fn fold_multiline_plain_scalar_preserves_internal_double_spaces() {
        let folded = fold_multiline_plain_scalar(
            "Type II tests for the sugar\n  x  milk interaction term where this\n  preserves x  milk",
        );
        assert_eq!(
            folded,
            "Type II tests for the sugar x  milk interaction term where this preserves x  milk"
        );
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

    #[test]
    fn extract_yaml_list_value_keeps_leading_newline_for_block_style() {
        let normalized_yaml = "\
fig-subcap:
  - \"The world\"
  - \"Systematic sampling\"
  - \"Stratified sampling\"
  - \"Cluster sampling\"
";

        let options =
            extract_options_from_normalized_yaml(normalized_yaml).expect("expected parsed options");
        let (_, value) = options
            .into_iter()
            .find(|(k, _)| k == "fig-subcap")
            .expect("missing fig-subcap option");

        assert!(
            value.starts_with('\n'),
            "expected block-style value to start with newline, got: {value:?}"
        );
    }

    #[test]
    fn format_as_hashpipe_canonicalizes_multiline_sequence_to_block_style() {
        let options = vec![
            (
                "fig-subcap".to_string(),
                ChunkOptionValue::Simple(
                    "- \"The world\"\n- \"Systematic sampling\"\n- \"Stratified sampling\"\n- \"Cluster sampling\""
                        .to_string(),
                ),
            ),
        ];

        let lines = format_as_hashpipe("r", &options, 80, None).expect("expected hashpipe lines");

        assert_eq!(lines.first().map(String::as_str), Some("#| fig-subcap:"));
        assert_eq!(
            lines.get(1).map(String::as_str),
            Some("#|   - \"The world\"")
        );
    }
}

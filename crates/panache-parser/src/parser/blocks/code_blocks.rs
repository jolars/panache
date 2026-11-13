//! Fenced code block parsing utilities.

use crate::parser::diagnostics::{Diagnostics, SyntaxError, SyntaxErrorSource};
use crate::parser::utils::attributes::emit_code_info_attrs;
use crate::parser::utils::chunk_options::hashpipe_comment_prefix;
use crate::syntax::SyntaxKind;
use rowan::{GreenNodeBuilder, TextRange};

use super::blockquotes::{count_blockquote_markers, strip_n_blockquote_markers};
use super::container_prefix::{
    StrippedLines, advance_emitted_marker_columns, content_line_prefix_tail,
};
use super::myst_directives::DirectiveCloser;
use crate::options::{Dialect, Flavor};
use crate::parser::utils::container_stack::{byte_index_at_column, leading_indent};
use crate::parser::utils::tree_copy::copy_green_children;
use crate::parser::yaml::{
    YamlValidationContext, locate_yaml_diagnostic_ctx, parse_stream_with_prefix,
};

pub(crate) use super::container_prefix::{
    bq_outer_of_list, emit_blockquote_prefix_tokens, strip_list_indent,
};

use crate::parser::utils::helpers::{
    strip_leading_spaces, strip_newline, trim_end_spaces_tabs, trim_start_spaces_tabs,
};

/// Represents the type of code block based on its info string syntax.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodeBlockType {
    /// Display-only block with shortcut syntax: ```python
    DisplayShortcut { language: String },
    /// Display-only block with explicit Pandoc syntax: ```{.python}
    DisplayExplicit { classes: Vec<String> },
    /// Executable chunk (Quarto/RMarkdown): ```{python}
    Executable { language: String },
    /// Raw block for specific output format: ```{=html}
    Raw { format: String },
    /// No language specified: ```
    Plain,
}

/// Parsed attributes from a code block info string.
#[derive(Debug, Clone, PartialEq)]
pub struct InfoString {
    pub raw: String,
    pub block_type: CodeBlockType,
    pub attributes: Vec<(String, Option<String>)>, // key-value pairs
}

impl InfoString {
    /// Parse an info string into structured attributes.
    ///
    /// Brace-delimited info strings (`{...}`) carry Pandoc attribute semantics
    /// (executable chunks, raw blocks, attribute lists). In the CommonMark
    /// dialect they have no special meaning — the info string is opaque and the
    /// first word is just the language class — so callers in that dialect should
    /// use [`InfoString::parse_with_dialect`]. Plain [`parse`](Self::parse)
    /// retains the Pandoc interpretation for backward compatibility.
    pub fn parse(raw: &str) -> Self {
        Self::parse_with_dialect(raw, crate::options::Dialect::Pandoc)
    }

    /// Parse an info string, honoring dialect-specific brace semantics.
    pub fn parse_with_dialect(raw: &str, dialect: crate::options::Dialect) -> Self {
        let trimmed = raw.trim();

        if trimmed.is_empty() {
            return InfoString {
                raw: raw.to_string(),
                block_type: CodeBlockType::Plain,
                attributes: Vec::new(),
            };
        }

        if dialect != crate::options::Dialect::Pandoc {
            let language = trimmed.split_whitespace().next().unwrap_or(trimmed);
            return InfoString {
                raw: raw.to_string(),
                block_type: CodeBlockType::DisplayShortcut {
                    language: language.to_string(),
                },
                attributes: Vec::new(),
            };
        }

        if let Some(stripped) = trimmed.strip_prefix('{')
            && let Some(content) = stripped.strip_suffix('}')
        {
            return Self::parse_explicit(raw, content);
        }

        if let Some(brace_start) = trimmed.find('{') {
            let language = trimmed[..brace_start].trim();
            if !language.is_empty() && !language.contains(char::is_whitespace) {
                let attr_part = &trimmed[brace_start..];
                if let Some(stripped) = attr_part.strip_prefix('{')
                    && let Some(content) = stripped.strip_suffix('}')
                {
                    let attrs = Self::parse_attributes(content);
                    return InfoString {
                        raw: raw.to_string(),
                        block_type: CodeBlockType::DisplayShortcut {
                            language: language.to_string(),
                        },
                        attributes: attrs,
                    };
                }
            }
        }

        let language = trimmed.split_whitespace().next().unwrap_or(trimmed);
        InfoString {
            raw: raw.to_string(),
            block_type: CodeBlockType::DisplayShortcut {
                language: language.to_string(),
            },
            attributes: Vec::new(),
        }
    }

    fn parse_explicit(raw: &str, content: &str) -> Self {
        let trimmed_content = content.trim();
        if let Some(format_name) = trimmed_content.strip_prefix('=')
            && !format_name.is_empty()
            && format_name.chars().all(|c| c.is_alphanumeric())
            && !format_name.contains(char::is_whitespace)
        {
            return InfoString {
                raw: raw.to_string(),
                block_type: CodeBlockType::Raw {
                    format: format_name.to_string(),
                },
                attributes: Vec::new(),
            };
        }

        let prelim_attrs = Self::parse_chunk_options(content);

        let mut first_lang_token = None;
        for (key, val) in prelim_attrs.iter() {
            if val.is_none() && !key.starts_with('#') && !key.chars().all(|c| c == '-') {
                first_lang_token = Some(key.as_str());
                break;
            }
        }

        let first_token = first_lang_token.unwrap_or("");

        if first_token.starts_with('.') {
            let attrs = Self::parse_pandoc_attributes(content);

            let classes: Vec<String> = attrs
                .iter()
                .filter(|(k, v)| k.starts_with('.') && v.is_none())
                .map(|(k, _)| k[1..].to_string())
                .collect();

            let non_class_attrs: Vec<(String, Option<String>)> = attrs
                .into_iter()
                .filter(|(k, _)| !k.starts_with('.') || k.contains('='))
                .collect();

            InfoString {
                raw: raw.to_string(),
                block_type: CodeBlockType::DisplayExplicit { classes },
                attributes: non_class_attrs,
            }
        } else if !first_token.is_empty() && !first_token.starts_with('#') {
            let attrs = Self::parse_chunk_options(content);
            let lang_index = attrs.iter().position(|(k, _)| k == first_token).unwrap();

            let mut has_implicit_label = false;
            let implicit_label_value = if lang_index + 1 < attrs.len() {
                let (label_key, val) = &attrs[lang_index + 1];
                if val.is_none() && !label_key.starts_with('.') && !label_key.starts_with('#') {
                    has_implicit_label = true;
                    Some(label_key.clone())
                } else {
                    None
                }
            } else {
                None
            };

            let mut final_attrs: Vec<(String, Option<String>)> = attrs
                .into_iter()
                .enumerate()
                .filter(|(i, _)| {
                    if *i == lang_index {
                        return false;
                    }
                    if has_implicit_label && *i == lang_index + 1 {
                        return false;
                    }
                    true
                })
                .map(|(_, attr)| attr)
                .collect();

            if let Some(label_val) = implicit_label_value {
                final_attrs.insert(0, ("label".to_string(), Some(label_val)));
            }

            InfoString {
                raw: raw.to_string(),
                block_type: CodeBlockType::Executable {
                    language: first_token.to_string(),
                },
                attributes: final_attrs,
            }
        } else {
            let attrs = Self::parse_pandoc_attributes(content);
            InfoString {
                raw: raw.to_string(),
                block_type: CodeBlockType::Plain,
                attributes: attrs,
            }
        }
    }

    /// Parse Pandoc-style attributes for display blocks: {.class #id key="value"}
    /// Spaces are the primary delimiter. Pandoc spec prefers explicit quoting.
    fn parse_pandoc_attributes(content: &str) -> Vec<(String, Option<String>)> {
        let mut attrs = Vec::new();
        let mut chars = content.chars().peekable();

        while chars.peek().is_some() {
            while matches!(chars.peek(), Some(&' ') | Some(&'\t')) {
                chars.next();
            }

            if chars.peek().is_none() {
                break;
            }

            let mut key = String::new();
            while let Some(&ch) = chars.peek() {
                if ch == '=' || ch == ' ' || ch == '\t' {
                    break;
                }
                key.push(ch);
                chars.next();
            }

            if key.is_empty() {
                break;
            }

            while matches!(chars.peek(), Some(&' ') | Some(&'\t')) {
                chars.next();
            }

            if chars.peek() == Some(&'=') {
                chars.next(); // consume '='

                while matches!(chars.peek(), Some(&' ') | Some(&'\t')) {
                    chars.next();
                }

                let value = if chars.peek() == Some(&'"') {
                    chars.next(); // consume opening quote
                    let mut val = String::new();
                    while let Some(&ch) = chars.peek() {
                        chars.next();
                        if ch == '"' {
                            break;
                        }
                        if ch == '\\' {
                            if let Some(&next_ch) = chars.peek() {
                                chars.next();
                                val.push(next_ch);
                            }
                        } else {
                            val.push(ch);
                        }
                    }
                    val
                } else {
                    let mut val = String::new();
                    while let Some(&ch) = chars.peek() {
                        if ch == ' ' || ch == '\t' {
                            break;
                        }
                        val.push(ch);
                        chars.next();
                    }
                    val
                };

                attrs.push((key, Some(value)));
            } else {
                attrs.push((key, None));
            }
        }

        attrs
    }

    fn parse_chunk_options(content: &str) -> Vec<(String, Option<String>)> {
        let mut attrs = Vec::new();
        let mut chars = content.chars().peekable();

        while chars.peek().is_some() {
            while matches!(chars.peek(), Some(&' ') | Some(&'\t') | Some(&',')) {
                chars.next();
            }

            if chars.peek().is_none() {
                break;
            }

            let mut key = String::new();
            while let Some(&ch) = chars.peek() {
                if ch == '=' || ch == ' ' || ch == '\t' || ch == ',' {
                    break;
                }
                key.push(ch);
                chars.next();
            }

            if key.is_empty() {
                break;
            }

            while matches!(chars.peek(), Some(&' ') | Some(&'\t') | Some(&',')) {
                chars.next();
            }

            if chars.peek() == Some(&'=') {
                chars.next(); // consume '='

                while matches!(chars.peek(), Some(&' ') | Some(&'\t') | Some(&',')) {
                    chars.next();
                }

                let value = if chars.peek() == Some(&'"') {
                    chars.next(); // consume opening quote
                    let mut val = String::new();
                    while let Some(&ch) = chars.peek() {
                        chars.next();
                        if ch == '"' {
                            break;
                        }
                        if ch == '\\' {
                            if let Some(&next_ch) = chars.peek() {
                                chars.next();
                                val.push(next_ch);
                            }
                        } else {
                            val.push(ch);
                        }
                    }
                    val
                } else {
                    let mut val = String::new();
                    let mut depth = 0; // Track parentheses/brackets/braces depth
                    let mut in_quote: Option<char> = None; // Track if inside ' or "
                    let mut escaped = false; // Track if previous char was backslash

                    while let Some(&ch) = chars.peek() {
                        if escaped {
                            val.push(ch);
                            chars.next();
                            escaped = false;
                            continue;
                        }

                        if ch == '\\' {
                            val.push(ch);
                            chars.next();
                            escaped = true;
                            continue;
                        }

                        if let Some(quote_char) = in_quote {
                            val.push(ch);
                            chars.next();
                            if ch == quote_char {
                                in_quote = None; // Close quote
                            }
                            continue;
                        }

                        if ch == '"' || ch == '\'' {
                            in_quote = Some(ch);
                            val.push(ch);
                            chars.next();
                            continue;
                        }

                        if ch == '(' || ch == '[' || ch == '{' {
                            depth += 1;
                            val.push(ch);
                            chars.next();
                            continue;
                        }

                        if ch == ')' || ch == ']' || ch == '}' {
                            depth -= 1;
                            val.push(ch);
                            chars.next();
                            continue;
                        }

                        if depth == 0
                            && (ch == ','
                                || (matches!(ch, ' ' | '\t') && starts_chunk_option(chars.clone())))
                        {
                            break;
                        }

                        val.push(ch);
                        chars.next();
                    }
                    val.trim_end().to_string()
                };

                attrs.push((key, Some(value)));
            } else {
                attrs.push((key, None));
            }
        }

        attrs
    }

    /// Legacy function - kept for backward compatibility in mixed-form parsing
    /// For new code, use parse_pandoc_attributes or parse_chunk_options
    fn parse_attributes(content: &str) -> Vec<(String, Option<String>)> {
        Self::parse_chunk_options(content)
    }
}

/// Information about a detected code fence opening.
#[derive(Debug, Clone)]
pub(crate) struct FenceInfo {
    pub fence_char: char,
    pub fence_count: usize,
    pub info_string: String,
}

pub(crate) fn is_gfm_math_fence(fence: &FenceInfo) -> bool {
    fence.info_string.trim() == "math"
}

/// Try to detect a fenced code block opening from content.
/// Returns fence info if this is a valid opening fence.
pub(crate) fn try_parse_fence_open(
    content: &str,
    dialect: crate::options::Dialect,
) -> Option<FenceInfo> {
    let trimmed = strip_leading_spaces(content);

    let (fence_char, fence_count) = if trimmed.starts_with('`') {
        let count = trimmed.chars().take_while(|&c| c == '`').count();
        ('`', count)
    } else if trimmed.starts_with('~') {
        let count = trimmed.chars().take_while(|&c| c == '~').count();
        ('~', count)
    } else {
        return None;
    };

    if fence_count < 3 {
        return None;
    }

    let info_string_raw = &trimmed[fence_count..];
    let (info_string_trimmed, _) = strip_newline(info_string_raw);
    let info_string = if let Some(stripped) = info_string_trimmed.strip_prefix(' ') {
        stripped.to_string()
    } else {
        info_string_trimmed.to_string()
    };

    if fence_char == '`' && info_string.contains('`') {
        return None;
    }

    if dialect == crate::options::Dialect::Pandoc {
        let bare = info_string.trim();
        if !bare.is_empty() {
            let is_valid = if let Some(brace_start) = bare.find('{') {
                let before = bare[..brace_start].trim();
                !before.contains(char::is_whitespace) && bare.ends_with('}')
            } else {
                bare.split_whitespace().nth(1).is_none()
            };
            if !is_valid {
                return None;
            }
        }
    }

    Some(FenceInfo {
        fence_char,
        fence_count,
        info_string,
    })
}

#[allow(clippy::too_many_arguments)]
fn prepare_fence_open_line<'a>(
    builder: &mut GreenNodeBuilder<'static>,
    source_line: &'a str,
    first_line_override: Option<&'a str>,
    bq_depth: usize,
    list_content_col: usize,
    list_marker_consumed_on_line_0: bool,
    bq_outer: bool,
    content_indent: usize,
) -> (&'a str, &'a str) {
    if let Some(first_line) = first_line_override {
        if bq_depth > 0 && source_line != first_line {
            let stripped = strip_n_blockquote_markers(source_line, bq_depth);
            let prefix_len = source_line.len().saturating_sub(stripped.len());
            if prefix_len > 0 {
                emit_blockquote_prefix_tokens(builder, &source_line[..prefix_len]);
            }
        }
        let first_trimmed = strip_leading_spaces(first_line);
        let leading_ws_len = first_line.len().saturating_sub(first_trimmed.len());
        if leading_ws_len > 0 {
            builder.token(SyntaxKind::WHITESPACE.into(), &first_line[..leading_ws_len]);
        }
        return (first_trimmed, first_line);
    }

    let mut s: &'a str = source_line;
    let mut pending_ws_start: Option<usize> = None;
    let suppress_list = list_marker_consumed_on_line_0;

    let flush_ws = |builder: &mut GreenNodeBuilder<'static>,
                    pending: &mut Option<usize>,
                    current_offset: usize| {
        if let Some(start) = *pending
            && current_offset > start
        {
            builder.token(
                SyntaxKind::LINE_PREFIX.into(),
                &source_line[start..current_offset],
            );
        }
        *pending = None;
    };

    let do_strip_list = |s: &mut &'a str, pending: &mut Option<usize>| {
        if list_content_col == 0 {
            return;
        }
        let stripped = if suppress_list {
            advance_emitted_marker_columns(s, list_content_col)
        } else {
            strip_list_indent(s, list_content_col)
        };
        let consumed = s.len() - stripped.len();
        if consumed > 0 {
            let start = source_line.len() - s.len();
            if !suppress_list && pending.is_none() {
                *pending = Some(start);
            }
            *s = stripped;
        }
    };

    let do_strip_bq =
        |builder: &mut GreenNodeBuilder<'static>, s: &mut &'a str, pending: &mut Option<usize>| {
            if bq_depth == 0 {
                return;
            }
            let current_offset = source_line.len() - s.len();
            flush_ws(builder, pending, current_offset);
            *s = strip_n_blockquote_markers(s, bq_depth);
        };

    if bq_outer {
        do_strip_bq(builder, &mut s, &mut pending_ws_start);
        do_strip_list(&mut s, &mut pending_ws_start);
    } else {
        do_strip_list(&mut s, &mut pending_ws_start);
        do_strip_bq(builder, &mut s, &mut pending_ws_start);
    }

    if content_indent > 0 {
        let indent_bytes = byte_index_at_column(s, content_indent);
        if s.len() >= indent_bytes && indent_bytes > 0 {
            let start = source_line.len() - s.len();
            if pending_ws_start.is_none() {
                pending_ws_start = Some(start);
            }
            s = &s[indent_bytes..];
        }
    }

    let final_offset = source_line.len() - s.len();
    flush_ws(builder, &mut pending_ws_start, final_offset);

    let first_trimmed = strip_leading_spaces(s);
    let leading_ws_len = s.len().saturating_sub(first_trimmed.len());
    if leading_ws_len > 0 {
        builder.token(SyntaxKind::WHITESPACE.into(), &s[..leading_ws_len]);
    }
    (first_trimmed, s)
}

pub(crate) fn compute_hashpipe_preamble_line_count(
    content_lines: &[&str],
    prefix: &str,
    bq_depth: usize,
    list_content_col: usize,
    bq_outer: bool,
    content_indent: usize,
    lazy_gobble: bool,
) -> usize {
    let preview = |idx: usize| -> Option<&str> {
        let line = content_lines.get(idx)?;
        let after_indent = content_line_prefix_tail(
            line,
            bq_depth,
            list_content_col,
            bq_outer,
            content_indent,
            lazy_gobble,
        );
        Some(strip_newline(after_indent).0)
    };

    let mut line_idx = 0usize;
    while let Some(preview_without_newline) = preview(line_idx) {
        if is_hashpipe_option_line(preview_without_newline, prefix)
            || is_hashpipe_continuation_line(preview_without_newline, prefix)
        {
            line_idx += 1;
            continue;
        }
        if is_hashpipe_blank_line(preview_without_newline, prefix)
            && preview(line_idx + 1)
                .is_some_and(|next| trim_start_spaces_tabs(next).starts_with(prefix))
        {
            line_idx += 1;
            continue;
        }
        break;
    }

    line_idx
}

fn hashpipe_composite_marker<'a>(
    first_line: &'a str,
    prefix: &str,
    bq_depth: usize,
    list_content_col: usize,
    bq_outer: bool,
    content_indent: usize,
    lazy_gobble: bool,
) -> &'a str {
    let after_container = content_line_prefix_tail(
        first_line,
        bq_depth,
        list_content_col,
        bq_outer,
        content_indent,
        lazy_gobble,
    );
    let container_len = first_line.len() - after_container.len();
    let ws_before = after_container.len() - trim_start_spaces_tabs(after_container).len();
    let marker_len = (container_len + ws_before + prefix.len()).min(first_line.len());
    &first_line[..marker_len]
}

fn is_hashpipe_option_line(line_without_newline: &str, prefix: &str) -> bool {
    let trimmed_start = trim_start_spaces_tabs(line_without_newline);
    if !trimmed_start.starts_with(prefix) {
        return false;
    }
    let after_prefix = &trimmed_start[prefix.len()..];
    let rest = trim_start_spaces_tabs(after_prefix);
    let Some(colon_idx) = rest.find(':') else {
        return false;
    };
    let key = trim_end_spaces_tabs(&rest[..colon_idx]);
    !key.is_empty()
}

fn is_hashpipe_continuation_line(line_without_newline: &str, prefix: &str) -> bool {
    let trimmed_start = trim_start_spaces_tabs(line_without_newline);
    if !trimmed_start.starts_with(prefix) {
        return false;
    }
    let after_prefix = &trimmed_start[prefix.len()..];
    let Some(first) = after_prefix.chars().next() else {
        return false;
    };
    if first != ' ' && first != '\t' {
        return false;
    }
    !trim_start_spaces_tabs(after_prefix).is_empty()
}

fn is_hashpipe_blank_line(line_without_newline: &str, prefix: &str) -> bool {
    let trimmed_start = trim_start_spaces_tabs(line_without_newline);
    let Some(after_prefix) = trimmed_start.strip_prefix(prefix) else {
        return false;
    };
    trim_start_spaces_tabs(after_prefix).is_empty()
}

/// Check if a line is a valid closing fence for the given fence info.
pub(crate) fn is_closing_fence(content: &str, fence: &FenceInfo) -> bool {
    let trimmed = strip_leading_spaces(content);

    if !trimmed.starts_with(fence.fence_char) {
        return false;
    }

    let closing_count = trimmed
        .chars()
        .take_while(|&c| c == fence.fence_char)
        .count();

    if closing_count < fence.fence_count {
        return false;
    }

    trimmed[closing_count..].trim().is_empty()
}

/// Tracks how far ahead a forward scan for a fence's closer may look before it
/// walks out of the container the fence was opened in.
///
/// A bare fence only opens a code block when a matching closer follows, so the
/// scan has to stop where the container does — a closer beyond it belongs to
/// somebody else. Pandoc's `listItem` gobbles under-indented lines lazily
/// (`- ``` ` / `  c` / `x` / `  ``` ` is one `CodeBlock "c\nx"`), but a blank
/// line arms the indent requirement: the next non-blank line must reach the
/// content column or the item ends there. That is exactly the boundary the
/// scan must respect, or a top-level fence past the item is mistaken for the
/// item's closer and adopts everything in between.
///
/// `content_col` of 0 means no indent-bearing container, so the scan never
/// exits. Blockquote depth is handled by the callers, which break on a marker
/// drop before consulting this.
pub(crate) struct ContainerExitScan {
    content_col: usize,
    blank_seen: bool,
}

impl ContainerExitScan {
    pub(crate) fn new(content_col: usize) -> Self {
        Self {
            content_col,
            blank_seen: false,
        }
    }

    /// Feed the next line's container-inner text; `true` once the container has
    /// ended and the scan must stop.
    pub(crate) fn exits(&mut self, inner: &str) -> bool {
        if inner.trim().is_empty() {
            self.blank_seen = true;
            return false;
        }
        let exits =
            self.blank_seen && self.content_col > 0 && leading_indent(inner).0 < self.content_col;
        self.blank_seen = false;
        exits
    }
}

/// Whitespace separates options only when another assignment or fence attribute follows.
/// In particular, comparison operators belong to the current expression.
fn starts_chunk_option(chars: impl Iterator<Item = char>) -> bool {
    let mut chars = chars.skip_while(|ch| matches!(ch, ' ' | '\t')).peekable();
    if matches!(chars.peek(), None | Some(',' | '}' | '#')) {
        return true;
    }

    let is_class = chars.peek() == Some(&'.');
    let mut has_key = false;
    while matches!(chars.peek(), Some(ch) if ch.is_alphanumeric() || matches!(ch, '_' | '.' | '-'))
    {
        has_key = true;
        chars.next();
    }
    if is_class && matches!(chars.peek(), None | Some(' ' | '\t' | ',' | '}')) {
        return true;
    }
    while matches!(chars.peek(), Some(' ' | '\t')) {
        chars.next();
    }
    has_key && chars.next() == Some('=') && chars.peek() != Some(&'=')
}

fn emit_chunk_options(builder: &mut GreenNodeBuilder<'static>, content: &str) {
    if content.trim().is_empty() {
        builder.token(SyntaxKind::TEXT.into(), content);
        return;
    }

    builder.start_node(SyntaxKind::CHUNK_OPTIONS.into());

    let mut pos = 0;
    let bytes = content.as_bytes();

    while pos < bytes.len() {
        let ws_start = pos;
        while pos < bytes.len() {
            let ch = bytes[pos] as char;
            if ch != ' ' && ch != '\t' && ch != ',' {
                break;
            }
            pos += 1;
        }
        if pos > ws_start {
            builder.token(SyntaxKind::TEXT.into(), &content[ws_start..pos]);
        }

        if pos >= bytes.len() {
            break;
        }

        if bytes[pos] as char == '}' {
            builder.token(SyntaxKind::TEXT.into(), &content[pos..pos + 1]);
            pos += 1;
            if pos < bytes.len() {
                builder.token(SyntaxKind::TEXT.into(), &content[pos..]);
            }
            break;
        }

        let key_start = pos;
        while pos < bytes.len() {
            let ch = bytes[pos] as char;
            if ch == '=' || ch == ' ' || ch == '\t' || ch == ',' || ch == '}' {
                break;
            }
            pos += 1;
        }

        if pos == key_start {
            if pos < bytes.len() {
                builder.token(SyntaxKind::TEXT.into(), &content[pos..]);
            }
            break;
        }

        let key = &content[key_start..pos];

        let ws_before_eq_start = pos;
        while pos < bytes.len() && matches!(bytes[pos] as char, ' ' | '\t') {
            pos += 1;
        }

        if pos < bytes.len() && bytes[pos] as char == '=' {
            builder.start_node(SyntaxKind::CHUNK_OPTION.into());
            builder.token(SyntaxKind::CHUNK_OPTION_KEY.into(), key);

            if pos > ws_before_eq_start {
                builder.token(SyntaxKind::TEXT.into(), &content[ws_before_eq_start..pos]);
            }

            builder.token(SyntaxKind::TEXT.into(), "=");
            pos += 1; // consume '='

            let ws_after_eq_start = pos;
            while pos < bytes.len() && matches!(bytes[pos] as char, ' ' | '\t') {
                pos += 1;
            }
            if pos > ws_after_eq_start {
                builder.token(SyntaxKind::TEXT.into(), &content[ws_after_eq_start..pos]);
            }

            if pos < bytes.len() {
                let quote_char = bytes[pos] as char;
                if quote_char == '"' || quote_char == '\'' {
                    builder.token(
                        SyntaxKind::CHUNK_OPTION_QUOTE.into(),
                        &content[pos..pos + 1],
                    );
                    pos += 1; // consume opening quote

                    let val_start = pos;
                    let mut escaped = false;
                    while pos < bytes.len() {
                        let ch = bytes[pos] as char;
                        if !escaped && ch == quote_char {
                            break;
                        }
                        escaped = !escaped && ch == '\\';
                        pos += 1;
                    }

                    if pos > val_start {
                        builder.token(
                            SyntaxKind::CHUNK_OPTION_VALUE.into(),
                            &content[val_start..pos],
                        );
                    }

                    if pos < bytes.len() && bytes[pos] as char == quote_char {
                        builder.token(
                            SyntaxKind::CHUNK_OPTION_QUOTE.into(),
                            &content[pos..pos + 1],
                        );
                        pos += 1;
                    }
                } else {
                    let val_start = pos;
                    let mut depth = 0;
                    let mut in_quote = None;
                    let mut escaped = false;

                    while pos < bytes.len() {
                        let ch = bytes[pos] as char;
                        if escaped {
                            escaped = false;
                        } else if ch == '\\' {
                            escaped = true;
                        } else if let Some(quote) = in_quote {
                            if ch == quote {
                                in_quote = None;
                            }
                        } else {
                            match ch {
                                '"' | '\'' | '`' => in_quote = Some(ch),
                                '(' | '[' | '{' => depth += 1,
                                ')' | ']' => {
                                    if depth > 0 {
                                        depth -= 1;
                                    } else {
                                        break;
                                    }
                                }
                                '}' => {
                                    if depth > 0 {
                                        depth -= 1;
                                    } else {
                                        break; // End of chunk options
                                    }
                                }
                                ',' if depth == 0 => {
                                    break; // Next option
                                }
                                ' ' | '\t'
                                    if depth == 0
                                        && starts_chunk_option(content[pos..].chars()) =>
                                {
                                    break;
                                }
                                _ => {}
                            }
                        }
                        pos += 1;
                    }

                    if pos > val_start {
                        builder.token(
                            SyntaxKind::CHUNK_OPTION_VALUE.into(),
                            &content[val_start..pos],
                        );
                    }
                }
            }

            builder.finish_node(); // CHUNK_OPTION
        } else {
            let kind = match key.as_bytes().first() {
                Some(b'.') => SyntaxKind::ATTR_CLASS,
                Some(b'#') => SyntaxKind::ATTR_ID,
                _ => SyntaxKind::CHUNK_LABEL,
            };
            builder.start_node(kind.into());
            builder.token(SyntaxKind::TEXT.into(), key);
            builder.finish_node();
            if pos > ws_before_eq_start {
                builder.token(SyntaxKind::TEXT.into(), &content[ws_before_eq_start..pos]);
            }
        }
    }

    builder.finish_node(); // CHUNK_OPTIONS
}

fn emit_code_info_node(
    builder: &mut GreenNodeBuilder<'static>,
    info_string: &str,
    dialect: crate::options::Dialect,
) {
    builder.start_node(SyntaxKind::CODE_INFO.into());

    let info = InfoString::parse_with_dialect(info_string, dialect);

    match &info.block_type {
        CodeBlockType::DisplayShortcut { language } => {
            builder.token(SyntaxKind::CODE_LANGUAGE.into(), language);

            let after_lang = &info_string[language.len()..];
            if !after_lang.is_empty()
                && !emit_code_info_attrs(builder, after_lang, /* carve */ false)
            {
                builder.token(SyntaxKind::TEXT.into(), after_lang);
            }
        }
        CodeBlockType::Executable { language } => {
            builder.token(SyntaxKind::TEXT.into(), "{");
            builder.token(SyntaxKind::CODE_LANGUAGE.into(), language);

            let start_offset = 1 + language.len(); // Skip "{r"
            if start_offset < info_string.len() {
                let rest = &info_string[start_offset..];
                emit_chunk_options(builder, rest);
            }
        }
        CodeBlockType::DisplayExplicit { .. } => {
            if !emit_code_info_attrs(builder, info_string, /* carve */ true) {
                builder.token(SyntaxKind::TEXT.into(), info_string);
            }
        }
        CodeBlockType::Raw { .. } | CodeBlockType::Plain => {
            builder.token(SyntaxKind::TEXT.into(), info_string);
        }
    }

    builder.finish_node(); // CodeInfo
}

/// Parse a fenced code block, consuming lines from the parser.
/// Parse a fenced code block, consuming lines from the parser.
/// Returns the new position after the code block.
///
/// All container geometry (blockquote depth, list-item indent,
/// footnote/definition base indent, and the bq-vs-list strip order) is
/// derived from `window.prefix()`; detection scans and the open-fence
/// emitter read those derived scalars, and content/closing-fence lines
/// re-emit their container prefix via [`StrippedLines::emit_prefix_at`].
pub(crate) fn parse_fenced_code_block(
    builder: &mut GreenNodeBuilder<'static>,
    window: &StrippedLines<'_, '_>,
    fence: FenceInfo,
    first_line_override: Option<&str>,
    diags: &Diagnostics,
    flavor: Flavor,
    myst_directive_closer: Option<&DirectiveCloser>,
) -> usize {
    let lines = window.raw();
    let start_pos = window.pos();
    let prefix = window.prefix();
    let bq_depth = prefix.bq_depth();
    let list_content_col = prefix.list_content_col();
    let list_marker_consumed_on_line_0 = prefix.list_marker_consumed_on_line_0;
    let bq_outer = bq_outer_of_list(prefix);
    let content_indent = prefix.content_indent();
    let lazy_gobble = prefix.lazy_blockquote_gobble;

    builder.start_node(SyntaxKind::CODE_BLOCK.into());

    let (first_trimmed, _first_inner) = prepare_fence_open_line(
        builder,
        lines[start_pos],
        first_line_override,
        bq_depth,
        list_content_col,
        list_marker_consumed_on_line_0,
        bq_outer,
        content_indent,
    );

    builder.start_node(SyntaxKind::CODE_FENCE_OPEN.into());
    builder.token(
        SyntaxKind::CODE_FENCE_MARKER.into(),
        &first_trimmed[..fence.fence_count],
    );

    let after_fence = &first_trimmed[fence.fence_count..];
    if let Some(_space_stripped) = after_fence.strip_prefix(' ') {
        builder.token(SyntaxKind::WHITESPACE.into(), " ");
        if !fence.info_string.is_empty() {
            emit_code_info_node(builder, &fence.info_string, Dialect::for_flavor(flavor));
        }
    } else if !fence.info_string.is_empty() {
        emit_code_info_node(builder, &fence.info_string, Dialect::for_flavor(flavor));
    }

    let (_, newline_str) = strip_newline(first_trimmed);
    if !newline_str.is_empty() {
        builder.token(SyntaxKind::NEWLINE.into(), newline_str);
    }
    builder.finish_node(); // CodeFenceOpen

    let mut current_pos = start_pos + 1;
    let mut content_lines: Vec<&str> = Vec::new(); // Store original lines for lossless parsing
    let mut found_closing = false;

    while current_pos < lines.len() {
        let line = lines[current_pos];

        let probe = if bq_outer {
            line
        } else {
            strip_list_indent(line, list_content_col)
        };
        let (line_bq_depth, _) = count_blockquote_markers(probe);
        let gobbled_lazily = Dialect::for_flavor(flavor) == crate::options::Dialect::Pandoc
            && bq_depth > 0
            && !line.trim().is_empty();
        if line_bq_depth < bq_depth && !gobbled_lazily {
            break;
        }

        let inner_stripped = window.peek_prefix_at(current_pos);

        // The directive extracts its body before nested code is parsed, so
        // its closer takes precedence over a code closer or unfinished code.
        if myst_directive_closer.is_some_and(|closer| closer.matches(line)) {
            break;
        }

        if is_closing_fence(inner_stripped, &fence) {
            found_closing = true;
            current_pos += 1;
            break;
        }

        content_lines.push(line);
        current_pos += 1;
    }

    if !content_lines.is_empty() {
        builder.start_node(SyntaxKind::CODE_CONTENT.into());
        let hashpipe_prefix = match InfoString::parse(&fence.info_string).block_type {
            CodeBlockType::Executable { language } => hashpipe_comment_prefix(&language),
            _ => None,
        };

        let mut line_idx = 0usize;
        if let Some(prefix) = hashpipe_prefix {
            let prepared_hashpipe_lines = compute_hashpipe_preamble_line_count(
                &content_lines,
                prefix,
                bq_depth,
                list_content_col,
                bq_outer,
                content_indent,
                lazy_gobble,
            );
            if prepared_hashpipe_lines > 0 {
                builder.start_node(SyntaxKind::HASHPIPE_YAML_PREAMBLE.into());
                builder.start_node(SyntaxKind::HASHPIPE_YAML_CONTENT.into());

                let content: String = content_lines[..prepared_hashpipe_lines].concat();
                let marker = hashpipe_composite_marker(
                    content_lines[0],
                    prefix,
                    bq_depth,
                    list_content_col,
                    bq_outer,
                    content_indent,
                    lazy_gobble,
                );

                let yaml_ctx = YamlValidationContext::hashpipe(flavor);
                if let Some((diag, start_off, end_off)) =
                    locate_yaml_diagnostic_ctx(&content, marker, yaml_ctx)
                {
                    let host_start =
                        content_lines[0].as_ptr() as usize - lines[0].as_ptr() as usize;
                    diags.push(SyntaxError {
                        range: TextRange::new(
                            ((host_start + start_off) as u32).into(),
                            ((host_start + end_off) as u32).into(),
                        ),
                        message: diag.message.to_string(),
                        source: SyntaxErrorSource::Yaml,
                    });
                    while line_idx < prepared_hashpipe_lines {
                        let after_indent = window.emit_prefix_at(builder, start_pos + 1 + line_idx);
                        let (line_without_newline, newline_str) = strip_newline(after_indent);
                        if !line_without_newline.is_empty() {
                            builder.token(SyntaxKind::TEXT.into(), line_without_newline);
                        }
                        if !newline_str.is_empty() {
                            builder.token(SyntaxKind::NEWLINE.into(), newline_str);
                        }
                        line_idx += 1;
                    }
                } else {
                    let stream = parse_stream_with_prefix(&content, marker)
                        .green()
                        .to_owned();
                    copy_green_children(builder, &stream);
                }
                line_idx = prepared_hashpipe_lines;

                builder.finish_node(); // HASHPIPE_YAML_CONTENT
                builder.finish_node(); // HASHPIPE_YAML_PREAMBLE
            }
        }

        for k in line_idx..content_lines.len() {
            let after_indent = window.emit_prefix_at(builder, start_pos + 1 + k);
            let (line_without_newline, newline_str) = strip_newline(after_indent);

            if !line_without_newline.is_empty() {
                builder.token(SyntaxKind::TEXT.into(), line_without_newline);
            }

            if !newline_str.is_empty() {
                builder.token(SyntaxKind::NEWLINE.into(), newline_str);
            }
        }
        builder.finish_node(); // CodeContent
    }

    if found_closing {
        let closing_stripped = window.emit_prefix_at(builder, current_pos - 1);
        let (closing_without_newline, newline_str) = strip_newline(closing_stripped);
        let closing_trimmed_start = strip_leading_spaces(closing_without_newline);
        let leading_ws_len = closing_without_newline.len() - closing_trimmed_start.len();
        let closing_count = closing_trimmed_start
            .chars()
            .take_while(|&c| c == fence.fence_char)
            .count();
        let trailing_after_marker = &closing_trimmed_start[closing_count..];

        builder.start_node(SyntaxKind::CODE_FENCE_CLOSE.into());
        if leading_ws_len > 0 {
            builder.token(
                SyntaxKind::WHITESPACE.into(),
                &closing_without_newline[..leading_ws_len],
            );
        }
        builder.token(
            SyntaxKind::CODE_FENCE_MARKER.into(),
            &closing_trimmed_start[..closing_count],
        );
        if !trailing_after_marker.is_empty() {
            builder.token(SyntaxKind::WHITESPACE.into(), trailing_after_marker);
        }
        if !newline_str.is_empty() {
            builder.token(SyntaxKind::NEWLINE.into(), newline_str);
        }
        builder.finish_node(); // CodeFenceClose
    }

    builder.finish_node(); // CodeBlock

    current_pos
}

/// Parse a GFM math fence (``` math ... ```) as DISPLAY_MATH while preserving bytes.
///
/// Container geometry is derived from `window.prefix()`, mirroring
/// [`parse_fenced_code_block`].
pub(crate) fn parse_fenced_math_block(
    builder: &mut GreenNodeBuilder<'static>,
    window: &StrippedLines<'_, '_>,
    fence: FenceInfo,
    first_line_override: Option<&str>,
    dialect: Dialect,
) -> usize {
    let lines = window.raw();
    let start_pos = window.pos();
    let prefix = window.prefix();
    let bq_depth = prefix.bq_depth();
    let list_content_col = prefix.list_content_col();
    let list_marker_consumed_on_line_0 = prefix.list_marker_consumed_on_line_0;
    let bq_outer = bq_outer_of_list(prefix);
    let content_indent = prefix.content_indent();

    builder.start_node(SyntaxKind::DISPLAY_MATH.into());

    let (first_trimmed, _first_inner) = prepare_fence_open_line(
        builder,
        lines[start_pos],
        first_line_override,
        bq_depth,
        list_content_col,
        list_marker_consumed_on_line_0,
        bq_outer,
        content_indent,
    );
    let (opening_without_newline, opening_newline) = strip_newline(first_trimmed);
    builder.token(
        SyntaxKind::DISPLAY_MATH_MARKER.into(),
        opening_without_newline,
    );
    if !opening_newline.is_empty() {
        builder.token(SyntaxKind::NEWLINE.into(), opening_newline);
    }

    let mut current_pos = start_pos + 1;
    let mut content_lines: Vec<&str> = Vec::new();
    let mut found_closing = false;

    while current_pos < lines.len() {
        let line = lines[current_pos];

        let probe = if bq_outer {
            line
        } else {
            strip_list_indent(line, list_content_col)
        };
        let (line_bq_depth, _) = count_blockquote_markers(probe);
        let gobbled_lazily = dialect == Dialect::Pandoc && bq_depth > 0 && !line.trim().is_empty();
        if line_bq_depth < bq_depth && !gobbled_lazily {
            break;
        }

        let inner_stripped = window.peek_prefix_at(current_pos);

        if is_closing_fence(inner_stripped, &fence) {
            found_closing = true;
            current_pos += 1;
            break;
        }

        content_lines.push(line);
        current_pos += 1;
    }

    if !content_lines.is_empty() {
        let mut content = String::new();
        for k in 0..content_lines.len() {
            let after_indent = window.emit_prefix_at(builder, start_pos + 1 + k);
            let (line_without_newline, newline_str) = strip_newline(after_indent);
            content.push_str(line_without_newline);
            content.push_str(newline_str);
        }
        builder.token(SyntaxKind::TEXT.into(), &content);
    }

    if found_closing {
        let closing_stripped = window.emit_prefix_at(builder, current_pos - 1);
        let (closing_without_newline, newline_str) = strip_newline(closing_stripped);
        let closing_trimmed_start = strip_leading_spaces(closing_without_newline);
        let leading_ws_len = closing_without_newline.len() - closing_trimmed_start.len();
        let closing_count = closing_trimmed_start
            .chars()
            .take_while(|&c| c == fence.fence_char)
            .count();
        let trailing_after_marker = &closing_trimmed_start[closing_count..];

        if leading_ws_len > 0 {
            builder.token(
                SyntaxKind::WHITESPACE.into(),
                &closing_without_newline[..leading_ws_len],
            );
        }
        builder.token(
            SyntaxKind::DISPLAY_MATH_MARKER.into(),
            &closing_trimmed_start[..closing_count],
        );
        if !trailing_after_marker.is_empty() {
            builder.token(SyntaxKind::WHITESPACE.into(), trailing_after_marker);
        }
        if !newline_str.is_empty() {
            builder.token(SyntaxKind::NEWLINE.into(), newline_str);
        }
    }

    builder.finish_node(); // DisplayMath
    current_pos
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::options::Dialect;

    #[test]
    fn test_backtick_fence() {
        let fence = try_parse_fence_open("```python", Dialect::Pandoc).unwrap();
        assert_eq!(fence.fence_char, '`');
        assert_eq!(fence.fence_count, 3);
        assert_eq!(fence.info_string, "python");
    }

    #[test]
    fn multiword_bare_info_is_not_a_fence_in_pandoc() {
        assert!(try_parse_fence_open("```haskell foo", Dialect::Pandoc).is_none());
        assert!(try_parse_fence_open("~~~haskell foo", Dialect::Pandoc).is_none());
        assert!(try_parse_fence_open("```@example foo bar", Dialect::Pandoc).is_none());
        assert!(try_parse_fence_open("```haskell ", Dialect::Pandoc).is_some());
        assert!(try_parse_fence_open("``` haskell", Dialect::Pandoc).is_some());
        assert!(try_parse_fence_open("```{.haskell .foo}", Dialect::Pandoc).is_some());
        assert!(try_parse_fence_open("```bash {filename=\"Terminal\"}", Dialect::Pandoc).is_some());
        assert!(try_parse_fence_open("```haskell {.numberLines}", Dialect::Pandoc).is_some());
        assert!(try_parse_fence_open("```haskell {.numberLines} foo", Dialect::Pandoc).is_none());
        assert!(try_parse_fence_open("```haskell foo {.x}", Dialect::Pandoc).is_none());
        assert!(try_parse_fence_open("```{.x} foo", Dialect::Pandoc).is_none());
    }

    #[test]
    fn multiword_bare_info_is_a_fence_in_commonmark() {
        let fence = try_parse_fence_open("```haskell foo", Dialect::CommonMark).unwrap();
        assert_eq!(fence.info_string, "haskell foo");
        assert!(try_parse_fence_open("~~~haskell foo", Dialect::CommonMark).is_some());
    }

    #[test]
    fn hashpipe_preamble_includes_blank_line_in_block_scalar() {
        let lines = [
            "#| fig-alt: |\n",
            "#|   First paragraph.\n",
            "#|\n",
            "#|   Second paragraph.\n",
            "plot(1)\n",
        ];
        assert_eq!(
            compute_hashpipe_preamble_line_count(&lines, "#|", 0, 0, false, 0, false),
            4
        );
    }

    #[test]
    fn hashpipe_blank_line_predicate() {
        assert!(is_hashpipe_blank_line("#|", "#|"));
        assert!(is_hashpipe_blank_line("#|   ", "#|"));
        assert!(!is_hashpipe_blank_line("#| key: v", "#|"));
        assert!(!is_hashpipe_blank_line("plot(1)", "#|"));
    }

    #[test]
    fn test_tilde_fence() {
        let fence = try_parse_fence_open("~~~", Dialect::Pandoc).unwrap();
        assert_eq!(fence.fence_char, '~');
        assert_eq!(fence.fence_count, 3);
        assert_eq!(fence.info_string, "");
    }

    #[test]
    fn test_long_fence() {
        let fence = try_parse_fence_open("`````", Dialect::Pandoc).unwrap();
        assert_eq!(fence.fence_count, 5);
    }

    #[test]
    fn test_two_backticks_invalid() {
        assert!(try_parse_fence_open("``", Dialect::Pandoc).is_none());
    }

    #[test]
    fn test_backtick_fence_with_backtick_in_info_is_invalid() {
        assert!(try_parse_fence_open("`````hi````there`````", Dialect::Pandoc).is_none());
    }

    #[test]
    fn test_closing_fence() {
        let fence = FenceInfo {
            fence_char: '`',
            fence_count: 3,
            info_string: String::new(),
        };
        assert!(is_closing_fence("```", &fence));
        assert!(is_closing_fence("````", &fence));
        assert!(!is_closing_fence("``", &fence));
        assert!(!is_closing_fence("~~~", &fence));
    }

    #[test]
    fn test_fenced_code_preserves_leading_gt() {
        let input = "```\n> foo\n```\n";
        let tree = crate::parse(input, None);
        assert_eq!(tree.text().to_string(), input);
    }

    #[test]
    fn test_fenced_code_in_blockquote_preserves_opening_fence_marker() {
        let input = "> ```\n> code\n> ```\n";
        let tree = crate::parse(input, None);
        assert_eq!(tree.text().to_string(), input);
    }

    #[test]
    fn test_fenced_code_in_definition_list_with_unicode_content_does_not_panic() {
        let input = "Term\n: ```\n├── pyproject.toml\n```\n";
        let tree = crate::parse(input, None);
        assert_eq!(tree.text().to_string(), input);
    }

    #[test]
    fn test_info_string_plain() {
        let info = InfoString::parse("");
        assert_eq!(info.block_type, CodeBlockType::Plain);
        assert!(info.attributes.is_empty());
    }

    #[test]
    fn test_info_string_shortcut() {
        let info = InfoString::parse("python");
        assert_eq!(
            info.block_type,
            CodeBlockType::DisplayShortcut {
                language: "python".to_string()
            }
        );
        assert!(info.attributes.is_empty());
    }

    #[test]
    fn test_info_string_shortcut_with_trailing() {
        let info = InfoString::parse("python extra stuff");
        assert_eq!(
            info.block_type,
            CodeBlockType::DisplayShortcut {
                language: "python".to_string()
            }
        );
    }

    #[test]
    fn test_info_string_display_explicit() {
        let info = InfoString::parse("{.python}");
        assert_eq!(
            info.block_type,
            CodeBlockType::DisplayExplicit {
                classes: vec!["python".to_string()]
            }
        );
    }

    #[test]
    fn test_info_string_display_explicit_multiple() {
        let info = InfoString::parse("{.python .numberLines}");
        assert_eq!(
            info.block_type,
            CodeBlockType::DisplayExplicit {
                classes: vec!["python".to_string(), "numberLines".to_string()]
            }
        );
    }

    #[test]
    fn test_info_string_executable() {
        let info = InfoString::parse("{python}");
        assert_eq!(
            info.block_type,
            CodeBlockType::Executable {
                language: "python".to_string()
            }
        );
    }

    #[test]
    fn test_info_string_executable_with_options() {
        let info = InfoString::parse("{python echo=false warning=true}");
        assert_eq!(
            info.block_type,
            CodeBlockType::Executable {
                language: "python".to_string()
            }
        );
        assert_eq!(info.attributes.len(), 2);
        assert_eq!(
            info.attributes[0],
            ("echo".to_string(), Some("false".to_string()))
        );
        assert_eq!(
            info.attributes[1],
            ("warning".to_string(), Some("true".to_string()))
        );
    }

    #[test]
    fn test_info_string_executable_with_commas() {
        let info = InfoString::parse("{r, echo=FALSE, warning=TRUE}");
        assert_eq!(
            info.block_type,
            CodeBlockType::Executable {
                language: "r".to_string()
            }
        );
        assert_eq!(info.attributes.len(), 2);
        assert_eq!(
            info.attributes[0],
            ("echo".to_string(), Some("FALSE".to_string()))
        );
        assert_eq!(
            info.attributes[1],
            ("warning".to_string(), Some("TRUE".to_string()))
        );
    }

    #[test]
    fn test_info_string_executable_mixed_commas_spaces() {
        let info = InfoString::parse("{r, echo=FALSE, label=\"my chunk\"}");
        assert_eq!(
            info.block_type,
            CodeBlockType::Executable {
                language: "r".to_string()
            }
        );
        assert_eq!(info.attributes.len(), 2);
        assert_eq!(
            info.attributes[0],
            ("echo".to_string(), Some("FALSE".to_string()))
        );
        assert_eq!(
            info.attributes[1],
            ("label".to_string(), Some("my chunk".to_string()))
        );
    }

    #[test]
    fn test_info_string_mixed_shortcut_and_attrs() {
        let info = InfoString::parse("python {.numberLines}");
        assert_eq!(
            info.block_type,
            CodeBlockType::DisplayShortcut {
                language: "python".to_string()
            }
        );
        assert_eq!(info.attributes.len(), 1);
        assert_eq!(info.attributes[0], (".numberLines".to_string(), None));
    }

    #[test]
    fn test_info_string_mixed_with_key_value() {
        let info = InfoString::parse("python {.numberLines startFrom=\"100\"}");
        assert_eq!(
            info.block_type,
            CodeBlockType::DisplayShortcut {
                language: "python".to_string()
            }
        );
        assert_eq!(info.attributes.len(), 2);
        assert_eq!(info.attributes[0], (".numberLines".to_string(), None));
        assert_eq!(
            info.attributes[1],
            ("startFrom".to_string(), Some("100".to_string()))
        );
    }

    #[test]
    fn test_info_string_explicit_with_id_and_classes() {
        let info = InfoString::parse("{#mycode .haskell .numberLines startFrom=\"100\"}");
        assert_eq!(
            info.block_type,
            CodeBlockType::DisplayExplicit {
                classes: vec!["haskell".to_string(), "numberLines".to_string()]
            }
        );
        let has_id = info.attributes.iter().any(|(k, _)| k == "#mycode");
        let has_start = info
            .attributes
            .iter()
            .any(|(k, v)| k == "startFrom" && v == &Some("100".to_string()));
        assert!(has_id);
        assert!(has_start);
    }

    #[test]
    fn test_info_string_raw_html() {
        let info = InfoString::parse("{=html}");
        assert_eq!(
            info.block_type,
            CodeBlockType::Raw {
                format: "html".to_string()
            }
        );
        assert!(info.attributes.is_empty());
    }

    #[test]
    fn test_info_string_raw_latex() {
        let info = InfoString::parse("{=latex}");
        assert_eq!(
            info.block_type,
            CodeBlockType::Raw {
                format: "latex".to_string()
            }
        );
    }

    #[test]
    fn test_info_string_raw_openxml() {
        let info = InfoString::parse("{=openxml}");
        assert_eq!(
            info.block_type,
            CodeBlockType::Raw {
                format: "openxml".to_string()
            }
        );
    }

    #[test]
    fn test_info_string_raw_ms() {
        let info = InfoString::parse("{=ms}");
        assert_eq!(
            info.block_type,
            CodeBlockType::Raw {
                format: "ms".to_string()
            }
        );
    }

    #[test]
    fn test_info_string_raw_html5() {
        let info = InfoString::parse("{=html5}");
        assert_eq!(
            info.block_type,
            CodeBlockType::Raw {
                format: "html5".to_string()
            }
        );
    }

    #[test]
    fn test_info_string_raw_not_combined_with_attrs() {
        let info = InfoString::parse("{=html .class}");
        assert_ne!(
            info.block_type,
            CodeBlockType::Raw {
                format: "html".to_string()
            }
        );
    }

    #[test]
    fn test_parse_pandoc_attributes_spaces() {
        let attrs = InfoString::parse_pandoc_attributes(".python .numberLines startFrom=\"10\"");
        assert_eq!(attrs.len(), 3);
        assert_eq!(attrs[0], (".python".to_string(), None));
        assert_eq!(attrs[1], (".numberLines".to_string(), None));
        assert_eq!(attrs[2], ("startFrom".to_string(), Some("10".to_string())));
    }

    #[test]
    fn test_parse_pandoc_attributes_no_commas() {
        let attrs = InfoString::parse_pandoc_attributes("#id .class key=value");
        assert_eq!(attrs.len(), 3);
        assert_eq!(attrs[0], ("#id".to_string(), None));
        assert_eq!(attrs[1], (".class".to_string(), None));
        assert_eq!(attrs[2], ("key".to_string(), Some("value".to_string())));
    }

    #[test]
    fn test_parse_chunk_options_commas() {
        let attrs = InfoString::parse_chunk_options("r, echo=FALSE, warning=TRUE");
        assert_eq!(attrs.len(), 3);
        assert_eq!(attrs[0], ("r".to_string(), None));
        assert_eq!(attrs[1], ("echo".to_string(), Some("FALSE".to_string())));
        assert_eq!(attrs[2], ("warning".to_string(), Some("TRUE".to_string())));
    }

    #[test]
    fn test_parse_chunk_options_no_spaces() {
        let attrs = InfoString::parse_chunk_options("r,echo=FALSE,warning=TRUE");
        assert_eq!(attrs.len(), 3);
        assert_eq!(attrs[0], ("r".to_string(), None));
        assert_eq!(attrs[1], ("echo".to_string(), Some("FALSE".to_string())));
        assert_eq!(attrs[2], ("warning".to_string(), Some("TRUE".to_string())));
    }

    #[test]
    fn test_parse_chunk_options_mixed() {
        let attrs = InfoString::parse_chunk_options("python echo=False, warning=True");
        assert_eq!(attrs.len(), 3);
        assert_eq!(attrs[0], ("python".to_string(), None));
        assert_eq!(attrs[1], ("echo".to_string(), Some("False".to_string())));
        assert_eq!(attrs[2], ("warning".to_string(), Some("True".to_string())));
    }

    #[test]
    fn test_parse_chunk_options_spaced_expressions() {
        for expression in [
            r#"x == "a""#,
            "n > 1",
            "n + 1 > 2",
            r#"x == "a, } ( b" && y != 'c, ]'"#,
            r#"grepl(")", x) && n > 1"#,
        ] {
            for separator in [", ", " ", "\t"] {
                let info = InfoString::parse(&format!(
                    "{{r example, eval = {expression}{separator}echo=FALSE}}"
                ));
                assert_eq!(
                    info.attributes,
                    vec![
                        ("label".to_string(), Some("example".to_string())),
                        ("eval".to_string(), Some(expression.to_string())),
                        ("echo".to_string(), Some("FALSE".to_string())),
                    ],
                    "{expression:?}, {separator:?}"
                );
            }
        }
    }

    #[test]
    fn test_parse_chunk_options_nested_function_call() {
        let attrs = InfoString::parse_chunk_options(r#"r pep-cg, dependson=c("foo", "bar")"#);
        assert_eq!(attrs.len(), 3);
        assert_eq!(attrs[0], ("r".to_string(), None));
        assert_eq!(attrs[1], ("pep-cg".to_string(), None));
        assert_eq!(
            attrs[2],
            (
                "dependson".to_string(),
                Some(r#"c("foo", "bar")"#.to_string())
            )
        );
    }

    #[test]
    fn test_parse_chunk_options_nested_with_spaces() {
        let attrs = InfoString::parse_chunk_options(r#"r, cache.path=file.path("cache", "dir")"#);
        assert_eq!(attrs.len(), 2);
        assert_eq!(attrs[0], ("r".to_string(), None));
        assert_eq!(
            attrs[1],
            (
                "cache.path".to_string(),
                Some(r#"file.path("cache", "dir")"#.to_string())
            )
        );
    }

    #[test]
    fn test_parse_chunk_options_deeply_nested() {
        let attrs = InfoString::parse_chunk_options(r#"r, x=list(a=c(1,2), b=c(3,4))"#);
        assert_eq!(attrs.len(), 2);
        assert_eq!(attrs[0], ("r".to_string(), None));
        assert_eq!(
            attrs[1],
            (
                "x".to_string(),
                Some(r#"list(a=c(1,2), b=c(3,4))"#.to_string())
            )
        );
    }

    #[test]
    fn test_parse_chunk_options_brackets_and_braces() {
        let attrs = InfoString::parse_chunk_options(r#"r, data=df[rows, cols], config={a:1, b:2}"#);
        assert_eq!(attrs.len(), 3);
        assert_eq!(attrs[0], ("r".to_string(), None));
        assert_eq!(
            attrs[1],
            ("data".to_string(), Some("df[rows, cols]".to_string()))
        );
        assert_eq!(
            attrs[2],
            ("config".to_string(), Some("{a:1, b:2}".to_string()))
        );
    }

    #[test]
    fn test_parse_chunk_options_quotes_with_parens() {
        let attrs = InfoString::parse_chunk_options(r#"r, label="test (with parens)", echo=TRUE"#);
        assert_eq!(attrs.len(), 3);
        assert_eq!(attrs[0], ("r".to_string(), None));
        assert_eq!(
            attrs[1],
            ("label".to_string(), Some("test (with parens)".to_string()))
        );
        assert_eq!(attrs[2], ("echo".to_string(), Some("TRUE".to_string())));
    }

    #[test]
    fn test_parse_chunk_options_escaped_quotes() {
        let attrs = InfoString::parse_chunk_options(r#"r, label="has \"quoted\" text""#);
        assert_eq!(attrs.len(), 2);
        assert_eq!(attrs[0], ("r".to_string(), None));
        assert_eq!(
            attrs[1],
            (
                "label".to_string(),
                Some(r#"has "quoted" text"#.to_string())
            )
        );
    }

    #[test]
    fn test_display_vs_executable_parsing() {
        let info1 = InfoString::parse("{.python .numberLines startFrom=\"10\"}");
        assert!(matches!(
            info1.block_type,
            CodeBlockType::DisplayExplicit { .. }
        ));

        let info2 = InfoString::parse("{r, echo=FALSE, warning=TRUE}");
        assert!(matches!(info2.block_type, CodeBlockType::Executable { .. }));
        assert_eq!(info2.attributes.len(), 2);
    }

    #[test]
    fn test_info_string_executable_implicit_label() {
        let info = InfoString::parse("{r mylabel}");
        assert!(matches!(
            info.block_type,
            CodeBlockType::Executable { ref language } if language == "r"
        ));
        assert_eq!(info.attributes.len(), 1);
        assert_eq!(
            info.attributes[0],
            ("label".to_string(), Some("mylabel".to_string()))
        );
    }

    #[test]
    fn test_info_string_executable_implicit_label_with_options() {
        let info = InfoString::parse("{r mylabel, echo=FALSE}");
        assert!(matches!(
            info.block_type,
            CodeBlockType::Executable { ref language } if language == "r"
        ));
        assert_eq!(info.attributes.len(), 2);
        assert_eq!(
            info.attributes[0],
            ("label".to_string(), Some("mylabel".to_string()))
        );
        assert_eq!(
            info.attributes[1],
            ("echo".to_string(), Some("FALSE".to_string()))
        );
    }

    #[test]
    fn test_compute_hashpipe_preamble_line_count_for_block_scalar() {
        let content_lines = vec![
            "#| fig-cap: |\n",
            "#|   A caption\n",
            "#|   spanning lines\n",
            "a <- 1\n",
        ];
        let count =
            compute_hashpipe_preamble_line_count(&content_lines, "#|", 0, 0, false, 0, false);
        assert_eq!(count, 3);
    }

    #[test]
    fn test_compute_hashpipe_preamble_line_count_stops_at_non_option() {
        let content_lines = vec!["#| label: fig-plot\n", "plot(1:10)\n", "#| echo: false\n"];
        let count =
            compute_hashpipe_preamble_line_count(&content_lines, "#|", 0, 0, false, 0, false);
        assert_eq!(count, 1);
    }

    #[test]
    fn test_compute_hashpipe_preamble_line_count_stops_at_standalone_prefix() {
        let content_lines = vec!["#| label: fig-plot\n", "#|\n", "plot(1:10)\n"];
        let count =
            compute_hashpipe_preamble_line_count(&content_lines, "#|", 0, 0, false, 0, false);
        assert_eq!(count, 1);
    }
}

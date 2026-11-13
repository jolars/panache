use crate::config::{Config, Flavor};
use crate::syntax::{
    AstNode, CellOptionResolution, CodeBlock, MystDirective, SyntaxKind, SyntaxNode, YamlMetadata,
    YamlNode, parse_yaml_document, text_without_line_prefixes,
};
use panache_parser::parser::blocks::code_blocks::{CodeBlockType, InfoString};
use rowan::NodeOrToken;
use std::collections::{BTreeMap, HashMap};

use super::Formatter;
use super::hashpipe;

impl Formatter {
    pub(super) fn format_block_code(&mut self, node: &SyntaxNode, indent: usize) {
        log::trace!("Formatting code block");
        if let Some(previous) = node.prev_sibling()
            && previous.kind() == SyntaxKind::PARAGRAPH
            && !self.output.ends_with("\n\n")
            && !self.output.ends_with("\n \n")
        {
            self.output.push('\n');
        }
        if let Some(directive) = node.parent().and_then(MystDirective::cast) {
            let directive_offset = self.myst_directive_fence_offset(directive.syntax());
            if is_unclosed_fenced_code_block(node) {
                // Raw fenced blocks already carry container prefixes. Replace
                // those prefixes once without changing the unfinished payload.
                let indent_str = " ".repeat(indent);
                for line in text_without_line_prefixes(node).split_inclusive('\n') {
                    self.output.push_str(&indent_str);
                    self.output
                        .push_str(strip_leading_spaces_budget(line, directive_offset));
                }
                return;
            }
            let is_fenced = node
                .children()
                .any(|child| child.kind() == SyntaxKind::CODE_FENCE_OPEN);
            // Moving the directive fence removes MyST's body dedent. Fenced
            // code already spends part of that budget on its own opener.
            let content_offset = self.container_content_offset(node) + directive_offset;
            let strip_content_columns = if is_fenced {
                let fence_indent: String = node
                    .children_with_tokens()
                    .take_while(|element| {
                        matches!(element, NodeOrToken::Token(token)
                            if matches!(token.kind(), SyntaxKind::WHITESPACE | SyntaxKind::LINE_PREFIX))
                    })
                    .filter_map(NodeOrToken::into_token)
                    .map(|token| token.text().to_string())
                    .collect();
                content_offset.saturating_sub(indent_columns(&fence_indent))
            } else {
                content_offset
            };
            let strip_content_columns =
                (strip_content_columns > 0).then_some(strip_content_columns);
            self.format_container_code_block(
                node,
                &" ".repeat(indent),
                indent,
                false,
                strip_content_columns,
                true,
            );
        } else {
            self.format_code_block(node);
        }
    }
}

pub type FormattedCodeMap = HashMap<usize, String>;

#[derive(Debug, Clone)]
pub struct ExternalCodeBlock {
    pub offset: usize,
    pub language: String,
    pub original: String,
    pub formatter_input: String,
    pub hashpipe_prefix: Option<String>,
    pub code_style: BTreeMap<String, String>,
}

/// Format a code block, normalizing fence markers and attributes based on config
pub(super) fn format_code_block(
    node: &SyntaxNode,
    config: &Config,
    formatted_code: &FormattedCodeMap,
    output: &mut String,
) {
    if is_unclosed_fenced_code_block(node) {
        output.push_str(&node.text().to_string());
        return;
    }

    let (info_node, _, extracted_content) = extract_code_block_parts(node);
    let mut content = extracted_content;
    if let Some(formatted) = formatted_code.get(&usize::from(node.text_range().start())) {
        content = expand_tabs_with_width(formatted, config.tab_width);
    }

    let info_node = match info_node {
        Some(node) => node,
        None => {
            let mut final_content = content;
            if !matches!(config.tab_stops, crate::config::TabStopMode::Preserve) {
                final_content = expand_tabs_with_width(&final_content, config.tab_width);
            }
            let fence_char = '`';
            let fence_length = determine_fence_length(&final_content, fence_char);
            output.push_str(&fence_char.to_string().repeat(fence_length));
            output.push('\n');
            output.push_str(&final_content);
            output.push_str(&fence_char.to_string().repeat(fence_length));
            output.push('\n');
            return;
        }
    };

    let info_string_raw = info_node.text().to_string();
    let info = InfoString::parse(&info_string_raw);

    let mut final_content = content;
    if !matches!(config.tab_stops, crate::config::TabStopMode::Preserve) {
        final_content = expand_tabs_with_width(&final_content, config.tab_width);
    }

    let fence_char = '`';

    let fence_length = determine_fence_length(&final_content, fence_char);

    let use_hashpipe = matches!(config.flavor, Flavor::Quarto | Flavor::RMarkdown)
        && matches!(&info.block_type, CodeBlockType::Executable { .. });

    if use_hashpipe
        && format_code_block_hashpipe(
            node,
            &info_node,
            &final_content,
            fence_char,
            fence_length,
            config,
            output,
        )
    {
        return;
    }

    let formatted_info = format_info_string(&info_node, &info);

    log::trace!("formatted_info = '{}'", formatted_info);

    for _ in 0..fence_length {
        output.push(fence_char);
    }
    if !formatted_info.is_empty() {
        output.push_str(&formatted_info);
    }
    output.push('\n');
    output.push_str(&final_content);
    for _ in 0..fence_length {
        output.push(fence_char);
    }
    output.push('\n');
}

fn is_unclosed_fenced_code_block(node: &SyntaxNode) -> bool {
    let has_open = node
        .children()
        .any(|child| child.kind() == SyntaxKind::CODE_FENCE_OPEN);
    let has_close = node
        .children()
        .any(|child| child.kind() == SyntaxKind::CODE_FENCE_CLOSE);

    has_open && !has_close
}

fn expand_tabs_with_width(text: &str, tab_width: usize) -> String {
    let mut out = String::with_capacity(text.len());
    let mut col = 0usize;
    for ch in text.chars() {
        match ch {
            '\t' => {
                let spaces = tab_width - (col % tab_width);
                out.push_str(&" ".repeat(spaces));
                col += spaces;
            }
            '\n' => {
                out.push('\n');
                col = 0;
            }
            _ => {
                out.push(ch);
                col += 1;
            }
        }
    }
    out
}

fn strip_indent_columns(indent: &str, columns: usize) -> String {
    strip_indent_columns_budget(indent, columns).0.to_string()
}

/// Peel up to `columns` columns of leading indentation off `indent`, returning
/// the tail and the columns left unspent.
///
/// The leftover matters because a code line's strippable indent can be split
/// across two tokens: the emitter peels a container prefix into `WHITESPACE`
/// but leaves the opening fence's own indent inside the line's `TEXT`, so a
/// budget that outlives the `WHITESPACE` still has work to do.
fn strip_indent_columns_budget(indent: &str, columns: usize) -> (&str, usize) {
    let mut remaining = columns;
    let mut idx = 0;
    for (i, ch) in indent.char_indices() {
        if remaining == 0 {
            break;
        }
        match ch {
            ' ' => {
                remaining -= 1;
                idx = i + 1;
            }
            '\t' => {
                remaining = remaining.saturating_sub(4);
                idx = i + 1;
            }
            _ => break,
        }
    }
    (&indent[idx..], remaining)
}

/// Spend a leftover indent budget against a code line's own leading spaces.
///
/// Spaces only: a tab is worth four columns at a four-column stop but fewer
/// mid-stop, and dropping one for a smaller budget would delete code bytes the
/// formatter has no way to put back. Leaving it is the lossless direction.
pub(super) fn strip_leading_spaces_budget(text: &str, columns: usize) -> &str {
    let n = text.chars().take(columns).take_while(|&c| c == ' ').count();
    &text[n..]
}

fn indent_columns(indent: &str) -> usize {
    let mut cols = 0usize;
    for ch in indent.chars() {
        match ch {
            ' ' => cols += 1,
            '\t' => cols += 4 - (cols % 4),
            _ => break,
        }
    }
    cols
}

/// A fenced code block whose immediate container is a blockquote — the one
/// place a line-start `WHITESPACE` inside `CODE_CONTENT` can be dropped
/// token-wise rather than by column.
///
/// The parser re-emits each continuation line's container prefix inside
/// `CODE_CONTENT` for losslessness. Inside a blockquote that prefix is either a
/// marker's padding or pandoc's lazy-line gobble, both of which the emitter
/// reproduces exactly, so the whole token is container syntax. Everywhere else
/// the token boundary is not column-exact once tabs are involved (a `:\t`
/// definition marker at content column 4 peels `"\t\t"`, eight columns), so the
/// indent has to come off by column via [`strip_indent_columns`] instead.
///
/// Mirrors `fenced_in_blockquote` in the pandoc-native projector
/// (`panache_parser::to_pandoc_ast`); the two must agree on what a code block's
/// payload is.
fn fenced_in_blockquote(node: &SyntaxNode, has_fence: bool) -> bool {
    has_fence && node.parent().map(|p| p.kind()) == Some(SyntaxKind::BLOCK_QUOTE)
}

fn extract_code_block_parts(node: &SyntaxNode) -> (Option<SyntaxNode>, Option<String>, String) {
    let mut info_node: Option<SyntaxNode> = None;
    let mut language: Option<String> = None;
    let mut content = String::new();
    let mut has_fence = false;
    let mut fence_indent = String::new();
    let mut fence_indent_cols = 0usize;
    let drop_container_indent = fenced_in_blockquote(
        node,
        node.children()
            .any(|child| child.kind() == SyntaxKind::CODE_FENCE_OPEN),
    );

    for child in node.children_with_tokens() {
        match child {
            NodeOrToken::Token(t) => {
                if matches!(t.kind(), SyntaxKind::WHITESPACE | SyntaxKind::LINE_PREFIX)
                    && !has_fence
                {
                    fence_indent.push_str(t.text());
                }
            }
            NodeOrToken::Node(n) => match n.kind() {
                SyntaxKind::CODE_FENCE_OPEN => {
                    has_fence = true;
                    fence_indent_cols = indent_columns(&fence_indent);
                    for child_token in n.children_with_tokens() {
                        if let NodeOrToken::Node(node) = child_token
                            && node.kind() == SyntaxKind::CODE_INFO
                        {
                            for info_token in node.children_with_tokens() {
                                if let NodeOrToken::Token(t) = info_token
                                    && t.kind() == SyntaxKind::CODE_LANGUAGE
                                {
                                    language = Some(t.text().to_string());
                                }
                            }
                            info_node = Some(node);
                        }
                    }
                }
                SyntaxKind::CODE_CONTENT => {
                    let base_indent_cols = if has_fence { fence_indent_cols } else { 4 };
                    let mut line_content = String::new();
                    let mut line_indent = String::new();
                    let mut at_line_start = true;
                    let mut saw_blockquote_marker = false;

                    for token in n.children_with_tokens() {
                        match token {
                            NodeOrToken::Token(t) => match t.kind() {
                                SyntaxKind::LINE_PREFIX
                                    if at_line_start && !t.text().trim().is_empty() =>
                                {
                                    saw_blockquote_marker = true;
                                }
                                SyntaxKind::WHITESPACE | SyntaxKind::LINE_PREFIX
                                    if at_line_start =>
                                {
                                    if drop_container_indent {
                                        saw_blockquote_marker = false;
                                    } else if saw_blockquote_marker {
                                        let ws = t.text();
                                        if let Some(stripped) = ws.strip_prefix(' ') {
                                            line_indent.push_str(stripped);
                                        } else {
                                            line_indent.push_str(ws);
                                        }
                                        saw_blockquote_marker = false;
                                    } else {
                                        line_indent.push_str(t.text());
                                    }
                                }
                                SyntaxKind::TEXT => {
                                    saw_blockquote_marker = false;
                                    if at_line_start && t.text().is_empty() {
                                        continue;
                                    }
                                    let mut text = t.text();
                                    if at_line_start {
                                        let (kept, budget) = strip_indent_columns_budget(
                                            &line_indent,
                                            base_indent_cols,
                                        );
                                        if kept.is_empty() && budget > 0 {
                                            text = strip_leading_spaces_budget(text, budget);
                                        }
                                        line_content.push_str(kept);
                                        line_indent.clear();
                                        at_line_start = false;
                                    }
                                    line_content.push_str(text);
                                }
                                SyntaxKind::NEWLINE => {
                                    saw_blockquote_marker = false;
                                    if !at_line_start {
                                        content.push_str(&line_content);
                                    }
                                    content.push('\n');
                                    line_content.clear();
                                    line_indent.clear();
                                    at_line_start = true;
                                }
                                _ => {}
                            },
                            NodeOrToken::Node(inner_node) => {
                                let node_text = inner_node.text().to_string();
                                if node_text.is_empty() {
                                    continue;
                                }
                                if at_line_start {
                                    line_content.push_str(&strip_indent_columns(
                                        &line_indent,
                                        base_indent_cols,
                                    ));
                                    line_indent.clear();
                                    at_line_start = false;
                                }
                                line_content.push_str(&node_text);
                            }
                        }
                    }

                    if !at_line_start {
                        content.push_str(&line_content);
                    }
                }
                _ => {}
            },
        }
    }

    (info_node, language, content)
}

/// Split container-stripped code-block `content` into its leading hashpipe
/// preamble (the `#|` header lines) and the remaining body, using the parser's
/// embedded `HASHPIPE_YAML_CONTENT` extent. The preamble is the first
/// `line_count` physical lines of `content` (container prefixes are already
/// stripped from both, so line counts line up). Returns `None` when the block
/// has no hashpipe preamble.
fn split_hashpipe_header(content: &str, code_block_node: &SyntaxNode) -> Option<(String, String)> {
    let line_count = hashpipe::hashpipe_preamble_line_count(code_block_node)?;
    let mut header_end = 0usize;
    for _ in 0..line_count {
        match content[header_end..].find('\n') {
            Some(rel) => header_end += rel + 1,
            None => {
                header_end = content.len();
                break;
            }
        }
    }
    Some((
        content[..header_end].to_string(),
        content[header_end..].to_string(),
    ))
}

pub(super) fn determine_fence_length(content: &str, fence_char: char) -> usize {
    let mut max_sequence = 0;
    let mut current_sequence = 0;

    for ch in content.chars() {
        if ch == fence_char {
            current_sequence += 1;
            max_sequence = max_sequence.max(current_sequence);
        } else if ch == '\n' || ch == '\r' || current_sequence > 0 {
            current_sequence = 0;
        }
    }

    (max_sequence + 1).max(3)
}

/// One entry in an executable fence's `{language ...}` info string, in the
/// order it appears. Class/Id sit on the fence as bare `.foo`/`#foo`
/// attributes; Label and KeyValue are the comma-list members.
#[derive(Debug, Clone)]
pub(super) enum ChunkOptionRepr {
    /// `.foo` attribute (text includes the leading `.`).
    Class(String),
    /// `#foo` attribute (text includes the leading `#`).
    Id(String),
    /// Bareword label, e.g. `mylabel` in `{r mylabel}`.
    Label(String),
    /// `key=value` (with or without quotes).
    KeyValue {
        key: String,
        value: String,
        is_quoted: bool,
    },
}

/// Extract chunk options from CST CHUNK_OPTIONS node, preserving order.
fn extract_chunk_options_from_cst(info_node: &SyntaxNode) -> Vec<ChunkOptionRepr> {
    use crate::syntax::{ChunkInfoItem, CodeInfo};

    let Some(info) = CodeInfo::cast(info_node.clone()) else {
        return Vec::new();
    };

    let mut options = Vec::new();
    let mut pending_label_parts = Vec::new();
    for item in info.chunk_items() {
        match item {
            ChunkInfoItem::Label(label) => {
                let value = label.text();
                if !value.is_empty() {
                    pending_label_parts.push(value);
                }
            }
            ChunkInfoItem::Class(class) => {
                let value = class.text();
                if !value.is_empty() {
                    options.push(ChunkOptionRepr::Class(value));
                }
            }
            ChunkInfoItem::Id(id) => {
                let value = id.text();
                if !value.is_empty() {
                    options.push(ChunkOptionRepr::Id(value));
                }
            }
            ChunkInfoItem::Option(option) => {
                if !pending_label_parts.is_empty() {
                    options.push(ChunkOptionRepr::Label(pending_label_parts.join(" ")));
                    pending_label_parts.clear();
                }
                if let (Some(key), Some(value)) = (option.key(), option.value()) {
                    options.push(ChunkOptionRepr::KeyValue {
                        key,
                        value,
                        is_quoted: option.is_quoted(),
                    });
                }
            }
        }
    }

    if !pending_label_parts.is_empty() {
        options.push(ChunkOptionRepr::Label(pending_label_parts.join(" ")));
    }

    options
}

/// Render the contents of an executable fence's `{...}` info string for the
/// given language and (possibly empty) option list. Class/id attributes are
/// emitted space-separated immediately after the language (pandoc canonical
/// shape — `{python .marimo .cell-code}`). Labels and `key=value` pairs are
/// emitted comma-separated after a `, ` separator (Quarto chunk-option
/// style — `{r, label="x", echo=false}`).
pub(super) fn render_executable_info(language: &str, options: &[ChunkOptionRepr]) -> String {
    let mut attr_parts = Vec::new();
    let mut option_parts = Vec::new();
    for option in options {
        match option {
            ChunkOptionRepr::Class(text) | ChunkOptionRepr::Id(text) => {
                attr_parts.push(text.clone());
            }
            ChunkOptionRepr::Label(text) => option_parts.push(text.clone()),
            ChunkOptionRepr::KeyValue {
                key,
                value,
                is_quoted,
            } => {
                if *is_quoted {
                    let quote = if value.contains('"') && !value.contains('\'') {
                        '\''
                    } else {
                        '"'
                    };
                    option_parts.push(format!("{}={}{}{}", key, quote, value, quote));
                } else {
                    option_parts.push(format!("{}={}", key, value));
                }
            }
        }
    }

    let mut out = String::from("{");
    out.push_str(language);
    if !attr_parts.is_empty() {
        out.push(' ');
        out.push_str(&attr_parts.join(" "));
    }
    if !option_parts.is_empty() {
        out.push_str(", ");
        out.push_str(&option_parts.join(", "));
    }
    out.push('}');
    out
}

fn format_info_string(info_node: &SyntaxNode, info: &InfoString) -> String {
    log::trace!(
        "format_info_string: block_type={:?}, raw='{}'",
        info.block_type,
        info.raw
    );
    match &info.block_type {
        CodeBlockType::Plain => {
            if info.attributes.is_empty() {
                String::new()
            } else {
                format!("{{{}}}", format_attributes(&info.attributes, false))
            }
        }
        CodeBlockType::DisplayShortcut { language } => {
            if info.attributes.is_empty() {
                info.raw.trim().to_string()
            } else {
                format!(
                    "{} {{{}}}",
                    language,
                    format_attributes(&info.attributes, false)
                )
            }
        }
        CodeBlockType::DisplayExplicit { classes } => {
            if let Some(first_class) = classes.first() {
                if info.attributes.is_empty() && classes.len() == 1 {
                    first_class.clone()
                } else {
                    let mut attrs: Vec<String> =
                        classes.iter().skip(1).map(|c| format!(".{}", c)).collect();
                    attrs.extend(info.attributes.iter().map(|(k, v)| {
                        if let Some(val) = v {
                            format!("{}=\"{}\"", k, val)
                        } else {
                            k.clone()
                        }
                    }));
                    if attrs.is_empty() {
                        first_class.clone()
                    } else {
                        format!("{} {{{}}}", first_class, attrs.join(" "))
                    }
                }
            } else {
                if info.attributes.is_empty() {
                    String::new()
                } else {
                    format!("{{{}}}", format_attributes(&info.attributes, false))
                }
            }
        }
        CodeBlockType::Executable { language } => {
            let options = extract_chunk_options_from_cst(info_node);
            render_executable_info(language, &options)
        }
        CodeBlockType::Raw { format } => {
            format!("{{={}}}", format)
        }
    }
}

/// Format a code block using Quarto hashpipe style for executable chunks.
///
/// Converts simple inline options to hashpipe format with YAML syntax,
/// while keeping complex expressions in the inline position.
/// If the language's comment syntax is unknown, returns false to fall back to inline format.
fn format_code_block_hashpipe(
    code_block_node: &SyntaxNode,
    info_node: &SyntaxNode,
    content: &str,
    fence_char: char,
    fence_length: usize,
    config: &Config,
    output: &mut String,
) -> bool {
    let info = InfoString::parse(&info_node.text().to_string());
    let language = match &info.block_type {
        CodeBlockType::Executable { language } => language,
        _ => unreachable!("hashpipe only for executable chunks"),
    };

    let Some(comment_prefix) = hashpipe::get_comment_prefix(language) else {
        return false; // Unknown language - fall back to inline format
    };
    let ((simple, complex), had_content_hashpipe) =
        hashpipe::split_options_from_cst_with_content(info_node, code_block_node);

    let hashpipe_lines = match hashpipe::format_as_hashpipe(
        language,
        &simple,
        config.line_width,
        config.wrap.as_ref(),
    ) {
        Some(lines) => lines,
        None => return false, // Unknown language - fall back to inline format
    };

    for _ in 0..fence_length {
        output.push(fence_char);
    }
    output.push_str(&render_executable_info(language, &complex));
    output.push('\n');

    for line in &hashpipe_lines {
        output.push_str(line);
        output.push('\n');
    }

    let body = if had_content_hashpipe {
        split_hashpipe_header(content, code_block_node)
            .map(|(_header, body)| body)
            .unwrap_or_else(|| content.to_string())
    } else {
        content.to_string()
    };

    if !hashpipe_lines.is_empty() {
        let body_without_leading_blanks = strip_leading_blank_lines(&body);
        let (body_without_marker_separators, had_marker_separator) =
            strip_leading_hashpipe_blank_markers(body_without_leading_blanks, comment_prefix);
        if !body_without_marker_separators.trim().is_empty()
            && (had_marker_separator || !body_without_marker_separators.starts_with(comment_prefix))
        {
            output.push('\n');
        }
        output.push_str(body_without_marker_separators);
    } else {
        output.push_str(&body);
    }

    for _ in 0..fence_length {
        output.push(fence_char);
    }
    output.push('\n');

    true // Successfully formatted as hashpipe
}

fn strip_leading_blank_lines(content: &str) -> &str {
    let mut idx = 0usize;

    while idx < content.len() {
        let rest = &content[idx..];
        let Some(line_end) = rest.find('\n') else {
            if rest.trim().is_empty() {
                return "";
            }
            break;
        };

        let line = &rest[..=line_end];
        let line_without_newline = line.trim_end_matches(['\r', '\n']);
        if line_without_newline.trim().is_empty() {
            idx += line_end + 1;
            continue;
        }

        break;
    }

    &content[idx..]
}

fn strip_leading_hashpipe_blank_markers<'a>(content: &'a str, prefix: &str) -> (&'a str, bool) {
    let mut idx = 0usize;
    let mut consumed = false;

    while idx < content.len() {
        let rest = &content[idx..];
        let Some(line_end) = rest.find('\n') else {
            let trimmed = rest.trim_start_matches([' ', '\t']).trim_end_matches('\r');
            if trimmed == prefix {
                consumed = true;
                idx = content.len();
            }
            break;
        };

        let line = &rest[..line_end];
        let trimmed = line.trim_start_matches([' ', '\t']).trim_end_matches('\r');
        if trimmed == prefix {
            consumed = true;
            idx += line_end + 1;
            continue;
        }
        break;
    }

    (&content[idx..], consumed)
}

fn format_attributes(attrs: &[(String, Option<String>)], preserve_unquoted: bool) -> String {
    let separator = if preserve_unquoted {
        ", " // Executable chunks use commas
    } else {
        " " // Display blocks use spaces
    };

    attrs
        .iter()
        .map(|(k, v)| {
            if let Some(val) = v {
                if preserve_unquoted {
                    let needs_quotes = (val.contains(' ') || val.contains(','))
                        && !val.contains('(')
                        && !val.contains('[')
                        && !val.contains('{');

                    if needs_quotes {
                        let escaped_val = val.replace('\\', "\\\\").replace('"', "\\\"");
                        format!("{}=\"{}\"", k, escaped_val)
                    } else {
                        format!("{}={}", k, val)
                    }
                } else {
                    let escaped_val = val.replace('\\', "\\\\").replace('"', "\\\"");
                    format!("{}=\"{}\"", k, escaped_val)
                }
            } else {
                k.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(separator)
}

/// Extract the language (directive argument) and verbatim body text from a
/// code directive. Other opaque directive bodies are not source code. The body
/// text is taken byte-for-byte so it matches the lookup key used when
/// substituting formatted output.
pub(crate) fn extract_myst_directive_parts(node: &SyntaxNode) -> Option<(String, String)> {
    let directive = MystDirective::cast(node.clone())?;
    if !directive.is_code() {
        return None;
    }
    let argument = directive.argument().unwrap_or_default();
    let language = argument.strip_prefix('.').unwrap_or(&argument).to_string();
    Some((language, directive.body()?))
}

/// Collect all code blocks from the syntax tree for external formatting.
pub fn collect_code_blocks(
    tree: &SyntaxNode,
    _input: &str,
    config: &Config,
) -> Vec<ExternalCodeBlock> {
    let mut result = Vec::new();
    let document_code_style = document_code_style(tree);
    for node in tree.descendants() {
        if node.kind() == SyntaxKind::MYST_DIRECTIVE {
            if let Some((language, content)) = extract_myst_directive_parts(&node) {
                if content.is_empty() {
                    continue;
                }
                if language.is_empty() && !config.formatters.contains_key("") {
                    continue;
                }
                result.push(ExternalCodeBlock {
                    offset: usize::from(node.text_range().start()),
                    language,
                    original: content.clone(),
                    formatter_input: content,
                    hashpipe_prefix: None,
                    code_style: document_code_style.clone(),
                });
            }
            continue;
        }

        if node.kind() != SyntaxKind::CODE_BLOCK {
            continue;
        }

        let (info_node, language, content) = extract_code_block_parts(&node);
        if content.is_empty() {
            continue;
        }

        let info = info_node
            .as_ref()
            .map(|n| InfoString::parse(&n.text().to_string()))
            .unwrap_or_else(|| InfoString::parse(""));

        let language = language.unwrap_or_else(|| match &info.block_type {
            CodeBlockType::DisplayShortcut { language }
            | CodeBlockType::Executable { language } => language.clone(),
            CodeBlockType::DisplayExplicit { classes } => {
                classes.first().cloned().unwrap_or_default()
            }
            _ => String::new(),
        });

        if language.is_empty() && !config.formatters.contains_key("") {
            continue;
        }

        let mut code_style = document_code_style.clone();
        code_style.extend(code_style_options(&node, &info));
        let (formatter_input, hashpipe_prefix) =
            if matches!(config.flavor, Flavor::Quarto | Flavor::RMarkdown)
                && matches!(info.block_type, CodeBlockType::Executable { .. })
                && hashpipe::get_comment_prefix(&language).is_some()
            {
                split_hashpipe_header(&content, &node)
                    .map(|(header, body)| (body, Some(header)))
                    .unwrap_or_else(|| (content.clone(), None))
            } else {
                (content.clone(), None)
            };

        result.push(ExternalCodeBlock {
            offset: usize::from(node.text_range().start()),
            language,
            original: content,
            formatter_input,
            hashpipe_prefix,
            code_style,
        });
    }

    result
}

fn document_code_style(tree: &SyntaxNode) -> BTreeMap<String, String> {
    tree.children()
        .filter_map(YamlMetadata::cast)
        .filter_map(|metadata| metadata.document())
        .filter_map(|document| document.block_map())
        .flat_map(|mapping| mapping.entries())
        .filter(|entry| entry.key_text().as_deref() == Some("code-style"))
        .map(|entry| {
            entry
                .value()
                .and_then(|value| value.as_node())
                .map(|value| code_style_map(&value))
                .unwrap_or_default()
        })
        .last()
        .unwrap_or_default()
}

fn code_style_options(node: &SyntaxNode, info: &InfoString) -> BTreeMap<String, String> {
    if let Some(cell) = CodeBlock::cast(node.clone()).and_then(|block| block.executable_cell()) {
        for option in cell.resolved_options() {
            if option.key() != "code-style" {
                continue;
            }
            if let CellOptionResolution::Resolved(declaration) = option.resolution() {
                if let Some(value) = declaration.yaml_value() {
                    return code_style_map(value);
                }
                if let Some(value) = declaration.cooked_value() {
                    return parse_code_style_map(value);
                }
            }
            return BTreeMap::new();
        }
    }

    info.attributes
        .iter()
        .find(|(key, _)| key == "code-style")
        .and_then(|(_, value)| value.as_deref())
        .map(parse_code_style_map)
        .unwrap_or_default()
}

fn parse_code_style_map(value: &str) -> BTreeMap<String, String> {
    parse_yaml_document(value)
        .and_then(|document| document.as_node())
        .map(|node| code_style_map(&node))
        .unwrap_or_default()
}

fn code_style_map(value: &YamlNode) -> BTreeMap<String, String> {
    let entries: Vec<(Option<String>, Option<String>)> = match value {
        YamlNode::BlockMap(map) => map
            .entries()
            .map(|entry| {
                (
                    entry.key_text(),
                    entry
                        .value()
                        .and_then(|value| value.as_scalar())
                        .map(|value| value.value()),
                )
            })
            .collect(),
        YamlNode::FlowMap(map) => map
            .entries()
            .map(|entry| {
                (
                    entry.key_text(),
                    entry
                        .value()
                        .and_then(|value| value.as_scalar())
                        .map(|value| value.value()),
                )
            })
            .collect(),
        _ => return BTreeMap::new(),
    };

    entries
        .into_iter()
        .filter_map(|(key, value)| {
            let (key, value) = (key?, value?);
            let key = key.trim().to_ascii_lowercase().replace('_', "-");
            if matches!(key.as_str(), "line-width" | "indent-width")
                && value
                    .parse::<usize>()
                    .ok()
                    .filter(|width| *width > 0)
                    .is_none()
            {
                return None;
            }
            Some((key, value))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{collect_code_blocks, split_hashpipe_header};
    use crate::config::{Config, Extensions, Flavor, ParserOptions};
    use crate::syntax::SyntaxKind;
    use std::collections::BTreeMap;

    #[test]
    fn document_code_style_defaults_merge_with_each_block() {
        let input = "---\ncode-style:\n  line-width: 40\n  indent-width: 2\n---\n\n```{r}\nx <- 1\n```\n\n```{r}\n#| code-style: {line-width: 80}\n\nx <- 2\n```\n\n```{.r code-style=\"{indent-width: 4}\"}\nx <- 3\n```\n";
        let config = Config {
            flavor: Flavor::Quarto,
            parser_extensions: Extensions::for_flavor(Flavor::Quarto),
            ..Default::default()
        };
        let tree = crate::parser::parse(
            input,
            Some(ParserOptions {
                flavor: config.flavor,
                extensions: config.parser_extensions.clone(),
                ..Default::default()
            }),
        );

        let blocks = collect_code_blocks(&tree, input, &config);
        assert_eq!(blocks.len(), 3);
        assert_eq!(
            blocks[0].code_style,
            BTreeMap::from([
                ("line-width".to_string(), "40".to_string()),
                ("indent-width".to_string(), "2".to_string()),
            ])
        );
        assert_eq!(blocks[1].code_style["line-width"], "80");
        assert_eq!(blocks[1].code_style["indent-width"], "2");
        assert_eq!(blocks[2].code_style["line-width"], "40");
        assert_eq!(blocks[2].code_style["indent-width"], "4");
    }

    #[test]
    fn last_document_code_style_field_applies_to_every_block() {
        let input = "---\ncode-style:\n  line-width: 40\n  indent-width: 2\n---\n\n```r\nx <- 1\n```\n\n---\ncode-style:\n  line-width: 80\n---\n\n```r\nx <- 2\n```\n";
        let config = Config {
            flavor: Flavor::Pandoc,
            parser_extensions: Extensions::for_flavor(Flavor::Pandoc),
            ..Default::default()
        };
        let tree = crate::parser::parse(
            input,
            Some(ParserOptions {
                flavor: config.flavor,
                extensions: config.parser_extensions.clone(),
                ..Default::default()
            }),
        );

        let blocks = collect_code_blocks(&tree, input, &config);
        assert_eq!(blocks.len(), 2);
        for block in blocks {
            assert_eq!(
                block.code_style,
                BTreeMap::from([("line-width".to_string(), "80".to_string())])
            );
        }
    }

    #[test]
    fn repeated_code_style_key_in_one_metadata_block_uses_last_value() {
        let input = "---\ncode-style: {line-width: 40}\ncode-style:\n  line-width: 60\n  line-width: 80\n---\n\n```r\nx <- 1\n```\n";
        let config = Config {
            flavor: Flavor::Pandoc,
            parser_extensions: Extensions::for_flavor(Flavor::Pandoc),
            ..Default::default()
        };
        let tree = crate::parser::parse(
            input,
            Some(ParserOptions {
                flavor: config.flavor,
                extensions: config.parser_extensions.clone(),
                ..Default::default()
            }),
        );

        let blocks = collect_code_blocks(&tree, input, &config);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].code_style["line-width"], "80");
    }

    #[test]
    fn split_hashpipe_header_handles_empty_value_with_indented_list() {
        let input = "```{r}\n#| fig-cap:\n#|   - A\n#|   - B\n```\n";
        let options = ParserOptions {
            flavor: Flavor::Quarto,
            extensions: Extensions::for_flavor(Flavor::Quarto),
            ..Default::default()
        };
        let tree = crate::parser::parse(input, Some(options));
        let code_block = tree
            .descendants()
            .find(|node| node.kind() == SyntaxKind::CODE_BLOCK)
            .expect("code block");

        let content = "#| fig-cap:\n#|   - A\n#|   - B\n";
        let split = split_hashpipe_header(content, &code_block);
        assert!(split.is_some(), "expected hashpipe header split");
        let (header, body) = split.unwrap();
        assert_eq!(header, content);
        assert_eq!(body, "");
    }
}

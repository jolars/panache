use crate::directives::extract_directive_from_node;
use crate::formatter::Formatter;
use crate::syntax::{
    AstNode, MystDirective, SyntaxKind, SyntaxNode, TomlMetadata, text_without_line_prefixes,
};
use rowan::NodeOrToken;

use super::code_blocks::{determine_fence_length, strip_leading_spaces_budget};
use super::math;

impl Formatter {
    /// Format an HTML block, optionally moving its container indentation.
    ///
    /// An HTML block that continues a list item or definition keeps its source
    /// indentation in its text. When the formatter moves the container's content
    /// column (normalizing `-   ` to `- `, say), `reindent = Some((from, to))`
    /// swaps the source column `from` for the formatted column `to` on body
    /// lines. The first line skips this adjustment when it follows the list
    /// marker, whose indentation has already been emitted. Lines that don't
    /// reach `from`, and ignored regions, keep the source text.
    pub(super) fn format_html_block_reindented(
        &mut self,
        node: &SyntaxNode,
        reindent: Option<(usize, usize)>,
    ) {
        self.process_format_directive(node);

        let mut text = String::new();
        let mut after_marker = false;
        for element in node.descendants_with_tokens() {
            let NodeOrToken::Token(token) = element else {
                continue;
            };
            if token.kind() == SyntaxKind::LINE_PREFIX {
                // Raw blocks have no parsed inner containers. Their entire
                // prefix belongs to the enclosing container, which emits it.
                if node.kind() == SyntaxKind::HTML_BLOCK_RAW
                    || self.config.dialect() == panache_parser::Dialect::CommonMark
                {
                    continue;
                }
                if token.text().contains('>') {
                    after_marker = true;
                } else if after_marker {
                    after_marker = false;
                } else {
                    text.push_str(token.text());
                }
                continue;
            }
            after_marker = false;
            text.push_str(token.text());
        }
        // A prefix-only final line still belongs to the raw block without a
        // source newline; the enclosing container must emit that blank line.
        if text.ends_with('\n')
            && node
                .last_token()
                .is_some_and(|token| token.kind() == SyntaxKind::LINE_PREFIX)
        {
            text.push('\n');
        }
        if !self.directive_tracker.is_formatting_ignored()
            && let Some(directive) = node.parent().and_then(MystDirective::cast)
        {
            let offset = self.myst_directive_fence_offset(directive.syntax());
            if offset > 0 {
                text = text
                    .split_inclusive('\n')
                    .map(|line| strip_leading_spaces_budget(line, offset))
                    .collect();
            }
        }
        let starts_on_marker_line = node
            .first_token()
            .and_then(|token| token.prev_token())
            .is_some_and(|prev| !prev.text().ends_with('\n'));
        if let Some((from, to)) = reindent
            && !self.directive_tracker.is_formatting_ignored()
            && let Some(moved) = reindent_lines(&text, from, to, starts_on_marker_line)
        {
            text = moved;
        }
        self.output.push_str(&text);
        if !text.ends_with('\n') {
            self.output.push('\n');
        }
    }

    pub(super) fn format_comment(&mut self, node: &SyntaxNode) {
        self.process_format_directive(node);
        let text = node.text().to_string();
        self.output.push_str(&text);
        if !text.ends_with('\n') {
            self.output.push('\n');
        }
    }

    pub(super) fn format_latex_command(&mut self, node: &SyntaxNode) {
        let formatted = math::format_latex_math_environment(node, &self.config)
            .unwrap_or_else(|| text_without_line_prefixes(node));
        self.output.push_str(&formatted);
    }

    pub(super) fn format_tex_block(&mut self, node: &SyntaxNode) {
        log::trace!("Formatting TeX block");
        for child in node.children_with_tokens() {
            if let NodeOrToken::Token(token) = child {
                self.output.push_str(token.text());
            }
        }
        if !self.output.ends_with('\n') {
            self.output.push('\n');
        }
    }

    fn process_format_directive(&mut self, node: &SyntaxNode) {
        let Some(directive) = extract_directive_from_node(node) else {
            return;
        };
        self.directive_tracker.process_directive(&directive);
        if matches!(directive, crate::directives::Directive::Start(_))
            && self.directive_tracker.is_formatting_ignored()
            && self.ignore_region_start.is_none()
        {
            self.ignore_region_start = Some(self.output.len());
        }
    }
}

/// Swap `from` columns of leading whitespace for `to` spaces on every
/// non-blank line of `text`, keeping any columns left by a partial tab.
/// `None` when a non-blank line doesn't reach `from`, so the caller keeps
/// the text as is.
fn reindent_lines(text: &str, from: usize, to: usize, skip_first_line: bool) -> Option<String> {
    let mut out = String::with_capacity(text.len());
    for (i, line) in text.split_inclusive('\n').enumerate() {
        if i == 0 && skip_first_line {
            out.push_str(line);
            continue;
        }
        let body = line.trim_end_matches(['\n', '\r']);
        let newline = &line[body.len()..];
        if body.trim().is_empty() {
            out.push_str(line);
            continue;
        }
        let mut col = 0usize;
        let mut bytes = 0usize;
        for b in body.bytes() {
            if col >= from {
                break;
            }
            match b {
                b' ' => col += 1,
                b'\t' => col = (col / 4 + 1) * 4,
                _ => break,
            }
            bytes += 1;
        }
        if col < from {
            return None;
        }
        out.push_str(&" ".repeat(to + col - from));
        out.push_str(&body[bytes..]);
        out.push_str(newline);
    }
    Some(out)
}

/// A single space after the closing colon makes valued MyST options reparse to
/// the same CST, while valueless options remain compact.
fn format_directive_option(node: &SyntaxNode) -> String {
    let mut name = String::new();
    let mut value = String::new();
    for element in node.children_with_tokens() {
        if let NodeOrToken::Token(token) = element {
            match token.kind() {
                SyntaxKind::MYST_DIRECTIVE_OPTION_NAME => name = token.text().to_string(),
                SyntaxKind::MYST_DIRECTIVE_OPTION_VALUE => value = token.text().to_string(),
                _ => {}
            }
        }
    }
    if value.is_empty() {
        format!(":{name}:")
    } else {
        format!(":{name}: {value}")
    }
}

fn format_directive_fence(node: &SyntaxNode, minimum_length: usize) -> String {
    let mut text = String::new();
    for element in node.children_with_tokens() {
        if let NodeOrToken::Token(token) = element {
            match token.kind() {
                SyntaxKind::MYST_DIRECTIVE_FENCE => {
                    let fence_char = token.text().chars().next().expect("directive fence");
                    text.push_str(
                        &fence_char
                            .to_string()
                            .repeat(minimum_length.max(token.text().len())),
                    );
                }
                SyntaxKind::NEWLINE | SyntaxKind::LINE_PREFIX => {}
                SyntaxKind::WHITESPACE if text.is_empty() => {}
                _ => text.push_str(token.text()),
            }
        }
    }
    text
}

impl Formatter {
    pub(super) fn myst_directive_fence_offset(&self, node: &SyntaxNode) -> usize {
        node.children()
            .find(|child| child.kind() == SyntaxKind::MYST_DIRECTIVE_OPEN)
            .map_or(0, |open| {
                open.children_with_tokens()
                    .filter_map(NodeOrToken::into_token)
                    .take_while(|token| token.kind() != SyntaxKind::MYST_DIRECTIVE_FENCE)
                    .filter(|token| token.kind() == SyntaxKind::WHITESPACE)
                    .map(|token| token.text().len())
                    .sum::<usize>()
                    .saturating_sub(self.container_content_offset(node))
            })
    }

    pub(super) fn format_myst_directive(&mut self, node: &SyntaxNode, indent: usize) {
        let mut open_node: Option<SyntaxNode> = None;
        let mut close_node: Option<SyntaxNode> = None;
        let mut options = Vec::new();
        let mut body = Vec::new();
        let mut verbatim_body: Option<SyntaxNode> = None;
        for element in node.children_with_tokens() {
            if let NodeOrToken::Node(child) = element {
                match child.kind() {
                    SyntaxKind::MYST_DIRECTIVE_OPEN => {
                        open_node = Some(child);
                    }
                    SyntaxKind::MYST_DIRECTIVE_CLOSE => {
                        close_node = Some(child);
                    }
                    SyntaxKind::MYST_DIRECTIVE_OPTION => options.push(child),
                    SyntaxKind::MYST_DIRECTIVE_BODY => verbatim_body = Some(child),
                    _ => body.push(child),
                }
            }
        }

        // Nested code blocks and directives may grow their fences while
        // formatting, so size the enclosing fence from the rendered body.
        let saved_output = std::mem::take(&mut self.output);
        // Include the opener's newline so blocks can request a separator
        // even when they are the first content in the scratch buffer.
        self.output.push('\n');
        let indent_str = " ".repeat(indent);
        // Literal content keeps the opener's offset because MyST removes that
        // offset when extracting the body, independently of container prefixes.
        let fence_offset = if verbatim_body.is_some() {
            self.myst_directive_fence_offset(node)
        } else {
            0
        };
        let fence_indent = " ".repeat(indent + fence_offset);

        for option in &options {
            self.output.push_str(&fence_indent);
            self.output.push_str(&format_directive_option(option));
            self.output.push('\n');
        }

        if let Some(body_node) = verbatim_body {
            let body_text = self
                .formatted_code
                .get(&usize::from(node.text_range().start()))
                .cloned()
                .unwrap_or_else(|| text_without_line_prefixes(&body_node));
            for line in body_text.split_inclusive('\n') {
                if line != "\n" {
                    self.output.push_str(&indent_str);
                }
                self.output.push_str(line);
            }
            if !self.output.ends_with('\n') {
                self.output.push('\n');
            }
        } else {
            let leading = body
                .iter()
                .take_while(|c| c.kind() == SyntaxKind::BLANK_LINE)
                .count();
            let trailing = body
                .iter()
                .rev()
                .take_while(|c| c.kind() == SyntaxKind::BLANK_LINE)
                .count();
            let end = body.len().saturating_sub(trailing).max(leading);

            if !options.is_empty() && leading < end {
                self.output.push('\n');
            }

            let mut prev_blank = false;
            for child in &body[leading..end] {
                if child.kind() == SyntaxKind::BLANK_LINE {
                    if !prev_blank {
                        self.output.push('\n');
                        prev_blank = true;
                    }
                    continue;
                }
                self.format_node_sync(child, indent);
                if !self.output.ends_with('\n') {
                    self.output.push('\n');
                }
                prev_blank = false;
            }
        }

        let rendered_body = std::mem::replace(&mut self.output, saved_output);
        let rendered_body = &rendered_body[1..];
        let minimum_length = open_node
            .as_ref()
            .and_then(|open| {
                open.children_with_tokens().find_map(|element| {
                    element
                        .into_token()
                        .filter(|token| token.kind() == SyntaxKind::MYST_DIRECTIVE_FENCE)
                })
            })
            .and_then(|fence| fence.text().chars().next())
            .map_or(3, |fence_char| {
                determine_fence_length(rendered_body, fence_char)
            });

        if let Some(open) = &open_node {
            self.output.push_str(&fence_indent);
            self.output
                .push_str(&format_directive_fence(open, minimum_length));
            self.output.push('\n');
        }
        self.output.push_str(rendered_body);

        if let Some(close) = &close_node {
            if !self.output.ends_with('\n') {
                self.output.push('\n');
            }
            self.output.push_str(&fence_indent);
            self.output
                .push_str(&format_directive_fence(close, minimum_length));
            self.output.push('\n');
        }
        self.consecutive_blank_lines = 0;
    }

    pub(super) fn format_raw_block(&mut self, node: &SyntaxNode) {
        let text = node.text().to_string();
        if TomlMetadata::cast(node.clone()).is_some_and(|metadata| !metadata.is_closed()) {
            self.output.push_str(&text);
        } else {
            self.output.push_str(text.trim_end_matches('\n'));
            self.output.push('\n');
        }
        self.consecutive_blank_lines = 0;
    }
}

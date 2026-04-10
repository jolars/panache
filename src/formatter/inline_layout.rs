use crate::config::Config;
use crate::formatter::sentence_wrap::{
    SentenceBoundaryClass, SentenceLanguage, SentenceSegment, is_sentence_boundary_segment,
    resolve_sentence_language,
};
use crate::syntax::{SyntaxKind, SyntaxNode};
use rowan::NodeOrToken;
use std::borrow::Cow;
use std::fmt::Write;
use unicode_width::UnicodeWidthStr;

/// Escape special characters in text to prevent ambiguous parsing.
///
/// # Arguments
/// * `text` - The text to escape
/// * `skip_emphasis_delim` - Whether to skip escaping * and _ (when direct child of EMPHASIS/STRONG)
/// * `prev_is_text` - Whether the previous token was TEXT (for intraword underscore detection)
/// * `next_is_text` - Whether the next token is TEXT (for intraword underscore detection)
fn escape_special_chars(
    text: &str,
    skip_emphasis_delim: bool,
    prev_is_text: bool,
    next_is_text: bool,
    escape_underscores: bool,
) -> String {
    let mut result = String::with_capacity(text.len() * 2);
    let is_single_underscore = text == "_";
    let mut chars = text.char_indices().peekable();

    while let Some((byte_idx, ch)) = chars.next() {
        match ch {
            '*' => {
                // Only escape asterisks when NOT a direct child of EMPHASIS/STRONG
                if !skip_emphasis_delim {
                    result.push('\\');
                }
                result.push(ch);
            }
            '_' => {
                // For underscores, only escape at word boundaries
                // Intraword underscores like foo_bar are left unescaped
                let at_start = byte_idx == 0;
                let at_end = chars.peek().is_none();

                // If the entire text is just "_", always escape it (not intraword)
                if is_single_underscore {
                    if !skip_emphasis_delim {
                        result.push('\\');
                    }
                    result.push(ch);
                    continue;
                }

                // If underscore is at start and previous token was TEXT, it's intraword
                let intraword_start = at_start && prev_is_text;
                // If underscore is at end and next token is TEXT, it's intraword
                let intraword_end = at_end && next_is_text;

                let is_intraword = intraword_start || intraword_end;

                if escape_underscores && !skip_emphasis_delim && !is_intraword {
                    result.push('\\');
                }
                result.push(ch);
            }
            // Escape special syntax characters
            '[' | ']' | '|' | '~' | '`' => {
                result.push('\\');
                result.push(ch);
            }
            '\\' => {
                // Keep backslash as-is
                result.push(ch);
            }
            _ => {
                result.push(ch);
            }
        }
    }

    result
}

fn expand_tabs_with_width<'a>(text: &'a str, tab_width: usize) -> Cow<'a, str> {
    if !text.contains('\t') {
        return Cow::Borrowed(text);
    }
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
    Cow::Owned(out)
}

fn starts_with_ascii_whitespace(text: &str) -> bool {
    text.chars().next().is_some_and(|c| c.is_ascii_whitespace())
}

fn ends_with_ascii_whitespace(text: &str) -> bool {
    text.chars()
        .next_back()
        .is_some_and(|c| c.is_ascii_whitespace())
}

fn append_normalized_link_dest(dest: &str, out: &mut String) {
    let dest_trimmed = dest.trim();
    let mut split_at = None;
    for (i, ch) in dest_trimmed.char_indices() {
        if ch.is_whitespace() {
            split_at = Some(i);
            break;
        }
    }

    let Some(split_at) = split_at else {
        out.push_str(dest_trimmed);
        return;
    };

    let (url, rest) = dest_trimmed.split_at(split_at);
    let title = rest.trim();
    if title.is_empty() {
        out.push_str(url);
        return;
    }

    out.push_str(url);
    out.push(' ');
    if title.starts_with('\'') && title.ends_with('\'') && title.len() >= 2 {
        out.push('"');
        out.push_str(&title[1..title.len() - 1]);
        out.push('"');
    } else {
        out.push_str(title);
    }
}

fn is_initialism_with_periods(word: &str) -> bool {
    if !word.ends_with('.') {
        return false;
    }
    let parts: Vec<&str> = word.split('.').collect();
    if parts.len() < 3 || !parts.last().is_some_and(|part| part.is_empty()) {
        return false;
    }
    parts[..parts.len() - 1]
        .iter()
        .all(|part| part.len() == 1 && part.chars().all(|c| c.is_ascii_uppercase()))
}

fn is_year_like(word: &str) -> bool {
    word.len() == 4 && word.chars().all(|c| c.is_ascii_digit())
}

fn normalize_inline_for_sentence<'a>(text: &'a str) -> Cow<'a, str> {
    if text.contains('\n') {
        Cow::Owned(text.replace('\n', " "))
    } else {
        Cow::Borrowed(text)
    }
}

fn should_merge_initialism_year(left: &str, left_ws_after: bool, right: &str) -> bool {
    left_ws_after && is_initialism_with_periods(left) && is_year_like(right)
}

#[derive(Clone, Copy)]
pub(super) enum NodeWrapMode {
    Reflow,
    Sentence,
}

#[derive(Clone, Copy)]
pub(super) enum WrapStrategy {
    ParagraphReflow,
    ParagraphSentence,
    ListReflow { in_blockquote: bool },
    ListSentence { in_blockquote: bool },
}

#[derive(Clone, Copy)]
pub(super) struct NodeWrapOptions<'a> {
    pub widths: &'a [usize],
    pub mode: NodeWrapMode,
    pub atomic_links_root: bool,
    pub strip_standalone_blockquote_markers: bool,
}

impl<'a> NodeWrapOptions<'a> {
    pub(super) fn reflow(widths: &'a [usize]) -> Self {
        Self {
            widths,
            mode: NodeWrapMode::Reflow,
            atomic_links_root: false,
            strip_standalone_blockquote_markers: false,
        }
    }

    pub(super) fn sentence() -> Self {
        Self {
            widths: &[],
            mode: NodeWrapMode::Sentence,
            atomic_links_root: true,
            strip_standalone_blockquote_markers: false,
        }
    }
}

impl WrapStrategy {
    fn options<'a>(self, widths: &'a [usize]) -> NodeWrapOptions<'a> {
        match self {
            Self::ParagraphReflow => NodeWrapOptions::reflow(widths),
            Self::ParagraphSentence => NodeWrapOptions::sentence(),
            Self::ListReflow { in_blockquote } => NodeWrapOptions {
                strip_standalone_blockquote_markers: in_blockquote,
                ..NodeWrapOptions::reflow(widths)
            },
            Self::ListSentence { in_blockquote } => NodeWrapOptions {
                strip_standalone_blockquote_markers: in_blockquote,
                ..NodeWrapOptions::sentence()
            },
        }
    }
}

struct StreamingCoreSink<'a> {
    default_line_width: usize,
    line_widths: &'a [usize],
    sentence_mode: bool,
    out: Vec<String>,
    line: String,
    line_width: usize,
    line_has_piece: bool,
    prev_ws_after: bool,
    pending_piece: Option<SentenceSegment>,
    strip_standalone_blockquote_markers: bool,
    merge_initialism_year: bool,
    sentence_language: SentenceLanguage,
}

impl<'a> StreamingCoreSink<'a> {
    fn new(
        line_widths: &'a [usize],
        sentence_mode: bool,
        strip_standalone_blockquote_markers: bool,
        merge_initialism_year: bool,
        sentence_language: SentenceLanguage,
    ) -> Self {
        Self {
            default_line_width: line_widths.last().copied().unwrap_or(0),
            line_widths,
            sentence_mode,
            out: Vec::new(),
            line: String::new(),
            line_width: 0,
            line_has_piece: false,
            prev_ws_after: false,
            pending_piece: None,
            strip_standalone_blockquote_markers,
            merge_initialism_year,
            sentence_language,
        }
    }

    fn consume(
        &mut self,
        segment: SentenceSegment,
        is_last: bool,
        next_segment: Option<&SentenceSegment>,
    ) {
        let piece_width = UnicodeWidthStr::width(segment.text.as_str());
        if !self.sentence_mode {
            let width_limit = self
                .line_widths
                .get(self.out.len())
                .copied()
                .unwrap_or(self.default_line_width);
            let spacer_width = usize::from(self.line_has_piece && self.prev_ws_after);
            if self.line_has_piece && self.line_width + spacer_width + piece_width > width_limit {
                self.out.push(std::mem::take(&mut self.line));
                self.line_width = 0;
                self.line_has_piece = false;
                self.prev_ws_after = false;
            }
        }
        if self.line_has_piece && self.prev_ws_after {
            self.line.push(' ');
            self.line_width += 1;
        }
        self.line.push_str(&segment.text);
        self.line_width += piece_width;
        self.line_has_piece = true;
        self.prev_ws_after = segment.has_whitespace_after;

        if self.sentence_mode
            && is_sentence_boundary_segment(&segment, next_segment, is_last, self.sentence_language)
        {
            self.out.push(std::mem::take(&mut self.line));
            self.line_width = 0;
            self.line_has_piece = false;
            self.prev_ws_after = false;
        }
    }

    fn emit_piece(&mut self, piece: String, ws_after: bool) {
        self.emit_piece_with_boundary(piece, ws_after, SentenceBoundaryClass::Normal);
    }

    fn emit_piece_with_boundary(
        &mut self,
        piece: String,
        ws_after: bool,
        boundary_class: SentenceBoundaryClass,
    ) {
        if self.strip_standalone_blockquote_markers && piece == ">" {
            return;
        }
        let incoming = SentenceSegment {
            text: piece,
            has_whitespace_after: ws_after,
            boundary_class,
        };
        if let Some(mut pending) = self.pending_piece.take() {
            if self.merge_initialism_year
                && should_merge_initialism_year(
                    &pending.text,
                    pending.has_whitespace_after,
                    &incoming.text,
                )
            {
                pending.text = format!("{} {}", pending.text, incoming.text);
                pending.has_whitespace_after = incoming.has_whitespace_after;
                pending.boundary_class = incoming.boundary_class;
                self.pending_piece = Some(pending);
                return;
            }
            self.consume(pending, false, Some(&incoming));
        }
        self.pending_piece = Some(incoming);
    }

    fn force_line_break(&mut self) {
        if let Some(pending) = self.pending_piece.take() {
            self.consume(pending, false, None);
        }
        self.out.push(std::mem::take(&mut self.line));
        self.line_width = 0;
        self.line_has_piece = false;
        self.prev_ws_after = false;
    }

    fn has_content_or_pending(&self) -> bool {
        self.line_has_piece || self.pending_piece.is_some()
    }

    fn finish(mut self) -> Vec<String> {
        if let Some(pending) = self.pending_piece.take() {
            self.consume(pending, true, None);
        }
        if self.line_has_piece {
            self.out.push(self.line);
        } else if self.out.is_empty() {
            self.out.push(String::new());
        }
        self.out
    }
}

pub(super) fn wrap_text_first_fit(text: &str, line_width: usize) -> Vec<String> {
    let words: Vec<&str> = text.split_ascii_whitespace().collect();
    let line_widths = [line_width];
    let mut sink =
        StreamingCoreSink::new(&line_widths, false, false, false, SentenceLanguage::English);
    for (idx, word) in words.iter().enumerate() {
        let ws_after = idx + 1 < words.len();
        sink.emit_piece((*word).to_string(), ws_after);
    }
    sink.finish()
}

fn node_starts_with_whitespace(node: &SyntaxNode) -> bool {
    for child in node.children_with_tokens() {
        match child {
            NodeOrToken::Token(t) if t.kind() == SyntaxKind::TEXT => {
                return t.text().starts_with(char::is_whitespace);
            }
            NodeOrToken::Token(t)
                if matches!(
                    t.kind(),
                    SyntaxKind::EMPHASIS_MARKER | SyntaxKind::STRONG_MARKER
                ) =>
            {
                continue;
            }
            NodeOrToken::Node(n) => {
                if node_starts_with_whitespace(&n) {
                    return true;
                }
            }
            _ => continue,
        }
    }
    false
}

fn append_link_closing(node: &SyntaxNode, out: &mut String) {
    let mut past_link_text = false;
    for child in node.children_with_tokens() {
        match child {
            NodeOrToken::Node(link_child) => match link_child.kind() {
                SyntaxKind::LINK_TEXT => past_link_text = true,
                SyntaxKind::LINK_DEST | SyntaxKind::LINK_REF | SyntaxKind::ATTRIBUTE => {
                    if past_link_text {
                        if link_child.kind() == SyntaxKind::LINK_DEST {
                            let raw = link_child.text().to_string();
                            append_normalized_link_dest(&raw, out);
                        } else {
                            let _ = write!(out, "{}", link_child.text());
                        }
                    }
                }
                _ => {}
            },
            NodeOrToken::Token(t) => {
                if past_link_text {
                    match t.kind() {
                        SyntaxKind::LINK_TEXT_END
                        | SyntaxKind::LINK_DEST_START
                        | SyntaxKind::LINK_DEST_END
                        | SyntaxKind::TEXT => out.push_str(t.text()),
                        _ => {}
                    }
                }
            }
        }
    }
}

fn append_image_closing(node: &SyntaxNode, out: &mut String) {
    let mut past_image_alt = false;
    for child in node.children_with_tokens() {
        match child {
            NodeOrToken::Node(img_child) => match img_child.kind() {
                SyntaxKind::IMAGE_ALT => past_image_alt = true,
                SyntaxKind::LINK_DEST | SyntaxKind::ATTRIBUTE | SyntaxKind::LINK_REF => {
                    if past_image_alt {
                        if img_child.kind() == SyntaxKind::LINK_DEST {
                            let raw = img_child.text().to_string();
                            append_normalized_link_dest(&raw, out);
                        } else {
                            let _ = write!(out, "{}", img_child.text());
                        }
                    }
                }
                _ => {}
            },
            NodeOrToken::Token(t) => {
                if past_image_alt {
                    match t.kind() {
                        SyntaxKind::IMAGE_ALT_END
                        | SyntaxKind::IMAGE_DEST_START
                        | SyntaxKind::IMAGE_DEST_END
                        | SyntaxKind::TEXT => out.push_str(t.text()),
                        _ => {}
                    }
                }
            }
        }
    }
}

struct TraversalBuilder<'a> {
    sink: StreamingCoreSink<'a>,
    current_piece: Option<String>,
    current_piece_boundary_class: SentenceBoundaryClass,
    pending_space: bool,
    skip_next_leading_whitespace: bool,
}

impl<'a> TraversalBuilder<'a> {
    fn new(
        line_widths: &'a [usize],
        sentence_mode: bool,
        strip_standalone_blockquote_markers: bool,
        sentence_language: SentenceLanguage,
    ) -> Self {
        Self {
            sink: StreamingCoreSink::new(
                line_widths,
                sentence_mode,
                strip_standalone_blockquote_markers,
                true,
                sentence_language,
            ),
            current_piece: None,
            current_piece_boundary_class: SentenceBoundaryClass::Normal,
            pending_space: false,
            skip_next_leading_whitespace: false,
        }
    }

    fn push_piece(&mut self, text: &str) {
        self.push_piece_with_boundary(text, SentenceBoundaryClass::Normal);
    }

    fn push_piece_with_boundary(&mut self, text: &str, boundary_class: SentenceBoundaryClass) {
        if self.pending_space {
            self.flush_current(true);
            self.current_piece = Some(text.to_string());
            self.current_piece_boundary_class = boundary_class;
            self.pending_space = false;
        } else if let Some(current) = &mut self.current_piece {
            current.push_str(text);
            self.current_piece_boundary_class = boundary_class;
        } else {
            self.current_piece = Some(text.to_string());
            self.current_piece_boundary_class = boundary_class;
        }
    }

    fn pending_space(&self) -> bool {
        self.pending_space
    }

    fn set_pending_space(&mut self, value: bool) {
        self.pending_space = value;
    }

    fn skip_next_leading_whitespace(&self) -> bool {
        self.skip_next_leading_whitespace
    }

    fn set_skip_next_leading_whitespace(&mut self, value: bool) {
        self.skip_next_leading_whitespace = value;
    }

    fn is_at_inline_footnote_open(&self) -> bool {
        self.current_piece
            .as_deref()
            .is_some_and(|piece| piece.ends_with("^["))
    }

    fn flush_current(&mut self, ws_after: bool) {
        if let Some(piece) = self.current_piece.take() {
            self.sink
                .emit_piece_with_boundary(piece, ws_after, self.current_piece_boundary_class);
            self.current_piece_boundary_class = SentenceBoundaryClass::Normal;
        }
    }

    fn finish(mut self) -> Vec<String> {
        self.flush_current(false);
        self.sink.finish()
    }

    fn push_verbatim_lines(&mut self, text: &str) {
        self.flush_current(false);
        let mut lines = text.lines().peekable();
        while let Some(line) = lines.next() {
            self.sink.emit_piece(line.to_string(), false);
            if lines.peek().is_some() {
                self.sink.force_line_break();
            }
        }
    }

    fn push_verbatim_block(&mut self, text: &str) {
        self.set_pending_space(false);
        self.flush_current(false);
        if self.sink.has_content_or_pending() {
            self.sink.force_line_break();
        }
        self.push_verbatim_lines(text);
        if self.sink.has_content_or_pending() {
            self.sink.force_line_break();
        }
    }

    fn push_hard_line_break(&mut self, marker: &str) {
        self.set_pending_space(false);
        self.flush_current(false);
        if !marker.is_empty() {
            self.sink.emit_piece(marker.to_string(), false);
        }
        self.sink.force_line_break();
    }
}

fn process_node_recursive(
    config: &Config,
    node: &SyntaxNode,
    sink: &mut TraversalBuilder<'_>,
    format_inline_fn: &dyn Fn(&SyntaxNode) -> String,
    in_link_text: bool,
    atomic_links: bool,
    in_inline_footnote: bool,
) {
    let mut children = node.children_with_tokens().peekable();
    let mut prev_is_text = false;
    let mut skip_marker_whitespace = false;
    while let Some(el) = children.next() {
        let current_is_text = matches!(&el, NodeOrToken::Token(t) if t.kind() == SyntaxKind::TEXT);
        let next_is_text = matches!(
            children.peek(),
            Some(NodeOrToken::Token(tok)) if tok.kind() == SyntaxKind::TEXT
        );
        match el {
            NodeOrToken::Token(t) => match t.kind() {
                SyntaxKind::HARD_LINE_BREAK => {
                    skip_marker_whitespace = false;
                    let marker = if config.extensions.escaped_line_breaks {
                        "\\"
                    } else {
                        t.text().trim_end_matches(['\r', '\n'])
                    };
                    sink.push_hard_line_break(marker);
                }
                SyntaxKind::WHITESPACE | SyntaxKind::NEWLINE | SyntaxKind::BLANK_LINE => {
                    if skip_marker_whitespace {
                        skip_marker_whitespace = false;
                        continue;
                    }
                    if in_inline_footnote && sink.is_at_inline_footnote_open() {
                        continue;
                    }
                    sink.set_pending_space(true);
                }
                SyntaxKind::INLINE_FOOTNOTE_START | SyntaxKind::INLINE_FOOTNOTE_END => {
                    skip_marker_whitespace = false;
                    if !in_inline_footnote {
                        sink.push_piece(t.text());
                    }
                }
                SyntaxKind::BLOCK_QUOTE_MARKER => {
                    skip_marker_whitespace = true;
                }
                SyntaxKind::ESCAPED_CHAR => {
                    skip_marker_whitespace = false;
                    if in_link_text && t.text() == r"\_" {
                        sink.push_piece("_");
                    } else {
                        sink.push_piece(t.text());
                    }
                }
                SyntaxKind::NONBREAKING_SPACE => {
                    skip_marker_whitespace = false;
                    sink.push_piece(r"\ ");
                }
                SyntaxKind::EMPHASIS_MARKER | SyntaxKind::STRONG_MARKER => {}
                SyntaxKind::TEXT => {
                    skip_marker_whitespace = false;
                    let text = expand_tabs_with_width(t.text(), config.tab_width);
                    let mut text_to_process = text.as_ref();
                    if sink.skip_next_leading_whitespace() {
                        text_to_process =
                            text.trim_start_matches(|c: char| c.is_ascii_whitespace());
                        sink.set_skip_next_leading_whitespace(false);
                    } else if !text.is_empty() && starts_with_ascii_whitespace(&text) {
                        sink.set_pending_space(true);
                    }
                    let mut saw_word = false;
                    for word in text_to_process.split_ascii_whitespace() {
                        if saw_word {
                            sink.set_pending_space(true);
                        }
                        let processed_word = escape_special_chars(
                            word,
                            false,
                            prev_is_text,
                            next_is_text,
                            !in_link_text,
                        );
                        sink.push_piece(&processed_word);
                        saw_word = true;
                    }
                    if saw_word && ends_with_ascii_whitespace(&text) {
                        sink.set_pending_space(true);
                    }
                }
                _ => {
                    skip_marker_whitespace = false;
                    sink.push_piece(t.text());
                }
            },
            NodeOrToken::Node(n) => match n.kind() {
                SyntaxKind::LIST => {
                    skip_marker_whitespace = false;
                    sink.set_pending_space(true)
                }
                SyntaxKind::CODE_BLOCK | SyntaxKind::BLANK_LINE => {}
                SyntaxKind::INLINE_FOOTNOTE => {
                    skip_marker_whitespace = false;
                    let had_pending_space = sink.pending_space();
                    sink.set_pending_space(false);
                    sink.push_piece("^[");
                    sink.set_skip_next_leading_whitespace(true);
                    process_node_recursive(
                        config,
                        &n,
                        sink,
                        format_inline_fn,
                        in_link_text,
                        atomic_links,
                        true,
                    );
                    sink.set_pending_space(false);
                    sink.push_piece("]");
                    sink.set_skip_next_leading_whitespace(false);
                    sink.set_pending_space(had_pending_space);
                }
                SyntaxKind::PARAGRAPH if matches!(node.kind(), SyntaxKind::LIST_ITEM) => {
                    skip_marker_whitespace = false;
                    let has_blank_before = n
                        .prev_sibling()
                        .map(|prev| prev.kind() == SyntaxKind::BLANK_LINE)
                        .unwrap_or(false);
                    if !has_blank_before {
                        process_node_recursive(
                            config,
                            &n,
                            sink,
                            format_inline_fn,
                            in_link_text,
                            atomic_links,
                            in_inline_footnote,
                        );
                    }
                }
                SyntaxKind::PARAGRAPH => process_node_recursive(
                    config,
                    &n,
                    sink,
                    format_inline_fn,
                    in_link_text,
                    atomic_links,
                    in_inline_footnote,
                ),
                SyntaxKind::EMPHASIS => {
                    skip_marker_whitespace = false;
                    if node_starts_with_whitespace(&n) {
                        sink.set_pending_space(true);
                        sink.set_skip_next_leading_whitespace(true);
                    }
                    sink.push_piece("*");
                    process_node_recursive(
                        config,
                        &n,
                        sink,
                        format_inline_fn,
                        in_link_text,
                        atomic_links,
                        in_inline_footnote,
                    );
                    sink.set_skip_next_leading_whitespace(false);
                    let had_pending_space = sink.pending_space();
                    sink.set_pending_space(false);
                    sink.push_piece("*");
                    sink.set_pending_space(had_pending_space);
                }
                SyntaxKind::STRONG => {
                    skip_marker_whitespace = false;
                    if node_starts_with_whitespace(&n) {
                        sink.set_pending_space(true);
                        sink.set_skip_next_leading_whitespace(true);
                    }
                    sink.push_piece("**");
                    process_node_recursive(
                        config,
                        &n,
                        sink,
                        format_inline_fn,
                        in_link_text,
                        atomic_links,
                        in_inline_footnote,
                    );
                    sink.set_skip_next_leading_whitespace(false);
                    let had_pending_space = sink.pending_space();
                    sink.set_pending_space(false);
                    sink.push_piece("**");
                    sink.set_pending_space(had_pending_space);
                }
                SyntaxKind::LINK => {
                    skip_marker_whitespace = false;
                    if atomic_links {
                        let formatted = format_inline_fn(&n);
                        let text = normalize_inline_for_sentence(&formatted);
                        sink.push_piece(text.as_ref());
                    } else {
                        sink.push_piece("[");
                        for child in n.children_with_tokens() {
                            if let NodeOrToken::Node(link_child) = child
                                && link_child.kind() == SyntaxKind::LINK_TEXT
                            {
                                process_node_recursive(
                                    config,
                                    &link_child,
                                    sink,
                                    format_inline_fn,
                                    true,
                                    atomic_links,
                                    in_inline_footnote,
                                );
                            }
                        }
                        let mut closing = String::new();
                        append_link_closing(&n, &mut closing);
                        sink.push_piece(&closing);
                    }
                }
                SyntaxKind::IMAGE_LINK => {
                    skip_marker_whitespace = false;
                    if atomic_links {
                        let formatted = format_inline_fn(&n);
                        let text = normalize_inline_for_sentence(&formatted);
                        sink.push_piece(text.as_ref());
                    } else {
                        sink.push_piece("![");
                        for child in n.children_with_tokens() {
                            if let NodeOrToken::Node(img_child) = child
                                && img_child.kind() == SyntaxKind::IMAGE_ALT
                            {
                                process_node_recursive(
                                    config,
                                    &img_child,
                                    sink,
                                    format_inline_fn,
                                    true,
                                    atomic_links,
                                    in_inline_footnote,
                                );
                            }
                        }
                        let mut closing = String::new();
                        append_image_closing(&n, &mut closing);
                        sink.push_piece(&closing);
                    }
                }
                SyntaxKind::CODE_SPAN
                | SyntaxKind::INLINE_EXECUTABLE_CODE
                | SyntaxKind::INLINE_EXEC_CODE => {
                    skip_marker_whitespace = false;
                    let text = format_inline_fn(&n);
                    sink.push_piece_with_boundary(&text, SentenceBoundaryClass::NonBoundary);
                }
                SyntaxKind::DISPLAY_MATH => {
                    skip_marker_whitespace = false;
                    let mut trailing_attrs = None;
                    let mut consumed_interstitial_whitespace = false;
                    if config.extensions.quarto_crossrefs {
                        if let Some(NodeOrToken::Token(t)) = children.peek()
                            && t.kind() == SyntaxKind::WHITESPACE
                        {
                            let _ = children.next();
                            consumed_interstitial_whitespace = true;
                        }
                        if let Some(next) = children.peek() {
                            match next {
                                NodeOrToken::Node(attr_node)
                                    if attr_node.kind() == SyntaxKind::ATTRIBUTE =>
                                {
                                    trailing_attrs = Some(attr_node.text().to_string());
                                    let _ = children.next();
                                }
                                NodeOrToken::Token(t)
                                    if t.kind() == SyntaxKind::TEXT
                                        && t.text().trim_start().starts_with('{') =>
                                {
                                    trailing_attrs = Some(t.text().to_string());
                                    let _ = children.next();
                                }
                                _ => {}
                            }
                        }
                    }
                    if consumed_interstitial_whitespace && trailing_attrs.is_none() {
                        sink.set_pending_space(true);
                    }

                    let mut text = format_inline_fn(&n);
                    let is_environment_math = text.starts_with("\\begin{");
                    let in_list_item = n
                        .ancestors()
                        .any(|ancestor| ancestor.kind() == SyntaxKind::LIST_ITEM);
                    if let Some(attrs) = trailing_attrs {
                        text.push(' ');
                        text.push_str(attrs.trim());
                    }
                    let verbatim = text.trim_end_matches(['\r', '\n']);
                    if is_environment_math && in_list_item {
                        sink.push_piece(verbatim);
                    } else {
                        sink.push_verbatim_block(verbatim);
                    }
                }
                _ => {
                    let text = format_inline_fn(&n);
                    sink.push_piece(&text);
                }
            },
        }
        prev_is_text = current_is_text;
    }
}

pub(super) fn wrapped_lines_for_paragraph(
    _config: &Config,
    node: &SyntaxNode,
    width: usize,
    format_inline_fn: &dyn Fn(&SyntaxNode) -> String,
) -> Vec<String> {
    if is_fence_like_triplet_paragraph(node) {
        return node
            .text()
            .to_string()
            .lines()
            .map(ToString::to_string)
            .collect();
    }
    log::debug!("wrapped_lines_for_paragraph called with width={}", width);
    let out_lines = wrapped_lines_for_node(
        _config,
        node,
        &[width],
        format_inline_fn,
        WrapStrategy::ParagraphReflow,
    );
    log::debug!("Wrapped into {} lines", out_lines.len());
    out_lines
}

pub(super) fn wrapped_lines_for_paragraph_with_widths(
    _config: &Config,
    node: &SyntaxNode,
    widths: &[usize],
    format_inline_fn: &dyn Fn(&SyntaxNode) -> String,
) -> Vec<String> {
    if is_fence_like_triplet_paragraph(node) {
        return node
            .text()
            .to_string()
            .lines()
            .map(ToString::to_string)
            .collect();
    }
    log::debug!("wrapped_lines_for_paragraph_with_widths called");
    let out_lines = wrapped_lines_for_node(
        _config,
        node,
        widths,
        format_inline_fn,
        WrapStrategy::ParagraphReflow,
    );
    log::debug!("Wrapped into {} lines", out_lines.len());
    out_lines
}

pub(super) fn sentence_lines_for_paragraph(
    _config: &Config,
    node: &SyntaxNode,
    format_inline_fn: &dyn Fn(&SyntaxNode) -> String,
) -> Vec<String> {
    log::debug!("sentence_lines_for_paragraph called");
    wrapped_lines_for_node(
        _config,
        node,
        &[],
        format_inline_fn,
        WrapStrategy::ParagraphSentence,
    )
}

pub(super) fn wrapped_lines_for_node(
    config: &Config,
    node: &SyntaxNode,
    widths: &[usize],
    format_inline_fn: &dyn Fn(&SyntaxNode) -> String,
    strategy: WrapStrategy,
) -> Vec<String> {
    let options = strategy.options(widths);
    let sentence_mode = matches!(options.mode, NodeWrapMode::Sentence);
    let line_widths = if sentence_mode || !options.widths.is_empty() {
        options.widths
    } else {
        &[1]
    };
    let sentence_language = resolve_sentence_language(node);
    let mut builder = TraversalBuilder::new(
        line_widths,
        sentence_mode,
        options.strip_standalone_blockquote_markers,
        sentence_language,
    );
    process_node_recursive(
        config,
        node,
        &mut builder,
        format_inline_fn,
        false,
        options.atomic_links_root,
        false,
    );
    builder.finish()
}

fn is_fence_like_triplet_paragraph(node: &SyntaxNode) -> bool {
    if node.kind() != SyntaxKind::PARAGRAPH {
        return false;
    }

    let text = node.text().to_string();
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() != 3 {
        return false;
    }

    let first = lines[0].trim();
    let middle = lines[1].trim();
    let last = lines[2].trim();

    let is_fence = |line: &str| line.len() >= 3 && line.chars().all(|c| c == ':');
    is_fence(first) && is_fence(last) && !middle.is_empty()
}

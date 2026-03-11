use crate::config::Config;
use crate::formatter::math_delimiters::has_ambiguous_dollar_delimiters;
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
            '[' | '|' | '~' | '`' => {
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

fn fast_display_width(text: &str) -> usize {
    UnicodeWidthStr::width(text)
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

fn is_sentence_boundary_piece(word: &str, has_whitespace_after: bool, is_last: bool) -> bool {
    let trimmed = word.trim_end_matches(['"', '\'', ')', ']', '}']);
    if trimmed.ends_with("...") || trimmed.ends_with("…") {
        return false;
    }
    let Some(last_char) = trimmed.chars().last() else {
        return false;
    };
    matches!(last_char, '.' | '!' | '?') && (has_whitespace_after || is_last)
}

fn strip_blockquote_prefix(line: &str) -> &str {
    if let Some(rest) = line.strip_prefix("> ") {
        rest
    } else if let Some(rest) = line.strip_prefix('>') {
        rest
    } else {
        line
    }
}

#[derive(Clone, Copy)]
struct Piece<'a> {
    word: &'a str,
    whitespace_after: bool,
}

#[derive(Clone)]
pub(super) struct WrapWord {
    pub word: String,
    pub whitespace: String,
    pub penalty: String,
}

fn is_sentence_boundary(word: &WrapWord, is_last: bool) -> bool {
    let mut trimmed = word.word.as_str();
    trimmed = trimmed.trim_end_matches(['"', '\'', ')', ']', '}']);
    if trimmed.ends_with("...") || trimmed.ends_with("…") {
        return false;
    }
    let Some(last_char) = trimmed.chars().last() else {
        return false;
    };
    matches!(last_char, '.' | '!' | '?') && (!word.whitespace.is_empty() || is_last)
}

pub(super) fn sentence_lines_from_words(words: &[WrapWord]) -> Vec<String> {
    let mut out_lines = Vec::new();
    let mut current = String::new();
    for (idx, word) in words.iter().enumerate() {
        let is_last = idx + 1 == words.len();
        current.push_str(&word.word);
        if is_sentence_boundary(word, is_last) {
            current.push_str(&word.penalty);
            if !current.is_empty() {
                out_lines.push(current);
                current = String::new();
            }
        } else {
            current.push_str(&word.whitespace);
        }
    }
    if !current.is_empty() {
        out_lines.push(current);
    }
    out_lines
}

fn build_words_from_pieces<'a>(pieces: Vec<Piece<'a>>) -> Vec<WrapWord> {
    pieces
        .into_iter()
        .map(|piece| WrapWord {
            word: piece.word.to_string(),
            whitespace: if piece.whitespace_after {
                " ".to_string()
            } else {
                String::new()
            },
            penalty: String::new(),
        })
        .collect()
}

pub(super) fn build_words(
    config: &Config,
    node: &SyntaxNode,
    format_inline_fn: &dyn Fn(&SyntaxNode) -> String,
) -> Vec<WrapWord> {
    let mut arena: Vec<Box<str>> = Vec::new();
    build_words_from_pieces(build_pieces_with_mode(
        config,
        node,
        &mut arena,
        format_inline_fn,
        false,
        false,
    ))
}

pub(super) fn wrap_words_first_fit(words: &[WrapWord], line_widths: &[usize]) -> Vec<String> {
    let default_line_width = line_widths.last().copied().unwrap_or(0);
    let mut out = Vec::new();
    let mut line = String::new();
    let mut line_width = 0usize;
    for w in words {
        let ww = fast_display_width(&w.word);
        let line_limit = line_widths
            .get(out.len())
            .copied()
            .unwrap_or(default_line_width);
        let spacer = usize::from(!line.is_empty() && !w.whitespace.is_empty());
        if !line.is_empty() && line_width + spacer + ww > line_limit {
            out.push(std::mem::take(&mut line));
            line_width = 0;
        }
        if !line.is_empty() {
            line.push(' ');
            line_width += 1;
        }
        line.push_str(&w.word);
        line_width += ww;
    }
    if !line.is_empty() {
        out.push(line);
    } else if out.is_empty() {
        out.push(String::new());
    }
    out
}

pub(super) fn wrap_text_first_fit(text: &str, line_width: usize) -> Vec<String> {
    let words: Vec<WrapWord> = text
        .split_whitespace()
        .map(|w| WrapWord {
            word: w.to_string(),
            whitespace: " ".to_string(),
            penalty: String::new(),
        })
        .collect();
    wrap_words_first_fit(&words, &[line_width])
}

fn build_pieces_with_mode<'a>(
    _config: &Config,
    node: &SyntaxNode,
    arena: &'a mut Vec<Box<str>>,
    format_inline_fn: &dyn Fn(&SyntaxNode) -> String,
    in_link_text: bool,
    atomic_links: bool,
) -> Vec<Piece<'a>> {
    struct Builder {
        pieces: Vec<String>,
        whitespace_after: Vec<bool>,
        last_piece_pos: Option<usize>,
        pending_space: bool,
        skip_next_leading_whitespace: bool,
    }

    impl Builder {
        fn new() -> Self {
            Self {
                pieces: Vec::new(),
                whitespace_after: Vec::new(),
                last_piece_pos: None,
                pending_space: false,
                skip_next_leading_whitespace: false,
            }
        }
        fn flush_pending(&mut self) {
            if self.pending_space {
                if let Some(prev) = self.last_piece_pos {
                    self.whitespace_after[prev] = true;
                }
                self.pending_space = false;
            }
        }
        fn attach_to_previous(&mut self, text: &str) {
            if let Some(pos) = self.last_piece_pos {
                self.pieces[pos].push_str(text);
            } else {
                self.start_new_piece(text);
            }
        }
        fn start_new_piece(&mut self, text: &str) {
            self.pieces.push(text.to_string());
            self.whitespace_after.push(false);
            self.last_piece_pos = Some(self.pieces.len() - 1);
        }
        fn push_piece(&mut self, text: &str) {
            if self.pending_space {
                self.flush_pending();
                self.start_new_piece(text);
            } else {
                self.attach_to_previous(text);
            }
        }
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

    fn process_node_recursive(
        _config: &Config,
        node: &SyntaxNode,
        b: &mut Builder,
        format_inline_fn: &dyn Fn(&SyntaxNode) -> String,
        in_link_text: bool,
        atomic_links: bool,
    ) {
        let mut children = node.children_with_tokens().peekable();
        let mut prev_is_text = false;
        while let Some(el) = children.next() {
            let current_is_text =
                matches!(&el, NodeOrToken::Token(t) if t.kind() == SyntaxKind::TEXT);
            let next_is_text = matches!(
                children.peek(),
                Some(NodeOrToken::Token(tok)) if tok.kind() == SyntaxKind::TEXT
            );
            match el {
                NodeOrToken::Token(t) => match t.kind() {
                    SyntaxKind::WHITESPACE | SyntaxKind::NEWLINE | SyntaxKind::BLANK_LINE => {
                        b.pending_space = true;
                    }
                    SyntaxKind::BLOCKQUOTE_MARKER => {}
                    SyntaxKind::ESCAPED_CHAR => {
                        if in_link_text && t.text() == r"\_" {
                            b.push_piece("_");
                        } else {
                            b.push_piece(t.text());
                        }
                    }
                    SyntaxKind::EMPHASIS_MARKER | SyntaxKind::STRONG_MARKER => {}
                    SyntaxKind::TEXT => {
                        let text = expand_tabs_with_width(t.text(), _config.tab_width);
                        let mut text_to_process = text.as_ref();
                        if !text.is_empty() && text.starts_with(char::is_whitespace) {
                            if b.skip_next_leading_whitespace {
                                text_to_process = text.trim_start();
                                b.skip_next_leading_whitespace = false;
                            } else {
                                b.pending_space = true;
                            }
                        }
                        let mut saw_word = false;
                        for word in text_to_process.split_whitespace() {
                            if saw_word {
                                b.pending_space = true;
                            }
                            let processed_word = escape_special_chars(
                                word,
                                false,
                                prev_is_text,
                                next_is_text,
                                !in_link_text,
                            );
                            b.push_piece(&processed_word);
                            saw_word = true;
                        }
                        if saw_word && text.ends_with(char::is_whitespace) {
                            b.pending_space = true;
                        }
                    }
                    _ => b.push_piece(t.text()),
                },
                NodeOrToken::Node(n) => match n.kind() {
                    SyntaxKind::LIST => b.pending_space = true,
                    SyntaxKind::CODE_BLOCK | SyntaxKind::BLANK_LINE => {}
                    SyntaxKind::PARAGRAPH if matches!(node.kind(), SyntaxKind::LIST_ITEM) => {
                        let has_blank_before = n
                            .prev_sibling()
                            .map(|prev| prev.kind() == SyntaxKind::BLANK_LINE)
                            .unwrap_or(false);
                        if !has_blank_before {
                            process_node_recursive(
                                _config,
                                &n,
                                b,
                                format_inline_fn,
                                in_link_text,
                                atomic_links,
                            );
                        }
                    }
                    SyntaxKind::PARAGRAPH => process_node_recursive(
                        _config,
                        &n,
                        b,
                        format_inline_fn,
                        in_link_text,
                        atomic_links,
                    ),
                    SyntaxKind::EMPHASIS => {
                        if node_starts_with_whitespace(&n) {
                            b.pending_space = true;
                            b.skip_next_leading_whitespace = true;
                        }
                        b.push_piece("*");
                        process_node_recursive(
                            _config,
                            &n,
                            b,
                            format_inline_fn,
                            in_link_text,
                            atomic_links,
                        );
                        b.skip_next_leading_whitespace = false;
                        let had_pending_space = b.pending_space;
                        b.pending_space = false;
                        b.push_piece("*");
                        b.pending_space = had_pending_space;
                    }
                    SyntaxKind::STRONG => {
                        if node_starts_with_whitespace(&n) {
                            b.pending_space = true;
                            b.skip_next_leading_whitespace = true;
                        }
                        b.push_piece("**");
                        process_node_recursive(
                            _config,
                            &n,
                            b,
                            format_inline_fn,
                            in_link_text,
                            atomic_links,
                        );
                        b.skip_next_leading_whitespace = false;
                        let had_pending_space = b.pending_space;
                        b.pending_space = false;
                        b.push_piece("**");
                        b.pending_space = had_pending_space;
                    }
                    SyntaxKind::LINK => {
                        if atomic_links {
                            let formatted = format_inline_fn(&n);
                            let text = normalize_inline_for_sentence(&formatted);
                            b.push_piece(text.as_ref());
                        } else {
                            b.push_piece("[");
                            for child in n.children_with_tokens() {
                                if let NodeOrToken::Node(link_child) = child
                                    && link_child.kind() == SyntaxKind::LINK_TEXT
                                {
                                    process_node_recursive(
                                        _config,
                                        &link_child,
                                        b,
                                        format_inline_fn,
                                        true,
                                        atomic_links,
                                    );
                                }
                            }
                            let mut closing = String::new();
                            let mut past_link_text = false;
                            for child in n.children_with_tokens() {
                                match child {
                                    NodeOrToken::Node(link_child) => match link_child.kind() {
                                        SyntaxKind::LINK_TEXT => past_link_text = true,
                                        SyntaxKind::LINK_DEST
                                        | SyntaxKind::LINK_REF
                                        | SyntaxKind::ATTRIBUTE => {
                                            if past_link_text {
                                                if link_child.kind() == SyntaxKind::LINK_DEST {
                                                    let raw = link_child.text().to_string();
                                                    append_normalized_link_dest(&raw, &mut closing);
                                                } else {
                                                    let _ = write!(
                                                        &mut closing,
                                                        "{}",
                                                        link_child.text()
                                                    );
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
                                                | SyntaxKind::TEXT => closing.push_str(t.text()),
                                                _ => {}
                                            }
                                        }
                                    }
                                }
                            }
                            b.attach_to_previous(&closing);
                        }
                    }
                    SyntaxKind::IMAGE_LINK => {
                        if atomic_links {
                            let formatted = format_inline_fn(&n);
                            let text = normalize_inline_for_sentence(&formatted);
                            b.push_piece(text.as_ref());
                        } else {
                            b.push_piece("![");
                            for child in n.children_with_tokens() {
                                if let NodeOrToken::Node(img_child) = child
                                    && img_child.kind() == SyntaxKind::IMAGE_ALT
                                {
                                    process_node_recursive(
                                        _config,
                                        &img_child,
                                        b,
                                        format_inline_fn,
                                        true,
                                        atomic_links,
                                    );
                                }
                            }
                            let mut closing = String::new();
                            let mut past_image_alt = false;
                            for child in n.children_with_tokens() {
                                match child {
                                    NodeOrToken::Node(img_child) => match img_child.kind() {
                                        SyntaxKind::IMAGE_ALT => past_image_alt = true,
                                        SyntaxKind::LINK_DEST | SyntaxKind::ATTRIBUTE => {
                                            if past_image_alt {
                                                if img_child.kind() == SyntaxKind::LINK_DEST {
                                                    let raw = img_child.text().to_string();
                                                    append_normalized_link_dest(&raw, &mut closing);
                                                } else {
                                                    let _ = write!(
                                                        &mut closing,
                                                        "{}",
                                                        img_child.text()
                                                    );
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
                                                | SyntaxKind::TEXT => closing.push_str(t.text()),
                                                _ => {}
                                            }
                                        }
                                    }
                                }
                            }
                            b.attach_to_previous(&closing);
                        }
                    }
                    _ => {
                        let text = format_inline_fn(&n);
                        b.push_piece(&text);
                    }
                },
            }
            prev_is_text = current_is_text;
        }
    }

    let mut b = Builder::new();
    process_node_recursive(
        _config,
        node,
        &mut b,
        format_inline_fn,
        in_link_text,
        atomic_links,
    );
    let mut merged_len = 0usize;
    let mut i = 0;
    while i < b.pieces.len() {
        let current_idx = i;
        if current_idx + 1 < b.pieces.len()
            && b.whitespace_after[current_idx]
            && is_initialism_with_periods(&b.pieces[current_idx])
            && is_year_like(&b.pieces[current_idx + 1])
        {
            let right = std::mem::take(&mut b.pieces[current_idx + 1]);
            b.pieces[current_idx].push(' ');
            b.pieces[current_idx].push_str(&right);
            b.whitespace_after[current_idx] = b.whitespace_after[current_idx + 1];
            i += 2;
        } else {
            i += 1;
        }
        if merged_len != current_idx {
            b.pieces.swap(merged_len, current_idx);
            b.whitespace_after.swap(merged_len, current_idx);
        }
        merged_len += 1;
    }
    b.pieces.truncate(merged_len);
    b.whitespace_after.truncate(merged_len);

    let start_idx = arena.len();
    for text in b.pieces {
        arena.push(text.into_boxed_str());
    }
    b.whitespace_after
        .into_iter()
        .enumerate()
        .map(|(offset, whitespace_after)| {
            let s: &'a str = &arena[start_idx + offset];
            Piece {
                word: s,
                whitespace_after,
            }
        })
        .collect()
}

pub(super) fn wrapped_lines_for_paragraph(
    _config: &Config,
    node: &SyntaxNode,
    width: usize,
    format_inline_fn: &dyn Fn(&SyntaxNode) -> String,
) -> Vec<String> {
    log::debug!("wrapped_lines_for_paragraph called with width={}", width);
    if let Some(lines) = special_case_lines(_config, node, format_inline_fn) {
        return lines;
    }

    let out_lines =
        wrap_node_greedy_streaming(_config, node, &[width], format_inline_fn, false, false);
    log::debug!("Wrapped into {} lines", out_lines.len());
    out_lines
}

pub(super) fn wrapped_lines_for_paragraph_with_widths(
    _config: &Config,
    node: &SyntaxNode,
    widths: &[usize],
    format_inline_fn: &dyn Fn(&SyntaxNode) -> String,
) -> Vec<String> {
    log::debug!("wrapped_lines_for_paragraph_with_widths called");
    if let Some(lines) = special_case_lines(_config, node, format_inline_fn) {
        return lines;
    }

    let line_widths = if widths.is_empty() { &[1] } else { widths };
    let out_lines =
        wrap_node_greedy_streaming(_config, node, line_widths, format_inline_fn, false, false);
    log::debug!("Wrapped into {} lines", out_lines.len());
    out_lines
}

pub(super) fn sentence_lines_for_paragraph(
    _config: &Config,
    node: &SyntaxNode,
    format_inline_fn: &dyn Fn(&SyntaxNode) -> String,
) -> Vec<String> {
    log::debug!("sentence_lines_for_paragraph called");
    if let Some(lines) = special_case_lines(_config, node, format_inline_fn) {
        return lines;
    }
    wrap_node_greedy_streaming(_config, node, &[], format_inline_fn, true, true)
}

fn wrap_node_greedy_streaming(
    _config: &Config,
    node: &SyntaxNode,
    line_widths: &[usize],
    format_inline_fn: &dyn Fn(&SyntaxNode) -> String,
    sentence_mode: bool,
    atomic_links_root: bool,
) -> Vec<String> {
    struct GreedySink<'a> {
        default_line_width: usize,
        line_widths: &'a [usize],
        sentence_mode: bool,
        out: Vec<String>,
        line: String,
        line_width: usize,
        line_has_piece: bool,
        prev_ws_after: bool,
        pending_piece: Option<(String, bool)>,
    }

    impl<'a> GreedySink<'a> {
        fn new(line_widths: &'a [usize], sentence_mode: bool) -> Self {
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
            }
        }

        fn consume(&mut self, piece: String, piece_ws_after: bool, is_last: bool) {
            let piece_width = fast_display_width(&piece);
            if !self.sentence_mode {
                let width_limit = self
                    .line_widths
                    .get(self.out.len())
                    .copied()
                    .unwrap_or(self.default_line_width);
                let spacer_width = usize::from(self.line_has_piece && self.prev_ws_after);
                if self.line_has_piece && self.line_width + spacer_width + piece_width > width_limit
                {
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
            self.line.push_str(&piece);
            self.line_width += piece_width;
            self.line_has_piece = true;
            self.prev_ws_after = piece_ws_after;

            if self.sentence_mode && is_sentence_boundary_piece(&piece, piece_ws_after, is_last) {
                self.out.push(std::mem::take(&mut self.line));
                self.line_width = 0;
                self.line_has_piece = false;
                self.prev_ws_after = false;
            }
        }

        fn emit_piece(&mut self, piece: String, ws_after: bool) {
            if let Some((pending, pending_ws_after)) = self.pending_piece.take() {
                if pending_ws_after && is_initialism_with_periods(&pending) && is_year_like(&piece)
                {
                    self.pending_piece = Some((format!("{pending} {piece}"), ws_after));
                    return;
                }
                self.consume(pending, pending_ws_after, false);
            }
            self.pending_piece = Some((piece, ws_after));
        }

        fn finish(mut self) -> Vec<String> {
            if let Some((pending, pending_ws_after)) = self.pending_piece.take() {
                self.consume(pending, pending_ws_after, true);
            }
            if self.line_has_piece {
                self.out.push(self.line);
            } else if self.out.is_empty() {
                self.out.push(String::new());
            }
            self.out
        }
    }

    struct StreamingBuilder<'a> {
        sink: GreedySink<'a>,
        current_piece: Option<String>,
        pending_space: bool,
        skip_next_leading_whitespace: bool,
    }

    impl<'a> StreamingBuilder<'a> {
        fn new(line_widths: &'a [usize], sentence_mode: bool) -> Self {
            Self {
                sink: GreedySink::new(line_widths, sentence_mode),
                current_piece: None,
                pending_space: false,
                skip_next_leading_whitespace: false,
            }
        }

        fn flush_current(&mut self, ws_after: bool) {
            if let Some(piece) = self.current_piece.take() {
                self.sink.emit_piece(piece, ws_after);
            }
        }

        fn push_piece(&mut self, text: &str) {
            if self.pending_space {
                self.flush_current(true);
                self.current_piece = Some(text.to_string());
                self.pending_space = false;
            } else if let Some(current) = &mut self.current_piece {
                current.push_str(text);
            } else {
                self.current_piece = Some(text.to_string());
            }
        }

        fn finish(mut self) -> Vec<String> {
            self.flush_current(false);
            self.sink.finish()
        }
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

    fn process_node_recursive(
        _config: &Config,
        node: &SyntaxNode,
        b: &mut StreamingBuilder<'_>,
        format_inline_fn: &dyn Fn(&SyntaxNode) -> String,
        in_link_text: bool,
        atomic_links: bool,
    ) {
        let mut children = node.children_with_tokens().peekable();
        let mut prev_is_text = false;

        while let Some(el) = children.next() {
            let current_is_text =
                matches!(&el, NodeOrToken::Token(t) if t.kind() == SyntaxKind::TEXT);
            let next_is_text = matches!(
                children.peek(),
                Some(NodeOrToken::Token(tok)) if tok.kind() == SyntaxKind::TEXT
            );
            match el {
                NodeOrToken::Token(t) => match t.kind() {
                    SyntaxKind::WHITESPACE | SyntaxKind::NEWLINE | SyntaxKind::BLANK_LINE => {
                        b.pending_space = true;
                    }
                    SyntaxKind::BLOCKQUOTE_MARKER => {}
                    SyntaxKind::ESCAPED_CHAR => {
                        if in_link_text && t.text() == r"\_" {
                            b.push_piece("_");
                        } else {
                            b.push_piece(t.text());
                        }
                    }
                    SyntaxKind::EMPHASIS_MARKER | SyntaxKind::STRONG_MARKER => {}
                    SyntaxKind::TEXT => {
                        let text = expand_tabs_with_width(t.text(), _config.tab_width);
                        let mut text_to_process = text.as_ref();
                        if !text.is_empty() && text.starts_with(char::is_whitespace) {
                            if b.skip_next_leading_whitespace {
                                text_to_process = text.trim_start();
                                b.skip_next_leading_whitespace = false;
                            } else {
                                b.pending_space = true;
                            }
                        }

                        let mut saw_word = false;
                        for word in text_to_process.split_whitespace() {
                            if saw_word {
                                b.pending_space = true;
                            }
                            let processed_word = escape_special_chars(
                                word,
                                false,
                                prev_is_text,
                                next_is_text,
                                !in_link_text,
                            );
                            b.push_piece(&processed_word);
                            saw_word = true;
                        }
                        if saw_word && text.ends_with(char::is_whitespace) {
                            b.pending_space = true;
                        }
                    }
                    _ => b.push_piece(t.text()),
                },
                NodeOrToken::Node(n) => match n.kind() {
                    SyntaxKind::LIST => b.pending_space = true,
                    SyntaxKind::CODE_BLOCK | SyntaxKind::BLANK_LINE => {}
                    SyntaxKind::PARAGRAPH if matches!(node.kind(), SyntaxKind::LIST_ITEM) => {
                        let has_blank_before = n
                            .prev_sibling()
                            .map(|prev| prev.kind() == SyntaxKind::BLANK_LINE)
                            .unwrap_or(false);
                        if !has_blank_before {
                            process_node_recursive(
                                _config,
                                &n,
                                b,
                                format_inline_fn,
                                in_link_text,
                                atomic_links,
                            );
                        }
                    }
                    SyntaxKind::PARAGRAPH => process_node_recursive(
                        _config,
                        &n,
                        b,
                        format_inline_fn,
                        in_link_text,
                        atomic_links,
                    ),
                    SyntaxKind::EMPHASIS => {
                        if node_starts_with_whitespace(&n) {
                            b.pending_space = true;
                            b.skip_next_leading_whitespace = true;
                        }
                        b.push_piece("*");
                        process_node_recursive(
                            _config,
                            &n,
                            b,
                            format_inline_fn,
                            in_link_text,
                            atomic_links,
                        );
                        b.skip_next_leading_whitespace = false;
                        let had_pending_space = b.pending_space;
                        b.pending_space = false;
                        b.push_piece("*");
                        b.pending_space = had_pending_space;
                    }
                    SyntaxKind::STRONG => {
                        if node_starts_with_whitespace(&n) {
                            b.pending_space = true;
                            b.skip_next_leading_whitespace = true;
                        }
                        b.push_piece("**");
                        process_node_recursive(
                            _config,
                            &n,
                            b,
                            format_inline_fn,
                            in_link_text,
                            atomic_links,
                        );
                        b.skip_next_leading_whitespace = false;
                        let had_pending_space = b.pending_space;
                        b.pending_space = false;
                        b.push_piece("**");
                        b.pending_space = had_pending_space;
                    }
                    SyntaxKind::LINK => {
                        if atomic_links {
                            let formatted = format_inline_fn(&n);
                            let text = normalize_inline_for_sentence(&formatted);
                            b.push_piece(text.as_ref());
                        } else {
                            b.push_piece("[");
                            for child in n.children_with_tokens() {
                                if let NodeOrToken::Node(link_child) = child
                                    && link_child.kind() == SyntaxKind::LINK_TEXT
                                {
                                    process_node_recursive(
                                        _config,
                                        &link_child,
                                        b,
                                        format_inline_fn,
                                        true,
                                        atomic_links,
                                    );
                                }
                            }

                            let mut closing = String::new();
                            let mut past_link_text = false;
                            for child in n.children_with_tokens() {
                                match child {
                                    NodeOrToken::Node(link_child) => match link_child.kind() {
                                        SyntaxKind::LINK_TEXT => past_link_text = true,
                                        SyntaxKind::LINK_DEST
                                        | SyntaxKind::LINK_REF
                                        | SyntaxKind::ATTRIBUTE => {
                                            if past_link_text {
                                                if link_child.kind() == SyntaxKind::LINK_DEST {
                                                    let raw = link_child.text().to_string();
                                                    append_normalized_link_dest(&raw, &mut closing);
                                                } else {
                                                    let _ = write!(
                                                        &mut closing,
                                                        "{}",
                                                        link_child.text()
                                                    );
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
                                                | SyntaxKind::TEXT => closing.push_str(t.text()),
                                                _ => {}
                                            }
                                        }
                                    }
                                }
                            }
                            b.push_piece(&closing);
                        }
                    }
                    SyntaxKind::IMAGE_LINK => {
                        if atomic_links {
                            let formatted = format_inline_fn(&n);
                            let text = normalize_inline_for_sentence(&formatted);
                            b.push_piece(text.as_ref());
                        } else {
                            b.push_piece("![");
                            for child in n.children_with_tokens() {
                                if let NodeOrToken::Node(img_child) = child
                                    && img_child.kind() == SyntaxKind::IMAGE_ALT
                                {
                                    process_node_recursive(
                                        _config,
                                        &img_child,
                                        b,
                                        format_inline_fn,
                                        true,
                                        atomic_links,
                                    );
                                }
                            }

                            let mut closing = String::new();
                            let mut past_image_alt = false;
                            for child in n.children_with_tokens() {
                                match child {
                                    NodeOrToken::Node(img_child) => match img_child.kind() {
                                        SyntaxKind::IMAGE_ALT => past_image_alt = true,
                                        SyntaxKind::LINK_DEST | SyntaxKind::ATTRIBUTE => {
                                            if past_image_alt {
                                                if img_child.kind() == SyntaxKind::LINK_DEST {
                                                    let raw = img_child.text().to_string();
                                                    append_normalized_link_dest(&raw, &mut closing);
                                                } else {
                                                    let _ = write!(
                                                        &mut closing,
                                                        "{}",
                                                        img_child.text()
                                                    );
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
                                                | SyntaxKind::TEXT => closing.push_str(t.text()),
                                                _ => {}
                                            }
                                        }
                                    }
                                }
                            }
                            b.push_piece(&closing);
                        }
                    }
                    _ => {
                        let text = format_inline_fn(&n);
                        b.push_piece(&text);
                    }
                },
            }
            prev_is_text = current_is_text;
        }
    }

    let mut builder = StreamingBuilder::new(line_widths, sentence_mode);
    process_node_recursive(
        _config,
        node,
        &mut builder,
        format_inline_fn,
        false,
        atomic_links_root,
    );
    builder.finish()
}

fn special_case_lines(
    config: &Config,
    node: &SyntaxNode,
    format_inline_fn: &dyn Fn(&SyntaxNode) -> String,
) -> Option<Vec<String>> {
    let mut has_hard_breaks = false;
    let mut has_dollar_text = false;
    for el in node.descendants_with_tokens() {
        if el.kind() == SyntaxKind::HARD_LINE_BREAK {
            has_hard_breaks = true;
        }
        if let NodeOrToken::Token(t) = el
            && t.text().contains('$')
        {
            has_dollar_text = true;
        }
        if has_hard_breaks && has_dollar_text {
            break;
        }
    }

    let has_blockquote_markers = node
        .children_with_tokens()
        .any(|el| matches!(el, NodeOrToken::Token(t) if t.kind() == SyntaxKind::BLOCKQUOTE_MARKER));
    let in_blockquote = node
        .ancestors()
        .any(|ancestor| ancestor.kind() == SyntaxKind::BLOCKQUOTE);
    if has_dollar_text {
        let paragraph_text = node.text().to_string();
        let normalized: Cow<'_, str> = if paragraph_text.contains("\r\n") {
            Cow::Owned(paragraph_text.replace("\r\n", "\n"))
        } else {
            Cow::Borrowed(paragraph_text.as_str())
        };
        if has_ambiguous_dollar_delimiters(&normalized) && !has_blockquote_markers {
            return Some(paragraph_text.lines().map(ToString::to_string).collect());
        }
        let standalone_fences = normalized
            .lines()
            .map(|line| {
                if has_blockquote_markers {
                    strip_blockquote_prefix(line)
                } else {
                    line
                }
            })
            .filter(|line| line.trim_start().starts_with("$$"))
            .count();
        if standalone_fences >= 2 && standalone_fences % 2 == 0 {
            if has_blockquote_markers {
                return Some(
                    paragraph_text
                        .lines()
                        .map(strip_blockquote_prefix)
                        .map(ToString::to_string)
                        .collect(),
                );
            }
            return Some(
                paragraph_text
                    .lines()
                    .map(|line| line.trim_end().to_string())
                    .collect(),
            );
        }
        let fence_marker_count = normalized.match_indices("$$").count();
        if fence_marker_count >= 2 && fence_marker_count.is_multiple_of(2) && in_blockquote {
            return Some(
                paragraph_text
                    .lines()
                    .map(strip_blockquote_prefix)
                    .map(|line| line.trim_end().to_string())
                    .collect(),
            );
        }
    }

    if !has_hard_breaks {
        return None;
    }

    let mut result = String::new();
    let mut skip_next_whitespace = false;
    for child in node.children_with_tokens() {
        match child {
            NodeOrToken::Node(n) => {
                skip_next_whitespace = false;
                result.push_str(&format_inline_fn(&n));
            }
            NodeOrToken::Token(t) => {
                if t.kind() == SyntaxKind::BLOCKQUOTE_MARKER {
                    skip_next_whitespace = true;
                } else if t.kind() == SyntaxKind::WHITESPACE && skip_next_whitespace {
                    skip_next_whitespace = false;
                } else if t.kind() == SyntaxKind::HARD_LINE_BREAK {
                    skip_next_whitespace = false;
                    if config.extensions.escaped_line_breaks {
                        result.push_str("\\\n");
                    } else {
                        result.push_str(t.text());
                    }
                } else {
                    skip_next_whitespace = false;
                    result.push_str(t.text());
                }
            }
        }
    }

    Some(result.lines().map(|s| s.to_string()).collect())
}

use crate::config::Config;
use crate::syntax::{SyntaxKind, SyntaxNode};
use rowan::NodeOrToken;
use textwrap::wrap_algorithms::WrapAlgorithm;

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
    let chars: Vec<char> = text.chars().collect();

    for (i, &ch) in chars.iter().enumerate() {
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
                let at_start = i == 0;
                let at_end = i == chars.len() - 1;

                // If the entire text is just "_", always escape it (not intraword)
                if text == "_" {
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
            '[' | '~' | '`' => {
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

fn normalize_link_dest(dest: &str) -> String {
    let dest_trimmed = dest.trim();
    let mut split_at = None;
    for (i, ch) in dest_trimmed.char_indices() {
        if ch.is_whitespace() {
            split_at = Some(i);
            break;
        }
    }

    let Some(split_at) = split_at else {
        return dest_trimmed.to_string();
    };

    let (url, rest) = dest_trimmed.split_at(split_at);
    let title = rest.trim();
    if title.is_empty() {
        return url.to_string();
    }

    let normalized_title = if title.starts_with('\'') && title.ends_with('\'') && title.len() >= 2 {
        format!("\"{}\"", &title[1..title.len() - 1])
    } else {
        title.to_string()
    };

    format!("{} {}", url, normalized_title)
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

fn is_sentence_boundary(word: &textwrap::core::Word<'_>, is_last: bool) -> bool {
    let mut trimmed = word.word;
    trimmed = trimmed.trim_end_matches(['"', '\'', ')', ']', '}']);

    if trimmed.ends_with("...") || trimmed.ends_with("…") {
        return false;
    }

    let Some(last_char) = trimmed.chars().last() else {
        return false;
    };

    matches!(last_char, '.' | '!' | '?') && (!word.whitespace.is_empty() || is_last)
}

fn normalize_inline_for_sentence(text: &str) -> String {
    text.replace('\n', " ")
}

pub(super) fn sentence_lines_from_words(words: &[textwrap::core::Word<'_>]) -> Vec<String> {
    let mut out_lines = Vec::new();
    let mut current = String::new();

    for (idx, word) in words.iter().enumerate() {
        let is_last = idx + 1 == words.len();
        current.push_str(word.word);

        if is_sentence_boundary(word, is_last) {
            current.push_str(word.penalty);
            if !current.is_empty() {
                out_lines.push(current);
                current = String::new();
            }
        } else {
            current.push_str(word.whitespace);
        }
    }

    if !current.is_empty() {
        out_lines.push(current);
    }

    out_lines
}

fn build_words_for_sentence<'a>(
    _config: &Config,
    node: &SyntaxNode,
    arena: &'a mut Vec<Box<str>>,
    format_inline_fn: &dyn Fn(&SyntaxNode) -> String,
) -> Vec<textwrap::core::Word<'a>> {
    build_words_with_mode(_config, node, arena, format_inline_fn, false, true)
}

pub(super) fn build_words<'a>(
    _config: &Config,
    node: &SyntaxNode,
    arena: &'a mut Vec<Box<str>>,
    format_inline_fn: &dyn Fn(&SyntaxNode) -> String,
) -> Vec<textwrap::core::Word<'a>> {
    build_words_with_mode(_config, node, arena, format_inline_fn, false, false)
}

fn build_words_with_mode<'a>(
    _config: &Config,
    node: &SyntaxNode,
    arena: &'a mut Vec<Box<str>>,
    format_inline_fn: &dyn Fn(&SyntaxNode) -> String,
    in_link_text: bool,
    atomic_links: bool,
) -> Vec<textwrap::core::Word<'a>> {
    struct Builder<'a> {
        arena: &'a mut Vec<Box<str>>,
        piece_idx: Vec<usize>,
        whitespace_after: Vec<bool>,
        last_piece_pos: Option<usize>,
        pending_space: bool,
        skip_next_leading_whitespace: bool,
    }

    impl<'a> Builder<'a> {
        fn new(arena: &'a mut Vec<Box<str>>) -> Self {
            Self {
                arena,
                piece_idx: Vec::new(),
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
                let prev_idx = self.piece_idx[pos];
                let prev = &self.arena[prev_idx];
                let mut combined = String::with_capacity(prev.len() + text.len());
                combined.push_str(prev);
                combined.push_str(text);
                self.arena.push(combined.into_boxed_str());
                let new_idx = self.arena.len() - 1;
                self.piece_idx[pos] = new_idx;
            } else {
                // No previous piece; start a new one.
                self.start_new_piece(text);
            }
        }

        fn start_new_piece(&mut self, text: &str) {
            self.arena.push(Box::<str>::from(text));
            let idx = self.arena.len() - 1;
            self.piece_idx.push(idx);
            self.whitespace_after.push(false);
            self.last_piece_pos = Some(self.piece_idx.len() - 1);
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

    /// Check if a node's first TEXT content starts with whitespace
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
                    // Skip markers, continue looking
                    continue;
                }
                NodeOrToken::Node(n) => {
                    // Recurse into child nodes
                    if node_starts_with_whitespace(&n) {
                        return true;
                    }
                }
                _ => continue,
            }
        }
        false
    }

    fn process_node_recursive<'cfg>(
        _config: &'cfg Config,
        node: &SyntaxNode,
        b: &mut Builder,
        format_inline_fn: &dyn Fn(&SyntaxNode) -> String,
        in_link_text: bool,
        atomic_links: bool,
    ) {
        let children: Vec<_> = node.children_with_tokens().collect();

        for (idx, el) in children.iter().enumerate() {
            match el {
                NodeOrToken::Token(t) => match t.kind() {
                    SyntaxKind::WHITESPACE | SyntaxKind::NEWLINE | SyntaxKind::BLANK_LINE => {
                        b.pending_space = true;
                    }
                    // Skip blockquote markers - they're in the tree for losslessness but
                    // the formatter adds them dynamically when formatting BlockQuote nodes
                    SyntaxKind::BLOCKQUOTE_MARKER => {}
                    SyntaxKind::ESCAPED_CHAR => {
                        if in_link_text && t.text() == r"\_" {
                            b.push_piece("_");
                        } else {
                            // Token already includes backslash (e.g., "\*")
                            b.push_piece(t.text());
                        }
                    }
                    SyntaxKind::EMPHASIS_MARKER | SyntaxKind::STRONG_MARKER => {
                        // Skip original markers - we'll add normalized ones
                    }
                    SyntaxKind::TEXT => {
                        let text = expand_tabs_with_width(t.text(), _config.tab_width);

                        // Check if prev/next siblings are TEXT (for intraword underscore detection)
                        let prev_is_text = idx > 0
                            && matches!(
                                &children[idx - 1],
                                NodeOrToken::Token(tok) if tok.kind() == SyntaxKind::TEXT
                            );
                        let next_is_text = idx + 1 < children.len()
                            && matches!(
                                &children[idx + 1],
                                NodeOrToken::Token(tok) if tok.kind() == SyntaxKind::TEXT
                            );

                        // Split TEXT tokens on whitespace to create separate words
                        // Note: We don't preserve leading spaces here since list item continuations
                        // will be re-indented by the formatter

                        // Check if text starts with whitespace
                        let mut text_to_process = text.as_str();
                        if !text.is_empty() && text.starts_with(char::is_whitespace) {
                            if b.skip_next_leading_whitespace {
                                // Skip the leading whitespace - it's been moved outside the emphasis/strong
                                text_to_process = text.trim_start();
                                b.skip_next_leading_whitespace = false;
                            } else {
                                b.pending_space = true;
                            }
                        }

                        let words: Vec<&str> = text_to_process.split_whitespace().collect();

                        for (i, word) in words.iter().enumerate() {
                            if i > 0 {
                                b.pending_space = true;
                            }
                            // Always escape special characters in TEXT tokens
                            // ESCAPED_CHAR tokens are handled separately and preserve their backslashes
                            let processed_word = escape_special_chars(
                                word,
                                false,
                                prev_is_text,
                                next_is_text,
                                !in_link_text,
                            );
                            b.push_piece(&processed_word);
                        }

                        // If text ends with whitespace, mark pending space for next piece
                        if !words.is_empty() && text.ends_with(char::is_whitespace) {
                            b.pending_space = true;
                        }
                    }
                    _ => {
                        b.push_piece(t.text());
                    }
                },
                NodeOrToken::Node(n) => match n.kind() {
                    SyntaxKind::LIST => {
                        b.pending_space = true;
                    }
                    SyntaxKind::CODE_BLOCK | SyntaxKind::BLANK_LINE => {
                        // Skip code blocks and blank lines - they'll be handled separately
                        // Don't recurse into them
                    }
                    SyntaxKind::PARAGRAPH if matches!(node.kind(), SyntaxKind::LIST_ITEM) => {
                        // For PARAGRAPH children of ListItem, check if there's a BlankLine before it
                        // If yes, it's a true continuation paragraph (skip in wrapping)
                        // If no, it's a lazy continuation (include in wrapping)
                        let has_blank_before = n
                            .prev_sibling()
                            .map(|prev| prev.kind() == SyntaxKind::BLANK_LINE)
                            .unwrap_or(false);

                        if has_blank_before {
                            // True continuation paragraph - skip in wrapping, format separately
                        } else {
                            // Lazy continuation - include in wrapping
                            process_node_recursive(
                                _config,
                                n,
                                b,
                                format_inline_fn,
                                in_link_text,
                                atomic_links,
                            );
                        }
                    }
                    SyntaxKind::PARAGRAPH => {
                        // Recursively process PARAGRAPH content instead of treating it as a unit
                        process_node_recursive(
                            _config,
                            n,
                            b,
                            format_inline_fn,
                            in_link_text,
                            atomic_links,
                        );
                    }
                    SyntaxKind::EMPHASIS => {
                        // Check if content starts with whitespace - if so, preserve it before opening marker
                        if node_starts_with_whitespace(n) {
                            b.pending_space = true;
                            b.skip_next_leading_whitespace = true;
                        }
                        b.push_piece("*");
                        process_node_recursive(
                            _config,
                            n,
                            b,
                            format_inline_fn,
                            in_link_text,
                            atomic_links,
                        ); // Inside emphasis now
                        // Reset the flag (it should have been consumed by first TEXT, but just in case)
                        b.skip_next_leading_whitespace = false;
                        // Save pending space state (from trailing whitespace in content)
                        let had_pending_space = b.pending_space;
                        // Clear pending space before closing marker (trim trailing whitespace)
                        b.pending_space = false;
                        b.push_piece("*");
                        // Restore pending space state for next sibling
                        b.pending_space = had_pending_space;
                    }
                    SyntaxKind::STRONG => {
                        // Check if content starts with whitespace - if so, preserve it before opening marker
                        if node_starts_with_whitespace(n) {
                            b.pending_space = true;
                            b.skip_next_leading_whitespace = true;
                        }
                        b.push_piece("**");
                        process_node_recursive(
                            _config,
                            n,
                            b,
                            format_inline_fn,
                            in_link_text,
                            atomic_links,
                        ); // Inside emphasis now
                        // Reset the flag (it should have been consumed by first TEXT, but just in case)
                        b.skip_next_leading_whitespace = false;
                        // Save pending space state (from trailing whitespace in content)
                        let had_pending_space = b.pending_space;
                        // Clear pending space before closing marker (trim trailing whitespace)
                        b.pending_space = false;
                        b.push_piece("**");
                        // Restore pending space state for next sibling
                        b.pending_space = had_pending_space;
                    }
                    SyntaxKind::LINK => {
                        if atomic_links {
                            let text = normalize_inline_for_sentence(&format_inline_fn(n));
                            b.push_piece(&text);
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
                                        SyntaxKind::LINK_TEXT => {
                                            past_link_text = true;
                                        }
                                        SyntaxKind::LINK_DEST
                                        | SyntaxKind::LINK_REF
                                        | SyntaxKind::ATTRIBUTE => {
                                            if past_link_text {
                                                if link_child.kind() == SyntaxKind::LINK_DEST {
                                                    closing.push_str(&normalize_link_dest(
                                                        &link_child.text().to_string(),
                                                    ));
                                                } else {
                                                    closing
                                                        .push_str(&link_child.text().to_string());
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
                                                | SyntaxKind::LINK_DEST_END => {
                                                    closing.push_str(t.text());
                                                }
                                                SyntaxKind::TEXT => {
                                                    closing.push_str(t.text());
                                                }
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
                            let text = normalize_inline_for_sentence(&format_inline_fn(n));
                            b.push_piece(&text);
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
                                        SyntaxKind::IMAGE_ALT => {
                                            past_image_alt = true;
                                        }
                                        SyntaxKind::LINK_DEST | SyntaxKind::ATTRIBUTE => {
                                            if past_image_alt {
                                                if img_child.kind() == SyntaxKind::LINK_DEST {
                                                    closing.push_str(&normalize_link_dest(
                                                        &img_child.text().to_string(),
                                                    ));
                                                } else {
                                                    closing.push_str(&img_child.text().to_string());
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
                                                | SyntaxKind::IMAGE_DEST_END => {
                                                    closing.push_str(t.text());
                                                }
                                                SyntaxKind::TEXT => {
                                                    closing.push_str(t.text());
                                                }
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
                        // For other inline nodes, format and push as single piece
                        let text = format_inline_fn(n);
                        b.push_piece(&text);
                    }
                },
            }
        }
    }

    let mut b = Builder::new(arena);
    process_node_recursive(
        _config,
        node,
        &mut b,
        format_inline_fn,
        in_link_text,
        atomic_links,
    ); // Start outside emphasis

    let mut merged_piece_idx: Vec<usize> = Vec::with_capacity(b.piece_idx.len());
    let mut merged_whitespace_after: Vec<bool> = Vec::with_capacity(b.piece_idx.len());
    let mut i = 0;
    while i < b.piece_idx.len() {
        let idx = b.piece_idx[i];
        let s_owned = b.arena[idx].to_string();

        if i + 1 < b.piece_idx.len()
            && b.whitespace_after.get(i).copied().unwrap_or(false)
            && is_initialism_with_periods(&s_owned)
        {
            let next_idx = b.piece_idx[i + 1];
            let next_owned = b.arena[next_idx].to_string();
            if is_year_like(&next_owned) {
                let combined = format!("{s_owned} {next_owned}");
                b.arena.push(combined.into_boxed_str());
                let combined_idx = b.arena.len() - 1;
                merged_piece_idx.push(combined_idx);
                merged_whitespace_after
                    .push(b.whitespace_after.get(i + 1).copied().unwrap_or(false));
                i += 2;
                continue;
            }
        }

        merged_piece_idx.push(idx);
        merged_whitespace_after.push(b.whitespace_after.get(i).copied().unwrap_or(false));
        i += 1;
    }

    let mut words: Vec<textwrap::core::Word<'a>> = Vec::with_capacity(merged_piece_idx.len());
    for (i, &idx) in merged_piece_idx.iter().enumerate() {
        let s: &'a str = &b.arena[idx];
        let mut w = textwrap::core::Word::from(s);
        if merged_whitespace_after.get(i).copied().unwrap_or(false) {
            w.whitespace = " ";
        }
        words.push(w);
    }
    words
}

pub(super) fn wrapped_lines_for_paragraph(
    _config: &Config,
    node: &SyntaxNode,
    width: usize,
    format_inline_fn: &dyn Fn(&SyntaxNode) -> String,
) -> Vec<String> {
    log::debug!("wrapped_lines_for_paragraph called with width={}", width);

    let paragraph_text = node.text().to_string();
    let normalized = paragraph_text.replace("\r\n", "\n");
    let standalone_fences = normalized
        .lines()
        .filter(|line| line.trim() == "$$")
        .count();
    if standalone_fences >= 2 && standalone_fences % 2 == 0 {
        return paragraph_text
            .lines()
            .map(|line| line.trim_end().to_string())
            .collect();
    }

    // Check if paragraph contains hard line breaks
    let has_hard_breaks = node
        .descendants_with_tokens()
        .any(|el| el.kind() == SyntaxKind::HARD_LINE_BREAK);

    if has_hard_breaks {
        // Don't wrap paragraphs with hard line breaks - preserve the breaks
        // But normalize hard breaks and format inline elements
        log::debug!("Paragraph contains hard line breaks - preserving them");

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
                        // Normalize to backslash-newline if extension enabled
                        if _config.extensions.escaped_line_breaks {
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

        return result.lines().map(|s| s.to_string()).collect();
    }

    let mut arena: Vec<Box<str>> = Vec::new();
    let words = build_words(_config, node, &mut arena, format_inline_fn);
    log::debug!("Built {} words for paragraph", words.len());
    log::trace!(
        "Words: {:?}",
        words.iter().map(|w| w.word).collect::<Vec<_>>()
    );

    let algo = WrapAlgorithm::new();
    let line_widths = [width];
    let lines = algo.wrap(&words, &line_widths);
    log::debug!("Wrapped into {} lines", lines.len());

    let mut out_lines = Vec::with_capacity(lines.len());

    for line in lines {
        let mut acc = String::new();
        for (i, w) in line.iter().enumerate() {
            acc.push_str(w.word);
            if i + 1 < line.len() {
                acc.push_str(w.whitespace);
            } else {
                acc.push_str(w.penalty);
            }
        }
        log::trace!("Line: '{}'", acc);
        out_lines.push(acc);
    }
    out_lines
}

pub(super) fn wrapped_lines_for_paragraph_with_widths(
    _config: &Config,
    node: &SyntaxNode,
    widths: &[usize],
    format_inline_fn: &dyn Fn(&SyntaxNode) -> String,
) -> Vec<String> {
    log::debug!("wrapped_lines_for_paragraph_with_widths called");

    let paragraph_text = node.text().to_string();
    let normalized = paragraph_text.replace("\r\n", "\n");
    let standalone_fences = normalized
        .lines()
        .filter(|line| line.trim() == "$$")
        .count();
    if standalone_fences >= 2 && standalone_fences % 2 == 0 {
        return paragraph_text
            .lines()
            .map(|line| line.trim_end().to_string())
            .collect();
    }

    // Check if paragraph contains hard line breaks
    let has_hard_breaks = node
        .descendants_with_tokens()
        .any(|el| el.kind() == SyntaxKind::HARD_LINE_BREAK);

    if has_hard_breaks {
        // Don't wrap paragraphs with hard line breaks - preserve the breaks
        // But normalize hard breaks and format inline elements
        log::debug!("Paragraph contains hard line breaks - preserving them");

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
                        // Normalize to backslash-newline if extension enabled
                        if _config.extensions.escaped_line_breaks {
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

        return result.lines().map(|s| s.to_string()).collect();
    }

    let mut arena: Vec<Box<str>> = Vec::new();
    let words = build_words(_config, node, &mut arena, format_inline_fn);
    log::debug!("Built {} words for paragraph", words.len());

    let algo = WrapAlgorithm::new();
    let line_widths = if widths.is_empty() { &[1] } else { widths };
    let lines = algo.wrap(&words, line_widths);
    log::debug!("Wrapped into {} lines", lines.len());

    let mut out_lines = Vec::with_capacity(lines.len());

    for line in lines {
        let mut acc = String::new();
        for (i, w) in line.iter().enumerate() {
            acc.push_str(w.word);
            if i + 1 < line.len() {
                acc.push_str(w.whitespace);
            } else {
                acc.push_str(w.penalty);
            }
        }
        out_lines.push(acc);
    }
    out_lines
}

pub(super) fn sentence_lines_for_paragraph(
    _config: &Config,
    node: &SyntaxNode,
    format_inline_fn: &dyn Fn(&SyntaxNode) -> String,
) -> Vec<String> {
    log::debug!("sentence_lines_for_paragraph called");

    let paragraph_text = node.text().to_string();
    let normalized = paragraph_text.replace("\r\n", "\n");
    let standalone_fences = normalized
        .lines()
        .filter(|line| line.trim() == "$$")
        .count();
    if standalone_fences >= 2 && standalone_fences % 2 == 0 {
        return paragraph_text
            .lines()
            .map(|line| line.trim_end().to_string())
            .collect();
    }

    let has_hard_breaks = node
        .descendants_with_tokens()
        .any(|el| el.kind() == SyntaxKind::HARD_LINE_BREAK);

    if has_hard_breaks {
        log::debug!("Paragraph contains hard line breaks - preserving them");

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
                        if _config.extensions.escaped_line_breaks {
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

        return result.lines().map(|s| s.to_string()).collect();
    }

    let mut arena: Vec<Box<str>> = Vec::new();
    let words = build_words_for_sentence(_config, node, &mut arena, format_inline_fn);
    log::debug!("Built {} words for paragraph", words.len());

    sentence_lines_from_words(&words)
}

use crate::config::{BlankLines, Config, WrapMode};
use crate::syntax::{SyntaxKind, SyntaxNode};

use rowan::NodeOrToken;
use textwrap::wrap_algorithms::WrapAlgorithm;

pub struct Formatter {
    output: String,
    config: Config,
    consecutive_blank_lines: usize,
    fenced_div_depth: usize,
}

fn is_block_element(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::PARAGRAPH
            | SyntaxKind::List
            | SyntaxKind::DefinitionList
            | SyntaxKind::BlockQuote
            | SyntaxKind::MathBlock
            | SyntaxKind::CodeBlock
            | SyntaxKind::SimpleTable
            | SyntaxKind::PipeTable
    )
}

impl Formatter {
    pub fn new(config: Config) -> Self {
        Self {
            output: String::with_capacity(8192),
            config,
            consecutive_blank_lines: 0,
            fenced_div_depth: 0,
        }
    }

    fn build_words<'a>(
        &self,
        node: &SyntaxNode,
        arena: &'a mut Vec<Box<str>>,
    ) -> Vec<textwrap::core::Word<'a>> {
        use rowan::NodeOrToken;

        struct Builder<'a> {
            arena: &'a mut Vec<Box<str>>,
            piece_idx: Vec<usize>,
            whitespace_after: Vec<bool>,
            last_piece_pos: Option<usize>,
            pending_space: bool,
        }

        impl<'a> Builder<'a> {
            fn new(arena: &'a mut Vec<Box<str>>) -> Self {
                Self {
                    arena,
                    piece_idx: Vec::new(),
                    whitespace_after: Vec::new(),
                    last_piece_pos: None,
                    pending_space: false,
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

        fn process_node_recursive(formatter: &Formatter, node: &SyntaxNode, b: &mut Builder) {
            for el in node.children_with_tokens() {
                match el {
                    NodeOrToken::Token(t) => match t.kind() {
                        SyntaxKind::WHITESPACE | SyntaxKind::NEWLINE | SyntaxKind::BlankLine => {
                            b.pending_space = true;
                        }
                        SyntaxKind::EscapedChar => {
                            let escaped = format!("\\{}", t.text());
                            b.push_piece(&escaped);
                        }
                        SyntaxKind::EmphasisMarker | SyntaxKind::StrongMarker => {
                            // Skip original markers - we'll add normalized ones
                        }
                        SyntaxKind::TEXT => {
                            let text = t.text();

                            // If text starts with 4+ spaces, it might be indented code - preserve as-is
                            if text.starts_with("    ") {
                                b.push_piece(text);
                            } else {
                                // Split TEXT tokens on whitespace to create separate words

                                // Check if text starts with whitespace
                                if !text.is_empty() && text.starts_with(char::is_whitespace) {
                                    b.pending_space = true;
                                }

                                let words: Vec<&str> = text.split_whitespace().collect();

                                for (i, word) in words.iter().enumerate() {
                                    if i > 0 {
                                        b.pending_space = true;
                                    }
                                    b.push_piece(word);
                                }

                                // If text ends with whitespace, mark pending space for next piece
                                if !words.is_empty() && text.ends_with(char::is_whitespace) {
                                    b.pending_space = true;
                                }
                            }
                        }
                        _ => {
                            b.push_piece(t.text());
                        }
                    },
                    NodeOrToken::Node(n) => match n.kind() {
                        SyntaxKind::List => {
                            b.pending_space = true;
                        }
                        SyntaxKind::Emphasis => {
                            b.push_piece("*");
                            process_node_recursive(formatter, &n, b);
                            b.push_piece("*");
                        }
                        SyntaxKind::Strong => {
                            b.push_piece("**");
                            process_node_recursive(formatter, &n, b);
                            b.push_piece("**");
                        }
                        _ => {
                            // For other inline nodes, format and push as single piece
                            let text = formatter.format_inline_node(&n);
                            b.push_piece(&text);
                        }
                    },
                }
            }
        }

        let mut b = Builder::new(arena);
        process_node_recursive(self, node, &mut b);

        let mut words: Vec<textwrap::core::Word<'a>> = Vec::with_capacity(b.piece_idx.len());
        for (i, &idx) in b.piece_idx.iter().enumerate() {
            let s: &'a str = &b.arena[idx];
            let mut w = textwrap::core::Word::from(s);
            if b.whitespace_after.get(i).copied().unwrap_or(false) {
                w.whitespace = " ";
            }
            words.push(w);
        }
        words
    }

    /// Format an inline node to normalized string (e.g., emphasis with asterisks)
    #[allow(clippy::only_used_in_recursion)]
    fn format_inline_node(&self, node: &SyntaxNode) -> String {
        use rowan::NodeOrToken;

        match node.kind() {
            SyntaxKind::Emphasis => {
                let mut content = String::new();
                for child in node.children_with_tokens() {
                    match child {
                        NodeOrToken::Node(n) => content.push_str(&self.format_inline_node(&n)),
                        NodeOrToken::Token(t) => {
                            if t.kind() != SyntaxKind::EmphasisMarker {
                                content.push_str(t.text());
                            }
                        }
                    }
                }
                format!("*{}*", content)
            }
            SyntaxKind::Strong => {
                let mut content = String::new();
                for child in node.children_with_tokens() {
                    match child {
                        NodeOrToken::Node(n) => content.push_str(&self.format_inline_node(&n)),
                        NodeOrToken::Token(t) => {
                            if t.kind() != SyntaxKind::StrongMarker {
                                content.push_str(t.text());
                            }
                        }
                    }
                }
                format!("**{}**", content)
            }
            _ => {
                // For other inline nodes, just return their text
                node.text().to_string()
            }
        }
    }

    fn wrapped_lines_for_paragraph(&self, node: &SyntaxNode, width: usize) -> Vec<String> {
        log::debug!("wrapped_lines_for_paragraph called with width={}", width);
        let mut arena: Vec<Box<str>> = Vec::new();
        let words = self.build_words(node, &mut arena);
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

    pub fn format(mut self, node: &SyntaxNode) -> String {
        self.format_node(node, 0);
        self.output
    }

    fn format_node(&mut self, node: &SyntaxNode, indent: usize) {
        // Reset blank line counter when we hit a non-blank node
        if node.kind() != SyntaxKind::BlankLine {
            self.consecutive_blank_lines = 0;
        }

        let line_width = self.config.line_width;

        match node.kind() {
            SyntaxKind::ROOT | SyntaxKind::DOCUMENT => {
                for el in node.children_with_tokens() {
                    match el {
                        rowan::NodeOrToken::Node(n) => self.format_node(&n, indent),
                        rowan::NodeOrToken::Token(t) => match t.kind() {
                            SyntaxKind::WHITESPACE => {}
                            SyntaxKind::NEWLINE => {}
                            SyntaxKind::BlankLine => {
                                self.output.push('\n');
                            }
                            SyntaxKind::EscapedChar => {
                                // Re-add the backslash for escaped characters
                                self.output.push('\\');
                                self.output.push_str(t.text());
                            }
                            SyntaxKind::ImageLinkStart
                            | SyntaxKind::LinkStart
                            | SyntaxKind::LatexCommand => {
                                self.output.push_str(t.text());
                            }
                            _ => self.output.push_str(t.text()),
                        },
                    }
                }
            }

            SyntaxKind::Heading => {
                log::trace!("Formatting heading");
                // Determine level
                let mut level = 1;
                let mut content = String::new();
                let mut saw_content = false;

                for child in node.children() {
                    match child.kind() {
                        SyntaxKind::AtxHeadingMarker => {
                            let t = child.text().to_string();
                            level = t.chars().take_while(|&c| c == '#').count().clamp(1, 6);
                        }
                        SyntaxKind::SetextHeadingUnderline => {
                            let t = child.text().to_string();
                            if t.chars().all(|c| c == '=') {
                                level = 1;
                            } else {
                                level = 2;
                            }
                        }
                        SyntaxKind::HeadingContent => {
                            let mut t = child.text().to_string();
                            // Trim trailing spaces and closing hashes in ATX form
                            t = t.trim_end().to_string();
                            // Remove trailing " ###" if present
                            let trimmed_hash = t.trim_end_matches('#').to_string();
                            if trimmed_hash.len() != t.len() {
                                t = trimmed_hash.trim_end().to_string();
                            }
                            // Normalize internal newlines
                            content = t.trim().to_string();
                            saw_content = true;
                        }
                        _ => {}
                    }
                }
                if !saw_content {
                    content = node.text().to_string();
                }
                self.output.push_str(&"#".repeat(level));
                self.output.push(' ');
                self.output.push_str(&content);
                self.output.push('\n');

                if let Some(next) = node.next_sibling()
                    && is_block_element(next.kind())
                    && !self.output.ends_with("\n\n")
                {
                    self.output.push('\n');
                }
            }

            SyntaxKind::HorizontalRule => {
                // Output normalized horizontal rule (always use "---")
                self.output.push_str("---");
                self.output.push('\n');

                // Ensure blank line after if followed by block element
                if let Some(next) = node.next_sibling()
                    && is_block_element(next.kind())
                    && !self.output.ends_with("\n\n")
                {
                    self.output.push('\n');
                }
            }

            SyntaxKind::LatexEnvironment => {
                // Output the environment exactly as written
                let text = node.text().to_string();
                self.output.push_str(&text);
                if !text.ends_with('\n') {
                    self.output.push('\n');
                }
            }

            SyntaxKind::Comment => {
                let text = node.text().to_string();
                self.output.push_str(&text);
                if !text.ends_with('\n') {
                    self.output.push('\n');
                }
            }

            SyntaxKind::LatexCommand => {
                // Standalone LaTeX commands - preserve exactly as written
                let text = node.text().to_string();
                self.output.push_str(&text);
                // Don't add extra newlines for standalone LaTeX commands
            }

            SyntaxKind::BlockQuote => {
                log::trace!("Formatting blockquote");
                // Determine nesting depth by counting ancestor BlockQuote nodes (including self)
                let mut depth = 0usize;
                let mut cur = Some(node.clone());
                while let Some(n) = cur {
                    if n.kind() == SyntaxKind::BlockQuote {
                        depth += 1;
                    }
                    cur = n.parent();
                }

                // Prefixes for quoted content and blank quoted lines
                let content_prefix = "> ".repeat(depth); // includes trailing space
                let blank_prefix = content_prefix.trim_end(); // no trailing space

                // Format children (paragraphs, blank lines) with proper > prefix per depth
                let wrap_mode = self.config.wrap.clone().unwrap_or(WrapMode::Reflow);

                for child in node.children() {
                    match child.kind() {
                        SyntaxKind::PARAGRAPH => match wrap_mode {
                            WrapMode::Preserve => {
                                let text = child.text().to_string();
                                for line in text.lines() {
                                    self.output.push_str(&content_prefix);
                                    self.output.push_str(line);
                                    self.output.push('\n');
                                }
                            }
                            WrapMode::Reflow => {
                                let width =
                                    self.config.line_width.saturating_sub(content_prefix.len());
                                let lines = self.wrapped_lines_for_paragraph(&child, width);
                                for line in lines {
                                    self.output.push_str(&content_prefix);
                                    self.output.push_str(&line);
                                    self.output.push('\n');
                                }
                            }
                        },
                        SyntaxKind::BlankLine => {
                            self.output.push_str(blank_prefix);
                            self.output.push('\n');
                        }
                        SyntaxKind::HorizontalRule => {
                            self.output.push_str(&content_prefix);
                            self.output.push_str("---");
                            self.output.push('\n');
                        }
                        SyntaxKind::Heading => {
                            // Format heading with blockquote prefix
                            let heading_text = self.format_heading(&child);
                            for line in heading_text.lines() {
                                self.output.push_str(&content_prefix);
                                self.output.push_str(line);
                                self.output.push('\n');
                            }
                        }
                        SyntaxKind::List => {
                            // Format list with blockquote prefix
                            // Save current output, format list to temp, then prefix each line
                            let saved_output = self.output.clone();
                            self.output.clear();
                            self.format_node(&child, indent);
                            let list_output = self.output.clone();
                            self.output = saved_output;

                            for line in list_output.lines() {
                                if line.is_empty() {
                                    self.output.push_str(blank_prefix);
                                } else {
                                    self.output.push_str(&content_prefix);
                                    self.output.push_str(line);
                                }
                                self.output.push('\n');
                            }
                        }
                        _ => {
                            // Handle other content within block quotes
                            self.format_node(&child, indent);
                        }
                    }
                }
            }

            SyntaxKind::PARAGRAPH => {
                let text = node.text().to_string();
                log::debug!("Formatting paragraph, text length: {}", text.len());

                // If paragraph contains display math across lines ($$\n...\n$$), preserve as-is
                // Check that it's actually dollar signs, not just any characters
                let has_multiline_display_math = text.contains("$$\n") || text.contains("\n$$");
                if has_multiline_display_math {
                    log::debug!("Paragraph has multiline display math, preserving");
                    self.output.push_str(&text);
                    if !self.output.ends_with('\n') {
                        self.output.push('\n');
                    }
                    return;
                }

                // Check if paragraph contains inline display math ($$...$$)
                // Only reformat if it's on a single line
                if self.contains_inline_display_math(node) {
                    log::debug!("Paragraph has inline display math");
                    self.format_paragraph_with_display_math(node, indent, line_width);
                    return;
                }

                let wrap_mode = self.config.wrap.clone().unwrap_or(WrapMode::Reflow);
                log::debug!(
                    "Paragraph wrap mode: {:?}, line_width: {}",
                    wrap_mode,
                    line_width
                );
                match wrap_mode {
                    WrapMode::Preserve => {
                        log::trace!("Preserving paragraph line breaks");
                        self.output.push_str(&text);
                        if !self.output.ends_with('\n') {
                            self.output.push('\n');
                        }
                    }
                    WrapMode::Reflow => {
                        log::trace!("Reflowing paragraph to {} width", line_width);
                        let lines = self.wrapped_lines_for_paragraph(node, line_width);

                        for (i, line) in lines.iter().enumerate() {
                            if i > 0 {
                                self.output.push('\n');
                            }
                            self.output.push_str(line);
                        }

                        if !self.output.ends_with('\n') {
                            self.output.push('\n');
                        }
                    }
                }
            }

            SyntaxKind::List => {
                // Add blank line before top-level lists (indent == 0) that follow content.
                // Don't add for nested lists (indent > 0) as they follow their parent item's content.
                if indent == 0 && !self.output.is_empty() && !self.output.ends_with("\n\n") {
                    self.output.push('\n');
                }
                let mut prev_was_item = false;
                for child in node.children() {
                    if child.kind() == SyntaxKind::ListItem {
                        if prev_was_item {
                            while self.output.ends_with("\n\n") {
                                self.output.pop();
                            }
                        }
                        prev_was_item = true;
                    }
                    if child.kind() == SyntaxKind::BlankLine {
                        continue;
                    }
                    self.format_node(&child, indent);
                }
                if !self.output.ends_with('\n') {
                    self.output.push('\n');
                }
            }

            SyntaxKind::DefinitionList => {
                // Add blank line before top-level definition lists
                if indent == 0 && !self.output.is_empty() && !self.output.ends_with("\n\n") {
                    self.output.push('\n');
                }
                for child in node.children() {
                    if child.kind() == SyntaxKind::BlankLine {
                        continue;
                    }
                    self.format_node(&child, indent);
                }
                if !self.output.ends_with('\n') {
                    self.output.push('\n');
                }
            }

            SyntaxKind::DefinitionItem => {
                // Format term and definitions in compact format (no blank lines)
                for child in node.children() {
                    if child.kind() == SyntaxKind::BlankLine {
                        continue; // Skip blank lines for compact format
                    }
                    self.format_node(&child, indent);
                }
            }

            SyntaxKind::Term => {
                // Format term - just emit text with newline
                for child in node.children_with_tokens() {
                    match child {
                        NodeOrToken::Token(tok) if tok.kind() == SyntaxKind::TEXT => {
                            self.output.push_str(tok.text());
                        }
                        NodeOrToken::Token(tok) if tok.kind() == SyntaxKind::NEWLINE => {
                            self.output.push('\n');
                        }
                        NodeOrToken::Node(n) => {
                            self.format_node(&n, indent);
                        }
                        _ => {}
                    }
                }
            }

            SyntaxKind::Definition => {
                // Format definition with marker and content
                self.output.push_str(":   ");
                for child in node.children_with_tokens() {
                    match child {
                        NodeOrToken::Token(tok) if tok.kind() == SyntaxKind::TEXT => {
                            self.output.push_str(tok.text());
                        }
                        NodeOrToken::Token(tok) if tok.kind() == SyntaxKind::NEWLINE => {
                            self.output.push('\n');
                        }
                        NodeOrToken::Token(tok) if tok.kind() == SyntaxKind::DefinitionMarker => {
                            // Skip - we already added `:   `
                        }
                        NodeOrToken::Token(tok) if tok.kind() == SyntaxKind::WHITESPACE => {
                            // Skip - we normalize spacing
                        }
                        NodeOrToken::Node(n) => {
                            self.format_node(&n, indent + 4);
                        }
                        _ => {}
                    }
                }
                if !self.output.ends_with('\n') {
                    self.output.push('\n');
                }
            }

            SyntaxKind::SimpleTable => {
                // Handle table with proper caption formatting
                for child in node.children() {
                    match child.kind() {
                        SyntaxKind::TableCaption => {
                            // Re-add the "Table:" prefix
                            self.output.push_str("Table: ");
                            self.output.push_str(&child.text().to_string());
                        }
                        _ => {
                            // For other table parts, preserve as-is
                            self.output.push_str(&child.text().to_string());
                        }
                    }
                }
            }

            SyntaxKind::PipeTable => {
                // Handle pipe table with proper caption formatting
                for child in node.children() {
                    match child.kind() {
                        SyntaxKind::TableCaption => {
                            // Re-add the "Table:" prefix
                            self.output.push_str("Table: ");
                            self.output.push_str(&child.text().to_string());
                        }
                        _ => {
                            // For other table parts, preserve as-is
                            self.output.push_str(&child.text().to_string());
                        }
                    }
                }
            }

            SyntaxKind::InlineMath => {
                // Check if this is display math (has BlockMathMarker)
                let is_display_math = node.children_with_tokens().any(|t| {
                    matches!(t, NodeOrToken::Token(tok) if tok.kind() == SyntaxKind::BlockMathMarker)
                });

                if is_display_math {
                    // Display math - output with space padding
                    self.output.push_str("$$ ");
                    for child in node.children() {
                        if child.kind() == SyntaxKind::TEXT {
                            self.output.push_str(&child.text().to_string());
                        }
                    }
                    self.output.push_str(" $$");
                } else {
                    // Regular inline math
                    for child in node.children() {
                        self.output.push_str(&child.text().to_string());
                    }
                }
            }

            SyntaxKind::ListItem => {
                // Compute indent and marker from leading tokens
                let mut marker = String::new();
                let mut local_indent = 0;
                let mut content_start = false;

                for el in node.children_with_tokens() {
                    match el {
                        NodeOrToken::Token(t) => match t.kind() {
                            SyntaxKind::WHITESPACE => {
                                if !content_start {
                                    local_indent += t.text().len();
                                }
                            }
                            SyntaxKind::ListMarker => {
                                marker = t.text().to_string();
                                content_start = true;
                            }
                            _ => {
                                content_start = true;
                            }
                        },
                        _ => {
                            content_start = true;
                        }
                    }
                }

                let total_indent = indent + local_indent;
                let hanging = marker.len() + 1 + total_indent; // +1 for the space after marker
                let available_width = self.config.line_width.saturating_sub(hanging);

                // Build words from the whole node, then drop the leading marker word
                let mut arena: Vec<Box<str>> = Vec::new();
                let mut words = self.build_words(node, &mut arena);
                if let Some(first) = words.first()
                    && first.word == marker
                {
                    // Remove the marker; we will print it ourselves with a following space
                    words.remove(0);
                }

                let algo = WrapAlgorithm::new();
                let line_widths = [available_width];
                let lines = algo.wrap(&words, &line_widths);

                for (i, line) in lines.iter().enumerate() {
                    if i == 0 {
                        self.output.push_str(&" ".repeat(total_indent));
                        self.output.push_str(&marker);
                        self.output.push(' ');
                    } else {
                        // Hanging indent includes marker + one space
                        self.output
                            .push_str(&" ".repeat(total_indent + marker.len() + 1));
                    }
                    for (j, w) in line.iter().enumerate() {
                        self.output.push_str(w.word);
                        if j + 1 < line.len() {
                            self.output.push_str(w.whitespace);
                        } else {
                            self.output.push_str(w.penalty);
                        }
                    }
                    self.output.push('\n');
                }

                // Format nested lists inside this list item aligned to the content column.
                for child in node.children() {
                    if child.kind() == SyntaxKind::List {
                        self.format_node(&child, total_indent + marker.len() + 1);
                    }
                }
            }

            SyntaxKind::FencedDiv => {
                // Use more colons for nested divs: 3 base + 2 per depth level
                let colon_count = 3 + (self.fenced_div_depth * 2);
                let colons = ":".repeat(colon_count);

                let mut attributes = None;

                for child in node.children() {
                    match child.kind() {
                        SyntaxKind::DivFenceOpen => {
                            // Extract and store attributes for later
                        }

                        SyntaxKind::DivInfo => {
                            attributes = Some(child.text().to_string());
                        }

                        SyntaxKind::DivFenceClose => {
                            // Will be handled after content
                        }

                        // Process any other child nodes (paragraphs, nested divs, etc.)
                        _ => {}
                    }
                }

                // Emit normalized opening fence
                if let Some(attrs) = &attributes {
                    self.output.push_str(&colons);
                    self.output.push(' ');
                    self.output.push_str(attrs);
                    self.output.push('\n');
                }

                // Increment depth for nested content
                self.fenced_div_depth += 1;

                // Process content
                for child in node.children() {
                    if !matches!(
                        child.kind(),
                        SyntaxKind::DivFenceOpen | SyntaxKind::DivInfo | SyntaxKind::DivFenceClose
                    ) {
                        self.format_node(&child, indent);
                    }
                }

                // Decrement depth after processing content
                self.fenced_div_depth -= 1;

                // Emit normalized closing fence
                self.output.push_str(&colons);
                self.output.push('\n');
            }

            SyntaxKind::InlineMathMarker => {
                // Output inline math as $...$ or $$...$$ (on the same line)
                self.output.push_str(node.text().to_string().trim());
            }

            SyntaxKind::MathBlock => {
                let mut label = None;
                let mut math_content = None;
                for child in node.children() {
                    match child.kind() {
                        SyntaxKind::MathContent => {
                            math_content = Some(child.text().to_string());
                        }
                        SyntaxKind::Attribute => {
                            label = Some(child.text().to_string().trim().to_string());
                        }
                        _ => {}
                    }
                }
                // Opening fence
                self.output.push_str("$$\n");
                // Math content
                if let Some(content) = math_content {
                    let math_indent = self.config.math_indent;
                    for line in content.trim().lines() {
                        self.output.push_str(&" ".repeat(math_indent));
                        self.output.push_str(line.trim_end());
                        self.output.push('\n');
                    }
                }
                // Closing fence (with label if present)
                self.output.push_str("$$");
                if let Some(lbl) = label {
                    self.output.push(' ');
                    self.output.push_str(&lbl);
                }
                self.output.push('\n');
            }

            SyntaxKind::CodeBlock => {
                log::trace!("Formatting code block");
                // Normalize code blocks to use backticks
                self.format_code_block(node);
            }

            SyntaxKind::YamlMetadata | SyntaxKind::PandocTitleBlock => {
                // Preserve these blocks as-is
                let text = node.text().to_string();
                self.output.push_str(&text);
                // Ensure these blocks end with appropriate spacing
                if !text.ends_with('\n') {
                    self.output.push('\n');
                }
            }

            SyntaxKind::BlankLine => {
                // Apply blank_lines config to collapse consecutive blank lines
                match self.config.blank_lines {
                    BlankLines::Preserve => {
                        // Always output blank line
                        self.output.push('\n');
                        self.consecutive_blank_lines += 1;
                    }
                    BlankLines::Collapse => {
                        // Only output if we haven't already output one blank line
                        if self.consecutive_blank_lines == 0 {
                            self.output.push('\n');
                            self.consecutive_blank_lines = 1;
                        }
                        // Otherwise skip this blank line (collapsing to one)
                    }
                }
            }

            SyntaxKind::Emphasis => {
                // Normalize emphasis to always use single asterisks
                self.output.push('*');
                for child in node.children_with_tokens() {
                    match child {
                        rowan::NodeOrToken::Node(n) => self.format_node(&n, indent),
                        rowan::NodeOrToken::Token(t) => {
                            if t.kind() != SyntaxKind::EmphasisMarker {
                                self.output.push_str(t.text());
                            }
                        }
                    }
                }
                self.output.push('*');
            }

            SyntaxKind::Strong => {
                // Normalize strong emphasis to always use double asterisks
                self.output.push_str("**");
                for child in node.children_with_tokens() {
                    match child {
                        rowan::NodeOrToken::Node(n) => self.format_node(&n, indent),
                        rowan::NodeOrToken::Token(t) => {
                            if t.kind() != SyntaxKind::StrongMarker {
                                self.output.push_str(t.text());
                            }
                        }
                    }
                }
                self.output.push_str("**");
            }

            _ => {
                // Fallback: append node text (should be rare with children_with_tokens above)
                self.output.push_str(&node.text().to_string());
            }
        }
    }

    /// Format a heading and return its text (without adding to output).
    fn format_heading(&self, node: &SyntaxNode) -> String {
        let mut level = 1;
        let mut content = String::new();
        let mut saw_content = false;

        for child in node.children() {
            match child.kind() {
                SyntaxKind::AtxHeadingMarker => {
                    let t = child.text().to_string();
                    level = t.chars().take_while(|&c| c == '#').count().clamp(1, 6);
                }
                SyntaxKind::SetextHeadingUnderline => {
                    let t = child.text().to_string();
                    if t.chars().all(|c| c == '=') {
                        level = 1;
                    } else {
                        level = 2;
                    }
                }
                SyntaxKind::HeadingContent => {
                    let mut t = child.text().to_string();
                    t = t.trim_end().to_string();
                    let trimmed_hash = t.trim_end_matches('#').to_string();
                    if trimmed_hash.len() != t.len() {
                        t = trimmed_hash.trim_end().to_string();
                    }
                    content = t.trim().to_string();
                    saw_content = true;
                }
                _ => {}
            }
        }
        if !saw_content {
            content = node.text().to_string();
        }

        format!("{} {}", "#".repeat(level), content)
    }

    /// Check if display math in paragraph is already formatted on separate lines
    fn contains_inline_display_math(&self, node: &SyntaxNode) -> bool {
        for child in node.descendants() {
            if child.kind() == SyntaxKind::InlineMath {
                // Check if it contains BlockMathMarker ($$)
                for token in child.children_with_tokens() {
                    if let NodeOrToken::Token(t) = token
                        && t.kind() == SyntaxKind::BlockMathMarker
                    {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Format a paragraph that contains inline display math by splitting it.
    /// Converts: "Some text $$x = y$$ more text" into text with display math formatted.
    fn format_paragraph_with_display_math(
        &mut self,
        node: &SyntaxNode,
        _indent: usize,
        line_width: usize,
    ) {
        let mut parts: Vec<(bool, String)> = Vec::new(); // (is_display_math, content)
        let mut current_text = String::new();

        for child in node.children_with_tokens() {
            match child {
                NodeOrToken::Node(n) => {
                    if n.kind() == SyntaxKind::InlineMath {
                        // Check if this is display math
                        let has_block_marker = n.children_with_tokens().any(|t| {
                            matches!(t, NodeOrToken::Token(tok) if tok.kind() == SyntaxKind::BlockMathMarker)
                        });

                        if has_block_marker {
                            // Save current text as paragraph part
                            if !current_text.trim().is_empty() {
                                parts.push((false, current_text.clone()));
                                current_text.clear();
                            }

                            // Extract math content
                            let math_content: String = n
                                .children_with_tokens()
                                .filter_map(|c| match c {
                                    NodeOrToken::Token(t) if t.kind() == SyntaxKind::TEXT => {
                                        Some(t.text().to_string())
                                    }
                                    _ => None,
                                })
                                .collect();

                            parts.push((true, math_content));
                        } else {
                            // Regular inline math - keep in text
                            current_text.push_str(&n.text().to_string());
                        }
                    } else {
                        current_text.push_str(&n.text().to_string());
                    }
                }
                NodeOrToken::Token(t) => {
                    if t.kind() != SyntaxKind::NEWLINE {
                        current_text.push_str(t.text());
                    } else {
                        current_text.push(' '); // Replace newlines with spaces for wrapping
                    }
                }
            }
        }

        // Save any remaining text
        if !current_text.trim().is_empty() {
            parts.push((false, current_text));
        }

        // Format each part - display math on separate lines within paragraph
        for (i, (is_display_math, content)) in parts.iter().enumerate() {
            if *is_display_math {
                // Format as display math on separate lines
                self.output.push('\n');
                self.output.push_str("$$\n");
                self.output.push_str(content.trim());
                self.output.push_str("\n$$\n");
            } else {
                // Add space before if not at start
                if i > 0 && !self.output.ends_with('\n') {
                    self.output.push('\n');
                }

                // Format as paragraph text with wrapping
                let text = content.trim();
                if !text.is_empty() {
                    let lines = textwrap::wrap(text, line_width);
                    for (j, line) in lines.iter().enumerate() {
                        if j > 0 {
                            self.output.push('\n');
                        }
                        self.output.push_str(line);
                    }
                }
            }
        }

        // End with newline
        if !self.output.ends_with('\n') {
            self.output.push('\n');
        }
    }

    /// Format a code block, normalizing fence markers to backticks
    fn format_code_block(&mut self, node: &SyntaxNode) {
        use rowan::NodeOrToken;

        let mut info_string = String::new();
        let mut content = String::new();

        // Extract info string and content from the AST
        for child in node.children_with_tokens() {
            if let NodeOrToken::Node(n) = child {
                match n.kind() {
                    SyntaxKind::CodeFenceOpen => {
                        // Find the info string
                        for token in n.children_with_tokens() {
                            if let NodeOrToken::Token(t) = token
                                && t.kind() == SyntaxKind::CodeInfo
                            {
                                info_string = t.text().to_string();
                            }
                        }
                    }
                    SyntaxKind::CodeContent => {
                        content = n.text().to_string();
                    }
                    _ => {}
                }
            }
        }

        // Output normalized code block with exactly 3 backticks
        self.output.push_str("```");
        if !info_string.is_empty() {
            self.output.push_str(&info_string);
        }
        self.output.push('\n');
        self.output.push_str(&content);
        self.output.push_str("```");
        self.output.push('\n');
    }
}

pub fn format_tree(tree: &SyntaxNode, config: &Config) -> String {
    log::info!(
        "Formatting document with config: line_width={}, wrap={:?}",
        config.line_width,
        config.wrap
    );
    let result = Formatter::new(config.clone()).format(tree);
    log::info!("Formatting complete: {} bytes output", result.len());
    result
}

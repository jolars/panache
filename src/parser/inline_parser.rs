use crate::config::Config;
use crate::syntax::{SyntaxKind, SyntaxNode, SyntaxToken};
use rowan::{GreenNode, GreenNodeBuilder};

mod architecture_tests;
mod bracketed_spans;
mod citations;
mod code_spans;
mod emphasis;
mod escapes;
mod inline_footnotes;
mod inline_math;
mod latex;
pub mod links; // Public for try_parse_inline_image used by block parser
mod native_spans;
mod raw_inline;
mod shortcodes;
mod strikeout;
mod subscript;
mod superscript;
mod tests;

use bracketed_spans::{emit_bracketed_span, try_parse_bracketed_span};
use citations::{
    emit_bare_citation, emit_bracketed_citation, try_parse_bare_citation,
    try_parse_bracketed_citation,
};
use code_spans::{emit_code_span, try_parse_code_span};
use emphasis::{emit_emphasis, try_parse_emphasis};
use escapes::{emit_escape, try_parse_escape};
use inline_footnotes::{
    emit_footnote_reference, emit_inline_footnote, try_parse_footnote_reference,
    try_parse_inline_footnote,
};
use inline_math::{
    emit_display_math, emit_double_backslash_display_math, emit_double_backslash_inline_math,
    emit_inline_math, emit_single_backslash_display_math, emit_single_backslash_inline_math,
    try_parse_display_math, try_parse_double_backslash_display_math,
    try_parse_double_backslash_inline_math, try_parse_inline_math,
    try_parse_single_backslash_display_math, try_parse_single_backslash_inline_math,
};
use latex::{parse_latex_command, try_parse_latex_command};
use links::{
    emit_autolink, emit_inline_image, emit_inline_link, emit_reference_image, emit_reference_link,
    try_parse_autolink, try_parse_inline_image, try_parse_inline_link, try_parse_reference_image,
    try_parse_reference_link,
};
use native_spans::{emit_native_span, try_parse_native_span};
use raw_inline::{emit_raw_inline, is_raw_inline};
use shortcodes::{emit_shortcode, try_parse_shortcode};
use strikeout::{emit_strikeout, try_parse_strikeout};
use subscript::{emit_subscript, try_parse_subscript};
use superscript::{emit_superscript, try_parse_superscript};

/// A token stream that allows lookahead across TEXT and NEWLINE token boundaries.
/// This is essential for parsing inline elements that can span multiple lines,
/// such as display math: `$$\nx = y\n$$`
///
/// The TokenStream preserves the original token structure (TEXT vs NEWLINE) to
/// maintain lossless parsing while providing convenient text extraction for
/// pattern matching.
pub struct TokenStream {
    /// The tokens to iterate over (TEXT and NEWLINE tokens from a paragraph)
    tokens: Vec<SyntaxToken>,
    /// Current position in the token stream
    position: usize,
    /// Cumulative byte offset tracking (for source position mapping)
    offset: usize,
}

impl TokenStream {
    /// Create a new TokenStream from a vector of tokens
    pub fn new(tokens: Vec<SyntaxToken>) -> Self {
        Self {
            tokens,
            position: 0,
            offset: 0,
        }
    }

    /// Create a TokenStream from a single text string (for nested/recursive parsing)
    /// This creates a single TEXT token internally
    pub fn from_text(text: &str) -> Self {
        if text.is_empty() {
            return Self::new(Vec::new());
        }

        // We can't create a proper SyntaxToken without going through the builder,
        // but for recursive cases we'll need a different approach.
        // For now, mark this as TODO and we'll handle it in Phase 2.
        unimplemented!("from_text will be implemented in Phase 2 for recursive parsing")
    }

    /// Peek at the current token without consuming it
    pub fn peek(&self) -> Option<&SyntaxToken> {
        self.tokens.get(self.position)
    }

    /// Peek at a token N positions ahead without consuming
    pub fn peek_ahead(&self, n: usize) -> Option<&SyntaxToken> {
        self.tokens.get(self.position + n)
    }

    /// Consume and return the current token, advancing the stream
    pub fn advance(&mut self) -> Option<SyntaxToken> {
        if self.position < self.tokens.len() {
            let token = self.tokens[self.position].clone();
            self.offset += token.text().len();
            self.position += 1;
            Some(token)
        } else {
            None
        }
    }

    /// Get the current byte offset in the original source
    pub fn current_offset(&self) -> usize {
        self.offset
    }

    /// Check if we're at the end of the stream
    pub fn is_at_end(&self) -> bool {
        self.position >= self.tokens.len()
    }

    /// Collect text from current position up to N tokens ahead.
    /// This concatenates TEXT and NEWLINE tokens into a single string for pattern matching.
    /// Does not consume tokens.
    pub fn peek_text_ahead(&self, token_count: usize) -> String {
        let mut result = String::new();
        for i in 0..token_count {
            if let Some(token) = self.peek_ahead(i) {
                result.push_str(token.text());
            } else {
                break;
            }
        }
        result
    }

    /// Collect text from current position until a predicate returns false.
    /// This is useful for collecting all TEXT/NEWLINE tokens in a sequence.
    /// Does not consume tokens.
    pub fn peek_text_while<F>(&self, mut predicate: F) -> String
    where
        F: FnMut(&SyntaxToken) -> bool,
    {
        let mut result = String::new();
        let mut i = 0;
        while let Some(token) = self.peek_ahead(i) {
            if !predicate(token) {
                break;
            }
            result.push_str(token.text());
            i += 1;
        }
        result
    }

    /// Get remaining tokens count
    pub fn remaining(&self) -> usize {
        self.tokens.len().saturating_sub(self.position)
    }

    /// Consume exactly N bytes of text by consuming tokens.
    /// Returns the consumed tokens. Used after a pattern match to consume
    /// exactly the matched text.
    ///
    /// If byte_count ends in the middle of a token, that token is NOT consumed.
    /// The caller must handle partial token consumption separately.
    pub fn consume_bytes(&mut self, byte_count: usize) -> Vec<SyntaxToken> {
        let mut consumed = Vec::new();
        let mut bytes_consumed = 0;

        while bytes_consumed < byte_count && !self.is_at_end() {
            let next_token = self.peek();
            if next_token.is_none() {
                break;
            }

            let token = next_token.unwrap();
            let token_len = token.text().len();

            // If adding this entire token would exceed byte_count, stop
            if bytes_consumed + token_len > byte_count {
                break;
            }

            // Consume this token completely
            let token = self.advance().unwrap();
            bytes_consumed += token_len;
            consumed.push(token);
        }

        consumed
    }

    /// Get the number of bytes that have been peeked but not yet consumed.
    /// This is used to track partial token consumption scenarios.
    pub fn bytes_until_position(&self) -> usize {
        self.offset
    }

    /// Consume exactly N bytes from the current position, handling partial token consumption.
    /// Returns (consumed_tokens, remaining_text_in_partial_token).
    ///
    /// This is used by inline math parsing to handle cases where a pattern match
    /// ends in the middle of a TEXT token (e.g., "$$ some text" where we only want "$$").
    ///
    /// When there's a partial token, this function:
    /// 1. Consumes all complete tokens
    /// 2. Advances past the partial token (consuming it)
    /// 3. Returns the unconsumed portion of that token for the caller to process
    pub fn consume_bytes_with_partial(
        &mut self,
        byte_count: usize,
    ) -> (Vec<SyntaxToken>, Option<String>) {
        let mut consumed = Vec::new();
        let mut bytes_consumed = 0;

        while bytes_consumed < byte_count && !self.is_at_end() {
            let next_token = self.peek();
            if next_token.is_none() {
                break;
            }

            let token_text = next_token.unwrap().text().to_string(); // Copy the text
            let token_len = token_text.len();

            // Check if we need partial consumption
            if bytes_consumed + token_len > byte_count {
                // We need to split this token
                let bytes_needed = byte_count - bytes_consumed;
                let remaining_part = token_text[bytes_needed..].to_string();

                // Advance past this token (it's being consumed, even if only partially)
                self.advance();

                // Return the remaining part for the caller to process
                return (consumed, Some(remaining_part));
            }

            // Consume this token completely
            let token = self.advance().unwrap();
            bytes_consumed += token_len;
            consumed.push(token);
        }

        (consumed, None)
    }
}

/// Parse inline elements from a token stream.
/// This is the core inline parsing function that handles multi-line patterns
/// like display math by looking ahead across TEXT/NEWLINE token boundaries.
///
/// The `reference_registry` parameter is optional - when None, reference links/images
/// won't be resolved (useful for nested contexts like link text).
pub fn parse_inline_tokens(
    builder: &mut GreenNodeBuilder,
    tokens: &mut TokenStream,
    config: &Config,
    reference_registry: Option<&crate::parser::block_parser::ReferenceRegistry>,
) {
    log::trace!(
        "Parsing inline tokens: {} tokens remaining",
        tokens.remaining(),
    );

    // Process tokens one at a time, preserving NEWLINE vs TEXT distinction.
    // Only collect text across tokens when checking for multi-line display math.

    while !tokens.is_at_end() {
        let current_token = tokens.peek();
        if current_token.is_none() {
            break;
        }

        let current = current_token.unwrap();

        // NEWLINE tokens always pass through as-is (losslessness)
        if current.kind() == SyntaxKind::NEWLINE {
            let newline = tokens.advance().unwrap();
            builder.token(SyntaxKind::NEWLINE.into(), newline.text());
            continue;
        }

        // For TEXT tokens, check if we should parse inline elements
        if current.kind() == SyntaxKind::TEXT {
            let text = current.text();

            // Check if this TEXT token might start a multi-line display math
            // by looking for $$ or \[ or \\[ at the beginning
            let might_be_multiline_math =
                text.starts_with("$$") || text.starts_with("\\[") || text.starts_with("\\\\[");

            if might_be_multiline_math {
                // Look ahead across ALL remaining tokens to find potential multi-line pattern
                // Don't limit lookahead - let the parsing function decide when to stop
                let lookahead = tokens.peek_text_ahead(tokens.remaining());

                // Try to parse multi-line display math
                let mut matched = false;

                // Try $$...$$
                if let Some((len, content)) = try_parse_display_math(&lookahead) {
                    let dollar_count = lookahead.chars().take_while(|&c| c == '$').count();
                    log::debug!("Matched multi-token display math: {} bytes", len);

                    // Check for trailing attributes (Quarto cross-reference support)
                    let after_math = &lookahead[len..];
                    log::debug!(
                        "After display math: {:?}, quarto_crossrefs={}",
                        &after_math[..after_math.len().min(30)],
                        config.extensions.quarto_crossrefs
                    );
                    let attr_len = if config.extensions.quarto_crossrefs {
                        use crate::parser::block_parser::attributes::try_parse_trailing_attributes;
                        if let Some((_attr_block, _)) = try_parse_trailing_attributes(after_math) {
                            log::debug!("Found attributes after display math");
                            // Find the position of { in after_math
                            let trimmed_after = after_math.trim_start();
                            if let Some(open_brace_pos) = trimmed_after.find('{') {
                                // Calculate total attribute length including leading whitespace
                                let ws_before_brace = after_math.len() - trimmed_after.len();
                                let attr_text_len = trimmed_after[open_brace_pos..]
                                    .find('}')
                                    .map(|close| close + 1)
                                    .unwrap_or(0);
                                ws_before_brace + open_brace_pos + attr_text_len
                            } else {
                                0
                            }
                        } else {
                            log::debug!("No attributes found after display math");
                            0
                        }
                    } else {
                        0
                    };

                    let total_len = len + attr_len;
                    let (_consumed_tokens, remaining_text) =
                        tokens.consume_bytes_with_partial(total_len);

                    // Emit the display math and attributes
                    emit_display_math(builder, content, dollar_count);

                    // Emit attributes if present
                    if attr_len > 0 {
                        use crate::parser::block_parser::attributes::{
                            emit_attributes, try_parse_trailing_attributes,
                        };
                        let attr_text = &lookahead[len..total_len];
                        if let Some((attr_block, _text_before)) =
                            try_parse_trailing_attributes(attr_text)
                        {
                            // Emit whitespace before attributes
                            let trimmed_after = attr_text.trim_start();
                            let ws_len = attr_text.len() - trimmed_after.len();
                            if ws_len > 0 {
                                builder.token(SyntaxKind::WHITESPACE.into(), &attr_text[..ws_len]);
                            }
                            emit_attributes(builder, &attr_block);
                        }
                    }

                    // If there's remaining text in a partially consumed token, process it
                    if let Some(remaining) = remaining_text {
                        log::debug!(
                            "Processing remaining text after display math: {:?}",
                            &remaining[..remaining.len().min(40)]
                        );
                        parse_inline_text(builder, &remaining, config, reference_registry);
                    }
                    matched = true;
                }
                // Try \[...\]
                else if config.extensions.tex_math_single_backslash {
                    if let Some((len, content)) =
                        try_parse_single_backslash_display_math(&lookahead)
                    {
                        log::debug!("Matched multi-token single backslash display math");
                        let (_consumed_tokens, remaining_text) =
                            tokens.consume_bytes_with_partial(len);
                        emit_single_backslash_display_math(builder, content);

                        // If there's remaining text in a partially consumed token, process it
                        if let Some(remaining) = remaining_text {
                            log::debug!(
                                "Processing remaining text after single backslash display math"
                            );
                            parse_inline_text(builder, &remaining, config, reference_registry);
                        }
                        matched = true;
                    }
                }
                // Try \\[...\\]
                else if config.extensions.tex_math_double_backslash
                    && let Some((len, content)) =
                        try_parse_double_backslash_display_math(&lookahead)
                {
                    log::debug!("Matched multi-token double backslash display math");
                    let (_consumed_tokens, remaining_text) = tokens.consume_bytes_with_partial(len);
                    emit_double_backslash_display_math(builder, content);

                    // If there's remaining text in a partially consumed token, process it
                    if let Some(remaining) = remaining_text {
                        log::debug!(
                            "Processing remaining text after double backslash display math"
                        );
                        parse_inline_text(builder, &remaining, config, reference_registry);
                    }
                    matched = true;
                }

                if matched {
                    continue;
                }
            }

            // No multi-line pattern matched, parse this TEXT token normally
            let token = tokens.advance().unwrap();
            parse_inline_text(builder, token.text(), config, reference_registry);
        } else {
            // Other token types pass through as-is
            let token = tokens.advance().unwrap();
            builder.token(token.kind().into(), token.text());
        }
    }
}

/// Parse inline elements from text content.
/// This is a standalone function used by both the main inline parser
/// and by nested contexts like link text.
///
/// The `reference_registry` parameter is optional - when None, reference links/images
/// won't be resolved (useful for nested contexts like link text).
pub fn parse_inline_text(
    builder: &mut GreenNodeBuilder,
    text: &str,
    config: &Config,
    reference_registry: Option<&crate::parser::block_parser::ReferenceRegistry>,
) {
    log::trace!(
        "Parsing inline text: {:?} ({} bytes), tex_math_single_backslash={}",
        &text[..text.len().min(40)],
        text.len(),
        config.extensions.tex_math_single_backslash
    );
    let mut pos = 0;
    let bytes = text.as_bytes();

    while pos < text.len() {
        // Try to parse backslash math FIRST (when enabled)
        // These take precedence over escapes per Pandoc spec
        // Single backslash math: \(...\) and \[...\]
        if bytes[pos] == b'\\'
            && pos + 1 < text.len()
            && config.extensions.tex_math_single_backslash
        {
            // Try display math first: \[...\]
            if bytes[pos + 1] == b'['
                && let Some((len, content)) = try_parse_single_backslash_display_math(&text[pos..])
            {
                log::debug!("Matched single backslash display math at pos {}", pos);
                emit_single_backslash_display_math(builder, content);
                pos += len;
                continue;
            }

            // Try inline math: \(...\)
            if bytes[pos + 1] == b'('
                && let Some((len, content)) = try_parse_single_backslash_inline_math(&text[pos..])
            {
                log::debug!("Matched single backslash inline math at pos {}", pos);
                emit_single_backslash_inline_math(builder, content);
                pos += len;
                continue;
            }
        }

        // Double backslash math: \\(...\\) and \\[...\\]
        if bytes[pos] == b'\\'
            && pos + 2 < text.len()
            && bytes[pos + 1] == b'\\'
            && config.extensions.tex_math_double_backslash
        {
            // Try display math first: \\[...\\]
            if bytes[pos + 2] == b'['
                && let Some((len, content)) = try_parse_double_backslash_display_math(&text[pos..])
            {
                log::debug!("Matched double backslash display math at pos {}", pos);
                emit_double_backslash_display_math(builder, content);
                pos += len;
                continue;
            }

            // Try inline math: \\(...\\)
            if bytes[pos + 2] == b'('
                && let Some((len, content)) = try_parse_double_backslash_inline_math(&text[pos..])
            {
                log::debug!("Matched double backslash inline math at pos {}", pos);
                emit_double_backslash_inline_math(builder, content);
                pos += len;
                continue;
            }
        }

        // Try to parse backslash escape (after math checks when enabled)
        // This prevents escaped delimiters from being parsed
        if bytes[pos] == b'\\'
            && let Some((len, ch, escape_type)) = try_parse_escape(&text[pos..])
        {
            log::debug!("Matched escape at pos {}: \\{}", pos, ch);
            emit_escape(builder, ch, escape_type);
            pos += len;
            continue;
        }

        // Try to parse LaTeX command (after escapes, before code)
        // This handles \cite{ref}, \textbf{text}, etc.
        if bytes[pos] == b'\\'
            && let Some(len) = try_parse_latex_command(&text[pos..])
        {
            log::debug!("Matched LaTeX command at pos {}", pos);
            parse_latex_command(builder, &text[pos..], len);
            pos += len;
            continue;
        }

        // Try to parse Quarto shortcodes (before code spans, since both can start with braces)
        if bytes[pos] == b'{'
            && config.extensions.quarto_shortcodes
            && let Some((len, content, is_escaped)) = try_parse_shortcode(&text[pos..])
        {
            log::debug!(
                "Matched shortcode at pos {}: escaped={}, content={:?}",
                pos,
                is_escaped,
                &content[..content.len().min(20)]
            );
            emit_shortcode(builder, &content, is_escaped);
            pos += len;
            continue;
        }

        // Try to parse code span or raw inline span
        if bytes[pos] == b'`'
            && let Some((len, content, backtick_count, attributes)) =
                try_parse_code_span(&text[pos..])
        {
            log::debug!(
                "Matched code span at pos {}: {} backticks, attributes={:?}",
                pos,
                backtick_count,
                attributes
            );

            // Check if this is a raw inline span (has {=format} attribute)
            if let Some(ref attrs) = attributes
                && config.extensions.raw_attribute
                && let Some(format) = is_raw_inline(attrs)
            {
                log::debug!("Matched raw inline span at pos {}: format={}", pos, format);
                emit_raw_inline(builder, content, backtick_count, format);
                pos += len;
                continue;
            }

            // Regular code span
            emit_code_span(builder, content, backtick_count, attributes);
            pos += len;
            continue;
        }

        // Try to parse inline footnote (^[...])
        if bytes[pos] == b'^'
            && pos + 1 < text.len()
            && bytes[pos + 1] == b'['
            && let Some((len, content)) = try_parse_inline_footnote(&text[pos..])
        {
            log::debug!("Matched inline footnote at pos {}", pos);
            emit_inline_footnote(builder, content, config);
            pos += len;
            continue;
        }

        // Try to parse superscript (^text^)
        // Must come after inline footnote check to avoid conflict with ^[
        if bytes[pos] == b'^'
            && let Some((len, content)) = try_parse_superscript(&text[pos..])
        {
            log::debug!("Matched superscript at pos {}", pos);
            emit_superscript(builder, content, config);
            pos += len;
            continue;
        }

        // Try to parse subscript (~text~)
        // Must come before strikeout check to avoid conflict with ~~
        if bytes[pos] == b'~'
            && let Some((len, content)) = try_parse_subscript(&text[pos..])
        {
            log::debug!("Matched subscript at pos {}", pos);
            emit_subscript(builder, content, config);
            pos += len;
            continue;
        }

        // Try to parse strikeout (~~text~~)
        if bytes[pos] == b'~'
            && pos + 1 < text.len()
            && bytes[pos + 1] == b'~'
            && let Some((len, content)) = try_parse_strikeout(&text[pos..])
        {
            log::debug!("Matched strikeout at pos {}", pos);
            emit_strikeout(builder, content, config);
            pos += len;
            continue;
        }

        // Try to parse inline math (must check for $$ first for display math)
        if bytes[pos] == b'$' {
            // Try display math first ($$...$$)
            if let Some((len, content)) = try_parse_display_math(&text[pos..]) {
                let dollar_count = text[pos..].chars().take_while(|&c| c == '$').count();
                log::debug!(
                    "Matched display math at pos {}: {} dollars",
                    pos,
                    dollar_count
                );

                // Check for trailing attributes (Quarto cross-reference support)
                let after_math = &text[pos + len..];
                let attr_len = if config.extensions.quarto_crossrefs {
                    use crate::parser::block_parser::attributes::try_parse_trailing_attributes;
                    if let Some((_attr_block, _)) = try_parse_trailing_attributes(after_math) {
                        log::debug!("Found attributes after inline display math");
                        // Find the position of { in after_math
                        let trimmed_after = after_math.trim_start();
                        if let Some(open_brace_pos) = trimmed_after.find('{') {
                            // Calculate total attribute length including leading whitespace
                            let ws_before_brace = after_math.len() - trimmed_after.len();
                            let attr_text_len = trimmed_after[open_brace_pos..]
                                .find('}')
                                .map(|close| close + 1)
                                .unwrap_or(0);
                            ws_before_brace + open_brace_pos + attr_text_len
                        } else {
                            0
                        }
                    } else {
                        0
                    }
                } else {
                    0
                };

                // Emit display math
                emit_display_math(builder, content, dollar_count);

                // Emit attributes if present
                if attr_len > 0 {
                    use crate::parser::block_parser::attributes::{
                        emit_attributes, try_parse_trailing_attributes,
                    };
                    let attr_text = &text[pos + len..pos + len + attr_len];
                    if let Some((attr_block, _)) = try_parse_trailing_attributes(attr_text) {
                        // Emit whitespace before attributes
                        let trimmed_after = attr_text.trim_start();
                        let ws_len = attr_text.len() - trimmed_after.len();
                        if ws_len > 0 {
                            builder.token(SyntaxKind::WHITESPACE.into(), &attr_text[..ws_len]);
                        }
                        emit_attributes(builder, &attr_block);
                    }
                }

                pos += len + attr_len;
                continue;
            }

            // Try inline math ($...$)
            if let Some((len, content)) = try_parse_inline_math(&text[pos..]) {
                log::debug!("Matched inline math at pos {}", pos);
                emit_inline_math(builder, content);
                pos += len;
                continue;
            }
        }

        // Try to parse automatic link
        if bytes[pos] == b'<'
            && let Some((len, url)) = try_parse_autolink(&text[pos..])
        {
            log::debug!("Matched autolink at pos {}: {}", pos, url);
            emit_autolink(builder, &text[pos..pos + len], url);
            pos += len;
            continue;
        }

        // Try to parse native span (after autolink since both start with <)
        if bytes[pos] == b'<'
            && let Some((len, content, attributes)) = try_parse_native_span(&text[pos..])
        {
            log::debug!("Matched native span at pos {}", pos);
            emit_native_span(builder, content, &attributes, config);
            pos += len;
            continue;
        }

        // Try to parse inline image (must come before inline link since it starts with ![)
        if pos + 1 < text.len()
            && bytes[pos] == b'!'
            && bytes[pos + 1] == b'['
            && let Some((len, alt_text, dest, attributes)) = try_parse_inline_image(&text[pos..])
        {
            log::debug!("Matched inline image at pos {}: dest={}", pos, dest);
            emit_inline_image(
                builder,
                &text[pos..pos + len],
                alt_text,
                dest,
                attributes,
                config,
            );
            pos += len;
            continue;
        }

        // Try to parse reference image (after inline image check)
        // Only if we have a registry and reference_links extension is enabled
        if pos + 1 < text.len()
            && bytes[pos] == b'!'
            && bytes[pos + 1] == b'['
            && config.extensions.reference_links
            && let Some(_registry) = reference_registry
        {
            let allow_shortcut = config.extensions.shortcut_reference_links;
            if let Some((len, alt_text, label, is_shortcut)) =
                try_parse_reference_image(&text[pos..], allow_shortcut)
            {
                log::debug!(
                    "Matched reference image at pos {}: label={:?}, shortcut={}",
                    pos,
                    label,
                    is_shortcut
                );
                emit_reference_image(builder, alt_text, &label, is_shortcut, config);
                pos += len;
                continue;
            }
        }

        // Try to parse footnote reference [^id] (before inline/reference links)
        // Only if footnotes extension is enabled
        if bytes[pos] == b'['
            && pos + 1 < text.len()
            && bytes[pos + 1] == b'^'
            && config.extensions.footnotes
            && let Some((len, id)) = try_parse_footnote_reference(&text[pos..])
        {
            log::debug!("Matched footnote reference at pos {}: [^{}]", pos, id);
            emit_footnote_reference(builder, &id);
            pos += len;
            continue;
        }

        // Try to parse inline link
        if bytes[pos] == b'['
            && let Some((len, link_text, dest, attributes)) = try_parse_inline_link(&text[pos..])
        {
            log::debug!("Matched inline link at pos {}: dest={}", pos, dest);
            emit_inline_link(
                builder,
                &text[pos..pos + len],
                link_text,
                dest,
                attributes,
                config,
            );
            pos += len;
            continue;
        }

        // Try to parse reference link (after inline link check)
        // Only if we have a registry and reference_links extension is enabled
        if bytes[pos] == b'['
            && config.extensions.reference_links
            && let Some(_registry) = reference_registry
        {
            let allow_shortcut = config.extensions.shortcut_reference_links;
            if let Some((len, link_text, label, is_shortcut)) =
                try_parse_reference_link(&text[pos..], allow_shortcut)
            {
                log::debug!(
                    "Matched reference link at pos {}: label={:?}, shortcut={}",
                    pos,
                    label,
                    is_shortcut
                );
                emit_reference_link(builder, link_text, &label, is_shortcut, config);
                pos += len;
                continue;
            }
        }

        // Try to parse bracketed citation (after link since both start with [)
        if bytes[pos] == b'['
            && let Some((len, content)) = try_parse_bracketed_citation(&text[pos..])
        {
            log::debug!("Matched bracketed citation at pos {}", pos);
            emit_bracketed_citation(builder, content);
            pos += len;
            continue;
        }

        // Try to parse bracketed span (after link and citation since all start with [)
        if bytes[pos] == b'['
            && let Some((len, content, attributes)) = try_parse_bracketed_span(&text[pos..])
        {
            log::debug!(
                "Matched bracketed span at pos {}: attrs={}",
                pos,
                attributes
            );
            emit_bracketed_span(builder, &content, &attributes, config);
            pos += len;
            continue;
        }

        // Try to parse bare citation (author-in-text)
        if (bytes[pos] == b'@'
            || (bytes[pos] == b'-' && pos + 1 < text.len() && bytes[pos + 1] == b'@'))
            && let Some((len, key, has_suppress)) = try_parse_bare_citation(&text[pos..])
        {
            log::debug!("Matched bare citation at pos {}: key={}", pos, key);
            emit_bare_citation(builder, key, has_suppress);
            pos += len;
            continue;
        }

        // Try to parse emphasis
        if (bytes[pos] == b'*' || bytes[pos] == b'_')
            && let Some((len, inner_text, level, delim_char)) = try_parse_emphasis(&text[pos..])
        {
            log::debug!(
                "Matched emphasis at pos {}: level={}, delim={}",
                pos,
                level,
                delim_char
            );
            emit_emphasis(builder, inner_text, level, delim_char, config);
            pos += len;
            continue;
        }

        // No inline element matched - emit as plain text
        let next_pos = find_next_inline_start(&text[pos..]);
        let text_chunk = if next_pos > 0 {
            &text[pos..pos + next_pos]
        } else {
            &text[pos..]
        };

        if !text_chunk.is_empty() {
            builder.token(SyntaxKind::TEXT.into(), text_chunk);
        }

        if next_pos > 0 {
            pos += next_pos;
        } else {
            break;
        }
    }
}

/// Find the next position where an inline element might start.
/// Returns the number of bytes to skip, or 0 if at end.
fn find_next_inline_start(text: &str) -> usize {
    for (i, ch) in text.char_indices() {
        match ch {
            '\\' | '`' | '*' | '_' | '[' | '!' | '<' | '$' | '^' | '~' | '@' => return i.max(1),
            '{' => {
                // Only stop if this could be a shortcode ({{<)
                if i + 2 < text.len()
                    && text.as_bytes()[i + 1] == b'{'
                    && text.as_bytes()[i + 2] == b'<'
                {
                    return i.max(1);
                }
            }
            '-' => {
                // Check if this might be a suppress-author citation -@
                if i + 1 < text.len() && text.as_bytes()[i + 1] == b'@' {
                    return i.max(1);
                }
            }
            _ => {}
        }
    }
    text.len()
}

/// The InlineParser takes a block-level CST and processes inline elements within text content.
/// It traverses the tree, finds TEXT tokens that need inline parsing, and replaces them
/// with properly parsed inline elements (emphasis, links, math, etc.).
pub struct InlineParser {
    root: SyntaxNode,
    #[allow(dead_code)] // TODO: Will be used for reference link/image resolution
    reference_registry: crate::parser::block_parser::ReferenceRegistry,
    config: Config,
}

impl InlineParser {
    pub fn new(
        root: SyntaxNode,
        config: Config,
        reference_registry: crate::parser::block_parser::ReferenceRegistry,
    ) -> Self {
        Self {
            root,
            reference_registry,
            config,
        }
    }

    /// Parse inline elements within the block-level CST.
    /// Traverses the tree and replaces TEXT tokens with parsed inline elements.
    pub fn parse(self) -> SyntaxNode {
        let green = self.parse_node(&self.root);
        SyntaxNode::new_root(green)
    }

    /// Recursively parse a node, replacing TEXT tokens with inline elements.
    fn parse_node(&self, node: &SyntaxNode) -> GreenNode {
        let mut builder = GreenNodeBuilder::new();
        self.copy_node_to_builder(&mut builder, node);
        builder.finish()
    }

    /// Copy a node and its children to the builder, recursively parsing inline elements.
    fn copy_node_to_builder(&self, builder: &mut GreenNodeBuilder, node: &SyntaxNode) {
        builder.start_node(node.kind().into());

        // For nodes that contain inline content (like paragraphs), collect tokens
        // and parse them together to handle multi-line patterns like display math
        if self.should_use_token_stream(node) {
            let tokens: Vec<SyntaxToken> = node
                .children_with_tokens()
                .filter_map(|child| child.into_token())
                .collect();

            let mut token_stream = TokenStream::new(tokens);
            parse_inline_tokens(
                builder,
                &mut token_stream,
                &self.config,
                Some(&self.reference_registry),
            );
        } else {
            // For other nodes, recursively process children as before
            let mut children = node.children_with_tokens().peekable();
            while let Some(child) = children.next() {
                match child {
                    rowan::NodeOrToken::Node(n) => {
                        self.copy_node_to_builder(builder, &n);
                    }
                    rowan::NodeOrToken::Token(t) => {
                        // Check for hard line breaks: two or more spaces at end of line, or backslash at end of line
                        // Only in non-verbatim contexts
                        if t.kind() == SyntaxKind::TEXT
                            && self.should_parse_inline(&t)
                            && let Some(rowan::NodeOrToken::Token(next)) = children.peek()
                            && next.kind() == SyntaxKind::NEWLINE
                        {
                            let text = t.text();

                            // Check for backslash-newline hard line break (requires escaped_line_breaks extension)
                            if self.config.extensions.escaped_line_breaks && text.ends_with('\\') {
                                // Emit the text before the backslash
                                let text_before = &text[..text.len() - 1];
                                if !text_before.is_empty() {
                                    self.parse_text_with_refs(builder, text_before);
                                }
                                // Emit hard line break - preserve the backslash for losslessness
                                builder.token(SyntaxKind::HardLineBreak.into(), "\\\n");
                                // Skip the NEWLINE token
                                children.next();
                                continue;
                            }

                            // Check for two-or-more-spaces hard line break (always enabled in Pandoc)
                            let trailing_spaces =
                                text.chars().rev().take_while(|&c| c == ' ').count();
                            if trailing_spaces >= 2 {
                                // Emit the text before the trailing spaces
                                let text_before = &text[..text.len() - trailing_spaces];
                                if !text_before.is_empty() {
                                    self.parse_text_with_refs(builder, text_before);
                                }
                                // Emit hard line break - preserve the trailing spaces for losslessness
                                let spaces = " ".repeat(trailing_spaces);
                                builder.token(
                                    SyntaxKind::HardLineBreak.into(),
                                    &format!("{}\n", spaces),
                                );
                                // Skip the NEWLINE token
                                children.next();
                                continue;
                            }
                        }

                        // Normal token processing
                        if self.should_parse_inline(&t) {
                            // Parse inline text, passing registry for reference resolution
                            self.parse_text_with_refs(builder, t.text());
                        } else {
                            builder.token(t.kind().into(), t.text());
                        }
                    }
                }
            }
        }

        builder.finish_node();
    }

    /// Check if a node should use token-stream parsing (for multi-line inline patterns).
    /// Currently, this is enabled for PARAGRAPH nodes that contain potential multi-line
    /// patterns like display math ($$\n...\n$$).
    fn should_use_token_stream(&self, node: &SyntaxNode) -> bool {
        if node.kind() != SyntaxKind::PARAGRAPH {
            return false;
        }

        // Check if paragraph contains potential multi-line display math
        // We look for patterns like: $$\n or \n$$
        // This is a conservative heuristic to avoid affecting single-line paragraphs
        let text = node.to_string();

        // Check for display math delimiters near newlines
        if text.contains("$$\n") || text.contains("\n$$") {
            return true;
        }

        // Check for backslash display math near newlines: \[\n or \n\]
        if text.contains("\\[\n")
            || text.contains("\n\\]")
            || text.contains("\\\\[\n")
            || text.contains("\n\\\\]")
        {
            return true;
        }

        false
    }

    /// Parse inline text with reference link/image resolution support.
    fn parse_text_with_refs(&self, builder: &mut GreenNodeBuilder, text: &str) {
        // Pass the reference registry for reference link/image resolution
        parse_inline_text(builder, text, &self.config, Some(&self.reference_registry));
    }

    /// Check if a token should be parsed for inline elements.
    /// Per spec: "Backslash escapes do not work in verbatim contexts"
    fn should_parse_inline(&self, token: &SyntaxToken) -> bool {
        if token.kind() != SyntaxKind::TEXT {
            return false;
        }

        // Check if we're in a verbatim context (code block, LaTeX environment, HTML block)
        // or line block (where inline parsing is handled differently - preserves line structure)
        if let Some(parent) = token.parent() {
            match parent.kind() {
                SyntaxKind::CodeBlock
                | SyntaxKind::CodeContent
                | SyntaxKind::LatexEnvironment
                | SyntaxKind::LatexEnvBegin
                | SyntaxKind::LatexEnvEnd
                | SyntaxKind::LatexEnvContent
                | SyntaxKind::HtmlBlock
                | SyntaxKind::HtmlBlockTag
                | SyntaxKind::HtmlBlockContent
                | SyntaxKind::LineBlockLine => {
                    return false;
                }
                _ => {}
            }
        }

        true
    }
}

#[cfg(test)]
mod inline_tests {
    use super::*;
    use crate::config::Config;
    use crate::parser::block_parser::BlockParser;

    fn find_nodes_by_kind(node: &SyntaxNode, kind: SyntaxKind) -> Vec<String> {
        let mut results = Vec::new();
        for child in node.descendants() {
            if child.kind() == kind {
                results.push(child.to_string());
            }
        }
        results
    }

    fn parse_inline(input: &str) -> SyntaxNode {
        let config = Config::default();
        let (block_tree, registry) = BlockParser::new(input, &config).parse();
        InlineParser::new(block_tree, config, registry).parse()
    }

    #[test]
    fn test_inline_parser_preserves_text() {
        let input = "This is plain text.";
        let inline_tree = parse_inline(input);

        // Should preserve the text unchanged for now
        let text = inline_tree.to_string();
        assert!(text.contains("This is plain text."));
    }

    #[test]
    fn test_parse_autolink() {
        let input = "Visit <https://example.com> for more.";
        let inline_tree = parse_inline(input);

        let autolinks = find_nodes_by_kind(&inline_tree, SyntaxKind::AutoLink);
        assert_eq!(autolinks.len(), 1);
        assert_eq!(autolinks[0], "<https://example.com>");
    }

    #[test]
    fn test_parse_email_autolink() {
        let input = "Email me at <user@example.com>.";
        let inline_tree = parse_inline(input);

        let autolinks = find_nodes_by_kind(&inline_tree, SyntaxKind::AutoLink);
        assert_eq!(autolinks.len(), 1);
        assert_eq!(autolinks[0], "<user@example.com>");
    }

    #[test]
    fn test_parse_inline_link() {
        let input = "Click [here](https://example.com) to continue.";
        let inline_tree = parse_inline(input);

        let links = find_nodes_by_kind(&inline_tree, SyntaxKind::Link);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0], "[here](https://example.com)");
    }

    #[test]
    fn test_parse_link_with_title() {
        let input = r#"See [this](url "title") link."#;
        let inline_tree = parse_inline(input);

        let links = find_nodes_by_kind(&inline_tree, SyntaxKind::Link);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0], r#"[this](url "title")"#);
    }

    #[test]
    fn test_multiple_links() {
        let input = "Visit [site1](url1) and [site2](url2).";
        let inline_tree = parse_inline(input);

        let links = find_nodes_by_kind(&inline_tree, SyntaxKind::Link);
        assert_eq!(links.len(), 2);
    }

    #[test]
    fn test_parse_inline_image() {
        let input = "Here is an image: ![alt text](image.jpg).";
        let inline_tree = parse_inline(input);

        let images = find_nodes_by_kind(&inline_tree, SyntaxKind::ImageLink);
        assert_eq!(images.len(), 1);
        assert_eq!(images[0], "![alt text](image.jpg)");
    }

    #[test]
    fn test_parse_image_with_title() {
        let input = r#"See ![photo](pic.jpg "My Photo") here."#;
        let inline_tree = parse_inline(input);

        let images = find_nodes_by_kind(&inline_tree, SyntaxKind::ImageLink);
        assert_eq!(images.len(), 1);
        assert_eq!(images[0], r#"![photo](pic.jpg "My Photo")"#);
    }

    #[test]
    fn test_multiple_images() {
        let input = "![img1](a.jpg) and ![img2](b.jpg)";
        let inline_tree = parse_inline(input);

        let images = find_nodes_by_kind(&inline_tree, SyntaxKind::ImageLink);
        assert_eq!(images.len(), 2);
    }

    #[test]
    fn test_link_and_image_together() {
        let input = "A [link](url) and an ![image](pic.jpg).";
        let inline_tree = parse_inline(input);

        let links = find_nodes_by_kind(&inline_tree, SyntaxKind::Link);
        let images = find_nodes_by_kind(&inline_tree, SyntaxKind::ImageLink);
        assert_eq!(links.len(), 1);
        assert_eq!(images.len(), 1);
    }

    #[test]
    fn test_parse_bare_citation() {
        let input = "As @doe99 notes, the result is clear.";
        let inline_tree = parse_inline(input);

        let citations = find_nodes_by_kind(&inline_tree, SyntaxKind::Citation);
        assert_eq!(citations.len(), 1);
        assert_eq!(citations[0], "@doe99");
    }

    #[test]
    fn test_parse_bracketed_citation() {
        let input = "This is a fact [@doe99].";
        let inline_tree = parse_inline(input);

        let citations = find_nodes_by_kind(&inline_tree, SyntaxKind::Citation);
        assert_eq!(citations.len(), 1);
        assert_eq!(citations[0], "[@doe99]");
    }

    #[test]
    fn test_parse_multiple_citations() {
        let input = "Multiple sources [@doe99; @smith2000; @jones2010].";
        let inline_tree = parse_inline(input);

        let citations = find_nodes_by_kind(&inline_tree, SyntaxKind::Citation);
        assert_eq!(citations.len(), 1);
        assert_eq!(citations[0], "[@doe99; @smith2000; @jones2010]");
    }

    #[test]
    fn test_parse_citation_with_locator() {
        let input = "See the discussion [@doe99, pp. 33-35].";
        let inline_tree = parse_inline(input);

        let citations = find_nodes_by_kind(&inline_tree, SyntaxKind::Citation);
        assert_eq!(citations.len(), 1);
        assert_eq!(citations[0], "[@doe99, pp. 33-35]");
    }

    #[test]
    fn test_parse_suppress_author_citation() {
        let input = "Smith says blah [-@smith04].";
        let inline_tree = parse_inline(input);

        let citations = find_nodes_by_kind(&inline_tree, SyntaxKind::Citation);
        assert_eq!(citations.len(), 1);
        assert_eq!(citations[0], "[-@smith04]");
    }

    #[test]
    fn test_parse_bare_suppress_citation() {
        let input = "See -@doe99 for details.";
        let inline_tree = parse_inline(input);

        let citations = find_nodes_by_kind(&inline_tree, SyntaxKind::Citation);
        assert_eq!(citations.len(), 1);
        assert_eq!(citations[0], "-@doe99");
    }

    #[test]
    fn test_citation_not_conflicting_with_email() {
        // Email in autolink should not be parsed as citation
        let input = "Email <user@example.com> for info.";
        let inline_tree = parse_inline(input);

        let autolinks = find_nodes_by_kind(&inline_tree, SyntaxKind::AutoLink);
        let citations = find_nodes_by_kind(&inline_tree, SyntaxKind::Citation);
        assert_eq!(autolinks.len(), 1);
        assert_eq!(citations.len(), 0);
    }

    #[test]
    fn test_parse_native_span_simple() {
        let input = "Text with <span>highlighted</span> content.";
        let inline_tree = parse_inline(input);

        let spans = find_nodes_by_kind(&inline_tree, SyntaxKind::BracketedSpan);
        assert_eq!(spans.len(), 1);
        assert!(spans[0].contains("highlighted"));
    }

    #[test]
    fn test_parse_native_span_with_class() {
        let input = r#"Use <span class="important">this</span> wisely."#;
        let inline_tree = parse_inline(input);

        let spans = find_nodes_by_kind(&inline_tree, SyntaxKind::BracketedSpan);
        assert_eq!(spans.len(), 1);
        assert!(spans[0].contains("this"));
    }

    #[test]
    fn test_parse_native_span_with_markdown() {
        let input = "<span>Contains *emphasis* and `code`</span>.";
        let inline_tree = parse_inline(input);

        let spans = find_nodes_by_kind(&inline_tree, SyntaxKind::BracketedSpan);
        assert_eq!(spans.len(), 1);

        // Should have parsed the emphasis and code inside
        let emphasis = find_nodes_by_kind(&inline_tree, SyntaxKind::Emphasis);
        let code = find_nodes_by_kind(&inline_tree, SyntaxKind::CodeSpan);
        assert_eq!(emphasis.len(), 1);
        assert_eq!(code.len(), 1);
    }

    #[test]
    fn test_native_span_not_confused_with_autolink() {
        let input = "Link <https://example.com> and <span>text</span>.";
        let inline_tree = parse_inline(input);

        let autolinks = find_nodes_by_kind(&inline_tree, SyntaxKind::AutoLink);
        let spans = find_nodes_by_kind(&inline_tree, SyntaxKind::BracketedSpan);
        assert_eq!(autolinks.len(), 1);
        assert_eq!(spans.len(), 1);
    }

    // TokenStream tests
    mod token_stream_tests {
        use super::*;
        use rowan::GreenNodeBuilder;

        fn create_test_tokens(parts: Vec<(&str, SyntaxKind)>) -> Vec<SyntaxToken> {
            let mut builder = GreenNodeBuilder::new();
            builder.start_node(SyntaxKind::DOCUMENT.into());

            for (text, kind) in parts {
                builder.token(kind.into(), text);
            }

            builder.finish_node();
            let green = builder.finish();
            let root = SyntaxNode::new_root(green);

            root.children_with_tokens()
                .filter_map(|child| child.into_token())
                .collect()
        }

        #[test]
        fn test_token_stream_peek() {
            let tokens = create_test_tokens(vec![
                ("hello", SyntaxKind::TEXT),
                ("\n", SyntaxKind::NEWLINE),
                ("world", SyntaxKind::TEXT),
            ]);

            let stream = TokenStream::new(tokens);
            assert_eq!(stream.peek().unwrap().text(), "hello");
            assert_eq!(stream.peek().unwrap().kind(), SyntaxKind::TEXT);
        }

        #[test]
        fn test_token_stream_peek_ahead() {
            let tokens = create_test_tokens(vec![
                ("hello", SyntaxKind::TEXT),
                ("\n", SyntaxKind::NEWLINE),
                ("world", SyntaxKind::TEXT),
            ]);

            let stream = TokenStream::new(tokens);
            assert_eq!(stream.peek_ahead(0).unwrap().text(), "hello");
            assert_eq!(stream.peek_ahead(1).unwrap().text(), "\n");
            assert_eq!(stream.peek_ahead(2).unwrap().text(), "world");
            assert!(stream.peek_ahead(3).is_none());
        }

        #[test]
        fn test_token_stream_advance() {
            let tokens = create_test_tokens(vec![
                ("hello", SyntaxKind::TEXT),
                ("\n", SyntaxKind::NEWLINE),
                ("world", SyntaxKind::TEXT),
            ]);

            let mut stream = TokenStream::new(tokens);

            let token1 = stream.advance().unwrap();
            assert_eq!(token1.text(), "hello");
            assert_eq!(stream.current_offset(), 5);

            let token2 = stream.advance().unwrap();
            assert_eq!(token2.text(), "\n");
            assert_eq!(stream.current_offset(), 6);

            let token3 = stream.advance().unwrap();
            assert_eq!(token3.text(), "world");
            assert_eq!(stream.current_offset(), 11);

            assert!(stream.advance().is_none());
            assert!(stream.is_at_end());
        }

        #[test]
        fn test_token_stream_peek_text_ahead() {
            let tokens = create_test_tokens(vec![
                ("$$", SyntaxKind::TEXT),
                ("\n", SyntaxKind::NEWLINE),
                ("x = y", SyntaxKind::TEXT),
                ("\n", SyntaxKind::NEWLINE),
                ("$$", SyntaxKind::TEXT),
            ]);

            let stream = TokenStream::new(tokens);

            // Peek at first 3 tokens: "$$\nx = y"
            let text = stream.peek_text_ahead(3);
            assert_eq!(text, "$$\nx = y");

            // Peek at all 5 tokens
            let text = stream.peek_text_ahead(5);
            assert_eq!(text, "$$\nx = y\n$$");
        }

        #[test]
        fn test_token_stream_peek_text_while() {
            let tokens = create_test_tokens(vec![
                ("hello", SyntaxKind::TEXT),
                ("\n", SyntaxKind::NEWLINE),
                ("world", SyntaxKind::TEXT),
                ("!", SyntaxKind::TEXT),
            ]);

            let stream = TokenStream::new(tokens);

            // Collect TEXT and NEWLINE tokens (stop at anything else, but we only have TEXT/NEWLINE)
            let text = stream.peek_text_while(|t| {
                t.kind() == SyntaxKind::TEXT || t.kind() == SyntaxKind::NEWLINE
            });
            assert_eq!(text, "hello\nworld!");
        }

        #[test]
        fn test_token_stream_remaining() {
            let tokens = create_test_tokens(vec![
                ("a", SyntaxKind::TEXT),
                ("b", SyntaxKind::TEXT),
                ("c", SyntaxKind::TEXT),
            ]);

            let mut stream = TokenStream::new(tokens);
            assert_eq!(stream.remaining(), 3);

            stream.advance();
            assert_eq!(stream.remaining(), 2);

            stream.advance();
            assert_eq!(stream.remaining(), 1);

            stream.advance();
            assert_eq!(stream.remaining(), 0);
        }

        #[test]
        fn test_token_stream_empty() {
            let stream = TokenStream::new(Vec::new());
            assert!(stream.peek().is_none());
            assert!(stream.is_at_end());
            assert_eq!(stream.remaining(), 0);
            assert_eq!(stream.peek_text_ahead(10), "");
        }
    }
}

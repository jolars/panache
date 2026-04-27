//! Recursive emphasis parsing using Pandoc's algorithm.
//!
//! This module implements emphasis/strong emphasis parsing using a recursive
//! descent approach based on Pandoc's Haskell implementation in
//! `Readers/Markdown.hs:L1662-L1722`.
//!
//! **Key algorithm**: Left-to-right, greedy, first-match wins
//! 1. Parse text left-to-right
//! 2. When we see delimiters, try to parse emphasis (look for matching closer)
//! 3. If successful, emit emphasis node and continue from after closer
//! 4. If failed (no closer found), emit delimiter as literal and continue
//! 5. Nested emphasis is handled naturally by recursive parsing of content
//!
//! **Example**: `*foo **bar* baz**`
//! - See `*`, try to parse EMPH
//! - Parse content: see `**`, try to parse STRONG
//! - STRONG finds closer `**` at end → succeeds, emits STRONG[bar* baz]
//! - Outer `*` can't find closer (all delimiters consumed) → fails, emits `*foo` as literal
//! - Result: `*foo` + STRONG[bar* baz]
//!
//! This matches Pandoc's behavior exactly.

use crate::options::ParserOptions;
use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;

// Import inline element parsers from sibling modules
use super::bookdown::{
    try_parse_bookdown_definition, try_parse_bookdown_reference, try_parse_bookdown_text_reference,
};
use super::bracketed_spans::{emit_bracketed_span, try_parse_bracketed_span};
use super::citations::{
    emit_bare_citation, emit_bracketed_citation, try_parse_bare_citation,
    try_parse_bracketed_citation,
};
use super::code_spans::{emit_code_span, try_parse_code_span};
use super::emoji::{emit_emoji, try_parse_emoji};
use super::escapes::{EscapeType, emit_escape, try_parse_escape};
use super::inline_executable::{emit_inline_executable, try_parse_inline_executable};
use super::inline_footnotes::{
    emit_footnote_reference, emit_inline_footnote, try_parse_footnote_reference,
    try_parse_inline_footnote,
};
use super::latex::{parse_latex_command, try_parse_latex_command};
use super::links::{
    emit_autolink, emit_bare_uri_link, emit_inline_image, emit_inline_link, emit_reference_image,
    emit_reference_link, try_parse_autolink, try_parse_bare_uri, try_parse_inline_image,
    try_parse_inline_link, try_parse_reference_image, try_parse_reference_link,
};
use super::mark::{emit_mark, try_parse_mark};
use super::math::{
    emit_display_math, emit_display_math_environment, emit_double_backslash_display_math,
    emit_double_backslash_inline_math, emit_gfm_inline_math, emit_inline_math,
    emit_single_backslash_display_math, emit_single_backslash_inline_math, try_parse_display_math,
    try_parse_double_backslash_display_math, try_parse_double_backslash_inline_math,
    try_parse_gfm_inline_math, try_parse_inline_math, try_parse_math_environment,
    try_parse_single_backslash_display_math, try_parse_single_backslash_inline_math,
};
use super::native_spans::{emit_native_span, try_parse_native_span};
use super::raw_inline::is_raw_inline;
use super::shortcodes::{emit_shortcode, try_parse_shortcode};
use super::strikeout::{emit_strikeout, try_parse_strikeout};
use super::subscript::{emit_subscript, try_parse_subscript};
use super::superscript::{emit_superscript, try_parse_superscript};

/// Parse inline text using the recursive emphasis algorithm.
///
/// This is the main entry point for parsing inline content with Pandoc-style
/// recursive emphasis handling. It uses a greedy left-to-right, first-match-wins
/// approach that matches Pandoc's behavior exactly.
///
/// **Algorithm**:
/// 1. Parse text left-to-right trying each inline element type in precedence order
/// 2. When we see `*` or `_`, try to parse emphasis recursively
/// 3. Nested emphasis naturally consumes delimiters before outer matches
/// 4. All inline elements (code, links, math, etc.) are parsed on-the-fly
///
/// # Arguments
/// * `text` - The inline text to parse
/// * `config` - Configuration for extensions and formatting
/// * `builder` - The CST builder to emit nodes to
pub fn parse_inline_text_recursive(
    builder: &mut GreenNodeBuilder,
    text: &str,
    config: &ParserOptions,
) {
    log::trace!(
        "Recursive inline parsing: {:?} ({} bytes)",
        &text[..text.len().min(40)],
        text.len()
    );

    parse_inline_range(text, 0, text.len(), config, builder);

    log::trace!("Recursive inline parsing complete");
}

/// Parse inline elements from text content.
/// This is a standalone function used for recursive inline parsing within blocks.
///
/// The `allow_reference_links` parameter is accepted for compatibility but not currently used.
/// Set to `false` in nested contexts (inside link text, image alt, spans) to prevent recursive parsing.
pub fn parse_inline_text(
    builder: &mut GreenNodeBuilder,
    text: &str,
    config: &ParserOptions,
    _allow_reference_links: bool,
) {
    log::trace!(
        "Parsing inline text (recursive): {:?} ({} bytes)",
        &text[..text.len().min(40)],
        text.len()
    );

    // Use recursive parsing with Pandoc's algorithm for emphasis
    parse_inline_text_recursive(builder, text, config);
}

/// Try to parse emphasis starting at the given position.
///
/// This is the entry point for recursive emphasis parsing, equivalent to
/// Pandoc's `enclosure` function.
///
/// Returns Some((bytes_consumed, delim_count)) if emphasis was successfully parsed,
/// or None if the delimiter should be treated as literal text.
/// When returning None, the delim_count tells the caller how many delimiter
/// characters to skip (to avoid re-parsing parts of a failed delimiter run).
///
/// # Arguments
/// * `text` - The full text being parsed
/// * `pos` - Current position in text (where the delimiter starts)
/// * `end` - End boundary (don't search for closers beyond this)
/// * `config` - Configuration
/// * `builder` - CST builder
///
/// **Algorithm**:
/// 1. Count opening delimiters
/// 2. Check if followed by whitespace (if so, return None)
/// 3. Dispatch to parse_one/two/three based on count
/// 4. Those functions parse content and look for matching closer (within bounds)
/// 5. If closer found, emit node and return bytes consumed
/// 6. If not found, return None with delimiter count (caller skips entire run)
pub fn try_parse_emphasis(
    text: &str,
    pos: usize,
    end: usize,
    config: &ParserOptions,
    builder: &mut GreenNodeBuilder,
) -> Option<(usize, usize)> {
    let bytes = text.as_bytes();

    if pos >= bytes.len() {
        return None;
    }

    let delim_char = bytes[pos] as char;
    if delim_char != '*' && delim_char != '_' {
        return None;
    }

    // Count consecutive delimiters
    let mut count = 0;
    while pos + count < bytes.len() && bytes[pos + count] == bytes[pos] {
        count += 1;
    }

    let after_pos = pos + count;

    log::trace!(
        "try_parse_emphasis: '{}' x {} at pos {}",
        delim_char,
        count,
        pos
    );

    // Check if followed by whitespace (Pandoc rule: treat as literal)
    if after_pos < text.len()
        && let Some(next_char) = text[after_pos..].chars().next()
        && next_char.is_whitespace()
    {
        log::trace!("Delimiter followed by whitespace, treating as literal");
        return None;
    }

    // For underscores: check intraword_underscores extension (Pandoc lines 1668-1672)
    // Can't open if preceded by alphanumeric (prevents foo_bar from parsing)
    if delim_char == '_'
        && pos > 0
        && let Some(prev_char) = text[..pos].chars().last()
        && prev_char.is_alphanumeric()
    {
        log::trace!("Underscore preceded by alphanumeric, can't open (intraword)");
        return None;
    }

    // Dispatch based on delimiter count
    let result = match count {
        1 => try_parse_one(text, pos, delim_char, end, config, builder),
        2 => try_parse_two(text, pos, delim_char, end, config, builder),
        3 => try_parse_three(text, pos, delim_char, end, config, builder),
        _ => {
            // 4+ delimiters: treat as literal (Pandoc behavior)
            log::trace!("{} delimiters (4+), treating as literal", count);
            None
        }
    };

    // If parsing succeeded, return (bytes_consumed, delim_count)
    // If failed, return None but the caller will know to skip `count` delimiters
    result.map(|consumed| (consumed, count))
}

/// Try to parse emphasis in a nested context (bypassing opener validity checks).
///
/// This mirrors Pandoc's behavior where `one` can call `two c mempty` directly,
/// bypassing the `enclosure` opener validity checks. This is needed because
/// patterns like `***foo **bar** baz***` require `**` followed by space to be
/// parsed as a nested strong opener.
///
/// Returns Some((bytes_consumed, delim_count)) if successful, None otherwise.
fn try_parse_emphasis_nested(
    text: &str,
    pos: usize,
    end: usize,
    config: &ParserOptions,
    builder: &mut GreenNodeBuilder,
) -> Option<(usize, usize)> {
    let bytes = text.as_bytes();

    if pos >= bytes.len() {
        return None;
    }

    let delim_char = bytes[pos] as char;
    if delim_char != '*' && delim_char != '_' {
        return None;
    }

    // Count consecutive delimiters
    let mut count = 0;
    while pos + count < bytes.len() && bytes[pos + count] == bytes[pos] {
        count += 1;
    }

    log::trace!(
        "try_parse_emphasis_nested: '{}' x {} at pos {}",
        delim_char,
        count,
        pos
    );

    // For underscores: still check intraword_underscores (prevents foo_bar parsing)
    // This check applies even in nested contexts
    if delim_char == '_'
        && pos > 0
        && let Some(prev_char) = text[..pos].chars().last()
        && prev_char.is_alphanumeric()
    {
        log::trace!("Underscore preceded by alphanumeric, can't open (intraword)");
        return None;
    }

    // NOTE: We intentionally skip the "delimiter followed by whitespace" check here.
    // In nested contexts (inside `one` calling `two`), Pandoc allows openers
    // followed by whitespace because the opener has already been matched.

    // Dispatch based on delimiter count
    let result = match count {
        1 => try_parse_one(text, pos, delim_char, end, config, builder),
        2 => try_parse_two(text, pos, delim_char, end, config, builder),
        3 => try_parse_three(text, pos, delim_char, end, config, builder),
        _ => {
            // 4+ delimiters: treat as literal (Pandoc behavior)
            log::trace!("{} delimiters (4+), treating as literal", count);
            None
        }
    };

    result.map(|consumed| (consumed, count))
}

/// Try to parse emphasis with *** opening delimiter.
///
/// Tries to match closers in order: *** → ** → *
/// Returns Some(bytes_consumed) if successful, None otherwise.
fn try_parse_three(
    text: &str,
    pos: usize,
    delim_char: char,
    end: usize,
    config: &ParserOptions,
    builder: &mut GreenNodeBuilder,
) -> Option<usize> {
    let content_start = pos + 3;
    let one = delim_char.to_string();
    let two = one.repeat(2);

    log::trace!("try_parse_three: '{}' x 3 at pos {}", delim_char, pos);

    // Pandoc algorithm (line 1695): Parse content UNTIL we see a VALID ender
    // We loop through potential enders, checking if each is valid.
    // Invalid enders (like `**` preceded by whitespace) are skipped.
    let mut search_pos = content_start;

    loop {
        // Find next potential ender
        let closer_start = match find_first_potential_ender(text, search_pos, delim_char, end) {
            Some(p) => p,
            None => {
                log::trace!("No potential ender found for ***");
                return None;
            }
        };

        log::trace!("Potential ender at pos {}", closer_start);

        // Count how many delimiters we have at closer_start
        let bytes = text.as_bytes();
        let mut closer_count = 0;
        let mut check_pos = closer_start;
        while check_pos < bytes.len() && bytes[check_pos] == delim_char as u8 {
            closer_count += 1;
            check_pos += 1;
        }

        log::trace!(
            "Found {} x {} at pos {}",
            delim_char,
            closer_count,
            closer_start
        );

        // Try to match closers in order: ***, **, * (Pandoc lines 1696-1698)

        // Try *** (line 1696)
        if closer_count >= 3 && is_valid_ender(text, closer_start, delim_char, 3) {
            log::trace!("Matched *** closer, emitting Strong[Emph[content]]");

            builder.start_node(SyntaxKind::STRONG.into());
            builder.token(SyntaxKind::STRONG_MARKER.into(), &two);

            builder.start_node(SyntaxKind::EMPHASIS.into());
            builder.token(SyntaxKind::EMPHASIS_MARKER.into(), &one);
            parse_inline_range_nested(text, content_start, closer_start, config, builder);
            builder.token(SyntaxKind::EMPHASIS_MARKER.into(), &one);
            builder.finish_node(); // EMPHASIS

            builder.token(SyntaxKind::STRONG_MARKER.into(), &two);
            builder.finish_node(); // STRONG

            return Some(closer_start + 3 - pos);
        }

        // Try ** (line 1697)
        if closer_count >= 2 && is_valid_ender(text, closer_start, delim_char, 2) {
            log::trace!("Matched ** closer, wrapping as Strong and continuing with one");

            let continue_pos = closer_start + 2;

            if let Some(final_closer_pos) =
                parse_until_closer_with_nested_two(text, continue_pos, delim_char, 1, end, config)
            {
                log::trace!(
                    "Found * closer at pos {}, emitting Emph[Strong[...], ...]",
                    final_closer_pos
                );

                builder.start_node(SyntaxKind::EMPHASIS.into());
                builder.token(SyntaxKind::EMPHASIS_MARKER.into(), &one);

                builder.start_node(SyntaxKind::STRONG.into());
                builder.token(SyntaxKind::STRONG_MARKER.into(), &two);
                parse_inline_range_nested(text, content_start, closer_start, config, builder);
                builder.token(SyntaxKind::STRONG_MARKER.into(), &two);
                builder.finish_node(); // STRONG

                // Parse additional content between ** and * (up to but not including the closer)
                parse_inline_range_nested(text, continue_pos, final_closer_pos, config, builder);

                builder.token(SyntaxKind::EMPHASIS_MARKER.into(), &one);
                builder.finish_node(); // EMPHASIS

                return Some(final_closer_pos + 1 - pos);
            }

            // Fallback: emit * + STRONG
            log::trace!("No * closer found after **, emitting * + STRONG");
            builder.token(SyntaxKind::TEXT.into(), &one);

            builder.start_node(SyntaxKind::STRONG.into());
            builder.token(SyntaxKind::STRONG_MARKER.into(), &two);
            parse_inline_range_nested(text, content_start, closer_start, config, builder);
            builder.token(SyntaxKind::STRONG_MARKER.into(), &two);
            builder.finish_node(); // STRONG

            return Some(closer_start + 2 - pos);
        }

        // Try * (line 1698)
        if closer_count >= 1 && is_valid_ender(text, closer_start, delim_char, 1) {
            log::trace!("Matched * closer, wrapping as Emph and continuing with two");

            let continue_pos = closer_start + 1;

            if let Some(final_closer_pos) =
                parse_until_closer_with_nested_one(text, continue_pos, delim_char, 2, end, config)
            {
                log::trace!(
                    "Found ** closer at pos {}, emitting Strong[Emph[...], ...]",
                    final_closer_pos
                );

                builder.start_node(SyntaxKind::STRONG.into());
                builder.token(SyntaxKind::STRONG_MARKER.into(), &two);

                builder.start_node(SyntaxKind::EMPHASIS.into());
                builder.token(SyntaxKind::EMPHASIS_MARKER.into(), &one);
                parse_inline_range_nested(text, content_start, closer_start, config, builder);
                builder.token(SyntaxKind::EMPHASIS_MARKER.into(), &one);
                builder.finish_node(); // EMPHASIS

                parse_inline_range_nested(text, continue_pos, final_closer_pos, config, builder);

                builder.token(SyntaxKind::STRONG_MARKER.into(), &two);
                builder.finish_node(); // STRONG

                return Some(final_closer_pos + 2 - pos);
            }

            // Fallback: emit ** + EMPH
            log::trace!("No ** closer found after *, emitting ** + EMPH");
            builder.token(SyntaxKind::TEXT.into(), &two);

            builder.start_node(SyntaxKind::EMPHASIS.into());
            builder.token(SyntaxKind::EMPHASIS_MARKER.into(), &one);
            parse_inline_range_nested(text, content_start, closer_start, config, builder);
            builder.token(SyntaxKind::EMPHASIS_MARKER.into(), &one);
            builder.finish_node(); // EMPHASIS

            return Some(closer_start + 1 - pos);
        }

        // No valid ender at this position - continue searching after this delimiter run
        log::trace!(
            "No valid ender at pos {}, continuing search from {}",
            closer_start,
            closer_start + closer_count
        );
        search_pos = closer_start + closer_count;
    }
}

/// Find the first potential emphasis ender (delimiter character) starting from `start`.
/// This implements Pandoc's `many (notFollowedBy (ender c 1) >> inline)` -
/// we parse inline content until we hit a delimiter that could be an ender.
fn find_first_potential_ender(
    text: &str,
    start: usize,
    delim_char: char,
    end: usize,
) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut pos = start;

    while pos < end.min(text.len()) {
        // Check if we found the delimiter character
        if bytes[pos] == delim_char as u8 {
            // Check if it's escaped
            let is_escaped = {
                let mut backslash_count = 0;
                let mut check_pos = pos;
                while check_pos > 0 && bytes[check_pos - 1] == b'\\' {
                    backslash_count += 1;
                    check_pos -= 1;
                }
                backslash_count % 2 == 1
            };

            if !is_escaped {
                // Found a potential ender
                return Some(pos);
            }
        }

        pos += 1;
    }

    None
}

/// Check if a delimiter at the given position is a valid ender.
/// This implements Pandoc's `ender c n` function.
fn is_valid_ender(text: &str, pos: usize, delim_char: char, delim_count: usize) -> bool {
    let bytes = text.as_bytes();

    // Check we have exactly delim_count delimiters (not more, not less)
    if pos + delim_count > text.len() {
        return false;
    }

    for i in 0..delim_count {
        if bytes[pos + i] != delim_char as u8 {
            return false;
        }
    }

    // Check no delimiter immediately before
    if pos > 0 && bytes[pos - 1] == delim_char as u8 {
        return false;
    }

    // Check no delimiter immediately after
    let after_pos = pos + delim_count;
    if after_pos < bytes.len() && bytes[after_pos] == delim_char as u8 {
        return false;
    }

    // For underscores, check right-flanking (not preceded by whitespace)
    // Pandoc's `ender` for asterisks has NO right-flanking requirement
    if delim_char == '_' {
        if pos > 0
            && let Some(prev_char) = text[..pos].chars().last()
            && prev_char.is_whitespace()
        {
            return false;
        }

        // Check not followed by alphanumeric (right-flanking rule for underscores)
        if after_pos < text.len()
            && let Some(next_char) = text[after_pos..].chars().next()
            && next_char.is_alphanumeric()
        {
            return false;
        }
    }

    true
}

/// Try to parse emphasis with ** opening delimiter.
///
/// Tries to match ** closer only. No fallback.
/// Returns Some(bytes_consumed) if successful, None otherwise.
fn try_parse_two(
    text: &str,
    pos: usize,
    delim_char: char,
    end: usize,
    config: &ParserOptions,
    builder: &mut GreenNodeBuilder,
) -> Option<usize> {
    let content_start = pos + 2;

    log::trace!("try_parse_two: '{}' x 2 at pos {}", delim_char, pos);

    // Try to find ** closer, checking for nested * emphasis along the way
    if let Some(closer_pos) =
        parse_until_closer_with_nested_one(text, content_start, delim_char, 2, end, config)
    {
        log::trace!("Found ** closer at pos {}", closer_pos);

        // Emit STRONG(content)
        builder.start_node(SyntaxKind::STRONG.into());
        builder.token(SyntaxKind::STRONG_MARKER.into(), &text[pos..pos + 2]);
        parse_inline_range_nested(text, content_start, closer_pos, config, builder);
        builder.token(
            SyntaxKind::STRONG_MARKER.into(),
            &text[closer_pos..closer_pos + 2],
        );
        builder.finish_node(); // STRONG

        return Some(closer_pos + 2 - pos);
    }

    // No closer found
    log::trace!("No closer found for **");
    None
}

/// Try to parse emphasis with * opening delimiter.
///
/// Tries to match * closer.
/// Returns Some(bytes_consumed) if successful, None otherwise.
///
/// **Pandoc algorithm**: While parsing content, if we encounter **,
/// try to parse it as `two` (strong) recursively. If `two` succeeds,
/// it consumes the ** delimiters, potentially preventing us from finding
/// a closer for the outer *. This creates priority where ** can "steal"
/// matches from *.
fn try_parse_one(
    text: &str,
    pos: usize,
    delim_char: char,
    end: usize,
    config: &ParserOptions,
    builder: &mut GreenNodeBuilder,
) -> Option<usize> {
    let content_start = pos + 1;

    log::trace!("try_parse_one: '{}' x 1 at pos {}", delim_char, pos);

    // Try to find * closer using Pandoc's algorithm with nested two attempts
    if let Some(closer_pos) =
        parse_until_closer_with_nested_two(text, content_start, delim_char, 1, end, config)
    {
        log::trace!("Found * closer at pos {}", closer_pos);

        // Emit EMPH(content)
        builder.start_node(SyntaxKind::EMPHASIS.into());
        builder.token(SyntaxKind::EMPHASIS_MARKER.into(), &text[pos..pos + 1]);
        parse_inline_range_nested(text, content_start, closer_pos, config, builder);
        builder.token(
            SyntaxKind::EMPHASIS_MARKER.into(),
            &text[closer_pos..closer_pos + 1],
        );
        builder.finish_node(); // EMPHASIS

        return Some(closer_pos + 1 - pos);
    }

    // No closer found
    log::trace!("No closer found for *");
    None
}

/// Parse inline content and look for a matching closer, with nested two attempts.
///
/// This implements Pandoc's algorithm from Markdown.hs lines 1712-1717:
/// When parsing `*...*`, if we encounter `**` (and it's not followed by
/// another `*` that would close the outer emphasis), try to parse it as
/// `two c mempty` (strong). If `two` succeeds, those `**` delimiters are
/// consumed, and we continue searching for the `*` closer.
///
/// This creates a priority system where `**` can "steal" matches from `*`.
///
/// Example: `*foo **bar* baz**`
/// - When parsing the outer `*...*`, we encounter `**` at position 5
/// - We try `two` which succeeds with `**bar* baz**`
/// - Now there's no `*` closer for the outer `*`, so it fails
/// - Result: literal `*foo ` + STRONG("bar* baz")
///
/// # Arguments
/// * `end` - Don't search beyond this position (respects nesting boundaries)
fn parse_until_closer_with_nested_two(
    text: &str,
    start: usize,
    delim_char: char,
    delim_count: usize,
    end: usize,
    config: &ParserOptions,
) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut pos = start;

    while pos < end.min(text.len()) {
        if bytes[pos] == b'`'
            && let Some(m) = try_parse_inline_executable(
                &text[pos..],
                config.extensions.rmarkdown_inline_code,
                config.extensions.quarto_inline_code,
            )
        {
            log::trace!(
                "Skipping inline executable span of {} bytes at pos {}",
                m.total_len,
                pos
            );
            pos += m.total_len;
            continue;
        }

        // Skip over code spans - their content is protected from delimiter matching
        if bytes[pos] == b'`'
            && let Some((len, _, _, _)) = try_parse_code_span(&text[pos..])
        {
            log::trace!("Skipping code span of {} bytes at pos {}", len, pos);
            pos += len;
            continue;
        }

        // Skip over inline math - their content is protected from delimiter matching
        if bytes[pos] == b'$'
            && let Some((len, _)) = try_parse_inline_math(&text[pos..])
        {
            log::trace!("Skipping inline math of {} bytes at pos {}", len, pos);
            pos += len;
            continue;
        }

        // Skip over links - their content is protected from delimiter matching
        if bytes[pos] == b'['
            && let Some((len, _, _, _)) = try_parse_inline_link(&text[pos..])
        {
            log::trace!("Skipping inline link of {} bytes at pos {}", len, pos);
            pos += len;
            continue;
        }

        // Pandoc algorithm: If we're looking for a single delimiter (*) and
        // encounter a double delimiter (**), try to parse it as `two` (strong).
        // This happens BEFORE checking if pos is a closer for our current emphasis.
        if delim_count == 1
            && pos + 2 <= text.len()
            && bytes[pos] == delim_char as u8
            && bytes[pos + 1] == delim_char as u8
        {
            // First check if the first delimiter is escaped
            let first_is_escaped = {
                let mut backslash_count = 0;
                let mut check_pos = pos;
                while check_pos > 0 && bytes[check_pos - 1] == b'\\' {
                    backslash_count += 1;
                    check_pos -= 1;
                }
                backslash_count % 2 == 1
            };

            if first_is_escaped {
                // First * is escaped, skip it and continue
                // The second * might be a closer or start of emphasis
                log::trace!(
                    "First * at pos {} is escaped, skipping to check second *",
                    pos
                );
                pos = advance_char_boundary(text, pos, end);
                continue;
            }

            // Check that there's NOT a third delimiter (which would make this
            // part of a longer run that we shouldn't treat as `two`)
            let no_third_delim = pos + 2 >= bytes.len() || bytes[pos + 2] != delim_char as u8;

            if no_third_delim {
                log::trace!(
                    "try_parse_one: found ** at pos {}, attempting nested two",
                    pos
                );

                // Try to parse as `two` (strong emphasis)
                // We create a temporary builder to test if `two` succeeds
                let mut temp_builder = GreenNodeBuilder::new();
                if let Some(two_consumed) =
                    try_parse_two(text, pos, delim_char, end, config, &mut temp_builder)
                {
                    // `two` succeeded! Those ** delimiters are consumed.
                    // We skip past the `two` and continue searching for our `*` closer.
                    log::trace!(
                        "Nested two succeeded, consumed {} bytes, continuing search",
                        two_consumed
                    );
                    pos += two_consumed;
                    continue;
                }
                // `two` failed - this means the entire `one` parse should fail!
                // In Pandoc, the `try (string [c,c] >> notFollowedBy (ender c 1) >> two c mempty)`
                // alternative fails, and the first alternative `notFollowedBy (ender c 1) >> inline`
                // also fails because we ARE followed by an ender (the first * of **).
                // So the entire content parsing fails, and `one` returns failure.
                log::trace!("Nested two failed at pos {}, entire one() should fail", pos);
                return None;
            }
        }

        // Check if we have a potential closer here
        if pos + delim_count <= text.len() {
            let mut matches = true;
            for i in 0..delim_count {
                if bytes[pos + i] != delim_char as u8 {
                    matches = false;
                    break;
                }
            }

            if matches {
                // IMPORTANT: Check that there are EXACTLY delim_count delimiters,
                // not more. E.g., when looking for `*`, we shouldn't match
                // `*` that's part of a longer run.

                // Check: not escaped (preceded by odd number of backslashes)
                let is_escaped = {
                    let mut backslash_count = 0;
                    let mut check_pos = pos;
                    while check_pos > 0 && bytes[check_pos - 1] == b'\\' {
                        backslash_count += 1;
                        check_pos -= 1;
                    }
                    backslash_count % 2 == 1 // Odd number = escaped
                };

                // Allow matching at the start OR end of a delimiter run.
                // This lets `**` close at the end of `***` (after a nested `*` closes),
                // while still avoiding matches in the middle of longer runs.
                let at_run_start = pos == 0 || bytes[pos - 1] != delim_char as u8;
                let after_pos = pos + delim_count;
                let at_run_end = after_pos >= bytes.len() || bytes[after_pos] != delim_char as u8;

                if (at_run_start || at_run_end) && !is_escaped {
                    // Found a potential closer!
                    // For underscores, check right-flanking: closer must be preceded by non-whitespace
                    // For asterisks, Pandoc doesn't require right-flanking (see ender function in Markdown.hs)
                    if delim_char == '_'
                        && pos > start
                        && let Some(prev_char) = text[..pos].chars().last()
                        && prev_char.is_whitespace()
                    {
                        log::trace!(
                            "Underscore closer preceded by whitespace at pos {}, not right-flanking",
                            pos
                        );
                        // Not a valid closer, continue searching
                        pos = advance_char_boundary(text, pos, end);
                        continue;
                    }

                    log::trace!(
                        "Found exact {} x {} closer at pos {}",
                        delim_char,
                        delim_count,
                        pos
                    );
                    return Some(pos);
                }
            }
        }

        // Not a closer, move to next UTF-8 boundary.
        pos = advance_char_boundary(text, pos, end);
    }

    None
}

/// Parse inline content and look for a matching closer, with nested one attempts.
///
/// This implements the symmetric case to `parse_until_closer_with_nested_two`:
/// When parsing `**...**`, if we encounter `*` (and it's not followed by
/// another `*` that would be part of our `**` closer), try to parse it as
/// `one c mempty` (emphasis). If `one` succeeds, those `*` delimiters are
/// consumed, and we continue searching for the `**` closer.
///
/// This ensures nested emphasis closes before the outer strong emphasis.
///
/// Example: `**bold with *italic***`
/// - When parsing the outer `**...**, we scan for `**` closer
/// - At position 12, we encounter a single `*` (start of `*italic`)
/// - We try `one` which succeeds with `*italic*` (consuming the first `*` from `***`)
/// - We continue scanning and find `**` at position 20 (the remaining `**` from `***`)
/// - Result: STRONG["bold with " EMPHASIS["italic"]]
///
/// # Arguments
/// * `end` - Don't search beyond this position (respects nesting boundaries)
fn parse_until_closer_with_nested_one(
    text: &str,
    start: usize,
    delim_char: char,
    delim_count: usize,
    end: usize,
    config: &ParserOptions,
) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut pos = start;

    while pos < end.min(text.len()) {
        if bytes[pos] == b'`'
            && let Some(m) = try_parse_inline_executable(
                &text[pos..],
                config.extensions.rmarkdown_inline_code,
                config.extensions.quarto_inline_code,
            )
        {
            log::trace!(
                "Skipping inline executable span of {} bytes at pos {}",
                m.total_len,
                pos
            );
            pos += m.total_len;
            continue;
        }

        // Skip over code spans - their content is protected from delimiter matching
        if bytes[pos] == b'`'
            && let Some((len, _, _, _)) = try_parse_code_span(&text[pos..])
        {
            log::trace!("Skipping code span of {} bytes at pos {}", len, pos);
            pos += len;
            continue;
        }

        // Skip over inline math - their content is protected from delimiter matching
        if bytes[pos] == b'$'
            && let Some((len, _)) = try_parse_inline_math(&text[pos..])
        {
            log::trace!("Skipping inline math of {} bytes at pos {}", len, pos);
            pos += len;
            continue;
        }

        // Skip over links - their content is protected from delimiter matching
        if bytes[pos] == b'['
            && let Some((len, _, _, _)) = try_parse_inline_link(&text[pos..])
        {
            log::trace!("Skipping inline link of {} bytes at pos {}", len, pos);
            pos += len;
            continue;
        }

        // Pandoc algorithm: If we're looking for a double delimiter (**) and
        // encounter a single delimiter (*), check if it's a valid emphasis opener.
        // If it is, try to parse it as `one` (emphasis). If `one` succeeds, skip
        // over it. If `one` fails, the outer `two` also fails (delimiter poisoning).
        // If the `*` is NOT a valid opener (e.g., followed by whitespace or escaped),
        // skip it and continue looking for the `**` closer.
        if delim_count == 2 && pos < text.len() && bytes[pos] == delim_char as u8 {
            // Check that there's NOT a second delimiter immediately after
            // (which would make this part of our `**` closer or another `**` opener)
            let no_second_delim = pos + 1 >= bytes.len() || bytes[pos + 1] != delim_char as u8;

            if no_second_delim {
                // Check if this * is escaped (preceded by odd number of backslashes)
                let is_escaped = {
                    let mut backslash_count = 0;
                    let mut check_pos = pos;
                    while check_pos > 0 && bytes[check_pos - 1] == b'\\' {
                        backslash_count += 1;
                        check_pos -= 1;
                    }
                    backslash_count % 2 == 1
                };

                if is_escaped {
                    // Escaped delimiter - just literal text, skip it
                    log::trace!("* at pos {} is escaped, skipping", pos);
                    pos = advance_char_boundary(text, pos, end);
                    continue;
                }

                // Check if this * is a valid emphasis opener (Pandoc's enclosure rule).
                // A delimiter followed by whitespace is NOT an opener - it's literal text.
                let after_delim = pos + 1;
                let followed_by_whitespace = after_delim < text.len()
                    && text[after_delim..]
                        .chars()
                        .next()
                        .is_some_and(|c| c.is_whitespace());

                if followed_by_whitespace {
                    // Not a valid opener - just literal text, skip it
                    log::trace!(
                        "* at pos {} followed by whitespace, not an opener, skipping",
                        pos
                    );
                    pos = advance_char_boundary(text, pos, end);
                    continue;
                }

                log::trace!(
                    "try_parse_two: found * at pos {}, attempting nested one",
                    pos
                );

                // Try to parse as `one` (emphasis)
                // We create a temporary builder to test if `one` succeeds
                let mut temp_builder = GreenNodeBuilder::new();
                if let Some(one_consumed) =
                    try_parse_one(text, pos, delim_char, end, config, &mut temp_builder)
                {
                    // `one` succeeded! Those * delimiters are consumed.
                    // We skip past the `one` and continue searching for our `**` closer.
                    log::trace!(
                        "Nested one succeeded, consumed {} bytes, continuing search",
                        one_consumed
                    );
                    pos += one_consumed;
                    continue;
                }

                // `one` failed to find a closer. According to Pandoc's algorithm,
                // this means the outer `two` should also fail. An unmatched inner
                // delimiter "poisons" the outer emphasis.
                // Example: `**foo *bar**` - the `*` can't find a closer, so the
                // outer `**` should fail and the whole thing becomes literal.
                log::trace!(
                    "Nested one failed at pos {}, poisoning outer two (no closer found)",
                    pos
                );
                return None;
            }
        }

        // Check if we have a potential closer here
        if pos + delim_count <= text.len() {
            let mut matches = true;
            for i in 0..delim_count {
                if bytes[pos + i] != delim_char as u8 {
                    matches = false;
                    break;
                }
            }

            if matches {
                // Check: not escaped (preceded by odd number of backslashes)
                let is_escaped = {
                    let mut backslash_count = 0;
                    let mut check_pos = pos;
                    while check_pos > 0 && bytes[check_pos - 1] == b'\\' {
                        backslash_count += 1;
                        check_pos -= 1;
                    }
                    backslash_count % 2 == 1 // Odd number = escaped
                };

                // Allow matching at the start OR end of a delimiter run.
                // This lets `**` close at the end of `***` (after a nested `*` closes),
                // while still avoiding matches in the middle of longer runs.
                let at_run_start = pos == 0 || bytes[pos - 1] != delim_char as u8;
                let after_pos = pos + delim_count;
                let at_run_end = after_pos >= bytes.len() || bytes[after_pos] != delim_char as u8;

                if (at_run_start || at_run_end) && !is_escaped {
                    // Found a potential closer!
                    // For underscores, check right-flanking: closer must be preceded by non-whitespace
                    // For asterisks, Pandoc doesn't require right-flanking (see ender function in Markdown.hs)
                    if delim_char == '_'
                        && pos > start
                        && let Some(prev_char) = text[..pos].chars().last()
                        && prev_char.is_whitespace()
                    {
                        log::trace!(
                            "Underscore closer preceded by whitespace at pos {}, not right-flanking",
                            pos
                        );
                        // Not a valid closer, continue searching
                        pos = advance_char_boundary(text, pos, end);
                        continue;
                    }

                    log::trace!(
                        "Found exact {} x {} closer at pos {}",
                        delim_char,
                        delim_count,
                        pos
                    );
                    return Some(pos);
                }
            }
        }

        // Not a closer, move to next UTF-8 boundary.
        pos = advance_char_boundary(text, pos, end);
    }

    None
}

///
/// This is the recursive inline parser that handles all inline elements:
/// - Text
/// - Escapes (highest priority)
/// - Code spans
/// - Math (inline and display)
/// - Emphasis/strong (via try_parse_emphasis)
/// - Other inline elements
///
/// **Important**: This is where the greedy left-to-right parsing happens.
/// When we see `**`, we try to parse it as STRONG. If it succeeds, those
/// delimiters are consumed and won't be available for outer emphasis.
///
/// # Arguments
/// * `nested_emphasis` - If true, bypass opener validity checks for emphasis.
///   Set to true when called from within emphasis parsing (e.g., from try_parse_one/two/three).
fn parse_inline_range(
    text: &str,
    start: usize,
    end: usize,
    config: &ParserOptions,
    builder: &mut GreenNodeBuilder,
) {
    parse_inline_range_impl(text, start, end, config, builder, false)
}

/// Same as `parse_inline_range` but bypasses opener validity checks for emphasis.
/// Used within emphasis parsing contexts (e.g., from try_parse_one/two/three).
fn parse_inline_range_nested(
    text: &str,
    start: usize,
    end: usize,
    config: &ParserOptions,
    builder: &mut GreenNodeBuilder,
) {
    parse_inline_range_impl(text, start, end, config, builder, true)
}

fn is_emoji_boundary(text: &str, pos: usize) -> bool {
    if pos > 0 {
        let prev = text.as_bytes()[pos - 1] as char;
        if prev.is_ascii_alphanumeric() || prev == '_' {
            return false;
        }
    }
    true
}

#[inline]
fn advance_char_boundary(text: &str, pos: usize, end: usize) -> usize {
    if pos >= end || pos >= text.len() {
        return pos;
    }
    let ch_len = text[pos..]
        .chars()
        .next()
        .map_or(1, std::primitive::char::len_utf8);
    (pos + ch_len).min(end)
}

fn parse_inline_range_impl(
    text: &str,
    start: usize,
    end: usize,
    config: &ParserOptions,
    builder: &mut GreenNodeBuilder,
    nested_emphasis: bool,
) {
    log::trace!(
        "parse_inline_range: start={}, end={}, text={:?}",
        start,
        end,
        &text[start..end]
    );
    let mut pos = start;
    let mut text_start = start;

    while pos < end {
        let byte = text.as_bytes()[pos];

        // Backslash math (highest priority if enabled)
        if byte == b'\\' {
            // Try double backslash display math first: \\[...\\]
            if config.extensions.tex_math_double_backslash {
                if let Some((len, content)) = try_parse_double_backslash_display_math(&text[pos..])
                {
                    if pos > text_start {
                        builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
                    }
                    log::trace!("Matched double backslash display math at pos {}", pos);
                    emit_double_backslash_display_math(builder, content);
                    pos += len;
                    text_start = pos;
                    continue;
                }

                // Try double backslash inline math: \\(...\\)
                if let Some((len, content)) = try_parse_double_backslash_inline_math(&text[pos..]) {
                    if pos > text_start {
                        builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
                    }
                    log::trace!("Matched double backslash inline math at pos {}", pos);
                    emit_double_backslash_inline_math(builder, content);
                    pos += len;
                    text_start = pos;
                    continue;
                }
            }

            // Try single backslash display math: \[...\]
            if config.extensions.tex_math_single_backslash {
                if let Some((len, content)) = try_parse_single_backslash_display_math(&text[pos..])
                {
                    if pos > text_start {
                        builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
                    }
                    log::trace!("Matched single backslash display math at pos {}", pos);
                    emit_single_backslash_display_math(builder, content);
                    pos += len;
                    text_start = pos;
                    continue;
                }

                // Try single backslash inline math: \(...\)
                if let Some((len, content)) = try_parse_single_backslash_inline_math(&text[pos..]) {
                    if pos > text_start {
                        builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
                    }
                    log::trace!("Matched single backslash inline math at pos {}", pos);
                    emit_single_backslash_inline_math(builder, content);
                    pos += len;
                    text_start = pos;
                    continue;
                }
            }

            // Try math environments \begin{equation}...\end{equation}
            if config.extensions.raw_tex
                && let Some((len, begin_marker, content, end_marker)) =
                    try_parse_math_environment(&text[pos..])
            {
                if pos > text_start {
                    builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
                }
                log::trace!("Matched math environment at pos {}", pos);
                emit_display_math_environment(builder, begin_marker, content, end_marker);
                pos += len;
                text_start = pos;
                continue;
            }

            // Try bookdown reference: \@ref(label)
            if config.extensions.bookdown_references
                && let Some((len, label)) = try_parse_bookdown_reference(&text[pos..])
            {
                if pos > text_start {
                    builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
                }
                log::trace!("Matched bookdown reference at pos {}: {}", pos, label);
                super::citations::emit_bookdown_crossref(builder, label);
                pos += len;
                text_start = pos;
                continue;
            }

            // Try escapes (after bookdown refs and backslash math)
            if let Some((len, ch, escape_type)) = try_parse_escape(&text[pos..]) {
                let escape_enabled = match escape_type {
                    EscapeType::HardLineBreak => config.extensions.escaped_line_breaks,
                    EscapeType::NonbreakingSpace => config.extensions.all_symbols_escapable,
                    EscapeType::Literal => {
                        const BASE_ESCAPABLE: &str = "\\`*_{}[]()>#+-.!";
                        BASE_ESCAPABLE.contains(ch) || config.extensions.all_symbols_escapable
                    }
                };
                if !escape_enabled {
                    // Don't treat as hard line break - skip the escape and continue
                    // The backslash will be included in the next TEXT token
                    pos = advance_char_boundary(text, pos, end);
                    continue;
                }

                // Emit accumulated text
                if pos > text_start {
                    builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
                }

                log::trace!("Matched escape at pos {}: \\{}", pos, ch);
                emit_escape(builder, ch, escape_type);
                pos += len;
                text_start = pos;
                continue;
            }

            // Try LaTeX commands (after escapes, before shortcodes)
            if config.extensions.raw_tex
                && let Some(len) = try_parse_latex_command(&text[pos..])
            {
                if pos > text_start {
                    builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
                }
                log::trace!("Matched LaTeX command at pos {}", pos);
                parse_latex_command(builder, &text[pos..], len);
                pos += len;
                text_start = pos;
                continue;
            }
        }

        // Try Quarto shortcodes: {{< shortcode >}}
        if byte == b'{'
            && pos + 1 < text.len()
            && text.as_bytes()[pos + 1] == b'{'
            && let Some((len, name, attrs)) = try_parse_shortcode(&text[pos..])
        {
            if pos > text_start {
                builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
            }
            log::trace!("Matched shortcode at pos {}: {}", pos, &name);
            emit_shortcode(builder, &name, attrs);
            pos += len;
            text_start = pos;
            continue;
        }

        // Try inline executable code spans (`... `r expr`` and `... `{r} expr``)
        if byte == b'`'
            && let Some(m) = try_parse_inline_executable(
                &text[pos..],
                config.extensions.rmarkdown_inline_code,
                config.extensions.quarto_inline_code,
            )
        {
            if pos > text_start {
                builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
            }
            log::trace!("Matched inline executable code at pos {}", pos);
            emit_inline_executable(builder, &m);
            pos += m.total_len;
            text_start = pos;
            continue;
        }

        // Try code spans
        if byte == b'`'
            && let Some((len, content, backtick_count, attributes)) =
                try_parse_code_span(&text[pos..])
        {
            // Emit accumulated text
            if pos > text_start {
                builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
            }

            log::trace!(
                "Matched code span at pos {}: {} backticks",
                pos,
                backtick_count
            );

            // Check for raw inline
            if let Some(ref attrs) = attributes
                && config.extensions.raw_attribute
                && let Some(format) = is_raw_inline(attrs)
            {
                use super::raw_inline::emit_raw_inline;
                log::trace!("Matched raw inline span at pos {}: format={}", pos, format);
                emit_raw_inline(builder, content, backtick_count, format);
            } else if !config.extensions.inline_code_attributes && attributes.is_some() {
                let code_span_len = backtick_count * 2 + content.len();
                emit_code_span(builder, content, backtick_count, None);
                pos += code_span_len;
                text_start = pos;
                continue;
            } else {
                emit_code_span(builder, content, backtick_count, attributes);
            }

            pos += len;
            text_start = pos;
            continue;
        }

        // Try textual emoji aliases: :smile:
        if byte == b':'
            && config.extensions.emoji
            && is_emoji_boundary(text, pos)
            && let Some((len, _alias)) = try_parse_emoji(&text[pos..])
        {
            if pos > text_start {
                builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
            }
            log::trace!("Matched emoji at pos {}", pos);
            emit_emoji(builder, &text[pos..pos + len]);
            pos += len;
            text_start = pos;
            continue;
        }

        // Try inline footnotes: ^[note]
        if byte == b'^'
            && pos + 1 < text.len()
            && text.as_bytes()[pos + 1] == b'['
            && config.extensions.inline_footnotes
            && let Some((len, content)) = try_parse_inline_footnote(&text[pos..])
        {
            if pos > text_start {
                builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
            }
            log::trace!("Matched inline footnote at pos {}", pos);
            emit_inline_footnote(builder, content, config);
            pos += len;
            text_start = pos;
            continue;
        }

        // Try superscript: ^text^
        if byte == b'^'
            && config.extensions.superscript
            && let Some((len, content)) = try_parse_superscript(&text[pos..])
        {
            if pos > text_start {
                builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
            }
            log::trace!("Matched superscript at pos {}", pos);
            emit_superscript(builder, content, config);
            pos += len;
            text_start = pos;
            continue;
        }

        // Try bookdown definition: (\#label) or (ref:label)
        if byte == b'(' && config.extensions.bookdown_references {
            if let Some((len, label)) = try_parse_bookdown_definition(&text[pos..]) {
                if pos > text_start {
                    builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
                }
                log::trace!("Matched bookdown definition at pos {}: {}", pos, label);
                builder.token(SyntaxKind::TEXT.into(), &text[pos..pos + len]);
                pos += len;
                text_start = pos;
                continue;
            }
            if let Some((len, label)) = try_parse_bookdown_text_reference(&text[pos..]) {
                if pos > text_start {
                    builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
                }
                log::trace!("Matched bookdown text reference at pos {}: {}", pos, label);
                builder.token(SyntaxKind::TEXT.into(), &text[pos..pos + len]);
                pos += len;
                text_start = pos;
                continue;
            }
        }

        // Try subscript: ~text~
        if byte == b'~'
            && config.extensions.subscript
            && let Some((len, content)) = try_parse_subscript(&text[pos..])
        {
            if pos > text_start {
                builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
            }
            log::trace!("Matched subscript at pos {}", pos);
            emit_subscript(builder, content, config);
            pos += len;
            text_start = pos;
            continue;
        }

        // Try strikeout: ~~text~~
        if byte == b'~'
            && config.extensions.strikeout
            && let Some((len, content)) = try_parse_strikeout(&text[pos..])
        {
            if pos > text_start {
                builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
            }
            log::trace!("Matched strikeout at pos {}", pos);
            emit_strikeout(builder, content, config);
            pos += len;
            text_start = pos;
            continue;
        }

        // Try mark/highlight: ==text==
        if byte == b'='
            && config.extensions.mark
            && let Some((len, content)) = try_parse_mark(&text[pos..])
        {
            if pos > text_start {
                builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
            }
            log::trace!("Matched mark at pos {}", pos);
            emit_mark(builder, content, config);
            pos += len;
            text_start = pos;
            continue;
        }

        // Try GFM inline math: $`...`$
        if byte == b'$'
            && config.extensions.tex_math_gfm
            && let Some((len, content)) = try_parse_gfm_inline_math(&text[pos..])
        {
            if pos > text_start {
                builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
            }
            log::trace!("Matched GFM inline math at pos {}", pos);
            emit_gfm_inline_math(builder, content);
            pos += len;
            text_start = pos;
            continue;
        }

        // Try math ($...$, $$...$$)
        if byte == b'$' && config.extensions.tex_math_dollars {
            // Try display math first ($$...$$)
            if let Some((len, content)) = try_parse_display_math(&text[pos..]) {
                // Emit accumulated text
                if pos > text_start {
                    builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
                }

                let dollar_count = text[pos..].chars().take_while(|&c| c == '$').count();
                log::trace!(
                    "Matched display math at pos {}: {} dollars",
                    pos,
                    dollar_count
                );

                // Check for trailing attributes (Quarto cross-reference support)
                let after_math = &text[pos + len..];
                let attr_len = if config.extensions.quarto_crossrefs {
                    use crate::parser::utils::attributes::try_parse_trailing_attributes;
                    if let Some((_attr_block, _)) = try_parse_trailing_attributes(after_math) {
                        let trimmed_after = after_math.trim_start();
                        if let Some(open_brace_pos) = trimmed_after.find('{') {
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

                let total_len = len + attr_len;
                emit_display_math(builder, content, dollar_count);

                // Emit attributes if present
                if attr_len > 0 {
                    use crate::parser::utils::attributes::{
                        emit_attributes, try_parse_trailing_attributes,
                    };
                    let attr_text = &text[pos + len..pos + total_len];
                    if let Some((attr_block, _text_before)) =
                        try_parse_trailing_attributes(attr_text)
                    {
                        let trimmed_after = attr_text.trim_start();
                        let ws_len = attr_text.len() - trimmed_after.len();
                        if ws_len > 0 {
                            builder.token(SyntaxKind::WHITESPACE.into(), &attr_text[..ws_len]);
                        }
                        emit_attributes(builder, &attr_block);
                    }
                }

                pos += total_len;
                text_start = pos;
                continue;
            }

            // Try inline math ($...$)
            if let Some((len, content)) = try_parse_inline_math(&text[pos..]) {
                // Emit accumulated text
                if pos > text_start {
                    builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
                }

                log::trace!("Matched inline math at pos {}", pos);
                emit_inline_math(builder, content);
                pos += len;
                text_start = pos;
                continue;
            }

            // Neither display nor inline math matched - emit the $ as literal text
            // This ensures each $ gets its own TEXT token for CST compatibility
            if pos > text_start {
                builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
            }
            builder.token(SyntaxKind::TEXT.into(), "$");
            pos = advance_char_boundary(text, pos, end);
            text_start = pos;
            continue;
        }

        // Try autolinks: <url> or <email>
        if byte == b'<'
            && config.extensions.autolinks
            && let Some((len, url)) = try_parse_autolink(&text[pos..])
        {
            if pos > text_start {
                builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
            }
            log::trace!("Matched autolink at pos {}", pos);
            emit_autolink(builder, &text[pos..pos + len], url);
            pos += len;
            text_start = pos;
            continue;
        }

        if config.extensions.autolink_bare_uris
            && let Some((len, url)) = try_parse_bare_uri(&text[pos..])
        {
            if pos > text_start {
                builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
            }
            log::trace!("Matched bare URI at pos {}", pos);
            emit_bare_uri_link(builder, url, config);
            pos += len;
            text_start = pos;
            continue;
        }

        // Try native spans: <span>text</span> (after autolink since both start with <)
        if byte == b'<'
            && config.extensions.native_spans
            && let Some((len, content, attributes)) = try_parse_native_span(&text[pos..])
        {
            if pos > text_start {
                builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
            }
            log::trace!("Matched native span at pos {}", pos);
            emit_native_span(builder, content, &attributes, config);
            pos += len;
            text_start = pos;
            continue;
        }

        // Images and links - process in order: inline image, reference image, footnote ref, inline link, reference link
        if byte == b'!' && pos + 1 < text.len() && text.as_bytes()[pos + 1] == b'[' {
            // Try inline image: ![alt](url)
            if let Some((len, alt_text, dest, attributes)) = try_parse_inline_image(&text[pos..]) {
                if pos > text_start {
                    builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
                }
                log::trace!("Matched inline image at pos {}", pos);
                emit_inline_image(
                    builder,
                    &text[pos..pos + len],
                    alt_text,
                    dest,
                    attributes,
                    config,
                );
                pos += len;
                text_start = pos;
                continue;
            }

            // Try reference image: ![alt][ref] or ![alt]
            if config.extensions.reference_links {
                let allow_shortcut = config.extensions.shortcut_reference_links;
                if let Some((len, alt_text, reference, is_implicit)) =
                    try_parse_reference_image(&text[pos..], allow_shortcut)
                {
                    if pos > text_start {
                        builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
                    }
                    log::trace!("Matched reference image at pos {}", pos);
                    emit_reference_image(builder, alt_text, &reference, is_implicit, config);
                    pos += len;
                    text_start = pos;
                    continue;
                }
            }
        }

        // Process bracket-starting elements
        if byte == b'[' {
            // Try footnote reference: [^id]
            if config.extensions.footnotes
                && let Some((len, id)) = try_parse_footnote_reference(&text[pos..])
            {
                if pos > text_start {
                    builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
                }
                log::trace!("Matched footnote reference at pos {}", pos);
                emit_footnote_reference(builder, &id);
                pos += len;
                text_start = pos;
                continue;
            }

            // Try inline link: [text](url)
            if config.extensions.inline_links
                && let Some((len, link_text, dest, attributes)) =
                    try_parse_inline_link(&text[pos..])
            {
                if pos > text_start {
                    builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
                }
                log::trace!("Matched inline link at pos {}", pos);
                emit_inline_link(
                    builder,
                    &text[pos..pos + len],
                    link_text,
                    dest,
                    attributes,
                    config,
                );
                pos += len;
                text_start = pos;
                continue;
            }

            // Try reference link: [text][ref] or [text]
            if config.extensions.reference_links {
                let allow_shortcut = config.extensions.shortcut_reference_links;
                if let Some((len, link_text, reference, is_implicit)) =
                    try_parse_reference_link(&text[pos..], allow_shortcut)
                {
                    if pos > text_start {
                        builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
                    }
                    log::trace!("Matched reference link at pos {}", pos);
                    emit_reference_link(builder, link_text, &reference, is_implicit, config);
                    pos += len;
                    text_start = pos;
                    continue;
                }
            }

            // Try bracketed citation: [@cite]
            if config.extensions.citations
                && let Some((len, content)) = try_parse_bracketed_citation(&text[pos..])
            {
                if pos > text_start {
                    builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
                }
                log::trace!("Matched bracketed citation at pos {}", pos);
                emit_bracketed_citation(builder, content);
                pos += len;
                text_start = pos;
                continue;
            }
        }

        // Try bracketed spans: [text]{.class}
        // Must come after links/citations
        if byte == b'['
            && config.extensions.bracketed_spans
            && let Some((len, text_content, attrs)) = try_parse_bracketed_span(&text[pos..])
        {
            if pos > text_start {
                builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
            }
            log::trace!("Matched bracketed span at pos {}", pos);
            emit_bracketed_span(builder, &text_content, &attrs, config);
            pos += len;
            text_start = pos;
            continue;
        }

        // Try bare citation: @cite (must come after bracketed elements)
        if byte == b'@'
            && (config.extensions.citations || config.extensions.quarto_crossrefs)
            && let Some((len, key, has_suppress)) = try_parse_bare_citation(&text[pos..])
        {
            let is_crossref =
                config.extensions.quarto_crossrefs && super::citations::is_quarto_crossref_key(key);
            if is_crossref || config.extensions.citations {
                if pos > text_start {
                    builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
                }
                if is_crossref {
                    log::trace!("Matched Quarto crossref at pos {}: {}", pos, &key);
                    super::citations::emit_crossref(builder, key, has_suppress);
                } else {
                    log::trace!("Matched bare citation at pos {}: {}", pos, &key);
                    emit_bare_citation(builder, key, has_suppress);
                }
                pos += len;
                text_start = pos;
                continue;
            }
        }

        // Try suppress-author citation: -@cite
        if byte == b'-'
            && pos + 1 < text.len()
            && text.as_bytes()[pos + 1] == b'@'
            && (config.extensions.citations || config.extensions.quarto_crossrefs)
            && let Some((len, key, has_suppress)) = try_parse_bare_citation(&text[pos..])
        {
            let is_crossref =
                config.extensions.quarto_crossrefs && super::citations::is_quarto_crossref_key(key);
            if is_crossref || config.extensions.citations {
                if pos > text_start {
                    builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
                }
                if is_crossref {
                    log::trace!("Matched Quarto crossref at pos {}: {}", pos, &key);
                    super::citations::emit_crossref(builder, key, has_suppress);
                } else {
                    log::trace!("Matched suppress-author citation at pos {}: {}", pos, &key);
                    emit_bare_citation(builder, key, has_suppress);
                }
                pos += len;
                text_start = pos;
                continue;
            }
        }

        // Try to parse emphasis at this position
        if byte == b'*' || byte == b'_' {
            // Count the delimiter run to avoid re-parsing
            let bytes = text.as_bytes();
            let mut delim_count = 0;
            while pos + delim_count < bytes.len() && bytes[pos + delim_count] == byte {
                delim_count += 1;
            }

            // Emit any accumulated text before the delimiter
            if pos > text_start {
                log::trace!(
                    "Emitting TEXT before delimiter: {:?}",
                    &text[text_start..pos]
                );
                builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
                text_start = pos; // Update text_start after emission
            }

            // Try to parse emphasis
            // Use nested variant (bypass opener validity) when in nested context
            let emphasis_result = if nested_emphasis {
                try_parse_emphasis_nested(text, pos, end, config, builder)
            } else {
                try_parse_emphasis(text, pos, end, config, builder)
            };

            if let Some((consumed, _)) = emphasis_result {
                // Successfully parsed emphasis
                log::trace!(
                    "Parsed emphasis, consumed {} bytes from pos {}",
                    consumed,
                    pos
                );
                pos += consumed;
                text_start = pos;
            } else {
                // Failed to parse, delimiter run will be treated as regular text
                // Skip the ENTIRE delimiter run to avoid re-parsing parts of it
                log::trace!(
                    "Failed to parse emphasis at pos {}, skipping {} delimiters as literal",
                    pos,
                    delim_count
                );
                pos += delim_count;
                // DON'T update text_start - let the delimiters accumulate
            }
            continue;
        }

        // Check for newlines - may need to emit as hard line break
        if byte == b'\r' && pos + 1 < end && text.as_bytes()[pos + 1] == b'\n' {
            let text_before = &text[text_start..pos];

            // Check for trailing spaces hard line break (always enabled in Pandoc)
            let trailing_spaces = text_before.chars().rev().take_while(|&c| c == ' ').count();
            if trailing_spaces >= 2 {
                // Emit text before the trailing spaces
                let text_content = &text_before[..text_before.len() - trailing_spaces];
                if !text_content.is_empty() {
                    builder.token(SyntaxKind::TEXT.into(), text_content);
                }
                let spaces = " ".repeat(trailing_spaces);
                builder.token(
                    SyntaxKind::HARD_LINE_BREAK.into(),
                    &format!("{}\r\n", spaces),
                );
                pos += 2;
                text_start = pos;
                continue;
            }

            // hard_line_breaks: treat all single newlines as hard line breaks
            if config.extensions.hard_line_breaks {
                if !text_before.is_empty() {
                    builder.token(SyntaxKind::TEXT.into(), text_before);
                }
                builder.token(SyntaxKind::HARD_LINE_BREAK.into(), "\r\n");
                pos += 2;
                text_start = pos;
                continue;
            }

            // Regular newline
            if !text_before.is_empty() {
                builder.token(SyntaxKind::TEXT.into(), text_before);
            }
            builder.token(SyntaxKind::NEWLINE.into(), "\r\n");
            pos += 2;
            text_start = pos;
            continue;
        }

        if byte == b'\n' {
            let text_before = &text[text_start..pos];

            // Check for trailing spaces hard line break (always enabled in Pandoc)
            let trailing_spaces = text_before.chars().rev().take_while(|&c| c == ' ').count();
            if trailing_spaces >= 2 {
                // Emit text before the trailing spaces
                let text_content = &text_before[..text_before.len() - trailing_spaces];
                if !text_content.is_empty() {
                    builder.token(SyntaxKind::TEXT.into(), text_content);
                }
                let spaces = " ".repeat(trailing_spaces);
                builder.token(SyntaxKind::HARD_LINE_BREAK.into(), &format!("{}\n", spaces));
                pos += 1;
                text_start = pos;
                continue;
            }

            // hard_line_breaks: treat all single newlines as hard line breaks
            if config.extensions.hard_line_breaks {
                if !text_before.is_empty() {
                    builder.token(SyntaxKind::TEXT.into(), text_before);
                }
                builder.token(SyntaxKind::HARD_LINE_BREAK.into(), "\n");
                pos += 1;
                text_start = pos;
                continue;
            }

            // Regular newline
            if !text_before.is_empty() {
                builder.token(SyntaxKind::TEXT.into(), text_before);
            }
            builder.token(SyntaxKind::NEWLINE.into(), "\n");
            pos += 1;
            text_start = pos;
            continue;
        }

        // Regular character, keep accumulating
        pos = advance_char_boundary(text, pos, end);
    }

    // Emit any remaining text
    if pos > text_start && text_start < end {
        log::trace!("Emitting remaining TEXT: {:?}", &text[text_start..end]);
        builder.token(SyntaxKind::TEXT.into(), &text[text_start..end]);
    }

    log::trace!("parse_inline_range complete: start={}, end={}", start, end);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::{SyntaxKind, SyntaxNode};
    use rowan::GreenNode;

    #[test]
    fn test_recursive_simple_emphasis() {
        let text = "*test*";
        let config = ParserOptions::default();
        let mut builder = GreenNodeBuilder::new();

        parse_inline_text_recursive(&mut builder, text, &config);

        let green: GreenNode = builder.finish();
        let node = SyntaxNode::new_root(green);

        // Should be lossless
        assert_eq!(node.text().to_string(), text);

        // Should have EMPHASIS node
        let has_emph = node.descendants().any(|n| n.kind() == SyntaxKind::EMPHASIS);
        assert!(has_emph, "Should have EMPHASIS node");
    }

    #[test]
    fn test_recursive_nested() {
        let text = "*foo **bar** baz*";
        let config = ParserOptions::default();
        let mut builder = GreenNodeBuilder::new();

        // Wrap in a PARAGRAPH node (inline content needs a parent)
        builder.start_node(SyntaxKind::PARAGRAPH.into());
        parse_inline_text_recursive(&mut builder, text, &config);
        builder.finish_node();

        let green: GreenNode = builder.finish();
        let node = SyntaxNode::new_root(green);

        // Should be lossless
        assert_eq!(node.text().to_string(), text);

        // Should have both EMPHASIS and STRONG
        let has_emph = node.descendants().any(|n| n.kind() == SyntaxKind::EMPHASIS);
        let has_strong = node.descendants().any(|n| n.kind() == SyntaxKind::STRONG);

        assert!(has_emph, "Should have EMPHASIS node");
        assert!(has_strong, "Should have STRONG node");
    }

    /// Test that we can parse a simple emphasis case
    #[test]
    fn test_parse_simple_emphasis() {
        use crate::options::ParserOptions;
        use crate::syntax::SyntaxNode;
        use rowan::GreenNode;

        let text = "*test*";
        let config = ParserOptions::default();
        let mut builder = GreenNodeBuilder::new();

        // Try to parse emphasis at position 0
        let result = try_parse_emphasis(text, 0, text.len(), &config, &mut builder);

        // Should successfully parse
        assert_eq!(result, Some((6, 1))); // Consumed all 6 bytes, delimiter count 1

        // Check the generated CST
        let green: GreenNode = builder.finish();
        let node = SyntaxNode::new_root(green);

        // The root IS the EMPHASIS node
        assert_eq!(node.kind(), SyntaxKind::EMPHASIS);

        // Verify losslessness: CST text should match input
        assert_eq!(node.text().to_string(), text);
    }

    /// Test parsing nested emphasis/strong
    #[test]
    fn test_parse_nested_emphasis_strong() {
        use crate::options::ParserOptions;

        let text = "*foo **bar** baz*";
        let config = ParserOptions::default();
        let mut builder = GreenNodeBuilder::new();

        // Parse the whole range
        parse_inline_range(text, 0, text.len(), &config, &mut builder);

        let green = builder.finish();
        let node = crate::syntax::SyntaxNode::new_root(green);

        // Verify losslessness
        assert_eq!(node.text().to_string(), text);

        // Should have EMPHASIS and STRONG nodes
        let has_emph = node.descendants().any(|n| n.kind() == SyntaxKind::EMPHASIS);
        let has_strong = node.descendants().any(|n| n.kind() == SyntaxKind::STRONG);

        assert!(has_emph, "Should have EMPHASIS node");
        assert!(has_strong, "Should have STRONG node");
    }

    /// Test Pandoc's "three" algorithm: ***foo* bar**
    /// Expected: Strong[Emph[foo], bar]
    /// Current bug: Parses as *Strong[foo* bar]
    #[test]
    fn test_triple_emphasis_star_then_double_star() {
        use crate::options::ParserOptions;
        use crate::syntax::SyntaxNode;
        use rowan::GreenNode;

        let text = "***foo* bar**";
        let config = ParserOptions::default();
        let mut builder = GreenNodeBuilder::new();

        builder.start_node(SyntaxKind::DOCUMENT.into());
        parse_inline_range(text, 0, text.len(), &config, &mut builder);
        builder.finish_node();

        let green: GreenNode = builder.finish();
        let node = SyntaxNode::new_root(green);

        // Verify losslessness
        assert_eq!(node.text().to_string(), text);

        // Expected structure: STRONG > EMPH > "foo"
        // The STRONG should contain EMPH, not the other way around
        let structure = format!("{:#?}", node);

        // Should have both STRONG and EMPH
        assert!(structure.contains("STRONG"), "Should have STRONG node");
        assert!(structure.contains("EMPHASIS"), "Should have EMPHASIS node");

        // STRONG should be outer, EMPH should be inner
        // Check that STRONG comes before EMPH in tree traversal
        let mut found_strong = false;
        let mut found_emph_after_strong = false;
        for descendant in node.descendants() {
            if descendant.kind() == SyntaxKind::STRONG {
                found_strong = true;
            }
            if found_strong && descendant.kind() == SyntaxKind::EMPHASIS {
                found_emph_after_strong = true;
                break;
            }
        }

        assert!(
            found_emph_after_strong,
            "EMPH should be inside STRONG, not before it. Current structure:\n{}",
            structure
        );
    }

    /// Test Pandoc's "three" algorithm: ***foo** bar*
    /// Expected: Emph[Strong[foo], bar]
    #[test]
    fn test_triple_emphasis_double_star_then_star() {
        use crate::options::ParserOptions;
        use crate::syntax::SyntaxNode;
        use rowan::GreenNode;

        let text = "***foo** bar*";
        let config = ParserOptions::default();
        let mut builder = GreenNodeBuilder::new();

        builder.start_node(SyntaxKind::DOCUMENT.into());
        parse_inline_range(text, 0, text.len(), &config, &mut builder);
        builder.finish_node();

        let green: GreenNode = builder.finish();
        let node = SyntaxNode::new_root(green);

        // Verify losslessness
        assert_eq!(node.text().to_string(), text);

        // Expected structure: EMPH > STRONG > "foo"
        let structure = format!("{:#?}", node);

        // Should have both EMPH and STRONG
        assert!(structure.contains("EMPHASIS"), "Should have EMPHASIS node");
        assert!(structure.contains("STRONG"), "Should have STRONG node");

        // EMPH should be outer, STRONG should be inner
        let mut found_emph = false;
        let mut found_strong_after_emph = false;
        for descendant in node.descendants() {
            if descendant.kind() == SyntaxKind::EMPHASIS {
                found_emph = true;
            }
            if found_emph && descendant.kind() == SyntaxKind::STRONG {
                found_strong_after_emph = true;
                break;
            }
        }

        assert!(
            found_strong_after_emph,
            "STRONG should be inside EMPH. Current structure:\n{}",
            structure
        );
    }

    /// Test that display math with attributes parses correctly
    /// Regression test for equation_attributes_single_line golden test
    #[test]
    fn test_display_math_with_attributes() {
        use crate::options::ParserOptions;
        use crate::syntax::SyntaxNode;
        use rowan::GreenNode;

        let text = "$$ E = mc^2 $$ {#eq-einstein}";
        let mut config = ParserOptions::default();
        config.extensions.quarto_crossrefs = true; // Enable Quarto cross-references

        let mut builder = GreenNodeBuilder::new();
        builder.start_node(SyntaxKind::DOCUMENT.into()); // Need a root node

        // Parse the whole text
        parse_inline_text_recursive(&mut builder, text, &config);

        builder.finish_node(); // Finish ROOT
        let green: GreenNode = builder.finish();
        let node = SyntaxNode::new_root(green);

        // Verify losslessness
        assert_eq!(node.text().to_string(), text);

        // Should have DISPLAY_MATH node
        let has_display_math = node
            .descendants()
            .any(|n| n.kind() == SyntaxKind::DISPLAY_MATH);
        assert!(has_display_math, "Should have DISPLAY_MATH node");

        // Should have ATTRIBUTE node
        let has_attributes = node
            .descendants()
            .any(|n| n.kind() == SyntaxKind::ATTRIBUTE);
        assert!(
            has_attributes,
            "Should have ATTRIBUTE node for {{#eq-einstein}}"
        );

        // Attributes should not be TEXT
        let math_followed_by_text = node.descendants().any(|n| {
            n.kind() == SyntaxKind::DISPLAY_MATH
                && n.next_sibling()
                    .map(|s| {
                        s.kind() == SyntaxKind::TEXT
                            && s.text().to_string().contains("{#eq-einstein}")
                    })
                    .unwrap_or(false)
        });
        assert!(
            !math_followed_by_text,
            "Attributes should not be parsed as TEXT"
        );
    }

    #[test]
    fn test_parse_inline_text_gfm_inline_link_destination_not_autolinked() {
        use crate::options::{Extensions, Flavor};

        let config = ParserOptions {
            flavor: Flavor::Gfm,
            extensions: Extensions::for_flavor(Flavor::Gfm),
            ..ParserOptions::default()
        };

        let mut builder = GreenNodeBuilder::new();
        builder.start_node(SyntaxKind::PARAGRAPH.into());
        parse_inline_text_recursive(
            &mut builder,
            "Second Link [link_text](https://link.com)",
            &config,
        );
        builder.finish_node();
        let green = builder.finish();
        let root = SyntaxNode::new_root(green);

        let links: Vec<_> = root
            .descendants()
            .filter(|n| n.kind() == SyntaxKind::LINK)
            .collect();
        assert_eq!(
            links.len(),
            1,
            "Expected exactly one LINK node for inline link, not nested bare URI autolink"
        );

        let link = links[0].clone();
        let mut link_text = None::<String>;
        let mut link_dest = None::<String>;

        for child in link.children() {
            match child.kind() {
                SyntaxKind::LINK_TEXT => link_text = Some(child.text().to_string()),
                SyntaxKind::LINK_DEST => link_dest = Some(child.text().to_string()),
                _ => {}
            }
        }

        assert_eq!(link_text.as_deref(), Some("link_text"));
        assert_eq!(link_dest.as_deref(), Some("https://link.com"));
    }

    #[test]
    fn test_autolink_bare_uri_utf8_boundary_safe() {
        let text = "§";
        let mut config = ParserOptions::default();
        config.extensions.autolink_bare_uris = true;
        let mut builder = GreenNodeBuilder::new();

        builder.start_node(SyntaxKind::DOCUMENT.into());
        parse_inline_text_recursive(&mut builder, text, &config);
        builder.finish_node();

        let green: GreenNode = builder.finish();
        let node = SyntaxNode::new_root(green);
        assert_eq!(node.text().to_string(), text);
    }

    #[test]
    fn test_parse_emphasis_unicode_content_no_panic() {
        let text = "*§*";
        let config = ParserOptions::default();
        let mut builder = GreenNodeBuilder::new();

        let result = try_parse_emphasis(text, 0, text.len(), &config, &mut builder);
        assert_eq!(result, Some((text.len(), 1)));

        let green: GreenNode = builder.finish();
        let node = SyntaxNode::new_root(green);
        assert_eq!(node.kind(), SyntaxKind::EMPHASIS);
        assert_eq!(node.text().to_string(), text);
    }
}

#[test]
fn test_two_with_nested_one_and_triple_closer() {
    // **bold with *italic***
    // Should parse as: Strong["bold with ", Emph["italic"]]
    // The *** at end is parsed as * (closes Emph) + ** (closes Strong)

    use crate::options::ParserOptions;
    use crate::syntax::SyntaxNode;
    use rowan::GreenNode;

    let text = "**bold with *italic***";
    let config = ParserOptions::default();
    let mut builder = GreenNodeBuilder::new();

    // parse_inline_range emits inline content directly
    parse_inline_range(text, 0, text.len(), &config, &mut builder);

    let green: GreenNode = builder.finish();
    let node = SyntaxNode::new_root(green);

    // Verify lossless parsing
    assert_eq!(node.text().to_string(), text, "Should be lossless");

    // The root node should be STRONG (parse_inline_range doesn't add wrapper)
    assert_eq!(
        node.kind(),
        SyntaxKind::STRONG,
        "Root should be STRONG, got: {:?}",
        node.kind()
    );

    // STRONG should contain EMPHASIS as a nested node
    let has_emphasis = node.children().any(|c| c.kind() == SyntaxKind::EMPHASIS);
    assert!(has_emphasis, "STRONG should contain EMPHASIS node");
}

#[test]
fn test_emphasis_with_trailing_space_before_closer() {
    // *foo * should parse as emphasis (Pandoc behavior)
    // For asterisks, Pandoc doesn't require right-flanking for closers

    use crate::options::ParserOptions;
    use crate::syntax::SyntaxNode;
    use rowan::GreenNode;

    let text = "*foo *";
    let config = ParserOptions::default();
    let mut builder = GreenNodeBuilder::new();

    // Try to parse emphasis at position 0
    let result = try_parse_emphasis(text, 0, text.len(), &config, &mut builder);

    // Should successfully parse (consumed all 6 bytes, delimiter count 1)
    assert_eq!(
        result,
        Some((6, 1)),
        "Should parse as emphasis, result: {:?}",
        result
    );

    // Check the generated CST
    let green: GreenNode = builder.finish();
    let node = SyntaxNode::new_root(green);

    // The root IS the EMPHASIS node
    assert_eq!(node.kind(), SyntaxKind::EMPHASIS);

    // Verify losslessness
    assert_eq!(node.text().to_string(), text);
}

#[test]
fn test_triple_emphasis_all_strong_nested() {
    // ***foo** bar **baz*** should parse as Emph[Strong[foo], " bar ", Strong[baz]]
    // Pandoc output confirms this

    use crate::options::ParserOptions;
    use crate::syntax::SyntaxNode;
    use rowan::GreenNode;

    let text = "***foo** bar **baz***";
    let config = ParserOptions::default();
    let mut builder = GreenNodeBuilder::new();

    parse_inline_range(text, 0, text.len(), &config, &mut builder);

    let green: GreenNode = builder.finish();
    let node = SyntaxNode::new_root(green);

    // Should have one EMPHASIS node at root
    let emphasis_nodes: Vec<_> = node
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::EMPHASIS)
        .collect();
    assert_eq!(
        emphasis_nodes.len(),
        1,
        "Should have exactly one EMPHASIS node, found: {}",
        emphasis_nodes.len()
    );

    // EMPHASIS should contain two STRONG nodes
    let emphasis_node = emphasis_nodes[0].clone();
    let strong_in_emphasis: Vec<_> = emphasis_node
        .children()
        .filter(|n| n.kind() == SyntaxKind::STRONG)
        .collect();
    assert_eq!(
        strong_in_emphasis.len(),
        2,
        "EMPHASIS should contain two STRONG nodes, found: {}",
        strong_in_emphasis.len()
    );

    // Verify losslessness
    assert_eq!(node.text().to_string(), text);
}

#[test]
fn test_triple_emphasis_all_emph_nested() {
    // ***foo* bar *baz*** should parse as Strong[Emph[foo], " bar ", Emph[baz]]
    // Pandoc output confirms this

    use crate::options::ParserOptions;
    use crate::syntax::SyntaxNode;
    use rowan::GreenNode;

    let text = "***foo* bar *baz***";
    let config = ParserOptions::default();
    let mut builder = GreenNodeBuilder::new();

    parse_inline_range(text, 0, text.len(), &config, &mut builder);

    let green: GreenNode = builder.finish();
    let node = SyntaxNode::new_root(green);

    // Should have one STRONG node at root
    let strong_nodes: Vec<_> = node
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::STRONG)
        .collect();
    assert_eq!(
        strong_nodes.len(),
        1,
        "Should have exactly one STRONG node, found: {}",
        strong_nodes.len()
    );

    // STRONG should contain two EMPHASIS nodes
    let strong_node = strong_nodes[0].clone();
    let emph_in_strong: Vec<_> = strong_node
        .children()
        .filter(|n| n.kind() == SyntaxKind::EMPHASIS)
        .collect();
    assert_eq!(
        emph_in_strong.len(),
        2,
        "STRONG should contain two EMPHASIS nodes, found: {}",
        emph_in_strong.len()
    );

    // Verify losslessness
    assert_eq!(node.text().to_string(), text);
}

// Multiline emphasis tests
#[test]
fn test_parse_emphasis_multiline() {
    // Per Pandoc spec, emphasis CAN contain newlines (soft breaks)
    use crate::options::ParserOptions;
    use crate::syntax::SyntaxNode;
    use rowan::GreenNode;

    let text = "*text on\nline two*";
    let config = ParserOptions::default();
    let mut builder = GreenNodeBuilder::new();

    let result = try_parse_emphasis(text, 0, text.len(), &config, &mut builder);

    // Should successfully parse all bytes
    assert_eq!(
        result,
        Some((text.len(), 1)),
        "Emphasis should parse multiline content"
    );

    // Check the generated CST
    let green: GreenNode = builder.finish();
    let node = SyntaxNode::new_root(green);

    // Should have EMPHASIS node
    assert_eq!(node.kind(), SyntaxKind::EMPHASIS);

    // Verify losslessness: should preserve the newline
    assert_eq!(node.text().to_string(), text);
    assert!(
        node.text().to_string().contains('\n'),
        "Should preserve newline in emphasis content"
    );
}

#[test]
fn test_parse_strong_multiline() {
    // Per Pandoc spec, strong emphasis CAN contain newlines
    use crate::options::ParserOptions;
    use crate::syntax::SyntaxNode;
    use rowan::GreenNode;

    let text = "**strong on\nline two**";
    let config = ParserOptions::default();
    let mut builder = GreenNodeBuilder::new();

    let result = try_parse_emphasis(text, 0, text.len(), &config, &mut builder);

    // Should successfully parse all bytes
    assert_eq!(
        result,
        Some((text.len(), 2)),
        "Strong emphasis should parse multiline content"
    );

    // Check the generated CST
    let green: GreenNode = builder.finish();
    let node = SyntaxNode::new_root(green);

    // Should have STRONG node
    assert_eq!(node.kind(), SyntaxKind::STRONG);

    // Verify losslessness
    assert_eq!(node.text().to_string(), text);
    assert!(
        node.text().to_string().contains('\n'),
        "Should preserve newline in strong content"
    );
}

#[test]
fn test_parse_triple_emphasis_multiline() {
    // Triple emphasis with newlines
    use crate::options::ParserOptions;
    use crate::syntax::SyntaxNode;
    use rowan::GreenNode;

    let text = "***both on\nline two***";
    let config = ParserOptions::default();
    let mut builder = GreenNodeBuilder::new();

    let result = try_parse_emphasis(text, 0, text.len(), &config, &mut builder);

    // Should successfully parse all bytes
    assert_eq!(
        result,
        Some((text.len(), 3)),
        "Triple emphasis should parse multiline content"
    );

    // Check the generated CST
    let green: GreenNode = builder.finish();
    let node = SyntaxNode::new_root(green);

    // Should have STRONG node (triple = strong + emph)
    let has_strong = node.descendants().any(|n| n.kind() == SyntaxKind::STRONG);
    assert!(has_strong, "Should have STRONG node");

    // Verify losslessness
    assert_eq!(node.text().to_string(), text);
    assert!(
        node.text().to_string().contains('\n'),
        "Should preserve newline in triple emphasis content"
    );
}

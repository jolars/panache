use crate::options::ParserOptions;
use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;

use super::blockquotes::count_blockquote_markers;
use super::container_prefix::StrippedLines;
use crate::parser::utils::container_stack::leading_indent;
use crate::parser::utils::helpers::strip_newline;
use crate::parser::utils::inline_emission;

/// Try to parse the start of a line block.
/// Returns Some(()) if this line starts a line block (| followed by space or end of line).
pub fn try_parse_line_block_start(line: &str) -> Option<()> {
    parse_line_block_line_marker(line).map(|_| ())
}

/// Parse a complete line block starting at current position.
/// Returns the new position after the line block.
///
pub(crate) fn parse_line_block(
    window: &StrippedLines<'_, '_>,
    builder: &mut GreenNodeBuilder<'static>,
    config: &ParserOptions,
) -> usize {
    let block = PreparedLineBlock::new(window);
    let end = block.end(window.pos());
    block.emit(window, builder, config);
    end
}

pub(crate) struct PreparedLineBlock {
    continuation_markers: Vec<Option<usize>>,
}

impl PreparedLineBlock {
    pub(crate) fn new(window: &StrippedLines<'_, '_>) -> Self {
        let prefix = window.prefix();
        let bq_depth = prefix.bq_depth();
        let list_content_col = prefix.list_content_col();
        let first = window
            .dispatch_override()
            .map_or_else(|| window.first_unconditional(), |(tail, _)| tail);
        let mut previous_is_bare = is_bare_marker(first);
        let mut continuation_markers = Vec::new();

        for (pos, &raw_line) in window.raw().iter().enumerate().skip(window.pos() + 1) {
            if prefix.list_start_ends_line(raw_line) {
                break;
            }
            let peek = window.peek_prefix_at(pos);
            let lazy_in_quote = bq_depth > 0 && count_blockquote_markers(raw_line).0 < bq_depth;
            let marker = if !previous_is_bare
                && !lazy_in_quote
                && continues_previous_line(peek, raw_line, list_content_col)
            {
                None
            } else if (!previous_is_bare || peek.starts_with('|'))
                && let Some(marker) = parse_line_block_line_marker(peek)
            {
                Some(marker)
            } else {
                break;
            };
            if marker.is_some() {
                previous_is_bare = is_bare_marker(peek);
            }
            continuation_markers.push(marker);
        }
        Self {
            continuation_markers,
        }
    }

    pub(crate) fn end(&self, start: usize) -> usize {
        start + 1 + self.continuation_markers.len()
    }

    pub(crate) fn emit(
        &self,
        window: &StrippedLines<'_, '_>,
        builder: &mut GreenNodeBuilder<'static>,
        config: &ParserOptions,
    ) {
        builder.start_node(SyntaxKind::LINE_BLOCK.into());

        builder.start_node(SyntaxKind::LINE_BLOCK_LINE.into());
        let first = window.emit_or_dispatch_tail(builder, window.pos());
        emit_line(builder, first, parse_line_block_line_marker(first), config);
        builder.finish_node();

        for (i, &marker) in self.continuation_markers.iter().enumerate() {
            builder.start_node(SyntaxKind::LINE_BLOCK_LINE.into());
            let tail = window.emit_prefix_at(builder, window.pos() + i + 1);
            emit_line(builder, tail, marker, config);
            builder.finish_node();
        }
        builder.finish_node();
    }
}

fn emit_line(
    builder: &mut GreenNodeBuilder<'static>,
    line: &str,
    marker: Option<usize>,
    config: &ParserOptions,
) {
    let content = if let Some(end) = marker {
        builder.token(SyntaxKind::LINE_BLOCK_MARKER.into(), &line[..end]);
        &line[end..]
    } else {
        line
    };
    let (text, newline) = strip_newline(content);
    if !text.is_empty() {
        inline_emission::emit_inlines(builder, text, config, false);
    }
    if !newline.is_empty() {
        builder.token(SyntaxKind::NEWLINE.into(), newline);
    }
}

/// Does this line continue the line above rather than open a line of its own?
///
/// Pandoc's rule is "the line starts with a space"
/// (`many (try (char ' ' >> anyLine))` in `lineBlockLine`), applied to the
/// text left after the enclosing containers have taken their prefix. `peek`
/// is that text, so it decides the ordinary cases.
///
/// The rule looks at nothing but that leading space: a line that *also*
/// carries a `| ` marker still folds in, because the gobble runs before the
/// next `lineBlockLine` gets a chance. `| a\n  | b` is one line reading
/// `a | b`, and the caller checks this predicate before the marker.
///
/// The list case needs the raw line too. Pandoc gobbles a list item's
/// continuation indent all-or-nothing (`optional (gobbleSpaces n)`): a lazy
/// line indented *less* than the content column keeps every leading space it
/// had, so it still reads as a continuation. Panache's strip is greedy and
/// eats what it can, so `" b |"` inside a two-column item reaches us as
/// `"b |"` — recover the pre-strip shape from `raw_line`. A line indented
/// *exactly* to the content column is not a continuation: there the gobble
/// succeeds and consumes all of it.
///
/// A whitespace-only line satisfies the rule and folds in as an empty
/// continuation, which is also what pandoc does — `| a\n  \n| b` is one line
/// block of two lines, not two line blocks.
fn continues_previous_line(peek: &str, raw_line: &str, list_content_col: usize) -> bool {
    if peek.starts_with(' ') {
        return true;
    }
    list_content_col > 0
        && raw_line.starts_with([' ', '\t'])
        && leading_indent(raw_line).0 < list_content_col
}

pub(crate) fn is_bare_marker(line: &str) -> bool {
    strip_newline(line).0 == "|"
}

/// Parse a line block marker and return the index where content starts.
/// Returns Some(index) if the line starts with "| " or just "|", None otherwise.
fn parse_line_block_line_marker(line: &str) -> Option<usize> {
    let trimmed_start = line.len() - line.trim_start().len();
    let after_indent = &line[trimmed_start..];

    if after_indent.starts_with("| ") {
        Some(trimmed_start + 2) // Skip "| "
    } else if is_bare_marker(line) {
        Some(1) // Just "|", no space
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::super::container_prefix::ContainerPrefix;
    use super::*;

    #[test]
    fn test_try_parse_line_block_start() {
        assert!(try_parse_line_block_start("| Some text").is_some());
        assert!(try_parse_line_block_start("| ").is_some());
        assert!(try_parse_line_block_start("|").is_some()); // Empty line block
        assert!(try_parse_line_block_start("  | Some text").is_some());

        assert!(try_parse_line_block_start("|No space").is_none());
        assert!(try_parse_line_block_start("Regular text").is_none());
        assert!(try_parse_line_block_start("").is_none());
    }

    #[test]
    fn test_parse_line_block_marker() {
        assert_eq!(parse_line_block_line_marker("| Some text"), Some(2));
        assert_eq!(parse_line_block_line_marker("| "), Some(2));
        assert_eq!(parse_line_block_line_marker("|"), Some(1)); // Empty line block
        assert_eq!(parse_line_block_line_marker("  | Indented"), Some(4));

        assert_eq!(parse_line_block_line_marker("|No space"), None);
        assert_eq!(parse_line_block_line_marker("Regular"), None);
    }

    #[test]
    fn test_simple_line_block() {
        let input = vec!["| Line one", "| Line two", "| Line three"];

        let mut builder = GreenNodeBuilder::new();
        let prefix = ContainerPrefix::default();
        let window = StrippedLines::new(&input, 0, &prefix);
        let new_pos = parse_line_block(&window, &mut builder, &ParserOptions::default());

        assert_eq!(new_pos, 3);
    }

    #[test]
    fn test_line_block_with_continuation() {
        let input = vec![
            "| This is a long line",
            "  that continues here",
            "| Second line",
        ];

        let mut builder = GreenNodeBuilder::new();
        let prefix = ContainerPrefix::default();
        let window = StrippedLines::new(&input, 0, &prefix);
        let new_pos = parse_line_block(&window, &mut builder, &ParserOptions::default());

        assert_eq!(new_pos, 3);
    }

    #[test]
    fn test_line_block_with_indentation() {
        let input = vec!["| First line", "|    Indented line", "| Back to normal"];

        let mut builder = GreenNodeBuilder::new();
        let prefix = ContainerPrefix::default();
        let window = StrippedLines::new(&input, 0, &prefix);
        let new_pos = parse_line_block(&window, &mut builder, &ParserOptions::default());

        assert_eq!(new_pos, 3);
    }

    #[test]
    fn continuation_needs_leading_space_at_top_level() {
        assert!(continues_previous_line("  b", "  b", 0));
        assert!(!continues_previous_line("b", "b", 0));
        assert!(!continues_previous_line("\n", "\n", 0));
        assert!(continues_previous_line("   \n", "   \n", 0));
    }

    #[test]
    fn indented_marker_line_continues_the_line_above() {
        assert!(continues_previous_line("  | b", "  | b", 0));
        assert!(continues_previous_line(" | b", " | b", 0));
        assert!(continues_previous_line("  |\n", "  |\n", 0));
        assert!(continues_previous_line("  |b", "  |b", 0));
        assert!(!continues_previous_line("| b", "| b", 0));
    }

    #[test]
    fn indented_marker_folds_in_at_top_level() {
        let input = vec!["| a", "  | b", "| c"];

        let mut builder = GreenNodeBuilder::new();
        let prefix = ContainerPrefix::default();
        let window = StrippedLines::new(&input, 0, &prefix);
        let new_pos = parse_line_block(&window, &mut builder, &ParserOptions::default());

        assert_eq!(new_pos, 3);
        let node = crate::syntax::SyntaxNode::new_root(builder.finish());
        let markers = node
            .descendants_with_tokens()
            .filter(|e| e.kind() == SyntaxKind::LINE_BLOCK_MARKER)
            .count();
        assert_eq!(markers, 2);
    }

    #[test]
    fn under_indented_lazy_line_continues_inside_a_list_item() {
        assert!(continues_previous_line("b |", " b |", 2));
        assert!(continues_previous_line(" b |", "   b |", 2));
        assert!(!continues_previous_line("b |", "  b |", 2));
        assert!(!continues_previous_line("b |", "b |", 2));
    }

    #[test]
    fn test_line_block_stops_at_non_line_block() {
        let input = vec!["| Line one", "| Line two", "Regular paragraph"];

        let mut builder = GreenNodeBuilder::new();
        let prefix = ContainerPrefix::default();
        let window = StrippedLines::new(&input, 0, &prefix);
        let new_pos = parse_line_block(&window, &mut builder, &ParserOptions::default());

        assert_eq!(new_pos, 2); // Should stop before "Regular paragraph"
    }
}

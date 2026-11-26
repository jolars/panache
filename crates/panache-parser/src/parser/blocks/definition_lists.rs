use crate::options::ParserOptions;
use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;

use crate::parser::blocks::container_prefix::{FrameVerdict, resolve_content_indent};
use crate::parser::utils::container_stack::{
    byte_index_at_column, leading_indent, leading_indent_from,
};
use crate::parser::utils::helpers::strip_newline;
use crate::parser::utils::inline_emission;

/// Tries to parse a definition list marker (`:` or `~`)
///
/// Returns Some((marker_char, indent_cols, spaces_after_cols, spaces_after_bytes)) if found, None otherwise.
/// The marker can be indented 0-3 spaces and must be followed by whitespace.
pub fn try_parse_definition_marker(line: &str) -> Option<(char, usize, usize, usize)> {
    try_parse_definition_marker_at(line, 0)
}

fn try_parse_definition_marker_at(
    line: &str,
    base_col: usize,
) -> Option<(char, usize, usize, usize)> {
    {
        let bytes = line.as_bytes();
        let mut i = 0;
        while i < bytes.len() && i < 3 && bytes[i] == b' ' {
            i += 1;
        }
        match bytes.get(i) {
            Some(&b':') | Some(&b'~') => {}
            _ => return None,
        }
    }

    let (indent_cols, indent_bytes) = leading_indent(line);
    if indent_cols > 3 {
        return None;
    }

    let after_indent = &line[indent_bytes..];

    let marker = after_indent.chars().next()?;
    if !matches!(marker, ':' | '~') {
        return None;
    }

    let after_marker = &after_indent[1..];

    if !after_marker.starts_with(' ')
        && !after_marker.starts_with('\t')
        && !after_marker.starts_with('\n')
        && !after_marker.starts_with('\r')
        && !after_marker.is_empty()
    {
        return None;
    }

    let marker_col = base_col + indent_cols;
    let (spaces_after_cols, spaces_after_bytes) = leading_indent_from(after_marker, marker_col + 1);

    Some((marker, marker_col, spaces_after_cols, spaces_after_bytes))
}

/// [`try_parse_definition_marker`] in the frame pandoc reads item contents in.
///
/// Pandoc reparses a list item's contents as a fresh block sequence starting
/// at the item's content column, so the marker's 0-3 space allowance is
/// measured from there rather than from column 0. Without this a `:` at a
/// content column of 4 or more is read as indented code, and the term above it
/// is left with no definition body at all.
///
/// Falls back to the column-0 frame, which is what a marker-line dispatch and
/// every top-level marker need. The returned `indent_cols` is absolute in both
/// frames, so callers keep using `byte_index_at_column` on the full line.
pub(in crate::parser) fn definition_marker_in_list_frame(
    line: &str,
    list_content_col: Option<usize>,
) -> Option<(char, usize, usize, usize)> {
    if let Some(content_col) = list_content_col
        && content_col > 0
        && resolve_content_indent(line, content_col).reaches_frame()
    {
        let base = byte_index_at_column(line, content_col);
        let base_col = leading_indent(&line[..base]).0;
        if let Some(found) = try_parse_definition_marker_at(&line[base..], base_col) {
            return Some(found);
        }
    }
    try_parse_definition_marker(line)
}

/// Emit a term line into the syntax tree.
///
/// `stripped_indent` carries the literal container-indent bytes the dispatcher
/// already removed from `line`; callers that pass an unstripped line pass
/// `None` and let the indent below be read off the line itself.
pub(crate) fn emit_term(
    builder: &mut GreenNodeBuilder<'static>,
    line: &str,
    stripped_indent: Option<&str>,
    config: &ParserOptions,
) {
    builder.start_node(SyntaxKind::TERM.into());
    let (text, newline_str) = strip_newline(line);
    let trimmed_text = text.trim_end();

    if let Some(indent) = stripped_indent.filter(|indent| !indent.is_empty()) {
        builder.token(SyntaxKind::WHITESPACE.into(), indent);
    }
    let indent_len = trimmed_text.len() - trimmed_text.trim_start().len();
    if indent_len > 0 {
        builder.token(SyntaxKind::WHITESPACE.into(), &trimmed_text[..indent_len]);
    }
    let term_text = &trimmed_text[indent_len..];

    if !term_text.is_empty() {
        inline_emission::emit_inlines(builder, term_text, config, false);
    }

    let trailing = &text[trimmed_text.len()..];
    if !trailing.is_empty() {
        builder.token(SyntaxKind::WHITESPACE.into(), trailing);
    }

    if !newline_str.is_empty() {
        builder.token(SyntaxKind::NEWLINE.into(), newline_str);
    }
    builder.finish_node(); // Term
}

/// Emit a definition marker preceded by the literal bytes of its indent.
///
/// The indent is passed as a slice rather than a column count: in the list
/// content-column frame it can contain a tab, which regenerating spaces would
/// silently rewrite.
pub(crate) fn emit_definition_marker(
    builder: &mut GreenNodeBuilder<'static>,
    marker: char,
    indent: &str,
) {
    if !indent.is_empty() {
        builder.token(SyntaxKind::WHITESPACE.into(), indent);
    }
    builder.token(SyntaxKind::DEFINITION_MARKER.into(), &marker.to_string());
}

use crate::parser::blocks::tables::{LineView, is_caption_followed_by_table};
use crate::parser::utils::container_stack::{Container, ContainerStack};

/// Blank lines a term may keep between itself and its first definition
/// marker. Pandoc's `definitionListItem` reads the term, then `option False $
/// blankline` — a single blank line — before the first `:`.
pub(in crate::parser) const MAX_BLANKS_BEFORE_DEFINITION: usize = 1;

/// Check if we're in a definition list.
pub(in crate::parser) fn in_definition_list(containers: &ContainerStack) -> bool {
    containers
        .stack
        .iter()
        .any(|c| matches!(c, Container::DefinitionList { .. }))
}

/// Look ahead past at most one blank line to find a definition marker.
/// Returns Some(blank_line_count) if found, None otherwise.
///
/// Pandoc's term parser takes an *optional single* `blankline` before the
/// first definition marker (`option False $ blankline`), so a second blank
/// detaches the term from the marker below it and neither is part of a
/// definition list: `T\n\n\n: b` is `Para "T"` + `Para ": b"`. Blank lines
/// between two *definitions* of the same term are a different rule — the
/// preceding body consumes them — and are not this lookahead's business.
///
/// `lines` is absolute-indexed. Pass a [`StrippedLines`] rather than a raw
/// slice whenever a container prefix is open: the marker test only accepts
/// 0-3 leading spaces, so a raw `> : b` never matches and the term line above
/// it would silently lose its definition body.
pub(in crate::parser) fn next_line_is_definition_marker(
    lines: &(impl LineView + ?Sized),
    pos: usize,
    table_captions_enabled: bool,
) -> Option<usize> {
    let mut check_pos = pos + 1;
    let mut blank_count = 0;
    while check_pos < lines.line_count() {
        let line = lines.line(check_pos);
        if line.trim().is_empty() {
            blank_count += 1;
            if blank_count > MAX_BLANKS_BEFORE_DEFINITION {
                return None;
            }
            check_pos += 1;
            continue;
        }
        let found = match lines.frame_verdict(check_pos) {
            FrameVerdict::Inside { rest } | FrameVerdict::Dedented { rest, .. } => {
                try_parse_definition_marker(rest)
            }
            FrameVerdict::FakedIndent { .. } => return None,
            FrameVerdict::StraddlingTab {
                rest,
                cols_before_tab,
            } => {
                let tab_end_col = (cols_before_tab as usize / 4 + 1) * 4;
                try_parse_definition_marker_at(&rest[1..], tab_end_col)
            }
        };
        if let Some((marker, ..)) = found {
            if marker == ':'
                && table_captions_enabled
                && is_caption_followed_by_table(lines, check_pos)
            {
                return None;
            }
            return Some(blank_count);
        } else {
            return None;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::blocks::tables::is_caption_followed_by_table;

    #[test]
    fn test_parse_definition_marker_colon() {
        assert_eq!(
            try_parse_definition_marker(":   Definition"),
            Some((':', 0, 3, 3))
        );
    }

    #[test]
    fn test_parse_definition_marker_tilde() {
        assert_eq!(
            try_parse_definition_marker("~   Definition"),
            Some(('~', 0, 3, 3))
        );
    }

    #[test]
    fn test_parse_definition_marker_indented() {
        assert_eq!(
            try_parse_definition_marker("  : Definition"),
            Some((':', 2, 1, 1))
        );
        assert_eq!(
            try_parse_definition_marker("   ~ Definition"),
            Some(('~', 3, 1, 1))
        );
        assert_eq!(try_parse_definition_marker("\t: Definition"), None);
    }

    #[test]
    fn test_parse_definition_marker_too_indented() {
        assert_eq!(try_parse_definition_marker("    : Definition"), None);
    }

    #[test]
    fn test_parse_definition_marker_no_space_after() {
        assert_eq!(try_parse_definition_marker(":Definition"), None);
    }

    #[test]
    fn test_parse_definition_marker_at_eol() {
        assert_eq!(try_parse_definition_marker(":"), Some((':', 0, 0, 0)));
    }

    #[test]
    fn test_parse_definition_marker_bare_with_newline() {
        assert_eq!(try_parse_definition_marker(":\n"), Some((':', 0, 0, 0)));
        assert_eq!(try_parse_definition_marker("~\n"), Some(('~', 0, 0, 0)));
        assert_eq!(try_parse_definition_marker(":\r\n"), Some((':', 0, 0, 0)));
    }

    #[test]
    fn next_line_marker_respects_table_captions() {
        let lines = [
            "Here's a table with a reference:",
            "",
            ": (\\#tab:mytable) A table with a reference.",
            "",
            "| A   | B   | C   |",
            "| --- | --- | --- |",
            "| 1   | 2   | 3   |",
        ];
        assert!(is_caption_followed_by_table(&lines[..], 2));
        assert_eq!(next_line_is_definition_marker(&lines[..], 0, true), None);
        assert_eq!(
            next_line_is_definition_marker(&lines[..], 0, false),
            Some(1)
        );
    }

    /// The caption probe must stay inside the container the caption sits in.
    ///
    /// A dash run at column 0 below the list is a thematic break that closes
    /// the definition body; it is not the table a `:   def` marker *inside*
    /// that body could caption. Pandoc reads both shapes as a definition list
    /// nested on `a`, plus a `HorizontalRule`, so the trailing break must not
    /// change how the marker line above it is classified.
    #[test]
    fn caption_probe_stops_at_the_container_boundary() {
        for input in [
            "T\n\n:   a\n\n    :   def\n\n----\n",
            "- T\n\n  :   a\n\n      :   def\n\n----\n",
        ] {
            let tree = crate::parse(input, Some(crate::ParserOptions::default()));
            assert_eq!(tree.text().to_string(), input, "lossless: {input:?}");
            let lists = tree
                .descendants()
                .filter(|n| n.kind() == crate::syntax::SyntaxKind::DEFINITION_LIST)
                .count();
            assert_eq!(
                lists, 2,
                "`:   def` should nest a definition list on `a`, not read as a \
                 caption for the thematic break below the container: {input:?}"
            );
        }
    }

    /// The other side of the bound: a table that really is *inside* the body
    /// still makes the `:` line above it a caption, so the marker must not
    /// nest a definition list. Guards against widening
    /// `caption_probe_stops_at_the_container_boundary` into "never a caption".
    #[test]
    fn caption_probe_still_fires_inside_the_container() {
        let input = "T\n\n:   a\n\n    :   def\n\n    ----\n    x\n    ----\n";
        let tree = crate::parse(input, Some(crate::ParserOptions::default()));
        let lists = tree
            .descendants()
            .filter(|n| n.kind() == crate::syntax::SyntaxKind::DEFINITION_LIST)
            .count();
        assert_eq!(
            lists, 1,
            "the dash run sits at the body's content column, so `:   def` \
             captions it and nests nothing"
        );
    }

    #[test]
    fn test_definition_list_preserves_first_content_line_losslessly() {
        let input = "[`--reference-doc=`*FILE*]{#option--reference-doc}\n\n:   Use the specified file as a style reference in producing a\n    docx or ODT file.\n\n    Docx\n\n    :   For best results, the reference docx should be a modified\n        version of a docx file produced using pandoc.\n";
        let tree = crate::parse(input, Some(crate::ParserOptions::default()));
        assert_eq!(tree.text().to_string(), input);
    }
}

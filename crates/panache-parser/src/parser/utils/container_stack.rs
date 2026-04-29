use super::list_item_buffer::ListItemBuffer;
use super::text_buffer::{ParagraphBuffer, TextBuffer};
use crate::parser::blocks::lists::ListMarker;
use rowan::Checkpoint;

#[derive(Debug, Clone)]
pub(crate) enum Container {
    BlockQuote {
        // No special tracking needed
    },
    Alert {
        blockquote_depth: usize,
    },
    FencedDiv {
        // No special tracking needed - closed by fence marker
    },
    List {
        marker: ListMarker,
        base_indent_cols: usize,
        has_blank_between_items: bool, // Track if list is loose (blank lines between items)
    },
    ListItem {
        content_col: usize,
        buffer: ListItemBuffer, // Buffer for list item content
        /// True iff this list item has so far only seen its marker line, with
        /// no real content (text, nested list, etc.) — a marker-only item.
        /// Used by CommonMark to close empty list items at the first blank
        /// line, per spec §5.2 ("a list item can begin with at most one
        /// blank line"). Pandoc keeps the item open across the blank.
        marker_only: bool,
        /// True when the marker's required-1-col space was virtually absorbed
        /// from a tab in the post-marker text rather than consumed as a
        /// literal byte. In that case the buffered content's first byte is at
        /// source column `content_col - 1`, not `content_col`. Used by
        /// indented-code-from-marker-line detection to walk col-aware leading
        /// whitespace correctly.
        virtual_marker_space: bool,
    },
    DefinitionList {
        // Definition lists don't need special tracking
    },
    DefinitionItem {
        // No special tracking needed
    },
    Definition {
        content_col: usize,
        plain_open: bool,
        plain_buffer: TextBuffer, // Buffer for accumulating PLAIN content
    },
    Paragraph {
        buffer: ParagraphBuffer, // Interleaved buffer for paragraph content with markers
        open_inline_math_envs: Vec<String>,
        open_display_math_dollar_count: Option<usize>,
        // Checkpoint at the position the paragraph started; used to retroactively
        // wrap buffered content as PARAGRAPH (or HEADING for multi-line setext)
        // when the paragraph is closed.
        start_checkpoint: Checkpoint,
    },
    FootnoteDefinition {
        content_col: usize,
    },
}

pub(crate) struct ContainerStack {
    pub(crate) stack: Vec<Container>,
}

const TAB_STOP: usize = 4;

impl ContainerStack {
    pub(crate) fn new() -> Self {
        Self { stack: Vec::new() }
    }

    pub(crate) fn depth(&self) -> usize {
        self.stack.len()
    }

    pub(crate) fn last(&self) -> Option<&Container> {
        self.stack.last()
    }

    pub(crate) fn push(&mut self, c: Container) {
        self.stack.push(c);
    }
}

/// Expand tabs to columns (tab stop = 4) and return (cols, byte_offset).
pub(crate) fn leading_indent(line: &str) -> (usize, usize) {
    leading_indent_from(line, 0)
}

/// Like [`leading_indent`] but seeds the column counter at `start_col` so tab
/// expansion honors source-column tab-stops. Use when the leading whitespace
/// being measured doesn't begin at source column 0 (e.g. the bytes after a
/// list marker, where the marker itself occupies columns
/// `[indent_cols, indent_cols + marker_len)`).
pub(crate) fn leading_indent_from(line: &str, start_col: usize) -> (usize, usize) {
    let mut cols = 0usize;
    let mut bytes = 0usize;
    for b in line.bytes() {
        match b {
            b' ' => {
                cols += 1;
                bytes += 1;
            }
            b'\t' => {
                let absolute = start_col + cols;
                cols += TAB_STOP - (absolute % TAB_STOP);
                bytes += 1;
            }
            _ => break,
        }
    }
    (cols, bytes)
}

/// Return byte index at a given column (tabs = 4).
pub(crate) fn byte_index_at_column(line: &str, target_col: usize) -> usize {
    let mut col = 0usize;
    let mut idx = 0usize;
    for (i, b) in line.bytes().enumerate() {
        if col >= target_col {
            return idx;
        }
        match b {
            b' ' => {
                col += 1;
                idx = i + 1;
            }
            b'\t' => {
                col += TAB_STOP - (col % TAB_STOP);
                idx = i + 1;
            }
            _ => break,
        }
    }
    idx
}

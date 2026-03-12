use super::list_item_buffer::ListItemBuffer;
use super::text_buffer::{ParagraphBuffer, TextBuffer};
use crate::parser::blocks::lists::ListMarker;

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
    let mut cols = 0usize;
    let mut bytes = 0usize;
    for b in line.bytes() {
        match b {
            b' ' => {
                cols += 1;
                bytes += 1;
            }
            b'\t' => {
                cols += TAB_STOP - (cols % TAB_STOP);
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

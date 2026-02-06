use crate::block_parser::lists::ListMarker;
use rowan::GreenNodeBuilder;

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) enum Container {
    BlockQuote {
        content_col: usize,
    },
    FencedDiv {
        // No special tracking needed - closed by fence marker
    },
    List {
        marker: ListMarker,
        base_indent_cols: usize,
    },
    ListItem {
        content_col: usize,
    },
    DefinitionList {
        // Definition lists don't need special tracking
    },
    DefinitionItem {
        // Track if we're in a term or definition
        in_definition: bool,
    },
    Definition {
        content_col: usize,
    },
    Paragraph {
        content_col: usize,
    },
}

pub(crate) struct ContainerStack {
    pub(crate) stack: Vec<Container>,
}

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

    #[allow(dead_code)]
    pub(crate) fn get(&self, idx: usize) -> Option<&Container> {
        self.stack.get(idx)
    }

    pub(crate) fn push(&mut self, c: Container) {
        self.stack.push(c);
    }

    /// Close containers from the top down until `keep` remain.
    pub(crate) fn close_to(&mut self, keep: usize, builder: &mut GreenNodeBuilder<'static>) {
        while self.stack.len() > keep {
            self.stack.pop();
            builder.finish_node();
        }
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
                cols += 4 - (cols % 4);
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
                col += 4 - (col % 4);
                idx = i + 1;
            }
            _ => break,
        }
    }
    idx
}

//! Shared, lazy lookahead for Jupyter math in headings and pipe tables.

use std::sync::Arc;

use crate::parser::utils::helpers::is_blank_line;

use super::container_prefix::{StripOp, StrippedLines};

#[derive(Default)]
pub(crate) struct LookaheadCache {
    entries: Vec<Arc<Lookahead>>,
}

pub(crate) struct Lookahead {
    text: String,
    offsets: Vec<usize>,
    start: usize,
    ops: Vec<StripOp>,
    lazy_blockquote_gobble: bool,
    // Preserve the different blank-line rules of headings and pipe tables.
    unicode_blank: bool,
}

impl LookaheadCache {
    pub(crate) fn get(
        &mut self,
        lines: &StrippedLines<'_, '_>,
        start: usize,
        first: &str,
        unicode_blank: bool,
    ) -> Arc<Lookahead> {
        // Dispatch advances monotonically. Drop expired views so lookup and
        // retained text do not grow with the number of blocks in a document.
        self.entries
            .retain(|entry| entry.start + entry.offsets.len() - 1 > start);
        if let Some(entry) = self
            .entries
            .iter()
            .find(|entry| entry.contains(lines, start, first, unicode_blank))
        {
            return Arc::clone(entry);
        }
        let entry = Arc::new(Lookahead::new(lines, start, first, unicode_blank));
        self.entries.push(Arc::clone(&entry));
        entry
    }
}

impl Lookahead {
    pub(crate) fn new(
        lines: &StrippedLines<'_, '_>,
        start: usize,
        first: &str,
        unicode_blank: bool,
    ) -> Self {
        let mut text = first.to_owned();
        let mut offsets = vec![0, text.len()];
        for i in start + 1..lines.raw().len() {
            let tail = lines.peek_prefix_at(i);
            if is_blank_line(tail) || (unicode_blank && tail.trim().is_empty()) {
                break;
            }
            text.push_str(tail);
            offsets.push(text.len());
        }
        Self {
            text,
            offsets,
            start,
            ops: lines.prefix().ops().to_vec(),
            lazy_blockquote_gobble: lines.prefix().lazy_blockquote_gobble,
            unicode_blank,
        }
    }

    fn contains(
        &self,
        lines: &StrippedLines<'_, '_>,
        start: usize,
        first: &str,
        unicode_blank: bool,
    ) -> bool {
        if self.ops != lines.prefix().ops()
            || self.lazy_blockquote_gobble != lines.prefix().lazy_blockquote_gobble
            || self.unicode_blank != unicode_blank
            || start < self.start
        {
            return false;
        }
        let line = start - self.start;
        self.offsets
            .get(line..line + 2)
            .is_some_and(|range| &self.text[range[0]..range[1]] == first)
    }

    pub(crate) fn text_at(&self, start: usize) -> &str {
        &self.text[self.offsets[start - self.start]..]
    }
}

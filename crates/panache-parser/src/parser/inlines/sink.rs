//! Output sink abstraction for inline emission.
//!
//! The inline parser ([`super::core`]) emits its CST through exactly three
//! operations: emit a leaf token, open a node, close a node. Abstracting those
//! behind [`InlineSink`] lets the common path write straight into a
//! [`GreenNodeBuilder`] (zero-cost, monomorphized) while a blockquote paragraph
//! can swap in [`MarkerInjectingSink`], which splices `BLOCK_QUOTE_MARKER`
//! tokens into the stream at recorded byte offsets during the *same* pass —
//! no temporary tree built and replayed.
//!
//! The marker-injection logic mirrors the lossless reconstruction rules:
//! a leaf token is split when a marker falls in its interior, and a marker
//! whose offset coincides with a node boundary is emitted *outside* the node
//! (before `start_node`) so it never nests inside e.g. an `EMPHASIS_MARKER`.

use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;

/// The three CST-building operations the inline emitter relies on.
///
/// Implemented for [`GreenNodeBuilder`] as a direct passthrough (the hot
/// path) and for [`MarkerInjectingSink`] for blockquote paragraphs.
pub trait InlineSink {
    fn token(&mut self, kind: rowan::SyntaxKind, text: &str);
    fn start_node(&mut self, kind: rowan::SyntaxKind);
    fn finish_node(&mut self);
}

impl InlineSink for GreenNodeBuilder<'_> {
    #[inline]
    fn token(&mut self, kind: rowan::SyntaxKind, text: &str) {
        GreenNodeBuilder::token(self, kind, text);
    }

    #[inline]
    fn start_node(&mut self, kind: rowan::SyntaxKind) {
        GreenNodeBuilder::start_node(self, kind);
    }

    #[inline]
    fn finish_node(&mut self) {
        GreenNodeBuilder::finish_node(self);
    }
}

/// An [`InlineSink`] that forwards into a real [`GreenNodeBuilder`] while
/// splicing blockquote markers at recorded byte offsets.
///
/// `marker_positions` is a sorted list of `(byte_offset, leading_spaces,
/// has_trailing_space)` tuples, where `byte_offset` is relative to the start of
/// the text fed to the inline parser. `offset` tracks how many bytes of that
/// text have been emitted so far; it advances *only* on [`token`](Self::token),
/// since node boundaries carry zero bytes.
pub(crate) struct MarkerInjectingSink<'a, 'b> {
    inner: &'a mut GreenNodeBuilder<'static>,
    marker_positions: &'b [(usize, usize, bool)],
    /// Index of the next marker to emit.
    idx: usize,
    /// Bytes of source text emitted so far.
    offset: usize,
}

impl<'a, 'b> MarkerInjectingSink<'a, 'b> {
    pub(crate) fn new(
        inner: &'a mut GreenNodeBuilder<'static>,
        marker_positions: &'b [(usize, usize, bool)],
    ) -> Self {
        Self {
            inner,
            marker_positions,
            idx: 0,
            offset: 0,
        }
    }

    /// Emit any markers whose offset equals the current byte position.
    fn emit_markers_at_current(&mut self) {
        while let Some(&(byte_offset, leading_spaces, has_trailing_space)) =
            self.marker_positions.get(self.idx)
            && byte_offset == self.offset
        {
            if leading_spaces > 0 {
                self.inner
                    .token(SyntaxKind::WHITESPACE.into(), &" ".repeat(leading_spaces));
            }
            self.inner.token(SyntaxKind::BLOCK_QUOTE_MARKER.into(), ">");
            if has_trailing_space {
                self.inner.token(SyntaxKind::WHITESPACE.into(), " ");
            }
            self.idx += 1;
        }
    }

    /// Flush any markers at or past the end of the emitted text. Must be called
    /// once after the inline parser finishes, to place trailing markers.
    pub(crate) fn finish(mut self) {
        self.emit_markers_at_current();
    }
}

impl InlineSink for MarkerInjectingSink<'_, '_> {
    fn token(&mut self, kind: rowan::SyntaxKind, text: &str) {
        let mut start = 0;
        while start < text.len() {
            // Markers at the current offset must be emitted before any bytes.
            self.emit_markers_at_current();

            let remaining = text.len() - start;
            let next_marker_offset = self
                .marker_positions
                .get(self.idx)
                .map(|(byte_offset, _, _)| *byte_offset);

            // If a marker falls strictly inside this token, split it there.
            if let Some(next) = next_marker_offset
                && next > self.offset
                && next < self.offset + remaining
            {
                let split_len = next - self.offset;
                let end = start + split_len;
                if end > start {
                    self.inner.token(kind, &text[start..end]);
                    self.offset += split_len;
                    start = end;
                    continue;
                }
            }

            self.inner.token(kind, &text[start..]);
            self.offset += remaining;
            break;
        }
    }

    fn start_node(&mut self, kind: rowan::SyntaxKind) {
        // Emit any markers at the current offset *outside* this node — otherwise
        // they nest inside (e.g. a BLOCK_QUOTE_MARKER inside an EMPHASIS_MARKER),
        // which breaks lossless reconstruction during reformatting.
        self.emit_markers_at_current();
        self.inner.start_node(kind);
    }

    fn finish_node(&mut self) {
        self.inner.finish_node();
    }
}

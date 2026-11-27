//! Output sink abstraction for inline emission.
//!
//! The inline parser ([`super::core`]) emits its CST through exactly three
//! operations: emit a leaf token, open a node, close a node. Abstracting those
//! behind [`InlineSink`] lets the common path write straight into a
//! [`GreenNodeBuilder`] (zero-cost, monomorphized) while a blockquote paragraph
//! can swap in [`MarkerInjectingSink`], which splices `LINE_PREFIX`
//! tokens into the stream at recorded byte offsets during the *same* pass —
//! no temporary tree built and replayed.
//!
//! The marker-injection logic mirrors the lossless reconstruction rules:
//! a leaf token is split when a marker falls in its interior, and a marker
//! whose offset coincides with a node boundary is emitted *outside* the node
//! by default. List-buffer grafting places the marker at the first token.

use crate::parser::utils::helpers::space_run;
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

/// Container bytes that were stripped off a continuation line before inline
/// parsing and must be spliced back into the emitted stream.
///
/// Both variants exist so the inline parser measures interior whitespace from
/// the *content* column rather than from column 0 — otherwise a construct that
/// preserves interior whitespace (a code span, say) bakes the container's
/// leading columns into its content.
#[derive(Debug, Clone, Copy)]
pub(crate) enum InjectedMarker<'a> {
    /// A blockquote's `>` plus the whitespace around it.
    BlockQuote {
        leading_spaces: usize,
        has_trailing_space: bool,
    },
    /// A list item's continuation indent, up to its content column.
    Indent(&'a str),
}

/// An [`InlineSink`] that forwards into a real [`GreenNodeBuilder`] while
/// splicing container markers back in at recorded byte offsets.
///
/// `marker_positions` is a sorted list of `(byte_offset, marker)` pairs, where
/// `byte_offset` is relative to the start of the text fed to the inline parser.
/// `offset` tracks how many bytes of that text have been emitted so far; it
/// advances *only* on [`token`](Self::token), since node boundaries carry zero
/// bytes.
pub(crate) struct MarkerInjectingSink<'a, 'b> {
    inner: &'a mut GreenNodeBuilder<'static>,
    marker_positions: &'b [(usize, InjectedMarker<'b>)],
    /// Index of the next marker to emit.
    idx: usize,
    /// Bytes of source text emitted so far.
    offset: usize,
    markers_before_node: bool,
}

impl<'a, 'b> MarkerInjectingSink<'a, 'b> {
    pub(crate) fn new(
        inner: &'a mut GreenNodeBuilder<'static>,
        marker_positions: &'b [(usize, InjectedMarker<'b>)],
    ) -> Self {
        Self {
            inner,
            marker_positions,
            idx: 0,
            offset: 0,
            markers_before_node: true,
        }
    }

    /// Match the list buffer's grafting order by inserting each prefix before
    /// the first source token on its line, inside any nodes already opened.
    pub(crate) fn with_markers_at_tokens(mut self) -> Self {
        self.markers_before_node = false;
        self
    }

    fn emit_markers_at_current(&mut self) {
        while let Some(&(byte_offset, marker)) = self.marker_positions.get(self.idx)
            && byte_offset == self.offset
        {
            match marker {
                InjectedMarker::BlockQuote {
                    leading_spaces,
                    has_trailing_space,
                } => {
                    if leading_spaces > 0 {
                        self.inner
                            .token(SyntaxKind::LINE_PREFIX.into(), &space_run(leading_spaces));
                    }
                    self.inner.token(SyntaxKind::LINE_PREFIX.into(), ">");
                    if has_trailing_space {
                        self.inner.token(SyntaxKind::LINE_PREFIX.into(), " ");
                    }
                }
                InjectedMarker::Indent(indent) => {
                    self.inner.token(SyntaxKind::LINE_PREFIX.into(), indent);
                }
            }
            self.idx += 1;
        }
    }

    /// Keep a raw block's leading container prefixes inside its line wrapper.
    pub(crate) fn start_node_with_leading_markers(&mut self, kind: rowan::SyntaxKind) {
        self.inner.start_node(kind);
        self.emit_markers_at_current();
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
            self.emit_markers_at_current();

            let remaining = text.len() - start;
            let next_marker_offset = self
                .marker_positions
                .get(self.idx)
                .map(|(byte_offset, _)| *byte_offset);

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
        if self.markers_before_node {
            self.emit_markers_at_current();
        }
        self.inner.start_node(kind);
    }

    fn finish_node(&mut self) {
        self.inner.finish_node();
    }
}

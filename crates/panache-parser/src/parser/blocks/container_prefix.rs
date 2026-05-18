//! Outer-container prefix vocabulary shared by block parsers that need
//! multi-line lookahead and per-line graft re-injection.
//!
//! [`ContainerPrefix`] captures the bytes the dispatcher / upstream
//! container code has already accounted for on each line — list-item
//! indent or marker, then blockquote markers. Block-level helpers that
//! walk raw `lines[..]` (e.g. `pandoc_html_open_tag_closes` and the HTML
//! block body-lift family) call [`ContainerPrefix::strip`] to skip past
//! those bytes before scanning.
//!
//! [`ContainerPrefixState`] is the graft-time re-injection counterpart.
//! When body content is reparsed from prefix-stripped text, the captured
//! per-line prefix bytes are re-emitted as kind-tagged tokens at line
//! starts so the resulting CST stays byte-equal to source. Folds the
//! older `BqPrefixState` (`html_blocks.rs`) and `LinePrefixState`
//! (`utils/list_item_buffer.rs`) — bq + list-indent on the same line
//! both round-trip cleanly under one structure.
//!
//! Tokenization preserved across the migration:
//!
//! - List-indent is emitted as a *single* `WHITESPACE` token (matching
//!   the legacy `LinePrefixState` behavior).
//! - Blockquote prefix is emitted byte-by-byte — `>` as
//!   `BLOCK_QUOTE_MARKER`, anything else as a 1-byte `WHITESPACE`
//!   (matching the legacy `BqPrefixState` byte-walker).

use rowan::GreenNodeBuilder;

use crate::syntax::SyntaxKind;

use super::super::block_dispatcher::BlockContext;
use super::blockquotes::strip_n_blockquote_markers;

/// Outer-container prefix on every line at the dispatcher level.
///
/// `list_content_col` is the column at which list-item content begins
/// (0 when not inside a list). On the list-marker line this consumes
/// the marker bytes; on continuation lines it consumes leading indent
/// (spaces or tabs). `bq_depth` is the blockquote nesting (0 when not
/// inside a blockquote).
#[derive(Copy, Clone, Default, Debug)]
pub(crate) struct ContainerPrefix {
    pub list_content_col: usize,
    pub bq_depth: usize,
    /// True iff the line at dispatch position (`lines[start_pos]`) is
    /// the LIST-MARKER line — i.e. the LIST_MARKER + WHITESPACE tokens
    /// for the first `list_content_col` columns have just been emitted
    /// upstream and must be skipped by the helper. False (default) when
    /// the dispatch fires on a continuation line: those leading-indent
    /// bytes are NOT upstream-emitted and must be preserved inside the
    /// block's content for losslessness.
    ///
    /// Affects only the line-0 strip semantics in emission helpers.
    /// Lookahead helpers (`pandoc_html_open_tag_closes`,
    /// `find_multiline_open_end`) always strip list_content_col on all
    /// lines — they're pure byte scans, not emission.
    pub list_marker_consumed_on_line_0: bool,
}

impl ContainerPrefix {
    /// Build from a `BlockContext`. List-content column is pulled from
    /// `list_indent_info`; bq depth from the ctx; the line-0 marker
    /// flag from `list_marker_consumed_on_line_0`.
    pub fn from_ctx(ctx: &BlockContext) -> Self {
        Self {
            list_content_col: ctx
                .list_indent_info
                .as_ref()
                .map(|i| i.content_col)
                .unwrap_or(0),
            bq_depth: ctx.blockquote_depth,
            list_marker_consumed_on_line_0: ctx.list_marker_consumed_on_line_0,
        }
    }

    /// Bq-only convenience for callers that don't have a `BlockContext`.
    #[allow(dead_code)]
    pub fn bq_only(bq_depth: usize) -> Self {
        Self {
            list_content_col: 0,
            bq_depth,
            list_marker_consumed_on_line_0: false,
        }
    }

    /// Strip list cols then bq markers, returning the inner content.
    /// Unconditional list strip — use this for multi-line lookahead and
    /// for continuation-line emission inside a known bq-with-list
    /// context. For line-0 emission, prefer
    /// [`Self::strip_line_0_for_emission`] which respects the
    /// upstream-emitted contract.
    pub fn strip<'a>(&self, line: &'a str) -> &'a str {
        let after_list = advance_columns(line, self.list_content_col);
        strip_n_blockquote_markers(after_list, self.bq_depth)
    }

    /// Strip semantics for the dispatch line (line 0) of an emission
    /// helper. Strips list_content_col bytes only when
    /// `list_marker_consumed_on_line_0` is true (upstream emitted the
    /// LIST_MARKER + WHITESPACE for this line); otherwise the leading
    /// indent stays in the inner content because it was NOT upstream-
    /// emitted (continuation-line dispatch). Bq markers are always
    /// stripped on line 0 (always upstream-emitted by the BLOCK_QUOTE
    /// container code path).
    pub fn strip_line_0_for_emission<'a>(&self, line: &'a str) -> &'a str {
        let after_list = if self.list_marker_consumed_on_line_0 {
            advance_columns(line, self.list_content_col)
        } else {
            line
        };
        strip_n_blockquote_markers(after_list, self.bq_depth)
    }

    /// Split a line into `(list_indent, bq_prefix, inner)` — the bytes
    /// consumed by the list-col advance, the bytes consumed by the bq
    /// marker strip, and the remaining inner content. Used by graft
    /// helpers that need to capture the consumed prefix bytes for
    /// re-injection.
    #[allow(dead_code)]
    pub fn split<'a>(&self, line: &'a str) -> (&'a str, &'a str, &'a str) {
        let after_list = advance_columns(line, self.list_content_col);
        let list_len = line.len() - after_list.len();
        let inner = strip_n_blockquote_markers(after_list, self.bq_depth);
        let bq_len = after_list.len() - inner.len();
        (&line[..list_len], &line[list_len..list_len + bq_len], inner)
    }
}

/// Advance past `target` columns of `line`. Tabs round up to the next
/// 4-column stop; tab that would overshoot the target is left intact
/// (mirrors `strip_list_item_indent`'s tab handling). Newlines / CR
/// short-circuit to the empty string — the line ended before the target
/// was reached.
fn advance_columns(line: &str, target: usize) -> &str {
    if target == 0 {
        return line;
    }
    let bytes = line.as_bytes();
    let mut col = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        if col >= target {
            return &line[i..];
        }
        match bytes[i] {
            b'\n' | b'\r' => return "",
            b'\t' => {
                let next = (col / 4 + 1) * 4;
                if next > target {
                    return &line[i..];
                }
                col = next;
                i += 1;
            }
            _ => {
                col += 1;
                i += 1;
            }
        }
    }
    ""
}

/// Per-line container-prefix re-injection state used by graft helpers
/// when content is reparsed from prefix-stripped text. Each entry
/// captures the prefix bytes that were stripped from one source line of
/// the body before the recursive parse; during graft, those bytes are
/// re-emitted as kind-tagged tokens at the start of each grafted line
/// so the result CST stays byte-equal to the source.
///
/// Folds the predecessor `BqPrefixState` (`html_blocks.rs`) and
/// `LinePrefixState` (`utils/list_item_buffer.rs`) — bq + list-indent
/// on the same line both round-trip cleanly under one structure. The
/// emitted tokenization is preserved: list-indent goes out as a single
/// `WHITESPACE` token (legacy `LinePrefixState` behavior), bq prefix
/// goes out byte-by-byte (legacy `BqPrefixState` byte-walker).
pub(crate) struct ContainerPrefixState {
    pub prefixes: Vec<ContainerPrefixLine>,
    pub line_idx: usize,
    pub at_line_start: bool,
}

impl ContainerPrefixState {
    /// Wrap a per-line prefix vector. Returns `None` when every entry
    /// is empty — callers should pass `&mut None` to graft helpers in
    /// that case to skip re-injection entirely.
    pub fn new(prefixes: Vec<ContainerPrefixLine>) -> Option<Self> {
        if prefixes.iter().all(ContainerPrefixLine::is_empty) {
            None
        } else {
            Some(Self {
                prefixes,
                line_idx: 0,
                at_line_start: true,
            })
        }
    }
}

/// One per-line entry in [`ContainerPrefixState`].
#[derive(Clone, Debug, Default)]
pub(crate) struct ContainerPrefixLine {
    /// List-indent bytes — emitted as a single `WHITESPACE` token at
    /// line start when non-empty.
    pub list_indent: String,
    /// Blockquote prefix bytes (mix of `>` and inter-marker whitespace).
    /// Emitted byte-by-byte after the list-indent token.
    pub bq_prefix: String,
}

impl ContainerPrefixLine {
    pub fn is_empty(&self) -> bool {
        self.list_indent.is_empty() && self.bq_prefix.is_empty()
    }

    pub fn bq_only(bq_prefix: String) -> Self {
        Self {
            list_indent: String::new(),
            bq_prefix,
        }
    }

    pub fn list_only(list_indent: String) -> Self {
        Self {
            list_indent,
            bq_prefix: String::new(),
        }
    }
}

/// Emit a captured per-line container prefix as kind-tagged tokens.
/// List-indent (if any) goes out as one `WHITESPACE`; bq prefix bytes
/// go out byte-by-byte as `BLOCK_QUOTE_MARKER` / `WHITESPACE`.
pub(crate) fn emit_container_prefix_tokens(
    builder: &mut GreenNodeBuilder<'static>,
    line: &ContainerPrefixLine,
) {
    if !line.list_indent.is_empty() {
        builder.token(SyntaxKind::WHITESPACE.into(), &line.list_indent);
    }
    for ch in line.bq_prefix.chars() {
        if ch == '>' {
            builder.token(SyntaxKind::BLOCK_QUOTE_MARKER.into(), ">");
        } else {
            let mut buf = [0u8; 4];
            builder.token(SyntaxKind::WHITESPACE.into(), ch.encode_utf8(&mut buf));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_bq_only_matches_legacy() {
        let p = ContainerPrefix::bq_only(1);
        assert_eq!(p.strip("> foo"), "foo");
        assert_eq!(p.strip(">> foo"), "> foo");
        assert_eq!(p.strip("> "), "");
        assert_eq!(p.strip("plain"), "plain");
    }

    #[test]
    fn strip_list_marker_line() {
        // `- > <div>` with content_col=2: advance past `- `, then strip `>`.
        let p = ContainerPrefix {
            list_content_col: 2,
            bq_depth: 1,
            ..Default::default()
        };
        assert_eq!(p.strip("- > <div>"), "<div>");
    }

    #[test]
    fn strip_list_continuation_line() {
        // `  > hello` with content_col=2: advance past `  `, then strip `>`.
        let p = ContainerPrefix {
            list_content_col: 2,
            bq_depth: 1,
            ..Default::default()
        };
        assert_eq!(p.strip("  > hello"), "hello");
    }

    #[test]
    fn strip_tab_indent_rounds_to_four() {
        let p = ContainerPrefix {
            list_content_col: 4,
            bq_depth: 0,
            ..Default::default()
        };
        assert_eq!(p.strip("\tfoo"), "foo");
    }

    #[test]
    fn strip_short_line_yields_empty() {
        let p = ContainerPrefix {
            list_content_col: 4,
            bq_depth: 0,
            ..Default::default()
        };
        assert_eq!(p.strip(""), "");
        assert_eq!(p.strip("\n"), "");
    }

    #[test]
    fn split_captures_consumed_bytes() {
        let p = ContainerPrefix {
            list_content_col: 2,
            bq_depth: 1,
            ..Default::default()
        };
        let (li, bq, inner) = p.split("  > hello");
        assert_eq!(li, "  ");
        assert_eq!(bq, "> ");
        assert_eq!(inner, "hello");
    }
}

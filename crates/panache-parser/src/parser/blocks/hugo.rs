//! Hugo's opaque syntax must be claimed before Markdown block interruption.

use std::any::Any;

use rowan::GreenNodeBuilder;

use crate::parser::block_dispatcher::{BlockContext, BlockDetectionResult, BlockParser};
use crate::parser::inlines::hugo::{self, HugoIndex, HugoSpan};
use crate::parser::inlines::sink::{InjectedMarker, MarkerInjectingSink};
use crate::syntax::SyntaxKind;

use super::container_prefix::{StripOp, StrippedLines};

#[derive(Default)]
pub(crate) struct LookaheadCache {
    entries: Vec<Lookahead>,
}

struct Lookahead {
    ops: Vec<StripOp>,
    lazy_blockquote_gobble: bool,
    base: usize,
    text: String,
    offsets: Vec<usize>,
    index: HugoIndex,
}

impl Lookahead {
    fn new(lines: &StrippedLines<'_, '_>, first: &str) -> Self {
        let mut text = first.to_owned();
        let mut offsets = vec![0, text.len()];
        for i in 1..lines.remaining() {
            let raw = lines.raw_at(i);
            let tail = lines.peek_prefix_at(lines.pos() + i);
            if !tail.trim().is_empty() && !lines.prefix().resolve(raw).reaches_frame() {
                break;
            }
            text.push_str(tail);
            offsets.push(text.len());
        }
        let index = HugoIndex::new(&text);
        Self {
            ops: lines.prefix().ops().to_vec(),
            lazy_blockquote_gobble: lines.prefix().lazy_blockquote_gobble,
            base: lines.pos(),
            text,
            offsets,
            index,
        }
    }

    fn contains(&self, lines: &StrippedLines<'_, '_>, first: &str) -> bool {
        if self.ops != lines.prefix().ops()
            || self.lazy_blockquote_gobble != lines.prefix().lazy_blockquote_gobble
            || lines.pos() < self.base
        {
            return false;
        }
        let line = lines.pos() - self.base;
        self.offsets
            .get(line..line + 2)
            .is_some_and(|range| &self.text[range[0]..range[1]] == first)
    }
}

#[derive(Debug)]
pub(crate) struct PreparedShortcode {
    pub text: String,
    pub span: HugoSpan,
    pub line_count: usize,
    pub start: usize,
    markers: Vec<(usize, String)>,
}

impl PreparedShortcode {
    fn injected_markers(&self) -> Vec<(usize, InjectedMarker<'_>)> {
        self.markers
            .iter()
            .map(|(offset, prefix)| (*offset, InjectedMarker::Indent(prefix.as_str())))
            .collect()
    }

    pub(crate) fn emit_heading(
        &self,
        builder: &mut GreenNodeBuilder<'static>,
        level: usize,
        config: &crate::options::ParserOptions,
    ) {
        let markers = self.injected_markers();
        let mut sink = MarkerInjectingSink::new(builder, &markers);
        super::headings::emit_atx_heading(&mut sink, &self.text, level, config);
        sink.finish();
    }
}

pub(crate) fn prepare(
    lines: &StrippedLines<'_, '_>,
    multiline_only: bool,
    first_override: Option<&str>,
) -> Option<PreparedShortcode> {
    let first = first_override.unwrap_or_else(|| lines.first_unconditional());
    if !first.contains("{{<") && !first.contains("{{%") {
        return None;
    }
    if let Some(cache) = lines.hugo_lookahead() {
        let mut cache = cache.borrow_mut();
        // Dispatch advances monotonically, so expired container views cannot
        // be reused. Keeping them would make cache lookup grow with item count.
        cache
            .entries
            .retain(|entry| entry.base + entry.offsets.len() - 1 > lines.pos());
        let entry = if let Some(i) = cache
            .entries
            .iter()
            .position(|entry| entry.contains(lines, first))
        {
            i
        } else {
            cache.entries.push(Lookahead::new(lines, first));
            cache.entries.len() - 1
        };
        prepare_from_lookahead(
            lines,
            first,
            first_override.is_none(),
            multiline_only,
            &cache.entries[entry],
        )
    } else {
        prepare_from_lookahead(
            lines,
            first,
            first_override.is_none(),
            multiline_only,
            &Lookahead::new(lines, first),
        )
    }
}

fn prepare_from_lookahead(
    lines: &StrippedLines<'_, '_>,
    first: &str,
    capture_first_prefix: bool,
    multiline_only: bool,
    lookahead: &Lookahead,
) -> Option<PreparedShortcode> {
    let offset = lookahead.offsets[lines.pos() - lookahead.base];
    let text = &lookahead.text[offset..];
    let mut start = 0;
    while start < first.len() {
        if text.as_bytes()[start] == b'`'
            && let Some((len, _, _, _)) =
                crate::parser::inlines::code_spans::try_parse_code_span(&text[start..])
        {
            start += len;
            continue;
        }
        if text.as_bytes()[start] == b'\\' {
            start += 1;
            if start < text.len() {
                start += text[start..].chars().next()?.len_utf8();
            }
            continue;
        }
        if let Some(span) = lookahead.index.scan_at(&lookahead.text, offset + start) {
            let mut end = start + span.end;
            let line_end = |end: usize| text[end..].find('\n').map_or(text.len(), |i| end + i + 1);
            let mut full_end = line_end(end);
            let mut cursor = end;
            // A call can close on the same line that opens another one. Hold
            // every continuation in that physical line before block dispatch.
            while cursor < full_end {
                if text.as_bytes()[cursor] == b'`'
                    && let Some((len, _, _, _)) =
                        crate::parser::inlines::code_spans::try_parse_code_span(&text[cursor..])
                {
                    cursor += len;
                } else if text.as_bytes()[cursor] == b'\\' {
                    cursor += 1;
                    if cursor < text.len() {
                        cursor += text[cursor..].chars().next()?.len_utf8();
                    }
                } else if let Some(next) = lookahead.index.scan_at(&lookahead.text, offset + cursor)
                {
                    cursor += next.end;
                    end = end.max(cursor);
                    // Calls on the same physical line share its already-known end.
                    if end > full_end {
                        full_end = line_end(end);
                    }
                } else {
                    cursor += text[cursor..].chars().next()?.len_utf8();
                }
            }
            let line_count = text[..end].bytes().filter(|&b| b == b'\n').count()
                + usize::from(!text[..end].ends_with('\n'));
            if multiline_only && line_count == 1 {
                start = end;
                continue;
            }
            let mut markers = Vec::new();
            if capture_first_prefix {
                let (_, indent) = lines
                    .prefix()
                    .strip_line_0_with_indent_emit(lines.raw_at(0));
                if let Some(indent) = indent {
                    markers.push((0, indent.to_owned()));
                }
                let raw = lines.dispatch_tail();
                let prefix_len = raw.len() - first.len();
                if prefix_len > 0 {
                    markers.push((0, raw[..prefix_len].to_owned()));
                }
            }
            for i in 1..line_count {
                let raw = lines.raw_at(i);
                let tail = lines.peek_prefix_at(lines.pos() + i);
                let prefix_len = raw.len() - tail.len();
                if prefix_len > 0 {
                    markers.push((
                        lookahead.offsets[lines.pos() - lookahead.base + i] - offset,
                        raw[..prefix_len].to_owned(),
                    ));
                }
            }
            return Some(PreparedShortcode {
                text: text[..full_end].to_owned(),
                span: span.into_owned(),
                line_count,
                start,
                markers,
            });
        }
        start += text[start..].chars().next()?.len_utf8();
    }
    None
}

pub(crate) struct HugoShortcodeParser;

impl BlockParser for HugoShortcodeParser {
    fn detect_prepared(
        &self,
        ctx: &BlockContext,
        lines: &StrippedLines<'_, '_>,
    ) -> Option<(BlockDetectionResult, Option<Box<dyn Any>>)> {
        if !ctx.config.extensions.hugo_shortcodes {
            return None;
        }
        let prepared = prepare(lines, false, None)?;
        if prepared.start > 3
            || !prepared.text[..prepared.start].trim().is_empty()
            || !prepared.text[prepared.start + prepared.span.end..]
                .trim()
                .is_empty()
        {
            return None;
        }
        Some((
            BlockDetectionResult::YesCanInterrupt,
            Some(Box::new(prepared)),
        ))
    }

    fn parse_prepared(
        &self,
        _ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        _lines: &StrippedLines<'_, '_>,
        payload: Option<&dyn Any>,
    ) -> usize {
        let prepared = payload
            .and_then(|p| p.downcast_ref::<PreparedShortcode>())
            .expect("Hugo detection carries its prepared span");
        emit_prepared(builder, prepared);
        prepared.line_count
    }

    fn name(&self) -> &'static str {
        "hugo_shortcode"
    }
}

pub(crate) fn emit_prepared(builder: &mut GreenNodeBuilder<'static>, prepared: &PreparedShortcode) {
    emit_prepared_block(builder, prepared, false);
}

pub(crate) fn emit_prepared_after_marker(
    builder: &mut GreenNodeBuilder<'static>,
    prepared: &PreparedShortcode,
) {
    emit_prepared_block(builder, prepared, true);
}

fn emit_prepared_block(
    builder: &mut GreenNodeBuilder<'static>,
    prepared: &PreparedShortcode,
    leading_as_trivia: bool,
) {
    if !leading_as_trivia {
        builder.start_node(SyntaxKind::HUGO_SHORTCODE_BLOCK.into());
    }
    let markers = prepared.injected_markers();
    let mut sink = MarkerInjectingSink::new(builder, &markers);
    use crate::parser::inlines::sink::InlineSink;
    if prepared.start > 0 {
        sink.token(
            SyntaxKind::WHITESPACE.into(),
            &prepared.text[..prepared.start],
        );
    }
    if leading_as_trivia {
        // The container formatter supplies the separator after its marker.
        sink.start_node(SyntaxKind::HUGO_SHORTCODE_BLOCK.into());
    }
    hugo::emit(&mut sink, &prepared.text[prepared.start..], &prepared.span);
    let suffix = &prepared.text[prepared.start + prepared.span.end..];
    if !suffix.is_empty() {
        sink.token(SyntaxKind::NEWLINE.into(), suffix);
    }
    sink.finish();
    builder.finish_node();
}

pub(crate) struct TomlMetadataParser;

impl BlockParser for TomlMetadataParser {
    fn detect_prepared(
        &self,
        ctx: &BlockContext,
        lines: &StrippedLines<'_, '_>,
    ) -> Option<(BlockDetectionResult, Option<Box<dyn Any>>)> {
        if !ctx.config.extensions.toml_metadata_block
            || !ctx.at_document_start
            || ctx.in_list
            || ctx.blockquote_depth > 0
            || lines
                .first_unconditional()
                .trim_start_matches('\u{feff}')
                .trim()
                != "+++"
        {
            return None;
        }
        let count = (1..lines.remaining())
            .find(|&i| lines.raw_at(i).trim_end() == "+++")
            .map_or(lines.remaining(), |i| i + 1);
        Some((BlockDetectionResult::Yes, Some(Box::new(count))))
    }

    fn parse_prepared(
        &self,
        _ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &StrippedLines<'_, '_>,
        payload: Option<&dyn Any>,
    ) -> usize {
        let count = *payload
            .and_then(|p| p.downcast_ref::<usize>())
            .expect("TOML detection carries its closing position");
        builder.start_node(SyntaxKind::TOML_METADATA.into());
        let (opening, newline) = crate::parser::utils::helpers::strip_newline(lines.raw_at(0));
        builder.token(SyntaxKind::TOML_METADATA_DELIM.into(), opening);
        if !newline.is_empty() {
            builder.token(SyntaxKind::NEWLINE.into(), newline);
        }
        let closed = count > 1 && lines.raw_at(count - 1).trim_end() == "+++";
        builder.start_node(SyntaxKind::TOML_METADATA_CONTENT.into());
        for i in 1..count - usize::from(closed) {
            builder.token(SyntaxKind::TOML_METADATA_TEXT.into(), lines.raw_at(i));
        }
        builder.finish_node();
        if closed {
            let (closing, newline) =
                crate::parser::utils::helpers::strip_newline(lines.raw_at(count - 1));
            builder.token(SyntaxKind::TOML_METADATA_DELIM.into(), closing);
            if !newline.is_empty() {
                builder.token(SyntaxKind::NEWLINE.into(), newline);
            }
        }
        builder.finish_node();
        count
    }

    fn name(&self) -> &'static str {
        "toml_metadata"
    }
}

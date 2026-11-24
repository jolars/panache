//! Colon containers from VitePress 1.x's markdown-it-container plugin.

use rowan::GreenNodeBuilder;

use super::container_prefix::StrippedLines;
use crate::parser::block_dispatcher::{
    BlockContext, BlockDetectionResult, BlockEffect, BlockParser,
};
use crate::parser::utils::{
    attributes::{emit_attribute_node, try_parse_trailing_attributes_allow_after_bracket},
    container_stack::{leading_indent, leading_indent_from},
    helpers::{is_blank_line, strip_newline},
};
use crate::syntax::SyntaxKind;
use std::any::Any;

pub(crate) struct VitePressContainerParser;

pub(crate) struct OpenContainer {
    list_prefix_bytes: usize,
    indent: usize,
    pub fence_len: usize,
    name_start: usize,
    name_end: usize,
}

pub(crate) struct Boundary {
    pub line: usize,
    pub explicit_close: bool,
    pub max_body_fence_len: usize,
}

/// Whether a line opens a VitePress colon container in its current frame.
pub fn is_vitepress_container_opener(line: &str) -> bool {
    OpenContainer::parse(line).is_some()
}

/// Length of a VitePress closing fence in its current frame, or zero.
pub fn vitepress_container_closing_fence_len(line: &str) -> usize {
    closing_fence_len(line, "")
}

impl OpenContainer {
    pub fn parse(line: &str) -> Option<Self> {
        Self::parse_in_frame(line, "")
    }

    pub(crate) fn parse_in_frame(line: &str, prefix: &str) -> Option<Self> {
        let (columns, indent) = frame_indent(line, prefix);
        if columns >= 4 {
            return None;
        }

        let fence_len = colon_count(&line[indent..]);
        if fence_len < 3 {
            return None;
        }

        let rest = &line[indent + fence_len..];
        let name_and_title = rest.trim_start();
        // markdown-it-container 4.0 validates the first space-separated word.
        let name = name_and_title.trim_end().split(' ').next()?;
        if !matches!(
            name,
            "info" | "tip" | "warning" | "danger" | "details" | "raw" | "v-pre" | "code-group"
        ) {
            return None;
        }

        let name_start = line.len() - name_and_title.len();
        Some(Self {
            list_prefix_bytes: 0,
            indent,
            fence_len,
            name_start,
            name_end: name_start + name.len(),
        })
    }

    pub fn emit(
        &self,
        builder: &mut GreenNodeBuilder<'static>,
        line: &str,
        flavor: crate::options::Flavor,
    ) {
        let (prefix, line) = line.split_at(self.list_prefix_bytes);
        let (content, newline) = strip_newline(line);
        let fence_end = self.indent + self.fence_len;
        builder.start_node(SyntaxKind::VITEPRESS_CONTAINER.into());
        builder.start_node(SyntaxKind::VITEPRESS_CONTAINER_OPEN.into());
        token(builder, SyntaxKind::LINE_PREFIX, prefix);
        token(builder, SyntaxKind::WHITESPACE, &content[..self.indent]);
        token(
            builder,
            SyntaxKind::VITEPRESS_CONTAINER_FENCE,
            &content[self.indent..fence_end],
        );
        token(
            builder,
            SyntaxKind::WHITESPACE,
            &content[fence_end..self.name_start],
        );
        token(
            builder,
            SyntaxKind::VITEPRESS_CONTAINER_TYPE,
            &content[self.name_start..self.name_end],
        );
        let rest = &content[self.name_end..];
        let title = rest.trim_start();
        let leading = rest.len() - title.len();
        let trimmed_title = title.trim_end();
        let attributes = (flavor == crate::options::Flavor::VitePress
            && matches!(
                &content[self.name_start..self.name_end],
                "tip" | "info" | "warning" | "danger" | "details"
            ))
        .then(|| try_parse_trailing_attributes_allow_after_bracket(trimmed_title, true))
        .flatten();
        token(builder, SyntaxKind::WHITESPACE, &rest[..leading]);
        if let Some((_, before, start)) = attributes {
            token(builder, SyntaxKind::VITEPRESS_CONTAINER_TITLE, before);
            token(
                builder,
                SyntaxKind::WHITESPACE,
                &trimmed_title[before.len()..start],
            );
            emit_attribute_node(builder, &trimmed_title[start..]);
        } else {
            token(
                builder,
                SyntaxKind::VITEPRESS_CONTAINER_TITLE,
                trimmed_title,
            );
        }
        token(
            builder,
            SyntaxKind::WHITESPACE,
            &title[trimmed_title.len()..],
        );
        token(builder, SyntaxKind::NEWLINE, newline);
        builder.finish_node();
    }

    /// The plugin establishes the body range before it tokenizes its contents.
    /// An outer fence therefore takes precedence over any block in its body.
    pub fn boundary(&self, lines: &StrippedLines<'_, '_>) -> Boundary {
        let prefix = lines.prefix();
        let mut boundary = Boundary {
            line: lines.raw().len(),
            explicit_close: false,
            max_body_fence_len: 0,
        };
        for index in lines.pos() + 1..lines.raw().len() {
            let raw = lines.raw()[index];
            let tail = lines.peek_prefix_at(index);
            let consumed = &raw[..raw.len() - tail.len()];
            if consumed.bytes().filter(|&byte| byte == b'>').count() < prefix.bq_depth()
                || (!is_blank_line(tail) && !prefix.resolve(raw).reaches_frame())
            {
                boundary.line = index;
                break;
            }
            let run = closing_fence_len(tail, consumed);
            if run >= self.fence_len {
                boundary.line = index;
                boundary.explicit_close = true;
                break;
            }
            boundary.max_body_fence_len = boundary.max_body_fence_len.max(run);
        }
        boundary
    }
}

impl BlockParser for VitePressContainerParser {
    fn effect(&self) -> BlockEffect {
        BlockEffect::OpenVitePressContainer
    }

    fn detect_prepared(
        &self,
        ctx: &BlockContext,
        lines: &StrippedLines<'_, '_>,
    ) -> Option<(BlockDetectionResult, Option<Box<dyn Any>>)> {
        if !ctx.config.extensions.vitepress_containers {
            return None;
        }
        let line = lines.first_unconditional();
        let raw = lines.raw_at(0);
        let mut open = OpenContainer::parse_in_frame(line, &raw[..raw.len() - line.len()])?;
        open.list_prefix_bytes = lines.first().strip_suffix(line)?.len();
        Some((BlockDetectionResult::YesCanInterrupt, Some(Box::new(open))))
    }

    fn parse_prepared(
        &self,
        ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &StrippedLines<'_, '_>,
        payload: Option<&dyn Any>,
    ) -> usize {
        let open = payload
            .and_then(|payload| payload.downcast_ref::<OpenContainer>())
            .expect("VitePress container opener was prepared");
        open.emit(builder, lines.first(), ctx.config.flavor);
        1
    }

    fn name(&self) -> &'static str {
        "vitepress_container"
    }
}

pub(crate) fn emit_close(builder: &mut GreenNodeBuilder<'static>, line: &str) {
    let (content, newline) = strip_newline(line);
    let (_, indent) = leading_indent(content);
    let fence_end = indent + colon_count(&content[indent..]);
    token(builder, SyntaxKind::WHITESPACE, &content[..indent]);
    token(
        builder,
        SyntaxKind::VITEPRESS_CONTAINER_FENCE,
        &content[indent..fence_end],
    );
    token(builder, SyntaxKind::WHITESPACE, &content[fence_end..]);
    token(builder, SyntaxKind::NEWLINE, newline);
}

fn closing_fence_len(line: &str, prefix: &str) -> usize {
    let (content, _) = strip_newline(line);
    let (columns, indent) = frame_indent(content, prefix);
    let rest = &content[indent..];
    let count = colon_count(rest);
    if columns < 4 && count >= 3 && rest[count..].trim_matches([' ', '\t']).is_empty() {
        count
    } else {
        0
    }
}

pub(in crate::parser) fn frame_indent(line: &str, prefix: &str) -> (usize, usize) {
    let column = prefix.bytes().fold(0, |column, byte| {
        if byte == b'\t' {
            column + 4 - column % 4
        } else {
            column + 1
        }
    });
    // A tab immediately after '>' supplies the optional single column of
    // quote padding. The remaining width still belongs to the fence line.
    if prefix.ends_with('>') && line.starts_with('\t') {
        if column % 4 == 3 {
            let (columns, bytes) = leading_indent_from(&line[1..], column + 1);
            (columns, bytes + 1)
        } else {
            leading_indent_from(line, column + 1)
        }
    } else {
        leading_indent_from(line, column)
    }
}

fn colon_count(line: &str) -> usize {
    line.bytes().take_while(|&byte| byte == b':').count()
}

fn token(builder: &mut GreenNodeBuilder<'static>, kind: SyntaxKind, text: &str) {
    if !text.is_empty() {
        builder.token(kind.into(), text);
    }
}

/// A VitePress code import is one physical line, including its path, options,
/// and tab label. Those fields are interpreted by VitePress.
pub(crate) struct VitePressSnippetParser;

pub(crate) fn snippet_marker_offset(line: &str, prefix: &str) -> Option<usize> {
    let (columns, offset) = frame_indent(line, prefix);
    (columns < 4 && line[offset..].starts_with("<<<")).then_some(offset)
}

pub(crate) fn emit_snippet(
    builder: &mut GreenNodeBuilder<'static>,
    line: &str,
    prefix_bytes: usize,
    marker_offset: usize,
) {
    let (prefix, line) = line.split_at(prefix_bytes);
    let (content, newline) = strip_newline(line);
    let (source, trailing) = content[marker_offset + 3..].split_at(
        content[marker_offset + 3..]
            .trim_end_matches([' ', '\t'])
            .len(),
    );
    let payload = source.trim_start_matches([' ', '\t']);
    builder.start_node(SyntaxKind::VITEPRESS_SNIPPET.into());
    token(builder, SyntaxKind::LINE_PREFIX, prefix);
    token(builder, SyntaxKind::WHITESPACE, &content[..marker_offset]);
    token(
        builder,
        SyntaxKind::VITEPRESS_SNIPPET_MARKER,
        &content[marker_offset..marker_offset + 3],
    );
    token(
        builder,
        SyntaxKind::WHITESPACE,
        &source[..source.len() - payload.len()],
    );
    token(builder, SyntaxKind::VITEPRESS_SNIPPET_SOURCE, payload);
    token(builder, SyntaxKind::WHITESPACE, trailing);
    token(builder, SyntaxKind::NEWLINE, newline);
    builder.finish_node();
}

impl BlockParser for VitePressSnippetParser {
    fn detect_prepared(
        &self,
        ctx: &BlockContext,
        lines: &StrippedLines<'_, '_>,
    ) -> Option<(BlockDetectionResult, Option<Box<dyn Any>>)> {
        if ctx.config.flavor != crate::options::Flavor::VitePress
            || ctx.paragraph_open
            || ctx.list_item_content_open
        {
            return None;
        }
        let line = lines.first_unconditional();
        let raw = lines.raw_at(0);
        let prefix_bytes = lines.first().strip_suffix(line)?.len();
        let marker = snippet_marker_offset(line, &raw[..raw.len() - line.len()])?;
        Some((
            BlockDetectionResult::Yes,
            Some(Box::new((prefix_bytes, marker))),
        ))
    }

    fn parse_prepared(
        &self,
        _ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &StrippedLines<'_, '_>,
        payload: Option<&dyn Any>,
    ) -> usize {
        let &(prefix_bytes, marker) = payload
            .and_then(|payload| payload.downcast_ref::<(usize, usize)>())
            .expect("VitePress snippet was prepared");
        emit_snippet(builder, lines.first(), prefix_bytes, marker);
        1
    }

    fn name(&self) -> &'static str {
        "vitepress_snippet"
    }
}

pub(crate) struct VitePressTocParser;

pub(crate) fn toc_marker_offset(line: &str, prefix: &str) -> Option<usize> {
    let (columns, offset) = frame_indent(line, prefix);
    let marker = strip_newline(&line[offset..]).0.split(' ').next()?;
    (columns < 4 && marker.eq_ignore_ascii_case("[[toc]]")).then_some(offset)
}

pub(crate) fn emit_toc(
    builder: &mut GreenNodeBuilder<'static>,
    line: &str,
    prefix_bytes: usize,
    marker_offset: usize,
) {
    let (prefix, line) = line.split_at(prefix_bytes);
    let (content, newline) = strip_newline(line);
    let marker_end = marker_offset + "[[toc]]".len();
    builder.start_node(SyntaxKind::VITEPRESS_TOC.into());
    token(builder, SyntaxKind::LINE_PREFIX, prefix);
    token(builder, SyntaxKind::WHITESPACE, &content[..marker_offset]);
    token(
        builder,
        SyntaxKind::VITEPRESS_TOC_MARKER,
        &content[marker_offset..marker_end],
    );
    token(
        builder,
        SyntaxKind::VITEPRESS_TOC_SUFFIX,
        &content[marker_end..],
    );
    token(builder, SyntaxKind::NEWLINE, newline);
    builder.finish_node();
}

impl BlockParser for VitePressTocParser {
    fn detect_prepared(
        &self,
        ctx: &BlockContext,
        lines: &StrippedLines<'_, '_>,
    ) -> Option<(BlockDetectionResult, Option<Box<dyn Any>>)> {
        if ctx.config.flavor != crate::options::Flavor::VitePress {
            return None;
        }
        let line = lines.first_unconditional();
        let raw = lines.raw_at(0);
        let prefix_bytes = lines.first().strip_suffix(line)?.len();
        let marker = toc_marker_offset(line, &raw[..raw.len() - line.len()])?;
        Some((
            BlockDetectionResult::YesCanInterrupt,
            Some(Box::new((prefix_bytes, marker))),
        ))
    }

    fn parse_prepared(
        &self,
        _ctx: &BlockContext,
        builder: &mut GreenNodeBuilder<'static>,
        lines: &StrippedLines<'_, '_>,
        payload: Option<&dyn Any>,
    ) -> usize {
        let &(prefix, marker) = payload
            .and_then(|payload| payload.downcast_ref::<(usize, usize)>())
            .expect("VitePress table of contents was prepared");
        emit_toc(builder, lines.first(), prefix, marker);
        1
    }

    fn name(&self) -> &'static str {
        "vitepress_toc"
    }
}

//! Resolve Pandoc HTML block boundaries and preserve the CST for their bodies.

use rowan::GreenNodeBuilder;

use super::super::block_dispatcher::html_block_cannot_interrupt;
use super::super::blocks::container_prefix::ContainerPrefixLine;
pub(super) use super::super::blocks::html_blocks::pandoc_html_block_start_is_valid as valid_start;
use super::super::blocks::tables::LineView;
use super::super::utils::helpers::is_blank_line;
use super::super::utils::list_item_buffer::graft_html_block_lift;
use super::{
    ContainerPrefix, HtmlBlockContext, HtmlBlockType, K, ParserOptions, SoftbreakFusion,
    StrippedLines, SyntaxNode, parse_html_block_with_wrapper, try_parse_html_block_start,
};

pub(super) struct PreparedHtml {
    pub(super) end: usize,
    fragment: rowan::GreenNode,
}

impl PreparedHtml {
    pub(super) fn emit(self, builder: &mut GreenNodeBuilder<'static>) {
        use rowan::WalkEvent;
        let root = SyntaxNode::new_root(self.fragment);
        for event in root.preorder_with_tokens() {
            match event {
                WalkEvent::Enter(rowan::NodeOrToken::Node(node)) if node != root => {
                    builder.start_node(node.kind().into());
                }
                WalkEvent::Enter(rowan::NodeOrToken::Token(token)) => {
                    builder.token(token.kind().into(), token.text());
                }
                WalkEvent::Leave(rowan::NodeOrToken::Node(node)) if node != root => {
                    builder.finish_node();
                }
                _ => {}
            }
        }
    }
}

pub(super) fn detect(content: &str, after_blank: bool) -> Option<HtmlBlockType> {
    let trimmed = content.trim_start_matches(' ');
    if content.len() - trimmed.len() >= 4 || !trimmed.starts_with('<') {
        return None;
    }
    let kind = try_parse_html_block_start(content, false)?;
    if html_block_cannot_interrupt(&kind, content, true)
        && (!after_blank || trimmed.len() != content.len())
    {
        return None;
    }
    Some(kind)
}

// A split tag may contribute siblings, and a lifted body needs its own
// Markdown parse. Retain the prepared children so emission cannot choose a
// different boundary or parse the body a second time.
pub(super) fn prepare(
    window: StrippedLines<'_, '_>,
    kind: HtmlBlockType,
    options: &ParserOptions,
    fusion: SoftbreakFusion,
    first_line_content: Option<&str>,
) -> Option<PreparedHtml> {
    let pos = window.pos();
    let lines = window.raw();
    let prefix = window.prefix();
    if !valid_start(lines, pos, prefix, &kind) {
        return None;
    }
    let wrapper_kind = if matches!(
        &kind,
        HtmlBlockType::BlockTag { tag_name, is_closing: false, .. } if tag_name == "div"
    ) && options.extensions.native_divs
    {
        K::HTML_BLOCK_DIV
    } else {
        K::HTML_BLOCK
    };
    let mut builder = GreenNodeBuilder::new();
    builder.start_node(K::DOCUMENT.into());
    let end = parse_html_block_with_wrapper(
        &mut builder,
        lines,
        pos,
        kind,
        &HtmlBlockContext {
            prefix,
            wrapper_kind,
            config: options,
            fusion,
        },
        first_line_content,
        None,
    );
    builder.finish_node();
    (end > pos).then(|| PreparedHtml {
        end,
        fragment: builder.finish(),
    })
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum BodyHtml {
    ListItem,
    DefinitionStart,
    FootnoteStart,
    Indented,
}

/// A marker-line block is parsed in its body's frame, with the original
/// continuation prefixes restored in the fragment.
pub(super) fn prepare_body(
    window: StrippedLines<'_, '_>,
    first: &str,
    first_indent: &str,
    mode: BodyHtml,
    use_paragraph: bool,
    options: &ParserOptions,
) -> Option<PreparedHtml> {
    let kind = try_parse_html_block_start(first, false)?;
    let raw_close_on_first = matches!(
        kind,
        HtmlBlockType::Comment | HtmlBlockType::ProcessingInstruction
    ) && super::is_closing_marker(first, &kind);
    let mut content = vec![first];
    let mut blank_start = None;
    let mut ended_at_boundary = false;
    let pos = window.pos();
    let prefix = window.prefix();
    let quoted_content_body = mode == BodyHtml::Indented && prefix.bq_depth() > 0;
    let may_fuse =
        mode == BodyHtml::ListItem || (mode == BodyHtml::DefinitionStart && raw_close_on_first);
    let verbatim_definition = mode == BodyHtml::DefinitionStart
        && prefix.bq_depth() == 0
        && matches!(
            kind,
            HtmlBlockType::BlockTag {
                is_verbatim: true,
                ..
            }
        );
    let single_line = !may_fuse && closes_on_first_line(first, &kind);
    if mode == BodyHtml::FootnoteStart && !single_line {
        return None;
    }
    for (index, &line) in window.raw().iter().enumerate().skip(pos + 1) {
        if single_line || (!verbatim_definition && window.ends_container_lines(index)) {
            ended_at_boundary = blank_start.is_none();
            break;
        }
        let required = ContainerPrefix::strip_required_ops(line, prefix.ops());
        if !verbatim_definition
            && required.is_none()
            && (blank_start.is_some() || is_blank_line(line))
        {
            break;
        }
        let inner = required.unwrap_or_else(|| prefix.strip(line));
        content.push(inner);
        if is_blank_line(inner) {
            blank_start.get_or_insert(content.len() - 1);
        } else {
            blank_start = None;
        }
    }
    if let Some(blank_start) = blank_start {
        content.truncate(blank_start);
    }
    let inner = prepare(
        StrippedLines::new(&content, 0, &ContainerPrefix::default()),
        kind,
        options,
        if may_fuse {
            SoftbreakFusion::ToDocEnd
        } else {
            SoftbreakFusion::None
        },
        None,
    )?;
    if mode == BodyHtml::FootnoteStart && inner.end != 1 {
        return None;
    }
    let root = SyntaxNode::new_root(inner.fragment);
    let normalized_source = content[..inner.end].concat();
    if root.text() != normalized_source.as_str() {
        return None;
    }
    if mode == BodyHtml::ListItem
        && root.children().nth(1).is_some()
        && ended_at_boundary
        && content
            .get(inner.end)
            .is_some_and(|line| !is_blank_line(line) && detect(line, false).is_none())
    {
        return None;
    }
    let mut builder = GreenNodeBuilder::new();
    builder.start_node(K::DOCUMENT.into());
    let mut prefixes = Vec::with_capacity(inner.end);
    prefixes.push(ContainerPrefixLine::list_only(first_indent.into()));
    for (&line, inner) in window.raw()[pos + 1..pos + inner.end]
        .iter()
        .zip(&content[1..inner.end])
    {
        let lead = &line[..line.len() - inner.len()];
        let mut captured = ContainerPrefixLine::default();
        if quoted_content_body {
            captured.push_bq(lead);
        } else {
            let through_quote = prefix.strip_through_last_quote(line).unwrap_or(line);
            let (quote, indent) = lead.split_at((line.len() - through_quote.len()).min(lead.len()));
            captured.push_bq(quote);
            captured.push_indent(indent);
        }
        prefixes.push(captured);
    }
    let ok = graft_html_block_lift(
        &mut builder,
        &root,
        rowan::TextSize::of(normalized_source.as_str()),
        prefixes,
        use_paragraph || mode == BodyHtml::FootnoteStart,
        mode == BodyHtml::Indented || (mode == BodyHtml::ListItem && !ended_at_boundary),
    );
    builder.finish_node();
    if !ok {
        return None;
    }
    let end = pos + inner.end;
    let fragment = builder.finish();
    let mut source = String::from(first_indent);
    source.push_str(first);
    for line in &window.raw()[pos + 1..end] {
        source.push_str(line);
    }
    (SyntaxNode::new_root(fragment.clone()).text() == source.as_str())
        .then_some(PreparedHtml { end, fragment })
}

fn closes_on_first_line(first: &str, kind: &HtmlBlockType) -> bool {
    use super::super::blocks::html_blocks::{count_tag_balance, pandoc_html_open_tag_closes};

    match kind {
        HtmlBlockType::BlockTag {
            closes_at_open_tag: true,
            ..
        } => pandoc_html_open_tag_closes(&[first], 0, &ContainerPrefix::default()),
        HtmlBlockType::BlockTag {
            tag_name,
            depth_aware: true,
            ..
        } => {
            let (opens, closes) = count_tag_balance(first, tag_name);
            opens > 0 && opens <= closes
        }
        _ => super::is_closing_marker(first, kind),
    }
}

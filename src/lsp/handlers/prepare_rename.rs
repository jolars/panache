use lsp_types::{PrepareRenameResponse, Range, TextDocumentPositionParams};

use super::super::conversions::{offset_to_position, position_to_offset};
use super::super::helpers;
use crate::lsp::context::get_open_document_context;
use crate::lsp::global_state::StateSnapshot;

pub(crate) fn prepare_rename(
    snap: &StateSnapshot,
    params: TextDocumentPositionParams,
) -> Option<PrepareRenameResponse> {
    let uri = params.text_document.uri;
    let position = params.position;

    let ctx = get_open_document_context(snap, &uri)?;

    let content = ctx.content.clone();
    let parsed_yaml_regions = ctx.parsed_yaml_regions.clone();

    let Some(offset) = position_to_offset(&content, position) else {
        log::debug!(
            "prepare_rename: position_to_offset failed uri={:?} line={} char={}",
            uri,
            position.line,
            position.character
        );
        return None;
    };
    if helpers::is_offset_in_yaml_frontmatter(&parsed_yaml_regions, offset) {
        return None;
    }

    let root = ctx.syntax_root();
    let range = helpers::example_label_range_at_offset(&root, offset)
        .or_else(|| helpers::find_symbol_text_range_at_offset(&root, offset));
    let Some(range) = range else {
        log::debug!(
            "prepare_rename: no symbol range uri={:?} line={} char={} offset={}",
            uri,
            position.line,
            position.character,
            offset
        );
        return None;
    };

    let start_offset: usize = range.start().into();
    let end_offset: usize = range.end().into();
    let Some(placeholder) = content.get(start_offset..end_offset) else {
        log::debug!(
            "prepare_rename: invalid utf8 slice uri={:?} range={}..{}",
            uri,
            start_offset,
            end_offset
        );
        return None;
    };

    let start = offset_to_position(&content, range.start().into());
    let end = offset_to_position(&content, range.end().into());
    Some(PrepareRenameResponse::RangeWithPlaceholder {
        range: Range { start, end },
        placeholder: placeholder.to_string(),
    })
}

use crate::syntax::{SyntaxKind, SyntaxNode};
use rowan::GreenNodeBuilder;

use super::model::{
    BasicYamlEntry, ShadowYamlOptions, ShadowYamlOutcome, ShadowYamlReport, YamlInputKind,
};

/// Parse YAML in shadow mode using prototype groundwork only.
///
/// This API is intentionally read-only and does not replace production YAML
/// parsing. By default it is disabled and reports `SkippedDisabled`.
pub fn parse_shadow(input: &str, options: ShadowYamlOptions) -> ShadowYamlReport {
    let line_count = input.lines().count().max(1);

    if !options.enabled {
        return ShadowYamlReport {
            outcome: ShadowYamlOutcome::SkippedDisabled,
            shadow_reason: "shadow-disabled",
            input_kind: options.input_kind,
            input_len_bytes: input.len(),
            line_count,
            normalized_input: None,
        };
    }

    let normalized = match options.input_kind {
        YamlInputKind::Plain => input.to_owned(),
        YamlInputKind::Hashpipe => normalize_hashpipe_input(input),
    };

    let parsed = parse_basic_mapping_tree(&normalized).is_some();

    ShadowYamlReport {
        outcome: if parsed {
            ShadowYamlOutcome::PrototypeParsed
        } else {
            ShadowYamlOutcome::PrototypeRejected
        },
        shadow_reason: if parsed {
            "prototype-basic-mapping-parsed"
        } else {
            "prototype-basic-mapping-rejected"
        },
        input_kind: options.input_kind,
        input_len_bytes: input.len(),
        line_count,
        normalized_input: Some(normalized),
    }
}

fn normalize_hashpipe_input(input: &str) -> String {
    input
        .lines()
        .map(strip_hashpipe_prefix)
        .collect::<Vec<_>>()
        .join("\n")
}

fn strip_hashpipe_prefix(line: &str) -> &str {
    if let Some(rest) = line.strip_prefix("#|") {
        return rest.strip_prefix(' ').unwrap_or(rest);
    }
    line
}

fn split_line_and_newline(line: &str) -> (&str, &str) {
    if let Some(without_lf) = line.strip_suffix('\n') {
        if let Some(without_crlf) = without_lf.strip_suffix('\r') {
            (without_crlf, "\r\n")
        } else {
            (without_lf, "\n")
        }
    } else {
        (line, "")
    }
}

fn parse_raw_mapping_line(line: &str) -> Option<(&str, &str)> {
    let (raw_key, raw_value) = line.split_once(':')?;
    if raw_key.trim().is_empty() || raw_value.trim().is_empty() {
        return None;
    }
    Some((raw_key, raw_value))
}

fn split_value_and_comment(raw_value: &str) -> (&str, Option<&str>) {
    if let Some(idx) = raw_value.find('#') {
        let (before, after) = raw_value.split_at(idx);
        if !before.trim().is_empty() {
            return (before.trim_end_matches([' ', '\t']), Some(after));
        }
    }
    (raw_value, None)
}

/// Parse one or more `key: value` lines and emit a prototype YAML mapping CST.
///
/// This remains prototype-scoped but models YAML mapping structure with explicit
/// block-map and entry/key/value nodes, plus key/colon/whitespace/value/newline
/// tokens.
pub fn parse_basic_mapping_tree(input: &str) -> Option<SyntaxNode> {
    if input.is_empty() {
        return None;
    }

    let mut builder = GreenNodeBuilder::new();
    builder.start_node(SyntaxKind::DOCUMENT.into());
    builder.start_node(SyntaxKind::YAML_METADATA_CONTENT.into());
    builder.start_node(SyntaxKind::YAML_BLOCK_MAP.into());

    for raw_line in input.split_inclusive('\n') {
        let (line, newline) = split_line_and_newline(raw_line);
        let (raw_key, raw_value) = parse_raw_mapping_line(line)?;
        builder.start_node(SyntaxKind::YAML_BLOCK_MAP_ENTRY.into());

        builder.start_node(SyntaxKind::YAML_BLOCK_MAP_KEY.into());
        builder.token(SyntaxKind::YAML_KEY.into(), raw_key);
        builder.token(SyntaxKind::YAML_COLON.into(), ":");
        builder.finish_node(); // YAML_BLOCK_MAP_KEY

        builder.start_node(SyntaxKind::YAML_BLOCK_MAP_VALUE.into());
        let (value_part, comment_part) = split_value_and_comment(raw_value);
        let leading_ws_len = value_part
            .bytes()
            .take_while(|b| *b == b' ' || *b == b'\t')
            .count();
        if leading_ws_len > 0 {
            builder.token(SyntaxKind::WHITESPACE.into(), &value_part[..leading_ws_len]);
        }
        builder.token(
            SyntaxKind::YAML_SCALAR.into(),
            &value_part[leading_ws_len..],
        );
        if let Some(comment) = comment_part {
            let leading_comment_ws_len = raw_value.len() - comment.len() - value_part.len();
            if leading_comment_ws_len > 0 {
                let start = value_part.len();
                let end = start + leading_comment_ws_len;
                builder.token(SyntaxKind::WHITESPACE.into(), &raw_value[start..end]);
            }
            builder.token(SyntaxKind::YAML_COMMENT.into(), comment);
        }
        builder.finish_node(); // YAML_BLOCK_MAP_VALUE

        if !newline.is_empty() {
            builder.token(SyntaxKind::NEWLINE.into(), newline);
        }
        builder.finish_node(); // YAML_BLOCK_MAP_ENTRY
    }

    builder.finish_node(); // YAML_BLOCK_MAP
    builder.finish_node(); // YAML_METADATA_CONTENT
    builder.finish_node(); // DOCUMENT
    Some(SyntaxNode::new_root(builder.finish()))
}

/// Parse a single-line YAML mapping entry like `title: My Title`.
///
/// This is intentionally minimal groundwork and currently supports exactly one
/// `key: value` line.
pub fn parse_basic_entry(input: &str) -> Option<BasicYamlEntry<'_>> {
    if input.contains('\n') {
        return None;
    }

    let (raw_key, raw_value) = input.split_once(':')?;
    let key = raw_key.trim();
    let value = raw_value.trim();

    if key.is_empty() || value.is_empty() {
        return None;
    }

    Some(BasicYamlEntry { key, value })
}

/// Parse a single-line YAML mapping entry and emit a tiny Rowan CST.
///
/// The current prototype emits:
/// DOCUMENT
///   YAML_METADATA_CONTENT
///     YAML_BLOCK_MAP
///       YAML_BLOCK_MAP_ENTRY
///         YAML_BLOCK_MAP_KEY
///           YAML_KEY(key)
///           YAML_COLON(":")
///         YAML_BLOCK_MAP_VALUE
///           [WHITESPACE(" ")] // when present in the original input
///           YAML_SCALAR(value)
pub fn parse_basic_entry_tree(input: &str) -> Option<SyntaxNode> {
    parse_basic_entry(input)?;
    parse_basic_mapping_tree(input)
}

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

/// Parse one or more `key: value` lines and emit a tiny Rowan CST.
///
/// This remains prototype-scoped but models YAML mapping structure with explicit
/// key/colon/whitespace/value/newline tokens.
pub fn parse_basic_mapping_tree(input: &str) -> Option<SyntaxNode> {
    if input.is_empty() {
        return None;
    }

    let mut builder = GreenNodeBuilder::new();
    builder.start_node(SyntaxKind::DOCUMENT.into());
    builder.start_node(SyntaxKind::YAML_METADATA_CONTENT.into());

    for raw_line in input.split_inclusive('\n') {
        let (line, newline) = split_line_and_newline(raw_line);
        let (raw_key, raw_value) = parse_raw_mapping_line(line)?;

        builder.token(SyntaxKind::YAML_KEY.into(), raw_key);
        builder.token(SyntaxKind::YAML_COLON.into(), ":");

        let leading_ws_len = raw_value
            .bytes()
            .take_while(|b| *b == b' ' || *b == b'\t')
            .count();
        if leading_ws_len > 0 {
            builder.token(SyntaxKind::WHITESPACE.into(), &raw_value[..leading_ws_len]);
        }
        builder.token(SyntaxKind::YAML_SCALAR.into(), &raw_value[leading_ws_len..]);

        if !newline.is_empty() {
            builder.token(SyntaxKind::NEWLINE.into(), newline);
        }
    }

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
///     TEXT(key)
///     TEXT(":")
///     [WHITESPACE(" ")] // when present in the original input
///     TEXT(value)
pub fn parse_basic_entry_tree(input: &str) -> Option<SyntaxNode> {
    parse_basic_entry(input)?;
    parse_basic_mapping_tree(input)
}

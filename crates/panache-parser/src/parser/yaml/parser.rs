use crate::syntax::{SyntaxKind, SyntaxNode};
use rowan::GreenNodeBuilder;

use super::model::{
    ShadowYamlOptions, ShadowYamlOutcome, ShadowYamlReport, YamlInputKind, YamlParseReport,
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

    let parsed = parse_yaml_tree(&normalized).is_some();

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

/// Parse prototype YAML tree structure from input
pub fn parse_yaml_tree(input: &str) -> Option<SyntaxNode> {
    parse_yaml_report(input).tree
}

/// Parse prototype YAML tree structure and include diagnostics on failure.
///
/// Diagnostics flow through the v2-aware
/// [`super::validator::validate_yaml`] pass, which composes per-cluster
/// `check_*` functions covering directive ordering, structural shape
/// (unterminated flow, trailing content, invalid keys, indent
/// anomalies, block-scalar header, etc.), and lex-level checks like
/// `LEX_INVALID_DOUBLE_QUOTED_ESCAPE`.
///
/// The returned tree, when present, comes from the v2 scanner+builder.
pub fn parse_yaml_report(input: &str) -> YamlParseReport {
    if let Some(err) = super::validator::validate_yaml(input) {
        return YamlParseReport {
            tree: None,
            diagnostics: vec![err],
        };
    }

    let v2_stream = super::parser_v2::parse_v2(input);
    let mut builder = GreenNodeBuilder::new();
    builder.start_node(SyntaxKind::DOCUMENT.into());
    builder.start_node(SyntaxKind::YAML_METADATA_CONTENT.into());
    let stream_green = v2_stream.green().into_owned();
    builder.start_node(SyntaxKind::YAML_STREAM.into());
    for child in stream_green.children() {
        match child {
            rowan::NodeOrToken::Node(n) => {
                push_green_node(&mut builder, n);
            }
            rowan::NodeOrToken::Token(t) => {
                builder.token(t.kind(), t.text());
            }
        }
    }
    builder.finish_node(); // YAML_STREAM
    builder.finish_node(); // YAML_METADATA_CONTENT
    builder.finish_node(); // DOCUMENT
    YamlParseReport {
        tree: Some(SyntaxNode::new_root(builder.finish())),
        diagnostics: Vec::new(),
    }
}

fn push_green_node(builder: &mut GreenNodeBuilder<'_>, node: &rowan::GreenNodeData) {
    builder.start_node(node.kind());
    for child in node.children() {
        match child {
            rowan::NodeOrToken::Node(n) => push_green_node(builder, n),
            rowan::NodeOrToken::Token(t) => builder.token(t.kind(), t.text()),
        }
    }
    builder.finish_node();
}

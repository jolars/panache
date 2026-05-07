use crate::syntax::{SyntaxKind, SyntaxNode};
use rowan::GreenNodeBuilder;

use super::lexer::lex_mapping_tokens_with_diagnostic;
use super::model::{
    ShadowYamlOptions, ShadowYamlOutcome, ShadowYamlReport, YamlDiagnostic, YamlInputKind,
    YamlParseReport, YamlToken, YamlTokenSpan, diagnostic_codes,
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

fn diag_at_token(
    token: &YamlTokenSpan<'_>,
    code: &'static str,
    message: &'static str,
) -> YamlDiagnostic {
    YamlDiagnostic {
        code,
        message,
        byte_start: token.byte_start,
        byte_end: token.byte_end,
    }
}

/// Parse prototype YAML tree structure from input
pub fn parse_yaml_tree(input: &str) -> Option<SyntaxNode> {
    parse_yaml_report(input).tree
}

/// Parse prototype YAML tree structure and include diagnostics on failure.
///
/// Diagnostics flow in three phases:
/// 1. The v1 lexer surfaces lex-level diagnostics (e.g.
///    `LEX_INVALID_DOUBLE_QUOTED_ESCAPE`) and produces a token stream
///    that correctly classifies column-0 directive markers — something
///    the v2 scanner cannot yet do because it folds
///    `!foo "bar"\n%TAG ...` into a single plain scalar.
/// 2. Directive-ordering checks (after-content, without-doc-start)
///    run over the v1 tokens.
/// 3. The v2-aware [`super::validator::validate_yaml`] pass surfaces
///    structural diagnostics (unterminated flow, trailing content,
///    invalid keys, indent anomalies, block-scalar header, etc.).
///
/// The returned tree, when present, comes from the v2 scanner+builder.
pub fn parse_yaml_report(input: &str) -> YamlParseReport {
    let tokens = match lex_mapping_tokens_with_diagnostic(input) {
        Ok(tokens) => tokens,
        Err(err) => {
            return YamlParseReport {
                tree: None,
                diagnostics: vec![err],
            };
        }
    };

    let mut seen_content = false;
    for token in &tokens {
        match token.kind {
            YamlToken::Directive if seen_content => {
                return YamlParseReport {
                    tree: None,
                    diagnostics: vec![diag_at_token(
                        token,
                        diagnostic_codes::PARSE_DIRECTIVE_AFTER_CONTENT,
                        "directive requires document end before subsequent directives",
                    )],
                };
            }
            YamlToken::Directive
            | YamlToken::Newline
            | YamlToken::Whitespace
            | YamlToken::Comment => {}
            YamlToken::DocumentEnd => seen_content = false,
            _ => seen_content = true,
        }
    }

    if let Some(directive) = tokens.iter().find(|t| t.kind == YamlToken::Directive)
        && !tokens.iter().any(|t| t.kind == YamlToken::DocumentStart)
    {
        return YamlParseReport {
            tree: None,
            diagnostics: vec![diag_at_token(
                directive,
                diagnostic_codes::PARSE_DIRECTIVE_WITHOUT_DOCUMENT_START,
                "directive requires an explicit document start marker",
            )],
        };
    }

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

use crate::syntax::{SyntaxKind, SyntaxNode};
use rowan::GreenNodeBuilder;

use super::lexer::lex_mapping_tokens;
use super::model::{
    ShadowYamlOptions, ShadowYamlOutcome, ShadowYamlReport, YamlInputKind, YamlToken, YamlTokenSpan,
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

fn emit_block_map<'a>(
    builder: &mut GreenNodeBuilder<'_>,
    tokens: &[YamlTokenSpan<'a>],
    i: &mut usize,
    stop_on_dedent: bool,
) -> Option<()> {
    let mut closed_by_dedent = false;
    while *i < tokens.len() {
        match tokens[*i].kind {
            YamlToken::Newline => {
                builder.token(SyntaxKind::NEWLINE.into(), tokens[*i].text);
                *i += 1;
            }
            YamlToken::DocumentStart
            | YamlToken::DocumentEnd
            | YamlToken::Directive
            | YamlToken::FlowMapEnd
            | YamlToken::FlowSeqEnd
            | YamlToken::Comma => {
                builder.token(SyntaxKind::YAML_SCALAR.into(), tokens[*i].text);
                *i += 1;
            }
            YamlToken::FlowMapStart | YamlToken::FlowSeqStart => {
                while *i < tokens.len() && tokens[*i].kind != YamlToken::Newline {
                    let kind = match tokens[*i].kind {
                        YamlToken::Whitespace => SyntaxKind::WHITESPACE,
                        YamlToken::Comment => SyntaxKind::YAML_COMMENT,
                        YamlToken::Tag => SyntaxKind::YAML_TAG,
                        _ => SyntaxKind::YAML_SCALAR,
                    };
                    builder.token(kind.into(), tokens[*i].text);
                    *i += 1;
                }
            }
            YamlToken::Anchor
            | YamlToken::Alias
            | YamlToken::BlockScalarHeader
            | YamlToken::BlockScalarContent => {
                builder.token(SyntaxKind::YAML_SCALAR.into(), tokens[*i].text);
                *i += 1;
            }
            YamlToken::Indent => return None,
            YamlToken::Dedent => {
                if stop_on_dedent {
                    *i += 1;
                    closed_by_dedent = true;
                    break;
                }
                return None;
            }
            _ => {
                builder.start_node(SyntaxKind::YAML_BLOCK_MAP_ENTRY.into());
                builder.start_node(SyntaxKind::YAML_BLOCK_MAP_KEY.into());

                let mut saw_colon = false;
                while *i < tokens.len() {
                    match tokens[*i].kind {
                        YamlToken::Key => {
                            builder.token(SyntaxKind::YAML_KEY.into(), tokens[*i].text);
                            *i += 1;
                        }
                        YamlToken::Tag => {
                            builder.token(SyntaxKind::YAML_TAG.into(), tokens[*i].text);
                            *i += 1;
                        }
                        YamlToken::Whitespace => {
                            builder.token(SyntaxKind::WHITESPACE.into(), tokens[*i].text);
                            *i += 1;
                        }
                        YamlToken::Colon => {
                            builder.token(SyntaxKind::YAML_COLON.into(), tokens[*i].text);
                            *i += 1;
                            saw_colon = true;
                            break;
                        }
                        _ => return None,
                    }
                }
                if !saw_colon {
                    return None;
                }
                builder.finish_node(); // YAML_BLOCK_MAP_KEY

                builder.start_node(SyntaxKind::YAML_BLOCK_MAP_VALUE.into());
                while *i < tokens.len() {
                    match tokens[*i].kind {
                        YamlToken::Scalar => {
                            builder.token(SyntaxKind::YAML_SCALAR.into(), tokens[*i].text);
                            *i += 1;
                        }
                        YamlToken::FlowMapStart
                        | YamlToken::FlowMapEnd
                        | YamlToken::FlowSeqStart
                        | YamlToken::FlowSeqEnd
                        | YamlToken::Comma
                        | YamlToken::Anchor
                        | YamlToken::Alias
                        | YamlToken::BlockScalarHeader
                        | YamlToken::BlockScalarContent => {
                            builder.token(SyntaxKind::YAML_SCALAR.into(), tokens[*i].text);
                            *i += 1;
                        }
                        YamlToken::Tag => {
                            builder.token(SyntaxKind::YAML_TAG.into(), tokens[*i].text);
                            *i += 1;
                        }
                        YamlToken::Comment => {
                            builder.token(SyntaxKind::YAML_COMMENT.into(), tokens[*i].text);
                            *i += 1;
                        }
                        YamlToken::Whitespace => {
                            builder.token(SyntaxKind::WHITESPACE.into(), tokens[*i].text);
                            *i += 1;
                        }
                        _ => break,
                    }
                }

                let mut trailing_newline: Option<&str> = None;
                if *i < tokens.len() && tokens[*i].kind == YamlToken::Newline {
                    trailing_newline = Some(tokens[*i].text);
                    *i += 1;
                }

                if *i < tokens.len() && tokens[*i].kind == YamlToken::Indent {
                    *i += 1;
                    builder.start_node(SyntaxKind::YAML_BLOCK_MAP.into());
                    emit_block_map(builder, tokens, i, true)?;
                    builder.finish_node(); // YAML_BLOCK_MAP
                }

                builder.finish_node(); // YAML_BLOCK_MAP_VALUE
                if let Some(newline) = trailing_newline {
                    builder.token(SyntaxKind::NEWLINE.into(), newline);
                }
                builder.finish_node(); // YAML_BLOCK_MAP_ENTRY
            }
        }
    }

    if stop_on_dedent && !closed_by_dedent {
        return None;
    }

    Some(())
}

/// Parse prototype YAML tree structure from input
pub fn parse_yaml_tree(input: &str) -> Option<SyntaxNode> {
    let tokens = lex_mapping_tokens(input)?;

    let mut builder = GreenNodeBuilder::new();
    builder.start_node(SyntaxKind::DOCUMENT.into());
    builder.start_node(SyntaxKind::YAML_METADATA_CONTENT.into());
    builder.start_node(SyntaxKind::YAML_BLOCK_MAP.into());
    let mut i = 0usize;
    emit_block_map(&mut builder, &tokens, &mut i, false)?;

    builder.finish_node(); // YAML_BLOCK_MAP
    builder.finish_node(); // YAML_METADATA_CONTENT
    builder.finish_node(); // DOCUMENT
    Some(SyntaxNode::new_root(builder.finish()))
}

use crate::syntax::{SyntaxKind, SyntaxNode};
use rowan::GreenNodeBuilder;

use super::lexer::{lex_mapping_tokens_with_diagnostic, split_once_unquoted};
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

fn emit_token_as_yaml(builder: &mut GreenNodeBuilder<'_>, token: &YamlTokenSpan<'_>) {
    let kind = match token.kind {
        YamlToken::Whitespace => SyntaxKind::WHITESPACE,
        YamlToken::Comment => SyntaxKind::YAML_COMMENT,
        YamlToken::Tag => SyntaxKind::YAML_TAG,
        YamlToken::Colon => SyntaxKind::YAML_COLON,
        _ => SyntaxKind::YAML_SCALAR,
    };
    builder.token(kind.into(), token.text);
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

fn emit_flow_sequence<'a>(
    builder: &mut GreenNodeBuilder<'_>,
    tokens: &[YamlTokenSpan<'a>],
    i: &mut usize,
) -> Result<(), YamlDiagnostic> {
    if *i >= tokens.len() || tokens[*i].kind != YamlToken::FlowSeqStart {
        return Err(YamlDiagnostic {
            code: diagnostic_codes::PARSE_EXPECTED_FLOW_SEQUENCE_START,
            message: "expected flow sequence start token",
            byte_start: tokens.get(*i).map(|t| t.byte_start).unwrap_or(0),
            byte_end: tokens.get(*i).map(|t| t.byte_end).unwrap_or(0),
        });
    }

    builder.start_node(SyntaxKind::YAML_FLOW_SEQUENCE.into());
    emit_token_as_yaml(builder, &tokens[*i]); // [
    *i += 1;

    let mut open_item = false;
    while *i < tokens.len() {
        match tokens[*i].kind {
            YamlToken::FlowSeqEnd => {
                if open_item {
                    builder.finish_node(); // YAML_FLOW_SEQUENCE_ITEM
                }
                emit_token_as_yaml(builder, &tokens[*i]); // ]
                *i += 1;
                if *i < tokens.len() {
                    match tokens[*i].kind {
                        YamlToken::Newline | YamlToken::Comment => {}
                        YamlToken::Whitespace if tokens[*i].text.trim().is_empty() => {}
                        _ => {
                            return Err(diag_at_token(
                                &tokens[*i],
                                diagnostic_codes::PARSE_TRAILING_CONTENT_AFTER_FLOW_END,
                                "trailing content after flow sequence end",
                            ));
                        }
                    }
                }
                builder.finish_node(); // YAML_FLOW_SEQUENCE
                return Ok(());
            }
            YamlToken::Comma => {
                if !open_item {
                    return Err(diag_at_token(
                        &tokens[*i],
                        diagnostic_codes::PARSE_INVALID_FLOW_SEQUENCE_COMMA,
                        "invalid comma position in flow sequence",
                    ));
                }
                builder.finish_node(); // YAML_FLOW_SEQUENCE_ITEM
                open_item = false;
                emit_token_as_yaml(builder, &tokens[*i]);
                *i += 1;
            }
            YamlToken::Whitespace if !open_item => {
                emit_token_as_yaml(builder, &tokens[*i]);
                *i += 1;
            }
            YamlToken::Scalar if !open_item && tokens[*i].text.trim().is_empty() => {
                emit_token_as_yaml(builder, &tokens[*i]);
                *i += 1;
            }
            YamlToken::FlowSeqStart => {
                if !open_item {
                    builder.start_node(SyntaxKind::YAML_FLOW_SEQUENCE_ITEM.into());
                    open_item = true;
                }
                emit_flow_sequence(builder, tokens, i)?;
            }
            YamlToken::FlowMapStart => {
                if !open_item {
                    builder.start_node(SyntaxKind::YAML_FLOW_SEQUENCE_ITEM.into());
                    open_item = true;
                }
                emit_flow_map(builder, tokens, i)?;
            }
            _ => {
                if !open_item {
                    builder.start_node(SyntaxKind::YAML_FLOW_SEQUENCE_ITEM.into());
                    open_item = true;
                }
                emit_token_as_yaml(builder, &tokens[*i]);
                *i += 1;
            }
        }
    }

    let (byte_start, byte_end) =
        if let Some(start) = tokens.iter().find(|t| t.kind == YamlToken::FlowSeqStart) {
            (
                start.byte_start,
                tokens.last().map(|t| t.byte_end).unwrap_or(start.byte_end),
            )
        } else {
            tokens
                .last()
                .map(|t| (t.byte_start, t.byte_end))
                .unwrap_or((0, 0))
        };
    Err(YamlDiagnostic {
        code: diagnostic_codes::PARSE_UNTERMINATED_FLOW_SEQUENCE,
        message: "unterminated flow sequence",
        byte_start,
        byte_end,
    })
}

fn emit_flow_map<'a>(
    builder: &mut GreenNodeBuilder<'_>,
    tokens: &[YamlTokenSpan<'a>],
    i: &mut usize,
) -> Result<(), YamlDiagnostic> {
    if *i >= tokens.len() || tokens[*i].kind != YamlToken::FlowMapStart {
        return Err(YamlDiagnostic {
            code: diagnostic_codes::PARSE_EXPECTED_FLOW_MAP_START,
            message: "expected flow map start token",
            byte_start: tokens.get(*i).map(|t| t.byte_start).unwrap_or(0),
            byte_end: tokens.get(*i).map(|t| t.byte_end).unwrap_or(0),
        });
    }

    builder.start_node(SyntaxKind::YAML_FLOW_MAP.into());
    emit_token_as_yaml(builder, &tokens[*i]); // {
    *i += 1;

    loop {
        // Skip inter-entry whitespace and newlines
        while *i < tokens.len()
            && matches!(tokens[*i].kind, YamlToken::Whitespace | YamlToken::Newline)
        {
            emit_token_as_yaml(builder, &tokens[*i]);
            *i += 1;
        }

        if *i >= tokens.len() {
            let (byte_start, byte_end) = tokens
                .last()
                .map(|t| (t.byte_start, t.byte_end))
                .unwrap_or((0, 0));
            return Err(YamlDiagnostic {
                code: diagnostic_codes::PARSE_UNTERMINATED_FLOW_MAP,
                message: "unterminated flow map",
                byte_start,
                byte_end,
            });
        }

        match tokens[*i].kind {
            YamlToken::FlowMapEnd => {
                emit_token_as_yaml(builder, &tokens[*i]);
                *i += 1;
                if *i < tokens.len() {
                    match tokens[*i].kind {
                        YamlToken::Newline
                        | YamlToken::Comment
                        | YamlToken::Whitespace
                        | YamlToken::FlowMapEnd
                        | YamlToken::FlowSeqEnd
                        | YamlToken::Comma => {}
                        _ => {
                            return Err(diag_at_token(
                                &tokens[*i],
                                diagnostic_codes::PARSE_TRAILING_CONTENT_AFTER_FLOW_END,
                                "trailing content after flow map end",
                            ));
                        }
                    }
                }
                builder.finish_node(); // YAML_FLOW_MAP
                return Ok(());
            }
            YamlToken::Comma => {
                emit_token_as_yaml(builder, &tokens[*i]);
                *i += 1;
            }
            _ => {
                emit_flow_map_entry(builder, tokens, i)?;
            }
        }
    }
}

fn emit_flow_map_entry<'a>(
    builder: &mut GreenNodeBuilder<'_>,
    tokens: &[YamlTokenSpan<'a>],
    i: &mut usize,
) -> Result<(), YamlDiagnostic> {
    builder.start_node(SyntaxKind::YAML_FLOW_MAP_ENTRY.into());
    builder.start_node(SyntaxKind::YAML_FLOW_MAP_KEY.into());

    // Emit leading whitespace inside key node
    while *i < tokens.len() && tokens[*i].kind == YamlToken::Whitespace {
        emit_token_as_yaml(builder, &tokens[*i]);
        *i += 1;
    }

    // Determine key/value split point
    let value_prefix: Option<&'a str> = match tokens.get(*i).map(|t| t.kind) {
        Some(YamlToken::Scalar) => {
            let scalar = tokens[*i];
            *i += 1;
            if let Some((key_text, rest_text)) = split_once_unquoted(scalar.text, ':') {
                builder.token(SyntaxKind::YAML_KEY.into(), key_text);
                builder.token(
                    SyntaxKind::YAML_COLON.into(),
                    &scalar.text[key_text.len()..key_text.len() + 1],
                );
                Some(rest_text)
            } else {
                // No colon — standalone scalar (implicit key or value-only)
                builder.token(SyntaxKind::YAML_SCALAR.into(), scalar.text);
                None
            }
        }
        Some(YamlToken::Key) => {
            // Already-tokenized key (from multi-line block lexing inside a flow map)
            builder.token(SyntaxKind::YAML_KEY.into(), tokens[*i].text);
            *i += 1;
            while *i < tokens.len() && tokens[*i].kind == YamlToken::Whitespace {
                emit_token_as_yaml(builder, &tokens[*i]);
                *i += 1;
            }
            if *i < tokens.len() && tokens[*i].kind == YamlToken::Colon {
                builder.token(SyntaxKind::YAML_COLON.into(), tokens[*i].text);
                *i += 1;
            }
            None
        }
        Some(YamlToken::Tag) => {
            emit_token_as_yaml(builder, &tokens[*i]);
            *i += 1;
            None
        }
        _ => None,
    };

    builder.finish_node(); // YAML_FLOW_MAP_KEY

    builder.start_node(SyntaxKind::YAML_FLOW_MAP_VALUE.into());
    if let Some(prefix) = value_prefix
        && !prefix.is_empty()
    {
        builder.token(SyntaxKind::YAML_SCALAR.into(), prefix);
    }
    emit_flow_value_tokens(builder, tokens, i)?;
    builder.finish_node(); // YAML_FLOW_MAP_VALUE

    builder.finish_node(); // YAML_FLOW_MAP_ENTRY
    Ok(())
}

fn emit_flow_value_tokens<'a>(
    builder: &mut GreenNodeBuilder<'_>,
    tokens: &[YamlTokenSpan<'a>],
    i: &mut usize,
) -> Result<(), YamlDiagnostic> {
    while *i < tokens.len() {
        match tokens[*i].kind {
            YamlToken::Comma | YamlToken::FlowMapEnd | YamlToken::FlowSeqEnd => break,
            YamlToken::FlowMapStart => emit_flow_map(builder, tokens, i)?,
            YamlToken::FlowSeqStart => emit_flow_sequence(builder, tokens, i)?,
            _ => {
                emit_token_as_yaml(builder, &tokens[*i]);
                *i += 1;
            }
        }
    }
    Ok(())
}

fn emit_block_seq<'a>(
    builder: &mut GreenNodeBuilder<'_>,
    tokens: &[YamlTokenSpan<'a>],
    i: &mut usize,
) -> Result<(), YamlDiagnostic> {
    while *i < tokens.len() {
        match tokens[*i].kind {
            YamlToken::Newline => {
                builder.token(SyntaxKind::NEWLINE.into(), tokens[*i].text);
                *i += 1;
            }
            YamlToken::DocumentStart => {
                builder.token(SyntaxKind::YAML_DOCUMENT_START.into(), tokens[*i].text);
                *i += 1;
            }
            YamlToken::DocumentEnd => {
                builder.token(SyntaxKind::YAML_DOCUMENT_END.into(), tokens[*i].text);
                *i += 1;
            }
            YamlToken::BlockSeqEntry => {
                builder.start_node(SyntaxKind::YAML_BLOCK_SEQUENCE_ITEM.into());
                builder.token(SyntaxKind::YAML_BLOCK_SEQ_ENTRY.into(), tokens[*i].text);
                *i += 1;
                while *i < tokens.len() && tokens[*i].kind != YamlToken::Newline {
                    emit_token_as_yaml(builder, &tokens[*i]);
                    *i += 1;
                }
                if *i < tokens.len() && tokens[*i].kind == YamlToken::Newline {
                    builder.token(SyntaxKind::NEWLINE.into(), tokens[*i].text);
                    *i += 1;
                }
                builder.finish_node(); // YAML_BLOCK_SEQUENCE_ITEM
            }
            _ => break,
        }
    }
    Ok(())
}

fn emit_block_map<'a>(
    builder: &mut GreenNodeBuilder<'_>,
    tokens: &[YamlTokenSpan<'a>],
    i: &mut usize,
    stop_on_dedent: bool,
) -> Result<(), YamlDiagnostic> {
    let mut closed_by_dedent = false;
    while *i < tokens.len() {
        match tokens[*i].kind {
            YamlToken::Newline => {
                builder.token(SyntaxKind::NEWLINE.into(), tokens[*i].text);
                *i += 1;
            }
            YamlToken::DocumentStart => {
                builder.token(SyntaxKind::YAML_DOCUMENT_START.into(), tokens[*i].text);
                *i += 1;
            }
            YamlToken::DocumentEnd => {
                builder.token(SyntaxKind::YAML_DOCUMENT_END.into(), tokens[*i].text);
                *i += 1;
            }
            YamlToken::Directive | YamlToken::Comma => {
                builder.token(SyntaxKind::YAML_SCALAR.into(), tokens[*i].text);
                *i += 1;
            }
            YamlToken::FlowMapEnd | YamlToken::FlowSeqEnd => {
                return Err(diag_at_token(
                    &tokens[*i],
                    diagnostic_codes::PARSE_UNEXPECTED_FLOW_CLOSER,
                    "unexpected flow closing token",
                ));
            }
            YamlToken::FlowMapStart | YamlToken::FlowSeqStart => {
                if tokens[*i].kind == YamlToken::FlowMapStart {
                    emit_flow_map(builder, tokens, i)?;
                } else {
                    emit_flow_sequence(builder, tokens, i)?;
                }
            }
            YamlToken::Anchor
            | YamlToken::Alias
            | YamlToken::BlockScalarHeader
            | YamlToken::BlockScalarContent => {
                builder.token(SyntaxKind::YAML_SCALAR.into(), tokens[*i].text);
                *i += 1;
            }
            YamlToken::Scalar | YamlToken::Comment => {
                while *i < tokens.len() && tokens[*i].kind != YamlToken::Newline {
                    if matches!(
                        tokens[*i].kind,
                        YamlToken::FlowMapEnd | YamlToken::FlowSeqEnd
                    ) {
                        return Err(diag_at_token(
                            &tokens[*i],
                            diagnostic_codes::PARSE_UNEXPECTED_FLOW_CLOSER,
                            "unexpected flow closing token",
                        ));
                    }
                    emit_token_as_yaml(builder, &tokens[*i]);
                    *i += 1;
                }
            }
            YamlToken::Indent => {
                return Err(diag_at_token(
                    &tokens[*i],
                    diagnostic_codes::PARSE_UNEXPECTED_INDENT,
                    "unexpected indent token while parsing block map",
                ));
            }
            YamlToken::Dedent => {
                if stop_on_dedent {
                    *i += 1;
                    closed_by_dedent = true;
                    break;
                }
                return Err(diag_at_token(
                    &tokens[*i],
                    diagnostic_codes::PARSE_UNEXPECTED_DEDENT,
                    "unexpected dedent token while parsing block map",
                ));
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
                        _ => {
                            return Err(diag_at_token(
                                &tokens[*i],
                                diagnostic_codes::PARSE_INVALID_KEY_TOKEN,
                                "invalid token while parsing block map key",
                            ));
                        }
                    }
                }
                if !saw_colon {
                    return Err(diag_at_token(
                        &tokens[(*i).saturating_sub(1)],
                        diagnostic_codes::PARSE_MISSING_COLON,
                        "missing colon in block map entry",
                    ));
                }
                builder.finish_node(); // YAML_BLOCK_MAP_KEY

                builder.start_node(SyntaxKind::YAML_BLOCK_MAP_VALUE.into());
                while *i < tokens.len() {
                    match tokens[*i].kind {
                        YamlToken::Scalar => {
                            builder.token(SyntaxKind::YAML_SCALAR.into(), tokens[*i].text);
                            *i += 1;
                        }
                        YamlToken::FlowMapStart => {
                            emit_flow_map(builder, tokens, i)?;
                        }
                        YamlToken::FlowSeqStart => {
                            emit_flow_sequence(builder, tokens, i)?;
                        }
                        YamlToken::Anchor
                        | YamlToken::Alias
                        | YamlToken::BlockScalarHeader
                        | YamlToken::BlockScalarContent => {
                            builder.token(SyntaxKind::YAML_SCALAR.into(), tokens[*i].text);
                            *i += 1;
                        }
                        YamlToken::FlowMapEnd | YamlToken::FlowSeqEnd | YamlToken::Comma => {
                            break;
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
                    // Emit trailing newline before nested content to preserve byte order
                    if let Some(newline) = trailing_newline.take() {
                        builder.token(SyntaxKind::NEWLINE.into(), newline);
                    }
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
        let (byte_start, byte_end) = tokens
            .last()
            .map(|t| (t.byte_start, t.byte_end))
            .unwrap_or((0, 0));
        return Err(YamlDiagnostic {
            code: diagnostic_codes::PARSE_UNTERMINATED_BLOCK_MAP,
            message: "unterminated indented block map",
            byte_start,
            byte_end,
        });
    }

    Ok(())
}

/// Parse prototype YAML tree structure from input
pub fn parse_yaml_tree(input: &str) -> Option<SyntaxNode> {
    parse_yaml_report(input).tree
}

/// Parse prototype YAML tree structure and include diagnostics on failure.
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

    let is_sequence = tokens
        .iter()
        .find(|t| {
            !matches!(
                t.kind,
                YamlToken::Newline
                    | YamlToken::Whitespace
                    | YamlToken::Comment
                    | YamlToken::DocumentStart
                    | YamlToken::DocumentEnd
                    | YamlToken::Directive
            )
        })
        .is_some_and(|t| t.kind == YamlToken::BlockSeqEntry);

    let mut builder = GreenNodeBuilder::new();
    builder.start_node(SyntaxKind::DOCUMENT.into());
    builder.start_node(SyntaxKind::YAML_METADATA_CONTENT.into());
    let mut i = 0usize;
    if is_sequence {
        builder.start_node(SyntaxKind::YAML_BLOCK_SEQUENCE.into());
        if let Err(err) = emit_block_seq(&mut builder, &tokens, &mut i) {
            return YamlParseReport {
                tree: None,
                diagnostics: vec![err],
            };
        }
        builder.finish_node(); // YAML_BLOCK_SEQUENCE
    } else {
        builder.start_node(SyntaxKind::YAML_BLOCK_MAP.into());
        if let Err(err) = emit_block_map(&mut builder, &tokens, &mut i, false) {
            return YamlParseReport {
                tree: None,
                diagnostics: vec![err],
            };
        }
        builder.finish_node(); // YAML_BLOCK_MAP
    }
    builder.finish_node(); // YAML_METADATA_CONTENT
    builder.finish_node(); // DOCUMENT
    YamlParseReport {
        tree: Some(SyntaxNode::new_root(builder.finish())),
        diagnostics: Vec::new(),
    }
}

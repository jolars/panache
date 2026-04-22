use crate::syntax::{SyntaxKind, SyntaxNode};
use rowan::GreenNodeBuilder;

use super::model::{
    BasicYamlEntry, ShadowYamlOptions, ShadowYamlOutcome, ShadowYamlReport, YamlInputKind,
    YamlShadowToken, YamlShadowTokenKind,
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

fn leading_indent(text: &str) -> usize {
    text.bytes()
        .take_while(|b| *b == b' ' || *b == b'\t')
        .count()
}

fn parse_raw_mapping_line(line: &str) -> Option<(&str, &str)> {
    let idx = find_unquoted_char(line, ':')?;
    let raw_key = &line[..idx];
    let raw_value = &line[idx + ':'.len_utf8()..];
    if raw_key.trim().is_empty() || raw_value.trim().is_empty() {
        return None;
    }
    Some((raw_key, raw_value))
}

fn split_value_and_comment(raw_value: &str) -> (&str, Option<&str>) {
    if let Some(idx) = find_unquoted_char(raw_value, '#') {
        let (before, after) = raw_value.split_at(idx);
        let starts_comment = before.chars().next_back().is_none_or(char::is_whitespace);
        if starts_comment {
            return (before.trim_end_matches([' ', '\t']), Some(after));
        }
    }
    (raw_value, None)
}

fn find_unquoted_char(text: &str, target: char) -> Option<usize> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut i = 0usize;
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped_in_double = false;

    while i < chars.len() {
        let (idx, ch) = chars[i];

        if in_double {
            if escaped_in_double {
                escaped_in_double = false;
                i += 1;
                continue;
            }
            match ch {
                '\\' => {
                    escaped_in_double = true;
                    i += 1;
                    continue;
                }
                '"' => {
                    in_double = false;
                    i += 1;
                    continue;
                }
                _ => {
                    i += 1;
                    continue;
                }
            }
        }

        if in_single {
            if ch == '\'' {
                if i + 1 < chars.len() && chars[i + 1].1 == '\'' {
                    i += 2;
                    continue;
                }
                in_single = false;
            }
            i += 1;
            continue;
        }

        match ch {
            '\'' => in_single = true,
            '"' => in_double = true,
            _ if ch == target => return Some(idx),
            _ => {}
        }
        i += 1;
    }

    None
}

fn split_tag_prefix(text: &str) -> (Option<&str>, &str) {
    let trimmed = text.trim_start_matches([' ', '\t']);
    if !trimmed.starts_with("!!") {
        return (None, text);
    }

    let rel_start = text.len() - trimmed.len();
    let rest = &text[rel_start + 2..];
    let end_rel = rest
        .char_indices()
        .find_map(|(i, ch)| (ch == ' ' || ch == '\t').then_some(i))
        .unwrap_or(rest.len());
    if end_rel == 0 {
        return (None, text);
    }

    let tag_end = rel_start + 2 + end_rel;
    let tag = &text[rel_start..tag_end];
    let value = &text[tag_end..];
    (Some(tag), value)
}

fn lex_mapping_line_tokens<'a>(
    line: &'a str,
    newline: &'a str,
    current_indent: usize,
    indent_stack: &mut Vec<usize>,
    out: &mut Vec<YamlShadowToken<'a>>,
) -> Option<()> {
    let line_indent = leading_indent(line);
    let content = &line[line_indent..];

    if content.trim().is_empty() {
        if !newline.is_empty() {
            out.push(YamlShadowToken {
                kind: YamlShadowTokenKind::Newline,
                text: newline,
            });
        }
        return Some(());
    }

    if line_indent > current_indent {
        indent_stack.push(line_indent);
        out.push(YamlShadowToken {
            kind: YamlShadowTokenKind::Indent,
            text: &line[..line_indent],
        });
    } else if line_indent < current_indent {
        while let Some(last) = indent_stack.last().copied() {
            if line_indent < last {
                indent_stack.pop();
                out.push(YamlShadowToken {
                    kind: YamlShadowTokenKind::Dedent,
                    text: "",
                });
            } else {
                break;
            }
        }
        if indent_stack.last().copied().unwrap_or(0) != line_indent {
            return None;
        }
    }

    if line_indent > 0 {
        out.push(YamlShadowToken {
            kind: YamlShadowTokenKind::Whitespace,
            text: &line[..line_indent],
        });
    }

    let (raw_key, raw_value) = parse_raw_mapping_line(content)?;

    let (key_tag, key_text) = split_tag_prefix(raw_key);
    if let Some(tag) = key_tag {
        out.push(YamlShadowToken {
            kind: YamlShadowTokenKind::Tag,
            text: tag,
        });
        let ws_len = leading_indent(key_text);
        if ws_len > 0 {
            out.push(YamlShadowToken {
                kind: YamlShadowTokenKind::Whitespace,
                text: &key_text[..ws_len],
            });
        }
        out.push(YamlShadowToken {
            kind: YamlShadowTokenKind::Key,
            text: &key_text[ws_len..],
        });
    } else {
        out.push(YamlShadowToken {
            kind: YamlShadowTokenKind::Key,
            text: raw_key,
        });
    }

    out.push(YamlShadowToken {
        kind: YamlShadowTokenKind::Colon,
        text: ":",
    });

    let (value_part, comment_part) = split_value_and_comment(raw_value);
    let leading_ws_len = leading_indent(value_part);
    if leading_ws_len > 0 {
        out.push(YamlShadowToken {
            kind: YamlShadowTokenKind::Whitespace,
            text: &value_part[..leading_ws_len],
        });
    }

    let scalar_part = &value_part[leading_ws_len..];
    let (value_tag, value_text) = split_tag_prefix(scalar_part);
    if let Some(tag) = value_tag {
        out.push(YamlShadowToken {
            kind: YamlShadowTokenKind::Tag,
            text: tag,
        });
        let ws_len = leading_indent(value_text);
        if ws_len > 0 {
            out.push(YamlShadowToken {
                kind: YamlShadowTokenKind::Whitespace,
                text: &value_text[..ws_len],
            });
        }
        out.push(YamlShadowToken {
            kind: YamlShadowTokenKind::Scalar,
            text: &value_text[ws_len..],
        });
    } else {
        out.push(YamlShadowToken {
            kind: YamlShadowTokenKind::Scalar,
            text: scalar_part,
        });
    }

    if let Some(comment) = comment_part {
        let leading_comment_ws_len = raw_value.len() - comment.len() - value_part.len();
        if leading_comment_ws_len > 0 {
            let start = value_part.len();
            let end = start + leading_comment_ws_len;
            out.push(YamlShadowToken {
                kind: YamlShadowTokenKind::Whitespace,
                text: &raw_value[start..end],
            });
        }
        out.push(YamlShadowToken {
            kind: YamlShadowTokenKind::Comment,
            text: comment,
        });
    }

    if !newline.is_empty() {
        out.push(YamlShadowToken {
            kind: YamlShadowTokenKind::Newline,
            text: newline,
        });
    }

    Some(())
}

pub fn lex_basic_mapping_tokens(input: &str) -> Option<Vec<YamlShadowToken<'_>>> {
    if input.is_empty() {
        return None;
    }

    let mut tokens = Vec::new();
    let mut indent_stack = vec![0usize];

    for raw_line in input.split_inclusive('\n') {
        let (line, newline) = split_line_and_newline(raw_line);
        let current_indent = indent_stack.last().copied().unwrap_or(0);
        lex_mapping_line_tokens(
            line,
            newline,
            current_indent,
            &mut indent_stack,
            &mut tokens,
        )?;
    }

    while indent_stack.len() > 1 {
        indent_stack.pop();
        tokens.push(YamlShadowToken {
            kind: YamlShadowTokenKind::Dedent,
            text: "",
        });
    }

    Some(tokens)
}

fn emit_block_map<'a>(
    builder: &mut GreenNodeBuilder<'_>,
    tokens: &[YamlShadowToken<'a>],
    i: &mut usize,
    stop_on_dedent: bool,
) -> Option<()> {
    let mut closed_by_dedent = false;
    while *i < tokens.len() {
        match tokens[*i].kind {
            YamlShadowTokenKind::Newline => {
                builder.token(SyntaxKind::NEWLINE.into(), tokens[*i].text);
                *i += 1;
            }
            YamlShadowTokenKind::Dedent => {
                if stop_on_dedent {
                    *i += 1;
                    closed_by_dedent = true;
                    break;
                }
                return None;
            }
            YamlShadowTokenKind::Indent => return None,
            _ => {
                builder.start_node(SyntaxKind::YAML_BLOCK_MAP_ENTRY.into());
                builder.start_node(SyntaxKind::YAML_BLOCK_MAP_KEY.into());

                let mut saw_colon = false;
                while *i < tokens.len() {
                    match tokens[*i].kind {
                        YamlShadowTokenKind::Key => {
                            builder.token(SyntaxKind::YAML_KEY.into(), tokens[*i].text);
                            *i += 1;
                        }
                        YamlShadowTokenKind::Tag => {
                            builder.token(SyntaxKind::YAML_TAG.into(), tokens[*i].text);
                            *i += 1;
                        }
                        YamlShadowTokenKind::Whitespace => {
                            builder.token(SyntaxKind::WHITESPACE.into(), tokens[*i].text);
                            *i += 1;
                        }
                        YamlShadowTokenKind::Colon => {
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
                        YamlShadowTokenKind::Scalar => {
                            builder.token(SyntaxKind::YAML_SCALAR.into(), tokens[*i].text);
                            *i += 1;
                        }
                        YamlShadowTokenKind::Tag => {
                            builder.token(SyntaxKind::YAML_TAG.into(), tokens[*i].text);
                            *i += 1;
                        }
                        YamlShadowTokenKind::Comment => {
                            builder.token(SyntaxKind::YAML_COMMENT.into(), tokens[*i].text);
                            *i += 1;
                        }
                        YamlShadowTokenKind::Whitespace => {
                            builder.token(SyntaxKind::WHITESPACE.into(), tokens[*i].text);
                            *i += 1;
                        }
                        _ => break,
                    }
                }

                let mut trailing_newline: Option<&str> = None;
                if *i < tokens.len() && tokens[*i].kind == YamlShadowTokenKind::Newline {
                    trailing_newline = Some(tokens[*i].text);
                    *i += 1;
                }

                if *i < tokens.len() && tokens[*i].kind == YamlShadowTokenKind::Indent {
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

/// Parse one or more `key: value` lines and emit a prototype YAML mapping CST.
///
/// This remains prototype-scoped but models YAML mapping structure with explicit
/// block-map and entry/key/value nodes, plus key/colon/whitespace/value/newline
/// tokens.
pub fn parse_basic_mapping_tree(input: &str) -> Option<SyntaxNode> {
    let tokens = lex_basic_mapping_tokens(input)?;

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

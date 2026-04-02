use rowan::TextRange;
use serde::Deserialize;

use super::{
    ExternalLinterParser, LinterError, ParseContext, line_col_to_offset,
    map_concatenated_offset_to_original_with_end_boundary,
};
use crate::linter::diagnostics::{Diagnostic, DiagnosticOrigin, Location};

#[derive(Debug, Deserialize)]
struct ShellcheckDiagnostic {
    code: i64,
    level: String,
    message: String,
    line: usize,
    #[serde(rename = "endLine")]
    end_line: usize,
    column: usize,
    #[serde(rename = "endColumn")]
    end_column: usize,
    #[serde(default)]
    fix: Option<ShellcheckFix>,
}

#[derive(Debug, Deserialize)]
struct ShellcheckFix {
    replacements: Vec<ShellcheckReplacement>,
}

#[derive(Debug, Deserialize)]
struct ShellcheckReplacement {
    line: usize,
    #[serde(rename = "endLine")]
    end_line: usize,
    column: usize,
    #[serde(rename = "endColumn")]
    end_column: usize,
    replacement: String,
    #[serde(default)]
    #[serde(rename = "insertionPoint")]
    insertion_point: Option<String>,
}

pub(crate) struct ShellcheckParser;

impl ExternalLinterParser for ShellcheckParser {
    const NAME: &'static str = "shellcheck";

    fn parse(ctx: &ParseContext<'_>) -> Result<Vec<Diagnostic>, LinterError> {
        use crate::linter::diagnostics::{Edit, Fix};

        let output: Vec<ShellcheckDiagnostic> = serde_json::from_str(ctx.output)
            .map_err(|e| LinterError::ParseError(format!("invalid shellcheck JSON: {}", e)))?;

        let mut diagnostics = Vec::new();
        for sc_diag in output {
            let (line, column, start_offset, end_offset) = if let Some(mappings) = ctx.mappings {
                let mapped_start =
                    line_col_to_offset(ctx.linted_input, sc_diag.line, sc_diag.column).and_then(
                        |offset| {
                            map_concatenated_offset_to_original_with_end_boundary(offset, mappings)
                        },
                    );
                let mapped_end =
                    line_col_to_offset(ctx.linted_input, sc_diag.end_line, sc_diag.end_column)
                        .and_then(|offset| {
                            map_concatenated_offset_to_original_with_end_boundary(offset, mappings)
                        });

                let fallback_block_start = mappings.first().map(|m| m.original_range.start);
                let start_offset = mapped_start.or(fallback_block_start).unwrap_or_else(|| {
                    line_col_to_offset(ctx.original_input, sc_diag.line, sc_diag.column)
                        .unwrap_or(ctx.original_input.len())
                });
                let end_offset = mapped_end.unwrap_or(start_offset.saturating_add(1));
                let (line, column) = offset_to_line_col(ctx.original_input, start_offset);
                (line, column, start_offset, end_offset)
            } else {
                let start_offset =
                    line_col_to_offset(ctx.original_input, sc_diag.line, sc_diag.column)
                        .unwrap_or(ctx.original_input.len());
                let end_offset =
                    line_col_to_offset(ctx.original_input, sc_diag.end_line, sc_diag.end_column)
                        .unwrap_or(ctx.original_input.len());
                (sc_diag.line, sc_diag.column, start_offset, end_offset)
            };
            let range = TextRange::new((start_offset as u32).into(), (end_offset as u32).into());
            let location = Location {
                line,
                column,
                range,
            };

            let fix = if let (Some(mappings), Some(fix)) = (ctx.mappings, sc_diag.fix.as_ref()) {
                let mut edits: Vec<(usize, Edit)> = Vec::new();
                for replacement in &fix.replacements {
                    let start =
                        line_col_to_offset(ctx.linted_input, replacement.line, replacement.column);
                    let end = line_col_to_offset(
                        ctx.linted_input,
                        replacement.end_line,
                        replacement.end_column,
                    );
                    let (Some(mut start), Some(mut end)) = (start, end) else {
                        edits.clear();
                        break;
                    };

                    if matches!(replacement.insertion_point.as_deref(), Some("afterEnd")) {
                        start = end;
                    } else if matches!(replacement.insertion_point.as_deref(), Some("beforeStart"))
                    {
                        end = start;
                    }

                    let Some(mapped_start) =
                        map_concatenated_offset_to_original_with_end_boundary(start, mappings)
                    else {
                        edits.clear();
                        break;
                    };
                    let Some(mapped_end) =
                        map_concatenated_offset_to_original_with_end_boundary(end, mappings)
                    else {
                        edits.clear();
                        break;
                    };

                    edits.push((
                        mapped_start,
                        Edit {
                            range: TextRange::new(
                                (mapped_start as u32).into(),
                                (mapped_end as u32).into(),
                            ),
                            replacement: replacement.replacement.clone(),
                        },
                    ));
                }

                if edits.is_empty() {
                    None
                } else {
                    edits.sort_by_key(|(start, _)| *start);
                    Some(Fix {
                        message: format!("Apply ShellCheck fix for SC{}", sc_diag.code),
                        edits: edits.into_iter().map(|(_, e)| e).collect(),
                    })
                }
            } else {
                None
            };

            let code = format!("SC{}", sc_diag.code);
            let diagnostic = match sc_diag.level.as_str() {
                "error" => Diagnostic::error(location, code, sc_diag.message),
                "warning" => Diagnostic::warning(location, code, sc_diag.message),
                _ => Diagnostic::info(location, code, sc_diag.message),
            }
            .with_origin(DiagnosticOrigin::External);
            diagnostics.push(if let Some(fix) = fix {
                diagnostic.with_fix(fix)
            } else {
                diagnostic
            });
        }
        Ok(diagnostics)
    }
}

fn offset_to_line_col(input: &str, offset: usize) -> (usize, usize) {
    let mut line = 1;
    let mut column = 1;
    for (idx, ch) in input.char_indices() {
        if idx >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }
    (line, column)
}

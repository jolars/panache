use rowan::TextRange;
use serde::Deserialize;

use super::{
    ExternalLinterParser, LinterError, ParseContext, line_col_to_offset,
    map_concatenated_offset_to_original_with_end_boundary,
};
use crate::linter::diagnostics::{Diagnostic, Location, Severity};

#[derive(Debug, Deserialize)]
pub(crate) struct RuffDiagnostic {
    code: String,
    message: String,
    location: RuffPosition,
    end_location: RuffPosition,
    #[allow(dead_code)]
    filename: String,
    #[serde(default)]
    severity: Option<String>,
    #[serde(default)]
    fix: Option<RuffFix>,
}

#[derive(Debug, Deserialize)]
struct RuffPosition {
    row: usize,
    column: usize,
}

#[derive(Debug, Deserialize)]
struct RuffFix {
    message: String,
    edits: Vec<RuffEdit>,
}

#[derive(Debug, Deserialize)]
struct RuffEdit {
    content: String,
    location: RuffPosition,
    end_location: RuffPosition,
}

pub(crate) struct RuffParser;

impl ExternalLinterParser for RuffParser {
    const NAME: &'static str = "ruff";

    fn parse(ctx: &ParseContext<'_>) -> Result<Vec<Diagnostic>, LinterError> {
        use crate::linter::diagnostics::{Edit, Fix};

        let output: Vec<RuffDiagnostic> = serde_json::from_str(ctx.output)
            .map_err(|e| LinterError::ParseError(format!("invalid ruff JSON: {}", e)))?;

        let mut diagnostics = Vec::new();
        for ruff_diag in output {
            let line = ruff_diag.location.row;
            let column = ruff_diag.location.column;
            let start_offset = line_col_to_offset(ctx.original_input, line, column)
                .unwrap_or(ctx.original_input.len());

            let end_line = ruff_diag.end_location.row;
            let end_column = ruff_diag.end_location.column;
            let end_offset = line_col_to_offset(ctx.original_input, end_line, end_column)
                .unwrap_or(ctx.original_input.len());

            let range = TextRange::new((start_offset as u32).into(), (end_offset as u32).into());
            let location = Location {
                line,
                column,
                range,
            };

            let fix = if let (Some(mappings), Some(fix)) = (ctx.mappings, ruff_diag.fix.as_ref()) {
                let mut edits = Vec::new();
                for edit in &fix.edits {
                    let start = line_col_to_offset(
                        ctx.linted_input,
                        edit.location.row,
                        edit.location.column,
                    );
                    let end = line_col_to_offset(
                        ctx.linted_input,
                        edit.end_location.row,
                        edit.end_location.column,
                    );

                    let Some(start) = start else {
                        edits.clear();
                        break;
                    };
                    let Some(end) = end else {
                        edits.clear();
                        break;
                    };

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

                    edits.push(Edit {
                        range: TextRange::new(
                            (mapped_start as u32).into(),
                            (mapped_end as u32).into(),
                        ),
                        replacement: edit.content.clone(),
                    });
                }

                if edits.is_empty() {
                    None
                } else {
                    Some(Fix {
                        message: fix.message.clone(),
                        edits,
                    })
                }
            } else {
                None
            };

            let diagnostic = match ruff_diag.severity.as_deref() {
                Some("error") => Diagnostic::error(location, ruff_diag.code, ruff_diag.message),
                Some("warning") => Diagnostic::warning(location, ruff_diag.code, ruff_diag.message),
                _ => Diagnostic {
                    severity: Severity::Info,
                    location,
                    message: ruff_diag.message,
                    code: ruff_diag.code,
                    fix: None,
                },
            };
            diagnostics.push(if let Some(fix) = fix {
                diagnostic.with_fix(fix)
            } else {
                diagnostic
            });
        }
        Ok(diagnostics)
    }
}

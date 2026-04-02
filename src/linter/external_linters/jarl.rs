use rowan::TextRange;
use serde::Deserialize;

use super::{
    ExternalLinterParser, LinterError, ParseContext, line_col_to_offset,
    map_concatenated_offset_to_original_with_end_boundary,
};
use crate::linter::diagnostics::{Diagnostic, Location};

#[derive(Debug, Deserialize)]
struct JarlOutput {
    diagnostics: Vec<JarlDiagnostic>,
    #[allow(dead_code)]
    errors: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct JarlDiagnostic {
    message: JarlMessage,
    #[allow(dead_code)]
    filename: String,
    range: [usize; 2],
    location: JarlLocation,
    fix: JarlFix,
}

#[derive(Debug, Deserialize)]
struct JarlMessage {
    name: String,
    body: String,
    #[allow(dead_code)]
    suggestion: Option<String>,
}

#[derive(Debug, Deserialize)]
struct JarlLocation {
    row: usize,
    column: usize,
}

#[derive(Debug, Deserialize)]
struct JarlFix {
    content: String,
    start: usize,
    end: usize,
    to_skip: bool,
}

pub(crate) struct JarlParser;

impl ExternalLinterParser for JarlParser {
    const NAME: &'static str = "jarl";

    fn parse(ctx: &ParseContext<'_>) -> Result<Vec<Diagnostic>, LinterError> {
        use crate::linter::diagnostics::{Edit, Fix};

        let output: JarlOutput = serde_json::from_str(ctx.output)
            .map_err(|e| LinterError::ParseError(format!("invalid jarl JSON: {}", e)))?;

        let mut diagnostics = Vec::new();
        for jarl_diag in output.diagnostics {
            let line = jarl_diag.location.row;
            let column = jarl_diag.location.column + 1;
            let range_len = jarl_diag.range[1].saturating_sub(jarl_diag.range[0]);
            let start_offset = line_col_to_offset(ctx.original_input, line, column)
                .unwrap_or(ctx.original_input.len());
            let end_offset = start_offset
                .saturating_add(range_len)
                .min(ctx.original_input.len());
            let range = TextRange::new((start_offset as u32).into(), (end_offset as u32).into());

            let location = Location {
                line,
                column,
                range,
            };

            let fix = if let Some(mappings) = ctx.mappings {
                if !jarl_diag.fix.to_skip {
                    if let (Some(fix_start), Some(fix_end)) = (
                        map_concatenated_offset_to_original_with_end_boundary(
                            jarl_diag.fix.start,
                            mappings,
                        ),
                        map_concatenated_offset_to_original_with_end_boundary(
                            jarl_diag.fix.end,
                            mappings,
                        ),
                    ) {
                        Some(Fix {
                            message: format!("Apply suggested fix: {}", jarl_diag.fix.content),
                            edits: vec![Edit {
                                range: TextRange::new(
                                    (fix_start as u32).into(),
                                    (fix_end as u32).into(),
                                ),
                                replacement: jarl_diag.fix.content.clone(),
                            }],
                        })
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            };

            let diagnostic =
                Diagnostic::warning(location, jarl_diag.message.name, jarl_diag.message.body);
            diagnostics.push(if let Some(fix) = fix {
                diagnostic.with_fix(fix)
            } else {
                diagnostic
            });
        }
        Ok(diagnostics)
    }
}

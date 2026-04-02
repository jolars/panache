use rowan::TextRange;
use serde::Deserialize;

use super::{
    ExternalLinterParser, LinterError, ParseContext, line_col_to_offset,
    map_concatenated_offset_to_original_with_end_boundary,
};
use crate::linter::diagnostics::{Diagnostic, DiagnosticOrigin, Location};

#[derive(Debug, Deserialize)]
struct EslintFileReport {
    messages: Vec<EslintMessage>,
}

#[derive(Debug, Deserialize)]
struct EslintMessage {
    #[serde(default, rename = "ruleId")]
    rule_id: Option<String>,
    severity: i64,
    message: String,
    line: usize,
    column: usize,
    #[serde(default, rename = "endLine")]
    end_line: Option<usize>,
    #[serde(default, rename = "endColumn")]
    end_column: Option<usize>,
    #[serde(default)]
    fix: Option<EslintFix>,
    #[serde(default)]
    suggestions: Vec<EslintSuggestion>,
}

#[derive(Debug, Deserialize)]
struct EslintSuggestion {
    fix: EslintFix,
}

#[derive(Debug, Deserialize)]
struct EslintFix {
    range: [usize; 2],
    text: String,
}

pub(crate) struct EslintParser;

impl ExternalLinterParser for EslintParser {
    const NAME: &'static str = "eslint";

    fn parse(ctx: &ParseContext<'_>) -> Result<Vec<Diagnostic>, LinterError> {
        use crate::linter::diagnostics::{Edit, Fix};

        let reports: Vec<EslintFileReport> = serde_json::from_str(ctx.output)
            .map_err(|e| LinterError::ParseError(format!("invalid eslint JSON: {}", e)))?;

        let mut diagnostics = Vec::new();
        for report in reports {
            for msg in report.messages {
                let line = msg.line;
                let column = msg.column;
                let start_offset = line_col_to_offset(ctx.original_input, line, column)
                    .unwrap_or(ctx.original_input.len());
                let end_line = msg.end_line.unwrap_or(line);
                let end_column = msg.end_column.unwrap_or(column.saturating_add(1));
                let end_offset = line_col_to_offset(ctx.original_input, end_line, end_column)
                    .unwrap_or(ctx.original_input.len());

                let location = Location {
                    line,
                    column,
                    range: TextRange::new((start_offset as u32).into(), (end_offset as u32).into()),
                };

                let code = msg.rule_id.unwrap_or_else(|| "eslint".to_string());
                let diagnostic = match msg.severity {
                    2 => Diagnostic::error(location, code, msg.message),
                    1 => Diagnostic::info(location, code, msg.message),
                    _ => Diagnostic::warning(location, code, msg.message),
                }
                .with_origin(DiagnosticOrigin::External);
                let selected_fix = msg
                    .fix
                    .or_else(|| msg.suggestions.into_iter().next().map(|s| s.fix));
                let mapped_fix =
                    if let (Some(mappings), Some(eslint_fix)) = (ctx.mappings, selected_fix) {
                        let fix_start = map_concatenated_offset_to_original_with_end_boundary(
                            eslint_fix.range[0],
                            mappings,
                        );
                        let fix_end = map_concatenated_offset_to_original_with_end_boundary(
                            eslint_fix.range[1],
                            mappings,
                        );
                        if let (Some(fix_start), Some(fix_end)) = (fix_start, fix_end) {
                            Some(Fix {
                                message: "Apply ESLint fix".to_string(),
                                edits: vec![Edit {
                                    range: TextRange::new(
                                        (fix_start as u32).into(),
                                        (fix_end as u32).into(),
                                    ),
                                    replacement: eslint_fix.text,
                                }],
                            })
                        } else {
                            None
                        }
                    } else {
                        None
                    };

                diagnostics.push(if let Some(fix) = mapped_fix {
                    diagnostic.with_fix(fix)
                } else {
                    diagnostic
                });
            }
        }

        Ok(diagnostics)
    }
}

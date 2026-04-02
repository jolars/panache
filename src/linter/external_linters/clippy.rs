use rowan::TextRange;
use serde::Deserialize;

use super::{ExternalLinterParser, LinterError, ParseContext, line_col_to_offset};
use crate::linter::diagnostics::{Diagnostic, Location, Severity};

#[derive(Debug, Deserialize)]
struct ClippyMessage {
    #[serde(rename = "$message_type")]
    message_type: String,
    #[serde(default)]
    message: String,
    #[serde(default)]
    level: String,
    #[serde(default)]
    code: Option<ClippyCode>,
    #[serde(default)]
    spans: Vec<ClippySpan>,
}

#[derive(Debug, Deserialize)]
struct ClippyCode {
    code: String,
}

#[derive(Debug, Deserialize)]
struct ClippySpan {
    line_start: usize,
    column_start: usize,
    line_end: usize,
    column_end: usize,
    is_primary: bool,
}

pub(crate) struct ClippyParser;

impl ExternalLinterParser for ClippyParser {
    const NAME: &'static str = "clippy";

    fn parse(ctx: &ParseContext<'_>) -> Result<Vec<Diagnostic>, LinterError> {
        let messages = parse_clippy_messages(ctx.output)
            .map_err(|e| LinterError::ParseError(format!("invalid clippy JSON: {}", e)))?;

        let mut diagnostics = Vec::new();
        for msg in messages {
            if msg.message_type != "diagnostic" {
                continue;
            }

            let Some(primary_span) = msg
                .spans
                .iter()
                .find(|s| s.is_primary)
                .or(msg.spans.first())
            else {
                continue;
            };

            let line = primary_span.line_start;
            let column = primary_span.column_start;
            let start_offset = line_col_to_offset(ctx.original_input, line, column)
                .unwrap_or(ctx.original_input.len());
            let end_offset = line_col_to_offset(
                ctx.original_input,
                primary_span.line_end,
                primary_span.column_end,
            )
            .unwrap_or(ctx.original_input.len());

            let location = Location {
                line,
                column,
                range: TextRange::new((start_offset as u32).into(), (end_offset as u32).into()),
            };

            let code = msg
                .code
                .map(|c| c.code)
                .unwrap_or_else(|| "clippy".to_string());
            let diagnostic = match msg.level.as_str() {
                "error" => Diagnostic::error(location, code, msg.message),
                "warning" => Diagnostic::warning(location, code, msg.message),
                _ => Diagnostic {
                    severity: Severity::Info,
                    location,
                    message: msg.message,
                    code,
                    fix: None,
                },
            };
            diagnostics.push(diagnostic);
        }

        Ok(diagnostics)
    }
}

fn parse_clippy_messages(output: &str) -> Result<Vec<ClippyMessage>, serde_json::Error> {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }

    let mut messages = Vec::new();
    for line in trimmed.lines() {
        let line = line.trim();
        if !line.starts_with('{') {
            continue;
        }
        messages.push(serde_json::from_str::<ClippyMessage>(line)?);
    }
    Ok(messages)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::linter::external_linters::ParseContext;

    #[test]
    fn parses_clippy_diagnostic_line() {
        let line = r#"{"$message_type":"diagnostic","message":"useless use of vec!","code":{"code":"clippy::useless_vec"},"level":"warning","spans":[{"line_start":1,"column_start":13,"line_end":1,"column_end":24,"is_primary":true}]}"#;
        let parsed = parse_clippy_messages(line).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].level, "warning");
    }

    #[test]
    fn maps_clippy_diagnostic_to_panache() {
        let ctx = ParseContext {
            output: r#"{"$message_type":"diagnostic","message":"useless use of vec!","code":{"code":"clippy::useless_vec"},"level":"warning","spans":[{"line_start":1,"column_start":13,"line_end":1,"column_end":24,"is_primary":true}]}"#,
            linted_input: "fn main(){ let x = vec![1,2]; }\n",
            original_input: "fn main(){ let x = vec![1,2]; }\n",
            mappings: None,
        };
        let diagnostics = ClippyParser::parse(&ctx).unwrap();
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "clippy::useless_vec");
    }
}

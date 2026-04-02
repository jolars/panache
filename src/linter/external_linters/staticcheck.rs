use rowan::TextRange;
use serde::Deserialize;

use super::{ExternalLinterParser, LinterError, ParseContext, line_col_to_offset};
use crate::linter::diagnostics::{Diagnostic, Location};

#[derive(Debug, Deserialize)]
struct StaticcheckDiagnostic {
    #[serde(rename = "code")]
    check: String,
    location: StaticcheckLocation,
    message: String,
}

#[derive(Debug, Deserialize)]
struct StaticcheckLocation {
    #[allow(dead_code)]
    file: String,
    line: usize,
    column: usize,
}

pub(crate) struct StaticcheckParser;

impl ExternalLinterParser for StaticcheckParser {
    const NAME: &'static str = "staticcheck";

    fn parse(ctx: &ParseContext<'_>) -> Result<Vec<Diagnostic>, LinterError> {
        let diagnostics: Vec<StaticcheckDiagnostic> = parse_staticcheck_output(ctx.output)
            .map_err(|e| LinterError::ParseError(format!("invalid staticcheck JSON: {}", e)))?;

        let mut output = Vec::new();
        for diag in diagnostics {
            let line = diag.location.line;
            let column = diag.location.column;
            let start_offset = line_col_to_offset(ctx.original_input, line, column)
                .unwrap_or(ctx.original_input.len());
            let end_offset = line_col_to_offset(ctx.original_input, line, column.saturating_add(1))
                .unwrap_or(ctx.original_input.len());

            let location = Location {
                line,
                column,
                range: TextRange::new((start_offset as u32).into(), (end_offset as u32).into()),
            };

            output.push(Diagnostic::warning(location, diag.check, diag.message));
        }

        Ok(output)
    }
}

fn parse_staticcheck_output(output: &str) -> Result<Vec<StaticcheckDiagnostic>, serde_json::Error> {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }

    if trimmed.starts_with('[') {
        return serde_json::from_str(trimmed);
    }

    let mut diagnostics = Vec::new();
    for line in trimmed.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        diagnostics.push(serde_json::from_str::<StaticcheckDiagnostic>(line)?);
    }
    Ok(diagnostics)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::linter::external_linters::ParseContext;

    #[test]
    fn parses_staticcheck_array_json() {
        let json =
            r#"[{"code":"SA1006","location":{"file":"x.go","line":2,"column":5},"message":"msg"}]"#;
        let parsed = parse_staticcheck_output(json).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].check, "SA1006");
    }

    #[test]
    fn parses_staticcheck_ndjson() {
        let json =
            r#"{"code":"SA1006","location":{"file":"x.go","line":2,"column":5},"message":"msg"}"#;
        let parsed = parse_staticcheck_output(json).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].location.line, 2);
    }

    #[test]
    fn maps_staticcheck_diagnostic_to_panache() {
        let ctx = ParseContext {
            output: r#"[{"code":"SA1006","location":{"file":"x.go","line":1,"column":1},"message":"msg"}]"#,
            linted_input: "x := 1\n",
            original_input: "x := 1\n",
            mappings: None,
        };
        let diagnostics = StaticcheckParser::parse(&ctx).unwrap();
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "SA1006");
    }
}

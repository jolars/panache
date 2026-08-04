//! Shared parser for the jolars family of linters (arity, fatou, badness).
//!
//! All three tools emit a top-level JSON array of diagnostics with 0-indexed
//! byte ranges into the linted file plus optional fix data. arity and fatou
//! share one schema modulo severity casing (arity: `Warning`, fatou:
//! `warning`) and fix shape (arity: single optional `fix` object, fatou:
//! `fixes` array). badness diverges slightly: flat `start`/`end` keys instead
//! of a `range` object, a plain-string `message`, and a fix carrying an
//! `edits` array whose entries may target *other* files (cross-file fixes,
//! which are meaningless inside a code block and therefore skipped).

use rowan::TextRange;
use serde::Deserialize;

use super::{
    ExternalLinterParser, LinterError, ParseContext,
    map_concatenated_offset_to_original_with_end_boundary,
};
use crate::linter::diagnostics::{
    Diagnostic, DiagnosticNoteKind, DiagnosticOrigin, Edit, Fix, Location,
};

#[derive(Debug, Deserialize)]
struct FamilyDiagnostic {
    rule: String,
    severity: String,
    range: FamilyRange,
    message: FamilyMessage,
    #[serde(default)]
    fix: Option<FamilyFix>,
    #[serde(default)]
    fixes: Vec<FamilyFix>,
}

#[derive(Debug, Deserialize)]
struct FamilyRange {
    start: usize,
    end: usize,
}

#[derive(Debug, Deserialize)]
struct FamilyMessage {
    #[allow(dead_code)]
    name: String,
    body: String,
    #[serde(default)]
    suggestion: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FamilyFix {
    content: String,
    start: usize,
    end: usize,
    applicability: String,
    description: String,
}

#[derive(Debug, Deserialize)]
struct BadnessDiagnostic {
    rule: String,
    severity: String,
    start: usize,
    end: usize,
    message: String,
    #[serde(default)]
    fix: Option<BadnessFix>,
}

#[derive(Debug, Deserialize)]
struct BadnessFix {
    edits: Vec<BadnessEdit>,
    applicability: String,
    description: String,
}

#[derive(Debug, Deserialize)]
struct BadnessEdit {
    content: String,
    start: usize,
    end: usize,
    /// Present only for cross-file edits (targeting a file other than the
    /// linted one).
    #[serde(default)]
    path: Option<String>,
}

/// Map a byte offset in the linted (possibly concatenated) input back to the
/// original document, clamped to the input like the other parsers do.
fn map_diagnostic_offset(offset: usize, ctx: &ParseContext<'_>) -> usize {
    match ctx.mappings {
        Some(mappings) => map_concatenated_offset_to_original_with_end_boundary(offset, mappings)
            .unwrap_or(ctx.original_input.len()),
        None => offset.min(ctx.original_input.len()),
    }
}

/// Map the diagnostic's byte range and resolve it to a `Location` in the
/// original document.
fn diagnostic_location(start: usize, end: usize, ctx: &ParseContext<'_>) -> Location {
    let start_offset = map_diagnostic_offset(start, ctx);
    let end_offset = map_diagnostic_offset(end, ctx).max(start_offset);
    let range = TextRange::new((start_offset as u32).into(), (end_offset as u32).into());
    Location::from_range(range, ctx.original_input)
}

/// Build an external diagnostic with the family's case-insensitive severity
/// words (`hint` folds into info, like the other parsers' fallback arm).
fn build_diagnostic(severity: &str, location: Location, rule: String, body: String) -> Diagnostic {
    match severity.to_ascii_lowercase().as_str() {
        "error" => Diagnostic::error(location, rule, body),
        "warning" => Diagnostic::warning(location, rule, body),
        _ => Diagnostic::info(location, rule, body),
    }
    .with_origin(DiagnosticOrigin::External)
}

/// Build a panache fix honoring the family's `safe`/`unsafe` applicability.
fn build_fix(applicability: &str, description: String, edits: Vec<Edit>) -> Fix {
    if applicability.eq_ignore_ascii_case("unsafe") {
        Fix::unsafe_fix(description, edits)
    } else {
        Fix::safe(description, edits)
    }
}

fn parse_family(ctx: &ParseContext<'_>, tool: &str) -> Result<Vec<Diagnostic>, LinterError> {
    let output: Vec<FamilyDiagnostic> = serde_json::from_str(ctx.output)
        .map_err(|e| LinterError::ParseError(format!("invalid {} JSON: {}", tool, e)))?;

    let mut diagnostics = Vec::new();
    for family_diag in output {
        let location = diagnostic_location(family_diag.range.start, family_diag.range.end, ctx);

        let fix = if let Some(mappings) = ctx.mappings {
            family_diag
                .fix
                .as_ref()
                .or_else(|| family_diag.fixes.first())
                .and_then(|family_fix| {
                    let fix_start = map_concatenated_offset_to_original_with_end_boundary(
                        family_fix.start,
                        mappings,
                    )?;
                    let fix_end = map_concatenated_offset_to_original_with_end_boundary(
                        family_fix.end,
                        mappings,
                    )?;
                    let edits = vec![Edit {
                        range: TextRange::new((fix_start as u32).into(), (fix_end as u32).into()),
                        replacement: family_fix.content.clone(),
                    }];
                    Some(build_fix(
                        &family_fix.applicability,
                        family_fix.description.clone(),
                        edits,
                    ))
                })
        } else {
            None
        };

        let mut diagnostic = build_diagnostic(
            &family_diag.severity,
            location,
            family_diag.rule,
            family_diag.message.body,
        );

        if let Some(suggestion) = family_diag.message.suggestion.as_ref()
            && !suggestion.trim().is_empty()
        {
            diagnostic = diagnostic.with_note(DiagnosticNoteKind::Help, suggestion.clone());
        }
        diagnostics.push(if let Some(fix) = fix {
            diagnostic.with_fix(fix)
        } else {
            diagnostic
        });
    }
    Ok(diagnostics)
}

fn parse_badness(ctx: &ParseContext<'_>) -> Result<Vec<Diagnostic>, LinterError> {
    let output: Vec<BadnessDiagnostic> = serde_json::from_str(ctx.output)
        .map_err(|e| LinterError::ParseError(format!("invalid badness JSON: {}", e)))?;

    let mut diagnostics = Vec::new();
    for badness_diag in output {
        let location = diagnostic_location(badness_diag.start, badness_diag.end, ctx);

        let fix = if let Some(mappings) = ctx.mappings {
            badness_diag.fix.as_ref().and_then(|badness_fix| {
                // A cross-file edit targets a file outside the linted code
                // blocks, so the fix as a whole cannot apply to this document.
                if badness_fix.edits.iter().any(|edit| edit.path.is_some()) {
                    return None;
                }
                let mut edits = Vec::new();
                for edit in &badness_fix.edits {
                    let start = map_concatenated_offset_to_original_with_end_boundary(
                        edit.start, mappings,
                    )?;
                    let end =
                        map_concatenated_offset_to_original_with_end_boundary(edit.end, mappings)?;
                    edits.push(Edit {
                        range: TextRange::new((start as u32).into(), (end as u32).into()),
                        replacement: edit.content.clone(),
                    });
                }
                if edits.is_empty() {
                    return None;
                }
                Some(build_fix(
                    &badness_fix.applicability,
                    badness_fix.description.clone(),
                    edits,
                ))
            })
        } else {
            None
        };

        let diagnostic = build_diagnostic(
            &badness_diag.severity,
            location,
            badness_diag.rule,
            badness_diag.message,
        );
        diagnostics.push(if let Some(fix) = fix {
            diagnostic.with_fix(fix)
        } else {
            diagnostic
        });
    }
    Ok(diagnostics)
}

pub(crate) struct ArityParser;

impl ExternalLinterParser for ArityParser {
    const NAME: &'static str = "arity";

    fn parse(ctx: &ParseContext<'_>) -> Result<Vec<Diagnostic>, LinterError> {
        parse_family(ctx, Self::NAME)
    }
}

pub(crate) struct FatouParser;

impl ExternalLinterParser for FatouParser {
    const NAME: &'static str = "fatou";

    fn parse(ctx: &ParseContext<'_>) -> Result<Vec<Diagnostic>, LinterError> {
        parse_family(ctx, Self::NAME)
    }
}

pub(crate) struct BadnessParser;

impl ExternalLinterParser for BadnessParser {
    const NAME: &'static str = "badness";

    fn parse(ctx: &ParseContext<'_>) -> Result<Vec<Diagnostic>, LinterError> {
        parse_badness(ctx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::linter::code_block_collector::BlockMapping;
    use crate::linter::diagnostics::{FixSafety, Severity};

    // Captured from `arity lint --no-config --output json` on `any(is.na(x))\n`.
    const ARITY_OUTPUT: &str = r#"[
  {
    "rule": "any-is-na",
    "severity": "Warning",
    "path": "input.R",
    "range": { "start": 0, "end": 13 },
    "message": {
      "name": "any-is-na",
      "body": "`any(is.na(x))` is the faster, clearer `anyNA(x)`",
      "suggestion": "Use `anyNA(x)`."
    },
    "fix": {
      "content": "anyNA(x)",
      "start": 0,
      "end": 13,
      "applicability": "safe",
      "description": "Replace `any(is.na(x))` with `anyNA(x)`"
    }
  },
  {
    "rule": "undefined-symbol",
    "severity": "Warning",
    "path": "input.R",
    "range": { "start": 10, "end": 11 },
    "message": {
      "name": "undefined-symbol",
      "body": "no in-scope binding or attached package exports `x`",
      "suggestion": null
    }
  }
]"#;

    // Captured from `fatou lint --no-config --output json`.
    const FATOU_OUTPUT: &str = r#"[
  {
    "rule": "nothing-comparison",
    "severity": "warning",
    "path": "fix.jl",
    "range": { "start": 3, "end": 15 },
    "message": {
      "name": "nothing-comparison",
      "body": "comparison against `nothing` by value; use `===` or `isnothing`",
      "suggestion": null
    },
    "fixes": [
      {
        "description": "Replace `==` with `===`",
        "content": "===",
        "start": 5,
        "end": 7,
        "applicability": "safe"
      }
    ]
  }
]"#;

    #[test]
    fn parses_arity_diagnostics_without_mappings() {
        let input = "any(is.na(x))\n";
        let ctx = ParseContext {
            output: ARITY_OUTPUT,
            linted_input: input,
            original_input: input,
            mappings: None,
        };
        let diagnostics = ArityParser::parse(&ctx).unwrap();
        assert_eq!(diagnostics.len(), 2);

        let diag = &diagnostics[0];
        assert_eq!(diag.code, "any-is-na");
        assert_eq!(diag.severity, Severity::Warning);
        assert_eq!(diag.origin, DiagnosticOrigin::External);
        assert_eq!(diag.location.line, 1);
        assert_eq!(diag.location.column, 1);
        assert_eq!(diag.location.range, TextRange::new(0.into(), 13.into()));
        assert_eq!(diag.notes.len(), 1);
        assert_eq!(diag.notes[0].kind, DiagnosticNoteKind::Help);
        assert_eq!(diag.notes[0].message, "Use `anyNA(x)`.");
        // Fixes require block mappings, matching the jarl/ruff parsers.
        assert!(diag.fix.is_none());

        let diag = &diagnostics[1];
        assert_eq!(diag.code, "undefined-symbol");
        assert!(diag.notes.is_empty());
        assert!(diag.fix.is_none());
    }

    #[test]
    fn maps_arity_ranges_and_fix_through_block_mappings() {
        // Document: heading, blank line, then an r code block on line 4.
        let original = "# Title\n\n```r\nany(is.na(x))\n```\n";
        // Concatenated file: three blank lines, then the block content.
        let linted = "\n\n\nany(is.na(x))\n";
        let mappings = vec![BlockMapping {
            concatenated_range: 3..17,
            original_range: 14..28,
            start_line: 4,
        }];
        // Offsets are relative to the concatenated file (block starts at 3).
        let output = r#"[
  {
    "rule": "any-is-na",
    "severity": "Warning",
    "path": "input.R",
    "range": { "start": 3, "end": 16 },
    "message": {
      "name": "any-is-na",
      "body": "`any(is.na(x))` is the faster, clearer `anyNA(x)`",
      "suggestion": "Use `anyNA(x)`."
    },
    "fix": {
      "content": "anyNA(x)",
      "start": 3,
      "end": 16,
      "applicability": "safe",
      "description": "Replace `any(is.na(x))` with `anyNA(x)`"
    }
  }
]"#;
        let ctx = ParseContext {
            output,
            linted_input: linted,
            original_input: original,
            mappings: Some(&mappings),
        };
        let diagnostics = ArityParser::parse(&ctx).unwrap();
        assert_eq!(diagnostics.len(), 1);

        let diag = &diagnostics[0];
        assert_eq!(diag.location.range, TextRange::new(14.into(), 27.into()));
        assert_eq!(diag.location.line, 4);
        assert_eq!(diag.location.column, 1);

        let fix = diag.fix.as_ref().expect("fix should map");
        assert_eq!(fix.safety, FixSafety::Safe);
        assert_eq!(fix.message, "Replace `any(is.na(x))` with `anyNA(x)`");
        assert_eq!(fix.edits.len(), 1);
        assert_eq!(fix.edits[0].range, TextRange::new(14.into(), 27.into()));
        assert_eq!(fix.edits[0].replacement, "anyNA(x)");
    }

    #[test]
    fn parses_fatou_diagnostics_with_fixes_array() {
        let original = "if x == nothing\n    y = 1\nend\n";
        let mappings = vec![BlockMapping {
            concatenated_range: 0..30,
            original_range: 0..30,
            start_line: 1,
        }];
        let ctx = ParseContext {
            output: FATOU_OUTPUT,
            linted_input: original,
            original_input: original,
            mappings: Some(&mappings),
        };
        let diagnostics = FatouParser::parse(&ctx).unwrap();
        assert_eq!(diagnostics.len(), 1);

        let diag = &diagnostics[0];
        assert_eq!(diag.code, "nothing-comparison");
        assert_eq!(diag.severity, Severity::Warning);
        assert!(diag.notes.is_empty());
        assert_eq!(diag.location.range, TextRange::new(3.into(), 15.into()));

        let fix = diag.fix.as_ref().expect("fix from fixes array");
        assert_eq!(fix.message, "Replace `==` with `===`");
        assert_eq!(fix.edits[0].range, TextRange::new(5.into(), 7.into()));
        assert_eq!(fix.edits[0].replacement, "===");
    }

    #[test]
    fn empty_fixes_array_yields_no_fix() {
        let input = "import Printf\nx = 1\n";
        let output = r#"[
  {
    "rule": "unused-import",
    "severity": "warning",
    "path": "input.jl",
    "range": { "start": 7, "end": 13 },
    "message": {
      "name": "unused-import",
      "body": "`Printf` is imported but never used",
      "suggestion": null
    },
    "fixes": []
  }
]"#;
        let mappings = vec![BlockMapping {
            concatenated_range: 0..20,
            original_range: 0..20,
            start_line: 1,
        }];
        let ctx = ParseContext {
            output,
            linted_input: input,
            original_input: input,
            mappings: Some(&mappings),
        };
        let diagnostics = FatouParser::parse(&ctx).unwrap();
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].fix.is_none());
    }

    #[test]
    fn maps_severities_case_insensitively() {
        let input = "x\n";
        for (severity, expected) in [
            ("Error", Severity::Error),
            ("error", Severity::Error),
            ("Warning", Severity::Warning),
            ("warning", Severity::Warning),
            ("Info", Severity::Info),
            ("info", Severity::Info),
            ("Hint", Severity::Info),
            ("hint", Severity::Info),
        ] {
            let output = format!(
                r#"[{{"rule":"r","severity":"{}","range":{{"start":0,"end":1}},"message":{{"name":"r","body":"b","suggestion":null}}}}]"#,
                severity
            );
            let ctx = ParseContext {
                output: &output,
                linted_input: input,
                original_input: input,
                mappings: None,
            };
            let diagnostics = ArityParser::parse(&ctx).unwrap();
            assert_eq!(diagnostics[0].severity, expected, "severity {}", severity);
        }
    }

    #[test]
    fn unsafe_applicability_yields_unsafe_fix() {
        let input = "x <- 1\n";
        let output = r#"[
  {
    "rule": "some-rule",
    "severity": "warning",
    "range": { "start": 0, "end": 1 },
    "message": { "name": "some-rule", "body": "b", "suggestion": null },
    "fix": {
      "content": "y",
      "start": 0,
      "end": 1,
      "applicability": "unsafe",
      "description": "Rename `x` to `y`"
    }
  }
]"#;
        let mappings = vec![BlockMapping {
            concatenated_range: 0..7,
            original_range: 0..7,
            start_line: 1,
        }];
        let ctx = ParseContext {
            output,
            linted_input: input,
            original_input: input,
            mappings: Some(&mappings),
        };
        let diagnostics = ArityParser::parse(&ctx).unwrap();
        let fix = diagnostics[0].fix.as_ref().expect("fix expected");
        assert_eq!(fix.safety, FixSafety::Unsafe);
    }

    #[test]
    fn drops_unmappable_fix_but_keeps_diagnostic() {
        let original = "# Title\n\n```r\nany(is.na(x))\n```\n";
        let linted = "\n\n\nany(is.na(x))\n";
        let mappings = vec![BlockMapping {
            concatenated_range: 3..17,
            original_range: 14..28,
            start_line: 4,
        }];
        // Fix offsets far outside every mapping.
        let output = r#"[
  {
    "rule": "some-rule",
    "severity": "warning",
    "range": { "start": 3, "end": 16 },
    "message": { "name": "some-rule", "body": "b", "suggestion": null },
    "fix": {
      "content": "anyNA(x)",
      "start": 100,
      "end": 113,
      "applicability": "safe",
      "description": "d"
    }
  }
]"#;
        let ctx = ParseContext {
            output,
            linted_input: linted,
            original_input: original,
            mappings: Some(&mappings),
        };
        let diagnostics = ArityParser::parse(&ctx).unwrap();
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].fix.is_none());
        assert_eq!(
            diagnostics[0].location.range,
            TextRange::new(14.into(), 27.into())
        );
    }

    // Captured from `badness lint --no-config --output json` on
    // `Wait ... what\n` (path key omitted here; the parser ignores it).
    const BADNESS_OUTPUT: &str = r#"[
  {
    "rule": "ellipsis",
    "severity": "warning",
    "path": "input.tex",
    "start": 5,
    "end": 8,
    "message": "literal `...` ellipsis; use `\\dots`",
    "fix": {
      "edits": [
        {
          "content": "\\dots",
          "start": 5,
          "end": 8
        }
      ],
      "applicability": "safe",
      "description": "Replace `...` with `\\dots`"
    },
    "related": []
  }
]"#;

    #[test]
    fn parses_badness_diagnostics_and_maps_fix() {
        // Document: heading, blank line, then a latex code block on line 4.
        let original = "# Title\n\n```latex\nWait ... what\n```\n";
        let linted = "\n\n\nWait ... what\n";
        let mappings = vec![BlockMapping {
            concatenated_range: 3..17,
            original_range: 18..32,
            start_line: 4,
        }];
        // Offsets shifted into the concatenated file (block starts at 3).
        let output = BADNESS_OUTPUT
            .replace(r#""start": 5"#, r#""start": 8"#)
            .replace(r#""end": 8"#, r#""end": 11"#);
        let ctx = ParseContext {
            output: &output,
            linted_input: linted,
            original_input: original,
            mappings: Some(&mappings),
        };
        let diagnostics = BadnessParser::parse(&ctx).unwrap();
        assert_eq!(diagnostics.len(), 1);

        let diag = &diagnostics[0];
        assert_eq!(diag.code, "ellipsis");
        assert_eq!(diag.severity, Severity::Warning);
        assert_eq!(diag.origin, DiagnosticOrigin::External);
        assert_eq!(diag.message, "literal `...` ellipsis; use `\\dots`");
        assert_eq!(diag.location.range, TextRange::new(23.into(), 26.into()));
        assert_eq!(diag.location.line, 4);
        assert_eq!(diag.location.column, 6);

        let fix = diag.fix.as_ref().expect("fix should map");
        assert_eq!(fix.safety, FixSafety::Safe);
        assert_eq!(fix.message, "Replace `...` with `\\dots`");
        assert_eq!(fix.edits.len(), 1);
        assert_eq!(fix.edits[0].range, TextRange::new(23.into(), 26.into()));
        assert_eq!(fix.edits[0].replacement, "\\dots");
    }

    #[test]
    fn badness_multi_edit_fix_maps_all_edits() {
        let input = "aa bb\n";
        let output = r#"[
  {
    "rule": "some-rule",
    "severity": "warning",
    "path": "input.tex",
    "start": 0,
    "end": 5,
    "message": "m",
    "fix": {
      "edits": [
        { "content": "x", "start": 0, "end": 2 },
        { "content": "y", "start": 3, "end": 5 }
      ],
      "applicability": "unsafe",
      "description": "d"
    },
    "related": []
  }
]"#;
        let mappings = vec![BlockMapping {
            concatenated_range: 0..6,
            original_range: 0..6,
            start_line: 1,
        }];
        let ctx = ParseContext {
            output,
            linted_input: input,
            original_input: input,
            mappings: Some(&mappings),
        };
        let diagnostics = BadnessParser::parse(&ctx).unwrap();
        let fix = diagnostics[0].fix.as_ref().expect("fix expected");
        assert_eq!(fix.safety, FixSafety::Unsafe);
        assert_eq!(fix.edits.len(), 2);
        assert_eq!(fix.edits[1].range, TextRange::new(3.into(), 5.into()));
        assert_eq!(fix.edits[1].replacement, "y");
    }

    #[test]
    fn badness_cross_file_fix_is_skipped() {
        let input = "aa bb\n";
        let output = r#"[
  {
    "rule": "some-rule",
    "severity": "warning",
    "path": "input.tex",
    "start": 0,
    "end": 5,
    "message": "m",
    "fix": {
      "edits": [
        { "content": "x", "start": 0, "end": 2 },
        { "content": "y", "start": 0, "end": 2, "path": "other.tex" }
      ],
      "applicability": "safe",
      "description": "d"
    },
    "related": []
  }
]"#;
        let mappings = vec![BlockMapping {
            concatenated_range: 0..6,
            original_range: 0..6,
            start_line: 1,
        }];
        let ctx = ParseContext {
            output,
            linted_input: input,
            original_input: input,
            mappings: Some(&mappings),
        };
        let diagnostics = BadnessParser::parse(&ctx).unwrap();
        assert_eq!(diagnostics.len(), 1);
        assert!(
            diagnostics[0].fix.is_none(),
            "cross-file fixes cannot apply inside a code block"
        );
    }

    #[test]
    fn malformed_json_is_a_parse_error() {
        let ctx = ParseContext {
            output: "not json",
            linted_input: "x\n",
            original_input: "x\n",
            mappings: None,
        };
        let result = ArityParser::parse(&ctx);
        assert!(matches!(result, Err(LinterError::ParseError(_))));
    }
}

use tower_lsp_server::ls_types::*;

use crate::linter;
use crate::linter::Severity as PanacheSeverity;

/// Helper to convert LSP UTF-16 position to byte offset in UTF-8 string
pub(crate) fn position_to_offset(text: &str, position: Position) -> Option<usize> {
    let mut offset = 0;
    let mut current_line = 0;

    for line in text.lines() {
        if current_line == position.line {
            // LSP uses UTF-16 code units, Rust uses UTF-8 bytes
            let mut utf16_offset = 0;
            for (byte_idx, ch) in line.char_indices() {
                if utf16_offset >= position.character as usize {
                    return Some(offset + byte_idx);
                }
                utf16_offset += ch.len_utf16();
            }
            // Position is at or past end of line
            return Some(offset + line.len());
        }
        // +1 for newline character
        offset += line.len() + 1;
        current_line += 1;
    }

    // Position is beyond document end
    if current_line == position.line {
        // Empty last line or position at very end
        return Some(offset);
    }

    None
}

/// Convert byte offset to LSP Position (line/character in UTF-16)
pub(crate) fn offset_to_position(text: &str, offset: usize) -> Position {
    let mut line = 0;
    let mut character = 0;
    let mut current_offset = 0;

    for text_line in text.lines() {
        if current_offset + text_line.len() >= offset {
            // Offset is in this line
            let line_offset = offset - current_offset;
            let line_slice = &text_line[..line_offset];
            character = line_slice.chars().map(|c| c.len_utf16()).sum::<usize>() as u32;
            break;
        }
        current_offset += text_line.len() + 1; // +1 for newline
        line += 1;
    }

    Position {
        line: line as u32,
        character,
    }
}

/// Convert panache Diagnostic to LSP Diagnostic
pub(crate) fn convert_diagnostic(diag: &linter::Diagnostic, text: &str) -> Diagnostic {
    let start = offset_to_position(text, diag.location.range.start().into());
    let end = offset_to_position(text, diag.location.range.end().into());

    let severity = match diag.severity {
        PanacheSeverity::Error => DiagnosticSeverity::ERROR,
        PanacheSeverity::Warning => DiagnosticSeverity::WARNING,
        PanacheSeverity::Info => DiagnosticSeverity::INFORMATION,
    };

    Diagnostic {
        range: Range { start, end },
        severity: Some(severity),
        code: Some(NumberOrString::String(diag.code.clone())),
        source: Some("panache".to_string()),
        message: diag.message.clone(),
        ..Default::default()
    }
}

/// Apply a single content change to text
pub(crate) fn apply_content_change(text: &str, change: &TextDocumentContentChangeEvent) -> String {
    match &change.range {
        Some(range) => {
            // Incremental edit with range
            let start_offset = position_to_offset(text, range.start).unwrap_or(0);
            let end_offset = position_to_offset(text, range.end).unwrap_or(text.len());

            let mut result =
                String::with_capacity(text.len() - (end_offset - start_offset) + change.text.len());
            result.push_str(&text[..start_offset]);
            result.push_str(&change.text);
            result.push_str(&text[end_offset..]);
            result
        }
        None => {
            // Full document update (fallback)
            change.text.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_offset_to_position_simple() {
        let text = "hello\nworld\n";

        let pos = offset_to_position(text, 0);
        assert_eq!(pos.line, 0);
        assert_eq!(pos.character, 0);

        let pos = offset_to_position(text, 3);
        assert_eq!(pos.line, 0);
        assert_eq!(pos.character, 3);

        let pos = offset_to_position(text, 6);
        assert_eq!(pos.line, 1);
        assert_eq!(pos.character, 0);

        let pos = offset_to_position(text, 9);
        assert_eq!(pos.line, 1);
        assert_eq!(pos.character, 3);
    }

    #[test]
    fn test_offset_to_position_utf16() {
        // "café" = 5 UTF-8 bytes, 4 UTF-16 code units
        let text = "café\n";

        let pos = offset_to_position(text, 0);
        assert_eq!(pos.character, 0);

        let pos = offset_to_position(text, 3);
        assert_eq!(pos.character, 3);

        // After é (2 UTF-8 bytes, but 1 UTF-16 code unit)
        let pos = offset_to_position(text, 5);
        assert_eq!(pos.character, 4);
    }

    #[test]
    fn test_offset_to_position_emoji() {
        // "👋" = 4 UTF-8 bytes, 2 UTF-16 code units (surrogate pair)
        let text = "hi👋\n";

        let pos = offset_to_position(text, 2);
        assert_eq!(pos.character, 2);

        // After emoji (6 bytes total, 4 UTF-16 code units)
        let pos = offset_to_position(text, 6);
        assert_eq!(pos.character, 4);
    }

    #[test]
    fn test_convert_diagnostic_basic() {
        use crate::linter::diagnostics::{Diagnostic as PanacheDiagnostic, Location, Severity};
        use rowan::TextRange;

        let text = "# H1\n\n### H3\n";

        let diag = PanacheDiagnostic {
            severity: Severity::Warning,
            location: Location {
                line: 3,
                column: 1,
                range: TextRange::new(7.into(), 14.into()),
            },
            message: "Heading level skipped from h1 to h3".to_string(),
            code: "heading-hierarchy".to_string(),
            fix: None,
        };

        let lsp_diag = convert_diagnostic(&diag, text);

        assert_eq!(lsp_diag.severity, Some(DiagnosticSeverity::WARNING));
        assert_eq!(
            lsp_diag.code,
            Some(NumberOrString::String("heading-hierarchy".to_string()))
        );
        assert_eq!(lsp_diag.source, Some("panache".to_string()));
        assert!(lsp_diag.message.contains("h1 to h3"));

        // Verify range conversion
        assert_eq!(lsp_diag.range.start.line, 2); // Line 3 in text becomes line 2 (0-indexed)
    }

    #[test]
    fn test_convert_diagnostic_severity() {
        use crate::linter::diagnostics::{Diagnostic as PanacheDiagnostic, Location, Severity};
        use rowan::TextRange;

        let text = "test\n";

        let error_diag = PanacheDiagnostic {
            severity: Severity::Error,
            location: Location {
                line: 1,
                column: 1,
                range: TextRange::new(0.into(), 4.into()),
            },
            message: "Error".to_string(),
            code: "test-error".to_string(),
            fix: None,
        };

        let lsp_diag = convert_diagnostic(&error_diag, text);
        assert_eq!(lsp_diag.severity, Some(DiagnosticSeverity::ERROR));

        let info_diag = PanacheDiagnostic {
            severity: Severity::Info,
            location: Location {
                line: 1,
                column: 1,
                range: TextRange::new(0.into(), 4.into()),
            },
            message: "Info".to_string(),
            code: "test-info".to_string(),
            fix: None,
        };

        let lsp_diag = convert_diagnostic(&info_diag, text);
        assert_eq!(lsp_diag.severity, Some(DiagnosticSeverity::INFORMATION));
    }

    #[test]
    fn test_position_to_offset_simple() {
        let text = "hello\nworld\n";

        // Start of first line
        assert_eq!(
            position_to_offset(
                text,
                Position {
                    line: 0,
                    character: 0
                }
            ),
            Some(0)
        );

        // Middle of first line
        assert_eq!(
            position_to_offset(
                text,
                Position {
                    line: 0,
                    character: 3
                }
            ),
            Some(3)
        );

        // End of first line
        assert_eq!(
            position_to_offset(
                text,
                Position {
                    line: 0,
                    character: 5
                }
            ),
            Some(5)
        );

        // Start of second line
        assert_eq!(
            position_to_offset(
                text,
                Position {
                    line: 1,
                    character: 0
                }
            ),
            Some(6)
        );

        // Middle of second line
        assert_eq!(
            position_to_offset(
                text,
                Position {
                    line: 1,
                    character: 3
                }
            ),
            Some(9)
        );
    }

    #[test]
    fn test_position_to_offset_utf8() {
        // "café" = 5 UTF-8 bytes, 4 UTF-16 code units (é = 2 bytes, 1 code unit)
        let text = "café\nworld\n";

        // Start of line
        assert_eq!(
            position_to_offset(
                text,
                Position {
                    line: 0,
                    character: 0
                }
            ),
            Some(0)
        );

        // After 'c' (1 byte, 1 UTF-16)
        assert_eq!(
            position_to_offset(
                text,
                Position {
                    line: 0,
                    character: 1
                }
            ),
            Some(1)
        );

        // After 'ca' (2 bytes, 2 UTF-16)
        assert_eq!(
            position_to_offset(
                text,
                Position {
                    line: 0,
                    character: 2
                }
            ),
            Some(2)
        );

        // After 'caf' (3 bytes, 3 UTF-16)
        assert_eq!(
            position_to_offset(
                text,
                Position {
                    line: 0,
                    character: 3
                }
            ),
            Some(3)
        );

        // After 'café' (5 bytes, 4 UTF-16)
        assert_eq!(
            position_to_offset(
                text,
                Position {
                    line: 0,
                    character: 4
                }
            ),
            Some(5)
        );
    }

    #[test]
    fn test_position_to_offset_emoji() {
        // "👋" = 4 UTF-8 bytes, 2 UTF-16 code units (surrogate pair)
        let text = "hi👋\n";

        // After "hi" (2 bytes, 2 UTF-16)
        assert_eq!(
            position_to_offset(
                text,
                Position {
                    line: 0,
                    character: 2
                }
            ),
            Some(2)
        );

        // After "hi👋" (6 bytes, 4 UTF-16)
        assert_eq!(
            position_to_offset(
                text,
                Position {
                    line: 0,
                    character: 4
                }
            ),
            Some(6)
        );
    }

    #[test]
    fn test_apply_content_change_insert() {
        let text = "hello world";
        let change = TextDocumentContentChangeEvent {
            range: Some(Range {
                start: Position {
                    line: 0,
                    character: 6,
                },
                end: Position {
                    line: 0,
                    character: 6,
                },
            }),
            range_length: None,
            text: "beautiful ".to_string(),
        };

        assert_eq!(apply_content_change(text, &change), "hello beautiful world");
    }

    #[test]
    fn test_apply_content_change_delete() {
        let text = "hello beautiful world";
        let change = TextDocumentContentChangeEvent {
            range: Some(Range {
                start: Position {
                    line: 0,
                    character: 5,
                },
                end: Position {
                    line: 0,
                    character: 15,
                },
            }),
            range_length: None,
            text: String::new(),
        };

        assert_eq!(apply_content_change(text, &change), "hello world");
    }

    #[test]
    fn test_apply_content_change_replace() {
        let text = "hello world";
        let change = TextDocumentContentChangeEvent {
            range: Some(Range {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: 0,
                    character: 5,
                },
            }),
            range_length: None,
            text: "goodbye".to_string(),
        };

        assert_eq!(apply_content_change(text, &change), "goodbye world");
    }

    #[test]
    fn test_apply_content_change_full_document() {
        let text = "old content";
        let change = TextDocumentContentChangeEvent {
            range: None,
            range_length: None,
            text: "new content".to_string(),
        };

        assert_eq!(apply_content_change(text, &change), "new content");
    }

    #[test]
    fn test_apply_content_change_multiline() {
        let text = "line1\nline2\nline3";
        let change = TextDocumentContentChangeEvent {
            range: Some(Range {
                start: Position {
                    line: 1,
                    character: 2,
                },
                end: Position {
                    line: 2,
                    character: 2,
                },
            }),
            range_length: None,
            text: "NEW\nLINE".to_string(),
        };

        assert_eq!(apply_content_change(text, &change), "line1\nliNEW\nLINEne3");
    }
}

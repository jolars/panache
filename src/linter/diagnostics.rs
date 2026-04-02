use rowan::TextRange;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticOrigin {
    BuiltIn,
    External,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
    pub line: usize,
    pub column: usize,
    pub range: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    pub range: TextRange,
    pub replacement: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fix {
    pub message: String,
    pub edits: Vec<Edit>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticNoteKind {
    Note,
    Help,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticNote {
    pub kind: DiagnosticNoteKind,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: Severity,
    pub location: Location,
    pub message: String,
    pub code: String,
    pub origin: DiagnosticOrigin,
    pub notes: Vec<DiagnosticNote>,
    pub fix: Option<Fix>,
}

impl Diagnostic {
    pub fn error(location: Location, code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Error,
            location,
            message: message.into(),
            code: code.into(),
            origin: DiagnosticOrigin::BuiltIn,
            notes: Vec::new(),
            fix: None,
        }
    }

    pub fn warning(
        location: Location,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            severity: Severity::Warning,
            location,
            message: message.into(),
            code: code.into(),
            origin: DiagnosticOrigin::BuiltIn,
            notes: Vec::new(),
            fix: None,
        }
    }

    pub fn info(location: Location, code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Info,
            location,
            message: message.into(),
            code: code.into(),
            origin: DiagnosticOrigin::BuiltIn,
            notes: Vec::new(),
            fix: None,
        }
    }

    pub fn with_fix(mut self, fix: Fix) -> Self {
        self.fix = Some(fix);
        self
    }

    pub fn with_origin(mut self, origin: DiagnosticOrigin) -> Self {
        self.origin = origin;
        self
    }

    pub fn with_note(mut self, kind: DiagnosticNoteKind, message: impl Into<String>) -> Self {
        self.notes.push(DiagnosticNote {
            kind,
            message: message.into(),
        });
        self
    }
}

impl Location {
    pub fn from_node(node: &crate::syntax::SyntaxNode, input: &str) -> Self {
        let range = node.text_range();
        let start_offset = range.start().into();
        let (line, column) = offset_to_line_col(input, start_offset);

        Self {
            line,
            column,
            range,
        }
    }

    pub fn from_range(range: TextRange, input: &str) -> Self {
        let start_offset = range.start().into();
        let (line, column) = offset_to_line_col(input, start_offset);

        Self {
            line,
            column,
            range,
        }
    }
}

fn offset_to_line_col(input: &str, offset: usize) -> (usize, usize) {
    let mut line = 1;
    let mut column = 1;

    for (i, ch) in input.char_indices() {
        if i >= offset {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_offset_to_line_col() {
        let input = "line 1\nline 2\nline 3";

        assert_eq!(offset_to_line_col(input, 0), (1, 1)); // 'l' in line 1
        assert_eq!(offset_to_line_col(input, 6), (1, 7)); // '\n' after line 1
        assert_eq!(offset_to_line_col(input, 7), (2, 1)); // 'l' in line 2
        assert_eq!(offset_to_line_col(input, 14), (3, 1)); // 'l' in line 3
    }

    #[test]
    fn test_diagnostic_builders() {
        let location = Location {
            line: 1,
            column: 5,
            range: TextRange::new(0.into(), 10.into()),
        };

        let diag = Diagnostic::error(location.clone(), "test-error", "Test error message");
        assert_eq!(diag.severity, Severity::Error);
        assert_eq!(diag.code, "test-error");
        assert_eq!(diag.message, "Test error message");
        assert_eq!(diag.origin, DiagnosticOrigin::BuiltIn);
        assert!(diag.notes.is_empty());
        assert!(diag.fix.is_none());

        let diag_with_fix =
            Diagnostic::warning(location, "test-warning", "Test warning").with_fix(Fix {
                message: "Fix message".to_string(),
                edits: vec![],
            });
        assert_eq!(diag_with_fix.severity, Severity::Warning);
        assert_eq!(diag_with_fix.origin, DiagnosticOrigin::BuiltIn);
        assert!(diag_with_fix.notes.is_empty());
        assert!(diag_with_fix.fix.is_some());
    }

    #[test]
    fn test_with_note_adds_diagnostic_note() {
        let location = Location {
            line: 1,
            column: 1,
            range: TextRange::new(0.into(), 1.into()),
        };
        let diag = Diagnostic::warning(location, "test-warning", "msg")
            .with_note(DiagnosticNoteKind::Help, "try this");
        assert_eq!(diag.notes.len(), 1);
        assert_eq!(diag.notes[0].kind, DiagnosticNoteKind::Help);
        assert_eq!(diag.notes[0].message, "try this");
    }
}

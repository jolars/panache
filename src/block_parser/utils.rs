//! Shared utilities for block parsing.

use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;

/// Helper to emit a line's text and newline tokens separately.
/// Lines from split_inclusive contain trailing newlines that must be separated.
pub(crate) fn emit_line_tokens(builder: &mut GreenNodeBuilder<'static>, line: &str) {
    if let Some(text) = line.strip_suffix('\n') {
        builder.token(SyntaxKind::TEXT.into(), text);
        builder.token(SyntaxKind::NEWLINE.into(), "\n");
    } else {
        // No trailing newline (last line of input)
        builder.token(SyntaxKind::TEXT.into(), line);
    }
}

/// Strip up to 3 leading spaces from a line.
pub(crate) fn strip_leading_spaces(line: &str) -> &str {
    line.strip_prefix("   ")
        .or_else(|| line.strip_prefix("  "))
        .or_else(|| line.strip_prefix(" "))
        .unwrap_or(line)
}

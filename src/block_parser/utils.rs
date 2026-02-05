//! Shared utilities for block parsing.

/// Strip up to 3 leading spaces from a line.
pub(crate) fn strip_leading_spaces(line: &str) -> &str {
    line.strip_prefix("   ")
        .or_else(|| line.strip_prefix("  "))
        .or_else(|| line.strip_prefix(" "))
        .unwrap_or(line)
}

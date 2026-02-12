//! Chunk option value classification for Quarto/RMarkdown code blocks.
//!
//! This module distinguishes between simple literal values (booleans, numbers, strings)
//! and complex R expressions (function calls, variables, etc.) to determine which
//! chunk options can be safely converted to hashpipe format.

/// Classification of chunk option values for conversion to hashpipe format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChunkOptionValue {
    /// Simple literal value that can be safely converted to YAML syntax.
    /// Examples: TRUE, FALSE, 7, "string"
    Simple(String),

    /// Complex R expression that should stay in inline format.
    /// Examples: paste(...), my_var, nrow(data)
    Expression(String),
}

/// Classify a chunk option value as either simple (convertible) or expression (skip).
///
/// Conservative approach: only classify as Simple if we're certain it's a literal.
/// When in doubt, classify as Expression to avoid breaking R code.
///
/// **Note**: The parser strips quotes from values, so we receive the inner string.
/// For `label="my chunk"`, value is `"my chunk"` (no quotes).
pub fn classify_value(value: &Option<String>) -> ChunkOptionValue {
    match value {
        None => ChunkOptionValue::Simple(String::new()), // Bare flag like `echo` is treated as true
        Some(v) => {
            // Parser strips quotes, so we get the inner value
            // Check if it looks like an R expression
            if is_boolean_literal(v) || is_numeric_literal(v) || is_simple_string(v) {
                ChunkOptionValue::Simple(v.clone())
            } else {
                ChunkOptionValue::Expression(v.clone())
            }
        }
    }
}

/// Check if a string value is simple enough to be safely formatted.
///
/// Returns false for strings that look like R expressions (function calls, operators, variables).
fn is_simple_string(s: &str) -> bool {
    // Empty strings are simple
    if s.is_empty() {
        return true;
    }

    // If it contains R expression characters, it's complex
    if s.contains('(')
        || s.contains(')')
        || s.contains('$')
        || s.contains('[')
        || s.contains(']')
        || s.contains('+')
        || s.contains('-')
        || s.contains('*')
        || s.contains('/')
        || s.contains('<')
        || s.contains('>')
        || s.contains('!')
    {
        return false;
    }

    // If it's a single bareword (could be a variable), it's complex
    // unless it contains spaces or special chars (then it's a string literal)
    if !s.contains(' ')
        && !s.contains('.')
        && !s.contains('/')
        && !s.contains('\\')
        && !s.contains(',')
        && s.chars().all(|c| c.is_alphanumeric() || c == '_')
    {
        // Looks like a variable name
        return false;
    }

    // Otherwise, treat as simple string (phrases, paths with dots/slashes)
    true
}

/// Check if a string is an R boolean literal.
///
/// Accepts: TRUE, FALSE, T, F (R's boolean constants)
pub fn is_boolean_literal(s: &str) -> bool {
    matches!(s, "TRUE" | "FALSE" | "T" | "F")
}

/// Check if a string is a numeric literal.
///
/// Accepts: integers (7, -3) and floats (3.14, -2.5, 1e-5)
pub fn is_numeric_literal(s: &str) -> bool {
    // Try parsing as f64 to catch integers and floats
    s.parse::<f64>().is_ok()
}

/// Check if a string is a quoted string literal.
///
/// Accepts both single and double quoted strings.
/// Does not validate escape sequences - just checks for matching quotes.
pub fn is_quoted_string(s: &str) -> bool {
    (s.starts_with('"') && s.ends_with('"') && s.len() >= 2)
        || (s.starts_with('\'') && s.ends_with('\'') && s.len() >= 2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_boolean_literal() {
        assert!(is_boolean_literal("TRUE"));
        assert!(is_boolean_literal("FALSE"));
        assert!(is_boolean_literal("T"));
        assert!(is_boolean_literal("F"));

        assert!(!is_boolean_literal("true"));
        assert!(!is_boolean_literal("false"));
        assert!(!is_boolean_literal("True"));
        assert!(!is_boolean_literal("MAYBE"));
    }

    #[test]
    fn test_is_numeric_literal() {
        // Integers
        assert!(is_numeric_literal("7"));
        assert!(is_numeric_literal("0"));
        assert!(is_numeric_literal("-3"));
        assert!(is_numeric_literal("100"));

        // Floats
        assert!(is_numeric_literal("3.14"));
        assert!(is_numeric_literal("-2.5"));
        assert!(is_numeric_literal("0.1"));

        // Scientific notation
        assert!(is_numeric_literal("1e5"));
        assert!(is_numeric_literal("1.5e-3"));

        // Not numeric
        assert!(!is_numeric_literal("abc"));
        assert!(!is_numeric_literal("7x"));
        assert!(!is_numeric_literal(""));
    }

    #[test]
    fn test_is_quoted_string() {
        // Double quotes
        assert!(is_quoted_string("\"hello\""));
        assert!(is_quoted_string("\"with spaces\""));
        assert!(is_quoted_string("\"\""));

        // Single quotes
        assert!(is_quoted_string("'hello'"));
        assert!(is_quoted_string("'with spaces'"));
        assert!(is_quoted_string("''"));

        // Not quoted
        assert!(!is_quoted_string("hello"));
        assert!(!is_quoted_string("\""));
        assert!(!is_quoted_string("'"));
        assert!(!is_quoted_string("\"hello'"));
        assert!(!is_quoted_string("'hello\""));
        assert!(!is_quoted_string(""));
    }

    #[test]
    fn test_classify_boolean() {
        let result = classify_value(&Some("TRUE".to_string()));
        assert_eq!(result, ChunkOptionValue::Simple("TRUE".to_string()));

        let result = classify_value(&Some("FALSE".to_string()));
        assert_eq!(result, ChunkOptionValue::Simple("FALSE".to_string()));
    }

    #[test]
    fn test_classify_number() {
        let result = classify_value(&Some("7".to_string()));
        assert_eq!(result, ChunkOptionValue::Simple("7".to_string()));

        let result = classify_value(&Some("3.14".to_string()));
        assert_eq!(result, ChunkOptionValue::Simple("3.14".to_string()));
    }

    #[test]
    fn test_classify_quoted_string() {
        let result = classify_value(&Some("\"hello\"".to_string()));
        assert_eq!(result, ChunkOptionValue::Simple("\"hello\"".to_string()));

        let result = classify_value(&Some("'world'".to_string()));
        assert_eq!(result, ChunkOptionValue::Simple("'world'".to_string()));
    }

    #[test]
    fn test_classify_function_call() {
        let result = classify_value(&Some("paste(\"a\", \"b\")".to_string()));
        assert_eq!(
            result,
            ChunkOptionValue::Expression("paste(\"a\", \"b\")".to_string())
        );
    }

    #[test]
    fn test_classify_variable() {
        let result = classify_value(&Some("my_var".to_string()));
        assert_eq!(result, ChunkOptionValue::Expression("my_var".to_string()));
    }

    #[test]
    fn test_classify_none() {
        let result = classify_value(&None);
        assert_eq!(result, ChunkOptionValue::Simple(String::new()));
    }

    #[test]
    fn test_classify_expression_with_operators() {
        let result = classify_value(&Some("x + y".to_string()));
        assert_eq!(result, ChunkOptionValue::Expression("x + y".to_string()));

        let result = classify_value(&Some("data$col".to_string()));
        assert_eq!(result, ChunkOptionValue::Expression("data$col".to_string()));

        let result = classify_value(&Some("vec[1]".to_string()));
        assert_eq!(result, ChunkOptionValue::Expression("vec[1]".to_string()));
    }
}

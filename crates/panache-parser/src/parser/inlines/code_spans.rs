use super::sink::InlineSink;
use crate::parser::utils::helpers::backtick_run;
/// Parsing for inline code spans (`code`)
use crate::syntax::SyntaxKind;

use crate::parser::utils::attributes::{
    AttributeBlock, emit_attribute_node, try_parse_trailing_attributes,
};

type CodeSpanAttributes<'a> = (AttributeBlock, &'a str);

/// Try to parse a code span starting at the current position.
/// Returns `(total_len, code_content, backtick_count, attributes)` if
/// successful. When trailing attributes are present, `attributes` carries both
/// the parsed [`AttributeBlock`] (for raw-inline / extension-gating decisions)
/// and the raw `{...}` source slice (for lossless structured emission).
pub fn try_parse_code_span(
    text: &str,
) -> Option<(usize, &str, usize, Option<CodeSpanAttributes<'_>>)> {
    let opening_backticks = text.bytes().take_while(|&b| b == b'`').count();
    if opening_backticks == 0 {
        return None;
    }

    let rest = &text[opening_backticks..];
    let rest_bytes = rest.as_bytes();

    let mut pos = 0;
    while pos < rest_bytes.len() {
        let next_tick = match rest_bytes[pos..].iter().position(|&b| b == b'`') {
            Some(off) => pos + off,
            None => break,
        };
        let mut closing_backticks = 0;
        while next_tick + closing_backticks < rest_bytes.len()
            && rest_bytes[next_tick + closing_backticks] == b'`'
        {
            closing_backticks += 1;
        }

        if closing_backticks == opening_backticks {
            let code_content = &rest[..next_tick];
            let after_close = opening_backticks + next_tick + closing_backticks;

            let remaining = &text[after_close..];
            if remaining.starts_with('{')
                && let Some(close_brace_pos) = remaining.find('}')
            {
                let attr_text = &remaining[..=close_brace_pos];
                if let Some((attrs, _)) = try_parse_trailing_attributes(attr_text) {
                    let total_len = after_close + close_brace_pos + 1;
                    return Some((
                        total_len,
                        code_content,
                        opening_backticks,
                        Some((attrs, attr_text)),
                    ));
                }
            }

            return Some((after_close, code_content, opening_backticks, None));
        }
        pos = next_tick + closing_backticks;
    }

    None
}

/// Backtick-run lengths in `text` that are still waiting for a closer.
///
/// A run only becomes a code-span opener once a matching run turns up, and
/// pandoc reaches `endline` — where a block start can interrupt a paragraph —
/// only *between* inlines, never from inside a code span. So a run left open at
/// the end of the buffered text makes whatever later line closes it code-span
/// content rather than a block start: ```` b ```r\nc\n``` ```` is one
/// `Para [Str "b", Code "r c"]`, not a paragraph plus a fenced code block.
/// Paragraph-interrupting block detectors consult this to stay out of an open
/// span.
///
/// Runs are reported in source order; a run whose closer is already in `text`
/// is consumed along with its span and not reported.
pub fn pending_code_span_openers(text: &str) -> Vec<usize> {
    let mut pending = Vec::new();
    let mut rest = text;
    while let Some(offset) = rest.find('`') {
        rest = &rest[offset..];
        if let Some((consumed, ..)) = try_parse_code_span(rest) {
            rest = &rest[consumed..];
            continue;
        }
        let run = rest.bytes().take_while(|&b| b == b'`').count();
        pending.push(run);
        rest = &rest[run..];
    }
    pending
}

/// Emit a code span node to the builder.
pub fn emit_code_span(
    builder: &mut impl InlineSink,
    content: &str,
    backtick_count: usize,
    attr_text: Option<&str>,
) {
    builder.start_node(SyntaxKind::INLINE_CODE.into());

    builder.token(
        SyntaxKind::INLINE_CODE_MARKER.into(),
        &backtick_run(backtick_count),
    );

    builder.token(SyntaxKind::INLINE_CODE_CONTENT.into(), content);

    builder.token(
        SyntaxKind::INLINE_CODE_MARKER.into(),
        &backtick_run(backtick_count),
    );

    if let Some(raw) = attr_text {
        emit_attribute_node(builder, raw);
    }

    builder.finish_node();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_code_span() {
        let result = try_parse_code_span("`code`");
        assert_eq!(result, Some((6, "code", 1, None)));
    }

    #[test]
    fn test_parse_code_span_with_backticks() {
        let result = try_parse_code_span("`` `backtick` ``");
        assert_eq!(result, Some((16, " `backtick` ", 2, None)));
    }

    #[test]
    fn test_parse_code_span_triple_backticks() {
        let result = try_parse_code_span("``` `` ```");
        assert_eq!(result, Some((10, " `` ", 3, None)));
    }

    #[test]
    fn test_parse_code_span_no_close() {
        let result = try_parse_code_span("`no close");
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_code_span_mismatched_close() {
        let result = try_parse_code_span("`single``");
        assert_eq!(result, None);
    }

    #[test]
    fn test_not_code_span() {
        let result = try_parse_code_span("no backticks");
        assert_eq!(result, None);
    }

    #[test]
    fn test_code_span_with_trailing_text() {
        let result = try_parse_code_span("`code` and more");
        assert_eq!(result, Some((6, "code", 1, None)));
    }

    #[test]
    fn test_code_span_with_simple_class() {
        let result = try_parse_code_span("`code`{.python}");
        let (len, content, backticks, attrs) = result.unwrap();
        assert_eq!(len, 15);
        assert_eq!(content, "code");
        assert_eq!(backticks, 1);
        assert!(attrs.is_some());
        let (attrs, raw) = attrs.unwrap();
        assert_eq!(attrs.classes, vec!["python"]);
        assert_eq!(raw, "{.python}");
    }

    #[test]
    fn test_code_span_with_id() {
        let result = try_parse_code_span("`code`{#mycode}");
        let (len, content, backticks, attrs) = result.unwrap();
        assert_eq!(len, 15);
        assert_eq!(content, "code");
        assert_eq!(backticks, 1);
        assert!(attrs.is_some());
        let (attrs, _raw) = attrs.unwrap();
        assert_eq!(attrs.identifier, Some("mycode".to_string()));
    }

    #[test]
    fn test_code_span_with_full_attributes() {
        let result = try_parse_code_span("`x + y`{#calc .haskell .eval}");
        let (len, content, backticks, attrs) = result.unwrap();
        assert_eq!(len, 29);
        assert_eq!(content, "x + y");
        assert_eq!(backticks, 1);
        assert!(attrs.is_some());
        let (attrs, _raw) = attrs.unwrap();
        assert_eq!(attrs.identifier, Some("calc".to_string()));
        assert_eq!(attrs.classes, vec!["haskell", "eval"]);
    }

    #[test]
    fn test_code_span_attributes_must_be_adjacent() {
        let result = try_parse_code_span("`code` {.python}");
        assert_eq!(result, Some((6, "code", 1, None)));
    }
}

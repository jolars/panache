//! Svelte template parsing (mdsvex).
//!
//! mdsvex documents embed Svelte's template syntax inside CommonMark prose.
//! Panache treats these constructs as **opaque, lossless spans**: the bytes
//! between the outermost braces are preserved verbatim and never re-parsed.
//! Only the *category* of the span is recorded, by leading sigil:
//!
//! - `{#if}` / `{:else}` / `{/each}` → [`SvelteKind::BlockLogic`]
//! - `{@html ...}` / `{@const ...}` / `{@debug ...}` → [`SvelteKind::Tag`]
//! - `{expr}` interpolation → [`SvelteKind::Expression`]
//!
//! Known MVP limitations (acceptable; both fall back to a literal `{`, so they
//! stay lossless):
//! - Brace matching is depth-counted, not string-literal-aware, so a `}` inside
//!   a JS string (`{ "}" }`) can terminate the span early.
//! - A span whose braces straddle a blank line (paragraph break) is not matched
//!   here, since inline parsing runs per block.

use super::sink::InlineSink;
use crate::syntax::SyntaxKind;

/// The category of a Svelte template span, by leading sigil.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SvelteKind {
    /// `{#if}`, `{:else}`, `{/each}`, ... (sigils `#`, `:`, `/`).
    BlockLogic,
    /// `{@html ...}`, `{@const ...}`, `{@debug ...}` (sigil `@`).
    Tag,
    /// `{expr}` interpolation (no logic/tag sigil).
    Expression,
}

impl SvelteKind {
    fn parent(self) -> SyntaxKind {
        match self {
            SvelteKind::BlockLogic => SyntaxKind::SVELTE_BLOCK_LOGIC,
            SvelteKind::Tag => SyntaxKind::SVELTE_TAG,
            SvelteKind::Expression => SyntaxKind::SVELTE_EXPRESSION,
        }
    }
}

/// Try to parse a Svelte template span starting at the current position.
///
/// Returns `(total_length, kind, content)` where `content` is the verbatim
/// bytes *between* the outer braces (the braces themselves are excluded).
/// Returns `None` if `text` does not start with a balanced `{...}` span, or if
/// it starts with `{{<` (left to the Quarto shortcode probe).
pub(crate) fn try_parse_svelte_template(text: &str) -> Option<(usize, SvelteKind, String)> {
    let bytes = text.as_bytes();

    // Must open with a single `{`.
    if bytes.first() != Some(&b'{') {
        return None;
    }

    // Leave `{{<` (Quarto shortcode opener) to the shortcode probe.
    if bytes.len() >= 3 && bytes[1] == b'{' && bytes[2] == b'<' {
        return None;
    }

    // Scan to the matching `}` at depth 0.
    let mut depth: i32 = 0;
    let mut pos = 0;
    let mut close = None;
    while pos < bytes.len() {
        match bytes[pos] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    close = Some(pos);
                    break;
                }
            }
            _ => {}
        }
        pos += 1;
    }

    let close = close?;
    let content = &text[1..close];
    let kind = classify(content);
    let total_len = close + 1;

    Some((total_len, kind, content.to_string()))
}

/// Classify a span by its first non-whitespace content byte.
fn classify(content: &str) -> SvelteKind {
    match content.trim_start().as_bytes().first() {
        Some(b'#') | Some(b':') | Some(b'/') => SvelteKind::BlockLogic,
        Some(b'@') => SvelteKind::Tag,
        _ => SvelteKind::Expression,
    }
}

/// Emit a Svelte template span as an opaque CST subtree.
pub(crate) fn emit_svelte_template(builder: &mut impl InlineSink, kind: SvelteKind, content: &str) {
    builder.start_node(kind.parent().into());

    builder.token(SyntaxKind::SVELTE_MARKER_OPEN.into(), "{");

    builder.start_node(SyntaxKind::SVELTE_CONTENT.into());
    if !content.is_empty() {
        builder.token(SyntaxKind::TEXT.into(), content);
    }
    builder.finish_node(); // SVELTE_CONTENT

    builder.token(SyntaxKind::SVELTE_MARKER_CLOSE.into(), "}");

    builder.finish_node(); // parent
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_expression() {
        let (len, kind, content) = try_parse_svelte_template("{count}").unwrap();
        assert_eq!(len, 7);
        assert_eq!(kind, SvelteKind::Expression);
        assert_eq!(content, "count");
    }

    #[test]
    fn classifies_block_logic() {
        let (_, kind, content) = try_parse_svelte_template("{#if active}rest").unwrap();
        assert_eq!(kind, SvelteKind::BlockLogic);
        assert_eq!(content, "#if active");
    }

    #[test]
    fn classifies_else_and_close() {
        assert_eq!(
            try_parse_svelte_template("{:else}").unwrap().1,
            SvelteKind::BlockLogic
        );
        assert_eq!(
            try_parse_svelte_template("{/each}").unwrap().1,
            SvelteKind::BlockLogic
        );
    }

    #[test]
    fn classifies_tag() {
        let (_, kind, content) = try_parse_svelte_template("{@html body}").unwrap();
        assert_eq!(kind, SvelteKind::Tag);
        assert_eq!(content, "@html body");
    }

    #[test]
    fn handles_nested_braces_in_expression() {
        // Object literal: the inner braces must not terminate the span early.
        let (len, kind, content) = try_parse_svelte_template("{ {a: 1} }rest").unwrap();
        assert_eq!(kind, SvelteKind::Expression);
        assert_eq!(content, " {a: 1} ");
        assert_eq!(len, "{ {a: 1} }".len());
    }

    #[test]
    fn preserves_internal_whitespace_verbatim() {
        let (_, _, content) = try_parse_svelte_template("{#each items  as  item}").unwrap();
        assert_eq!(content, "#each items  as  item");
    }

    #[test]
    fn rejects_unbalanced_brace() {
        assert!(try_parse_svelte_template("{#if active").is_none());
    }

    #[test]
    fn leaves_shortcode_opener_to_shortcode_probe() {
        assert!(try_parse_svelte_template("{{< meta x >}}").is_none());
    }

    #[test]
    fn rejects_non_brace_start() {
        assert!(try_parse_svelte_template("count}").is_none());
    }
}

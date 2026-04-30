//! Document-level link reference definition map for CommonMark inline
//! parsing.
//!
//! CommonMark §6.3 says reference links are valid only when the label
//! matches a definition that appears anywhere in the document, including
//! after the use site. The block-level parser already recognises
//! `[label]: dest` lines and emits them as separate blocks, but inline
//! parsing has historically treated every `[bracket pair]` as opaque on
//! shape alone — without checking whether the label resolves.
//!
//! The fix is a single forward scan over the input *before* inline
//! parsing runs, collecting every refdef label into a [`RefdefMap`].
//! The IR's bracket resolution pass consults this map to decide whether
//! a `[...]` (or `[...][...]`) opens a link or falls through to literal
//! text.
//!
//! ## Scope
//!
//! - The set is computed once per `Parser::parse` call from the original
//!   input string and shared (via `Arc`) with every inline parsing
//!   invocation that needs it. Inline fragments (e.g. heading text,
//!   paragraph text, table cell text) do not contain refdef definitions
//!   themselves, so a *fragment-level* scan is insufficient.
//!
//! - Labels are normalised per CommonMark §4.7: case-folded, leading and
//!   trailing whitespace stripped, internal whitespace runs collapsed to
//!   a single space. The same normalisation applies on the lookup side
//!   in the bracket resolution pass.
//!
//! - The scan does not attempt to detect refdefs inside code fences or
//!   raw HTML blocks; it accepts a small over-approximation in exchange
//!   for being a context-free linear walk. A bracket label that happens
//!   to *spell* a defined refdef inside a fenced code block would still
//!   resolve correctly under emission because emission walks the CST,
//!   which already excludes the fenced region. The over-approximation
//!   only matters if a bogus refdef-shaped line *outside* a code block
//!   would shadow real text — that case is also wrong under CommonMark
//!   semantics, so the approximation is fine.

use crate::options::Dialect;
use std::collections::HashSet;
use std::sync::Arc;

use crate::parser::blocks::reference_links::try_parse_reference_definition;

/// Set of normalised refdef labels collected from the document. Wrapped
/// in `Arc` so the (immutable) set can be cheaply cloned into every
/// inline parsing call.
pub type RefdefMap = Arc<HashSet<String>>;

/// Normalise a refdef label per CommonMark §4.7.
///
/// 1. Strip leading and trailing whitespace.
/// 2. Collapse internal whitespace runs (any mixture of spaces, tabs,
///    line endings) to a single space.
/// 3. Case-fold. CommonMark mandates Unicode case folding rather than
///    plain lowercasing; the two differ for characters whose folded
///    form is longer than the lowercased form, most notably the German
///    sharp S (`ẞ` lowercases to `ß` but folds to `ss`). We approximate
///    by lowercasing and then expanding any remaining `ß` to `ss` —
///    that matches the test renderer's `normalize_label` and is the
///    only multi-character fold spec.txt exercises beyond ASCII (spec
///    example #540).
pub fn normalize_label(label: &str) -> String {
    let trimmed = label.trim();
    let mut out = String::with_capacity(trimmed.len());
    let mut prev_ws = false;
    for ch in trimmed.chars() {
        if ch.is_whitespace() {
            if !prev_ws {
                out.push(' ');
                prev_ws = true;
            }
        } else {
            for low in ch.to_lowercase() {
                out.push(low);
            }
            prev_ws = false;
        }
    }
    out.replace('ß', "ss")
}

/// Walk the input string once and collect all reference definitions into
/// a [`RefdefMap`]. Only used for `Dialect::CommonMark`; callers should
/// pass an empty (or `None`) map for other dialects.
///
/// The scanner is line-based: at each line-start, it strips any
/// blockquote markers (`> ` / `>` runs) — refdefs are valid inside a
/// blockquote per CommonMark §4.7 (spec example #218) — and tries
/// [`try_parse_reference_definition`] on the surviving bytes. When the
/// parser reports a multi-line consumption the cursor advances past the
/// whole refdef in one step.
pub fn collect_refdef_labels(input: &str, dialect: Dialect) -> RefdefMap {
    let mut set: HashSet<String> = HashSet::new();
    let bytes = input.as_bytes();
    let mut pos = 0;

    while pos < bytes.len() {
        // Try at the unmodified line-start first; this covers the
        // top-level case and avoids allocating the stripped buffer for
        // most lines.
        if let Some((consumed, label, _url, _title)) =
            try_parse_reference_definition(&input[pos..], dialect)
        {
            set.insert(normalize_label(&label));
            pos += consumed.max(1);
            continue;
        }

        // Try after stripping a leading blockquote prefix. We do this
        // lazily: only when the line actually starts with `>` (possibly
        // preceded by up to 3 spaces).
        if line_starts_with_blockquote(&input[pos..])
            && let Some(stripped) = strip_blockquote_line(&input[pos..])
            && let Some((_, label, _, _)) = try_parse_reference_definition(&stripped, dialect)
        {
            set.insert(normalize_label(&label));
        }

        match memchr_newline(&bytes[pos..]) {
            Some(off) => {
                pos += off + 1;
            }
            None => break,
        }
    }

    Arc::new(set)
}

fn memchr_newline(bytes: &[u8]) -> Option<usize> {
    bytes.iter().position(|&b| b == b'\n')
}

/// `true` if the line starting at `text[0]` begins with a blockquote
/// marker (`>` after up to 3 leading spaces).
fn line_starts_with_blockquote(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() && i < 3 && bytes[i] == b' ' {
        i += 1;
    }
    bytes.get(i) == Some(&b'>')
}

/// Strip leading blockquote markers from a single line plus its possible
/// continuation lines, producing the inner text suitable for refdef
/// parsing. Returns `None` if no blockquote prefix is present.
///
/// Refdefs can span multiple lines (e.g. the title can wrap), so we
/// strip blockquote markers from each continuation line too. We stop at
/// a blank line or a line that doesn't continue with a blockquote
/// marker.
fn strip_blockquote_line(text: &str) -> Option<String> {
    if !line_starts_with_blockquote(text) {
        return None;
    }
    let mut out = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        let bytes = line.as_bytes();
        let mut i = 0;
        while i < bytes.len() && i < 3 && bytes[i] == b' ' {
            i += 1;
        }
        if bytes.get(i) != Some(&b'>') {
            // Not a blockquote continuation — stop.
            break;
        }
        i += 1;
        // Optional single space after `>`.
        if bytes.get(i) == Some(&b' ') {
            i += 1;
        }
        out.push_str(&line[i..]);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collects_simple_refdef() {
        let map = collect_refdef_labels("[foo]: /url\n", Dialect::CommonMark);
        assert!(map.contains("foo"));
    }

    #[test]
    fn collects_multiple_refdefs() {
        let input = "[foo]: /a\n[bar]: /b\n[baz]: /c\n";
        let map = collect_refdef_labels(input, Dialect::CommonMark);
        assert!(map.contains("foo"));
        assert!(map.contains("bar"));
        assert!(map.contains("baz"));
    }

    #[test]
    fn does_not_collect_non_refdef_lines() {
        let input = "Just a paragraph.\n\nAnother one.\n";
        let map = collect_refdef_labels(input, Dialect::CommonMark);
        assert!(map.is_empty());
    }

    #[test]
    fn collects_after_paragraph() {
        let input = "Some paragraph.\n\n[foo]: /url\n";
        let map = collect_refdef_labels(input, Dialect::CommonMark);
        assert!(map.contains("foo"));
    }

    #[test]
    fn case_folded_label() {
        let map = collect_refdef_labels("[FOO Bar]: /url\n", Dialect::CommonMark);
        assert!(map.contains("foo bar"));
    }

    #[test]
    fn collapses_internal_whitespace() {
        assert_eq!(normalize_label("  foo   bar\tbaz  "), "foo bar baz");
    }

    #[test]
    fn label_523_is_not_collected() {
        // CMark example 523 has no refdef; the bracket should fall through
        // to literal text under bracket resolution.
        let map = collect_refdef_labels("*foo [bar* baz]\n", Dialect::CommonMark);
        assert!(map.is_empty());
    }
}

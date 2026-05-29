//! Link conversion utilities for code actions.
//!
//! Converts a single inline link (`[text](url)`) to reference style
//! (`[text][label]` + `[label]: url`), or the reverse. Mirrors
//! `footnote_conversion`. Reference-style conversion reuses an existing
//! definition whose `(url, title)` pair matches; otherwise a slugged label
//! is generated. Inline conversion deletes the matching definition when
//! it was the last use.

use std::collections::HashSet;

use tower_lsp_server::ls_types::{Range, TextEdit};

use crate::syntax::{AstNode, Link, ReferenceDefinition, SyntaxNode};
use crate::utils::normalize_label;

use super::super::conversions::offset_to_position;

/// Find the innermost `LINK` node at the given offset.
pub fn find_link_at_position(tree: &SyntaxNode, offset: usize) -> Option<Link> {
    let text_size = rowan::TextSize::from(offset as u32);
    let token = tree.token_at_offset(text_size).right_biased()?;
    token.parent_ancestors().find_map(Link::cast)
}

/// True iff the link is reference-style AND its referenced definition exists.
pub fn can_convert_to_inline(link: &Link, tree: &SyntaxNode) -> bool {
    let Some(label) = link.reference().map(|r| r.label()) else {
        return false;
    };
    find_definition(tree, &label).is_some()
}

/// True iff the link is inline-style (has a destination).
pub fn can_convert_to_reference(link: &Link) -> bool {
    link.dest().is_some()
}

/// Convert a reference-style link at `link` to inline style.
///
/// Emits up to two edits:
/// 1. Replace `[text][label]` (or `[text][]`/`[text]`) with `[text](url "title")`.
/// 2. Delete the reference definition line if the converted occurrence was
///    its only consumer.
pub fn convert_to_inline(link: &Link, tree: &SyntaxNode, text: &str) -> Vec<TextEdit> {
    let Some(reference) = link.reference() else {
        return vec![];
    };
    let label = reference.label();
    let Some(definition) = find_definition(tree, &label) else {
        return vec![];
    };
    let Some(url) = definition.url() else {
        return vec![];
    };
    let title = definition.title();

    let link_text_content = link.text().map(|t| t.text_content()).unwrap_or_default();
    let new_inline = match title.as_deref() {
        Some(t) if !t.is_empty() => format!("[{}]({} \"{}\")", link_text_content, url, t),
        _ => format!("[{}]({})", link_text_content, url),
    };

    let link_range = link.syntax().text_range();
    let mut edits = vec![TextEdit {
        range: Range {
            start: offset_to_position(text, link_range.start().into()),
            end: offset_to_position(text, link_range.end().into()),
        },
        new_text: new_inline,
    }];

    if count_label_uses(tree, &label) == 1 {
        let def_node = definition.syntax();
        let def_start: usize = def_node.text_range().start().into();
        let def_end: usize = def_node.text_range().end().into();
        let extended_end = if def_end < text.len() && text.as_bytes()[def_end] == b'\n' {
            def_end + 1
        } else {
            def_end
        };
        edits.push(TextEdit {
            range: Range {
                start: offset_to_position(text, def_start),
                end: offset_to_position(text, extended_end),
            },
            new_text: String::new(),
        });
    }

    edits
}

/// Convert an inline link to reference style.
///
/// If an existing reference definition has a matching `(url, title)` pair,
/// reuse its label and emit only the in-place replacement edit. Otherwise
/// generate a slugged label and append a new definition after the last
/// existing one (or at end-of-document if none exists).
pub fn convert_to_reference(link: &Link, tree: &SyntaxNode, text: &str) -> Vec<TextEdit> {
    let Some(dest_node) = link.dest() else {
        return vec![];
    };
    let Some((url, title)) = split_url_and_title(&dest_node.url()) else {
        return vec![];
    };

    let link_text_content = link.text().map(|t| t.text_content()).unwrap_or_default();

    let defs: Vec<ReferenceDefinition> = tree
        .descendants()
        .filter_map(ReferenceDefinition::cast)
        .collect();

    // Reuse an existing definition with matching (url, title).
    let reuse = defs.iter().find_map(|def| {
        let def_url = def.url()?;
        if def_url != url {
            return None;
        }
        let def_title = def.title().unwrap_or_default();
        let want_title = title.clone().unwrap_or_default();
        if def_title == want_title {
            Some(def.label())
        } else {
            None
        }
    });

    let label = reuse.unwrap_or_else(|| {
        let existing: HashSet<String> = defs.iter().map(|d| normalize_label(&d.label())).collect();
        generate_label(&link_text_content, &url, &existing)
    });

    let link_range = link.syntax().text_range();
    let mut edits = vec![TextEdit {
        range: Range {
            start: offset_to_position(text, link_range.start().into()),
            end: offset_to_position(text, link_range.end().into()),
        },
        new_text: format!("[{}][{}]", link_text_content, label),
    }];

    // If we reused an existing def, we're done.
    if defs
        .iter()
        .any(|d| normalize_label(&d.label()) == normalize_label(&label))
    {
        return edits;
    }

    // Append a new definition after the last existing one, or at end-of-doc.
    let new_def = match title.as_deref() {
        Some(t) if !t.is_empty() => format!("[{}]: {} \"{}\"\n", label, url, t),
        _ => format!("[{}]: {}\n", label, url),
    };
    let (insert_pos, prefix) = match defs.last() {
        Some(last_def) => {
            let end: usize = last_def.syntax().text_range().end().into();
            (offset_to_position(text, end), "\n")
        }
        None => (offset_to_position(text, text.len()), "\n\n"),
    };
    edits.push(TextEdit {
        range: Range {
            start: insert_pos,
            end: insert_pos,
        },
        new_text: format!("{}{}", prefix, new_def),
    });

    edits
}

fn find_definition(tree: &SyntaxNode, label: &str) -> Option<ReferenceDefinition> {
    let target = normalize_label(label);
    tree.descendants()
        .filter_map(ReferenceDefinition::cast)
        .find(|def| normalize_label(&def.label()) == target)
}

/// Count how many `LINK` or `IMAGE_LINK` nodes resolve to the given label.
fn count_label_uses(tree: &SyntaxNode, label: &str) -> usize {
    let target = normalize_label(label);
    let mut count = 0;
    for node in tree.descendants() {
        if let Some(link) = Link::cast(node.clone())
            && let Some(r) = link.reference()
            && normalize_label(&r.label()) == target
        {
            count += 1;
            continue;
        }
        if let Some(image) = crate::syntax::ImageLink::cast(node.clone())
            && let Some(label_value) = image.reference_label()
            && normalize_label(&label_value) == target
        {
            count += 1;
        }
    }
    count
}

/// Split a `LinkDest` body into `(url, optional title)`.
///
/// Title is delimited by `"…"`, `'…'`, or `(…)`; this function strips those
/// delimiters when present and returns the inner content. Returns `None`
/// when the URL would be empty.
fn split_url_and_title(body: &str) -> Option<(String, Option<String>)> {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return None;
    }
    let url_end = trimmed.find(char::is_whitespace).unwrap_or(trimmed.len());
    let url = &trimmed[..url_end];
    if url.is_empty() {
        return None;
    }
    let rest = trimmed[url_end..].trim();
    let title = if rest.is_empty() {
        None
    } else {
        Some(strip_title_delimiters(rest))
    };
    Some((url.to_string(), title))
}

fn strip_title_delimiters(raw: &str) -> String {
    let bytes = raw.as_bytes();
    if bytes.len() >= 2 {
        let (open, close) = (bytes[0], bytes[bytes.len() - 1]);
        if matches!((open, close), (b'"', b'"') | (b'\'', b'\'') | (b'(', b')')) {
            return raw[1..raw.len() - 1].to_string();
        }
    }
    raw.to_string()
}

/// Build a refdef label from the link's text, falling back to the URL host
/// and finally to a numeric suffix when the slug would be empty or collides
/// with an existing label.
fn generate_label(text: &str, url: &str, existing: &HashSet<String>) -> String {
    let mut base = slugify(text);
    if base.is_empty() {
        base = slugify(url_host(url));
    }
    if base.is_empty() {
        base = "link".to_string();
    }
    if !existing.contains(&normalize_label(&base)) {
        return base;
    }
    for n in 2.. {
        let candidate = format!("{}-{}", base, n);
        if !existing.contains(&normalize_label(&candidate)) {
            return candidate;
        }
    }
    unreachable!("the integer range is exhausted before a free label is found")
}

fn slugify(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut prev_dash = false;
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

fn url_host(url: &str) -> &str {
    let after_scheme = url.split_once("://").map(|(_, rest)| rest).unwrap_or(url);
    after_scheme.split('/').next().unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    fn link_at(tree: &SyntaxNode, needle_offset: usize) -> Link {
        find_link_at_position(tree, needle_offset).expect("Link at cursor")
    }

    #[test]
    fn finds_inline_link_at_cursor() {
        let input = "See [a](https://example.com/) here.\n";
        let tree = parse(input, None);
        let offset = input.find("[a]").unwrap() + 1;
        let link = link_at(&tree, offset);
        assert!(can_convert_to_reference(&link));
        assert!(!can_convert_to_inline(&link, &tree));
    }

    #[test]
    fn finds_reference_link_at_cursor() {
        let input = "[a][site]\n\n[site]: https://example.com/\n";
        let tree = parse(input, None);
        let offset = input.find("[a]").unwrap() + 1;
        let link = link_at(&tree, offset);
        assert!(can_convert_to_inline(&link, &tree));
        assert!(!can_convert_to_reference(&link));
    }

    #[test]
    fn inline_to_reference_creates_new_def() {
        let input = "See [the docs](https://example.com/) for details.\n";
        let tree = parse(input, None);
        let offset = input.find("[the docs]").unwrap() + 2;
        let link = link_at(&tree, offset);
        let edits = convert_to_reference(&link, &tree, input);
        assert_eq!(edits.len(), 2);
        assert_eq!(edits[0].new_text, "[the docs][the-docs]");
        assert!(
            edits[1]
                .new_text
                .contains("[the-docs]: https://example.com/\n")
        );
    }

    #[test]
    fn inline_to_reference_reuses_matching_def() {
        let input = "See [the docs](https://example.com/) and [more](https://example.com/).\n\n[home]: https://example.com/\n";
        let tree = parse(input, None);
        let offset = input.find("[the docs]").unwrap() + 2;
        let link = link_at(&tree, offset);
        let edits = convert_to_reference(&link, &tree, input);
        assert_eq!(edits.len(), 1, "reused existing def — no new def needed");
        assert_eq!(edits[0].new_text, "[the docs][home]");
    }

    #[test]
    fn inline_to_reference_preserves_title() {
        let input = "See [docs](https://example.com/ \"Docs\") here.\n";
        let tree = parse(input, None);
        let offset = input.find("[docs]").unwrap() + 2;
        let link = link_at(&tree, offset);
        let edits = convert_to_reference(&link, &tree, input);
        assert_eq!(edits.len(), 2);
        assert!(
            edits[1]
                .new_text
                .contains("[docs]: https://example.com/ \"Docs\"")
        );
    }

    #[test]
    fn inline_to_reference_disambiguates_colliding_slug() {
        let input = "Read [Docs](https://a.example/) and [docs](https://b.example/).\n";
        let tree = parse(input, None);
        // First link converted: slug "docs".
        let first = link_at(&tree, input.find("[Docs]").unwrap() + 2);
        let edits1 = convert_to_reference(&first, &tree, input);
        assert_eq!(edits1[0].new_text, "[Docs][docs]");

        // Re-parse with the first def appended, then convert the second.
        let after_first =
            "Read [Docs][docs] and [docs](https://b.example/).\n\n[docs]: https://a.example/\n";
        let tree2 = parse(after_first, None);
        let second = link_at(&tree2, after_first.find("[docs](").unwrap() + 2);
        let edits2 = convert_to_reference(&second, &tree2, after_first);
        // "docs" collides with the existing def (different URL), so we expect "docs-2".
        assert_eq!(edits2[0].new_text, "[docs][docs-2]");
    }

    #[test]
    fn reference_to_inline_deletes_orphan_def() {
        let input = "[docs][d]\n\n[d]: https://example.com/\n";
        let tree = parse(input, None);
        let offset = input.find("[docs]").unwrap() + 2;
        let link = link_at(&tree, offset);
        let edits = convert_to_inline(&link, &tree, input);
        assert_eq!(edits.len(), 2);
        assert_eq!(edits[0].new_text, "[docs](https://example.com/)");
        assert_eq!(edits[1].new_text, ""); // def deletion
    }

    #[test]
    fn reference_to_inline_keeps_shared_def() {
        let input = "[a][d] and [b][d]\n\n[d]: https://example.com/\n";
        let tree = parse(input, None);
        let offset = input.find("[a]").unwrap() + 1;
        let link = link_at(&tree, offset);
        let edits = convert_to_inline(&link, &tree, input);
        assert_eq!(edits.len(), 1, "shared def stays in place");
        assert_eq!(edits[0].new_text, "[a](https://example.com/)");
    }

    #[test]
    fn reference_to_inline_preserves_title() {
        let input = "[a][d]\n\n[d]: https://example.com/ \"Title\"\n";
        let tree = parse(input, None);
        let offset = input.find("[a]").unwrap() + 1;
        let link = link_at(&tree, offset);
        let edits = convert_to_inline(&link, &tree, input);
        assert_eq!(edits[0].new_text, "[a](https://example.com/ \"Title\")");
    }
}

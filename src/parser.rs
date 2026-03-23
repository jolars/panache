//! Parser module for Pandoc/Quarto documents.
//!
//! This module implements a single-pass parser that constructs a lossless syntax tree (CST) for
//! Quarto documents.

use crate::config::Config;
use crate::range_utils::find_incremental_restart_offset;
use crate::syntax::{SyntaxKind, SyntaxNode};
use rowan::{GreenNode, GreenToken, NodeOrToken};

pub mod blocks;
pub mod inlines;
pub mod utils;
pub mod yaml;

mod block_dispatcher;
mod core;

// Re-export main parser
pub use core::Parser;

/// Parses a Quarto document string into a syntax tree.
///
/// Single-pass architecture: blocks emit inline structure during parsing.
///
/// # Examples
///
/// ```rust
/// use panache::parser::parse;
///
/// let input = "# Heading\n\nParagraph text.";
/// let tree = parse(input, None);
/// println!("{:#?}", tree);
/// ```
///
/// # Arguments
///
/// * `input` - The Quarto document content to parse
/// * `config` - Optional configuration. If None, uses default config.
pub fn parse(input: &str, config: Option<Config>) -> SyntaxNode {
    let config = config.unwrap_or_default();
    Parser::new(input, &config).parse()
}

pub struct IncrementalParseResult {
    pub tree: SyntaxNode,
    pub reparse_range: (usize, usize),
}

/// Incrementally update a syntax tree by reparsing from a safe restart boundary
/// to EOF and rebuilding the document root from old prefix + reparsed suffix.
pub fn parse_incremental_suffix(
    input: &str,
    config: Option<Config>,
    old_tree: &SyntaxNode,
    old_edit_range: (usize, usize),
    new_edit_range: (usize, usize),
) -> IncrementalParseResult {
    let config = config.unwrap_or_default();
    let input_len = input.len();

    let Some(old_edit) = normalize_range(old_edit_range) else {
        return full_reparse_result(input, &config);
    };
    let Some(new_edit) = normalize_range(new_edit_range) else {
        return full_reparse_result(input, &config);
    };
    if new_edit.1 > input_len {
        return full_reparse_result(input, &config);
    }

    if old_tree.kind() != SyntaxKind::DOCUMENT {
        return full_reparse_result(input, &config);
    }

    if let Some(section_window) =
        find_top_level_heading_section_window(old_tree, old_edit, new_edit, input_len)
        && let Some(result) = reparse_section_window(input, &config, old_tree, section_window)
    {
        return result;
    }

    let restart = find_incremental_restart_offset(old_tree, old_edit.0, old_edit.1);
    let old_restart = align_to_document_child_start(old_tree, restart);

    if (old_edit.0..old_edit.1).contains(&old_restart) {
        return full_reparse_result(input, &config);
    }

    let new_restart = map_old_offset_to_new(old_restart, old_edit, new_edit, input_len);
    if !input.is_char_boundary(new_restart) {
        return full_reparse_result(input, &config);
    }

    let suffix_text = &input[new_restart..];
    let suffix_tree = Parser::new(suffix_text, &config).parse();

    let mut children: Vec<NodeOrToken<GreenNode, GreenToken>> = old_tree
        .children_with_tokens()
        .filter_map(|element| {
            let range = element.text_range();
            let end: usize = range.end().into();
            if end <= old_restart {
                Some(element_to_green(element))
            } else {
                None
            }
        })
        .collect();
    children.extend(suffix_tree.children_with_tokens().map(element_to_green));

    let tree = SyntaxNode::new_root(GreenNode::new(SyntaxKind::DOCUMENT.into(), children));
    let len: usize = tree.text_range().end().into();

    IncrementalParseResult {
        tree,
        reparse_range: (new_restart, len),
    }
}

fn normalize_range(range: (usize, usize)) -> Option<(usize, usize)> {
    (range.0 <= range.1).then_some(range)
}

fn full_reparse_result(input: &str, config: &Config) -> IncrementalParseResult {
    let tree = Parser::new(input, config).parse();
    let len: usize = tree.text_range().end().into();
    IncrementalParseResult {
        tree,
        reparse_range: (0, len),
    }
}

fn align_to_document_child_start(tree: &SyntaxNode, offset: usize) -> usize {
    for child in tree.children_with_tokens() {
        let range = child.text_range();
        let start: usize = range.start().into();
        let end: usize = range.end().into();
        if offset <= start {
            return start;
        }
        if offset < end {
            return start;
        }
    }
    let len: usize = tree.text_range().end().into();
    len
}

fn map_old_offset_to_new(
    old_offset: usize,
    old_edit: (usize, usize),
    new_edit: (usize, usize),
    new_len: usize,
) -> usize {
    if old_offset <= old_edit.0 {
        return old_offset;
    }
    if old_offset >= old_edit.1 {
        let old_span = old_edit.1 - old_edit.0;
        let new_span = new_edit.1 - new_edit.0;
        let delta = new_span as isize - old_span as isize;
        return old_offset.saturating_add_signed(delta).min(new_len);
    }
    new_edit.1.min(new_len)
}

fn element_to_green(element: crate::syntax::SyntaxElement) -> NodeOrToken<GreenNode, GreenToken> {
    match element {
        NodeOrToken::Node(node) => NodeOrToken::Node(node.green().into_owned()),
        NodeOrToken::Token(token) => NodeOrToken::Token(token.green().to_owned()),
    }
}

#[derive(Debug, Clone, Copy)]
struct SectionWindow {
    old_start: usize,
    old_end: usize,
    new_start: usize,
    new_end: usize,
}

fn find_top_level_heading_section_window(
    old_tree: &SyntaxNode,
    old_edit: (usize, usize),
    new_edit: (usize, usize),
    new_len: usize,
) -> Option<SectionWindow> {
    let mut previous_heading: Option<(usize, usize)> = None;
    let mut next_heading: Option<(usize, usize)> = None;

    for child in old_tree.children() {
        if child.kind() != SyntaxKind::HEADING {
            continue;
        }

        let range = child.text_range();
        let start: usize = range.start().into();
        let end: usize = range.end().into();

        if start <= old_edit.0 {
            previous_heading = Some((start, end));
        } else {
            next_heading = Some((start, end));
            break;
        }
    }

    let (previous_start, previous_end) = previous_heading?;
    let (next_start, next_end) = next_heading?;

    if ranges_intersect(old_edit, (previous_start, previous_end))
        || ranges_intersect(old_edit, (next_start, next_end))
    {
        return None;
    }

    // Be conservative and only use the section window for edits that are
    // strictly inside the section body (not touching heading boundaries).
    if old_edit.0 <= previous_end || old_edit.1 >= next_start {
        return None;
    }

    let new_start = map_old_offset_to_new(previous_start, old_edit, new_edit, new_len);
    let new_end = map_old_offset_to_new(next_start, old_edit, new_edit, new_len);
    if new_start >= new_end || new_end > new_len {
        return None;
    }

    Some(SectionWindow {
        old_start: previous_start,
        old_end: next_start,
        new_start,
        new_end,
    })
}

fn ranges_intersect(a: (usize, usize), b: (usize, usize)) -> bool {
    a.0 < b.1 && b.0 < a.1
}

fn reparse_section_window(
    input: &str,
    config: &Config,
    old_tree: &SyntaxNode,
    section_window: SectionWindow,
) -> Option<IncrementalParseResult> {
    if !input.is_char_boundary(section_window.new_start)
        || !input.is_char_boundary(section_window.new_end)
    {
        return None;
    }

    let reparsed_window = Parser::new(
        &input[section_window.new_start..section_window.new_end],
        config,
    )
    .parse();

    let mut children: Vec<NodeOrToken<GreenNode, GreenToken>> = Vec::new();
    let mut inserted_window = false;

    for element in old_tree.children_with_tokens() {
        let range = element.text_range();
        let start: usize = range.start().into();
        let end: usize = range.end().into();

        if end <= section_window.old_start {
            children.push(element_to_green(element));
            continue;
        }

        if start >= section_window.old_end {
            if !inserted_window {
                children.extend(reparsed_window.children_with_tokens().map(element_to_green));
                inserted_window = true;
            }
            children.push(element_to_green(element));
            continue;
        }

        // Overlapping element is replaced by the reparsed section window.
    }

    if !inserted_window {
        children.extend(reparsed_window.children_with_tokens().map(element_to_green));
    }

    let tree = SyntaxNode::new_root(GreenNode::new(SyntaxKind::DOCUMENT.into(), children));
    Some(IncrementalParseResult {
        tree,
        reparse_range: (section_window.new_start, section_window.new_end),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply_edit(text: &str, old: (usize, usize), insert: &str) -> String {
        let mut out = String::with_capacity(text.len() - (old.1 - old.0) + insert.len());
        out.push_str(&text[..old.0]);
        out.push_str(insert);
        out.push_str(&text[old.1..]);
        out
    }

    #[test]
    fn incremental_suffix_matches_full_parse_for_tail_edit() {
        let input = "# H\n\npara one\n\npara two\n\npara three\n";
        let old_tree = parse(input, None);
        let old_edit = (30, 35);
        let updated = apply_edit(input, old_edit, "tail section");
        let new_edit = (30, 42);

        let inc = parse_incremental_suffix(&updated, None, &old_tree, old_edit, new_edit).tree;
        let full = parse(&updated, None);
        assert_eq!(inc.to_string(), full.to_string());
    }

    #[test]
    fn incremental_suffix_matches_full_parse_for_middle_edit() {
        let input = "# H\n\n- a\n- b\n\nfinal para\n";
        let old_tree = parse(input, None);
        let old_edit = (10, 11);
        let updated = apply_edit(input, old_edit, "alpha");
        let new_edit = (10, 15);

        let inc = parse_incremental_suffix(&updated, None, &old_tree, old_edit, new_edit).tree;
        let full = parse(&updated, None);
        assert_eq!(inc.to_string(), full.to_string());
    }

    #[test]
    fn incremental_suffix_matches_full_parse_for_setext_transition() {
        let input = "Intro\nSecond\n\nTail\n";
        let old_tree = parse(input, None);
        let old_edit = (5, 5);
        let updated = apply_edit(input, old_edit, "\n-----");
        let new_edit = (5, 11);

        let inc = parse_incremental_suffix(&updated, None, &old_tree, old_edit, new_edit).tree;
        let full = parse(&updated, None);
        assert_eq!(inc.to_string(), full.to_string());
    }

    #[test]
    fn incremental_suffix_matches_full_parse_for_lazy_blockquote_change() {
        let input = "> quoted\nlazy\n\nnext\n";
        let old_tree = parse(input, None);
        let old_edit = (9, 13);
        let updated = apply_edit(input, old_edit, "> line");
        let new_edit = (9, 15);

        let inc = parse_incremental_suffix(&updated, None, &old_tree, old_edit, new_edit).tree;
        let full = parse(&updated, None);
        assert_eq!(inc.to_string(), full.to_string());
    }

    #[test]
    fn incremental_uses_heading_section_window_when_available() {
        let input = "# Intro\n\nalpha\n\n# Middle\n\nbeta section\n\n# End\n\nomega\n";
        let old_tree = parse(input, None);
        let start = input.find("beta").expect("beta in test input");
        let old_edit = (start, start + 4);
        let updated = apply_edit(input, old_edit, "BETA");
        let new_edit = (start, start + 4);

        let inc = parse_incremental_suffix(&updated, None, &old_tree, old_edit, new_edit);
        let full = parse(&updated, None);
        assert_eq!(inc.tree.to_string(), full.to_string());
        assert!(
            inc.reparse_range.0 > 0,
            "section reparse should not start at 0"
        );
        assert!(
            inc.reparse_range.1 < updated.len(),
            "section reparse should stop before EOF"
        );
    }
}

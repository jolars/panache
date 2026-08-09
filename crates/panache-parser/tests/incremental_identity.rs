//! Regression tests locking in the structural-sharing property the incremental
//! reparser relies on: retained top-level blocks keep their `Arc` identity
//! across an edit, and unchanged blocks compare equal whether the document was
//! reparsed incrementally or from scratch.
//!
//! This property is what lets the downstream salsa analysis pipeline memoize
//! per-block: unchanged blocks are content-addressed cache hits. If a refactor
//! silently deep-copies subtrees (losing identity) or perturbs unchanged blocks,
//! these tests fail before the perf regression reaches salsa.

use panache_parser::SyntaxNode;
use panache_parser::parser::{parse, parse_with_errors};

mod common;
use common::reparse_or_full;
use rowan::{GreenNode, GreenNodeData, NodeOrToken};

fn apply_edit(text: &str, old: (usize, usize), insert: &str) -> String {
    let mut out = String::with_capacity(text.len() - (old.1 - old.0) + insert.len());
    out.push_str(&text[..old.0]);
    out.push_str(insert);
    out.push_str(&text[old.1..]);
    out
}

/// Owned green subtrees of the top-level `DOCUMENT` children (nodes only).
fn blocks(tree: &SyntaxNode) -> Vec<GreenNode> {
    tree.green()
        .children()
        .filter_map(|child| match child.to_owned() {
            NodeOrToken::Node(node) => Some(node),
            NodeOrToken::Token(_) => None,
        })
        .collect()
}

/// The address of a green node's shared allocation. Two green handles pointing
/// at the same allocation (i.e. sharing structure) report the same address.
fn green_addr(node: &GreenNode) -> usize {
    let data: &GreenNodeData = node;
    data as *const GreenNodeData as usize
}

#[test]
fn incremental_suffix_retains_prefix_block_identity() {
    // No headings -> the suffix-window strategy, which retains a genuine prefix.
    let input = "para one\n\npara two\n\npara three\n\npara four\n\npara five\n";
    let old_tree = parse(input, None);
    let old_blocks = blocks(&old_tree);

    let start = input.find("five").expect("marker present");
    let old_edit = (start, start + 4);
    let updated = apply_edit(input, old_edit, "FIVE");
    let new_edit = (start, start + 4);

    let inc = reparse_or_full(&updated, None, &old_tree, &[], old_edit, new_edit);
    assert_eq!(
        inc.strategy, "suffix_window",
        "expected the suffix strategy"
    );

    let new_blocks = blocks(&inc.tree);
    let old_addrs: std::collections::HashSet<usize> = old_blocks.iter().map(green_addr).collect();
    let shared = new_blocks
        .iter()
        .filter(|block| old_addrs.contains(&green_addr(block)))
        .count();

    // Every block except the edited trailing paragraph must be pointer-shared.
    assert!(
        shared >= new_blocks.len() - 1,
        "expected all-but-one prefix blocks to share Arc identity, got {shared}/{}",
        new_blocks.len()
    );
    assert!(shared > 0, "no blocks shared identity");
}

#[test]
fn section_window_retains_surrounding_block_identity() {
    let input = "# Intro\n\nalpha\n\n# Middle\n\nbeta section\n\n# End\n\nomega\n";
    let old_tree = parse(input, None);
    let old_blocks = blocks(&old_tree);

    let start = input.find("beta").expect("marker present");
    let old_edit = (start, start + 4);
    let updated = apply_edit(input, old_edit, "BETA");
    let new_edit = (start, start + 4);

    let inc = reparse_or_full(&updated, None, &old_tree, &[], old_edit, new_edit);
    assert_eq!(
        inc.strategy, "section_window",
        "expected the section strategy"
    );

    let new_blocks = blocks(&inc.tree);
    let old_addrs: std::collections::HashSet<usize> = old_blocks.iter().map(green_addr).collect();

    // The section window reparses the whole edited section (its heading, blank
    // lines, and body), so those blocks are rebuilt. The guarantee is that
    // blocks in *other* sections keep their `Arc` identity: the leading `Intro`
    // section and the trailing `End` section are untouched.
    assert!(
        old_addrs.contains(&green_addr(&new_blocks[0])),
        "the first block (Intro heading) should be pointer-shared"
    );
    let last = new_blocks.last().expect("non-empty document");
    assert!(
        old_addrs.contains(&green_addr(last)),
        "the last block (End section body) should be pointer-shared"
    );
    let shared = new_blocks
        .iter()
        .filter(|block| old_addrs.contains(&green_addr(block)))
        .count();
    assert!(
        shared >= 7,
        "sections outside the edit should be pointer-shared, got {shared}/{}",
        new_blocks.len()
    );
}

#[test]
fn incremental_and_full_reparse_agree_block_for_block() {
    let input = "# Intro\n\nalpha\n\n# Middle\n\nbeta section\n\n# End\n\nomega\n";
    let old_tree = parse(input, None);

    let start = input.find("beta").expect("marker present");
    let old_edit = (start, start + 4);
    let updated = apply_edit(input, old_edit, "BETA");
    let new_edit = (start, start + 4);

    let inc = reparse_or_full(&updated, None, &old_tree, &[], old_edit, new_edit);
    let full = parse(&updated, None);

    let inc_blocks = blocks(&inc.tree);
    let full_blocks = blocks(&full);

    // Incremental reparse must produce a tree structurally identical to a full
    // reparse of the same text, block for block. This is the guarantee that
    // content-addressed salsa memoization stays correct under either strategy.
    assert_eq!(
        inc_blocks.len(),
        full_blocks.len(),
        "block count diverged between incremental and full reparse"
    );
    for (i, (a, b)) in inc_blocks.iter().zip(&full_blocks).enumerate() {
        assert_eq!(
            a, b,
            "block {i} diverged between incremental and full reparse"
        );
    }
    assert_eq!(inc.tree.to_string(), full.to_string());
}

#[test]
fn unchanged_blocks_compare_equal_across_edit() {
    // The load-bearing salsa property: editing one block leaves every other
    // block `==` to its pre-edit counterpart (so per-block memos hit).
    let input = "# Intro\n\nalpha\n\n# Middle\n\nbeta section\n\n# End\n\nomega\n";
    let old_tree = parse(input, None);
    let old_blocks = blocks(&old_tree);

    let start = input.find("beta").expect("marker present");
    let old_edit = (start, start + 4);
    let updated = apply_edit(input, old_edit, "BETA");

    let full = parse(&updated, None);
    let new_blocks = blocks(&full);

    assert_eq!(old_blocks.len(), new_blocks.len());
    let differing = old_blocks
        .iter()
        .zip(&new_blocks)
        .filter(|(a, b)| a != b)
        .count();
    assert_eq!(
        differing, 1,
        "exactly one block should differ after a single-block edit"
    );
}

/// Extract the `$0...$0`-marked range from `marked`, returning the unmarked
/// text and the byte range the markers delimited.
fn extract_range(marked: &str) -> (String, (usize, usize)) {
    let start = marked.find("$0").expect("opening $0 marker");
    let rest = &marked[start + 2..];
    let end_rel = rest.find("$0").expect("closing $0 marker");
    let mut text = String::with_capacity(marked.len() - 4);
    text.push_str(&marked[..start]);
    text.push_str(&rest[..end_rel]);
    text.push_str(&rest[end_rel + 2..]);
    (text, (start, start + end_rel))
}

/// rust-analyzer-style structural check. `before_marked` holds `$0...$0`
/// markers around the deleted range; `insert` replaces it. Asserts:
///
/// 1. the incremental parse engaged `expected_strategy` (not a silent
///    fallback),
/// 2. it reparsed exactly `reparsed_len` bytes — a pinned granularity that
///    fails when a change silently widens the reparse window (a perf
///    regression correctness tests cannot see),
/// 3. the incremental tree's full `{:#?}` dump equals a from-scratch parse
///    of the edited text — structural identity, not text equality,
/// 4. the spliced syntax-error vector equals that parse's.
fn do_check(before_marked: &str, insert: &str, expected_strategy: &str, reparsed_len: usize) {
    let (before, old_edit) = extract_range(before_marked);
    let updated = apply_edit(&before, old_edit, insert);
    let new_edit = (old_edit.0, old_edit.0 + insert.len());

    let (old_tree, old_errors) = parse_with_errors(&before, None);
    let inc = reparse_or_full(&updated, None, &old_tree, &old_errors, old_edit, new_edit);
    let (full, full_errors) = parse_with_errors(&updated, None);

    assert_eq!(
        inc.strategy, expected_strategy,
        "wrong strategy for edit {old_edit:?} -> {insert:?} in {before:?}"
    );
    assert_eq!(
        inc.reparse_range.1 - inc.reparse_range.0,
        reparsed_len,
        "reparsed window has wrong length (range {:?}) for edit {old_edit:?} in {before:?}",
        inc.reparse_range
    );
    assert_eq!(
        format!("{:#?}", inc.tree),
        format!("{full:#?}"),
        "incremental tree diverged structurally from full parse"
    );
    assert_eq!(
        inc.errors, full_errors,
        "incremental syntax errors diverged from full parse"
    );
}

#[test]
fn do_check_suffix_window_tail_edit() {
    // Heading-free document: the suffix strategy restarts at a safe
    // top-level block boundary and reparses to EOF.
    do_check(
        "para one\n\npara two\n\npara three\n\npara four\n\npara $0five$0\n",
        "FIVE",
        "suffix_window",
        10,
    );
}

#[test]
fn do_check_suffix_window_reparses_to_eof_from_middle_edit() {
    // Documents the suffix-window gap: an edit in the middle of a
    // heading-free document reparses everything from the restart to EOF.
    // The region tier (roadmap Phase 8) should shrink this window; when it
    // does, this pinned length must go down, not up.
    do_check(
        "para one\n\npara $0two$0\n\npara three\n\npara four\n\npara five\n",
        "TWO",
        "suffix_window",
        43,
    );
}

#[test]
fn do_check_section_window_between_headings() {
    // Edit strictly inside a section body bounded by top-level headings:
    // only the enclosing section (previous heading to next heading) is
    // reparsed.
    do_check(
        "# Intro\n\nalpha\n\n# Middle\n\nbeta $0section$0\n\n# End\n\nomega\n",
        "SECTION",
        "section_window",
        24,
    );
}

#[test]
fn do_check_section_window_last_section_runs_to_eof() {
    do_check(
        "# Intro\n\nalpha\n\n# End\n\nom$0eg$0a\n",
        "EG",
        "section_window",
        13,
    );
}

#[test]
fn do_check_edit_at_document_start_declines_on_window_size() {
    // The restart clamps to the document start, so the "suffix" would be the
    // whole document: correct, but strictly more expensive than the full parse
    // it duplicates. The window-size cutoff declines before the guard cascade
    // and the window parse run, so the caller pays that full parse and nothing
    // else.
    do_check(
        "$0p$0ara one\n\npara two\n\npara three\n",
        "P",
        "full_reparse",
        31,
    );
}

#[test]
fn do_check_fallback_when_restart_would_pass_the_edit() {
    // Inserting at the blank line after an unterminated fence resolves the
    // enclosing block to the *following* paragraph, putting the restart
    // past the edit start; the guard bails to a full reparse rather than
    // retaining stale pre-edit bytes.
    do_check("```\ncode\n$0$0\npara after\n", "\\", "full_reparse", 22);
}

#[test]
fn incremental_reparse_is_lossless() {
    let input = "para one\n\npara two\n\npara three\n\npara four\n\npara five\n";
    let old_tree = parse(input, None);

    let start = input.find("three").expect("marker present");
    let old_edit = (start, start + 5);
    let updated = apply_edit(input, old_edit, "THREE!!");
    let new_edit = (start, start + 7);

    let inc = reparse_or_full(&updated, None, &old_tree, &[], old_edit, new_edit);
    assert_eq!(
        inc.tree.text().to_string(),
        updated,
        "incremental reparse must round-trip to the edited text"
    );
}

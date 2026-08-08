//! Parser module for Pandoc/Quarto documents.
//!
//! This module implements a single-pass parser that constructs a lossless syntax tree (CST) for
//! Quarto documents.

use crate::options::{Dialect, ParserOptions};
use crate::parser::inlines::refdef_map::{RefdefMap, collect_refdef_labels};
use crate::range_utils::find_incremental_restart_offset;
use crate::syntax::{SyntaxKind, SyntaxNode};

pub mod blocks;
pub mod diagnostics;
pub mod inlines;
pub mod math;
pub mod reparse;
pub mod utils;
pub mod yaml;

mod block_dispatcher;
mod core;
mod verify;

// Re-export main parser
pub use core::Parser;
pub use diagnostics::{Diagnostics, SyntaxError, SyntaxErrorSource};
pub use reparse::{Edit, ReparseStrategy, Reparsed, diff_edit, reparse};
pub use verify::fingerprint;

use verify::assert_matches_full_parse;

/// Parses a Quarto document string into a syntax tree.
///
/// Single-pass architecture: blocks emit inline structure during parsing.
///
/// Convenience wrapper that scans the input for reference-definition
/// labels via [`collect_refdef_labels`] before parsing. Callers that
/// already have a precomputed [`RefdefMap`] (e.g. salsa-cached) should
/// use [`parse_with_refdefs`] instead to skip the scan.
///
/// # Examples
///
/// ```rust
/// use panache_parser::parser::parse;
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
pub fn parse(input: &str, config: Option<ParserOptions>) -> SyntaxNode {
    parse_with_errors(input, config).0
}

/// Like [`parse`], but also returns the syntax errors the parser found in
/// embedded sublanguages (currently malformed frontmatter / hashpipe YAML).
///
/// The errors carry host-aligned ranges, so consumers (the linter) can turn
/// them straight into diagnostics without re-parsing the region or remapping
/// offsets. For pure Markdown the error list is empty.
pub fn parse_with_errors(
    input: &str,
    config: Option<ParserOptions>,
) -> (SyntaxNode, Vec<SyntaxError>) {
    let mut config = config.unwrap_or_default();
    populate_refdef_labels(input, &mut config);
    Parser::new(input, &config).parse_with_errors()
}

/// Parse with a caller-supplied refdef set.
///
/// Skips the [`collect_refdef_labels`] scan that [`parse`] performs.
/// Use this when the caller already has a cached [`RefdefMap`] for
/// `input` — e.g. from a salsa-tracked query — to avoid a redundant
/// document-level scan on every parse.
///
/// The supplied `refdefs` becomes the parser's refdef set, overriding
/// any value previously set on `options.refdef_labels`.
pub fn parse_with_refdefs(
    input: &str,
    options: Option<ParserOptions>,
    refdefs: RefdefMap,
) -> SyntaxNode {
    parse_with_refdefs_and_errors(input, options, refdefs).0
}

/// Like [`parse_with_refdefs`], but also returns embedded-sublanguage syntax
/// errors (see [`parse_with_errors`]). Used by the salsa parse query, which
/// caches the tree and the errors together from a single parse.
pub fn parse_with_refdefs_and_errors(
    input: &str,
    options: Option<ParserOptions>,
    refdefs: RefdefMap,
) -> (SyntaxNode, Vec<SyntaxError>) {
    let mut options = options.unwrap_or_default();
    options.refdef_labels = Some(refdefs);
    Parser::new(input, &options).parse_with_errors()
}

/// Pre-compute the document-level reference link label set.
///
/// CommonMark §6.3 makes reference link resolution depend on whether
/// the label matches a definition that may appear anywhere in the
/// document (including after the use site). The IR-based bracket
/// resolution pass in `inlines::inline_ir` consults this set to
/// distinguish a real shortcut/reference link from bracket-shaped
/// literal text.
///
/// Pandoc-markdown agrees on the document-scoped lookup rule: a
/// `[foo][bar]` shape with no `[bar]: ...` definition is literal text.
/// Both dialects populate this set so the dispatcher's reference-link
/// branch (under Pandoc) and the IR's `process_brackets` pass (under
/// CommonMark) can consult it uniformly.
///
/// Only populated when the caller hasn't already supplied one.
fn populate_refdef_labels(input: &str, config: &mut ParserOptions) {
    if config.refdef_labels.is_some() {
        return;
    }
    config.refdef_labels = Some(collect_refdef_labels(input, config.dialect));
}

pub struct IncrementalParseResult {
    pub tree: SyntaxNode,
    /// Embedded-sublanguage syntax errors for the **whole** document, not just
    /// the reparsed window: the retained prefix's errors are carried over from
    /// the caller-supplied `old_errors` and the window's are shifted into host
    /// coordinates. The governing invariant covers this vector as much as the
    /// tree — it must equal a full parse's, byte for byte.
    pub errors: Vec<SyntaxError>,
    pub reparse_range: (usize, usize),
    pub strategy: &'static str,
}

/// Splice the syntax errors of a reparse the same way the green children are
/// spliced: prefix kept, everything from `seam` onward re-derived.
///
/// Two buckets, not rust-analyzer's three. Both strategies parse their window
/// to EOF and both window starts are `<= edit.0`, where `map_old_offset_to_new`
/// is the identity — so the seam sits at the same offset in the old and the new
/// text and no error can straddle it. The straddling case is a
/// `debug_assert!` plus a `None` (which the caller turns into a bail) rather
/// than a guess. A real third bucket only appears once a *bounded* window
/// leaves a live suffix to shift by the edit delta, which is the region tier in
/// roadmap Phase 8.
///
/// Old errors that start at or after the seam are dropped: the window parse
/// re-derives them.
fn merge_incremental_errors(
    old_errors: &[SyntaxError],
    seam: usize,
    window_errors: Vec<SyntaxError>,
) -> Option<Vec<SyntaxError>> {
    let seam = rowan::TextSize::new(seam as u32);
    let mut merged = Vec::with_capacity(old_errors.len() + window_errors.len());

    for error in old_errors {
        if error.range.end() <= seam {
            merged.push(error.clone());
        } else if error.range.start() < seam {
            debug_assert!(
                false,
                "syntax error {:?} straddles the splice seam at {seam:?}",
                error.range
            );
            return None;
        }
    }

    for error in window_errors {
        merged.push(SyntaxError {
            range: rowan::TextRange::new(error.range.start() + seam, error.range.end() + seam),
            ..error
        });
    }

    debug_assert!(
        merged
            .windows(2)
            .all(|w| w[0].range.start() <= w[1].range.start()),
        "merged syntax errors are out of document order: {merged:?}"
    );
    Some(merged)
}

/// Incrementally update a syntax tree by reparsing either a bounded section
/// window (between top-level headings) or from a safe restart boundary to EOF.
///
/// Convenience wrapper that scans `input` for refdef labels via
/// [`collect_refdef_labels`] before reparsing. Callers that already have
/// a precomputed [`RefdefMap`] (e.g. salsa-cached) should use
/// [`parse_incremental_suffix_with_refdefs`] to skip the scan.
///
/// `old_errors` are the syntax errors of the *old* parse (the one that
/// produced `old_tree`); the retained prefix's share of them is carried into
/// the result. Pass `&[]` only when the old parse genuinely had none —
/// under-reporting here is a divergence the debug oracle will catch.
pub fn parse_incremental_suffix(
    input: &str,
    config: Option<ParserOptions>,
    old_tree: &SyntaxNode,
    old_errors: &[SyntaxError],
    old_edit_range: (usize, usize),
    new_edit_range: (usize, usize),
) -> IncrementalParseResult {
    let mut config = config.unwrap_or_default();
    populate_refdef_labels(input, &mut config);
    parse_incremental_suffix_inner(
        input,
        config,
        old_tree,
        old_errors,
        old_edit_range,
        new_edit_range,
    )
}

/// Incremental reparse with a caller-supplied refdef set.
///
/// See [`parse_with_refdefs`] for the rationale; this is the
/// incremental-parse counterpart. The supplied `refdefs` overrides any
/// value previously set on `options.refdef_labels`.
pub fn parse_incremental_suffix_with_refdefs(
    input: &str,
    options: Option<ParserOptions>,
    refdefs: RefdefMap,
    old_tree: &SyntaxNode,
    old_errors: &[SyntaxError],
    old_edit_range: (usize, usize),
    new_edit_range: (usize, usize),
) -> IncrementalParseResult {
    let mut options = options.unwrap_or_default();
    options.refdef_labels = Some(refdefs);
    parse_incremental_suffix_inner(
        input,
        options,
        old_tree,
        old_errors,
        old_edit_range,
        new_edit_range,
    )
}

fn parse_incremental_suffix_inner(
    input: &str,
    config: ParserOptions,
    old_tree: &SyntaxNode,
    old_errors: &[SyntaxError],
    old_edit_range: (usize, usize),
    new_edit_range: (usize, usize),
) -> IncrementalParseResult {
    match reparse_ranges(
        input,
        &config,
        old_tree,
        old_errors,
        old_edit_range,
        new_edit_range,
    ) {
        Some(reparsed) => IncrementalParseResult {
            tree: SyntaxNode::new_root(reparsed.green),
            errors: reparsed.errors,
            reparse_range: reparsed.reparse_range,
            strategy: reparsed.strategy.as_str(),
        },
        None => full_reparse_result(input, &config),
    }
}

/// The guard cascade and splice behind [`reparse`], expressed over the old and
/// new edit ranges rather than an [`Edit`].
///
/// Refusal-first: every guard returns [`None`] and leaves the full parse to the
/// caller. Never returns a best-effort tree.
fn reparse_ranges(
    input: &str,
    config: &ParserOptions,
    old_tree: &SyntaxNode,
    old_errors: &[SyntaxError],
    old_edit_range: (usize, usize),
    new_edit_range: (usize, usize),
) -> Option<Reparsed> {
    let input_len = input.len();

    let old_edit = normalize_range(old_edit_range)?;
    let new_edit = normalize_range(new_edit_range)?;
    if new_edit.1 > input_len {
        return None;
    }

    if old_tree.kind() != SyntaxKind::DOCUMENT {
        return None;
    }

    // The edit is described in the *old* text's coordinates, so an edit range
    // past the old tree's end means the caller's tree and text have gone out
    // of sync. Bail rather than let the offset reach rowan, which panics on a
    // range it cannot resolve.
    if old_edit.1 > usize::from(old_tree.text_range().end()) {
        return None;
    }

    // Reference definitions are document-scoped: retained blocks keep the
    // resolution they were parsed with, so an edit that can add, remove, or
    // alter a refdef (or footnote definition) invalidates them at a
    // distance. Bail on cheap textual evidence near the edit; the precise
    // old-set-vs-new-set comparison belongs to the host layer, which caches
    // both sets.
    if edit_may_touch_refdefs(old_tree, old_edit, input, new_edit) {
        return None;
    }

    if let Some(section_window) =
        find_top_level_heading_section_window(old_tree, old_edit, new_edit, input_len)
        && let Some(result) =
            reparse_section_window(input, config, old_tree, old_errors, section_window)
    {
        assert_matches_full_parse(&result, input, config);
        return Some(result);
    }

    let restart = find_incremental_restart_offset(old_tree, old_edit.0, old_edit.1);
    let old_restart = align_to_document_child_start(old_tree, restart);

    // The retained prefix is `old_tree`'s bytes up to the restart, so the
    // restart must not lie past the edit start: a later restart would keep
    // stale pre-edit bytes and drop the edit from the spliced tree. (An
    // edit at a blank line between blocks can produce such a restart — the
    // enclosing-block lookup resolves to the *following* block.)
    if old_restart > old_edit.0 {
        return None;
    }

    let new_restart = map_old_offset_to_new(old_restart, old_edit, new_edit, input_len);
    if !input.is_char_boundary(new_restart) {
        return None;
    }

    // Seam decoupling: the suffix is parsed as a standalone document, so
    // nothing may couple across the splice seam in the edited text.
    // Backward: the retained prefix must end at a blank line (or the seam
    // is the document start), else the suffix's first line could continue
    // a prefix paragraph, turn it into a setext heading, or attach lazily
    // to a prefix container. Forward-into-prefix: an indented suffix start
    // can be absorbed by a trailing prefix list, footnote definition, or
    // indented code block even across blank lines, so it also bails.
    if !seam_is_decoupled(input, new_restart)
        || !prefix_ends_structurally_decoupled(old_tree, old_restart)
    {
        return None;
    }

    // Fence pairing is global: an edit in the suffix can pair with (or
    // orphan) a fence-capable line retained in the prefix, flipping the
    // prefix's interpretation.
    if !prefix_fence_state_is_stable(&input[..new_restart]) {
        return None;
    }

    let suffix_text = &input[new_restart..];

    // The window is parsed standalone, so its first line looks like the
    // document's first line to the block dispatcher.
    if new_restart > 0 && window_start_manufactures_document_start_construct(suffix_text, config) {
        return None;
    }

    // A list (or definition-list) block continues across blank lines when
    // the next non-blank line carries a compatible marker, so a suffix
    // whose first line looks like a list continuation must not be parsed
    // standalone after a retained trailing list.
    if last_retained_block_can_absorb_marker(old_tree, old_restart)
        && first_nonblank_line_is_container_marker(suffix_text)
    {
        return None;
    }

    // Two backward couplings the seam's blank line does not break, because
    // pandoc permits a blank line in both: a `:`/`~` line after a paragraph
    // promotes it to a definition-list `TERM`, and the same line after a table
    // becomes that table's caption. Parsed standalone the suffix is only a
    // paragraph starting with a colon, so the splice keeps two blocks where a
    // full parse has one.
    //
    // A retained thematic break is the mirror image: `---` is also a
    // multiline-table border, so suffix content can turn the retained rule into
    // the top rule of a table that swallows the seam. `prefix_fence_state_is_stable`
    // cannot carry this one — dash runs are far too common for a parity count.
    if last_retained_block_kind(old_tree, old_restart)
        .is_some_and(last_retained_block_absorbs_colon_line)
        && first_nonblank_line_is_definition_marker(suffix_text)
    {
        return None;
    }

    // The mirror image: a bare dash run is a multiline-table rule as well as a
    // thematic break, and table rules pair across blank lines like a fence. A
    // dash run retained as a `HORIZONTAL_RULE` can therefore be re-read as the
    // *top* rule of a table that swallows the seam once the window supplies a
    // partner. `prefix_fence_state_is_stable` cannot carry this one: dash runs
    // are far too common for a parity count, so the guard reads the old tree
    // for a rule that actually parsed as one, and only bails when the window
    // has a partner to offer.
    if retained_prefix_has_thematic_break(old_tree, old_restart) && has_dash_rule_line(suffix_text)
    {
        return None;
    }
    let (suffix_tree, suffix_errors) = Parser::new(suffix_text, config).parse_with_errors();
    let errors = merge_incremental_errors(old_errors, new_restart, suffix_errors)?;

    // Splice on the green tree directly: retain the prefix children verbatim
    // (rowan's structural sharing keeps their `Arc` identity) and replace
    // everything from the restart boundary onward with the reparsed suffix.
    let old_green = old_tree.green();
    let split = first_child_ending_after(old_green, old_restart);
    let suffix_green = suffix_tree.green();
    let new_green = old_green.splice_children(
        split..,
        suffix_green.children().map(|child| child.to_owned()),
    );

    let len: usize = new_green.text_len().into();
    let result = Reparsed {
        green: new_green,
        errors,
        reparse_range: (new_restart, len),
        strategy: ReparseStrategy::SuffixWindow,
    };
    assert_matches_full_parse(&result, input, config);
    Some(result)
}

fn normalize_range(range: (usize, usize)) -> Option<(usize, usize)> {
    (range.0 <= range.1).then_some(range)
}

/// How far around an edit the refdef guard scans. Refdef and footnote
/// definition lines are short; a label whose `]:` sits further than this
/// from the edit is not worth the precision.
const REFDEF_SCAN_WINDOW: usize = 512;

/// Whether the edit could add, remove, or alter a reference or footnote
/// definition, judged by cheap textual evidence: a `]:` occurrence within a
/// bounded window around the edit, in the old text or the new. False
/// positives (a literal `]:` in prose) only cost a full reparse.
fn edit_may_touch_refdefs(
    old_tree: &SyntaxNode,
    old_edit: (usize, usize),
    input: &str,
    new_edit: (usize, usize),
) -> bool {
    let old_len: usize = old_tree.text_range().end().into();
    // Snap to token boundaries before slicing: a window edge landing inside a
    // multi-byte token is not a char boundary, and `SyntaxText::slice` panics
    // on one. Snapping outward also only ever widens the scan, which is the
    // safe direction for a conservative guard.
    let old_start = snap_out_to_token_boundary(
        old_tree,
        old_edit.0.saturating_sub(REFDEF_SCAN_WINDOW),
        false,
    );
    let old_end = snap_out_to_token_boundary(
        old_tree,
        old_edit.1.saturating_add(REFDEF_SCAN_WINDOW).min(old_len),
        true,
    );
    if old_start < old_end {
        let old_slice = old_tree
            .text()
            .slice(rowan::TextRange::new(
                (old_start as u32).into(),
                (old_end as u32).into(),
            ))
            .to_string();
        if old_slice.contains("]:") {
            return true;
        }
    }

    let new_start = new_edit.0.saturating_sub(REFDEF_SCAN_WINDOW);
    let new_end = new_edit
        .1
        .saturating_add(REFDEF_SCAN_WINDOW)
        .min(input.len());
    let new_start = floor_char_boundary(input, new_start);
    let new_end = floor_char_boundary(input, new_end);
    new_start < new_end && input[new_start..new_end].contains("]:")
}

/// Move `offset` outward (down when `upward` is false, up when it is true) to
/// the nearest token edge of `tree`.
///
/// Token edges are always char boundaries — a token's text is a whole `str` —
/// so this is how an arbitrary byte offset is made safe to slice the tree
/// with, without materializing the document's text.
fn snap_out_to_token_boundary(tree: &SyntaxNode, offset: usize, upward: bool) -> usize {
    let len: usize = tree.text_range().end().into();
    let clamped = offset.min(len);
    let at = tree.token_at_offset((clamped as u32).into());
    let snapped = if upward {
        at.right_biased()
            .map(|token| usize::from(token.text_range().end()))
    } else {
        at.left_biased()
            .map(|token| usize::from(token.text_range().start()))
    };
    snapped.unwrap_or(if upward { len } else { 0 }).min(len)
}

fn floor_char_boundary(text: &str, mut pos: usize) -> usize {
    while pos > 0 && !text.is_char_boundary(pos) {
        pos -= 1;
    }
    pos
}

/// Whether the fence-pairing state of a retained prefix is visibly stable.
///
/// Fence-like delimiters (backtick/tilde code fences, `:::` div fences,
/// `$$` display math, HTML comments) pair across blank lines, so their
/// interpretation is global: an edit after the prefix can supply or remove
/// a pairing partner and flip how prefix lines parse. This is a parity
/// heuristic — every delimiter-capable line in the prefix must have a
/// partner — not a proof; delimiter-looking lines in prose (e.g. docs
/// about Markdown) can fool it in both directions. The debug oracle
/// backstops false negatives; false positives only cost a full reparse.
/// The precise check (asking the old tree whether each candidate line is a
/// closed fence delimiter) is roadmap Phase 8 material.
fn prefix_fence_state_is_stable(prefix: &str) -> bool {
    let mut backtick = 0usize;
    let mut tilde = 0usize;
    let mut colon = 0usize;
    let mut math = 0usize;
    for line in prefix.lines() {
        let trimmed = line.trim_start_matches(' ');
        if line.len() - trimmed.len() > 3 {
            continue;
        }
        if trimmed.starts_with("```") {
            backtick += 1;
        } else if trimmed.starts_with("~~~") {
            tilde += 1;
        } else if trimmed.starts_with(":::") {
            colon += 1;
        } else if trimmed.starts_with("$$") {
            math += 1;
        }
    }
    backtick.is_multiple_of(2)
        && tilde.is_multiple_of(2)
        && colon.is_multiple_of(2)
        && math.is_multiple_of(2)
        && prefix.matches("<!--").count() == prefix.matches("-->").count()
}

/// Whether the retained prefix is *structurally* decoupled at `boundary`:
/// the last retained top-level child must be a `BLANK_LINE` node (or
/// nothing is retained). The textual blank-line check in
/// [`seam_is_decoupled`] is blind to containment — an unterminated
/// container (open `:::` div, fence, comment) can hold the seam's blank
/// line *inside* itself and still absorb anything appended after it, which
/// this check catches because the blank line is then not a top-level
/// child.
fn prefix_ends_structurally_decoupled(old_tree: &SyntaxNode, boundary: usize) -> bool {
    old_tree
        .children_with_tokens()
        .take_while(|child| usize::from(child.text_range().end()) <= boundary)
        .last()
        .is_none_or(|child| child.kind() == SyntaxKind::BLANK_LINE)
}

/// Whether the last retained top-level block before `boundary` is a
/// container that absorbs marker-led lines across blank lines.
fn last_retained_block_can_absorb_marker(old_tree: &SyntaxNode, boundary: usize) -> bool {
    old_tree
        .children()
        .take_while(|child| usize::from(child.text_range().end()) <= boundary)
        .filter(|child| child.kind() != SyntaxKind::BLANK_LINE)
        .last()
        .is_some_and(|child| {
            matches!(
                child.kind(),
                SyntaxKind::LIST | SyntaxKind::DEFINITION_LIST | SyntaxKind::BLOCK_QUOTE
            )
        })
}

/// The kind of the last retained top-level block before `boundary`, ignoring
/// blank lines.
fn last_retained_block_kind(old_tree: &SyntaxNode, boundary: usize) -> Option<SyntaxKind> {
    old_tree
        .children()
        .take_while(|child| usize::from(child.text_range().end()) <= boundary)
        .filter(|child| child.kind() != SyntaxKind::BLANK_LINE)
        .last()
        .map(|child| child.kind())
}

/// Whether a `:`/`~` marker line in the suffix would rewrite the last retained
/// block: a paragraph becomes a definition-list `TERM`, and a table absorbs
/// the line as its `TABLE_CAPTION`.
fn last_retained_block_absorbs_colon_line(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::PARAGRAPH
            | SyntaxKind::SIMPLE_TABLE
            | SyntaxKind::MULTILINE_TABLE
            | SyntaxKind::PIPE_TABLE
            | SyntaxKind::GRID_TABLE
    )
}

/// Whether any retained top-level block before `boundary` parsed as a
/// thematic break — a dash run that a multiline table could claim as a rule.
fn retained_prefix_has_thematic_break(old_tree: &SyntaxNode, boundary: usize) -> bool {
    old_tree
        .children()
        .take_while(|child| usize::from(child.text_range().end()) <= boundary)
        .any(|child| child.kind() == SyntaxKind::HORIZONTAL_RULE)
}

/// Whether `text` has a line that is nothing but a run of three or more
/// dashes: a multiline-table rule candidate.
fn has_dash_rule_line(text: &str) -> bool {
    text.lines().any(|line| {
        let trimmed = line.trim();
        trimmed.len() >= 3 && trimmed.bytes().all(|b| b == b'-')
    })
}

/// Whether the first non-blank line of `text` is a definition-list marker
/// line (`:` or `~` followed by space or end of line).
///
/// Distinct from [`first_nonblank_line_is_container_marker`] in what it
/// couples to: a container marker continues an open *container*, while a
/// definition marker reaches back and rewrites the preceding *paragraph*.
/// `:::` and `~~~` are fence openers, not markers, so the rest of the run must
/// not be another delimiter character.
fn first_nonblank_line_is_definition_marker(text: &str) -> bool {
    let Some(line) = text.lines().find(|line| !line.trim().is_empty()) else {
        return false;
    };
    let trimmed = line.trim_start();
    trimmed
        .strip_prefix([':', '~'])
        .is_some_and(|rest| rest.is_empty() || rest.starts_with(' ') || rest.starts_with('\t'))
}

/// Whether the first non-blank line of `text` starts with a marker that
/// could continue a preceding container: bullet or ordered list markers
/// (including pandoc's fancy single-letter forms), a definition-list `:`,
/// or a blockquote `>`.
fn first_nonblank_line_is_container_marker(text: &str) -> bool {
    let Some(line) = text.lines().find(|line| !line.trim().is_empty()) else {
        return false;
    };
    let trimmed = line.trim_start();
    // A blockquote marker needs no following space.
    if trimmed.starts_with('>') {
        return true;
    }
    if let Some(rest) = trimmed.strip_prefix(['-', '*', '+', ':']) {
        return rest.is_empty() || rest.starts_with(' ') || rest.starts_with('\t');
    }
    if trimmed.starts_with('(') {
        return true;
    }
    let marker_len = trimmed
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric())
        .count();
    if marker_len > 0 && marker_len <= 9 {
        let rest = &trimmed[marker_len..];
        if let Some(rest) = rest.strip_prefix(['.', ')']) {
            return rest.is_empty() || rest.starts_with(' ');
        }
    }
    false
}

/// Whether the first line of a window would be read as a construct only a
/// document's *first* line can produce.
///
/// A window is parsed as a standalone document, so `at_document_start` is true
/// at its first line. Three constructs test that flag with no
/// `|| ctx.has_blank_before` escape, and each would therefore be manufactured
/// where a full parse refuses:
///
/// - a pandoc `%` title block (`PandocTitleBlockParser::detect_prepared`),
/// - a MultiMarkdown title block (`MmdTitleBlockParser::detect_prepared`),
/// - under the CommonMark dialect, a YAML metadata block: those readers
///   recognize YAML only as frontmatter, and read a body `---` as a thematic
///   break. This one also manufactures *syntax errors*, malformed YAML being
///   the only source of them.
///
/// Every other `at_document_start` consumer is `||`-ed with `has_blank_before`,
/// which the seam guard already guarantees, so they agree either way. Textual
/// and deliberately over-eager: a false positive costs one full parse. The
/// principled fix is to separate "byte 0 of the document" from "blank-line
/// separated fragment start" in `BlockContext`, which belongs with the region
/// tier in roadmap Phase 8.
fn window_start_manufactures_document_start_construct(
    window: &str,
    config: &ParserOptions,
) -> bool {
    let Some(first) = window.lines().next() else {
        return false;
    };
    if config.extensions.pandoc_title_block && first.trim_start().starts_with('%') {
        return true;
    }
    if config.extensions.mmd_title_block && !first.trim().is_empty() && first.contains(':') {
        return true;
    }
    config.dialect == Dialect::CommonMark && first.trim() == "---"
}

/// Whether a splice seam at `seam` is structurally decoupled in `text`:
/// the prefix before it ends with a blank line (or is empty), and the
/// suffix's first non-blank line is not indented. Blank separation kills
/// paragraph/lazy continuation and setext underlines; the indentation check
/// covers list, footnote, and indented-code continuation, which absorb
/// indented lines even across blank lines.
fn seam_is_decoupled(text: &str, seam: usize) -> bool {
    if seam != 0 && !text[..seam].ends_with("\n\n") {
        return false;
    }
    let first_nonblank_indented = text[seam..]
        .lines()
        .find(|line| !line.trim().is_empty())
        .is_some_and(|line| line.starts_with(' ') || line.starts_with('\t'));
    !first_nonblank_indented
}

fn full_reparse_result(input: &str, config: &ParserOptions) -> IncrementalParseResult {
    let (tree, errors) = Parser::new(input, config).parse_with_errors();
    let len: usize = tree.text_range().end().into();
    IncrementalParseResult {
        tree,
        errors,
        reparse_range: (0, len),
        strategy: "full_reparse",
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

/// Index of the first top-level child whose end offset exceeds `pos`.
///
/// Every child before this index ends at or before `pos`, so those children can
/// be retained verbatim (by `Arc` identity) when splicing.
fn first_child_ending_after(green: &rowan::GreenNodeData, pos: usize) -> usize {
    let mut offset = 0usize;
    for (index, child) in green.children().enumerate() {
        let len: usize = child.text_len().into();
        offset += len;
        if offset > pos {
            return index;
        }
    }
    green.children().count()
}

/// Index of the first top-level child that starts at or after `pos`.
///
/// Together with [`first_child_ending_after`] this bounds the child range a
/// section-window reparse replaces, so the surrounding children keep their
/// `Arc` identity across the splice.
fn first_child_starting_at_or_after(green: &rowan::GreenNodeData, pos: usize) -> usize {
    let mut offset = 0usize;
    for (index, child) in green.children().enumerate() {
        if offset >= pos {
            return index;
        }
        let len: usize = child.text_len().into();
        offset += len;
    }
    green.children().count()
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
    let old_len: usize = old_tree.text_range().end().into();
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
    let (next_start, next_end) = next_heading.unwrap_or((old_len, old_len));

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
    config: &ParserOptions,
    old_tree: &SyntaxNode,
    old_errors: &[SyntaxError],
    section_window: SectionWindow,
) -> Option<Reparsed> {
    if !input.is_char_boundary(section_window.new_start)
        || !input.is_char_boundary(section_window.new_end)
    {
        return None;
    }

    // Backward seam: the reparsed region is parsed without its prefix, so
    // the retained prefix must be blank-line separated from it (see
    // `seam_is_decoupled`), and the prefix's fence-pairing state must not
    // be flippable by the reparsed content (`prefix_fence_state_is_stable`).
    if !seam_is_decoupled(input, section_window.new_start)
        || !prefix_ends_structurally_decoupled(old_tree, section_window.old_start)
        || !prefix_fence_state_is_stable(&input[..section_window.new_start])
    {
        return None;
    }

    // Defensive: a section window starts at a top-level `HEADING`, which is
    // none of these shapes, so this cannot fire today. It is here because that
    // is a property of how the window is *chosen*, not of the splice, and the
    // region tier will choose differently.
    if section_window.new_start > 0
        && window_start_manufactures_document_start_construct(
            &input[section_window.new_start..],
            config,
        )
    {
        return None;
    }

    // Parse from the window start TO EOF, not just to the window end: block
    // decisions inside the window (list-item buffering, tightness, div and
    // fence pairing) can depend on unbounded lookahead past the window, so
    // a standalone parse of only the window text is not trustworthy. This
    // costs the same as the suffix strategy's parse; the win over it is
    // below, where the unchanged tail is re-adopted by `Arc` identity.
    let (tail_tree, tail_errors) =
        Parser::new(&input[section_window.new_start..], config).parse_with_errors();
    // The tail parse covers everything from the window start to EOF, so it
    // re-derives the errors of the re-adopted suffix too — which is sound
    // precisely because that suffix is only re-adopted on structural equality.
    let errors = merge_incremental_errors(old_errors, section_window.new_start, tail_errors)?;
    let tail_green = tail_tree.green();
    let window_len = section_window.new_end - section_window.new_start;

    let old_green = old_tree.green();
    let start_idx = first_child_ending_after(old_green, section_window.old_start);
    let end_idx = first_child_starting_at_or_after(old_green, section_window.old_end);

    // Try to re-adopt the old tree's suffix: if the freshly parsed tail has
    // a top-level boundary exactly at the window end and its beyond-window
    // children are structurally equal to the old suffix children, splice
    // only the window portion so the retained suffix keeps its `Arc`
    // identity (which downstream per-block memoization relies on).
    let mut offset = 0usize;
    let mut boundary_idx = None;
    for (index, child) in tail_green.children().enumerate() {
        if offset == window_len {
            boundary_idx = Some(index);
            break;
        }
        if offset > window_len {
            break;
        }
        offset += usize::from(child.text_len());
    }
    if boundary_idx.is_none() && offset == window_len {
        boundary_idx = Some(tail_green.children().count());
    }

    if let Some(boundary_idx) = boundary_idx {
        let tail_matches_old_suffix = tail_green.children().count() - boundary_idx
            == old_green.children().count() - end_idx
            && tail_green
                .children()
                .skip(boundary_idx)
                .zip(old_green.children().skip(end_idx))
                .all(|(new_child, old_child)| new_child == old_child);
        if tail_matches_old_suffix {
            let new_green = old_green.splice_children(
                start_idx..end_idx,
                tail_green
                    .children()
                    .take(boundary_idx)
                    .map(|child| child.to_owned()),
            );
            return Some(Reparsed {
                green: new_green,
                errors,
                reparse_range: (section_window.new_start, section_window.new_end),
                strategy: ReparseStrategy::SectionWindow,
            });
        }
    }

    // No adoptable boundary: the parsed tail is still a correct parse of
    // everything from the window start, so splice it wholesale (this is the
    // suffix strategy anchored at the section start).
    let new_green = old_green.splice_children(
        start_idx..,
        tail_green.children().map(|child| child.to_owned()),
    );
    let len: usize = new_green.text_len().into();
    Some(Reparsed {
        green: new_green,
        errors,
        reparse_range: (section_window.new_start, len),
        strategy: ReparseStrategy::SuffixWindow,
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

    fn flavor_options(flavor: crate::options::Flavor) -> ParserOptions {
        ParserOptions {
            flavor,
            extensions: crate::options::Extensions::for_flavor(flavor),
            dialect: crate::options::Dialect::for_flavor(flavor),
            ..Default::default()
        }
    }

    fn quarto_options() -> ParserOptions {
        flavor_options(crate::options::Flavor::Quarto)
    }

    /// Apply `insert` at `at` (a pure insertion) and reparse incrementally.
    fn insert_incrementally(
        input: &str,
        at: usize,
        insert: &str,
        options: ParserOptions,
    ) -> IncrementalParseResult {
        let (old_tree, old_errors) = parse_with_errors(input, Some(options.clone()));
        let updated = apply_edit(input, (at, at), insert);
        parse_incremental_suffix(
            &updated,
            Some(options),
            &old_tree,
            &old_errors,
            (at, at),
            (at, at + insert.len()),
        )
    }

    #[test]
    fn parse_with_errors_reports_malformed_hashpipe_yaml() {
        let input = "```{r}\n#| echo: [\n1 + 1\n```\n";
        let (_tree, errors) = parse_with_errors(input, Some(quarto_options()));
        assert_eq!(errors.len(), 1, "expected one yaml error, got {errors:?}");
        assert_eq!(errors[0].source, SyntaxErrorSource::Yaml);
        let start: usize = errors[0].range.start().into();
        assert_eq!(start, input.find('[').unwrap());
    }

    #[test]
    fn parse_with_errors_reports_malformed_frontmatter_yaml() {
        let input = "---\ntitle: [\n---\n";
        let (_tree, errors) = parse_with_errors(input, None);
        assert_eq!(errors.len(), 1, "expected one yaml error, got {errors:?}");
        assert_eq!(errors[0].source, SyntaxErrorSource::Yaml);
        let start: usize = errors[0].range.start().into();
        assert_eq!(start, input.find('[').unwrap());
    }

    #[test]
    fn parse_with_errors_empty_for_valid_document() {
        let input = "---\ntitle: ok\n---\n\n```{r}\n#| echo: false\n1\n```\n";
        let (_tree, errors) = parse_with_errors(input, Some(quarto_options()));
        assert!(
            errors.is_empty(),
            "valid document should have no errors: {errors:?}"
        );
    }

    fn yaml_error(start: u32, end: u32) -> SyntaxError {
        SyntaxError {
            range: rowan::TextRange::new(start.into(), end.into()),
            message: format!("error at {start}"),
            source: SyntaxErrorSource::Yaml,
        }
    }

    #[test]
    fn merge_keeps_prefix_errors_and_shifts_window_errors() {
        let old = vec![yaml_error(3, 9)];
        let merged = merge_incremental_errors(&old, 20, vec![yaml_error(2, 5)]).unwrap();
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0], old[0]);
        assert_eq!(merged[1].range, rowan::TextRange::new(22.into(), 25.into()));
        assert_eq!(merged[1].message, "error at 2");
    }

    #[test]
    fn merge_drops_old_errors_the_window_reparses() {
        // The window parse re-derives everything from the seam onward, so an
        // old error there must not be carried over as well (it would double).
        let old = vec![yaml_error(3, 9), yaml_error(30, 40)];
        let merged = merge_incremental_errors(&old, 20, vec![yaml_error(10, 20)]).unwrap();
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0], old[0]);
        assert_eq!(merged[1].range, rowan::TextRange::new(30.into(), 40.into()));
    }

    #[test]
    fn merge_of_error_free_parses_is_empty() {
        assert!(
            merge_incremental_errors(&[], 12, Vec::new())
                .unwrap()
                .is_empty()
        );
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "straddles the splice seam")]
    fn merge_refuses_an_error_straddling_the_seam() {
        merge_incremental_errors(&[yaml_error(15, 25)], 20, Vec::new());
    }

    // A window is parsed as a standalone document, so `at_document_start` is
    // true at its first line. These three constructs are the only ones that
    // test it *without* an `|| has_blank_before` escape, so a window starting
    // on one of them manufactures a block a full parse would never produce.
    // Each case is a plausible keystroke: finishing the marker of a
    // frontmatter-shaped block that is not at the document start.

    #[test]
    fn refdef_guard_survives_a_scan_window_edge_inside_a_multibyte_token() {
        // Fuzz find (math.qmd, seed 12647662): the refdef guard sliced the old
        // tree at `edit +/- REFDEF_SCAN_WINDOW`, and an edge landing inside a
        // multi-byte token is not a char boundary, which `SyntaxText::slice`
        // panics on. The emoji is placed so that one edge falls inside it.
        let mut input = String::from("para one\n\n");
        input.push_str(&"x".repeat(REFDEF_SCAN_WINDOW - 12));
        input.push_str("\u{2705}\n\npara two\n");
        let edit_at = input.find("para two").unwrap();

        let (old_tree, old_errors) = parse_with_errors(&input, None);
        let updated = apply_edit(&input, (edit_at, edit_at + 8), "para three");
        let inc = parse_incremental_suffix(
            &updated,
            None,
            &old_tree,
            &old_errors,
            (edit_at, edit_at + 8),
            (edit_at, edit_at + 10),
        );
        assert_eq!(inc.tree.text().to_string(), updated);
    }

    #[test]
    fn incremental_bails_when_the_edit_range_exceeds_the_old_tree() {
        // A caller whose tree and text have drifted apart hands over an edit
        // the old tree cannot resolve. The fuzz harness reached this with a
        // lossy base parse, and rowan turns it into a panic; the parser must
        // bail instead.
        let input = "para one\n\npara two\n";
        let (old_tree, old_errors) = parse_with_errors(input, None);
        let past_end = usize::from(old_tree.text_range().end()) + 5;
        let inc = parse_incremental_suffix(
            input,
            None,
            &old_tree,
            &old_errors,
            (past_end, past_end + 4),
            (0, 0),
        );
        assert_eq!(inc.strategy, "full_reparse");
    }

    #[test]
    fn suffix_window_must_not_manufacture_a_pandoc_title_block() {
        let input = "intro para\n\n Title\n% Author\n\ntail para\n";
        let at = input.find(" Title").unwrap();
        let inc = insert_incrementally(
            input,
            at,
            "%",
            flavor_options(crate::options::Flavor::Pandoc),
        );
        assert_eq!(inc.strategy, "full_reparse");
    }

    #[test]
    fn suffix_window_must_not_manufacture_mid_document_yaml_under_commonmark() {
        // Under a CommonMark-family dialect `---`/`key: value`/`---` in the
        // body is a thematic break plus a setext heading, never metadata.
        let input = "intro para\n\n--\nkey: value\n---\n\ntail para\n";
        let at = input.find("--\nkey").unwrap() + 2;
        let inc = insert_incrementally(input, at, "-", flavor_options(crate::options::Flavor::Gfm));
        assert_eq!(inc.strategy, "full_reparse");
    }

    #[test]
    fn suffix_window_must_not_manufacture_an_mmd_title_block() {
        let input = "intro para\n\nKey value\nOther: thing\n\ntail para\n";
        let at = input.find("Key value").unwrap() + 3;
        let inc = insert_incrementally(
            input,
            at,
            ":",
            flavor_options(crate::options::Flavor::MultiMarkdown),
        );
        assert_eq!(inc.strategy, "full_reparse");
    }

    #[test]
    fn suffix_window_still_reuses_when_the_construct_is_not_live() {
        // The same edit under Pandoc, where `mmd_title_block` is off: the
        // guard must key on the extension, not on the line's shape.
        let input = "intro para\n\nKey value\nOther: thing\n\ntail para\n";
        let at = input.find("Key value").unwrap() + 3;
        let inc = insert_incrementally(
            input,
            at,
            ":",
            flavor_options(crate::options::Flavor::Pandoc),
        );
        assert_eq!(inc.strategy, "suffix_window");
    }

    #[test]
    fn incremental_suffix_matches_full_parse_for_tail_edit() {
        let input = "# H\n\npara one\n\npara two\n\npara three\n";
        let old_tree = parse(input, None);
        let old_edit = (30, 35);
        let updated = apply_edit(input, old_edit, "tail section");
        let new_edit = (30, 42);

        let inc = parse_incremental_suffix(&updated, None, &old_tree, &[], old_edit, new_edit).tree;
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

        let inc = parse_incremental_suffix(&updated, None, &old_tree, &[], old_edit, new_edit).tree;
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

        let inc = parse_incremental_suffix(&updated, None, &old_tree, &[], old_edit, new_edit).tree;
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

        let inc = parse_incremental_suffix(&updated, None, &old_tree, &[], old_edit, new_edit).tree;
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

        let inc = parse_incremental_suffix(&updated, None, &old_tree, &[], old_edit, new_edit);
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

    #[test]
    fn incremental_uses_section_window_for_last_section() {
        let input = "# Intro\n\nalpha\n\n# Last\n\nbeta section\n";
        let old_tree = parse(input, None);
        let start = input.find("beta").expect("beta in test input");
        let old_edit = (start, start + 4);
        let updated = apply_edit(input, old_edit, "BETA");
        let new_edit = (start, start + 4);

        let inc = parse_incremental_suffix(&updated, None, &old_tree, &[], old_edit, new_edit);
        let full = parse(&updated, None);
        assert_eq!(inc.tree.to_string(), full.to_string());
        assert!(
            inc.reparse_range.0 > 0,
            "last section should start at the last heading boundary"
        );
        assert_eq!(
            inc.reparse_range.1,
            updated.len(),
            "last section should end at EOF"
        );
    }

    #[test]
    fn incremental_does_not_use_section_window_when_edit_touches_heading() {
        let input = "# Intro\n\nalpha\n\n# Middle\n\nbeta\n\n# End\n\nomega\n";
        let old_tree = parse(input, None);
        let middle_start = input
            .find("# Middle")
            .expect("middle heading in test input");
        let old_edit = (middle_start, middle_start + 1);
        let updated = apply_edit(input, old_edit, "#");
        let new_edit = (middle_start, middle_start + 1);

        let inc = parse_incremental_suffix(&updated, None, &old_tree, &[], old_edit, new_edit);
        let full = parse(&updated, None);
        assert_eq!(inc.tree.to_string(), full.to_string());
        assert_eq!(
            inc.reparse_range.1,
            updated.len(),
            "edits on headings should avoid section-window reparsing"
        );
    }

    #[test]
    fn incremental_does_not_use_section_window_when_edit_crosses_next_heading() {
        let input = "# Intro\n\nalpha\n\n# Middle\n\nbeta\n\n# End\n\nomega\n";
        let old_tree = parse(input, None);
        let beta_start = input.find("beta").expect("beta in test input");
        let end_start = input.find("# End").expect("end heading in test input");
        let old_edit = (beta_start, end_start + 2);
        let updated = apply_edit(input, old_edit, "beta\n\n# ");
        let new_edit = (beta_start, beta_start + 8);

        let inc = parse_incremental_suffix(&updated, None, &old_tree, &[], old_edit, new_edit);
        let full = parse(&updated, None);
        assert_eq!(inc.tree.to_string(), full.to_string());
        assert_eq!(
            inc.reparse_range.1,
            updated.len(),
            "cross-heading edits should avoid section-window reparsing"
        );
    }

    #[test]
    fn incremental_ignores_nested_headings_for_window_boundaries() {
        let input = "# Intro\n\n> ## Nested\n> quote body\n\n# End\n\nomega\n";
        let old_tree = parse(input, None);
        let quote_start = input.find("quote body").expect("quote body in test input");
        let old_edit = (quote_start, quote_start + 5);
        let updated = apply_edit(input, old_edit, "QUOTE");
        let new_edit = (quote_start, quote_start + 5);

        let inc = parse_incremental_suffix(&updated, None, &old_tree, &[], old_edit, new_edit);
        let full = parse(&updated, None);
        assert_eq!(inc.tree.to_string(), full.to_string());
        assert!(
            inc.reparse_range.1 < updated.len(),
            "window boundary should be the next top-level heading, not nested heading"
        );
    }

    #[test]
    fn incremental_section_window_handles_list_tight_loose_transition() {
        let input = "# Intro\n\nprelude\n\n# Middle\n\n- one\n- two\n\n# End\n\nomega\n";
        let old_tree = parse(input, None);
        let two_start = input.find("- two").expect("list item in test input");
        let old_edit = (two_start, two_start);
        let updated = apply_edit(input, old_edit, "\n");
        let new_edit = (two_start, two_start + 1);

        let inc = parse_incremental_suffix(&updated, None, &old_tree, &[], old_edit, new_edit);
        let full = parse(&updated, None);
        assert_eq!(inc.tree.to_string(), full.to_string());
        assert!(
            inc.reparse_range.0 > 0 && inc.reparse_range.1 < updated.len(),
            "list transition inside section should remain section-bounded"
        );
    }

    #[test]
    fn incremental_section_window_handles_blockquote_lazy_transition() {
        let input = "# Intro\n\nprelude\n\n# Middle\n\n> quoted\nlazy line\n\n# End\n\nomega\n";
        let old_tree = parse(input, None);
        let lazy_start = input.find("lazy line").expect("lazy line in test input");
        let old_edit = (lazy_start, lazy_start);
        let updated = apply_edit(input, old_edit, "> ");
        let new_edit = (lazy_start, lazy_start + 2);

        let inc = parse_incremental_suffix(&updated, None, &old_tree, &[], old_edit, new_edit);
        let full = parse(&updated, None);
        assert_eq!(inc.tree.to_string(), full.to_string());
        assert!(
            inc.reparse_range.0 > 0 && inc.reparse_range.1 < updated.len(),
            "blockquote continuation change inside section should remain section-bounded"
        );
    }

    #[test]
    fn incremental_section_window_handles_fenced_div_with_nested_heading() {
        let input = "# Intro\n\nprelude\n\n# Middle\n\n::: {.callout-note}\n## Nested\nbody text\n:::\n\n# End\n\nomega\n";
        let old_tree = parse(input, None);
        let body_start = input.find("body text").expect("body text in test input");
        let old_edit = (body_start, body_start + 4);
        let updated = apply_edit(input, old_edit, "BODY");
        let new_edit = (body_start, body_start + 4);

        let inc = parse_incremental_suffix(&updated, None, &old_tree, &[], old_edit, new_edit);
        let full = parse(&updated, None);
        assert_eq!(inc.tree.to_string(), full.to_string());
        assert!(
            inc.reparse_range.0 > 0 && inc.reparse_range.1 < updated.len(),
            "fenced div edits should use top-level heading boundaries"
        );
    }

    #[test]
    fn incremental_handles_inserting_heading_inside_section_window() {
        let input = "# Intro\n\nalpha\n\n# Middle\n\nbeta\n\n# End\n\nomega\n";
        let old_tree = parse(input, None);
        let beta_start = input.find("beta").expect("beta in test input");
        let old_edit = (beta_start, beta_start);
        let updated = apply_edit(input, old_edit, "## Inserted\n\n");
        let new_edit = (beta_start, beta_start + "## Inserted\n\n".len());

        let inc = parse_incremental_suffix(&updated, None, &old_tree, &[], old_edit, new_edit);
        let full = parse(&updated, None);
        assert_eq!(inc.tree.to_string(), full.to_string());
        assert_eq!(
            inc.strategy, "section_window",
            "heading insertions within a bounded section should remain section-window mode"
        );
    }

    #[test]
    fn incremental_falls_back_when_deleting_next_heading_boundary() {
        let input = "# Intro\n\nalpha\n\n# Middle\n\nbeta\n\n# End\n\nomega\n";
        let old_tree = parse(input, None);
        let end_start = input.find("# End\n").expect("end heading in test input");
        let old_edit = (end_start, end_start + "# End\n\n".len());
        let updated = apply_edit(input, old_edit, "");
        let new_edit = (end_start, end_start);

        let inc = parse_incremental_suffix(&updated, None, &old_tree, &[], old_edit, new_edit);
        let full = parse(&updated, None);
        assert_eq!(inc.tree.to_string(), full.to_string());
        assert_ne!(
            inc.strategy, "section_window",
            "heading deletions across boundaries should avoid section-window mode"
        );
    }

    #[test]
    fn incremental_falls_back_when_editing_blank_line_after_heading() {
        let input = "# Intro\n\nalpha\n\n# Middle\n\nbeta\n\n# End\n\nomega\n";
        let old_tree = parse(input, None);
        let boundary = input
            .find("# Middle\n\n")
            .expect("middle heading boundary in test input");
        let blank_line_start = boundary + "# Middle\n".len();
        let old_edit = (blank_line_start, blank_line_start + 1);
        let updated = apply_edit(input, old_edit, "");
        let new_edit = (blank_line_start, blank_line_start);

        let inc = parse_incremental_suffix(&updated, None, &old_tree, &[], old_edit, new_edit);
        let full = parse(&updated, None);
        assert_eq!(inc.tree.to_string(), full.to_string());
        assert_ne!(
            inc.strategy, "section_window",
            "heading-adjacent blank line edits should avoid section-window mode"
        );
    }

    #[test]
    fn incremental_handles_frontmatter_to_first_heading_edit() {
        let input = "---\ntitle: Demo\n---\n\n# Intro\n\nalpha\n\n# Next\n\nomega\n";
        let old_tree = parse(input, None);
        let title_start = input.find("Demo").expect("frontmatter value in test input");
        let old_edit = (title_start, title_start + 4);
        let updated = apply_edit(input, old_edit, "Updated Demo");
        let new_edit = (title_start, title_start + "Updated Demo".len());

        let inc = parse_incremental_suffix(&updated, None, &old_tree, &[], old_edit, new_edit);
        let full = parse(&updated, None);
        assert_eq!(inc.tree.to_string(), full.to_string());
        assert_ne!(
            inc.strategy, "section_window",
            "frontmatter edits before first heading should use conservative mode"
        );
    }

    #[test]
    fn incremental_handles_frontmatter_delimiter_edit() {
        let input = "---\ntitle: Demo\n---\n\n# Intro\n\nalpha\n";
        let old_tree = parse(input, None);
        let first_delim_start = 0;
        let old_edit = (first_delim_start, first_delim_start + 3);
        let updated = apply_edit(input, old_edit, "----");
        let new_edit = (first_delim_start, first_delim_start + 4);

        let inc = parse_incremental_suffix(&updated, None, &old_tree, &[], old_edit, new_edit);
        let full = parse(&updated, None);
        assert_eq!(inc.tree.to_string(), full.to_string());
        assert_ne!(
            inc.strategy, "section_window",
            "frontmatter delimiter edits should stay in conservative mode"
        );
    }
}

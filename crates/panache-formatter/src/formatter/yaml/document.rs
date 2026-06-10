//! Top-level YAML document orchestration.
//!
//! Walks `YAML_STREAM` → `YAML_DOCUMENT` → body containers and
//! dispatches to per-container renderers
//! ([`block_map`](super::block_map),
//! [`block_sequence`](super::block_sequence),
//! [`flow`](super::flow), [`scalar`](super::scalar)).
//!
//! Phase 1.15b status: eight rules across the render pipeline. The CST
//! walk that builds `raw` is recursive (descends into nodes, emits
//! tokens): it applies rule 8 (collapse whitespace before an inline
//! `YAML_COMMENT` to one space — needs CST kind to distinguish `#` in
//! quoted scalars from comment indicators), rule 3 (convert
//! single-quoted scalar tokens to double-quoted when the de-escaped
//! content has no `\`, `'`, `"`, or control char — keep single
//! otherwise), and rule 5 (canonical flow spacing — takes over
//! emission for single-line, comment-free `YAML_FLOW_SEQUENCE` /
//! `YAML_FLOW_MAP` subtrees, producing `[a, b, c]` and
//! `{ k: v, ... }`). After that, rule 1 (canonical 2-space indent)
//! runs against per-CST-line depths precomputed from `root.text()`,
//! returning `None` for lines inside multi-line flow continuations so
//! rule 6's wrap indent survives across passes; then rule 6 (overflow
//! wrap: re-parse the post-indent buffer, walk top-level flow
//! containers in reverse byte order, replace overflowing single-line
//! forms with canonical multi-line — items at
//! `parent_content_column + 2`, closing bracket at
//! `parent_content_column`); then the Phase 1.15b plain-scalar wrap
//! pass (analog of rule 6 for block-map values: greedy word-wrap of
//! single-line plain scalars whose enclosing line exceeds
//! `line_width`, with continuation lines at `depth * 2` so the wrap
//! output round-trips through rule 1's multi-line continuation rule);
//! then rule 10 (strip trailing whitespace per line), rule 7
//! (collapse blank-line runs), and rule 13 (exactly one `\n` at EOF)
//! run as line-level post-passes. Multi-line flow
//! input now round-trips (parser accepts the closing-`]`/`}` at the
//! parent block-map's indent; rule 6 leaves already-wrapped containers
//! in place when they fit, or rewraps via `replace_range` when the
//! canonical single-line form would overflow). Flow containers with
//! embedded comments stay verbatim; block-scalar (`|`/`>`) interior
//! lines are preserved verbatim — rule 1 needs a real block-scalar
//! renderer to canonicalize their indent.

use panache_parser::SyntaxNode;
use panache_parser::syntax::{SyntaxKind, SyntaxToken};
use rowan::{TextSize, TokenAtOffset};

use super::options::{WrapMode, YamlFormatOptions};
use crate::formatter::sentence_wrap::{self, ResolvedProfile};

/// Render the given CST root into a string. The root is expected to be
/// the `DOCUMENT` node returned by
/// [`panache_parser::parser::yaml::parse_yaml_tree`], but any CST node
/// works for the walk — we descend into it recursively.
pub(super) fn render(root: &SyntaxNode, opts: &YamlFormatOptions) -> String {
    let depths = precompute_line_depths(root);
    let raw = walk_with_normalization(root);
    let indented = apply_canonical_indents(&raw, &depths);
    let flow_wrapped = apply_flow_wrap(indented, opts);
    let scalar_wrapped = apply_plain_scalar_wrap(flow_wrapped, opts);
    let stripped = strip_trailing_whitespace_per_line(scalar_wrapped);
    let collapsed = collapse_blank_line_runs(stripped);
    normalize_trailing_newline(collapsed)
}

/// Recursive CST walk producing the raw output. Rules applied during
/// the walk:
///
/// - **Rule 8** (`emit_token`): a `WHITESPACE` token immediately
///   preceding an inline `YAML_COMMENT` is emitted as a single space.
///   Standalone comments (preceded by `NEWLINE` or at file start) keep
///   their surrounding whitespace.
/// - **Rule 5** (`emit_flow_sequence` / `emit_flow_map`): single-line,
///   comment-free flow containers take over emission and produce
///   canonical spacing — `[a, b, c]` (no space inside `[]`, one space
///   after each `,`) and `{ k: v, ... }` (one space inside `{}`, one
///   space after each `,`, one space after each `:`).
///
/// Multi-line flow containers or those with embedded comments are
/// emitted verbatim via the generic recursive path — rule 6 will own
/// multi-line wrap; inline comments inside flow are a rare edge case
/// not worth handling here.
fn walk_with_normalization(root: &SyntaxNode) -> String {
    let mut out = String::with_capacity(root.text_range().len().into());
    emit_node(&mut out, root);
    out
}

fn emit_node(out: &mut String, node: &SyntaxNode) {
    match node.kind() {
        SyntaxKind::YAML_FLOW_SEQUENCE if can_canonicalize_flow(node) => {
            emit_flow_sequence(out, node);
        }
        SyntaxKind::YAML_FLOW_MAP if can_canonicalize_flow(node) => {
            emit_flow_map(out, node);
        }
        _ => {
            for child in node.children_with_tokens() {
                match child {
                    rowan::NodeOrToken::Token(t) => emit_token(out, &t),
                    rowan::NodeOrToken::Node(n) => emit_node(out, &n),
                }
            }
        }
    }
}

fn emit_token(out: &mut String, t: &SyntaxToken) {
    if t.kind() == SyntaxKind::WHITESPACE
        && (is_ws_before_inline_comment(t) || is_ws_after_block_structural(t))
    {
        out.push(' ');
    } else if let Some(converted) = try_convert_single_to_double(t.text()) {
        out.push_str(&converted);
    } else {
        out.push_str(t.text());
    }
}

/// STYLE.md rule 3: prefer double-quoted over single-quoted when the
/// content has nothing that would need backslash-escaping in
/// double-quoted form. Returns `Some(double_quoted)` if `text` is a
/// single-quoted scalar whose de-escaped content has no `\`, `'`, `"`,
/// or ASCII control char (0x00–0x1F or 0x7F); otherwise `None` (caller
/// emits verbatim). Keeping single-quoted when content has `'` is
/// conservative — double would handle bare `'` fine, but pretty_yaml
/// preserves the user's choice in that case and we match.
/// Control-char escaping in double-quoted form (TAB → `\t`,
/// LF → `\n` with continuation indent, etc.) is non-trivial and not
/// yet implemented; we keep single in those cases. Brackets/commas
/// inside flow containers are also `YAML_SCALAR` tokens but their
/// text never starts with `'`, so the prefix check filters them out
/// safely.
fn try_convert_single_to_double(text: &str) -> Option<String> {
    if text.len() < 2 || !text.starts_with('\'') || !text.ends_with('\'') {
        return None;
    }
    let inner = &text[1..text.len() - 1];
    let content = inner.replace("''", "'");
    if content.chars().any(|c| {
        let cp = c as u32;
        c == '\\' || c == '\'' || c == '"' || cp < 0x20 || cp == 0x7F
    }) {
        return None;
    }
    Some(format!("\"{content}\""))
}

/// STYLE.md rule 14: a `WHITESPACE` token sitting immediately between
/// a block structural indicator (`YAML_COLON` after a block-map key,
/// `YAML_BLOCK_SEQ_ENTRY` after a block-sequence `-`) and its inline
/// content collapses to a single space. Same-line content only — a
/// trailing-WS-then-NEWLINE shape (`key:    \n  value`) is left to
/// rule 10 to strip; the value's own indent line is governed by
/// rule 1. Flow containers handle their `:` / `,` spacing through
/// the canonical-emission path (`emit_flow_map_entry`), so this
/// rule only matters for block-level structural runs.
fn is_ws_after_block_structural(t: &SyntaxToken) -> bool {
    let Some(prev) = t.prev_token() else {
        return false;
    };
    if !matches!(
        prev.kind(),
        SyntaxKind::YAML_COLON | SyntaxKind::YAML_BLOCK_SEQ_ENTRY
    ) {
        return false;
    }
    match t.next_token() {
        Some(next) => next.kind() != SyntaxKind::NEWLINE,
        None => false,
    }
}

/// True if `t` is a `WHITESPACE` token whose forward run of contiguous
/// whitespace lands on a `YAML_COMMENT`, AND that comment is inline
/// (the previous non-whitespace token is not `NEWLINE`).
fn is_ws_before_inline_comment(t: &SyntaxToken) -> bool {
    let mut cursor = t.next_token();
    while let Some(tok) = cursor.as_ref() {
        if tok.kind() != SyntaxKind::WHITESPACE {
            break;
        }
        cursor = tok.next_token();
    }
    let Some(next) = cursor else {
        return false;
    };
    if next.kind() != SyntaxKind::YAML_COMMENT {
        return false;
    }
    let mut back = t.prev_token();
    while let Some(tok) = back.as_ref() {
        match tok.kind() {
            SyntaxKind::NEWLINE => return false,
            SyntaxKind::WHITESPACE => back = tok.prev_token(),
            _ => return true,
        }
    }
    false
}

/// True if a flow container can be emitted in canonical single-line
/// form. Multi-line containers stay verbatim (rule 6 will own wrap);
/// containers with embedded comments stay verbatim (preserving
/// `YAML_COMMENT` placement inside `{}`/`[]` is too rare to be worth
/// the complexity here).
fn can_canonicalize_flow(node: &SyntaxNode) -> bool {
    if node.text().to_string().contains('\n') {
        return false;
    }
    !node
        .descendants_with_tokens()
        .any(|c| matches!(c, rowan::NodeOrToken::Token(t) if t.kind() == SyntaxKind::YAML_COMMENT))
}

/// Canonical flow sequence: `[item1, item2, ...]`. No space inside the
/// brackets; one space after each comma. Items are recursively emitted
/// (so nested flows get their canonical form) then trimmed of stray
/// whitespace the parser may have absorbed.
fn emit_flow_sequence(out: &mut String, node: &SyntaxNode) {
    out.push('[');
    let items: Vec<_> = node
        .children()
        .filter(|c| c.kind() == SyntaxKind::YAML_FLOW_SEQUENCE_ITEM)
        .collect();
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        let mut inner = String::new();
        emit_node(&mut inner, item);
        out.push_str(inner.trim());
    }
    out.push(']');
}

/// Canonical flow map: `{ k: v, ... }`. One space inside the braces
/// (or `{}` if empty); one space after each comma; one space after
/// each `:`. If the parser couldn't structure the content (e.g.
/// `{key:value}` where no space disambiguates `:`), the inner text is
/// emitted verbatim between `{ ` and ` }` — matches pretty_yaml's
/// "normalize spacing around structure, don't re-parse content"
/// behavior.
fn emit_flow_map(out: &mut String, node: &SyntaxNode) {
    let entries: Vec<_> = node
        .children()
        .filter(|c| c.kind() == SyntaxKind::YAML_FLOW_MAP_ENTRY)
        .collect();
    if entries.is_empty() {
        let inner = inner_flow_text(node);
        if inner.is_empty() {
            out.push_str("{}");
        } else {
            out.push_str("{ ");
            out.push_str(&inner);
            out.push_str(" }");
        }
        return;
    }
    out.push_str("{ ");
    for (i, entry) in entries.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        emit_flow_map_entry(out, entry);
    }
    out.push_str(" }");
}

fn emit_flow_map_entry(out: &mut String, entry: &SyntaxNode) {
    let mut emitted_key = false;
    for child in entry.children() {
        match child.kind() {
            SyntaxKind::YAML_FLOW_MAP_KEY => {
                let mut buf = String::new();
                emit_node(&mut buf, &child);
                out.push_str(buf.trim());
                emitted_key = true;
            }
            SyntaxKind::YAML_FLOW_MAP_VALUE => {
                if emitted_key {
                    out.push(' ');
                }
                let mut buf = String::new();
                emit_node(&mut buf, &child);
                out.push_str(buf.trim());
            }
            _ => {}
        }
    }
}

/// Extract the content between the opening and closing brackets of a
/// flow container as a trimmed string. Used when a flow map's parser
/// couldn't structure its content (no `YAML_FLOW_MAP_ENTRY` children) —
/// we preserve the raw inner bytes rather than re-parsing.
fn inner_flow_text(node: &SyntaxNode) -> String {
    let text = node.text().to_string();
    let trimmed = text.trim();
    let inner = trimmed
        .strip_prefix(['{', '['])
        .and_then(|s| s.strip_suffix(['}', ']']))
        .unwrap_or(trimmed);
    inner.trim().to_string()
}

/// Precompute the canonical indent depth for each line in the CST's
/// own text (one entry per `\n`-terminated line, plus a trailing entry
/// for the final unterminated line). `None` means the line passes
/// through verbatim (whitespace-only, or block-scalar interior).
///
/// This is decoupled from the buffer so that rules 5 and 8 (which can
/// shift per-line byte counts) don't invalidate rule 1's CST-offset
/// lookup. The buffer-side pass (`apply_canonical_indents`) iterates
/// lines in parallel — none of the in-walk rules add or remove `\n`
/// for the lines they touch, so buffer line count matches CST line
/// count.
fn precompute_line_depths(root: &SyntaxNode) -> Vec<Option<usize>> {
    let text = root.text().to_string();
    let mut out = Vec::new();
    let mut line_start = 0usize;
    for line in text.split_inclusive('\n') {
        let trimmed_start = line
            .find(|c: char| c != ' ' && c != '\t')
            .unwrap_or(line.len());
        let depth = if trimmed_start == line.len() {
            None
        } else {
            canonical_indent_depth(root, line_start + trimmed_start)
        };
        out.push(depth);
        line_start += line.len();
    }
    out
}

/// STYLE.md rule 1: every content line is indented by `2 * depth`
/// spaces, where `depth` counts the line's containing
/// `YAML_BLOCK_MAP_ENTRY` + `YAML_BLOCK_SEQUENCE_ITEM` ancestors
/// (root-level entries/items are depth 1 → 0-space indent). Depths are
/// precomputed from CST text by [`precompute_line_depths`]; this pass
/// iterates buffer lines in lockstep and rewrites only the leading-WS
/// slice of each line. Block-scalar interior lines and whitespace-only
/// lines (`depth == None`) pass through verbatim.
fn apply_canonical_indents(raw: &str, depths: &[Option<usize>]) -> String {
    let mut out = String::with_capacity(raw.len());
    for (i, line) in raw.split_inclusive('\n').enumerate() {
        let trimmed_start = line
            .find(|c: char| c != ' ' && c != '\t')
            .unwrap_or(line.len());
        if trimmed_start == line.len() {
            out.push_str(line);
            continue;
        }
        match depths.get(i).copied().flatten() {
            Some(depth) => {
                for _ in 0..depth {
                    out.push_str("  ");
                }
                out.push_str(&line[trimmed_start..]);
            }
            None => out.push_str(line),
        }
    }
    out
}

/// Compute the canonical indent depth (number of 2-space steps) for
/// the line whose first non-whitespace byte sits at `offset`.
/// Returns `None` if the line is the interior of a block scalar — in
/// that case the caller preserves the original indent.
fn canonical_indent_depth(root: &SyntaxNode, offset: usize) -> Option<usize> {
    let offset_ts = TextSize::try_from(offset).ok()?;
    let token = match root.token_at_offset(offset_ts) {
        TokenAtOffset::Single(t) => t,
        TokenAtOffset::Between(_, right) => right,
        TokenAtOffset::None => return Some(0),
    };

    if token.kind() == SyntaxKind::YAML_SCALAR_TEXT
        && let Some(scalar) = token
            .parent()
            .filter(|p| p.kind() == SyntaxKind::YAML_SCALAR)
    {
        // Multi-line scalar continuation. The scanner splits a multi-line
        // scalar into per-line `YAML_SCALAR_TEXT` fragments interleaved
        // with `NEWLINE`, so the multi-line check, the scalar's start
        // offset, and the `|`/`>` block-indicator probe all read the
        // parent `YAML_SCALAR` node (not the individual fragment leaf).
        // Block scalars (`|`/`>`) bake their interior indent into the
        // source — proper canonicalization needs a real block-scalar
        // renderer, so preserve verbatim. Plain / single- / double-quoted
        // multi-line scalars have their continuation lines canonicalized
        // to the parent value's content column (depth * 2 spaces — one
        // level deeper than rule 1's default formula, matching
        // pretty_yaml's output for multi-line values). The first line of
        // the scalar doesn't hit this carve-out: when the scalar is a
        // value, the line's first non-WS byte is the key
        // (offset < scalar_start); when the scalar opens the line,
        // offset == scalar_start.
        let scalar_text = scalar.text().to_string();
        if scalar_text.contains('\n') {
            let scalar_start = usize::from(scalar.text_range().start());
            if scalar_text.starts_with('|') || scalar_text.starts_with('>') {
                // Block scalars (`|`/`>`) bake their interior indent into
                // the source — preserve continuation lines verbatim until a
                // real block-scalar renderer exists. The indicator line
                // (offset == scalar_start) falls through to rule 1's formula.
                if offset > scalar_start {
                    return None;
                }
            } else if offset >= scalar_start {
                // Plain / single- / double-quoted multi-line scalar. Both the
                // first line — when the value opens its own line below the key
                // (offset == scalar_start) — and the continuation lines
                // (offset > scalar_start) canonicalize to the value's content
                // column (depth * 2 spaces). Anchoring the first line here too
                // keeps a value-on-its-own-line scalar aligned with its
                // continuation; treating it as a default-depth line instead
                // de-indents only the first line and splits the scalar.
                let mut entry_item_ancestors = 0usize;
                let mut node = scalar.parent();
                while let Some(n) = node {
                    if matches!(
                        n.kind(),
                        SyntaxKind::YAML_BLOCK_MAP_ENTRY | SyntaxKind::YAML_BLOCK_SEQUENCE_ITEM
                    ) {
                        entry_item_ancestors += 1;
                    }
                    node = n.parent();
                }
                return Some(entry_item_ancestors);
            }
        }
    }

    // Multi-line flow continuation: rule 6 owns the indent for wrapped
    // flow content. If `offset` lands on a continuation line of an
    // enclosing `YAML_FLOW_SEQUENCE` / `YAML_FLOW_MAP` (its text spans
    // a newline between the flow's start and this offset), preserve the
    // existing indent — rule 1's block-context depth formula doesn't
    // apply inside a wrapped flow.
    let mut probe = token.parent();
    while let Some(n) = probe {
        if matches!(
            n.kind(),
            SyntaxKind::YAML_FLOW_SEQUENCE | SyntaxKind::YAML_FLOW_MAP
        ) {
            let flow_start = usize::from(n.text_range().start());
            if flow_start < offset {
                let span = n.text().to_string();
                let before_offset_in_flow = &span[..offset - flow_start];
                if before_offset_in_flow.contains('\n') {
                    return None;
                }
            }
        }
        probe = n.parent();
    }

    let mut entry_item_ancestors = 0usize;
    let mut node = token.parent();
    while let Some(n) = node {
        if matches!(
            n.kind(),
            SyntaxKind::YAML_BLOCK_MAP_ENTRY | SyntaxKind::YAML_BLOCK_SEQUENCE_ITEM
        ) {
            entry_item_ancestors += 1;
        }
        node = n.parent();
    }
    Some(entry_item_ancestors.saturating_sub(1))
}

/// STYLE.md rule 10: strip trailing ASCII space + tab from every
/// line. Applied uniformly, including inside `|`/`>` block scalars
/// (pretty_yaml does the same; this trades semantic strictness in
/// literal blocks for the "no trailing whitespace anywhere" invariant
/// that the spec pins). `\r` is preserved so CRLF line endings round
/// trip.
fn strip_trailing_whitespace_per_line(buf: String) -> String {
    if !buf.contains([' ', '\t']) {
        return buf;
    }
    let mut out = String::with_capacity(buf.len());
    for (i, line) in buf.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(line.trim_end_matches([' ', '\t']));
    }
    out
}

/// STYLE.md rule 7: runs of multiple interior blank lines collapse to
/// one max; leading blank lines are stripped entirely (mirroring
/// rule 13's "no trailing blank lines" — preamble whitespace before the
/// first content line is never meaningful). Applied after rule 10 so
/// that whitespace-only "blank" lines (which rule 10 reduces to empty)
/// participate uniformly. A blank line is an empty `""` slot in the
/// `\n`-split (or `"\r"` for a CRLF-only line).
fn collapse_blank_line_runs(buf: String) -> String {
    let lines: Vec<&str> = buf.split('\n').collect();
    let mut kept: Vec<&str> = Vec::with_capacity(lines.len());
    let mut prev_blank = false;
    let mut seen_content = false;
    for line in lines {
        let is_blank = line.is_empty() || line == "\r";
        if is_blank && !seen_content {
            continue;
        }
        if is_blank && prev_blank {
            continue;
        }
        kept.push(line);
        prev_blank = is_blank;
        if !is_blank {
            seen_content = true;
        }
    }
    kept.join("\n")
}

/// STYLE.md rule 13: a successfully-formatted document ends with
/// exactly one `\n`. Missing → add one; many → collapse to one.
fn normalize_trailing_newline(mut buf: String) -> String {
    let trimmed_len = buf.trim_end_matches('\n').len();
    buf.truncate(trimmed_len);
    buf.push('\n');
    buf
}

/// STYLE.md rule 6: when a flow container's canonical single-line form
/// would push its enclosing line past `opts.line_width`, rewrite the
/// container to canonical multi-line. Each item lands on its own line
/// indented at the parent entry/item's content column + 2; a trailing
/// comma follows the final item; the closing `]`/`}` sits on its own
/// line at the parent's content column. The opening bracket stays on
/// the key line — that's the one point of disagreement with Prettier
/// (we follow pretty_yaml).
///
/// Implementation: re-parse the post-indent buffer to find flow
/// containers. Single-line containers reach here in their canonical
/// rule-5 form; already-multi-line containers (either pass-2 input or
/// pre-wrapped user input) have their wrap indent preserved by rule 1
/// (`canonical_indent_depth` returns `None` inside multi-line flow).
/// Walk top-level (no flow ancestor) containers in reverse byte order
/// and `replace_range` overflowing ones with the canonical wrap shape.
/// Already-wrapped containers whose canonical single-line form fits
/// stay multi-line (matches pretty_yaml's "sticky multi-line" behavior);
/// already-wrapped containers that still overflow are rewritten via
/// `replace_range`, which canonicalizes any non-spec indent on the way
/// through.
fn apply_flow_wrap(buf: String, opts: &YamlFormatOptions) -> String {
    let Some(tree) = panache_parser::parser::yaml::parse_yaml_tree(&buf) else {
        return buf;
    };

    let mut targets: Vec<SyntaxNode> = tree
        .descendants()
        .filter(|n| {
            matches!(
                n.kind(),
                SyntaxKind::YAML_FLOW_SEQUENCE | SyntaxKind::YAML_FLOW_MAP
            )
        })
        .filter(|n| !has_flow_ancestor(n))
        .collect();
    targets.sort_by_key(|n| n.text_range().start());

    let mut out = buf;
    for container in targets.into_iter().rev() {
        let start = usize::from(container.text_range().start());
        let end = usize::from(container.text_range().end());
        let col = column_of_offset(&out, start);
        let single = canonical_single_line_flow(&container);
        let tail_chars = out[end..].split('\n').next().unwrap_or("").chars().count();
        let line_len = col + single.chars().count() + tail_chars;
        if line_len <= opts.line_width {
            continue;
        }
        let wrapped = canonical_wrapped_flow(&container);
        out.replace_range(start..end, &wrapped);
    }
    out
}

fn has_flow_ancestor(node: &SyntaxNode) -> bool {
    let mut p = node.parent();
    while let Some(n) = p {
        if matches!(
            n.kind(),
            SyntaxKind::YAML_FLOW_SEQUENCE | SyntaxKind::YAML_FLOW_MAP
        ) {
            return true;
        }
        p = n.parent();
    }
    false
}

fn column_of_offset(text: &str, offset: usize) -> usize {
    let prefix = &text[..offset];
    let last_nl = prefix.rfind('\n').map(|p| p + 1).unwrap_or(0);
    prefix[last_nl..].chars().count()
}

fn canonical_single_line_flow(node: &SyntaxNode) -> String {
    let mut out = String::new();
    match node.kind() {
        SyntaxKind::YAML_FLOW_SEQUENCE => emit_flow_sequence(&mut out, node),
        SyntaxKind::YAML_FLOW_MAP => emit_flow_map(&mut out, node),
        _ => out.push_str(&node.text().to_string()),
    }
    out
}

/// Canonical multi-line form for a flow container that overflows its
/// line. Items go one-per-line at the parent's content column + 2, with
/// a trailing comma; the closing bracket sits at the parent's content
/// column. The parent's content column is computed from the CST: it's
/// `2 * (block_entry_item_depth − 1)` for a flow in a block-map value,
/// and `2 * (block_entry_item_depth − 1) + 2` for a flow inside a block
/// sequence item (the `- ` prefix shifts the content column right by
/// two). Nested flow containers inside the wrapped items stay in
/// canonical single-line form — rule 6 only wraps the outermost
/// overflowing container in a single pass.
fn canonical_wrapped_flow(node: &SyntaxNode) -> String {
    let (open, close) = match node.kind() {
        SyntaxKind::YAML_FLOW_SEQUENCE => ('[', ']'),
        SyntaxKind::YAML_FLOW_MAP => ('{', '}'),
        _ => return node.text().to_string(),
    };

    let content_col = parent_content_col(node);
    let item_indent = " ".repeat(content_col + 2);
    let close_indent = " ".repeat(content_col);

    let mut out = String::new();
    out.push(open);

    let item_kind = match node.kind() {
        SyntaxKind::YAML_FLOW_SEQUENCE => SyntaxKind::YAML_FLOW_SEQUENCE_ITEM,
        SyntaxKind::YAML_FLOW_MAP => SyntaxKind::YAML_FLOW_MAP_ENTRY,
        _ => unreachable!(),
    };
    let items: Vec<_> = node.children().filter(|c| c.kind() == item_kind).collect();
    if items.is_empty() {
        out.push(close);
        return out;
    }

    out.push('\n');
    for item in &items {
        out.push_str(&item_indent);
        let rendered = match item.kind() {
            SyntaxKind::YAML_FLOW_SEQUENCE_ITEM => {
                let mut buf = String::new();
                emit_node(&mut buf, item);
                buf.trim().to_string()
            }
            SyntaxKind::YAML_FLOW_MAP_ENTRY => {
                let mut buf = String::new();
                emit_flow_map_entry(&mut buf, item);
                buf
            }
            _ => unreachable!(),
        };
        out.push_str(&rendered);
        out.push(',');
        out.push('\n');
    }
    out.push_str(&close_indent);
    out.push(close);
    out
}

/// Compute the "content column" of the entry/item that immediately
/// contains this flow node — where its `]`/`}` should sit on wrap. For
/// a flow in a block-map value, that's the line indent of the key
/// (canonical `2 * (depth − 1)`). For a flow in a block-sequence item,
/// the `- ` prefix shifts content right by two, so add another 2.
fn parent_content_col(node: &SyntaxNode) -> usize {
    let mut depth = 0usize;
    let mut in_block_seq_item = false;
    let mut found_first_block_anchor = false;
    let mut parent = node.parent();
    while let Some(p) = parent {
        match p.kind() {
            SyntaxKind::YAML_BLOCK_MAP_ENTRY => {
                depth += 1;
                found_first_block_anchor = true;
            }
            SyntaxKind::YAML_BLOCK_SEQUENCE_ITEM => {
                depth += 1;
                if !found_first_block_anchor {
                    in_block_seq_item = true;
                    found_first_block_anchor = true;
                }
            }
            _ => {}
        }
        parent = p.parent();
    }
    let canonical = 2 * depth.saturating_sub(1);
    if in_block_seq_item {
        canonical + 2
    } else {
        canonical
    }
}

/// STYLE.md rule 6 (plain-scalar overflow analog) and rule 15 (folded
/// block-scalar wrapping): re-lay-out block-map scalar values per the
/// active wrap mode.
///
/// Folded (`>`/`>-`/`>+`) scalars reflow per [`reflow_folded_scalar`]
/// under every wrapping mode. Single-line plain scalars wrap too:
/// under [`WrapMode::Reflow`] when the line exceeds `opts.line_width`
/// (continuation lines at `depth * 2`, the value column); under
/// [`WrapMode::Sentence`] / [`WrapMode::Semantic`] split into one
/// sentence per line.
///
/// Scope: block-map values only. Quoted (`'…'`, `"…"`) and literal
/// (`|`) scalars never wrap per the "Plain-scalar wrapping" section of
/// `STYLE.md`. Multi-line plain scalars are skipped (rule 1's
/// continuation pass handles them). Scalars in block sequences are
/// skipped: the wrap-continuation column there (parent content + 2)
/// disagrees with rule 1's multi-line-continuation column (`depth * 2`),
/// so that shape isn't idempotent and we defer it.
///
/// Inline comments and tag/anchor decorations on the value side cause
/// the scalar to skip wrap — keeping the algorithm simple.
///
/// Under [`WrapMode::Preserve`] this pass is a no-op (early return):
/// every scalar keeps its original line breaks. (Flow-container
/// wrapping in [`apply_flow_wrap`] is *not* gated on the prose-wrap
/// mode — overflowing flow collections wrap under every mode, since
/// that is a print-width concern rather than prose wrapping.)
///
/// Implementation: re-parse the post-flow-wrap buffer, walk
/// `YAML_BLOCK_MAP_VALUE` nodes, and rewrite each affected scalar with
/// `replace_range` (reverse byte order, so earlier offsets remain
/// valid).
fn apply_plain_scalar_wrap(buf: String, opts: &YamlFormatOptions) -> String {
    if opts.wrap == WrapMode::Preserve {
        return buf;
    }
    let Some(tree) = panache_parser::parser::yaml::parse_yaml_tree(&buf) else {
        return buf;
    };
    let profile = sentence_wrap::profile_from(opts.lang.as_deref(), &opts.no_break_abbreviations);

    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    for value_node in tree
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::YAML_BLOCK_MAP_VALUE)
    {
        if has_block_seq_ancestor(&value_node) {
            continue;
        }
        if value_has_inline_comment(&value_node) {
            continue;
        }
        if value_has_decoration(&value_node) {
            continue;
        }
        let Some(scalar) = value_node
            .children()
            .find(|n| n.kind() == SyntaxKind::YAML_SCALAR)
        else {
            continue;
        };
        let text = scalar.text().to_string();
        let scalar_start = usize::from(scalar.text_range().start());
        let scalar_end = usize::from(scalar.text_range().end());
        // Folded block scalars (`>`, `>-`, `>+`) reflow per wrap mode —
        // their single line breaks fold to spaces, so joining and
        // re-breaking is loss-free (STYLE.md rule 15). Literal (`|`)
        // scalars never wrap (newlines are significant); quoted scalars
        // never wrap either.
        if text.starts_with('>') {
            if let Some(wrapped) = reflow_folded_scalar(&text, opts, profile)
                && wrapped != text
            {
                edits.push((scalar_start, scalar_end, wrapped));
            }
            continue;
        }
        if text.starts_with('\'') || text.starts_with('"') || text.starts_with('|') {
            continue;
        }
        // Multi-line plain scalars are left to the continuation-indent
        // rule; only single-line plain values wrap here.
        if text.contains('\n') {
            continue;
        }
        let depth = block_entry_depth(&value_node);
        if depth == 0 {
            continue;
        }
        let line_start = buf[..scalar_start].rfind('\n').map(|p| p + 1).unwrap_or(0);
        let scalar_col = buf[line_start..scalar_start].chars().count();
        let indent = depth * 2;
        let wrapped = match opts.wrap {
            WrapMode::Reflow => {
                let line_end = buf[scalar_end..]
                    .find('\n')
                    .map(|p| scalar_end + p)
                    .unwrap_or(buf.len());
                if buf[line_start..line_end].chars().count() <= opts.line_width {
                    continue;
                }
                wrap_plain_scalar_text(&text, scalar_col, indent, opts.line_width)
            }
            // For a single-line plain value, semantic == sentence (there
            // are no author line breaks to preserve): both split into one
            // sentence per line at the value column.
            WrapMode::Sentence | WrapMode::Semantic => {
                let sentences = sentence_wrap::split_sentence_text(&text, profile);
                if sentences.len() <= 1 {
                    continue;
                }
                join_sentences_as_plain(&sentences, indent)
            }
            WrapMode::Preserve => continue,
        };
        if wrapped == text {
            continue;
        }
        edits.push((scalar_start, scalar_end, wrapped));
    }
    edits.sort_by_key(|(s, _, _)| *s);
    let mut out = buf;
    for (start, end, replacement) in edits.into_iter().rev() {
        out.replace_range(start..end, &replacement);
    }
    out
}

/// Lay out a sequence of sentences as a multi-line plain scalar value:
/// the first sentence stays on the key line; each subsequent sentence
/// starts a continuation line at the value column (`indent` spaces).
/// Plain multi-line scalars fold their line breaks to spaces, so this
/// round-trips back to the original prose.
fn join_sentences_as_plain(sentences: &[String], indent: usize) -> String {
    let indent_str = " ".repeat(indent);
    let mut out = String::new();
    for (i, sentence) in sentences.iter().enumerate() {
        if i > 0 {
            out.push('\n');
            out.push_str(&indent_str);
        }
        out.push_str(sentence);
    }
    out
}

fn has_block_seq_ancestor(value_node: &SyntaxNode) -> bool {
    let mut p = value_node.parent();
    while let Some(n) = p {
        if n.kind() == SyntaxKind::YAML_BLOCK_SEQUENCE_ITEM {
            return true;
        }
        p = n.parent();
    }
    false
}

fn value_has_inline_comment(value_node: &SyntaxNode) -> bool {
    value_node
        .children_with_tokens()
        .any(|c| matches!(c, rowan::NodeOrToken::Token(t) if t.kind() == SyntaxKind::YAML_COMMENT))
}

fn value_has_decoration(value_node: &SyntaxNode) -> bool {
    value_node.children_with_tokens().any(|c| {
        matches!(
            c,
            rowan::NodeOrToken::Token(t)
                if matches!(t.kind(), SyntaxKind::YAML_TAG | SyntaxKind::YAML_ANCHOR | SyntaxKind::YAML_ALIAS)
        )
    })
}

fn block_entry_depth(value_node: &SyntaxNode) -> usize {
    let mut count = 0usize;
    let mut p = value_node.parent();
    while let Some(n) = p {
        if matches!(
            n.kind(),
            SyntaxKind::YAML_BLOCK_MAP_ENTRY | SyntaxKind::YAML_BLOCK_SEQUENCE_ITEM
        ) {
            count += 1;
        }
        p = n.parent();
    }
    count
}

/// Greedy word-wrap of a plain scalar's text. `start_col` is the column
/// where the scalar's first character sits on its starting line;
/// `indent` is the canonical continuation column. Multi-space runs that
/// are not break points are preserved verbatim (so `x  milk` mid-value
/// keeps its double space). A multi-space run that IS the break point is
/// consumed entirely by the `\n` + continuation indent — pretty_yaml
/// keeps the leading character of the run as a trailing space, but
/// rule 10 would strip it anyway, and consuming the run here keeps
/// pass-2 output byte-stable.
fn wrap_plain_scalar_text(text: &str, start_col: usize, indent: usize, width: usize) -> String {
    let mut out = String::new();
    let mut col = start_col;
    let indent_str = " ".repeat(indent);
    let mut rest = text;
    let mut first_word = true;
    while !rest.is_empty() {
        let ws_end = rest
            .find(|c: char| !c.is_whitespace())
            .unwrap_or(rest.len());
        let ws = &rest[..ws_end];
        rest = &rest[ws_end..];
        let word_end = rest.find(|c: char| c.is_whitespace()).unwrap_or(rest.len());
        let word = &rest[..word_end];
        rest = &rest[word_end..];
        if word.is_empty() {
            break;
        }
        let ws_len = ws.chars().count();
        let word_len = word.chars().count();
        if first_word {
            out.push_str(ws);
            out.push_str(word);
            col += ws_len + word_len;
            first_word = false;
        } else if col + ws_len + word_len > width {
            out.push('\n');
            out.push_str(&indent_str);
            out.push_str(word);
            col = indent + word_len;
        } else {
            out.push_str(ws);
            out.push_str(word);
            col += ws_len + word_len;
        }
    }
    out
}

/// Number of leading ASCII spaces on `line`.
fn leading_spaces(line: &str) -> usize {
    line.len() - line.trim_start_matches(' ').len()
}

/// Reflow a **folded** (`>`) block scalar's body per the active wrap
/// mode, returning the rewritten scalar text (header + body) or `None`
/// when the scalar isn't a simple folded scalar we can safely reflow.
///
/// Folding semantics make every mode loss-free: a single line break
/// between two equally-indented non-empty lines folds to one space, so
/// joining a folded paragraph and re-breaking it anywhere round-trips.
/// The body is grouped into paragraphs of contiguous base-indent prose
/// lines; folding-significant lines act as separators and pass through
/// verbatim — blank lines (which fold to a newline) and more-indented
/// lines (which are literal within a folded scalar). Each paragraph is
/// then re-laid-out:
///
/// - **Reflow**: join the paragraph and greedy-fill to `line_width`.
/// - **Sentence**: join the paragraph, then one sentence per line.
/// - **Semantic**: keep the author's line breaks (each source line is a
///   unit) and additionally split each on sentence boundaries.
///
/// Bails (`None`) on an explicit indentation indicator (`>2`), a header
/// trailing comment, or an empty body — the rare shapes where reflowing
/// would need to reason about the indent the indicator pins.
fn reflow_folded_scalar(
    text: &str,
    opts: &YamlFormatOptions,
    profile: ResolvedProfile<'_>,
) -> Option<String> {
    let nl = text.find('\n')?;
    let header = &text[..nl];
    // Chars after `>`: only an optional chomping indicator is in scope.
    let chomping = header.trim_end().get(1..)?;
    if !(chomping.is_empty() || chomping == "-" || chomping == "+") {
        return None;
    }
    let body = &text[nl + 1..];
    let lines: Vec<&str> = body.split('\n').collect();
    let base_indent = lines
        .iter()
        .find(|l| !l.trim().is_empty())
        .map(|l| leading_spaces(l))?;
    if base_indent == 0 {
        return None;
    }
    let indent_str = " ".repeat(base_indent);

    let mut out_lines: Vec<String> = Vec::with_capacity(lines.len());
    let mut para: Vec<&str> = Vec::new();
    for line in &lines {
        // Blank and more-indented (literal) lines are folding-significant:
        // they break the current paragraph and pass through verbatim.
        if line.trim().is_empty() || leading_spaces(line) != base_indent {
            if !para.is_empty() {
                emit_folded_paragraph(
                    &para,
                    base_indent,
                    &indent_str,
                    opts,
                    profile,
                    &mut out_lines,
                );
                para.clear();
            }
            out_lines.push((*line).to_string());
        } else {
            para.push(&line[base_indent..]);
        }
    }
    if !para.is_empty() {
        emit_folded_paragraph(
            &para,
            base_indent,
            &indent_str,
            opts,
            profile,
            &mut out_lines,
        );
    }

    Some(format!("{header}\n{}", out_lines.join("\n")))
}

/// Re-lay-out one folded-scalar paragraph (`para`, the base-indent prose
/// lines with their indent already stripped) per the active wrap mode,
/// pushing fully-indented output lines onto `out`.
fn emit_folded_paragraph(
    para: &[&str],
    base_indent: usize,
    indent_str: &str,
    opts: &YamlFormatOptions,
    profile: ResolvedProfile<'_>,
    out: &mut Vec<String>,
) {
    match opts.wrap {
        WrapMode::Reflow => {
            let prose = para.join(" ");
            let wrapped = wrap_plain_scalar_text(&prose, base_indent, base_indent, opts.line_width);
            out.push(format!("{indent_str}{wrapped}"));
        }
        WrapMode::Sentence => {
            let prose = para.join(" ");
            for sentence in sentence_wrap::split_sentence_text(&prose, profile) {
                out.push(format!("{indent_str}{sentence}"));
            }
        }
        WrapMode::Semantic => {
            for line in para {
                for sentence in sentence_wrap::split_sentence_text(line, profile) {
                    out.push(format!("{indent_str}{sentence}"));
                }
            }
        }
        WrapMode::Preserve => {
            for line in para {
                out.push(format!("{indent_str}{line}"));
            }
        }
    }
}

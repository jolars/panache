//! Test-only CST → HTML renderer for CommonMark conformance checks.
//!
//! This is **not** a public API and not a complete CommonMark renderer. It
//! covers the constructs spec.txt exercises and emits HTML in the same shape
//! as the reference renderer (commonmark-hs / cmark) so we can byte-compare
//! against `expected_html` after the shared `<li>`/`</li>` whitespace
//! normalization.
//!
//! Coverage is intentionally incremental. When a spec example fails the
//! comparison it goes to the allowlist (if newly passing) or stays off it
//! (if newly broken). The renderer grows in lockstep with parser fixes.

use panache_parser::SyntaxNode;
use panache_parser::syntax::SyntaxKind;
use rowan::NodeOrToken;
use std::collections::HashMap;

pub fn render(tree: &SyntaxNode) -> String {
    let refs = collect_references(tree);
    let mut out = String::new();
    render_blocks(tree, &refs, &mut out);
    restore_entity_placeholders(&out)
}

#[derive(Debug, Clone)]
struct RefDef {
    url: String,
    title: Option<String>,
}

fn collect_references(tree: &SyntaxNode) -> HashMap<String, RefDef> {
    let mut refs = HashMap::new();
    for desc in tree.descendants() {
        if desc.kind() == SyntaxKind::REFERENCE_DEFINITION
            && let Some((label, def)) = parse_reference_definition(&desc)
        {
            refs.entry(normalize_label(&label)).or_insert(def);
        }
    }
    refs
}

fn parse_reference_definition(node: &SyntaxNode) -> Option<(String, RefDef)> {
    let mut label = String::new();
    for child in node.children() {
        if child.kind() == SyntaxKind::LINK {
            for grand in child.children() {
                if grand.kind() == SyntaxKind::LINK_TEXT {
                    label = collect_text(&grand);
                }
            }
        }
    }
    if label.is_empty() {
        return None;
    }

    let mut tail = String::new();
    for el in node.children_with_tokens() {
        if let NodeOrToken::Token(t) = el
            && t.kind() == SyntaxKind::TEXT
        {
            tail.push_str(t.text());
        }
    }
    let tail = tail.trim_start_matches(':').trim();
    let (url, title) = split_dest_and_title(tail);
    Some((
        label,
        RefDef {
            url: decode_entities(&decode_backslash_escapes(&strip_angle_brackets(&url))),
            title: title.map(|t| decode_entities(&decode_backslash_escapes(&t))),
        },
    ))
}

fn split_dest_and_title(text: &str) -> (String, Option<String>) {
    let text = text.trim();
    let mut url_end = text.len();
    let mut title = None;
    for (i, c) in text.char_indices() {
        if c.is_whitespace() {
            url_end = i;
            let rest = text[i..].trim();
            if let Some(t) = parse_title(rest) {
                title = Some(t);
            }
            break;
        }
    }
    (text[..url_end].to_string(), title)
}

fn parse_title(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    let (open, close) = match bytes[0] {
        b'"' => (b'"', b'"'),
        b'\'' => (b'\'', b'\''),
        b'(' => (b'(', b')'),
        _ => return None,
    };
    if !text.starts_with(open as char) {
        return None;
    }
    let inner_end = text[1..].rfind(close as char)?;
    Some(text[1..1 + inner_end].to_string())
}

fn normalize_label(label: &str) -> String {
    label
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn render_blocks(parent: &SyntaxNode, refs: &HashMap<String, RefDef>, out: &mut String) {
    for child in parent.children() {
        match child.kind() {
            SyntaxKind::HEADING => render_heading(&child, refs, out),
            SyntaxKind::PARAGRAPH => render_paragraph(&child, refs, out),
            SyntaxKind::LIST => render_list(&child, refs, out),
            SyntaxKind::BLOCK_QUOTE => render_block_quote(&child, refs, out),
            SyntaxKind::CODE_BLOCK => render_code_block(&child, out),
            SyntaxKind::HORIZONTAL_RULE => out.push_str("<hr />\n"),
            SyntaxKind::REFERENCE_DEFINITION => {} // not rendered
            SyntaxKind::HTML_BLOCK => render_html_block(&child, out),
            SyntaxKind::BLANK_LINE => {}
            // Anything we don't model yet: render its inline-projected text
            // verbatim wrapped in a paragraph as a best-effort fallback.
            _ => {
                let text = child.text().to_string();
                if !text.trim().is_empty() {
                    out.push_str("<p>");
                    out.push_str(&escape_html(text.trim_end_matches('\n')));
                    out.push_str("</p>\n");
                }
            }
        }
    }
}

fn render_heading(node: &SyntaxNode, refs: &HashMap<String, RefDef>, out: &mut String) {
    let level = heading_level(node).clamp(1, 6);
    let mut inner = String::new();
    if let Some(content) = node
        .children()
        .find(|c| c.kind() == SyntaxKind::HEADING_CONTENT)
    {
        render_inlines(&content, refs, &mut inner);
    }
    let trimmed = inner.trim_matches(|c: char| c == ' ' || c == '\t');
    out.push_str(&format!("<h{level}>"));
    out.push_str(trimmed);
    out.push_str(&format!("</h{level}>\n"));
}

fn heading_level(node: &SyntaxNode) -> usize {
    for child in node.children() {
        if child.kind() == SyntaxKind::ATX_HEADING_MARKER {
            for tok in child.children_with_tokens() {
                if let Some(t) = tok.as_token()
                    && t.kind() == SyntaxKind::ATX_HEADING_MARKER
                {
                    return t.text().chars().filter(|&c| c == '#').count();
                }
            }
        }
    }
    for el in node.descendants_with_tokens() {
        if let NodeOrToken::Token(t) = el
            && t.kind() == SyntaxKind::SETEXT_HEADING_UNDERLINE
        {
            return if t.text().trim_start().starts_with('=') {
                1
            } else {
                2
            };
        }
    }
    1
}

fn render_paragraph(node: &SyntaxNode, refs: &HashMap<String, RefDef>, out: &mut String) {
    let mut inner = String::new();
    render_inlines(node, refs, &mut inner);
    // CommonMark: trailing newlines of a paragraph are not part of its
    // content, and any hard line breaks at the end of the block are removed.
    // A backslash-form hard line break (`\\\n`) at end-of-block leaves the
    // backslash as literal text per spec ("Hard line breaks at the end of a
    // block are removed"; the spec example uses `foo\` ⇒ `<p>foo\</p>`).
    let trailing_backslash = paragraph_ends_with_backslash_hard_break(node);
    loop {
        if let Some(rest) = inner.strip_suffix('\n') {
            inner.truncate(rest.len());
            continue;
        }
        if let Some(rest) = inner.strip_suffix("<br />") {
            inner.truncate(rest.len());
            continue;
        }
        break;
    }
    if trailing_backslash {
        inner.push('\\');
    }
    inner = strip_paragraph_line_indent(&inner);
    out.push_str("<p>");
    out.push_str(&inner);
    out.push_str("</p>\n");
}

/// CommonMark §4.8: a paragraph's raw content is built by concatenating its
/// lines and stripping leading/trailing whitespace from each line. The parser
/// preserves those bytes inside paragraph TEXT tokens (CST is lossless), so
/// the renderer trims them here. We only trim ASCII spaces and tabs at line
/// starts; whitespace inside inline constructs is unaffected because their
/// render output never contains a bare leading space at line start (links/
/// images/code spans render as inline tags, and code-span newlines are
/// already turned into spaces during normalization).
///
/// `decode_entities` substitutes whitespace produced by entity references with
/// the placeholders defined in `entity_placeholders`, so this strip pass only
/// removes whitespace that came directly from source bytes. Placeholders are
/// converted back at the top of `render`.
fn strip_paragraph_line_indent(inner: &str) -> String {
    let mut out = String::with_capacity(inner.len());
    let mut at_line_start = true;
    for ch in inner.chars() {
        if at_line_start && (ch == ' ' || ch == '\t') {
            continue;
        }
        if ch == '\n' {
            // Trim trailing whitespace before pushing the newline.
            while let Some(c) = out.chars().last() {
                if c == ' ' || c == '\t' {
                    out.pop();
                } else {
                    break;
                }
            }
            out.push(ch);
            at_line_start = true;
            continue;
        }
        out.push(ch);
        at_line_start = false;
    }
    out
}

fn paragraph_ends_with_backslash_hard_break(node: &SyntaxNode) -> bool {
    for el in node
        .descendants_with_tokens()
        .collect::<Vec<_>>()
        .iter()
        .rev()
    {
        if let NodeOrToken::Token(t) = el {
            match t.kind() {
                SyntaxKind::HARD_LINE_BREAK => return t.text().starts_with('\\'),
                SyntaxKind::NEWLINE | SyntaxKind::WHITESPACE => continue,
                _ => return false,
            }
        }
    }
    false
}

fn render_list(node: &SyntaxNode, refs: &HashMap<String, RefDef>, out: &mut String) {
    let (tag, start_attr) = list_tag_and_start(node);
    let loose = is_loose_list(node);
    out.push_str(&format!("<{tag}{start_attr}>\n"));
    for item in node
        .children()
        .filter(|c| c.kind() == SyntaxKind::LIST_ITEM)
    {
        render_list_item(&item, refs, loose, out);
    }
    out.push_str(&format!("</{tag}>\n"));
}

fn list_tag_and_start(node: &SyntaxNode) -> (&'static str, String) {
    let first_marker = node
        .children()
        .find(|c| c.kind() == SyntaxKind::LIST_ITEM)
        .and_then(|item| {
            item.children_with_tokens()
                .filter_map(|el| el.into_token())
                .find(|t| t.kind() == SyntaxKind::LIST_MARKER)
                .map(|t| t.text().to_string())
        })
        .unwrap_or_default();
    let trimmed = first_marker.trim();
    if trimmed.starts_with(['-', '+', '*']) {
        ("ul", String::new())
    } else {
        let digits: String = trimmed.chars().take_while(|c| c.is_ascii_digit()).collect();
        let start = digits.parse::<u64>().unwrap_or(1);
        let attr = if start == 1 {
            String::new()
        } else {
            format!(" start=\"{start}\"")
        };
        ("ol", attr)
    }
}

fn is_loose_list(node: &SyntaxNode) -> bool {
    // CommonMark §5.3: a list is loose if any of its constituent list items
    // are separated by blank lines, or if any item directly contains two
    // block-level elements separated by a blank line. Approximation:
    // - any LIST_ITEM has a PARAGRAPH descendant (the parser uses PLAIN for
    //   tight items), OR
    // - two LIST_ITEMs are separated by a BLANK_LINE in the list node, OR
    // - any LIST_ITEM has a BLANK_LINE between two block-level children.
    let mut prev_was_item = false;
    for child in node.children_with_tokens() {
        match child {
            NodeOrToken::Node(n) => {
                if n.kind() == SyntaxKind::LIST_ITEM {
                    if n.descendants().any(|d| d.kind() == SyntaxKind::PARAGRAPH) {
                        return true;
                    }
                    if list_item_has_internal_blank(&n) {
                        return true;
                    }
                    prev_was_item = true;
                } else if n.kind() == SyntaxKind::BLANK_LINE
                    && prev_was_item
                    && n.next_sibling()
                        .map(|s| s.kind() == SyntaxKind::LIST_ITEM)
                        .unwrap_or(false)
                {
                    return true;
                }
            }
            NodeOrToken::Token(_) => {}
        }
    }
    false
}

fn list_item_has_internal_blank(item: &SyntaxNode) -> bool {
    let mut saw_block = false;
    for child in item.children() {
        match child.kind() {
            SyntaxKind::BLANK_LINE => {
                if saw_block
                    && child
                        .next_sibling()
                        .is_some_and(|s| is_block_child(s.kind()))
                {
                    return true;
                }
            }
            k if is_block_child(k) => {
                saw_block = true;
            }
            _ => {}
        }
    }
    false
}

fn is_block_child(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::PLAIN
            | SyntaxKind::PARAGRAPH
            | SyntaxKind::HEADING
            | SyntaxKind::CODE_BLOCK
            | SyntaxKind::BLOCK_QUOTE
            | SyntaxKind::LIST
            | SyntaxKind::HORIZONTAL_RULE
            | SyntaxKind::HTML_BLOCK
    )
}

fn render_list_item(
    item: &SyntaxNode,
    refs: &HashMap<String, RefDef>,
    loose: bool,
    out: &mut String,
) {
    out.push_str("<li>");
    if loose {
        out.push('\n');
    }
    let mut wrote_block = false;
    for child in item.children() {
        match child.kind() {
            SyntaxKind::PLAIN => {
                if loose {
                    render_paragraph(&child, refs, out);
                } else {
                    let mut inner = String::new();
                    render_inlines(&child, refs, &mut inner);
                    out.push_str(&strip_paragraph_line_indent(&inner));
                }
                wrote_block = true;
            }
            SyntaxKind::PARAGRAPH => {
                render_paragraph(&child, refs, out);
                wrote_block = true;
            }
            SyntaxKind::LIST => {
                if !loose && !wrote_block {
                    // edge: list item starts with a nested list
                }
                render_list(&child, refs, out);
                wrote_block = true;
            }
            SyntaxKind::CODE_BLOCK => {
                render_code_block(&child, out);
                wrote_block = true;
            }
            SyntaxKind::BLOCK_QUOTE => {
                render_block_quote(&child, refs, out);
                wrote_block = true;
            }
            SyntaxKind::HEADING => {
                render_heading(&child, refs, out);
                wrote_block = true;
            }
            SyntaxKind::HORIZONTAL_RULE => {
                out.push_str("<hr />\n");
                wrote_block = true;
            }
            SyntaxKind::BLANK_LINE => {}
            _ => {}
        }
    }
    out.push_str("</li>\n");
}

fn render_block_quote(node: &SyntaxNode, refs: &HashMap<String, RefDef>, out: &mut String) {
    out.push_str("<blockquote>\n");
    render_blocks(node, refs, out);
    out.push_str("</blockquote>\n");
}

fn render_code_block(node: &SyntaxNode, out: &mut String) {
    let lang = code_block_language(node);
    let class = if lang.is_empty() {
        String::new()
    } else {
        format!(" class=\"language-{}\"", escape_attr(&lang))
    };
    out.push_str(&format!("<pre><code{class}>"));
    let content = code_block_content(node);
    out.push_str(&escape_html(&content));
    out.push_str("</code></pre>\n");
}

fn code_block_language(node: &SyntaxNode) -> String {
    let Some(open) = node
        .children()
        .find(|c| c.kind() == SyntaxKind::CODE_FENCE_OPEN)
    else {
        return String::new();
    };
    for desc in open.descendants_with_tokens() {
        match desc {
            NodeOrToken::Node(n) if n.kind() == SyntaxKind::CODE_LANGUAGE => {
                return decode_backslash_escapes(&decode_entities(n.text().to_string().trim()));
            }
            NodeOrToken::Token(t) if t.kind() == SyntaxKind::CODE_LANGUAGE => {
                return decode_backslash_escapes(&decode_entities(t.text().trim()));
            }
            _ => {}
        }
    }
    String::new()
}

/// Number of space characters that precede the opening fence on the same line.
/// Used to drive CommonMark's "strip equivalent leading whitespace" rule.
fn fenced_opener_indent(node: &SyntaxNode) -> usize {
    let mut indent = 0usize;
    for el in node.children_with_tokens() {
        match el {
            NodeOrToken::Token(t) if t.kind() == SyntaxKind::WHITESPACE => {
                indent += t.text().chars().filter(|c| *c == ' ').count();
            }
            NodeOrToken::Node(n) if n.kind() == SyntaxKind::CODE_FENCE_OPEN => {
                return indent;
            }
            _ => {
                indent = 0;
            }
        }
    }
    0
}

fn code_block_content(node: &SyntaxNode) -> String {
    let is_fenced = node
        .children()
        .any(|c| c.kind() == SyntaxKind::CODE_FENCE_OPEN);
    let bq_depth = blockquote_depth(node);
    let li_indent = enclosing_list_item_content_column(node);
    let mut content = String::new();
    if is_fenced {
        // Per CommonMark §4.5: if the opening fence is indented, content lines
        // have an equivalent amount of leading whitespace removed (capped at
        // the opener's indent — extra indentation is preserved verbatim).
        let opener_indent = fenced_opener_indent(node);
        for child in node.children() {
            if child.kind() == SyntaxKind::CODE_CONTENT {
                let raw = child.text().to_string();
                let raw = if bq_depth > 0 {
                    strip_blockquote_prefix_per_line(&raw, bq_depth)
                } else {
                    raw
                };
                // Inside a list item the parser keeps the list-item content
                // column on each content line for losslessness. Strip it
                // before applying the opener-indent rule so the rendered
                // payload matches what the source meant.
                let raw = if li_indent > 0 {
                    strip_leading_spaces_per_line(&raw, li_indent)
                } else {
                    raw
                };
                if opener_indent == 0 {
                    content.push_str(&raw);
                } else {
                    for line in raw.split_inclusive('\n') {
                        let mut stripped = 0usize;
                        let bytes = line.as_bytes();
                        while stripped < opener_indent
                            && stripped < bytes.len()
                            && bytes[stripped] == b' '
                        {
                            stripped += 1;
                        }
                        content.push_str(&line[stripped..]);
                    }
                }
            }
        }
    } else {
        // Indented code block: strip up to 4 leading spaces (or 1 tab) from
        // each line. Blank lines (whitespace-only) are also stripped of up
        // to 4 leading spaces; any remaining whitespace is preserved.
        // Inside a list item, also strip the list-item content column first
        // so the 4-space indented-code marker remains the only thing the
        // generic strip removes.
        for child in node.children() {
            if child.kind() == SyntaxKind::CODE_CONTENT {
                let raw = child.text().to_string();
                let raw = if bq_depth > 0 {
                    strip_blockquote_prefix_per_line(&raw, bq_depth)
                } else {
                    raw
                };
                let raw = if li_indent > 0 {
                    strip_leading_spaces_per_line(&raw, li_indent)
                } else {
                    raw
                };
                for line in raw.split_inclusive('\n') {
                    if let Some(rest) = line.strip_prefix('\t') {
                        content.push_str(rest);
                        continue;
                    }
                    let body_len = line.len() - if line.ends_with('\n') { 1 } else { 0 };
                    let body = &line[..body_len];
                    let bytes = line.as_bytes();
                    let mut stripped = 0usize;
                    while stripped < 4 && stripped < body_len && bytes[stripped] == b' ' {
                        stripped += 1;
                    }
                    // If the line is whitespace-only and shorter than the
                    // indent, the entire line is consumed (only the newline
                    // remains).
                    if stripped == body_len
                        && body.chars().all(|c| c == ' ' || c == '\t')
                        && stripped < 4
                    {
                        if line.ends_with('\n') {
                            content.push('\n');
                        }
                    } else {
                        content.push_str(&line[stripped..]);
                    }
                }
            }
        }
    }
    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    content
}

/// Walk up to the immediate enclosing `LIST_ITEM` ancestor and return its
/// content column (the column at which post-marker content begins). Returns
/// `0` if the node is not inside a list item, so callers can short-circuit.
///
/// Stops at the *first* `LIST_ITEM` because deeper nesting already accounts
/// for outer list indents in the inner item's leading whitespace token.
fn enclosing_list_item_content_column(node: &SyntaxNode) -> usize {
    let mut current = node.parent();
    while let Some(parent) = current {
        if parent.kind() == SyntaxKind::LIST_ITEM {
            return list_item_content_column(&parent);
        }
        current = parent.parent();
    }
    0
}

fn list_item_content_column(item: &SyntaxNode) -> usize {
    let mut col = 0usize;
    for el in item.children_with_tokens() {
        match el {
            NodeOrToken::Token(t) => match t.kind() {
                SyntaxKind::WHITESPACE => col += t.text().chars().count(),
                SyntaxKind::LIST_MARKER => col += t.text().chars().count(),
                _ => return col,
            },
            NodeOrToken::Node(_) => return col,
        }
    }
    col
}

fn strip_leading_spaces_per_line(text: &str, max: usize) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        let bytes = line.as_bytes();
        let body_len = line.len() - if line.ends_with('\n') { 1 } else { 0 };
        let mut stripped = 0usize;
        while stripped < max && stripped < body_len && bytes[stripped] == b' ' {
            stripped += 1;
        }
        out.push_str(&line[stripped..]);
    }
    out
}

fn render_html_block(node: &SyntaxNode, out: &mut String) {
    let text = node.text().to_string();
    let depth = blockquote_depth(node);
    let stripped = if depth > 0 {
        strip_blockquote_prefix_per_line(&text, depth)
    } else {
        text
    };
    let trimmed = stripped.trim_end_matches('\n');
    out.push_str(trimmed);
    out.push('\n');
}

/// Count `BLOCK_QUOTE` ancestors so the renderer can strip the equivalent
/// number of `> ` prefixes from raw block content (HTML blocks, code-block
/// content). The parser preserves blockquote markers inside these nodes for
/// losslessness; the spec output excludes them.
fn blockquote_depth(node: &SyntaxNode) -> usize {
    let mut depth = 0;
    let mut current = node.parent();
    while let Some(parent) = current {
        if parent.kind() == SyntaxKind::BLOCK_QUOTE {
            depth += 1;
        }
        current = parent.parent();
    }
    depth
}

fn strip_blockquote_prefix_per_line(text: &str, depth: usize) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        let (line_body, newline) = if let Some(stripped) = line.strip_suffix('\n') {
            (stripped, "\n")
        } else {
            (line, "")
        };
        out.push_str(strip_blockquote_prefix(line_body, depth));
        out.push_str(newline);
    }
    out
}

fn strip_blockquote_prefix(line: &str, depth: usize) -> &str {
    let mut remaining = line;
    for _ in 0..depth {
        let bytes = remaining.as_bytes();
        let mut i = 0;
        while i < bytes.len() && i < 3 && bytes[i] == b' ' {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] != b'>' {
            break;
        }
        i += 1;
        if i < bytes.len() && bytes[i] == b' ' {
            i += 1;
        }
        remaining = &remaining[i..];
    }
    remaining
}

fn render_inlines(parent: &SyntaxNode, refs: &HashMap<String, RefDef>, out: &mut String) {
    for el in parent.children_with_tokens() {
        match el {
            NodeOrToken::Token(t) => render_token(&t, out),
            NodeOrToken::Node(n) => render_inline_node(&n, refs, out),
        }
    }
}

fn render_token(t: &rowan::SyntaxToken<panache_parser::syntax::PanacheLanguage>, out: &mut String) {
    match t.kind() {
        SyntaxKind::TEXT => out.push_str(&escape_html(&decode_entities(t.text()))),
        SyntaxKind::ESCAPED_CHAR => {
            // \x — emit just the escaped character, HTML-escaped
            let text = t.text();
            let ch = text.chars().nth(1).unwrap_or(' ');
            out.push_str(&escape_html(&ch.to_string()));
        }
        SyntaxKind::HARD_LINE_BREAK => out.push_str("<br />\n"),
        SyntaxKind::NEWLINE => out.push('\n'),
        SyntaxKind::WHITESPACE => out.push_str(t.text()),
        SyntaxKind::NONBREAKING_SPACE => out.push(' '),
        // Various stray tokens we skip silently; they'll show up as parser
        // bugs in the harness if they materially affect output.
        _ => {}
    }
}

fn render_inline_node(node: &SyntaxNode, refs: &HashMap<String, RefDef>, out: &mut String) {
    match node.kind() {
        SyntaxKind::EMPHASIS => {
            out.push_str("<em>");
            render_inlines(node, refs, out);
            out.push_str("</em>");
        }
        SyntaxKind::STRONG => {
            out.push_str("<strong>");
            render_inlines(node, refs, out);
            out.push_str("</strong>");
        }
        SyntaxKind::INLINE_CODE => {
            let raw: String = node
                .children_with_tokens()
                .filter_map(|el| el.into_token())
                .filter(|t| t.kind() == SyntaxKind::INLINE_CODE_CONTENT)
                .map(|t| t.text().to_string())
                .collect();
            let content = normalize_code_span(&raw);
            out.push_str("<code>");
            out.push_str(&escape_html(&content));
            out.push_str("</code>");
        }
        SyntaxKind::LINK => render_link(node, refs, out),
        SyntaxKind::IMAGE_LINK => render_image(node, refs, out),
        SyntaxKind::AUTO_LINK => render_autolink(node, out),
        SyntaxKind::INLINE_HTML => {
            // Emit verbatim — entities and backslashes are not processed
            // inside raw HTML spans (CommonMark §6.6). Whitespace inside the
            // span must survive `strip_paragraph_line_indent`, so substitute
            // private-use placeholders that `restore_entity_placeholders`
            // reverses before the final HTML is returned.
            for el in node.children_with_tokens() {
                if let NodeOrToken::Token(t) = el
                    && t.kind() == SyntaxKind::INLINE_HTML_CONTENT
                {
                    for c in t.text().chars() {
                        out.push(protect_entity_whitespace(c));
                    }
                }
            }
        }
        _ => {
            // Fallback: descend into children, treating everything as inline
            for el in node.children_with_tokens() {
                match el {
                    NodeOrToken::Token(t) => render_token(&t, out),
                    NodeOrToken::Node(n) => render_inline_node(&n, refs, out),
                }
            }
        }
    }
}

fn render_link(node: &SyntaxNode, refs: &HashMap<String, RefDef>, out: &mut String) {
    let text_node = node.children().find(|c| c.kind() == SyntaxKind::LINK_TEXT);
    let dest_node = node.children().find(|c| c.kind() == SyntaxKind::LINK_DEST);

    let (url, title) = if let Some(d) = dest_node.as_ref() {
        let raw = d.text().to_string();
        let (url, title) = split_dest_and_title(raw.trim_matches(['(', ')'].as_ref()));
        (
            decode_entities(&decode_backslash_escapes(&strip_angle_brackets(&url))),
            title.map(|t| decode_entities(&decode_backslash_escapes(&t))),
        )
    } else if let Some(label_node) = node.children().find(|c| c.kind() == SyntaxKind::LINK_REF) {
        let label = collect_text(&label_node);
        match refs.get(&normalize_label(&label)) {
            Some(def) => (def.url.clone(), def.title.clone()),
            None => {
                // Unresolved reference: render verbatim
                out.push_str(&escape_html(&node.text().to_string()));
                return;
            }
        }
    } else {
        // Shortcut reference [label] resolves with text as the label
        let label = text_node.as_ref().map(collect_text).unwrap_or_default();
        match refs.get(&normalize_label(&label)) {
            Some(def) => (def.url.clone(), def.title.clone()),
            None => {
                out.push_str(&escape_html(&node.text().to_string()));
                return;
            }
        }
    };

    out.push_str("<a href=\"");
    out.push_str(&encode_url(&url));
    out.push('"');
    if let Some(t) = title {
        out.push_str(" title=\"");
        out.push_str(&escape_attr(&t));
        out.push('"');
    }
    out.push('>');
    if let Some(text) = text_node {
        render_inlines(&text, refs, out);
    }
    out.push_str("</a>");
}

fn render_image(node: &SyntaxNode, refs: &HashMap<String, RefDef>, out: &mut String) {
    let alt_node = node.children().find(|c| c.kind() == SyntaxKind::IMAGE_ALT);
    let dest_node = node.children().find(|c| c.kind() == SyntaxKind::LINK_DEST);
    let ref_node = node.children().find(|c| c.kind() == SyntaxKind::LINK_REF);

    let (url, title) = if let Some(d) = dest_node.as_ref() {
        let raw = d.text().to_string();
        let (url, title) = split_dest_and_title(raw.trim_matches(['(', ')'].as_ref()));
        (
            decode_entities(&decode_backslash_escapes(&strip_angle_brackets(&url))),
            title.map(|t| decode_entities(&decode_backslash_escapes(&t))),
        )
    } else {
        // Reference-style image. CommonMark §6.4: a link label is normalized
        // by case-folding and collapsing whitespace.
        // - Full reference: `![alt][label]` — use LINK_REF text.
        // - Collapsed: `![alt][]` — empty LINK_REF, use IMAGE_ALT raw source.
        // - Shortcut: `![alt]` — no LINK_REF, use IMAGE_ALT raw source.
        // The IMAGE_ALT raw source preserves emphasis markers so labels match
        // the reference definition's bracket text byte-for-byte after
        // normalization.
        let label = match ref_node.as_ref() {
            Some(rn) => {
                let l = rn.text().to_string();
                if l.trim().is_empty() {
                    alt_node
                        .as_ref()
                        .map(|n| n.text().to_string())
                        .unwrap_or_default()
                } else {
                    l
                }
            }
            None => alt_node
                .as_ref()
                .map(|n| n.text().to_string())
                .unwrap_or_default(),
        };
        match refs.get(&normalize_label(&label)) {
            Some(def) => (def.url.clone(), def.title.clone()),
            None => {
                out.push_str(&escape_html(&node.text().to_string()));
                return;
            }
        }
    };

    let alt = alt_node.as_ref().map(collect_alt_text).unwrap_or_default();
    out.push_str("<img src=\"");
    out.push_str(&encode_url(&url));
    out.push_str("\" alt=\"");
    out.push_str(&escape_attr(&alt));
    out.push('"');
    if let Some(t) = title {
        out.push_str(" title=\"");
        out.push_str(&escape_attr(&t));
        out.push('"');
    }
    out.push_str(" />");
}

/// CommonMark §6.5: only the plain string content of the image description is
/// used for the `alt` attribute. Concatenate text tokens, but for nested
/// images/links descend only into their description (skip their destination
/// and title metadata).
fn collect_alt_text(node: &SyntaxNode) -> String {
    let mut out = String::new();
    push_alt_from(node, &mut out);
    out
}

fn push_alt_from(node: &SyntaxNode, out: &mut String) {
    for el in node.children_with_tokens() {
        match el {
            NodeOrToken::Token(t) => match t.kind() {
                SyntaxKind::TEXT | SyntaxKind::INLINE_CODE_CONTENT => out.push_str(t.text()),
                SyntaxKind::ESCAPED_CHAR => {
                    if let Some(ch) = t.text().chars().nth(1) {
                        out.push(ch);
                    }
                }
                _ => {}
            },
            NodeOrToken::Node(n) => match n.kind() {
                SyntaxKind::LINK => {
                    if let Some(text) = n.children().find(|c| c.kind() == SyntaxKind::LINK_TEXT) {
                        push_alt_from(&text, out);
                    }
                }
                SyntaxKind::IMAGE_LINK => {
                    if let Some(alt) = n.children().find(|c| c.kind() == SyntaxKind::IMAGE_ALT) {
                        push_alt_from(&alt, out);
                    }
                }
                _ => push_alt_from(&n, out),
            },
        }
    }
}

/// CommonMark §6.3: when a link destination is wrapped in `<...>`, the angle
/// brackets are stripped and entity/numeric references inside the wrapping
/// are decoded normally.
fn strip_angle_brackets(s: &str) -> String {
    let trimmed = s.trim();
    if trimmed.starts_with('<') && trimmed.ends_with('>') && trimmed.len() >= 2 {
        trimmed[1..trimmed.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

/// CommonMark §2.4: backslash before any ASCII punctuation char is treated as
/// an escape and emits the literal character. In link destinations and titles
/// these escapes apply before entity references; in raw text they apply
/// instead of treating the punctuation as a delimiter. We apply this to URL
/// and title strings extracted from the CST.
fn decode_backslash_escapes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\'
            && let Some(&next) = chars.peek()
            && next.is_ascii_punctuation()
        {
            chars.next();
            out.push(next);
            continue;
        }
        out.push(c);
    }
    out
}

fn render_autolink(node: &SyntaxNode, out: &mut String) {
    let target: String = node
        .children_with_tokens()
        .filter_map(|el| el.into_token())
        .filter(|t| t.kind() == SyntaxKind::TEXT)
        .map(|t| t.text().to_string())
        .collect();
    // Per CommonMark §6.4–6.5, an autolink is a URI autolink if its content has
    // a valid scheme (2–32 chars: letter then letter/digit/+./-, then `:`).
    // Otherwise, if it contains `@`, it's an email autolink (prepend mailto:).
    let href = if has_uri_scheme(&target) {
        target.clone()
    } else if target.contains('@') {
        format!("mailto:{}", target)
    } else {
        target.clone()
    };
    out.push_str("<a href=\"");
    out.push_str(&encode_url(&href));
    out.push_str("\">");
    out.push_str(&escape_html(&target));
    out.push_str("</a>");
}

fn has_uri_scheme(s: &str) -> bool {
    let bytes = s.as_bytes();
    if bytes.is_empty() || !bytes[0].is_ascii_alphabetic() {
        return false;
    }
    for (i, &b) in bytes.iter().enumerate() {
        if b == b':' {
            return (2..=32).contains(&i);
        }
        if !(b.is_ascii_alphanumeric() || b == b'+' || b == b'.' || b == b'-') {
            return false;
        }
    }
    false
}

fn normalize_code_span(raw: &str) -> String {
    // CommonMark §6.1: line endings → spaces; if the result both begins and
    // ends with a single ASCII space and is not entirely spaces, strip one
    // leading and one trailing space.
    let spaced: String = raw
        .chars()
        .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
        .collect();
    let bytes = spaced.as_bytes();
    let all_spaces = !bytes.is_empty() && bytes.iter().all(|&b| b == b' ');
    if !all_spaces
        && bytes.len() >= 2
        && bytes.first() == Some(&b' ')
        && bytes.last() == Some(&b' ')
    {
        return spaced[1..spaced.len() - 1].to_string();
    }
    spaced
}

fn collect_text(node: &SyntaxNode) -> String {
    node.descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .filter(|t| t.kind() == SyntaxKind::TEXT)
        .map(|t| t.text().to_string())
        .collect()
}

fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

fn escape_attr(s: &str) -> String {
    escape_html(s)
}

/// Minimal URL encoding per CommonMark §6.1: percent-encode bytes outside
/// the safe set, but pass through already-percent-encoded sequences.
fn encode_url(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'%' && i + 2 < bytes.len() && is_hex(bytes[i + 1]) && is_hex(bytes[i + 2]) {
            out.push('%');
            out.push(bytes[i + 1] as char);
            out.push(bytes[i + 2] as char);
            i += 3;
            continue;
        }
        if b == b'&' {
            out.push_str("&amp;");
        } else if is_url_safe(b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{:02X}", b));
        }
        i += 1;
    }
    out
}

fn is_hex(b: u8) -> bool {
    b.is_ascii_hexdigit()
}

fn is_url_safe(b: u8) -> bool {
    // Per CommonMark: alphanumerics + URI-reserved characters, *minus* `[` and
    // `]` which carry markdown-specific meaning and so are always
    // percent-encoded in rendered HTML output (e.g. spec §6.4 example with
    // `https://example.com/?search=][ref]` → `%5D%5Bref%5D`).
    matches!(b,
        b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9'
        | b'-' | b'.' | b'_' | b'~'
        | b':' | b'/' | b'?' | b'#' | b'@'
        | b'!' | b'$' | b'\'' | b'(' | b')' | b'*' | b'+' | b',' | b';' | b'='
    )
}

/// Decode CommonMark entity and numeric character references in a string.
/// Per spec §6.2: only references with a trailing `;` are recognized;
/// unknown names are left as literal text. Invalid code points (NUL,
/// surrogates, > U+10FFFF) are replaced with U+FFFD. This is applied
/// before HTML-escaping in inline contexts (paragraphs, links, etc.)
/// and is *not* applied inside code spans or code blocks.
///
/// ASCII whitespace produced by entity decoding is substituted with private-use
/// placeholder characters (see `entity_placeholders`) so paragraph line-indent
/// stripping doesn't treat decoded whitespace as if it were a source-level
/// indent. `restore_entity_placeholders` reverses the substitution before the
/// final HTML is returned.
fn decode_entities(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut idx = 0;
    while idx < input.len() {
        let rest = &input[idx..];
        if rest.starts_with('&')
            && let Some(semi) = find_entity_end(rest)
            && let Some(decoded) = decode_one_entity(&rest[..=semi])
        {
            for c in decoded.chars() {
                out.push(protect_entity_whitespace(c));
            }
            idx += semi + 1;
            continue;
        }
        let ch = rest.chars().next().unwrap();
        out.push(ch);
        idx += ch.len_utf8();
    }
    out
}

/// Map ASCII whitespace produced by entity decoding to private-use
/// placeholders so they survive the source-indent strip.
fn protect_entity_whitespace(c: char) -> char {
    match c {
        '\t' => '\u{E001}',
        ' ' => '\u{E002}',
        _ => c,
    }
}

fn restore_entity_placeholders(s: &str) -> String {
    s.replace('\u{E001}', "\t").replace('\u{E002}', " ")
}

/// Returns the byte index of the closing `;` if the prefix of `s` plausibly
/// looks like a CommonMark entity reference (named or numeric). Bails out on
/// invalid characters within the body.
fn find_entity_end(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    if bytes.first() != Some(&b'&') || bytes.len() < 3 {
        return None;
    }
    if bytes[1] == b'#' {
        let is_hex = matches!(bytes.get(2), Some(b'x' | b'X'));
        let body_start = if is_hex { 3 } else { 2 };
        let max_len = if is_hex { 6 } else { 7 };
        let mut i = body_start;
        while i < bytes.len() && i - body_start < max_len {
            let b = bytes[i];
            let valid = if is_hex {
                b.is_ascii_hexdigit()
            } else {
                b.is_ascii_digit()
            };
            if !valid {
                break;
            }
            i += 1;
        }
        if i == body_start {
            return None;
        }
        if bytes.get(i) == Some(&b';') {
            return Some(i);
        }
        return None;
    }
    // Named: `[A-Za-z][A-Za-z0-9]*;`. The longest HTML5 entity name is 31 chars
    // (`CounterClockwiseContourIntegral`).
    if !bytes[1].is_ascii_alphabetic() {
        return None;
    }
    let mut i = 2;
    while i < bytes.len() && i < 33 {
        let b = bytes[i];
        if b == b';' {
            return Some(i);
        }
        if !b.is_ascii_alphanumeric() {
            return None;
        }
        i += 1;
    }
    None
}

/// Decode a complete entity reference of the form `&NAME;`, `&#NNNN;`, or
/// `&#xHHHH;`. Returns `None` for unknown names so the caller can leave the
/// source bytes alone.
fn decode_one_entity(ent: &str) -> Option<String> {
    let body = ent.strip_prefix('&')?.strip_suffix(';')?;
    if let Some(rest) = body.strip_prefix('#') {
        let code = if let Some(hex) = rest.strip_prefix(['x', 'X']) {
            u32::from_str_radix(hex, 16).ok()?
        } else {
            rest.parse::<u32>().ok()?
        };
        return Some(decode_codepoint(code).to_string());
    }
    entities::ENTITIES
        .iter()
        .find(|e| e.entity == ent)
        .map(|e| e.characters.to_string())
}

fn decode_codepoint(c: u32) -> char {
    if c == 0 || c > 0x10FFFF || (0xD800..=0xDFFF).contains(&c) {
        '\u{FFFD}'
    } else {
        char::from_u32(c).unwrap_or('\u{FFFD}')
    }
}

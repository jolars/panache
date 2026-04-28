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
    out
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
    Some((label, RefDef { url, title }))
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
    // block-level elements separated by a blank line. Approximation: a list
    // is loose if any LIST_ITEM contains a PARAGRAPH (the parser uses PLAIN
    // for tight items) OR if multiple items are separated by a BLANK_LINE
    // in the list node.
    let mut prev_was_item = false;
    for child in node.children_with_tokens() {
        match child {
            NodeOrToken::Node(n) => {
                if n.kind() == SyntaxKind::LIST_ITEM {
                    if n.descendants().any(|d| d.kind() == SyntaxKind::PARAGRAPH) {
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
                    out.push_str("<p>");
                    render_inlines(&child, refs, out);
                    out.push_str("</p>\n");
                } else {
                    render_inlines(&child, refs, out);
                }
                wrote_block = true;
            }
            SyntaxKind::PARAGRAPH => {
                out.push_str("<p>");
                render_inlines(&child, refs, out);
                out.push_str("</p>\n");
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
    for child in node.children() {
        if child.kind() == SyntaxKind::CODE_FENCE_OPEN {
            for grand in child.children() {
                if grand.kind() == SyntaxKind::CODE_LANGUAGE {
                    return grand.text().to_string().trim().to_string();
                }
            }
            for tok in child.children_with_tokens() {
                if let Some(t) = tok.as_token()
                    && t.kind() == SyntaxKind::CODE_LANGUAGE
                {
                    return t.text().to_string().trim().to_string();
                }
            }
        }
    }
    String::new()
}

fn code_block_content(node: &SyntaxNode) -> String {
    let is_fenced = node
        .children()
        .any(|c| c.kind() == SyntaxKind::CODE_FENCE_OPEN);
    let mut content = String::new();
    if is_fenced {
        for child in node.children() {
            if child.kind() == SyntaxKind::CODE_CONTENT {
                content.push_str(&child.text().to_string());
            }
        }
    } else {
        // Indented code block: strip leading 4-space (or tab) indent on each line.
        for child in node.children() {
            if child.kind() == SyntaxKind::CODE_CONTENT {
                let raw = child.text().to_string();
                for line in raw.split_inclusive('\n') {
                    if let Some(rest) = line.strip_prefix("    ") {
                        content.push_str(rest);
                    } else if let Some(rest) = line.strip_prefix('\t') {
                        content.push_str(rest);
                    } else {
                        content.push_str(line);
                    }
                }
            }
        }
    }
    if !content.ends_with('\n') {
        content.push('\n');
    }
    content
}

fn render_html_block(node: &SyntaxNode, out: &mut String) {
    let text = node.text().to_string();
    let trimmed = text.trim_end_matches('\n');
    out.push_str(trimmed);
    out.push('\n');
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
        SyntaxKind::TEXT => out.push_str(&escape_html(t.text())),
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
        split_dest_and_title(raw.trim_matches(['(', ')'].as_ref()))
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

    let (url, title) = if let Some(d) = dest_node.as_ref() {
        let raw = d.text().to_string();
        split_dest_and_title(raw.trim_matches(['(', ')'].as_ref()))
    } else if let Some(label_node) = node.children().find(|c| c.kind() == SyntaxKind::LINK_REF) {
        let label = collect_text(&label_node);
        match refs.get(&normalize_label(&label)) {
            Some(def) => (def.url.clone(), def.title.clone()),
            None => {
                out.push_str(&escape_html(&node.text().to_string()));
                return;
            }
        }
    } else {
        out.push_str(&escape_html(&node.text().to_string()));
        return;
    };

    let alt = alt_node.as_ref().map(collect_text).unwrap_or_default();
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

fn render_autolink(node: &SyntaxNode, out: &mut String) {
    let target: String = node
        .children_with_tokens()
        .filter_map(|el| el.into_token())
        .filter(|t| t.kind() == SyntaxKind::TEXT)
        .map(|t| t.text().to_string())
        .collect();
    let href = if target.contains('@') && !target.starts_with("mailto:") {
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
    // Per CommonMark: alphanumerics + the URI-reserved characters.
    matches!(b,
        b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9'
        | b'-' | b'.' | b'_' | b'~'
        | b':' | b'/' | b'?' | b'#' | b'[' | b']' | b'@'
        | b'!' | b'$' | b'\'' | b'(' | b')' | b'*' | b'+' | b',' | b';' | b'='
    )
}

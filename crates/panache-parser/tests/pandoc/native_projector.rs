//! Test-only CST → Pandoc-native AST text projector.
//!
//! This is **not** a public API. It mirrors the role of `html_renderer.rs` in
//! the CommonMark conformance harness: walk the panache CST and emit a string
//! that, after `normalize_native()`, byte-equals the corresponding output of
//! `pandoc -f markdown -t native`.
//!
//! Coverage is intentionally narrow. Unsupported nodes emit
//! `Unsupported "<KIND>"` so a failing case stays visibly failing in the
//! report; expand coverage as the corpus grows.
//!
//! Output shape matches pandoc 3.9.0.2 with default-standalone-off behavior:
//! the document is rendered as a bare block list `[ <block>, ... ]`. The
//! comparison normalizer collapses whitespace runs, so ppShow's pretty-print
//! line breaks/indentation are not load-bearing.

use panache_parser::SyntaxNode;
use panache_parser::syntax::SyntaxKind;
use rowan::NodeOrToken;

pub fn project(tree: &SyntaxNode) -> String {
    let blocks = blocks_from_doc(tree);
    let mut out = String::new();
    out.push('[');
    for (i, b) in blocks.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push(' ');
        write_block(b, &mut out);
    }
    out.push_str(" ]");
    out
}

/// Canonical form of a Pandoc-native AST string. Tokenizes the input and
/// re-serializes it with single-space separation so that pretty-print line
/// breaks and indentation no longer affect equality.
pub fn normalize_native(s: &str) -> String {
    let mut tokens = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let c = bytes[i];
        match c {
            b' ' | b'\t' | b'\n' | b'\r' => {
                i += 1;
            }
            b'[' | b']' | b'(' | b')' | b',' => {
                tokens.push((c as char).to_string());
                i += 1;
            }
            b'"' => {
                // String literal: copy bytes until matching unescaped quote.
                let start = i;
                i += 1;
                while i < bytes.len() {
                    match bytes[i] {
                        b'\\' if i + 1 < bytes.len() => {
                            i += 2;
                        }
                        b'"' => {
                            i += 1;
                            break;
                        }
                        _ => {
                            i += 1;
                        }
                    }
                }
                tokens.push(s[start..i].to_string());
            }
            _ => {
                let start = i;
                while i < bytes.len() {
                    let b = bytes[i];
                    if matches!(
                        b,
                        b' ' | b'\t' | b'\n' | b'\r' | b'[' | b']' | b'(' | b')' | b',' | b'"'
                    ) {
                        break;
                    }
                    i += 1;
                }
                if i > start {
                    tokens.push(s[start..i].to_string());
                }
            }
        }
    }
    tokens.join(" ")
}

// Variant names mirror Pandoc's `Text.Pandoc.Definition` constructors so the
// emission code reads 1:1 against pandoc-native — `BlockQuote`, `CodeBlock`,
// `BulletList`, `OrderedList` are not redundant here, they are the spec names.
#[derive(Debug)]
#[allow(clippy::enum_variant_names)]
enum Block {
    Para(Vec<Inline>),
    Plain(Vec<Inline>),
    Header(usize, Attr, Vec<Inline>),
    BlockQuote(Vec<Block>),
    CodeBlock(Attr, String),
    HorizontalRule,
    BulletList(Vec<Vec<Block>>),
    OrderedList(usize, &'static str, &'static str, Vec<Vec<Block>>),
    Unsupported(String),
}

#[derive(Debug)]
enum Inline {
    Str(String),
    Space,
    SoftBreak,
    LineBreak,
    Emph(Vec<Inline>),
    Strong(Vec<Inline>),
    Strikeout(Vec<Inline>),
    Superscript(Vec<Inline>),
    Subscript(Vec<Inline>),
    Code(Attr, String),
    Link(Attr, Vec<Inline>, String, String),
    Image(Attr, Vec<Inline>, String, String),
    Unsupported(String),
}

#[derive(Debug, Default, Clone)]
struct Attr {
    id: String,
    classes: Vec<String>,
    kvs: Vec<(String, String)>,
}

// ----- block-level walking ------------------------------------------------

fn blocks_from_doc(doc: &SyntaxNode) -> Vec<Block> {
    let mut out = Vec::new();
    for child in doc.children() {
        if let Some(b) = block_from(&child) {
            out.push(b);
        }
    }
    out
}

fn block_from(node: &SyntaxNode) -> Option<Block> {
    match node.kind() {
        SyntaxKind::PARAGRAPH => Some(Block::Para(coalesce_inlines(inlines_from(node)))),
        SyntaxKind::PLAIN => Some(Block::Plain(coalesce_inlines(inlines_from(node)))),
        SyntaxKind::HEADING => Some(heading_block(node)),
        SyntaxKind::BLOCK_QUOTE => Some(Block::BlockQuote(blockquote_blocks(node))),
        SyntaxKind::CODE_BLOCK => Some(code_block(node)),
        SyntaxKind::HORIZONTAL_RULE => Some(Block::HorizontalRule),
        SyntaxKind::LIST => Some(list_block(node)),
        SyntaxKind::BLANK_LINE => None,
        // Reference definitions don't appear in pandoc-native output (they
        // resolve into the link they define).
        SyntaxKind::REFERENCE_DEFINITION => None,
        other => Some(Block::Unsupported(format!("{other:?}"))),
    }
}

fn heading_block(node: &SyntaxNode) -> Block {
    let level = heading_level(node);
    let inlines = node
        .children()
        .find(|c| c.kind() == SyntaxKind::HEADING_CONTENT)
        .map(|c| coalesce_inlines(inlines_from(&c)))
        .unwrap_or_default();
    let text = inlines_to_plaintext(&inlines);
    let id = pandoc_slugify(&text);
    Block::Header(level, Attr::with_id(id), inlines)
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

fn blockquote_blocks(node: &SyntaxNode) -> Vec<Block> {
    let mut out = Vec::new();
    for child in node.children() {
        if let Some(b) = block_from(&child) {
            out.push(b);
        }
    }
    out
}

fn code_block(node: &SyntaxNode) -> Block {
    let lang = code_block_language(node);
    let mut content = String::new();
    for child in node.children() {
        if child.kind() == SyntaxKind::CODE_CONTENT {
            content.push_str(&child.text().to_string());
        }
    }
    // Pandoc strips the trailing newline that closes the block.
    while content.ends_with('\n') {
        content.pop();
    }
    let attr = if lang.is_empty() {
        Attr::default()
    } else {
        Attr {
            id: String::new(),
            classes: vec![lang],
            kvs: Vec::new(),
        }
    };
    Block::CodeBlock(attr, content)
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
                return n.text().to_string().trim().to_string();
            }
            NodeOrToken::Token(t) if t.kind() == SyntaxKind::CODE_LANGUAGE => {
                return t.text().trim().to_string();
            }
            _ => {}
        }
    }
    String::new()
}

fn list_block(node: &SyntaxNode) -> Block {
    let loose = is_loose_list(node);
    let items: Vec<Vec<Block>> = node
        .children()
        .filter(|c| c.kind() == SyntaxKind::LIST_ITEM)
        .map(|item| list_item_blocks(&item, loose))
        .collect();
    if list_is_ordered(node) {
        let (start, style, delim) = ordered_list_attrs(node);
        Block::OrderedList(start, style, delim, items)
    } else {
        Block::BulletList(items)
    }
}

fn list_is_ordered(node: &SyntaxNode) -> bool {
    let Some(item) = node.children().find(|c| c.kind() == SyntaxKind::LIST_ITEM) else {
        return false;
    };
    let marker = item
        .children_with_tokens()
        .filter_map(|el| el.into_token())
        .find(|t| t.kind() == SyntaxKind::LIST_MARKER)
        .map(|t| t.text().to_string())
        .unwrap_or_default();
    let trimmed = marker.trim();
    !trimmed.starts_with(['-', '+', '*'])
}

fn ordered_list_attrs(node: &SyntaxNode) -> (usize, &'static str, &'static str) {
    let item = node.children().find(|c| c.kind() == SyntaxKind::LIST_ITEM);
    let marker = item
        .as_ref()
        .and_then(|i| {
            i.children_with_tokens()
                .filter_map(|el| el.into_token())
                .find(|t| t.kind() == SyntaxKind::LIST_MARKER)
                .map(|t| t.text().to_string())
        })
        .unwrap_or_default();
    let trimmed = marker.trim();
    let digits: String = trimmed.chars().take_while(|c| c.is_ascii_digit()).collect();
    let start: usize = digits.parse().unwrap_or(1);
    // The seed corpus only exercises Decimal/Period. Broader styles (roman,
    // alpha, two-paren delim) are projector-gap follow-ups.
    let style = "Decimal";
    let delim = if trimmed.ends_with(')') {
        "OneParen"
    } else {
        "Period"
    };
    (start, style, delim)
}

fn list_item_blocks(item: &SyntaxNode, loose: bool) -> Vec<Block> {
    let mut out = Vec::new();
    for child in item.children() {
        match child.kind() {
            SyntaxKind::PLAIN => {
                let inlines = coalesce_inlines(inlines_from(&child));
                if loose {
                    out.push(Block::Para(inlines));
                } else {
                    out.push(Block::Plain(inlines));
                }
            }
            _ => {
                if let Some(b) = block_from(&child) {
                    out.push(b);
                }
            }
        }
    }
    out
}

fn is_loose_list(node: &SyntaxNode) -> bool {
    let mut prev_was_item = false;
    for child in node.children_with_tokens() {
        if let NodeOrToken::Node(n) = child {
            if n.kind() == SyntaxKind::LIST_ITEM {
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
    }
    for item in node
        .children()
        .filter(|c| c.kind() == SyntaxKind::LIST_ITEM)
    {
        if item.children().any(|c| c.kind() == SyntaxKind::PARAGRAPH) {
            return true;
        }
    }
    false
}

// ----- inline walking -----------------------------------------------------

fn inlines_from(parent: &SyntaxNode) -> Vec<Inline> {
    let mut out = Vec::new();
    for el in parent.children_with_tokens() {
        match el {
            NodeOrToken::Token(t) => push_token_inline(&t, &mut out),
            NodeOrToken::Node(n) => out.push(inline_from_node(&n)),
        }
    }
    // Trailing NEWLINE inside paragraphs/headings is structural. Strip a
    // single trailing SoftBreak so the inline list ends on Str/Space, matching
    // pandoc's "trim trailing line endings" rule.
    while matches!(out.last(), Some(Inline::SoftBreak)) {
        out.pop();
    }
    out
}

fn push_token_inline(
    t: &rowan::SyntaxToken<panache_parser::syntax::PanacheLanguage>,
    out: &mut Vec<Inline>,
) {
    match t.kind() {
        SyntaxKind::TEXT => push_text(t.text(), out),
        SyntaxKind::WHITESPACE => out.push(Inline::Space),
        SyntaxKind::NEWLINE => out.push(Inline::SoftBreak),
        SyntaxKind::HARD_LINE_BREAK => out.push(Inline::LineBreak),
        SyntaxKind::ESCAPED_CHAR => {
            // \x — keep just the escaped character as a Str
            let s: String = t.text().chars().skip(1).collect();
            out.push(Inline::Str(s));
        }
        SyntaxKind::NONBREAKING_SPACE => out.push(Inline::Str("\u{a0}".to_string())),
        // Skip structural tokens (markers, brackets, fence bytes) that don't
        // contribute to the inline stream.
        _ => {}
    }
}

fn push_text(text: &str, out: &mut Vec<Inline>) {
    let mut buf = String::new();
    for ch in text.chars() {
        if ch == ' ' || ch == '\t' {
            if !buf.is_empty() {
                out.push(Inline::Str(std::mem::take(&mut buf)));
            }
            out.push(Inline::Space);
        } else if ch == '\n' {
            if !buf.is_empty() {
                out.push(Inline::Str(std::mem::take(&mut buf)));
            }
            out.push(Inline::SoftBreak);
        } else {
            buf.push(ch);
        }
    }
    if !buf.is_empty() {
        out.push(Inline::Str(buf));
    }
}

fn inline_from_node(node: &SyntaxNode) -> Inline {
    match node.kind() {
        SyntaxKind::EMPHASIS => Inline::Emph(coalesce_inlines(inlines_from_marked(node))),
        SyntaxKind::STRONG => Inline::Strong(coalesce_inlines(inlines_from_marked(node))),
        SyntaxKind::STRIKEOUT => Inline::Strikeout(coalesce_inlines(inlines_from_marked(node))),
        SyntaxKind::SUPERSCRIPT => Inline::Superscript(coalesce_inlines(inlines_from_marked(node))),
        SyntaxKind::SUBSCRIPT => Inline::Subscript(coalesce_inlines(inlines_from_marked(node))),
        SyntaxKind::INLINE_CODE => {
            let content: String = node
                .children_with_tokens()
                .filter_map(|el| el.into_token())
                .filter(|t| t.kind() == SyntaxKind::INLINE_CODE_CONTENT)
                .map(|t| t.text().to_string())
                .collect();
            Inline::Code(Attr::default(), content)
        }
        SyntaxKind::LINK => link_inline(node),
        SyntaxKind::IMAGE_LINK => image_inline(node),
        SyntaxKind::AUTO_LINK => autolink_inline(node),
        other => Inline::Unsupported(format!("{other:?}")),
    }
}

/// Inlines from a wrapper (Emph/Strong/...) where the structural markers are
/// child *nodes* (e.g. EMPHASIS_MARKER) rather than child tokens. We descend
/// through such marker children but skip their bytes.
fn inlines_from_marked(parent: &SyntaxNode) -> Vec<Inline> {
    let mut out = Vec::new();
    for el in parent.children_with_tokens() {
        match el {
            NodeOrToken::Token(t) => match t.kind() {
                SyntaxKind::EMPHASIS_MARKER
                | SyntaxKind::STRONG_MARKER
                | SyntaxKind::STRIKEOUT_MARKER
                | SyntaxKind::SUPERSCRIPT_MARKER
                | SyntaxKind::SUBSCRIPT_MARKER
                | SyntaxKind::MARK_MARKER => {}
                _ => push_token_inline(&t, &mut out),
            },
            NodeOrToken::Node(n) => match n.kind() {
                SyntaxKind::EMPHASIS_MARKER
                | SyntaxKind::STRONG_MARKER
                | SyntaxKind::STRIKEOUT_MARKER
                | SyntaxKind::SUPERSCRIPT_MARKER
                | SyntaxKind::SUBSCRIPT_MARKER
                | SyntaxKind::MARK_MARKER => {}
                _ => out.push(inline_from_node(&n)),
            },
        }
    }
    out
}

fn link_inline(node: &SyntaxNode) -> Inline {
    let text_node = node.children().find(|c| c.kind() == SyntaxKind::LINK_TEXT);
    let dest_node = node.children().find(|c| c.kind() == SyntaxKind::LINK_DEST);
    let text = text_node
        .as_ref()
        .map(|n| coalesce_inlines(inlines_from(n)))
        .unwrap_or_default();
    let (url, title) = dest_node
        .as_ref()
        .map(parse_link_dest)
        .unwrap_or((String::new(), String::new()));
    Inline::Link(Attr::default(), text, url, title)
}

fn image_inline(node: &SyntaxNode) -> Inline {
    let alt_node = node.children().find(|c| c.kind() == SyntaxKind::IMAGE_ALT);
    let dest_node = node.children().find(|c| c.kind() == SyntaxKind::LINK_DEST);
    let alt = alt_node
        .as_ref()
        .map(|n| coalesce_inlines(inlines_from(n)))
        .unwrap_or_default();
    let (url, title) = dest_node
        .as_ref()
        .map(parse_link_dest)
        .unwrap_or((String::new(), String::new()));
    Inline::Image(Attr::default(), alt, url, title)
}

fn autolink_inline(node: &SyntaxNode) -> Inline {
    let mut url = String::new();
    for el in node.children_with_tokens() {
        if let NodeOrToken::Token(t) = el
            && t.kind() == SyntaxKind::TEXT
        {
            url.push_str(t.text());
        }
    }
    let attr = Attr {
        id: String::new(),
        classes: vec!["uri".to_string()],
        kvs: Vec::new(),
    };
    Inline::Link(attr, vec![Inline::Str(url.clone())], url, String::new())
}

fn parse_link_dest(node: &SyntaxNode) -> (String, String) {
    // LINK_DEST currently holds the raw bytes between `(` and `)`. Split on
    // whitespace into URL and optional quoted title.
    let raw = node.text().to_string();
    let trimmed = raw.trim();
    if let Some(idx) = trimmed.find([' ', '\t']) {
        let url = trimmed[..idx].to_string();
        let rest = trimmed[idx..].trim();
        let title = parse_dest_title(rest);
        (url, title)
    } else {
        (trimmed.to_string(), String::new())
    }
}

fn parse_dest_title(s: &str) -> String {
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return String::new();
    }
    let (open, close) = match bytes[0] {
        b'"' => (b'"', b'"'),
        b'\'' => (b'\'', b'\''),
        b'(' => (b'(', b')'),
        _ => return String::new(),
    };
    if !s.starts_with(open as char) {
        return String::new();
    }
    if let Some(end) = s[1..].rfind(close as char) {
        return s[1..1 + end].to_string();
    }
    String::new()
}

// ----- coalescing & helpers ----------------------------------------------

fn coalesce_inlines(input: Vec<Inline>) -> Vec<Inline> {
    let mut out: Vec<Inline> = Vec::with_capacity(input.len());
    for inline in input {
        if let Inline::Str(s) = inline {
            if let Some(Inline::Str(prev)) = out.last_mut() {
                prev.push_str(&s);
            } else {
                out.push(Inline::Str(s));
            }
        } else if let Inline::Space = inline {
            // Collapse runs of Space into a single Space; pandoc never emits
            // two consecutive Space tokens.
            if matches!(out.last(), Some(Inline::Space) | Some(Inline::SoftBreak)) {
                continue;
            }
            out.push(Inline::Space);
        } else if let Inline::SoftBreak = inline {
            // SoftBreak after Space: drop the trailing Space to match pandoc
            // (line-end whitespace is not preserved as Space).
            if matches!(out.last(), Some(Inline::Space)) {
                out.pop();
            }
            out.push(Inline::SoftBreak);
        } else {
            out.push(inline);
        }
    }
    // Trim leading Space/SoftBreak — pandoc does not emit leading whitespace
    // inside a paragraph.
    while matches!(out.first(), Some(Inline::Space) | Some(Inline::SoftBreak)) {
        out.remove(0);
    }
    while matches!(out.last(), Some(Inline::Space) | Some(Inline::SoftBreak)) {
        out.pop();
    }
    out
}

fn inlines_to_plaintext(inlines: &[Inline]) -> String {
    let mut s = String::new();
    for i in inlines {
        match i {
            Inline::Str(t) => s.push_str(t),
            Inline::Space | Inline::SoftBreak => s.push(' '),
            Inline::LineBreak => s.push(' '),
            Inline::Emph(children)
            | Inline::Strong(children)
            | Inline::Strikeout(children)
            | Inline::Superscript(children)
            | Inline::Subscript(children) => s.push_str(&inlines_to_plaintext(children)),
            Inline::Code(_, c) => s.push_str(c),
            Inline::Link(_, alt, _, _) | Inline::Image(_, alt, _, _) => {
                s.push_str(&inlines_to_plaintext(alt))
            }
            Inline::Unsupported(_) => {}
        }
    }
    s
}

fn pandoc_slugify(text: &str) -> String {
    // Mirror crates/panache-formatter::utils::pandoc_slugify so the parser-side
    // projector doesn't need to depend on the formatter crate.
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in text.chars() {
        if ch.is_whitespace() {
            if !out.is_empty() && !prev_dash {
                out.push('-');
                prev_dash = true;
            }
            continue;
        }
        for lc in ch.to_lowercase() {
            if lc.is_alphanumeric() || lc == '_' || lc == '-' || lc == '.' {
                out.push(lc);
                prev_dash = lc == '-';
            }
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

impl Attr {
    fn with_id(id: String) -> Self {
        Self {
            id,
            classes: Vec::new(),
            kvs: Vec::new(),
        }
    }
}

// ----- text emission ------------------------------------------------------

fn write_block(b: &Block, out: &mut String) {
    match b {
        Block::Para(inlines) => {
            out.push_str("Para [");
            write_inline_list(inlines, out);
            out.push_str(" ]");
        }
        Block::Plain(inlines) => {
            out.push_str("Plain [");
            write_inline_list(inlines, out);
            out.push_str(" ]");
        }
        Block::Header(level, attr, inlines) => {
            out.push_str(&format!("Header {level} ("));
            write_attr(attr, out);
            out.push_str(") [");
            write_inline_list(inlines, out);
            out.push_str(" ]");
        }
        Block::BlockQuote(blocks) => {
            out.push_str("BlockQuote [");
            write_block_list(blocks, out);
            out.push_str(" ]");
        }
        Block::CodeBlock(attr, content) => {
            out.push_str("CodeBlock (");
            write_attr(attr, out);
            out.push_str(") ");
            write_haskell_string(content, out);
        }
        Block::HorizontalRule => out.push_str("HorizontalRule"),
        Block::BulletList(items) => {
            out.push_str("BulletList [");
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(" [");
                write_block_list(item, out);
                out.push_str(" ]");
            }
            out.push_str(" ]");
        }
        Block::OrderedList(start, style, delim, items) => {
            out.push_str(&format!("OrderedList ( {start} , {style} , {delim} ) ["));
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(" [");
                write_block_list(item, out);
                out.push_str(" ]");
            }
            out.push_str(" ]");
        }
        Block::Unsupported(name) => {
            out.push_str(&format!("Unsupported {name:?}"));
        }
    }
}

fn write_block_list(blocks: &[Block], out: &mut String) {
    for (i, b) in blocks.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push(' ');
        write_block(b, out);
    }
}

fn write_inline_list(inlines: &[Inline], out: &mut String) {
    for (i, inline) in inlines.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push(' ');
        write_inline(inline, out);
    }
}

fn write_inline(inline: &Inline, out: &mut String) {
    match inline {
        Inline::Str(s) => {
            out.push_str("Str ");
            write_haskell_string(s, out);
        }
        Inline::Space => out.push_str("Space"),
        Inline::SoftBreak => out.push_str("SoftBreak"),
        Inline::LineBreak => out.push_str("LineBreak"),
        Inline::Emph(children) => {
            out.push_str("Emph [");
            write_inline_list(children, out);
            out.push_str(" ]");
        }
        Inline::Strong(children) => {
            out.push_str("Strong [");
            write_inline_list(children, out);
            out.push_str(" ]");
        }
        Inline::Strikeout(children) => {
            out.push_str("Strikeout [");
            write_inline_list(children, out);
            out.push_str(" ]");
        }
        Inline::Superscript(children) => {
            out.push_str("Superscript [");
            write_inline_list(children, out);
            out.push_str(" ]");
        }
        Inline::Subscript(children) => {
            out.push_str("Subscript [");
            write_inline_list(children, out);
            out.push_str(" ]");
        }
        Inline::Code(attr, content) => {
            out.push_str("Code (");
            write_attr(attr, out);
            out.push_str(") ");
            write_haskell_string(content, out);
        }
        Inline::Link(attr, text, url, title) => {
            out.push_str("Link (");
            write_attr(attr, out);
            out.push_str(") [");
            write_inline_list(text, out);
            out.push_str(" ] ( ");
            write_haskell_string(url, out);
            out.push_str(" , ");
            write_haskell_string(title, out);
            out.push_str(" )");
        }
        Inline::Image(attr, alt, url, title) => {
            out.push_str("Image (");
            write_attr(attr, out);
            out.push_str(") [");
            write_inline_list(alt, out);
            out.push_str(" ] ( ");
            write_haskell_string(url, out);
            out.push_str(" , ");
            write_haskell_string(title, out);
            out.push_str(" )");
        }
        Inline::Unsupported(name) => {
            out.push_str(&format!("Unsupported {name:?}"));
        }
    }
}

fn write_attr(attr: &Attr, out: &mut String) {
    out.push(' ');
    write_haskell_string(&attr.id, out);
    out.push_str(" , [");
    for (i, c) in attr.classes.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push(' ');
        write_haskell_string(c, out);
    }
    if !attr.classes.is_empty() {
        out.push(' ');
    }
    out.push_str("] , [");
    for (i, (k, v)) in attr.kvs.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(" ( ");
        write_haskell_string(k, out);
        out.push_str(" , ");
        write_haskell_string(v, out);
        out.push_str(" )");
    }
    if !attr.kvs.is_empty() {
        out.push(' ');
    }
    out.push_str("] ");
}

fn write_haskell_string(s: &str, out: &mut String) {
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            _ => out.push(ch),
        }
    }
    out.push('"');
}

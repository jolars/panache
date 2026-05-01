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

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use panache_parser::SyntaxNode;
use panache_parser::syntax::SyntaxKind;
use rowan::NodeOrToken;

#[derive(Default)]
struct RefsCtx {
    refs: HashMap<String, (String, String)>,
    heading_ids: HashSet<String>,
    /// Footnote label → parsed body blocks. Lookup keyed by the raw label
    /// id text (no normalization needed — pandoc footnote labels are
    /// case-sensitive and not whitespace-collapsed).
    footnotes: HashMap<String, Vec<Block>>,
}

thread_local! {
    static REFS_CTX: RefCell<RefsCtx> = RefCell::new(RefsCtx::default());
}

pub fn project(tree: &SyntaxNode) -> String {
    let ctx = build_refs_ctx(tree);
    REFS_CTX.with(|c| *c.borrow_mut() = ctx);
    let mut blocks = blocks_from_doc(tree);
    fixup_empty_heading_ids(&mut blocks);
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
    REFS_CTX.with(|c| *c.borrow_mut() = RefsCtx::default());
    out
}

fn build_refs_ctx(tree: &SyntaxNode) -> RefsCtx {
    let mut ctx = RefsCtx::default();
    collect_refs_and_headings(tree, &mut ctx);
    ctx
}

fn collect_refs_and_headings(node: &SyntaxNode, ctx: &mut RefsCtx) {
    for child in node.children() {
        match child.kind() {
            SyntaxKind::REFERENCE_DEFINITION => {
                if let Some((label, url, title)) = parse_reference_def(&child) {
                    ctx.refs
                        .entry(normalize_ref_label(&label))
                        .or_insert((url, title));
                }
            }
            SyntaxKind::FOOTNOTE_DEFINITION => {
                if let Some((label, blocks)) = parse_footnote_def(&child) {
                    ctx.footnotes.entry(label).or_insert(blocks);
                }
            }
            SyntaxKind::HEADING => {
                let id = heading_implicit_id(&child);
                if !id.is_empty() {
                    ctx.heading_ids.insert(id);
                }
                collect_refs_and_headings(&child, ctx);
            }
            _ => collect_refs_and_headings(&child, ctx),
        }
    }
}

fn parse_footnote_def(node: &SyntaxNode) -> Option<(String, Vec<Block>)> {
    let label = footnote_label(node)?;
    let mut blocks = Vec::new();
    for child in node.children() {
        // The CST keeps each footnote-body line at its full raw indentation
        // (the 4-space body indent plus any nested-block indent). Most blocks
        // recover transparently because `coalesce_inlines` trims leading
        // spaces on paragraph content, but indented code blocks preserve all
        // leading whitespace — strip the 4 footnote-body spaces in addition
        // to the code block's own 4.
        if child.kind() == SyntaxKind::CODE_BLOCK
            && !child
                .children()
                .any(|c| c.kind() == SyntaxKind::CODE_FENCE_OPEN)
        {
            blocks.push(indented_code_block_with_extra_strip(&child, 4));
        } else if let Some(b) = block_from(&child) {
            blocks.push(b);
        }
    }
    Some((label, blocks))
}

fn indented_code_block_with_extra_strip(node: &SyntaxNode, extra: usize) -> Block {
    let attr = code_block_attr(node);
    let mut content = String::new();
    for child in node.children() {
        if child.kind() == SyntaxKind::CODE_CONTENT {
            content.push_str(&child.text().to_string());
        }
    }
    while content.ends_with('\n') {
        content.pop();
    }
    content = strip_leading_spaces_per_line(&content, extra);
    content = strip_indented_code_indent(&content);
    Block::CodeBlock(attr, content)
}

fn strip_leading_spaces_per_line(s: &str, n: usize) -> String {
    let mut out = String::with_capacity(s.len());
    for (i, line) in s.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let to_strip = line.chars().take(n).take_while(|&c| c == ' ').count();
        out.push_str(&line[to_strip..]);
    }
    out
}

fn footnote_label(node: &SyntaxNode) -> Option<String> {
    for el in node.children_with_tokens() {
        if let NodeOrToken::Token(t) = el
            && t.kind() == SyntaxKind::FOOTNOTE_LABEL_ID
        {
            return Some(t.text().to_string());
        }
    }
    None
}

fn parse_reference_def(node: &SyntaxNode) -> Option<(String, String, String)> {
    let link = node.children().find(|c| c.kind() == SyntaxKind::LINK)?;
    let label_node = link
        .children()
        .find(|c| c.kind() == SyntaxKind::LINK_TEXT)?;
    let label = label_node.text().to_string();

    let mut tail = String::new();
    let mut after_link = false;
    for el in node.children_with_tokens() {
        if after_link {
            match el {
                NodeOrToken::Token(t) => tail.push_str(t.text()),
                NodeOrToken::Node(n) => tail.push_str(&n.text().to_string()),
            }
        } else if let NodeOrToken::Node(n) = &el
            && n.kind() == SyntaxKind::LINK
        {
            after_link = true;
        }
    }

    let trimmed = tail.trim_start();
    let rest = trimmed.strip_prefix(':')?;
    let after_colon = rest.trim_start();
    let (url, after_url) = parse_ref_url(after_colon);
    let title = parse_dest_title(after_url.trim());
    Some((unescape_label(&label), url, title))
}

fn parse_ref_url(s: &str) -> (String, &str) {
    let s = s.trim_start();
    if let Some(rest) = s.strip_prefix('<')
        && let Some(end) = rest.find('>')
    {
        return (rest[..end].to_string(), &rest[end + 1..]);
    }
    let end = s.find(|c: char| c.is_whitespace()).unwrap_or(s.len());
    (s[..end].to_string(), &s[end..])
}

fn unescape_label(label: &str) -> String {
    let mut out = String::with_capacity(label.len());
    let mut chars = label.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\'
            && let Some(&next) = chars.peek()
            && is_ascii_punct(next)
        {
            out.push(next);
            chars.next();
        } else {
            out.push(ch);
        }
    }
    out
}

fn is_ascii_punct(c: char) -> bool {
    c.is_ascii() && (c.is_ascii_punctuation())
}

/// Pandoc/CommonMark reference-label normalization: case-fold and collapse
/// runs of whitespace to a single space, with leading/trailing trimmed.
fn normalize_ref_label(label: &str) -> String {
    let unescaped = unescape_label(label);
    let mut out = String::new();
    let mut last_space = false;
    for ch in unescaped.chars() {
        if ch.is_whitespace() {
            if !out.is_empty() && !last_space {
                out.push(' ');
                last_space = true;
            }
        } else {
            for lc in ch.to_lowercase() {
                out.push(lc);
            }
            last_space = false;
        }
    }
    if last_space {
        out.pop();
    }
    out
}

fn heading_implicit_id(node: &SyntaxNode) -> String {
    let inlines = node
        .children()
        .find(|c| c.kind() == SyntaxKind::HEADING_CONTENT)
        .map(|c| coalesce_inlines(inlines_from(&c)))
        .unwrap_or_default();
    let attr = node.children_with_tokens().find_map(|el| match el {
        NodeOrToken::Node(n) if n.kind() == SyntaxKind::ATTRIBUTE => Some(n.text().to_string()),
        NodeOrToken::Token(t) if t.kind() == SyntaxKind::ATTRIBUTE => Some(t.text().to_string()),
        _ => None,
    });
    if let Some(raw) = attr {
        let trimmed = raw.trim();
        if let Some(inner) = trimmed.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
            let parsed = parse_attr_block(inner);
            if !parsed.id.is_empty() {
                return parsed.id;
            }
        }
    }
    pandoc_slugify(&inlines_to_plaintext(&inlines))
}

fn lookup_ref(label: &str) -> Option<(String, String)> {
    let key = normalize_ref_label(label);
    REFS_CTX.with(|c| c.borrow().refs.get(&key).cloned())
}

fn lookup_heading_id(label: &str) -> Option<String> {
    let id = pandoc_slugify(&unescape_label(label));
    if id.is_empty() {
        return None;
    }
    REFS_CTX.with(|c| {
        if c.borrow().heading_ids.contains(&id) {
            Some(id)
        } else {
            None
        }
    })
}

/// Pandoc auto-numbers consecutive empty headings as `section`, `section-1`,
/// `section-2`, ... — our slugifier returns `""` for empty input, so fix the
/// IDs up after the fact at document level.
fn fixup_empty_heading_ids(blocks: &mut [Block]) {
    let mut count = 0u32;
    for block in blocks.iter_mut() {
        if let Block::Header(_, attr, inlines) = block
            && inlines.is_empty()
            && attr.id.is_empty()
        {
            attr.id = if count == 0 {
                "section".to_string()
            } else {
                format!("section-{count}")
            };
            count += 1;
        }
    }
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
    RawBlock(String, String),
    Table(TableData),
    Div(Attr, Vec<Block>),
    LineBlock(Vec<Vec<Inline>>),
    DefinitionList(Vec<(Vec<Inline>, Vec<Vec<Block>>)>),
    Unsupported(String),
}

#[derive(Debug)]
struct TableData {
    caption: Vec<Inline>,
    aligns: Vec<&'static str>,
    head_rows: Vec<Vec<Vec<Block>>>,
    body_rows: Vec<Vec<Vec<Block>>>,
}

#[derive(Debug)]
#[allow(clippy::enum_variant_names)]
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
    Math(&'static str, String),
    Span(Attr, Vec<Inline>),
    RawInline(String, String),
    Quoted(&'static str, Vec<Inline>),
    Note(Vec<Block>),
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
        // Footnote definitions are pulled into Note inlines at the
        // FOOTNOTE_REFERENCE site; the definition block itself is dropped.
        SyntaxKind::FOOTNOTE_DEFINITION => None,
        // YAML metadata becomes the document Meta wrapper, not a body block.
        // The projector emits a bare block list, so just drop these.
        SyntaxKind::YAML_METADATA => None,
        SyntaxKind::HTML_BLOCK => Some(html_block(node)),
        SyntaxKind::PIPE_TABLE => pipe_table(node).map(Block::Table),
        SyntaxKind::TEX_BLOCK => Some(tex_block(node)),
        SyntaxKind::FENCED_DIV => Some(fenced_div(node)),
        SyntaxKind::LINE_BLOCK => Some(line_block(node)),
        SyntaxKind::DEFINITION_LIST => Some(definition_list(node)),
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
    let attr = node
        .children_with_tokens()
        .find_map(|el| match el {
            NodeOrToken::Node(n) if n.kind() == SyntaxKind::ATTRIBUTE => Some(n.text().to_string()),
            NodeOrToken::Token(t) if t.kind() == SyntaxKind::ATTRIBUTE => {
                Some(t.text().to_string())
            }
            _ => None,
        })
        .map(|raw| {
            let trimmed = raw.trim();
            if let Some(inner) = trimmed.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
                let mut attr = parse_attr_block(inner);
                if attr.id.is_empty() {
                    attr.id = pandoc_slugify(&inlines_to_plaintext(&inlines));
                }
                attr
            } else {
                Attr::with_id(pandoc_slugify(&inlines_to_plaintext(&inlines)))
            }
        })
        .unwrap_or_else(|| Attr::with_id(pandoc_slugify(&inlines_to_plaintext(&inlines))));
    Block::Header(level, attr, inlines)
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
    let attr = code_block_attr(node);
    let is_fenced = node
        .children()
        .any(|c| c.kind() == SyntaxKind::CODE_FENCE_OPEN);
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
    if !is_fenced {
        content = strip_indented_code_indent(&content);
    }
    Block::CodeBlock(attr, content)
}

fn code_block_attr(node: &SyntaxNode) -> Attr {
    let Some(open) = node
        .children()
        .find(|c| c.kind() == SyntaxKind::CODE_FENCE_OPEN)
    else {
        return Attr::default();
    };
    let Some(info) = open.children().find(|c| c.kind() == SyntaxKind::CODE_INFO) else {
        return Attr::default();
    };
    let raw = info.text().to_string();
    let trimmed = raw.trim();
    if let Some(inner) = trimmed.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
        return parse_attr_block(inner);
    }
    // Shortcut form: `lang {.cls #id key=value}` — language followed by an
    // attribute block. Pandoc concatenates the language as the first class.
    if let Some(brace) = trimmed.find('{')
        && trimmed.ends_with('}')
    {
        let lang = trimmed[..brace].trim();
        let attr_inner = &trimmed[brace + 1..trimmed.len() - 1];
        let mut attr = parse_attr_block(attr_inner);
        if !lang.is_empty() {
            attr.classes.insert(0, lang.to_string());
        }
        return attr;
    }
    if !trimmed.is_empty() {
        return Attr {
            id: String::new(),
            classes: vec![trimmed.to_string()],
            kvs: Vec::new(),
        };
    }
    Attr::default()
}

/// Pandoc strips up to four leading spaces (or one tab) from each line of an
/// indented code block. The CST keeps the indent as part of CODE_CONTENT, so
/// we remove it here.
fn strip_indented_code_indent(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for (i, line) in s.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let stripped = if let Some(rest) = line.strip_prefix("    ") {
            rest
        } else if let Some(rest) = line.strip_prefix('\t') {
            rest
        } else {
            // Strip up to 3 leading spaces if present (pandoc tolerates short
            // indentation only on blank lines, which we don't try to detect
            // here — safer to leave non-conforming lines alone).
            line
        };
        out.push_str(stripped);
    }
    out
}

fn html_block(node: &SyntaxNode) -> Block {
    let mut content = node.text().to_string();
    while content.ends_with('\n') {
        content.pop();
    }
    Block::RawBlock("html".to_string(), content)
}

fn tex_block(node: &SyntaxNode) -> Block {
    let mut content = node.text().to_string();
    while content.ends_with('\n') {
        content.pop();
    }
    Block::RawBlock("tex".to_string(), content)
}

fn fenced_div(node: &SyntaxNode) -> Block {
    let attr = node
        .children()
        .find(|c| c.kind() == SyntaxKind::DIV_FENCE_OPEN)
        .map(|open| {
            let info = open
                .children()
                .find(|c| c.kind() == SyntaxKind::DIV_INFO)
                .map(|n| n.text().to_string())
                .unwrap_or_default();
            parse_div_info(info.trim())
        })
        .unwrap_or_default();
    let mut blocks = Vec::new();
    for child in node.children() {
        match child.kind() {
            SyntaxKind::DIV_FENCE_OPEN | SyntaxKind::DIV_FENCE_CLOSE => {}
            _ => {
                if let Some(b) = block_from(&child) {
                    blocks.push(b);
                }
            }
        }
    }
    Block::Div(attr, blocks)
}

/// Parse pandoc div info: either `{#id .class1 .class2 key=value}` or a single
/// bare class name like `Warning`.
fn parse_div_info(info: &str) -> Attr {
    if info.starts_with('{') && info.ends_with('}') {
        return parse_attr_block(&info[1..info.len() - 1]);
    }
    if !info.is_empty() {
        return Attr {
            id: String::new(),
            classes: vec![info.to_string()],
            kvs: Vec::new(),
        };
    }
    Attr::default()
}

/// Parse the body of an attribute block like `#my-id .class1 .class2 key=value`.
/// Whitespace-separated. Tokens starting with `#` are id, `.` are classes,
/// `key=value` (optionally quoted value) are kvs.
fn parse_attr_block(s: &str) -> Attr {
    let mut id = String::new();
    let mut classes: Vec<String> = Vec::new();
    let mut kvs: Vec<(String, String)> = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b' ' | b'\t' | b'\n' | b'\r' => {
                i += 1;
            }
            b'#' => {
                let start = i + 1;
                let mut j = start;
                while j < bytes.len() && !matches!(bytes[j], b' ' | b'\t' | b'\n' | b'\r') {
                    j += 1;
                }
                id = s[start..j].to_string();
                i = j;
            }
            b'.' => {
                let start = i + 1;
                let mut j = start;
                while j < bytes.len() && !matches!(bytes[j], b' ' | b'\t' | b'\n' | b'\r') {
                    j += 1;
                }
                classes.push(s[start..j].to_string());
                i = j;
            }
            _ => {
                // Read key up to `=` or whitespace.
                let key_start = i;
                while i < bytes.len() && !matches!(bytes[i], b' ' | b'\t' | b'\n' | b'\r' | b'=') {
                    i += 1;
                }
                let key = s[key_start..i].to_string();
                if i < bytes.len() && bytes[i] == b'=' {
                    i += 1;
                    let value = if i < bytes.len() && bytes[i] == b'"' {
                        i += 1;
                        let v_start = i;
                        while i < bytes.len() && bytes[i] != b'"' {
                            i += 1;
                        }
                        let v = s[v_start..i].to_string();
                        if i < bytes.len() {
                            i += 1;
                        }
                        v
                    } else {
                        let v_start = i;
                        while i < bytes.len() && !matches!(bytes[i], b' ' | b'\t' | b'\n' | b'\r') {
                            i += 1;
                        }
                        s[v_start..i].to_string()
                    };
                    kvs.push((key, value));
                } else if !key.is_empty() {
                    // Bare token (legacy class form).
                    classes.push(key);
                }
            }
        }
    }
    Attr { id, classes, kvs }
}

fn definition_list(node: &SyntaxNode) -> Block {
    let items: Vec<(Vec<Inline>, Vec<Vec<Block>>)> = node
        .children()
        .filter(|c| c.kind() == SyntaxKind::DEFINITION_ITEM)
        .map(|item| {
            let term = item
                .children()
                .find(|c| c.kind() == SyntaxKind::TERM)
                .map(|t| coalesce_inlines(inlines_from(&t)))
                .unwrap_or_default();
            let defs: Vec<Vec<Block>> = item
                .children()
                .filter(|c| c.kind() == SyntaxKind::DEFINITION)
                .map(|d| definition_blocks(&d))
                .collect();
            (term, defs)
        })
        .collect();
    Block::DefinitionList(items)
}

fn definition_blocks(def_node: &SyntaxNode) -> Vec<Block> {
    let mut out = Vec::new();
    for child in def_node.children() {
        match child.kind() {
            SyntaxKind::PLAIN => {
                out.push(Block::Plain(coalesce_inlines(inlines_from(&child))));
            }
            SyntaxKind::PARAGRAPH => {
                out.push(Block::Para(coalesce_inlines(inlines_from(&child))));
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

fn line_block(node: &SyntaxNode) -> Block {
    let lines: Vec<Vec<Inline>> = node
        .children()
        .filter(|c| c.kind() == SyntaxKind::LINE_BLOCK_LINE)
        .map(|line| {
            let mut out = Vec::new();
            for el in line.children_with_tokens() {
                match el {
                    NodeOrToken::Token(t) => match t.kind() {
                        SyntaxKind::LINE_BLOCK_MARKER | SyntaxKind::NEWLINE => {}
                        _ => push_token_inline(&t, &mut out),
                    },
                    NodeOrToken::Node(n) => out.push(inline_from_node(&n)),
                }
            }
            coalesce_inlines(out)
        })
        .collect();
    Block::LineBlock(lines)
}

fn latex_command_inline(node: &SyntaxNode) -> Inline {
    let content = node.text().to_string();
    Inline::RawInline("tex".to_string(), content)
}

fn bracketed_span_inline(node: &SyntaxNode) -> Inline {
    // Detect HTML-style `<span>...</span>` (parser quirk) — for now, leave as
    // Unsupported so the report still flags it; pandoc would emit RawInline.
    let is_html = node
        .children_with_tokens()
        .any(|el| matches!(&el, NodeOrToken::Token(t) if t.kind() == SyntaxKind::SPAN_BRACKET_OPEN && t.text().starts_with('<')));
    if is_html {
        return Inline::Unsupported("BRACKETED_SPAN".to_string());
    }
    let attr = node
        .children()
        .find(|c| c.kind() == SyntaxKind::SPAN_ATTRIBUTES)
        .map(|n| {
            let raw = n.text().to_string();
            let trimmed = raw.trim();
            if let Some(inner) = trimmed.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
                parse_attr_block(inner)
            } else {
                Attr::default()
            }
        })
        .unwrap_or_default();
    let content = node
        .children()
        .find(|c| c.kind() == SyntaxKind::SPAN_CONTENT)
        .map(|n| coalesce_inlines(inlines_from(&n)))
        .unwrap_or_default();
    Inline::Span(attr, content)
}

fn pipe_table(node: &SyntaxNode) -> Option<TableData> {
    let mut header_cells: Vec<Vec<Inline>> = Vec::new();
    let mut body_rows: Vec<Vec<Vec<Inline>>> = Vec::new();
    let mut aligns: Vec<&'static str> = Vec::new();
    let mut caption_inlines: Vec<Inline> = Vec::new();
    for child in node.children() {
        match child.kind() {
            SyntaxKind::TABLE_HEADER => {
                header_cells = pipe_table_cells(&child);
            }
            SyntaxKind::TABLE_SEPARATOR => {
                let raw = child.text().to_string();
                aligns = pipe_separator_aligns(&raw);
            }
            SyntaxKind::TABLE_ROW => {
                body_rows.push(pipe_table_cells(&child));
            }
            SyntaxKind::TABLE_CAPTION => {
                caption_inlines = pipe_table_caption(&child);
            }
            _ => {}
        }
    }
    let cols = header_cells
        .len()
        .max(body_rows.iter().map(Vec::len).max().unwrap_or(0))
        .max(aligns.len());
    if cols == 0 {
        return None;
    }
    while aligns.len() < cols {
        aligns.push("AlignDefault");
    }
    let head_rows = if header_cells.is_empty() {
        Vec::new()
    } else {
        vec![cells_to_plain_blocks(header_cells, cols)]
    };
    let body_rows: Vec<Vec<Vec<Block>>> = body_rows
        .into_iter()
        .map(|cells| cells_to_plain_blocks(cells, cols))
        .collect();
    Some(TableData {
        caption: caption_inlines,
        aligns,
        head_rows,
        body_rows,
    })
}

fn pipe_table_cells(row: &SyntaxNode) -> Vec<Vec<Inline>> {
    row.children()
        .filter(|c| c.kind() == SyntaxKind::TABLE_CELL)
        .map(|cell| coalesce_inlines(inlines_from(&cell)))
        .collect()
}

fn pipe_table_caption(node: &SyntaxNode) -> Vec<Inline> {
    // Walk all tokens after TABLE_CAPTION_PREFIX and collect inline content.
    let mut out = Vec::new();
    let mut after_prefix = false;
    for el in node.children_with_tokens() {
        match el {
            NodeOrToken::Node(n) => {
                if n.kind() == SyntaxKind::TABLE_CAPTION_PREFIX {
                    after_prefix = true;
                    continue;
                }
                if after_prefix {
                    out.push(inline_from_node(&n));
                }
            }
            NodeOrToken::Token(t) => {
                if t.kind() == SyntaxKind::TABLE_CAPTION_PREFIX {
                    after_prefix = true;
                    continue;
                }
                if after_prefix {
                    push_token_inline(&t, &mut out);
                }
            }
        }
    }
    coalesce_inlines(out)
}

fn pipe_separator_aligns(raw: &str) -> Vec<&'static str> {
    let trimmed = raw.trim_matches(|c: char| c == '\n' || c == '\r');
    let inner = trimmed.trim_start_matches('|').trim_end_matches('|');
    inner
        .split('|')
        .map(|seg| {
            let s = seg.trim();
            let left = s.starts_with(':');
            let right = s.ends_with(':');
            match (left, right) {
                (true, true) => "AlignCenter",
                (true, false) => "AlignLeft",
                (false, true) => "AlignRight",
                _ => "AlignDefault",
            }
        })
        .collect()
}

fn cells_to_plain_blocks(cells: Vec<Vec<Inline>>, cols: usize) -> Vec<Vec<Block>> {
    let mut out: Vec<Vec<Block>> = cells
        .into_iter()
        .map(|inlines| {
            if inlines.is_empty() {
                Vec::new()
            } else {
                vec![Block::Plain(inlines)]
            }
        })
        .collect();
    while out.len() < cols {
        out.push(Vec::new());
    }
    out
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
            NodeOrToken::Node(n) => push_inline_node(&n, &mut out),
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

fn push_inline_node(node: &SyntaxNode, out: &mut Vec<Inline>) {
    match node.kind() {
        SyntaxKind::LINK => render_link_inline(node, out),
        SyntaxKind::IMAGE_LINK => render_image_inline(node, out),
        _ => out.push(inline_from_node(node)),
    }
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
            Inline::Code(Attr::default(), strip_inline_code_padding(&content))
        }
        SyntaxKind::LINK | SyntaxKind::IMAGE_LINK => {
            // LINK / IMAGE_LINK render through `push_inline_node` so reference
            // resolution can emit multiple inlines (resolved Link, or unresolved
            // Str fragments). This single-Inline path is unreachable; emit
            // Unsupported as a guard rather than silently dropping.
            Inline::Unsupported(format!("{:?}", node.kind()))
        }
        SyntaxKind::AUTO_LINK => autolink_inline(node),
        SyntaxKind::INLINE_MATH => math_inline(node, "InlineMath"),
        SyntaxKind::DISPLAY_MATH => math_inline(node, "DisplayMath"),
        SyntaxKind::LATEX_COMMAND => latex_command_inline(node),
        SyntaxKind::BRACKETED_SPAN => bracketed_span_inline(node),
        SyntaxKind::INLINE_HTML => Inline::RawInline("html".to_string(), node.text().to_string()),
        SyntaxKind::FOOTNOTE_REFERENCE => footnote_reference_inline(node),
        SyntaxKind::INLINE_FOOTNOTE => inline_footnote_inline(node),
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
                _ => push_inline_node(&n, &mut out),
            },
        }
    }
    out
}

fn render_link_inline(node: &SyntaxNode, out: &mut Vec<Inline>) {
    let text_node = node.children().find(|c| c.kind() == SyntaxKind::LINK_TEXT);
    let dest_node = node.children().find(|c| c.kind() == SyntaxKind::LINK_DEST);
    let has_dest_paren = node
        .children_with_tokens()
        .any(|el| matches!(el, NodeOrToken::Token(t) if t.kind() == SyntaxKind::LINK_DEST_START));

    if has_dest_paren {
        let text = text_node
            .as_ref()
            .map(|n| coalesce_inlines(inlines_from(n)))
            .unwrap_or_default();
        let (url, title) = dest_node
            .as_ref()
            .map(parse_link_dest)
            .unwrap_or((String::new(), String::new()));
        out.push(Inline::Link(Attr::default(), text, url, title));
        return;
    }

    // Reference-style link: shortcut [label], implicit [label][], or full
    // [text][ref]. Distinguish by presence/contents of LINK_REF.
    let ref_node = node.children().find(|c| c.kind() == SyntaxKind::LINK_REF);
    let text_inlines = text_node
        .as_ref()
        .map(|n| coalesce_inlines(inlines_from(n)))
        .unwrap_or_default();
    let text_label = text_node
        .as_ref()
        .map(|n| n.text().to_string())
        .unwrap_or_default();

    let (label, has_second_brackets, second_inner) = match ref_node.as_ref() {
        Some(rn) => {
            let inner = rn.text().to_string();
            if inner.is_empty() {
                (text_label.clone(), true, String::new())
            } else {
                (inner.clone(), true, inner)
            }
        }
        None => (text_label.clone(), false, String::new()),
    };

    if let Some((url, title)) = lookup_ref(&label) {
        out.push(Inline::Link(Attr::default(), text_inlines, url, title));
        return;
    }

    if let Some(id) = lookup_heading_id(&label) {
        let url = format!("#{id}");
        out.push(Inline::Link(
            Attr::default(),
            text_inlines,
            url,
            String::new(),
        ));
        return;
    }

    // Unresolved: emit the original markdown bytes as plain text. The reader
    // assembles `[<text>]`, optionally followed by `[<ref>]` for a full or
    // implicit reference. Using Str inlines here (rather than Link with empty
    // dest) matches pandoc's behavior of leaving unresolved references as raw
    // text in the output stream.
    out.push(Inline::Str("[".to_string()));
    out.extend(text_inlines);
    let suffix = if has_second_brackets {
        format!("][{second_inner}]")
    } else {
        "]".to_string()
    };
    out.push(Inline::Str(suffix));
}

fn render_image_inline(node: &SyntaxNode, out: &mut Vec<Inline>) {
    let alt_node = node.children().find(|c| c.kind() == SyntaxKind::IMAGE_ALT);
    let dest_node = node.children().find(|c| c.kind() == SyntaxKind::LINK_DEST);
    let has_dest_paren = node.children_with_tokens().any(|el| {
        matches!(el, NodeOrToken::Token(t) if t.kind() == SyntaxKind::IMAGE_DEST_START
            || t.kind() == SyntaxKind::LINK_DEST_START)
    });

    if has_dest_paren {
        let alt = alt_node
            .as_ref()
            .map(|n| coalesce_inlines(inlines_from(n)))
            .unwrap_or_default();
        let (url, title) = dest_node
            .as_ref()
            .map(parse_link_dest)
            .unwrap_or((String::new(), String::new()));
        out.push(Inline::Image(Attr::default(), alt, url, title));
        return;
    }

    let ref_node = node.children().find(|c| c.kind() == SyntaxKind::LINK_REF);
    let alt_inlines = alt_node
        .as_ref()
        .map(|n| coalesce_inlines(inlines_from(n)))
        .unwrap_or_default();
    let alt_label = alt_node
        .as_ref()
        .map(|n| n.text().to_string())
        .unwrap_or_default();

    let (label, has_second_brackets, second_inner) = match ref_node.as_ref() {
        Some(rn) => {
            let inner = rn.text().to_string();
            if inner.is_empty() {
                (alt_label.clone(), true, String::new())
            } else {
                (inner.clone(), true, inner)
            }
        }
        None => (alt_label.clone(), false, String::new()),
    };

    if let Some((url, title)) = lookup_ref(&label) {
        out.push(Inline::Image(Attr::default(), alt_inlines, url, title));
        return;
    }

    if let Some(id) = lookup_heading_id(&label) {
        let url = format!("#{id}");
        out.push(Inline::Image(
            Attr::default(),
            alt_inlines,
            url,
            String::new(),
        ));
        return;
    }

    out.push(Inline::Str("![".to_string()));
    out.extend(alt_inlines);
    let suffix = if has_second_brackets {
        format!("][{second_inner}]")
    } else {
        "]".to_string()
    };
    out.push(Inline::Str(suffix));
}

/// Pandoc strips a single space at the start and end of inline code if the
/// content has at least one non-space char (preserving the user's intent to
/// disambiguate the leading backtick when the code starts with a backtick).
/// `` ` foo ` `` → `Code "foo"`; `` ` ` `` (single space) → `Code " "`.
fn strip_inline_code_padding(s: &str) -> String {
    if s.len() >= 2 && s.starts_with(' ') && s.ends_with(' ') && s.chars().any(|c| c != ' ') {
        return s[1..s.len() - 1].to_string();
    }
    s.to_string()
}

fn math_inline(node: &SyntaxNode, kind: &'static str) -> Inline {
    let mut content = String::new();
    for el in node.children_with_tokens() {
        if let NodeOrToken::Token(t) = el {
            match t.kind() {
                SyntaxKind::INLINE_MATH_MARKER | SyntaxKind::DISPLAY_MATH_MARKER => {}
                _ => content.push_str(t.text()),
            }
        }
    }
    Inline::Math(kind, content)
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
    // Pandoc treats `<foo@bar>` as an email autolink (class "email", `mailto:`
    // dest) when the body has no scheme but contains an `@`.
    let is_email = !url.contains("://") && !url.starts_with("mailto:") && url.contains('@');
    if is_email {
        let attr = Attr {
            id: String::new(),
            classes: vec!["email".to_string()],
            kvs: Vec::new(),
        };
        let dest = format!("mailto:{url}");
        return Inline::Link(attr, vec![Inline::Str(url)], dest, String::new());
    }
    let attr = Attr {
        id: String::new(),
        classes: vec!["uri".to_string()],
        kvs: Vec::new(),
    };
    Inline::Link(attr, vec![Inline::Str(url.clone())], url, String::new())
}

fn footnote_reference_inline(node: &SyntaxNode) -> Inline {
    let Some(label) = footnote_label(node) else {
        return Inline::Unsupported("FOOTNOTE_REFERENCE".to_string());
    };
    let blocks = REFS_CTX.with(|c| {
        c.borrow()
            .footnotes
            .get(&label)
            .map(|bs| bs.iter().map(clone_block).collect::<Vec<_>>())
    });
    match blocks {
        Some(bs) => Inline::Note(bs),
        // Unresolved footnote reference: pandoc emits the original bytes as
        // text rather than a `Note []`. Keep the raw token text for now.
        None => Inline::Str(node.text().to_string()),
    }
}

fn inline_footnote_inline(node: &SyntaxNode) -> Inline {
    let inlines = coalesce_inlines(inlines_from(node));
    if inlines.is_empty() {
        Inline::Note(Vec::new())
    } else {
        Inline::Note(vec![Block::Para(inlines)])
    }
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
    // Pandoc's `smart` extension is on by default for markdown. Apply the
    // simple in-Str substitutions here (apostrophe, dashes, ellipsis), then
    // restructure paired straight quotes into `Quoted` nodes.
    for inline in out.iter_mut() {
        if let Inline::Str(s) = inline {
            let mut t = smart_intraword_apostrophe(s);
            t = smart_dashes_and_ellipsis(&t);
            *s = t;
        }
    }
    smart_quote_pairs(out)
}

fn smart_quote_pairs(inlines: Vec<Inline>) -> Vec<Inline> {
    // Walk left-to-right, when a Str starts with a straight quote and the
    // previous element is a "boundary" (None/Space/SoftBreak/LineBreak), look
    // ahead for a matching close quote (Str ending with same quote char,
    // followed by a boundary). Wrap the inlines in between in a `Quoted` node.
    // Only handle quotes at Str boundaries; embedded or interleaved quotes are
    // not restructured (kept as-is) — pandoc has more nuanced rules but this
    // covers the common natural-text patterns in the corpus.
    fn is_boundary(prev: Option<&Inline>) -> bool {
        match prev {
            None => true,
            Some(Inline::Space | Inline::SoftBreak | Inline::LineBreak) => true,
            Some(Inline::Str(s)) => s.chars().last().is_some_and(|c| !c.is_alphanumeric()),
            _ => false,
        }
    }
    let mut out: Vec<Inline> = Vec::with_capacity(inlines.len());
    let n = inlines.len();
    let mut consumed = vec![false; n];
    for i in 0..n {
        if consumed[i] {
            continue;
        }
        // Try to detect an open quote at position i.
        let Inline::Str(s) = &inlines[i] else {
            out.push(clone_inline(&inlines[i]));
            consumed[i] = true;
            continue;
        };
        let first = s.chars().next();
        let quote = match first {
            Some('"') => Some('"'),
            Some('\'') => Some('\''),
            _ => None,
        };
        // Open quote condition: previous inline is boundary, AND there is at
        // least one more char after the quote in this Str OR there is a next
        // inline that's not a boundary (so the quote is "leading").
        let prev_is_boundary = is_boundary(out.last());
        let str_has_more = s.chars().count() > 1;
        // For the simplest reliable heuristic: only treat as open if the very
        // next char after the quote is non-space (open quote attaches to a
        // word).
        let next_char_is_word = s.chars().nth(1).is_some_and(|c| !c.is_whitespace());
        if let Some(q) = quote
            && prev_is_boundary
            && str_has_more
            && next_char_is_word
        {
            // Find the matching close.
            if let Some(close_idx) = find_matching_close(&inlines, i, q, &consumed) {
                // Build content: inlines from i to close_idx (inclusive),
                // strip the leading quote from inlines[i] and trailing quote
                // from inlines[close_idx].
                let kind = if q == '"' {
                    "DoubleQuote"
                } else {
                    "SingleQuote"
                };
                let mut content: Vec<Inline> = Vec::new();
                for j in i..=close_idx {
                    if consumed[j] {
                        continue;
                    }
                    let inline = &inlines[j];
                    if j == i {
                        if let Inline::Str(s) = inline {
                            let stripped: String = s.chars().skip(1).collect();
                            if !stripped.is_empty() {
                                content.push(Inline::Str(stripped));
                            }
                        }
                    } else if j == close_idx {
                        if let Inline::Str(s) = inline {
                            let mut stripped: String = s.chars().collect();
                            stripped.pop();
                            if !stripped.is_empty() {
                                content.push(Inline::Str(stripped));
                            }
                        }
                    } else {
                        content.push(clone_inline(inline));
                    }
                    consumed[j] = true;
                }
                out.push(Inline::Quoted(kind, content));
                continue;
            }
        }
        out.push(clone_inline(&inlines[i]));
        consumed[i] = true;
    }
    out
}

fn find_matching_close(
    inlines: &[Inline],
    open_idx: usize,
    quote: char,
    consumed: &[bool],
) -> Option<usize> {
    // First check: same Str ends with the matching quote (close in same Str).
    if let Inline::Str(s) = &inlines[open_idx]
        && s.chars().count() >= 3
        && s.ends_with(quote)
    {
        // Need to confirm the next inline (after this Str) is a boundary.
        let next = inlines.get(open_idx + 1);
        let after_is_boundary = match next {
            None => true,
            Some(Inline::Space | Inline::SoftBreak | Inline::LineBreak) => true,
            Some(Inline::Str(s)) => s.chars().next().is_some_and(|c| !c.is_alphanumeric()),
            _ => false,
        };
        if after_is_boundary {
            return Some(open_idx);
        }
    }
    // Otherwise, scan forward for a Str ending with the quote and followed by
    // a boundary.
    let n = inlines.len();
    let mut j = open_idx + 1;
    while j < n {
        if consumed[j] {
            return None;
        }
        match &inlines[j] {
            Inline::Str(s) => {
                if s.ends_with(quote) {
                    let next = inlines.get(j + 1);
                    let after_is_boundary = match next {
                        None => true,
                        Some(Inline::Space | Inline::SoftBreak | Inline::LineBreak) => true,
                        Some(Inline::Str(s)) => {
                            s.chars().next().is_some_and(|c| !c.is_alphanumeric())
                        }
                        _ => false,
                    };
                    if after_is_boundary {
                        return Some(j);
                    }
                }
            }
            Inline::Space | Inline::SoftBreak | Inline::LineBreak => {}
            // Don't span over markup atoms — keep search cheap and predictable.
            _ => {}
        }
        j += 1;
        // Cap search range — natural quoted spans are short.
        if j - open_idx > 32 {
            return None;
        }
    }
    None
}

fn clone_inline(inline: &Inline) -> Inline {
    match inline {
        Inline::Str(s) => Inline::Str(s.clone()),
        Inline::Space => Inline::Space,
        Inline::SoftBreak => Inline::SoftBreak,
        Inline::LineBreak => Inline::LineBreak,
        Inline::Emph(c) => Inline::Emph(c.iter().map(clone_inline).collect()),
        Inline::Strong(c) => Inline::Strong(c.iter().map(clone_inline).collect()),
        Inline::Strikeout(c) => Inline::Strikeout(c.iter().map(clone_inline).collect()),
        Inline::Superscript(c) => Inline::Superscript(c.iter().map(clone_inline).collect()),
        Inline::Subscript(c) => Inline::Subscript(c.iter().map(clone_inline).collect()),
        Inline::Code(a, s) => Inline::Code(a.clone(), s.clone()),
        Inline::Link(a, t, u, ti) => Inline::Link(
            a.clone(),
            t.iter().map(clone_inline).collect(),
            u.clone(),
            ti.clone(),
        ),
        Inline::Image(a, t, u, ti) => Inline::Image(
            a.clone(),
            t.iter().map(clone_inline).collect(),
            u.clone(),
            ti.clone(),
        ),
        Inline::Math(k, c) => Inline::Math(k, c.clone()),
        Inline::Span(a, c) => Inline::Span(a.clone(), c.iter().map(clone_inline).collect()),
        Inline::RawInline(f, c) => Inline::RawInline(f.clone(), c.clone()),
        Inline::Quoted(k, c) => Inline::Quoted(k, c.iter().map(clone_inline).collect()),
        Inline::Note(blocks) => Inline::Note(blocks.iter().map(clone_block).collect()),
        Inline::Unsupported(s) => Inline::Unsupported(s.clone()),
    }
}

fn clone_block(b: &Block) -> Block {
    match b {
        Block::Para(c) => Block::Para(c.iter().map(clone_inline).collect()),
        Block::Plain(c) => Block::Plain(c.iter().map(clone_inline).collect()),
        Block::Header(lvl, a, c) => {
            Block::Header(*lvl, a.clone(), c.iter().map(clone_inline).collect())
        }
        Block::BlockQuote(blocks) => Block::BlockQuote(blocks.iter().map(clone_block).collect()),
        Block::CodeBlock(a, s) => Block::CodeBlock(a.clone(), s.clone()),
        Block::HorizontalRule => Block::HorizontalRule,
        Block::BulletList(items) => Block::BulletList(
            items
                .iter()
                .map(|item| item.iter().map(clone_block).collect())
                .collect(),
        ),
        Block::OrderedList(start, style, delim, items) => Block::OrderedList(
            *start,
            style,
            delim,
            items
                .iter()
                .map(|item| item.iter().map(clone_block).collect())
                .collect(),
        ),
        Block::RawBlock(f, c) => Block::RawBlock(f.clone(), c.clone()),
        Block::Table(_) => Block::Unsupported("Table".to_string()),
        Block::Div(a, blocks) => Block::Div(a.clone(), blocks.iter().map(clone_block).collect()),
        Block::LineBlock(lines) => Block::LineBlock(
            lines
                .iter()
                .map(|line| line.iter().map(clone_inline).collect())
                .collect(),
        ),
        Block::DefinitionList(items) => Block::DefinitionList(
            items
                .iter()
                .map(|(term, defs)| {
                    (
                        term.iter().map(clone_inline).collect(),
                        defs.iter()
                            .map(|d| d.iter().map(clone_block).collect())
                            .collect(),
                    )
                })
                .collect(),
        ),
        Block::Unsupported(s) => Block::Unsupported(s.clone()),
    }
}

fn smart_dashes_and_ellipsis(s: &str) -> String {
    if !s.contains(['-', '.']) {
        return s.to_string();
    }
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'-' {
            if i + 2 < bytes.len() && bytes[i + 1] == b'-' && bytes[i + 2] == b'-' {
                out.push('\u{2014}');
                i += 3;
                continue;
            }
            if i + 1 < bytes.len() && bytes[i + 1] == b'-' {
                out.push('\u{2013}');
                i += 2;
                continue;
            }
        }
        if bytes[i] == b'.' && i + 2 < bytes.len() && bytes[i + 1] == b'.' && bytes[i + 2] == b'.' {
            out.push('\u{2026}');
            i += 3;
            continue;
        }
        // Read one UTF-8 char.
        let len = utf8_char_len(bytes[i]);
        out.push_str(&s[i..i + len]);
        i += len;
    }
    out
}

fn utf8_char_len(b: u8) -> usize {
    // Invalid start bytes (0x80..0xc0) advance one byte to recover.
    if b < 0xc0 {
        1
    } else if b < 0xe0 {
        2
    } else if b < 0xf0 {
        3
    } else {
        4
    }
}

fn smart_intraword_apostrophe(s: &str) -> String {
    if !s.contains('\'') {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    for (i, &c) in chars.iter().enumerate() {
        if c == '\'' {
            let prev = i.checked_sub(1).map(|j| chars[j]);
            let next = chars.get(i + 1).copied();
            let prev_word = prev.is_some_and(is_word_char);
            let next_word = next.is_some_and(is_word_char);
            if prev_word && next_word {
                out.push('\u{2019}');
                continue;
            }
        }
        out.push(c);
    }
    out
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric()
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
            Inline::Math(_, c) => s.push_str(c),
            Inline::Span(_, children) => s.push_str(&inlines_to_plaintext(children)),
            Inline::RawInline(_, _) => {}
            Inline::Quoted(_, children) => s.push_str(&inlines_to_plaintext(children)),
            Inline::Note(_) => {}
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
        Block::RawBlock(format, content) => {
            out.push_str("RawBlock ( Format ");
            write_haskell_string(format, out);
            out.push_str(" ) ");
            write_haskell_string(content, out);
        }
        Block::Table(data) => {
            write_table(data, out);
        }
        Block::Div(attr, blocks) => {
            out.push_str("Div (");
            write_attr(attr, out);
            out.push_str(") [");
            write_block_list(blocks, out);
            out.push_str(" ]");
        }
        Block::LineBlock(lines) => {
            out.push_str("LineBlock [");
            for (i, line) in lines.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(" [");
                write_inline_list(line, out);
                out.push_str(" ]");
            }
            out.push_str(" ]");
        }
        Block::DefinitionList(items) => {
            out.push_str("DefinitionList [");
            for (i, (term, defs)) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(" ( [");
                write_inline_list(term, out);
                out.push_str(" ] , [");
                for (j, def) in defs.iter().enumerate() {
                    if j > 0 {
                        out.push(',');
                    }
                    out.push_str(" [");
                    write_block_list(def, out);
                    out.push_str(" ]");
                }
                out.push_str(" ] )");
            }
            out.push_str(" ]");
        }
        Block::Unsupported(name) => {
            out.push_str(&format!("Unsupported {name:?}"));
        }
    }
}

fn write_table(data: &TableData, out: &mut String) {
    out.push_str("Table ( \"\" , [ ] , [ ] ) ( Caption Nothing [");
    if !data.caption.is_empty() {
        out.push_str(" Plain [");
        write_inline_list(&data.caption, out);
        out.push_str(" ]");
    }
    out.push_str(" ] ) [");
    for (i, align) in data.aligns.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&format!(" ( {align} , ColWidthDefault )"));
    }
    out.push_str(" ] ( TableHead ( \"\" , [ ] , [ ] ) [");
    for (i, row) in data.head_rows.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push(' ');
        write_table_row(row, out);
    }
    out.push_str(" ] ) [ TableBody ( \"\" , [ ] , [ ] ) ( RowHeadColumns 0 ) [ ] [");
    for (i, row) in data.body_rows.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push(' ');
        write_table_row(row, out);
    }
    out.push_str(" ] ] ( TableFoot ( \"\" , [ ] , [ ] ) [ ] )");
}

fn write_table_row(cells: &[Vec<Block>], out: &mut String) {
    out.push_str("Row ( \"\" , [ ] , [ ] ) [");
    for (i, cell) in cells.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(" Cell ( \"\" , [ ] , [ ] ) AlignDefault ( RowSpan 1 ) ( ColSpan 1 ) [");
        if !cell.is_empty() {
            write_block_list(cell, out);
        }
        out.push_str(" ]");
    }
    out.push_str(" ]");
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
        Inline::Math(kind, content) => {
            out.push_str("Math ");
            out.push_str(kind);
            out.push(' ');
            write_haskell_string(content, out);
        }
        Inline::Span(attr, children) => {
            out.push_str("Span (");
            write_attr(attr, out);
            out.push_str(") [");
            write_inline_list(children, out);
            out.push_str(" ]");
        }
        Inline::RawInline(format, content) => {
            out.push_str("RawInline ( Format ");
            write_haskell_string(format, out);
            out.push_str(" ) ");
            write_haskell_string(content, out);
        }
        Inline::Quoted(kind, children) => {
            out.push_str("Quoted ");
            out.push_str(kind);
            out.push_str(" [");
            write_inline_list(children, out);
            out.push_str(" ]");
        }
        Inline::Note(blocks) => {
            out.push_str("Note [");
            write_block_list(blocks, out);
            out.push_str(" ]");
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
    let mut prev_was_numeric_escape = false;
    for ch in s.chars() {
        let code = ch as u32;
        let is_ascii_printable = (0x20..0x7f).contains(&code);
        match ch {
            '"' => {
                out.push_str("\\\"");
                prev_was_numeric_escape = false;
            }
            '\\' => {
                out.push_str("\\\\");
                prev_was_numeric_escape = false;
            }
            '\n' => {
                out.push_str("\\n");
                prev_was_numeric_escape = false;
            }
            '\t' => {
                out.push_str("\\t");
                prev_was_numeric_escape = false;
            }
            '\r' => {
                out.push_str("\\r");
                prev_was_numeric_escape = false;
            }
            _ if is_ascii_printable => {
                // Disambiguate digit immediately after a numeric escape: `\160\&33`
                // versus `\16033`.
                if prev_was_numeric_escape && ch.is_ascii_digit() {
                    out.push_str("\\&");
                }
                out.push(ch);
                prev_was_numeric_escape = false;
            }
            _ => {
                // Non-printable or non-ASCII → decimal escape.
                out.push('\\');
                out.push_str(&code.to_string());
                prev_was_numeric_escape = true;
            }
        }
    }
    out.push('"');
}

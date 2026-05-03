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
    /// Heading text-range start → final disambiguated id. Lets
    /// `heading_block` look up the document-level id (with `section`
    /// fallback for empty slugs and `-1`/`-2` suffixes for duplicates)
    /// that was computed during the pre-pass.
    heading_id_by_offset: HashMap<u32, String>,
    /// Footnote label → parsed body blocks. Lookup keyed by the raw label
    /// id text (no normalization needed — pandoc footnote labels are
    /// case-sensitive and not whitespace-collapsed).
    footnotes: HashMap<String, Vec<Block>>,
    /// Example-list label (`@label`) → resolved item number. Pandoc
    /// numbers all `OrderedList(_, Example, _)` items across the entire
    /// document with one shared counter; labeled items also become
    /// referenceable so inline `@label` resolves to the item's number.
    example_label_to_num: HashMap<String, usize>,
    /// Example-list start number per `LIST` text-range start. Looked up
    /// in `ordered_list_attrs` so each Example list reports the first
    /// item's number — picking up where the previous Example list left
    /// off rather than restarting at 1.
    example_list_start_by_offset: HashMap<u32, usize>,
}

thread_local! {
    static REFS_CTX: RefCell<RefsCtx> = RefCell::new(RefsCtx::default());
}

pub fn project(tree: &SyntaxNode) -> String {
    let ctx = build_refs_ctx(tree);
    REFS_CTX.with(|c| *c.borrow_mut() = ctx);
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
    REFS_CTX.with(|c| *c.borrow_mut() = RefsCtx::default());
    out
}

fn build_refs_ctx(tree: &SyntaxNode) -> RefsCtx {
    let mut ctx = RefsCtx::default();
    let mut seen_ids: HashMap<String, u32> = HashMap::new();
    collect_refs_and_headings(tree, &mut ctx, &mut seen_ids);
    let mut example_counter: usize = 0;
    collect_example_numbering(tree, &mut ctx, &mut example_counter);
    ctx
}

/// Walk every `LIST` in document order and assign Example-list numbers.
/// Pandoc tracks one counter across all `OrderedList(_, Example, _)` lists
/// in a document, so each subsequent Example list picks up where the prior
/// one left off. Labeled items (`(@label)`) get a label → number mapping
/// for inline `@label` reference resolution.
fn collect_example_numbering(node: &SyntaxNode, ctx: &mut RefsCtx, counter: &mut usize) {
    for child in node.children() {
        if child.kind() == SyntaxKind::LIST && list_is_example(&child) {
            let list_offset: u32 = child.text_range().start().into();
            ctx.example_list_start_by_offset
                .insert(list_offset, *counter + 1);
            for item in child
                .children()
                .filter(|c| c.kind() == SyntaxKind::LIST_ITEM)
            {
                *counter += 1;
                if let Some(label) = example_item_label(&item) {
                    ctx.example_label_to_num.entry(label).or_insert(*counter);
                }
            }
            // Recurse into the list's contents to pick up nested Example
            // lists (rare but possible).
            collect_example_numbering(&child, ctx, counter);
        } else {
            collect_example_numbering(&child, ctx, counter);
        }
    }
}

/// `(@)` / `(@label)` markers identify Example list items. Returns true
/// iff the LIST's first item carries such a marker (pandoc decides the
/// list style from the first marker only).
fn list_is_example(list: &SyntaxNode) -> bool {
    let Some(item) = list.children().find(|c| c.kind() == SyntaxKind::LIST_ITEM) else {
        return false;
    };
    let marker = list_item_marker_text(&item);
    let trimmed = marker.trim();
    let body = if let Some(inner) = trimmed.strip_prefix('(').and_then(|s| s.strip_suffix(')')) {
        inner
    } else if let Some(inner) = trimmed.strip_suffix(')') {
        inner
    } else if let Some(inner) = trimmed.strip_suffix('.') {
        inner
    } else {
        trimmed
    };
    body.starts_with('@')
        && body[1..]
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn list_item_marker_text(item: &SyntaxNode) -> String {
    item.children_with_tokens()
        .filter_map(|el| el.into_token())
        .find(|t| t.kind() == SyntaxKind::LIST_MARKER)
        .map(|t| t.text().to_string())
        .unwrap_or_default()
}

/// Returns the `@label` text for an Example list item, or `None` for the
/// unlabeled `(@)` form.
fn example_item_label(item: &SyntaxNode) -> Option<String> {
    let marker = list_item_marker_text(item);
    let trimmed = marker.trim();
    let body = trimmed
        .strip_prefix('(')
        .and_then(|s| s.strip_suffix(')'))
        .or_else(|| trimmed.strip_suffix(')'))
        .or_else(|| trimmed.strip_suffix('.'))
        .unwrap_or(trimmed);
    let label = body.strip_prefix('@')?;
    if label.is_empty() {
        None
    } else {
        Some(label.to_string())
    }
}

fn collect_refs_and_headings(
    node: &SyntaxNode,
    ctx: &mut RefsCtx,
    seen_ids: &mut HashMap<String, u32>,
) {
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
                let (id, was_explicit) = heading_id_with_explicitness(&child);
                let final_id = if was_explicit {
                    // Explicit `{#x}` ids are kept verbatim; pandoc only
                    // warns on conflicts but does not auto-disambiguate.
                    seen_ids.entry(id.clone()).or_insert(0);
                    id
                } else {
                    let mut base = id;
                    if base.is_empty() {
                        base = "section".to_string();
                    }
                    let count = seen_ids.entry(base.clone()).or_insert(0);
                    let id = if *count == 0 {
                        base
                    } else {
                        format!("{base}-{count}")
                    };
                    *count += 1;
                    id
                };
                if !final_id.is_empty() {
                    let offset: u32 = child.text_range().start().into();
                    ctx.heading_ids.insert(final_id.clone());
                    ctx.heading_id_by_offset.insert(offset, final_id);
                }
                collect_refs_and_headings(&child, ctx, seen_ids);
            }
            _ => collect_refs_and_headings(&child, ctx, seen_ids),
        }
    }
}

/// Returns `(id, was_explicit)` for a HEADING node. Explicit ids come from
/// `{#id}` attributes; the auto-id is the slugified plaintext (which may be
/// empty for headings whose text contains no slug-eligible characters).
fn heading_id_with_explicitness(node: &SyntaxNode) -> (String, bool) {
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
                return (parsed.id, true);
            }
        }
    }
    (pandoc_slugify(&inlines_to_plaintext(&inlines)), false)
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
    let raw_format = code_block_raw_format(node);
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
    while content.ends_with('\n') {
        content.pop();
    }
    // Pandoc expands tabs (4-col stops) on code-block bodies before any
    // indent stripping, so a `:\t` marker followed by `\t\t\tcode` correctly
    // becomes `"        code"` after the 4-col definition-content offset is
    // stripped. Apply expansion first, then strip.
    content = content
        .split('\n')
        .map(expand_tabs_to_4)
        .collect::<Vec<_>>()
        .join("\n");
    content = strip_leading_spaces_per_line(&content, extra);
    if !is_fenced {
        content = strip_indented_code_indent(&content);
    }
    if let Some(fmt) = raw_format {
        return Block::RawBlock(fmt, content);
    }
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
    /// `Figure attr (Caption Nothing [caption-blocks]) [body-blocks]` —
    /// pandoc's implicit_figures wraps an image-only paragraph whose
    /// alt text becomes the caption and whose body re-includes the
    /// image as a Plain block.
    Figure(Attr, Vec<Block>, Vec<Block>),
    Unsupported(String),
}

#[derive(Debug)]
struct TableData {
    /// Pandoc's `+caption_attributes` extension lifts a trailing
    /// `{#id .class kv=...}` from the caption text into the Table's outer
    /// attribute. Default-empty for tables without caption attributes.
    attr: Attr,
    caption: Vec<Inline>,
    aligns: Vec<&'static str>,
    /// Per-column width. `None` → `ColWidthDefault`, `Some(f)` → `ColWidth f`.
    widths: Vec<Option<f64>>,
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
        // Pandoc title block (`% title\n% authors\n% date`) populates Meta
        // and produces no body block.
        SyntaxKind::PANDOC_TITLE_BLOCK => None,
        SyntaxKind::HTML_BLOCK => Some(html_block(node)),
        SyntaxKind::PIPE_TABLE => pipe_table(node).map(Block::Table),
        SyntaxKind::SIMPLE_TABLE => simple_table(node).map(Block::Table),
        SyntaxKind::GRID_TABLE => grid_table(node).map(Block::Table),
        SyntaxKind::MULTILINE_TABLE => multiline_table(node).map(Block::Table),
        SyntaxKind::TEX_BLOCK => Some(tex_block(node)),
        SyntaxKind::FENCED_DIV => Some(fenced_div(node)),
        SyntaxKind::LINE_BLOCK => Some(line_block(node)),
        SyntaxKind::DEFINITION_LIST => Some(definition_list(node)),
        SyntaxKind::FIGURE => Some(figure_block(node)),
        other => Some(Block::Unsupported(format!("{other:?}"))),
    }
}

/// Pandoc's `implicit_figures` extension wraps a paragraph that is *only*
/// an Image into a `Figure` block: `Figure (id, [], []) (Caption Nothing
/// [Plain alt]) [Plain [Image]]`. The image's alt-text inlines become the
/// caption; the body holds the image itself wrapped in a Plain. Any
/// attribute attached to the Image migrates to the Figure attr (id only)
/// — the Image keeps its classes/kvs.
fn figure_block(node: &SyntaxNode) -> Block {
    let mut alt: Vec<Inline> = Vec::new();
    let mut image_inline: Option<Inline> = None;
    if let Some(image) = node.children().find(|c| c.kind() == SyntaxKind::IMAGE_LINK) {
        let alt_node = image.children().find(|c| c.kind() == SyntaxKind::IMAGE_ALT);
        if let Some(an) = alt_node {
            alt = coalesce_inlines(inlines_from(&an));
        }
        let mut tmp = Vec::new();
        render_image_inline(&image, &mut tmp);
        if let Some(first) = tmp.into_iter().next() {
            image_inline = Some(first);
        }
    }
    let figure_attr = match &image_inline {
        Some(Inline::Image(attr, _, _, _)) if !attr.id.is_empty() => Attr::with_id(attr.id.clone()),
        _ => Attr::default(),
    };
    let caption = if alt.is_empty() {
        Vec::new()
    } else {
        vec![Block::Plain(alt)]
    };
    let body = match image_inline {
        Some(img) => vec![Block::Plain(vec![img])],
        None => Vec::new(),
    };
    Block::Figure(figure_attr, caption, body)
}

fn heading_block(node: &SyntaxNode) -> Block {
    let level = heading_level(node);
    let inlines = node
        .children()
        .find(|c| c.kind() == SyntaxKind::HEADING_CONTENT)
        .map(|c| coalesce_inlines(inlines_from(&c)))
        .unwrap_or_default();
    // Auto-id and disambiguation are computed in the `RefsCtx` pre-pass so
    // duplicate slugs and `section`-fallbacks are document-wide consistent.
    // Explicit attributes still need their classes/kvs parsed here.
    let offset: u32 = node.text_range().start().into();
    let final_id = REFS_CTX
        .with(|c| c.borrow().heading_id_by_offset.get(&offset).cloned())
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
                    attr.id = final_id.clone();
                }
                attr
            } else {
                Attr::with_id(final_id.clone())
            }
        })
        .unwrap_or_else(|| Attr::with_id(final_id));
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
    let raw_format = code_block_raw_format(node);
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
    if is_fenced {
        // Pandoc tab-expands code-block bodies before emission. For indented
        // code, the expansion happens inside `strip_indented_code_indent`
        // before the 4-col strip; for fenced code there is no strip, so do
        // it directly here.
        content = content
            .split('\n')
            .map(expand_tabs_to_4)
            .collect::<Vec<_>>()
            .join("\n");
    } else {
        content = strip_indented_code_indent(&content);
    }
    if let Some(fmt) = raw_format {
        return Block::RawBlock(fmt, content);
    }
    Block::CodeBlock(attr, content)
}

/// Pandoc's raw-attribute syntax (`Ext_raw_attribute`) treats a fenced code
/// block whose info string is exactly `{=format}` as a `RawBlock` of that
/// format rather than a `CodeBlock`. The brace contents must start with `=`
/// followed by a non-empty token, with no other classes/ids/key-value pairs.
fn code_block_raw_format(node: &SyntaxNode) -> Option<String> {
    let open = node
        .children()
        .find(|c| c.kind() == SyntaxKind::CODE_FENCE_OPEN)?;
    let info = open
        .children()
        .find(|c| c.kind() == SyntaxKind::CODE_INFO)?;
    let raw = info.text().to_string();
    let trimmed = raw.trim();
    let inner = trimmed
        .strip_prefix('{')
        .and_then(|s| s.strip_suffix('}'))?;
    let inner = inner.trim();
    let format = inner.strip_prefix('=')?.trim();
    if format.is_empty() || format.contains(char::is_whitespace) {
        return None;
    }
    Some(format.to_string())
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
            attr.classes.insert(0, normalize_lang_id(lang));
        }
        return attr;
    }
    if !trimmed.is_empty() {
        return Attr {
            id: String::new(),
            classes: vec![normalize_lang_id(trimmed)],
            kvs: Vec::new(),
        };
    }
    Attr::default()
}

/// Mirrors pandoc's `toLanguageId` (Markdown reader): lowercases the language
/// identifier and applies the GitHub-syntax-highlighting normalizations
/// (`c++` → `cpp`, `objective-c` → `objectivec`).
fn normalize_lang_id(lang: &str) -> String {
    let lower = lang.to_ascii_lowercase();
    match lower.as_str() {
        "c++" => "cpp".to_string(),
        "objective-c" => "objectivec".to_string(),
        _ => lower,
    }
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
        // Pandoc expands tabs to 4-column tab stops *before* stripping the
        // 4-column indent. Mixed `  \tfoo` therefore becomes `    foo` →
        // `foo` after strip, which is what `pandoc -t native` emits.
        let expanded = expand_tabs_to_4(line);
        let stripped = if let Some(rest) = expanded.strip_prefix("    ") {
            rest.to_string()
        } else if let Some(rest) = expanded.strip_prefix('\t') {
            rest.to_string()
        } else {
            // Strip up to 3 leading spaces if present (pandoc tolerates short
            // indentation only on blank lines, which we don't try to detect
            // here — safer to leave non-conforming lines alone).
            expanded
        };
        out.push_str(&stripped);
    }
    out
}

/// Expand `\t` to spaces using 4-column tab stops, starting from column 0
/// of `line`. Pandoc applies this to indented code blocks before stripping
/// the leading 4-column indent so the body byte-equals what pandoc emits.
fn expand_tabs_to_4(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut col = 0usize;
    for c in line.chars() {
        if c == '\t' {
            let next = (col / 4 + 1) * 4;
            for _ in col..next {
                out.push(' ');
            }
            col = next;
        } else {
            out.push(c);
            col += 1;
        }
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

/// Read a child `ATTRIBUTE` (node or token) on `parent` and parse its
/// `{...}` body into an `Attr`. Returns `Attr::default()` if no attribute
/// is attached or the body isn't `{...}`-shaped.
fn extract_attr_from_node(parent: &SyntaxNode) -> Attr {
    let raw = parent.children_with_tokens().find_map(|el| match el {
        NodeOrToken::Node(n) if n.kind() == SyntaxKind::ATTRIBUTE => Some(n.text().to_string()),
        NodeOrToken::Token(t) if t.kind() == SyntaxKind::ATTRIBUTE => Some(t.text().to_string()),
        _ => None,
    });
    let Some(raw) = raw else {
        return Attr::default();
    };
    let trimmed = raw.trim();
    if let Some(inner) = trimmed.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
        parse_attr_block(inner)
    } else {
        Attr::default()
    }
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

/// Parse HTML-style attributes `class="x" id="y" key="z"` into `Attr`,
/// mapping `class` (whitespace-split) → classes, `id` → id, others → kvs.
fn parse_html_attrs(s: &str) -> Attr {
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
            _ => {
                let key_start = i;
                while i < bytes.len() && !matches!(bytes[i], b' ' | b'\t' | b'\n' | b'\r' | b'=') {
                    i += 1;
                }
                let key = s[key_start..i].to_string();
                let value = if i < bytes.len() && bytes[i] == b'=' {
                    i += 1;
                    if i < bytes.len() && (bytes[i] == b'"' || bytes[i] == b'\'') {
                        let quote = bytes[i];
                        i += 1;
                        let v_start = i;
                        while i < bytes.len() && bytes[i] != quote {
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
                    }
                } else {
                    String::new()
                };
                if key.is_empty() {
                    continue;
                }
                match key.as_str() {
                    "class" => {
                        for c in value.split_ascii_whitespace() {
                            classes.push(c.to_string());
                        }
                    }
                    "id" => id = value,
                    _ => kvs.push((key, value)),
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
            let loose = is_loose_definition_item(&item);
            let defs: Vec<Vec<Block>> = item
                .children()
                .filter(|c| c.kind() == SyntaxKind::DEFINITION)
                .map(|d| definition_blocks(&d, loose))
                .collect();
            (term, defs)
        })
        .collect();
    Block::DefinitionList(items)
}

/// A `DEFINITION_ITEM` is "loose" iff there is a `BLANK_LINE` between the
/// `TERM` (or its preceding term continuations) and the first `DEFINITION`.
/// Pandoc renders loose definitions with `Para` blocks; tight ones use
/// `Plain`. The looseness is per-item (per-term group), not per-definition,
/// and applies to *all* definitions in the item — see pandoc's behavior.
fn is_loose_definition_item(item: &SyntaxNode) -> bool {
    let mut saw_term = false;
    for child in item.children_with_tokens() {
        if let NodeOrToken::Node(n) = child {
            match n.kind() {
                SyntaxKind::TERM => {
                    saw_term = true;
                }
                SyntaxKind::BLANK_LINE if saw_term => {
                    return true;
                }
                SyntaxKind::DEFINITION => {
                    return false;
                }
                _ => {}
            }
        }
    }
    false
}

fn definition_blocks(def_node: &SyntaxNode, loose: bool) -> Vec<Block> {
    // Definition body content lives at the marker's content offset (`: ` →
    // 2 columns by default). The CST keeps that indent on each line, so any
    // CODE_BLOCK descendant needs the offset stripped before pandoc-native
    // projection.
    let extra = definition_content_offset(def_node);
    let mut out = Vec::new();
    for child in def_node.children() {
        match child.kind() {
            SyntaxKind::PLAIN => {
                let inlines = coalesce_inlines(inlines_from(&child));
                if loose {
                    out.push(Block::Para(inlines));
                } else {
                    out.push(Block::Plain(inlines));
                }
            }
            SyntaxKind::PARAGRAPH => {
                out.push(Block::Para(coalesce_inlines(inlines_from(&child))));
            }
            SyntaxKind::CODE_BLOCK if extra > 0 => {
                out.push(indented_code_block_with_extra_strip(&child, extra));
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

/// Visual column where definition body content starts. The strip later runs
/// against the *tab-expanded* body, so this offset must be measured in
/// columns (tabs round to the next 4-col stop), not raw chars: `:\t` reaches
/// col 4, which is the column the body's strip should remove.
fn definition_content_offset(def_node: &SyntaxNode) -> usize {
    let mut col = 0usize;
    let mut saw_marker = false;
    for el in def_node.children_with_tokens() {
        if let NodeOrToken::Token(t) = el {
            match t.kind() {
                SyntaxKind::DEFINITION_MARKER => {
                    col = advance_col(col, t.text());
                    saw_marker = true;
                }
                SyntaxKind::WHITESPACE if saw_marker => {
                    return advance_col(col, t.text());
                }
                _ if saw_marker => return col,
                _ => {}
            }
        } else if saw_marker {
            return col;
        }
    }
    col
}

/// Advance a column counter by `s`, treating `\t` as moving to the next
/// 4-column tab stop and any other character as a single column.
fn advance_col(start: usize, s: &str) -> usize {
    let mut col = start;
    for c in s.chars() {
        if c == '\t' {
            col = (col / 4 + 1) * 4;
        } else {
            col += 1;
        }
    }
    col
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
    let is_html = node
        .children_with_tokens()
        .any(|el| matches!(&el, NodeOrToken::Token(t) if t.kind() == SyntaxKind::SPAN_BRACKET_OPEN && t.text().starts_with('<')));
    let attr_text = node.children_with_tokens().find_map(|el| match el {
        NodeOrToken::Token(t) if t.kind() == SyntaxKind::SPAN_ATTRIBUTES => {
            Some(t.text().to_string())
        }
        NodeOrToken::Node(n) if n.kind() == SyntaxKind::SPAN_ATTRIBUTES => {
            Some(n.text().to_string())
        }
        _ => None,
    });
    let attr = attr_text
        .map(|raw| {
            let trimmed = raw.trim();
            if is_html {
                parse_html_attrs(trimmed)
            } else if let Some(inner) = trimmed.strip_prefix('{').and_then(|s| s.strip_suffix('}'))
            {
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
    let (attr, caption_inlines) = extract_caption_attrs(caption_inlines);
    Some(TableData {
        attr,
        caption: caption_inlines,
        aligns,
        widths: vec![None; cols],
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

/// Pandoc's `+caption_attributes` extension lifts a trailing `{...}` from a
/// table caption into the Table's outer attribute. Walk the caption inlines
/// from the right looking for a balanced trailing `{...}` span: a Str
/// ending with `}` plus zero or more (Space, Str) pairs back until a Str
/// starts with `{`. If found, parse the brace contents as an attribute
/// block and drop those inlines (plus any preceding Space) from the caption
/// text.
fn extract_caption_attrs(mut inlines: Vec<Inline>) -> (Attr, Vec<Inline>) {
    let last_str_end = inlines
        .iter()
        .rposition(|i| matches!(i, Inline::Str(s) if s.ends_with('}')));
    let Some(end_idx) = last_str_end else {
        return (Attr::default(), inlines);
    };
    // Walk back to find the Str starting with `{`. Allow only Str/Space
    // between (no structural inlines like Emph), since attribute blocks
    // are plain text.
    let mut start_idx = end_idx;
    let mut found_open = false;
    loop {
        match &inlines[start_idx] {
            Inline::Str(s) => {
                if s.starts_with('{') {
                    found_open = true;
                    break;
                }
            }
            Inline::Space => {}
            _ => return (Attr::default(), inlines),
        }
        if start_idx == 0 {
            break;
        }
        start_idx -= 1;
    }
    if !found_open {
        return (Attr::default(), inlines);
    }
    // Concatenate the Str/Space slice into a flat string, then strip the
    // outer braces.
    let mut raw = String::new();
    for el in &inlines[start_idx..=end_idx] {
        match el {
            Inline::Str(s) => raw.push_str(s),
            Inline::Space => raw.push(' '),
            _ => return (Attr::default(), inlines),
        }
    }
    if !(raw.starts_with('{') && raw.ends_with('}')) {
        return (Attr::default(), inlines);
    }
    let inner = &raw[1..raw.len() - 1];
    let attr = parse_attr_block(inner);
    inlines.truncate(start_idx);
    if matches!(inlines.last(), Some(Inline::Space)) {
        inlines.pop();
    }
    (attr, inlines)
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
    // Strip surrounding whitespace before pipe-stripping so an indented
    // pipe-table separator (e.g. fenced-div content at column ≥1) doesn't
    // leave a leading whitespace segment that then counts as a phantom
    // column.
    let trimmed = raw.trim();
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

/// Pandoc-style `show` for `Double`. Decimal in `[0.1, 1e7)`, scientific
/// otherwise. Always emits a fractional component (`1.0` not `1`). Used for
/// `ColWidth N` rendering, where N is in `(0.0, 1.0)` for our cases.
fn show_double(x: f64) -> String {
    if x == 0.0 {
        return "0.0".to_string();
    }
    let abs = x.abs();
    if (0.1..1e7).contains(&abs) {
        let s = format!("{x}");
        if s.contains('.') || s.contains('e') {
            s
        } else {
            format!("{s}.0")
        }
    } else {
        // Rust's `{:e}` already matches Haskell's mantissa/exponent shape:
        // `8.333333333333333e-2`. Whole-number mantissa needs `.0` appended.
        let s = format!("{x:e}");
        if let Some((m, e)) = s.split_once('e') {
            if m.contains('.') {
                s
            } else {
                format!("{m}.0e{e}")
            }
        } else {
            s
        }
    }
}

// ----- simple table -------------------------------------------------------

/// Project a `SIMPLE_TABLE` node. Pandoc's "simple" table form:
///
/// ```text
///    Col1     Col2
/// -------- --------    ← TABLE_SEPARATOR (dash runs define columns)
///   data1    data2
///
/// Table: optional caption
/// ```
///
/// Headerless variant skips the header row and uses dash runs both above
/// and below the data. Alignment is derived from each header cell's
/// position relative to its column's dash run boundaries. For headerless
/// tables, alignment derives from the *first data row*.
fn simple_table(node: &SyntaxNode) -> Option<TableData> {
    let separator = node
        .children()
        .find(|c| c.kind() == SyntaxKind::TABLE_SEPARATOR)?;
    let cols = simple_table_dash_runs(&separator);
    if cols.is_empty() {
        return None;
    }
    let header = node
        .children()
        .find(|c| c.kind() == SyntaxKind::TABLE_HEADER);
    // Body rows: every TABLE_ROW. Drop a trailing all-dashes row — that is
    // the closing `---` separator of a headerless table that the parser
    // currently emits as a TABLE_ROW of dash cells.
    let mut body_rows_nodes: Vec<SyntaxNode> = node
        .children()
        .filter(|c| c.kind() == SyntaxKind::TABLE_ROW)
        .collect();
    if header.is_none()
        && body_rows_nodes
            .last()
            .map(simple_table_row_is_all_dashes)
            .unwrap_or(false)
    {
        body_rows_nodes.pop();
    }
    // Alignment: from header if present, else from the first data row.
    let aligns = if let Some(h) = &header {
        simple_table_aligns(h, &cols)
    } else if let Some(r0) = body_rows_nodes.first() {
        simple_table_aligns(r0, &cols)
    } else {
        vec!["AlignDefault"; cols.len()]
    };
    let head_rows = match &header {
        Some(h) => {
            let cells: Vec<Vec<Inline>> = simple_table_row_cells(h);
            vec![cells_to_plain_blocks(cells, cols.len())]
        }
        None => Vec::new(),
    };
    let body_rows: Vec<Vec<Vec<Block>>> = body_rows_nodes
        .iter()
        .map(|r| cells_to_plain_blocks(simple_table_row_cells(r), cols.len()))
        .collect();
    let caption_inlines = node
        .children()
        .find(|c| c.kind() == SyntaxKind::TABLE_CAPTION)
        .map(|n| pipe_table_caption(&n))
        .unwrap_or_default();
    let (attr, caption_inlines) = extract_caption_attrs(caption_inlines);
    Some(TableData {
        attr,
        caption: caption_inlines,
        aligns,
        widths: vec![None; cols.len()],
        head_rows,
        body_rows,
    })
}

/// Return the `(start_col, end_col)` (inclusive) of each dash run in a
/// `TABLE_SEPARATOR` node, where columns are 0-based offsets within the
/// separator's line.
fn simple_table_dash_runs(separator: &SyntaxNode) -> Vec<(usize, usize)> {
    let raw = separator.text().to_string();
    let line = raw.trim_end_matches(['\n', '\r']);
    let mut runs = Vec::new();
    let mut start: Option<usize> = None;
    for (i, ch) in line.char_indices() {
        if ch == '-' {
            if start.is_none() {
                start = Some(i);
            }
        } else if let Some(s) = start.take() {
            runs.push((s, i - 1));
        }
    }
    if let Some(s) = start.take() {
        runs.push((s, line.len() - 1));
    }
    runs
}

fn simple_table_row_cells(row: &SyntaxNode) -> Vec<Vec<Inline>> {
    // Zero-width TABLE_CELL nodes represent positionally-empty columns
    // (e.g. case 0094, where header words land in only some of the
    // dash-defined columns). Keep them as empty cells so the row's
    // column ordering matches the dash separator.
    row.children()
        .filter(|c| c.kind() == SyntaxKind::TABLE_CELL)
        .map(|cell| coalesce_inlines(inlines_from(&cell)))
        .collect()
}

fn simple_table_row_is_all_dashes(row: &SyntaxNode) -> bool {
    let mut had_cell = false;
    for cell in row
        .children()
        .filter(|c| c.kind() == SyntaxKind::TABLE_CELL)
    {
        let text = cell.text().to_string();
        let trimmed = text.trim();
        if trimmed.is_empty() {
            continue;
        }
        had_cell = true;
        if !trimmed.chars().all(|c| c == '-') {
            return false;
        }
    }
    had_cell
}

/// Derive alignments for a simple-table header (or first data row) by
/// comparing each cell's *visible* (whitespace-trimmed) column range to
/// the corresponding dash run. Multiline-table TABLE_CELL nodes include
/// the padding whitespace within the column slice, so we have to peel
/// off leading/trailing whitespace before applying the flushness rule.
/// (Single-line simple-table cells already exclude padding whitespace,
/// but the trim is a no-op there.)
fn simple_table_aligns(row: &SyntaxNode, cols: &[(usize, usize)]) -> Vec<&'static str> {
    let row_start: u32 = row.text_range().start().into();
    let mut cell_ranges: Vec<(usize, usize)> = Vec::new();
    for cell in row
        .children()
        .filter(|c| c.kind() == SyntaxKind::TABLE_CELL)
    {
        if cell.text_range().is_empty() {
            continue;
        }
        let text = cell.text().to_string();
        let lstrip = text.chars().take_while(|c| *c == ' ' || *c == '\t').count();
        let rstrip = text
            .chars()
            .rev()
            .take_while(|c| *c == ' ' || *c == '\t')
            .count();
        let trimmed_len = text.chars().count().saturating_sub(lstrip + rstrip);
        if trimmed_len == 0 {
            continue;
        }
        let start: u32 = cell.text_range().start().into();
        let s = (start - row_start) as usize;
        let visible_start = s + lstrip;
        let visible_end = visible_start + trimmed_len - 1;
        cell_ranges.push((visible_start, visible_end));
    }
    cols.iter()
        .map(|(col_start, col_end)| {
            let cell = cell_ranges
                .iter()
                .find(|(cs, ce)| ce >= col_start && cs <= col_end);
            match cell {
                Some((cs, ce)) => {
                    let left_flush = cs == col_start;
                    let right_flush = ce == col_end;
                    match (left_flush, right_flush) {
                        (true, true) => "AlignDefault",
                        (true, false) => "AlignLeft",
                        (false, true) => "AlignRight",
                        (false, false) => "AlignCenter",
                    }
                }
                None => "AlignDefault",
            }
        })
        .collect()
}

// ----- grid table ---------------------------------------------------------

/// Project a `GRID_TABLE` node. Pandoc-native column widths are
/// `(dash_count + 1) / 72`, where 72 is pandoc's reference width (default
/// `--columns`). Alignment colons in a `:===:` separator follow the same
/// rules as pipe tables. We use the *first* alignment-bearing separator —
/// the one immediately following the header — as the canonical alignment;
/// pandoc falls back to `AlignDefault` when no separator carries colons.
fn grid_table(node: &SyntaxNode) -> Option<TableData> {
    let first_sep = node
        .children()
        .find(|c| c.kind() == SyntaxKind::TABLE_SEPARATOR)?;
    let widths = grid_dash_widths(&first_sep);
    let cols = widths.len();
    if cols == 0 {
        return None;
    }
    // Alignment separator: scan all separators, prefer one with `:` markers.
    let mut aligns: Vec<&'static str> = vec!["AlignDefault"; cols];
    for sep in node
        .children()
        .filter(|c| c.kind() == SyntaxKind::TABLE_SEPARATOR)
    {
        let raw = sep.text().to_string();
        if raw.contains(':') {
            aligns = grid_separator_aligns(&raw, cols);
            break;
        }
    }
    let header = node
        .children()
        .find(|c| c.kind() == SyntaxKind::TABLE_HEADER);
    let head_rows = match &header {
        Some(h) => {
            let cells = grid_row_cells(h);
            vec![cells_to_plain_blocks(cells, cols)]
        }
        None => Vec::new(),
    };
    let body_rows: Vec<Vec<Vec<Block>>> = node
        .children()
        .filter(|c| c.kind() == SyntaxKind::TABLE_ROW)
        .map(|r| cells_to_plain_blocks(grid_row_cells(&r), cols))
        .collect();
    // Caption: a TABLE_CAPTION child of the GRID_TABLE.
    let caption_inlines = node
        .children()
        .find(|c| c.kind() == SyntaxKind::TABLE_CAPTION)
        .map(|n| pipe_table_caption(&n))
        .unwrap_or_default();
    let (attr, caption_inlines) = extract_caption_attrs(caption_inlines);
    Some(TableData {
        attr,
        caption: caption_inlines,
        aligns,
        widths: widths.into_iter().map(Some).collect(),
        head_rows,
        body_rows,
    })
}

fn grid_row_cells(row: &SyntaxNode) -> Vec<Vec<Inline>> {
    row.children()
        .filter(|c| c.kind() == SyntaxKind::TABLE_CELL)
        .map(|cell| coalesce_inlines(inlines_from(&cell)))
        .collect()
}

/// Compute per-column widths from a grid-table separator like
/// `+--------+----------+----------+`. The `+` characters delimit
/// columns; each run of dashes/equals/colons between two `+` is one
/// column. Pandoc's formula (`Text/Pandoc/Parsing/GridTable.hs::
/// fractionalColumnWidths`):
/// ```text
/// raw[i] = dashes[i] + 1       (include separator width)
/// norm   = max(sum(raw) + count - 2, 72)   (72 = readerColumns)
/// width[i] = raw[i] / norm
/// ```
fn grid_dash_widths(separator: &SyntaxNode) -> Vec<f64> {
    let raw_text = separator.text().to_string();
    let line = raw_text.trim_end_matches(['\n', '\r']);
    let mut raw: Vec<usize> = Vec::new();
    let mut count: usize = 0;
    let mut in_col = false;
    for ch in line.chars() {
        match ch {
            '+' => {
                if in_col {
                    raw.push(count + 1);
                    count = 0;
                }
                in_col = true;
            }
            _ => {
                if in_col {
                    count += 1;
                }
            }
        }
    }
    if raw.is_empty() {
        return Vec::new();
    }
    let total: usize = raw.iter().sum();
    let count = raw.len();
    let norm = (total + count).saturating_sub(2).max(72) as f64;
    raw.into_iter().map(|w| w as f64 / norm).collect()
}

fn grid_separator_aligns(raw: &str, cols: usize) -> Vec<&'static str> {
    let line = raw.trim_end_matches(['\n', '\r']);
    let mut aligns: Vec<&'static str> = Vec::with_capacity(cols);
    let mut col_start: Option<usize> = None;
    for (i, ch) in line.char_indices() {
        if ch == '+' {
            if let Some(s) = col_start.take() {
                let seg = &line[s..i];
                aligns.push(grid_segment_align(seg));
            }
            col_start = Some(i + 1);
        }
    }
    while aligns.len() < cols {
        aligns.push("AlignDefault");
    }
    aligns.truncate(cols);
    aligns
}

fn grid_segment_align(seg: &str) -> &'static str {
    let bytes = seg.as_bytes();
    let left = bytes.first() == Some(&b':');
    let right = bytes.last() == Some(&b':');
    match (left, right) {
        (true, true) => "AlignCenter",
        (true, false) => "AlignLeft",
        (false, true) => "AlignRight",
        _ => "AlignDefault",
    }
}

// ----- multiline table ----------------------------------------------------

/// Project a `MULTILINE_TABLE` node. Multi-line tables have an opening
/// `-----` border, an optional header (one or more lines), a
/// `----- ----- -----` column separator, body rows (each row possibly
/// spans multiple lines, separated from the next row by a blank line),
/// and a closing `-----` border. Cell content within a row is joined with
/// `SoftBreak` between source lines. Column widths are
/// `(dash_count + 1) / 72`.
fn multiline_table(node: &SyntaxNode) -> Option<TableData> {
    // The column-separator (the dashes between header and body) is the
    // *second* TABLE_SEPARATOR if there is a header, else the first.
    let separators: Vec<SyntaxNode> = node
        .children()
        .filter(|c| c.kind() == SyntaxKind::TABLE_SEPARATOR)
        .collect();
    let header = node
        .children()
        .find(|c| c.kind() == SyntaxKind::TABLE_HEADER);
    let column_sep = if header.is_some() {
        separators.get(1).cloned()
    } else {
        separators.first().cloned()
    }?;
    let cols = simple_table_dash_runs(&column_sep);
    if cols.is_empty() {
        return None;
    }
    // Per pandoc `widthsFromIndices`: each non-last column's width is
    // `dashes + spaces_after` (= start of next column - start of this); the
    // last column's width is `dashes + 1` (the indices' bump). Normalize
    // by `max(total, 72)`.
    let raw: Vec<usize> = cols
        .iter()
        .enumerate()
        .map(|(i, (s, e))| {
            if i + 1 < cols.len() {
                cols[i + 1].0 - s
            } else {
                e - s + 2
            }
        })
        .collect();
    let total: usize = raw.iter().sum();
    let norm = (total.max(72)) as f64;
    let widths: Vec<f64> = raw.into_iter().map(|w| w as f64 / norm).collect();
    // Alignment from header (if present) or first data row, using the
    // simple-table flushness rule against the column-separator dash runs.
    let aligns = if let Some(h) = &header {
        simple_table_aligns(h, &cols)
    } else if let Some(r0) = node.children().find(|c| c.kind() == SyntaxKind::TABLE_ROW) {
        simple_table_aligns(&r0, &cols)
    } else {
        vec!["AlignDefault"; cols.len()]
    };
    let head_rows = match &header {
        Some(h) => vec![multiline_row_cells_blocks(h, &cols)],
        None => Vec::new(),
    };
    let body_rows: Vec<Vec<Vec<Block>>> = node
        .children()
        .filter(|c| c.kind() == SyntaxKind::TABLE_ROW)
        .map(|r| multiline_row_cells_blocks(&r, &cols))
        .collect();
    let caption_inlines = node
        .children()
        .find(|c| c.kind() == SyntaxKind::TABLE_CAPTION)
        .map(|n| pipe_table_caption(&n))
        .unwrap_or_default();
    let (attr, caption_inlines) = extract_caption_attrs(caption_inlines);
    Some(TableData {
        attr,
        caption: caption_inlines,
        aligns,
        widths: widths.into_iter().map(Some).collect(),
        head_rows,
        body_rows,
    })
}

/// Slice each line of a multiline-table row by column ranges, then merge
/// each column's per-line text into a single Plain block with `SoftBreak`s
/// between source lines.
fn multiline_row_cells_blocks(row: &SyntaxNode, cols: &[(usize, usize)]) -> Vec<Vec<Block>> {
    let row_start: u32 = row.text_range().start().into();
    let raw = row.text().to_string();
    // Re-construct the row's per-line text. Tokens give us byte offsets, but
    // plain `.text()` is enough — split on '\n', then for each line, slice by
    // column ranges.
    let lines: Vec<&str> = raw.split_inclusive('\n').collect();
    let mut col_lines: Vec<Vec<String>> = vec![Vec::new(); cols.len()];
    let mut line_start_offset: usize = 0;
    for line in lines {
        let line_no_nl = line.trim_end_matches('\n');
        if line_no_nl.trim().is_empty() {
            line_start_offset += line.len();
            continue;
        }
        for (i, &(cs, ce)) in cols.iter().enumerate() {
            // Slice [cs..=ce] in chars from the line. Lines may be shorter.
            let slice = char_slice(line_no_nl, cs, ce + 1);
            let trimmed = slice.trim();
            if !trimmed.is_empty() {
                col_lines[i].push(trimmed.to_string());
            }
        }
        line_start_offset += line.len();
    }
    let _ = (row_start, line_start_offset);
    cols.iter()
        .enumerate()
        .map(|(i, _)| {
            let segments = &col_lines[i];
            if segments.is_empty() {
                return Vec::new();
            }
            // Re-parse the cell's joined text through panache's inline parser
            // so that `**bold**`, `` `code` ``, `[link](url)` etc. inside
            // multiline-table cells project as Strong/Code/Link rather than
            // raw Str (matches pandoc's `multilineTableHeader` behavior of
            // joining lines per column and parsing as Markdown).
            let joined = segments.join("\n");
            let inlines = parse_cell_text_inlines(&joined);
            if inlines.is_empty() {
                return Vec::new();
            }
            vec![Block::Plain(coalesce_inlines(inlines))]
        })
        .collect()
}

/// Parse a cell text fragment through panache's inline parser and return its
/// inline content. Used for multiline-table cells whose per-line slices are
/// not seen by the outer parser as inline-bearing TABLE_CELLs (the parser
/// holds raw TEXT for lines past the first). Empty or whitespace-only input
/// returns an empty vec.
fn parse_cell_text_inlines(text: &str) -> Vec<Inline> {
    if text.trim().is_empty() {
        return Vec::new();
    }
    let opts = panache_parser::ParserOptions {
        flavor: panache_parser::Flavor::Pandoc,
        dialect: panache_parser::Dialect::for_flavor(panache_parser::Flavor::Pandoc),
        extensions: panache_parser::Extensions::for_flavor(panache_parser::Flavor::Pandoc),
        ..panache_parser::ParserOptions::default()
    };
    let doc = panache_parser::parse(text, Some(opts));
    for node in doc.descendants() {
        if matches!(node.kind(), SyntaxKind::PARAGRAPH | SyntaxKind::PLAIN) {
            return inlines_from(&node);
        }
    }
    Vec::new()
}

fn char_slice(s: &str, start_char: usize, end_char: usize) -> &str {
    let mut start_byte = s.len();
    let mut end_byte = s.len();
    for (i, (b, _)) in s.char_indices().enumerate() {
        if i == start_char {
            start_byte = b;
        }
        if i == end_char {
            end_byte = b;
            break;
        }
    }
    if start_byte > end_byte {
        return "";
    }
    &s[start_byte..end_byte]
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
    let (mut start, style, delim) = classify_ordered_marker(marker.trim());
    if style == "Example" {
        let offset: u32 = node.text_range().start().into();
        if let Some(s) = REFS_CTX.with(|c| {
            c.borrow()
                .example_list_start_by_offset
                .get(&offset)
                .copied()
        }) {
            start = s;
        }
    }
    (start, style, delim)
}

/// Map a list-marker token (e.g. `1.`, `iv)`, `(A)`, `#.`, `(@)`) to the
/// pandoc-native `(start, style, delim)` tuple. Mirrors pandoc's parser logic
/// in `Text/Pandoc/Parsing/Lists.hs`: try `decimal`, then `exampleNum` (`@`),
/// then `defaultNum` (`#`), then `romanOne` (single `i`/`I`), then alpha,
/// then multi-char roman, in that order; the first matching form wins. The
/// start value for Example lists is left at 1 — pandoc tracks numbering
/// across lists at the document level, which we don't model.
fn classify_ordered_marker(trimmed: &str) -> (usize, &'static str, &'static str) {
    // Strip surrounding parens / trailing period or paren to get (body, delim).
    let (body, delim) =
        if let Some(inner) = trimmed.strip_prefix('(').and_then(|s| s.strip_suffix(')')) {
            (inner, "TwoParens")
        } else if let Some(inner) = trimmed.strip_suffix(')') {
            (inner, "OneParen")
        } else if let Some(inner) = trimmed.strip_suffix('.') {
            (inner, "Period")
        } else {
            (trimmed, "DefaultDelim")
        };

    // All-digit body → Decimal.
    if !body.is_empty() && body.chars().all(|c| c.is_ascii_digit()) {
        let start: usize = body.parse().unwrap_or(1);
        return (start, "Decimal", delim);
    }

    // `#` (DefaultStyle) — when style is DefaultStyle pandoc forces
    // DefaultDelim regardless of the actual punctuation.
    if body == "#" {
        return (1, "DefaultStyle", "DefaultDelim");
    }

    // `@` or `@label` (Example list).
    if let Some(rest) = body.strip_prefix('@')
        && rest
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return (1, "Example", delim);
    }

    // Single `i`/`I` is romanOne (tried before alpha, so `i.`/`I.` is Roman 1).
    if body == "i" {
        return (1, "LowerRoman", delim);
    }
    if body == "I" {
        return (1, "UpperRoman", delim);
    }

    // Single lowercase / uppercase letter → alpha.
    if body.len() == 1
        && let Some(c) = body.chars().next()
    {
        if c.is_ascii_lowercase() {
            return ((c as u8 - b'a') as usize + 1, "LowerAlpha", delim);
        }
        if c.is_ascii_uppercase() {
            return ((c as u8 - b'A') as usize + 1, "UpperAlpha", delim);
        }
    }

    // Multi-char roman lowercase/uppercase.
    if body
        .chars()
        .all(|c| matches!(c, 'i' | 'v' | 'x' | 'l' | 'c' | 'd' | 'm'))
        && let Some(n) = roman_to_int(body, false)
    {
        return (n, "LowerRoman", delim);
    }
    if body
        .chars()
        .all(|c| matches!(c, 'I' | 'V' | 'X' | 'L' | 'C' | 'D' | 'M'))
        && let Some(n) = roman_to_int(body, true)
    {
        return (n, "UpperRoman", delim);
    }

    // Fallback — the parser accepted some marker we don't classify; emit
    // Decimal/Period so the list renders rather than dropping coverage.
    (1, "Decimal", delim)
}

/// Convert a roman numeral string to its integer value. Returns `None` if the
/// string isn't a syntactically-valid roman numeral. Mirrors pandoc's
/// `romanNumeral` (greedy left-to-right with subtractive pairs).
fn roman_to_int(s: &str, upper: bool) -> Option<usize> {
    let normalize = |c: char| if upper { c } else { c.to_ascii_uppercase() };
    let value = |c: char| match c {
        'I' => 1,
        'V' => 5,
        'X' => 10,
        'L' => 50,
        'C' => 100,
        'D' => 500,
        'M' => 1000,
        _ => 0,
    };
    let chars: Vec<char> = s.chars().map(normalize).collect();
    if chars.is_empty() {
        return None;
    }
    let mut total = 0usize;
    let mut i = 0;
    while i < chars.len() {
        let v = value(chars[i]);
        if v == 0 {
            return None;
        }
        let next = chars.get(i + 1).copied().map(value).unwrap_or(0);
        if v < next {
            total += next - v;
            i += 2;
        } else {
            total += v;
            i += 1;
        }
    }
    Some(total)
}

fn list_item_blocks(item: &SyntaxNode, loose: bool) -> Vec<Block> {
    let mut out = Vec::new();
    let item_indent = list_item_content_offset(item);
    let task_checkbox = task_checkbox_for_item(item);
    let mut checkbox_emitted = false;
    for child in item.children() {
        match child.kind() {
            SyntaxKind::PLAIN => {
                let mut inlines = coalesce_inlines(inlines_from(&child));
                // Skip empty Plain blocks. The parser emits a PLAIN node for
                // any line under a list item, including the bare-marker line
                // (`-` followed by blank then indented content); pandoc only
                // counts blocks with actual inline content.
                if inlines.is_empty() {
                    continue;
                }
                if !checkbox_emitted && let Some(glyph) = task_checkbox {
                    inlines.insert(0, Inline::Space);
                    inlines.insert(0, Inline::Str(glyph.to_string()));
                    checkbox_emitted = true;
                }
                if loose {
                    out.push(Block::Para(inlines));
                } else {
                    out.push(Block::Plain(inlines));
                }
            }
            SyntaxKind::CODE_BLOCK => {
                // Both fenced and indented code blocks inside list items
                // carry the item-content indent on every body line in the
                // CST. Strip that offset so pandoc sees the same body it
                // would in a flat document. (For indented code, the helper
                // also strips the 4-space code-block indent on top of the
                // item offset; for fenced code, the offset strip alone is
                // sufficient.)
                out.push(indented_code_block_with_extra_strip(&child, item_indent));
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

/// Pandoc renders `- [ ] foo` as `Plain [Str "\u{2610}", Space, Str "foo"]`
/// (and `[x]`/`[X]` as `\u{2612}`). The parser keeps `[ ]`/`[x]`/`[X]` as a
/// dedicated `TASK_CHECKBOX` token on the `LIST_ITEM`; this helper returns
/// the matching ballot-box glyph if one is present.
fn task_checkbox_for_item(item: &SyntaxNode) -> Option<&'static str> {
    item.children_with_tokens()
        .filter_map(|el| el.into_token())
        .find(|t| t.kind() == SyntaxKind::TASK_CHECKBOX)
        .map(|t| {
            let text = t.text();
            if text.contains('x') || text.contains('X') {
                "\u{2612}"
            } else {
                "\u{2610}"
            }
        })
}

/// Number of leading-space columns each body-content line of `item` carries
/// in the CST. Mirrors pandoc's list-item content offset:
///   - bare-marker line (no WHITESPACE after LIST_MARKER): offset = marker
///     width (e.g. `1` for `-`, `2` for `1.`).
///   - marker followed by space(s): offset = marker width + WS width (the
///     visual column where content starts on the marker's line).
///
/// Nested list items also carry leading WHITESPACE *before* the LIST_MARKER
/// (the outer item's content offset). Include that so the cumulative depth
/// is captured — required for correctly stripping nested fenced/indented
/// code blocks.
fn list_item_content_offset(item: &SyntaxNode) -> usize {
    let mut marker_width = 0usize;
    let mut leading_ws = 0usize;
    let mut saw_marker = false;
    for el in item.children_with_tokens() {
        if let NodeOrToken::Token(t) = el {
            match t.kind() {
                SyntaxKind::WHITESPACE if !saw_marker => {
                    leading_ws += t.text().chars().count();
                }
                SyntaxKind::LIST_MARKER => {
                    marker_width += t.text().chars().count();
                    saw_marker = true;
                }
                SyntaxKind::WHITESPACE if saw_marker => {
                    return leading_ws + marker_width + t.text().chars().count();
                }
                _ if saw_marker => {
                    return leading_ws + marker_width;
                }
                _ => {}
            }
        } else if saw_marker {
            return leading_ws + marker_width;
        }
    }
    leading_ws + marker_width
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
        // Per CommonMark/pandoc: a list is loose if any item directly
        // contains a blank line between two block-level children. The
        // single-item form (`- a\n\n  b`) only manifests as a BLANK_LINE
        // sandwiched between non-blank block children inside the item.
        if has_internal_blank_between_blocks(&item) {
            return true;
        }
    }
    false
}

fn has_internal_blank_between_blocks(item: &SyntaxNode) -> bool {
    let mut saw_block_before = false;
    let mut pending_blank = false;
    for child in item.children() {
        match child.kind() {
            SyntaxKind::BLANK_LINE => {
                if saw_block_before {
                    pending_blank = true;
                }
            }
            // Bare-marker line emits an empty PLAIN (NEWLINE only); pandoc
            // doesn't count that as a block — its first real block is what
            // comes after the blank line.
            SyntaxKind::PLAIN if child_is_empty_plain(&child) => {}
            _ => {
                if pending_blank {
                    return true;
                }
                saw_block_before = true;
            }
        }
    }
    false
}

fn child_is_empty_plain(node: &SyntaxNode) -> bool {
    !node.children_with_tokens().any(|el| match el {
        NodeOrToken::Token(t) => !matches!(t.kind(), SyntaxKind::NEWLINE | SyntaxKind::WHITESPACE),
        NodeOrToken::Node(_) => true,
    })
}

// ----- inline walking -----------------------------------------------------

fn inlines_from(parent: &SyntaxNode) -> Vec<Inline> {
    let mut out = Vec::new();
    let mut iter = parent.children_with_tokens().peekable();
    while let Some(el) = iter.next() {
        match el {
            NodeOrToken::Token(t) => push_token_inline(&t, &mut out),
            NodeOrToken::Node(n) if n.kind() == SyntaxKind::LATEX_COMMAND => {
                emit_latex_command_with_absorb(&n, &mut iter, &mut out);
            }
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

/// Pandoc's tex inline reader absorbs trailing horizontal whitespace into the
/// raw command when (and only when) the command is `\letters` with no brace
/// arguments — `\foo bar` becomes `RawInline tex "\\foo "` + `Str "bar"`,
/// while `\frac{a}{b} bar` keeps the space outside (`RawInline tex
/// "\\frac{a}{b}"` + `Space` + `Str "bar"`). The discriminator is the last
/// byte of the command text: ASCII letter → absorb, otherwise → don't.
fn emit_latex_command_with_absorb<I>(
    node: &SyntaxNode,
    iter: &mut std::iter::Peekable<I>,
    out: &mut Vec<Inline>,
) where
    I: Iterator<Item = rowan::SyntaxElement<panache_parser::syntax::PanacheLanguage>>,
{
    let mut content = node.text().to_string();
    let ends_in_letter = content
        .chars()
        .next_back()
        .is_some_and(|c| c.is_ascii_alphabetic());
    if ends_in_letter
        && let Some(NodeOrToken::Token(t)) = iter.peek()
        && t.kind() == SyntaxKind::TEXT
    {
        let text = t.text().to_string();
        let bytes = text.as_bytes();
        let mut absorbed = 0;
        while absorbed < bytes.len() && (bytes[absorbed] == b' ' || bytes[absorbed] == b'\t') {
            absorbed += 1;
        }
        if absorbed > 0 {
            content.push_str(&text[..absorbed]);
            out.push(Inline::RawInline("tex".to_string(), content));
            iter.next();
            let remainder = &text[absorbed..];
            if !remainder.is_empty() {
                push_text(remainder, out);
            }
            return;
        }
    }
    out.push(Inline::RawInline("tex".to_string(), content));
}

fn push_inline_node(node: &SyntaxNode, out: &mut Vec<Inline>) {
    match node.kind() {
        SyntaxKind::LINK => render_link_inline(node, out),
        SyntaxKind::IMAGE_LINK => render_image_inline(node, out),
        SyntaxKind::CITATION => render_citation_inline(node, out),
        _ => out.push(inline_from_node(node)),
    }
}

/// Pandoc treats `(@label)` and bare `@label` as Example-list references
/// when the label was defined as an Example item; the inline becomes
/// `Str "N"` (just the digits — surrounding parens come from adjacent
/// source bytes which our coalesce pass merges back in). Citations
/// without an Example match still emit `Unsupported "CITATION"` until
/// general citation support lands.
fn render_citation_inline(node: &SyntaxNode, out: &mut Vec<Inline>) {
    let key = node
        .children_with_tokens()
        .filter_map(|el| el.into_token())
        .find(|t| t.kind() == SyntaxKind::CITATION_KEY)
        .map(|t| t.text().to_string())
        .unwrap_or_default();
    let resolved = REFS_CTX.with(|c| c.borrow().example_label_to_num.get(&key).copied());
    if let Some(n) = resolved {
        out.push(Inline::Str(n.to_string()));
    } else {
        out.push(Inline::Unsupported("CITATION".to_string()));
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
        SyntaxKind::EMPHASIS => {
            Inline::Emph(coalesce_inlines_keep_edges(inlines_from_marked(node)))
        }
        SyntaxKind::STRONG => {
            Inline::Strong(coalesce_inlines_keep_edges(inlines_from_marked(node)))
        }
        SyntaxKind::STRIKEOUT => {
            Inline::Strikeout(coalesce_inlines_keep_edges(inlines_from_marked(node)))
        }
        SyntaxKind::SUPERSCRIPT => {
            Inline::Superscript(coalesce_inlines_keep_edges(inlines_from_marked(node)))
        }
        SyntaxKind::SUBSCRIPT => {
            Inline::Subscript(coalesce_inlines_keep_edges(inlines_from_marked(node)))
        }
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
    let mut iter = parent.children_with_tokens().peekable();
    while let Some(el) = iter.next() {
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
                _ if n.kind() == SyntaxKind::LATEX_COMMAND => {
                    emit_latex_command_with_absorb(&n, &mut iter, &mut out);
                }
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
        out.push(Inline::Link(extract_attr_from_node(node), text, url, title));
        return;
    }

    // Reference-style link: shortcut [label], implicit [label][], or full
    // [text][ref]. Distinguish by presence/contents of LINK_REF.
    let ref_node = node.children().find(|c| c.kind() == SyntaxKind::LINK_REF);
    let resolved_text_inlines = text_node
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
        out.push(Inline::Link(
            extract_attr_from_node(node),
            resolved_text_inlines,
            url,
            title,
        ));
        return;
    }

    if let Some(id) = lookup_heading_id(&label) {
        let url = format!("#{id}");
        out.push(Inline::Link(
            extract_attr_from_node(node),
            resolved_text_inlines,
            url,
            String::new(),
        ));
        return;
    }

    // Unresolved: emit the original markdown bytes as plain text. The reader
    // assembles `[<text>]`, optionally followed by `[<ref>]` for a full or
    // implicit reference. Using Str inlines here (rather than Link with empty
    // dest) matches pandoc's behavior of leaving unresolved references as raw
    // text in the output stream. Use keep_edges so leading/trailing whitespace
    // inside `[ ... ]` survives — pandoc preserves source whitespace for
    // unresolved references (`[ foo ]` → `Str "[", Space, Str "foo", Space,
    // Str "]"`), unlike resolved Links which strip edges.
    let unresolved_text_inlines = text_node
        .as_ref()
        .map(|n| coalesce_inlines_keep_edges(inlines_from(n)))
        .unwrap_or_default();
    out.push(Inline::Str("[".to_string()));
    out.extend(unresolved_text_inlines);
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
        out.push(Inline::Image(extract_attr_from_node(node), alt, url, title));
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
        out.push(Inline::Image(
            extract_attr_from_node(node),
            alt_inlines,
            url,
            title,
        ));
        return;
    }

    if let Some(id) = lookup_heading_id(&label) {
        let url = format!("#{id}");
        out.push(Inline::Image(
            extract_attr_from_node(node),
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

/// Pandoc's inline code reader (`Markdown.hs::code`) replaces internal
/// newlines with spaces (each `\n` → one space) and then `trim`s leading
/// and trailing whitespace from the result. Internal whitespace runs are
/// preserved.
fn strip_inline_code_padding(s: &str) -> String {
    let collapsed: String = s.chars().map(|c| if c == '\n' { ' ' } else { c }).collect();
    collapsed.trim().to_string()
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
    // Pandoc only treats `<scheme:body>` as a URI autolink when `scheme` is
    // in its known-schemes allowlist (see pandoc/src/Text/Pandoc/URI.hs).
    // Otherwise the original `<...>` bytes are emitted as raw HTML.
    if !is_known_uri_scheme(&url) {
        return Inline::RawInline("html".to_string(), node.text().to_string());
    }
    let attr = Attr {
        id: String::new(),
        classes: vec!["uri".to_string()],
        kvs: Vec::new(),
    };
    Inline::Link(attr, vec![Inline::Str(url.clone())], url, String::new())
}

/// Pandoc's URI scheme allowlist (IANA + a few unofficial ones). Mirrors
/// `pandoc/src/Text/Pandoc/URI.hs`. Lowercase comparison.
fn is_known_uri_scheme(url: &str) -> bool {
    let scheme_end = url.find(':');
    let Some(end) = scheme_end else {
        return false;
    };
    let scheme = url[..end].to_ascii_lowercase();
    PANDOC_KNOWN_SCHEMES.binary_search(&scheme.as_str()).is_ok()
}

/// Pandoc-known URI schemes, sorted for `binary_search`. Mirrors
/// `pandoc/src/Text/Pandoc/URI.hs`'s `schemes` set.
#[rustfmt::skip]
const PANDOC_KNOWN_SCHEMES: &[&str] = &[
    "aaa", "aaas", "about", "acap", "acct", "acr",
    "adiumxtra", "afp", "afs", "aim", "appdata", "apt",
    "attachment", "aw", "barion", "beshare", "bitcoin", "blob",
    "bolo", "browserext", "callto", "cap", "chrome", "chrome-extension",
    "cid", "coap", "coaps", "com-eventbrite-attendee", "content", "crid",
    "cvs", "data", "dav", "dict", "dis", "dlna-playcontainer",
    "dlna-playsingle", "dns", "dntp", "doi", "dtn", "dvb",
    "ed2k", "example", "facetime", "fax", "feed", "feedready",
    "file", "filesystem", "finger", "fish", "ftp", "gemini",
    "geo", "gg", "git", "gizmoproject", "go", "gopher",
    "graph", "gtalk", "h323", "ham", "hcp", "http",
    "https", "hxxp", "hxxps", "hydrazone", "iax", "icap",
    "icon", "im", "imap", "info", "iotdisco", "ipn",
    "ipp", "ipps", "irc", "irc6", "ircs", "iris",
    "iris.beep", "iris.lwz", "iris.xpc", "iris.xpcs", "isbn", "isostore",
    "itms", "jabber", "jar", "javascript", "jms", "keyparc",
    "lastfm", "ldap", "ldaps", "lvlt", "magnet", "mailserver",
    "mailto", "maps", "market", "message", "mid", "mms",
    "modem", "mongodb", "moz", "ms-access", "ms-browser-extension", "ms-drive-to",
    "ms-enrollment", "ms-excel", "ms-gamebarservices", "ms-getoffice", "ms-help", "ms-infopath",
    "ms-media-stream-id", "ms-officeapp", "ms-powerpoint", "ms-project", "ms-publisher", "ms-search-repair",
    "ms-secondary-screen-controller", "ms-secondary-screen-setup", "ms-settings", "ms-settings-airplanemode", "ms-settings-bluetooth", "ms-settings-camera",
    "ms-settings-cellular", "ms-settings-cloudstorage", "ms-settings-connectabledevices", "ms-settings-displays-topology", "ms-settings-emailandaccounts", "ms-settings-language",
    "ms-settings-location", "ms-settings-lock", "ms-settings-nfctransactions", "ms-settings-notifications", "ms-settings-power", "ms-settings-privacy",
    "ms-settings-proximity", "ms-settings-screenrotation", "ms-settings-wifi", "ms-settings-workplace", "ms-spd", "ms-sttoverlay",
    "ms-transit-to", "ms-virtualtouchpad", "ms-visio", "ms-walk-to", "ms-whiteboard", "ms-whiteboard-cmd",
    "ms-word", "msnim", "msrp", "msrps", "mtqp", "mumble",
    "mupdate", "mvn", "news", "nfs", "ni", "nih",
    "nntp", "notes", "ocf", "oid", "onenote", "onenote-cmd",
    "opaquelocktoken", "pack", "palm", "paparazzi", "pkcs11", "platform",
    "pmid", "pop", "pres", "prospero", "proxy", "psyc",
    "pwid", "qb", "query", "redis", "rediss", "reload",
    "res", "resource", "rmi", "rsync", "rtmfp", "rtmp",
    "rtsp", "rtsps", "rtspu", "secondlife", "service", "session",
    "sftp", "sgn", "shttp", "sieve", "sip", "sips",
    "skype", "smb", "sms", "smtp", "snews", "snmp",
    "soap.beep", "soap.beeps", "soldat", "spotify", "ssh", "steam",
    "stun", "stuns", "submit", "svn", "tag", "teamspeak",
    "tel", "teliaeid", "telnet", "tftp", "things", "thismessage",
    "tip", "tn3270", "tool", "turn", "turns", "tv",
    "udp", "unreal", "urn", "ut2004", "v-event", "vemmi",
    "ventrilo", "videotex", "view-source", "vnc", "wais", "webcal",
    "wpid", "ws", "wss", "wtai", "wyciwyg", "xcon",
    "xcon-userid", "xfire", "xmlrpc.beep", "xmlrpc.beeps", "xmpp", "xri",
    "ymsgr", "z39.50", "z39.50r", "z39.50s",
];

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
    // LINK_DEST holds the raw bytes between `(` and `)`. Split into URL and
    // optional quoted title, then percent-escape unsafe characters in the URL
    // to match pandoc's `escapeURI`.
    let raw = node.text().to_string();
    let trimmed = raw.trim();
    // `<URL>` form: pandoc strips the angle brackets, even if the URL
    // contains otherwise-ambiguous characters like spaces or parens.
    if let Some(rest) = trimmed.strip_prefix('<')
        && let Some(end) = rest.find('>')
    {
        let url = &rest[..end];
        let after = rest[end + 1..].trim();
        let title = parse_dest_title(after);
        return (escape_link_dest(url), title);
    }
    // URL/title boundary: a title starts with `"`, `'`, or `(` after
    // whitespace. Without one, the entire string is the URL — internal
    // spaces still get percent-escaped.
    let bytes = trimmed.as_bytes();
    let mut url_end = trimmed.len();
    let mut i = 0;
    while i < bytes.len() {
        if matches!(bytes[i], b' ' | b'\t' | b'\n') {
            let mut j = i;
            while j < bytes.len() && matches!(bytes[j], b' ' | b'\t' | b'\n') {
                j += 1;
            }
            if j < bytes.len() && matches!(bytes[j], b'"' | b'\'' | b'(') {
                url_end = i;
                break;
            }
            i = j;
        } else {
            i += 1;
        }
    }
    let url_raw = &trimmed[..url_end];
    let title = parse_dest_title(trimmed[url_end..].trim());
    (escape_link_dest(url_raw), title)
}

/// Mirrors pandoc's `escapeURI`: percent-escape ASCII whitespace and the
/// punctuation `<>|"{}[]^\``. Other ASCII and all non-ASCII chars are
/// preserved as-is.
fn escape_link_dest(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        let needs_escape = ch.is_whitespace()
            || matches!(
                ch,
                '<' | '>' | '|' | '"' | '{' | '}' | '[' | ']' | '^' | '`'
            );
        if needs_escape {
            let mut buf = [0u8; 4];
            for &b in ch.encode_utf8(&mut buf).as_bytes() {
                out.push_str(&format!("%{b:02X}"));
            }
        } else {
            out.push(ch);
        }
    }
    out
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
    coalesce_inlines_inner(input, true)
}

/// Inside markup atoms (Emph/Strong/Strikeout/Sup/Sub), pandoc preserves
/// leading/trailing whitespace inside the wrapper — e.g. `*foo bar *` projects
/// as `Emph [Str "foo", Space, Str "bar", Space]`. Block-level paragraphs and
/// headers strip edge whitespace, but inline markup wrappers do not.
fn coalesce_inlines_keep_edges(input: Vec<Inline>) -> Vec<Inline> {
    coalesce_inlines_inner(input, false)
}

fn coalesce_inlines_inner(input: Vec<Inline>, trim_edges: bool) -> Vec<Inline> {
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
    if trim_edges {
        // Trim leading/trailing Space/SoftBreak — pandoc does not emit edge
        // whitespace inside a paragraph or header.
        while matches!(out.first(), Some(Inline::Space) | Some(Inline::SoftBreak)) {
            out.remove(0);
        }
        while matches!(out.last(), Some(Inline::Space) | Some(Inline::SoftBreak)) {
            out.pop();
        }
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
    let out = smart_quote_pairs(out);
    apply_abbreviations(out)
}

/// Pandoc's default abbreviation list (from `pandoc/data/abbreviations`).
/// When a Str token *exactly equal to* one of these (i.e. the abbrev is a
/// suffix of the projected Str preceded by a non-letter / non-dot char or the
/// start of the Str) is followed by a `Space`, pandoc replaces the space with
/// a non-breaking space appended to the Str. Sorted to allow `binary_search`.
const PANDOC_ABBREVIATIONS: &[&str] = &[
    "Apr.", "Aug.", "Bros.", "Capt.", "Co.", "Corp.", "Dec.", "Dr.", "Feb.", "Fr.", "Gen.", "Gov.",
    "Hon.", "Inc.", "Jan.", "Jr.", "Jul.", "Jun.", "Ltd.", "M.A.", "M.D.", "Mar.", "Mr.", "Mrs.",
    "Ms.", "No.", "Nov.", "Oct.", "Ph.D.", "Pres.", "Prof.", "Rep.", "Rev.", "Sen.", "Sep.",
    "Sept.", "Sgt.", "Sr.", "St.", "aet.", "aetat.", "al.", "bk.", "c.", "cf.", "ch.", "chap.",
    "chs.", "col.", "cp.", "d.", "e.g.", "ed.", "eds.", "esp.", "f.", "fasc.", "ff.", "fig.",
    "fl.", "fol.", "fols.", "i.e.", "ill.", "incl.", "n.", "n.b.", "nn.", "p.", "pp.", "pt.",
    "q.v.", "s.v.", "s.vv.", "saec.", "sec.", "univ.", "viz.", "vol.", "vs.",
];

fn matches_abbreviation_suffix(s: &str) -> bool {
    for &abbr in PANDOC_ABBREVIATIONS {
        if let Some(prefix) = s.strip_suffix(abbr) {
            if prefix.is_empty() {
                return true;
            }
            let last = prefix.chars().next_back().unwrap();
            if !last.is_alphanumeric() && last != '.' {
                return true;
            }
        }
    }
    false
}

/// Apply pandoc's `+abbreviations` extension as a post-pass over a flat inline
/// list. For each `Str` ending in a known abbreviation followed by `Space`,
/// drop the `Space`, append `\u{a0}` (NBSP) to the `Str`, and merge the
/// following `Str` (if any) into it. Recurses into `Quoted` content because
/// `Quoted` is built inside `smart_quote_pairs` after the parent
/// `coalesce_inlines_inner` already ran on its source list, so its content
/// won't have been abbreviation-processed yet. Other inline wrappers (`Emph`,
/// `Strong`, `Link`, `Image`, `Note`, …) are constructed via their own
/// `coalesce_inlines_*` call, so their contents are already processed.
fn apply_abbreviations(inlines: Vec<Inline>) -> Vec<Inline> {
    let inlines: Vec<Inline> = inlines
        .into_iter()
        .map(|inline| match inline {
            Inline::Quoted(kind, content) => Inline::Quoted(kind, apply_abbreviations(content)),
            other => other,
        })
        .collect();
    let mut out: Vec<Inline> = Vec::with_capacity(inlines.len());
    let mut iter = inlines.into_iter().peekable();
    while let Some(inline) = iter.next() {
        if let Inline::Str(ref s) = inline
            && matches_abbreviation_suffix(s)
            && matches!(iter.peek(), Some(Inline::Space))
        {
            // Drop the Space.
            iter.next();
            let Inline::Str(mut new_s) = inline else {
                unreachable!()
            };
            new_s.push('\u{a0}');
            // Merge with the following Str if present.
            if let Some(Inline::Str(_)) = iter.peek()
                && let Some(Inline::Str(next_s)) = iter.next()
            {
                new_s.push_str(&next_s);
            }
            out.push(Inline::Str(new_s));
        } else {
            out.push(inline);
        }
    }
    out
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
        // Open quote condition: previous inline is boundary, AND either
        // (a) the Str has more chars after the quote and the next char is
        //     non-space (open quote attaches to a word in the same Str), or
        // (b) the Str is *only* the quote and the next inline is a markup
        //     atom (Emph/Strong/...), so the quote attaches across atoms.
        let prev_is_boundary = is_boundary(out.last());
        let str_has_more = s.chars().count() > 1;
        let next_char_is_word = s.chars().nth(1).is_some_and(|c| !c.is_whitespace());
        let next_is_markup_atom = matches!(
            inlines.get(i + 1),
            Some(
                Inline::Emph(_)
                    | Inline::Strong(_)
                    | Inline::Strikeout(_)
                    | Inline::Superscript(_)
                    | Inline::Subscript(_)
                    | Inline::Code(_, _)
            )
        );
        let attaches =
            (str_has_more && next_char_is_word) || (!str_has_more && next_is_markup_atom);
        if let Some(q) = quote
            && prev_is_boundary
            && attaches
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
                    if j == i && j == close_idx {
                        // Open and close in the same Str — strip both ends.
                        if let Inline::Str(s) = inline {
                            let mut chars: Vec<char> = s.chars().collect();
                            if chars.len() >= 2 {
                                chars.remove(0);
                                chars.pop();
                            }
                            let stripped: String = chars.into_iter().collect();
                            if !stripped.is_empty() {
                                content.push(Inline::Str(stripped));
                            }
                        }
                    } else if j == i {
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
        Block::Figure(a, caption, body) => Block::Figure(
            a.clone(),
            caption.iter().map(clone_block).collect(),
            body.iter().map(clone_block).collect(),
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
        Block::Figure(attr, caption, body) => {
            out.push_str("Figure (");
            write_attr(attr, out);
            out.push_str(") ( Caption Nothing [");
            write_block_list(caption, out);
            out.push_str(" ] ) [");
            write_block_list(body, out);
            out.push_str(" ]");
        }
        Block::Unsupported(name) => {
            out.push_str(&format!("Unsupported {name:?}"));
        }
    }
}

fn write_table(data: &TableData, out: &mut String) {
    out.push_str("Table (");
    write_attr(&data.attr, out);
    out.push_str(") ( Caption Nothing [");
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
        let width = data.widths.get(i).copied().unwrap_or(None);
        match width {
            None => out.push_str(&format!(" ( {align} , ColWidthDefault )")),
            Some(w) => out.push_str(&format!(" ( {align} , ColWidth {} )", show_double(w))),
        }
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

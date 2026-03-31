use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_lsp_server::ls_types::{Location, Range, Uri};

use crate::Config;
use crate::lsp::DocumentState;
use crate::salsa::Db;
use crate::syntax::{
    AstNode, AttributeNode, Citation, CodeBlock, CodeSpan, Crossref, FootnoteReference, ImageLink,
    InlineMath, Link, LinkRef, ParsedYamlRegionSnapshot, SyntaxKind, SyntaxNode,
};
use crate::utils::{normalize_anchor_label, normalize_label};
use rowan::{NodeOrToken, TextRange, TextSize};

use super::config::load_config;

/// Helper to get document content from the document map
pub(crate) async fn get_document_content(
    document_map: &Arc<Mutex<HashMap<String, DocumentState>>>,
    salsa_db: &Arc<Mutex<crate::salsa::SalsaDb>>,
    uri: &Uri,
) -> Option<String> {
    let state = {
        let doc_map = document_map.lock().await;
        doc_map.get(&uri.to_string())?.clone()
    };
    let db = salsa_db.lock().await;
    Some(state.salsa_file.text(&*db).clone())
}

/// Helper to get document content and tree from the document map
pub(crate) async fn get_document_content_and_tree(
    document_map: &Arc<Mutex<HashMap<String, DocumentState>>>,
    salsa_db: &Arc<Mutex<crate::salsa::SalsaDb>>,
    uri: &Uri,
) -> Option<(String, SyntaxNode)> {
    let state = {
        let doc_map = document_map.lock().await;
        doc_map.get(&uri.to_string())?.clone()
    };
    let db = salsa_db.lock().await;
    Some((
        state.salsa_file.text(&*db).clone(),
        SyntaxNode::new_root(state.tree.clone()),
    ))
}

/// Helper to load config with URI-based flavor detection
pub(crate) async fn get_config(
    client: &tower_lsp_server::Client,
    workspace_root: &Arc<Mutex<Option<PathBuf>>>,
    uri: &Uri,
) -> Config {
    let workspace_root = workspace_root.lock().await.clone();
    load_config(client, &workspace_root, Some(uri)).await
}

/// Combined helper: get document and config in one call
pub(crate) async fn get_document_and_config(
    client: &tower_lsp_server::Client,
    document_map: &Arc<Mutex<HashMap<String, DocumentState>>>,
    salsa_db: &Arc<Mutex<crate::salsa::SalsaDb>>,
    workspace_root: &Arc<Mutex<Option<PathBuf>>>,
    uri: &Uri,
) -> Option<(String, Config)> {
    let content = get_document_content(document_map, salsa_db, uri).await?;
    let config = get_config(client, workspace_root, uri).await;
    Some((content, config))
}

pub(crate) async fn get_definition_index_with_includes(
    document_map: &Arc<Mutex<HashMap<String, DocumentState>>>,
    salsa_db: &Arc<Mutex<crate::salsa::SalsaDb>>,
    uri: &Uri,
) -> crate::salsa::DefinitionIndex {
    let (salsa_file, salsa_config, root_path) = {
        let doc_map = document_map.lock().await;
        let Some(state) = doc_map.get(&uri.to_string()) else {
            return crate::salsa::DefinitionIndex::default();
        };
        let root_path = state
            .path
            .clone()
            .unwrap_or_else(|| PathBuf::from("<memory>"));
        (state.salsa_file, state.salsa_config, root_path)
    };
    let db = salsa_db.lock().await;
    let graph =
        crate::salsa::project_graph(&*db, salsa_file, salsa_config, root_path.clone()).clone();
    let mut index =
        crate::salsa::definition_index(&*db, salsa_file, salsa_config, root_path).clone();
    for path in graph.documents().iter() {
        if let Some(include_file) = db.file_text(path.clone()) {
            let include_index =
                crate::salsa::definition_index(&*db, include_file, salsa_config, path.clone());
            index.merge_from(include_index);
        }
    }
    index
}

pub(crate) fn citation_definition_locations(
    index: &crate::salsa::CitationDefinitionIndex,
    key: &str,
    default_uri: &Uri,
    default_content: &str,
    db: &dyn crate::salsa::Db,
) -> Vec<Location> {
    let mut out = Vec::new();
    let norm = normalize_label(key);
    if let Some(entries) = index.by_key(&norm) {
        for entry in entries {
            let entry_uri = Uri::from_file_path(&entry.path).unwrap_or_else(|| default_uri.clone());
            let text = if entry_uri == *default_uri {
                default_content.to_string()
            } else {
                db.file_text(entry.path.clone())
                    .map(|file| file.text(db).clone())
                    .unwrap_or_default()
            };
            out.push(Location {
                uri: entry_uri,
                range: Range {
                    start: crate::lsp::conversions::offset_to_position(
                        &text,
                        entry.range.start().into(),
                    ),
                    end: crate::lsp::conversions::offset_to_position(
                        &text,
                        entry.range.end().into(),
                    ),
                },
            });
        }
    }

    out.sort_by(|a, b| {
        a.uri
            .as_str()
            .cmp(b.uri.as_str())
            .then(a.range.start.line.cmp(&b.range.start.line))
            .then(a.range.start.character.cmp(&b.range.start.character))
            .then(a.range.end.line.cmp(&b.range.end.line))
            .then(a.range.end.character.cmp(&b.range.end.character))
    });
    out.dedup_by(|a, b| a.uri == b.uri && a.range == b.range);
    out
}

/// Find the syntax node at the given byte offset
pub(crate) fn find_node_at_offset(root: &SyntaxNode, offset: usize) -> Option<SyntaxNode> {
    let text_size = TextSize::from(offset as u32);
    let range = TextRange::new(text_size, text_size);
    match root.covering_element(range) {
        NodeOrToken::Node(node) => Some(node),
        NodeOrToken::Token(token) => token.parent(),
    }
}

pub(crate) fn is_offset_in_yaml_frontmatter(
    parsed_yaml_regions: &[ParsedYamlRegionSnapshot],
    offset: usize,
) -> bool {
    parsed_yaml_regions
        .iter()
        .find(|region| region.is_frontmatter())
        .is_some_and(|frontmatter| {
            let range = frontmatter.host_range();
            range.start <= offset && offset < range.end
        })
}

pub(crate) fn is_yaml_frontmatter_valid(parsed_yaml_regions: &[ParsedYamlRegionSnapshot]) -> bool {
    parsed_yaml_regions
        .iter()
        .find(|region| region.is_frontmatter())
        .is_none_or(ParsedYamlRegionSnapshot::is_valid)
}

/// Extract the reference label from a LinkRef or FootnoteReference node
pub(crate) fn extract_reference_label(node: &SyntaxNode) -> Option<(String, bool)> {
    if let Some(link_ref) = LinkRef::cast(node.clone()) {
        return Some((normalize_label(&link_ref.label()), false));
    }

    if let Some(footnote_ref) = FootnoteReference::cast(node.clone()) {
        let id = footnote_ref.id();
        if !id.is_empty() {
            return Some((normalize_label(&id), true));
        }
    }

    None
}

pub(crate) fn extract_reference_target(node: &SyntaxNode) -> Option<(String, bool)> {
    if let Some(reference) = extract_reference_label(node) {
        return Some(reference);
    }

    if let Some(link) = Link::cast(node.clone())
        && let Some(link_ref) = link.reference()
    {
        return extract_reference_label(link_ref.syntax());
    }

    if let Some(image) = ImageLink::cast(node.clone())
        && let Some(label) = image.reference_label()
    {
        return Some((normalize_label(&label), false));
    }

    None
}

pub(crate) fn extract_citation_key(node: &SyntaxNode) -> Option<String> {
    node_and_ancestors(node)
        .find_map(Citation::cast)
        .and_then(|citation| citation.keys().first().map(|key| key.text()))
}

pub(crate) fn extract_crossref_key(node: &SyntaxNode) -> Option<String> {
    node_and_ancestors(node)
        .find_map(Crossref::cast)
        .and_then(|crossref| crossref.keys().first().map(|key| key.text()))
}

pub(crate) fn extract_chunk_label_key(node: &SyntaxNode) -> Option<String> {
    chunk_label_entry_at_node(node).map(|entry| entry.value().to_string())
}

pub(crate) fn extract_attribute_id_key(node: &SyntaxNode) -> Option<String> {
    if let Some(attribute) = AttributeNode::cast(node.clone())
        && let Some(id) = attribute.id()
    {
        return Some(id);
    }

    let mut current = node.clone();
    while let Some(parent) = current.parent() {
        if let Some(attribute) = AttributeNode::cast(parent.clone())
            && let Some(id) = attribute.id()
        {
            return Some(id);
        }
        current = parent;
    }

    None
}

pub(crate) fn extract_heading_id_key(node: &SyntaxNode) -> Option<String> {
    if let Some(attribute) = AttributeNode::cast(node.clone())
        && let Some(id) = attribute.id()
        && attribute_has_heading_ancestor(attribute.syntax())
    {
        return Some(normalize_anchor_label(&id));
    }

    let mut current = node.clone();
    while let Some(parent) = current.parent() {
        if let Some(attribute) = AttributeNode::cast(parent.clone())
            && let Some(id) = attribute.id()
            && attribute_has_heading_ancestor(attribute.syntax())
        {
            return Some(normalize_anchor_label(&id));
        }
        current = parent;
    }

    None
}

pub(crate) fn extract_heading_link_target(node: &SyntaxNode) -> Option<String> {
    if let Some(link) = Link::cast(node.clone()) {
        return heading_target_from_link(&link);
    }

    let mut current = node.clone();
    while let Some(parent) = current.parent() {
        if let Some(link) = Link::cast(parent.clone()) {
            return heading_target_from_link(&link);
        }
        current = parent;
    }

    None
}

pub(crate) fn extract_example_label_target_at_offset(
    root: &SyntaxNode,
    offset: usize,
) -> Option<String> {
    let text = root.text().to_string();
    if offset > text.len() {
        return None;
    }
    example_label_at_offset(&text, offset).map(normalize_label)
}

pub(crate) fn extract_symbol_text_range(node: &SyntaxNode) -> Option<TextRange> {
    if let Some(crossref) = Crossref::cast(node.clone()) {
        return crossref.keys().first().map(|key| key.text_range());
    }
    if let Some(citation) = Citation::cast(node.clone()) {
        return citation.keys().first().map(|key| key.text_range());
    }
    if let Some(entry) = chunk_label_entry_at_node(node) {
        return Some(entry.value_range());
    }
    if let Some(attribute) = AttributeNode::cast(node.clone())
        && attribute.id().is_some()
    {
        return attribute.id_value_range();
    }
    if let Some(link) = Link::cast(node.clone()) {
        if let Some(dest) = link.dest() {
            return dest.hash_anchor_id_range();
        }
        if let Some(link_ref) = link.reference() {
            return link_ref.label_value_range();
        }
        if link.reference().is_none() {
            return link.text().map(|text| text.syntax().text_range());
        }
    }

    if let Some(math) = InlineMath::cast(node.clone())
        && let Some(range) = math.content_range()
    {
        return Some(range);
    }

    if let Some(code) = CodeSpan::cast(node.clone())
        && let Some(range) = code.content_range()
    {
        return Some(range);
    }

    if let Some(image) = ImageLink::cast(node.clone())
        && let Some(range) = image.reference_label_range()
    {
        return Some(range);
    }

    if let Some(footnote_ref) = FootnoteReference::cast(node.clone()) {
        return footnote_ref
            .id_value_range()
            .or_else(|| Some(footnote_ref.id_range()));
    }

    None
}

pub(crate) fn example_label_range_at_offset(root: &SyntaxNode, offset: usize) -> Option<TextRange> {
    let text = root.text().to_string();
    if offset > text.len() {
        return None;
    }
    let (start, label) = example_label_span_at_offset(&text, offset)?;
    let label_start = rowan::TextSize::from((start + 2) as u32);
    let label_end = rowan::TextSize::from((start + 2 + label.len()) as u32);
    Some(TextRange::new(label_start, label_end))
}

pub(crate) fn find_symbol_text_range_at_offset(
    root: &SyntaxNode,
    offset: usize,
) -> Option<TextRange> {
    let mut node = find_node_at_offset(root, offset)?;

    loop {
        if let Some(range) = extract_symbol_text_range(&node) {
            return Some(range);
        }
        node = node.parent()?;
    }
}

fn attribute_has_heading_ancestor(node: &SyntaxNode) -> bool {
    node.ancestors()
        .any(|ancestor| ancestor.kind() == SyntaxKind::HEADING)
}

fn node_and_ancestors(node: &SyntaxNode) -> impl Iterator<Item = SyntaxNode> {
    std::iter::once(node.clone()).chain(node.ancestors())
}

fn heading_target_from_link(link: &Link) -> Option<String> {
    if let Some(dest) = link.dest() {
        let id = normalize_anchor_label(&dest.hash_anchor_id()?);
        return (!id.is_empty()).then_some(id);
    }

    if link.reference().is_none()
        && let Some(text) = link.text()
    {
        let label = normalize_label(&text.text_content());
        return (!label.is_empty()).then_some(label);
    }

    None
}

fn example_label_spans(text: &str) -> impl Iterator<Item = (usize, &str)> {
    text.char_indices().filter_map(|(idx, ch)| {
        if ch != '(' {
            return None;
        }
        let slice = &text[idx..];
        let rest = slice.strip_prefix("(@")?;
        let label_end = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
            .count();
        if label_end == 0 {
            return None;
        }
        if rest.chars().nth(label_end) != Some(')') {
            return None;
        }
        Some((idx, &rest[..label_end]))
    })
}

fn example_label_at_offset(text: &str, offset: usize) -> Option<&str> {
    example_label_span_at_offset(text, offset).map(|(_, label)| label)
}

fn example_label_span_at_offset(text: &str, offset: usize) -> Option<(usize, &str)> {
    let start = offset.saturating_sub(128);
    let end = (offset + 128).min(text.len());
    let window = text.get(start..end)?;
    let rel_offset = offset - start;

    for (idx, label) in example_label_spans(window) {
        let label_start = idx + 2;
        let label_end = label_start + label.len();
        if label_start <= rel_offset && rel_offset <= label_end {
            return Some((start + idx, label));
        }
    }

    None
}

fn chunk_label_entry_at_node(node: &SyntaxNode) -> Option<crate::syntax::ChunkLabelEntry> {
    let node_range = node.text_range();
    let block = node_and_ancestors(node).find_map(CodeBlock::cast)?;
    block.chunk_label_entries().into_iter().find(|entry| {
        let declaration = entry.declaration_range();
        let value = entry.value_range();
        text_range_contains(declaration, node_range) || text_range_contains(value, node_range)
    })
}

fn text_range_contains(outer: TextRange, inner: TextRange) -> bool {
    outer.start() <= inner.start() && inner.end() <= outer.end()
}

#[cfg(test)]
fn extract_reference_definition_label(node: &SyntaxNode) -> Option<String> {
    crate::syntax::ReferenceDefinition::cast(node.clone())
        .map(|def| normalize_label(&def.label()))
        .filter(|label| !label.is_empty())
}

/// Extract the label from a definition node (ReferenceDefinition or FootnoteDefinition)
#[cfg(test)]
fn extract_definition_label(node: &SyntaxNode) -> Option<String> {
    match node.kind() {
        SyntaxKind::REFERENCE_DEFINITION => extract_reference_definition_label(node),
        SyntaxKind::FOOTNOTE_DEFINITION => crate::syntax::FootnoteDefinition::cast(node.clone())
            .map(|def| normalize_label(&def.id()))
            .filter(|label| !label.is_empty()),
        _ => None,
    }
}

/// Find a definition node matching the given label
#[cfg(test)]
pub(crate) fn find_definition_node(
    root: &SyntaxNode,
    label: &str,
    is_footnote: bool,
) -> Option<SyntaxNode> {
    let target_kind = if is_footnote {
        SyntaxKind::FOOTNOTE_DEFINITION
    } else {
        SyntaxKind::REFERENCE_DEFINITION
    };

    root.descendants().find(|node| {
        node.kind() == target_kind && extract_definition_label(node).as_deref() == Some(label)
    })
}

/// Find the definition for a reference at the given offset
/// Returns the TextRange of the definition if found
#[cfg(test)]
pub(crate) fn find_definition_at_offset(root: &SyntaxNode, offset: usize) -> Option<TextRange> {
    // Find the node at this offset
    let mut node = find_node_at_offset(root, offset)?;

    // Walk up the tree to find a reference node
    loop {
        if let Some((label, is_footnote)) = extract_reference_target(&node) {
            // Found a reference - now find its definition
            let definition = find_definition_node(root, &label, is_footnote)?;
            return Some(definition.text_range());
        }

        // Move up to parent
        node = node.parent()?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to parse a document for testing
    fn parse(input: &str) -> SyntaxNode {
        crate::parse(input, None)
    }

    #[test]
    fn test_extract_symbol_text_range_for_inline_math_content() {
        let input = "Text with $x^2$ inline math";
        let root = parse(input);
        let node = root
            .descendants()
            .find_map(InlineMath::cast)
            .expect("inline math");

        let range = extract_symbol_text_range(node.syntax()).expect("content range");
        let start: usize = range.start().into();
        let end: usize = range.end().into();
        assert_eq!(&input[start..end], "x^2");
    }

    #[test]
    fn test_extract_symbol_text_range_for_code_span_content() {
        let input = "Use `fmt` command";
        let root = parse(input);
        let node = root
            .descendants()
            .find_map(CodeSpan::cast)
            .expect("code span");

        let range = extract_symbol_text_range(node.syntax()).expect("content range");
        let start: usize = range.start().into();
        let end: usize = range.end().into();
        assert_eq!(&input[start..end], "fmt");
    }

    #[test]
    fn test_find_node_at_offset() {
        let root = parse("[text][ref]");

        // Offset 0: at "["
        let node = find_node_at_offset(&root, 0);
        assert!(node.is_some());

        // Offset 7: at "r" in "ref"
        let node = find_node_at_offset(&root, 7);
        assert!(node.is_some());
    }

    #[test]
    fn test_normalize_label() {
        assert_eq!(crate::utils::normalize_label("Foo"), "foo");
        assert_eq!(crate::utils::normalize_label("foo bar"), "foo bar");
        assert_eq!(crate::utils::normalize_label("foo  bar"), "foo bar");
        assert_eq!(crate::utils::normalize_label(" foo bar "), "foo bar");
    }

    #[test]
    fn test_extract_reference_label_from_link_ref() {
        let root = parse("[text][ref]");
        let link_ref = root
            .descendants()
            .find_map(LinkRef::cast)
            .expect("Should find LinkRef");

        let (label, is_footnote) =
            extract_reference_label(link_ref.syntax()).expect("Should extract label");
        assert_eq!(label, "ref");
        assert!(!is_footnote);
    }

    #[test]
    fn test_extract_reference_label_from_footnote() {
        let root = parse("[^1]");
        let footnote_ref = root
            .descendants()
            .find_map(FootnoteReference::cast)
            .expect("Should find FootnoteReference");

        let (label, is_footnote) =
            extract_reference_label(footnote_ref.syntax()).expect("Should extract label");
        assert_eq!(label, "1");
        assert!(is_footnote);
    }

    #[test]
    fn test_extract_symbol_text_range_for_footnote_id_value() {
        let input = "Text[^note] here";
        let root = parse(input);
        let node = root
            .descendants()
            .find_map(FootnoteReference::cast)
            .expect("footnote reference");

        let range = extract_symbol_text_range(node.syntax()).expect("id value range");
        let start: usize = range.start().into();
        let end: usize = range.end().into();
        assert_eq!(&input[start..end], "note");
    }

    #[test]
    fn test_extract_definition_label_from_reference() {
        let root = parse("[ref]: /url");
        let def = root
            .descendants()
            .find_map(crate::syntax::ReferenceDefinition::cast)
            .expect("Should find ReferenceDefinition");

        let label = extract_definition_label(def.syntax()).expect("Should extract label");
        assert_eq!(label, "ref");
    }

    #[test]
    fn test_extract_definition_label_from_footnote() {
        let root = parse("[^1]: content");
        let def = root
            .descendants()
            .find(|n| n.kind() == SyntaxKind::FOOTNOTE_DEFINITION)
            .expect("Should find FootnoteDefinition");

        let label = extract_definition_label(&def).expect("Should extract label");
        assert_eq!(label, "1");
    }

    #[test]
    fn test_find_definition_node_reference() {
        let root = parse("[text][ref]\n\n[ref]: /url");
        let def = find_definition_node(&root, "ref", false);
        assert!(def.is_some());
        assert_eq!(def.unwrap().kind(), SyntaxKind::REFERENCE_DEFINITION);
    }

    #[test]
    fn test_find_definition_node_case_insensitive() {
        let root = parse("[text][REF]\n\n[ref]: /url");
        let def = find_definition_node(&root, "ref", false);
        assert!(def.is_some());
    }

    #[test]
    fn test_find_definition_node_footnote() {
        let root = parse("Text[^1]\n\n[^1]: content");
        let def = find_definition_node(&root, "1", true);
        assert!(def.is_some());
        assert_eq!(def.unwrap().kind(), SyntaxKind::FOOTNOTE_DEFINITION);
    }

    #[test]
    fn test_find_definition_node_not_found() {
        let root = parse("[text][ref]");
        let def = find_definition_node(&root, "ref", false);
        assert!(def.is_none());
    }

    #[test]
    fn test_find_definition_at_offset_reference_link() {
        let input = "[text][ref]\n\n[ref]: /url";
        let root = parse(input);

        // Offset 7: at "r" in [ref]
        let range = find_definition_at_offset(&root, 7);
        assert!(range.is_some());

        let range = range.unwrap();
        let def_text = &input[range.start().into()..range.end().into()];
        assert!(def_text.contains("[ref]: /url"));
    }

    #[test]
    fn test_find_definition_at_offset_footnote() {
        let input = "Text[^1]\n\n[^1]: content";
        let root = parse(input);

        // Offset 5: at "[^1]"
        let range = find_definition_at_offset(&root, 5);
        assert!(range.is_some());

        let range = range.unwrap();
        let def_text = &input[range.start().into()..range.end().into()];
        assert!(def_text.contains("[^1]:"));
    }

    #[test]
    fn test_find_definition_at_offset_not_on_reference() {
        let root = parse("Just some text");
        let range = find_definition_at_offset(&root, 0);
        assert!(range.is_none());
    }

    #[test]
    fn test_find_definition_at_offset_reference_not_found() {
        let root = parse("[text][ref]");
        // Even though we're on a reference, there's no definition
        let range = find_definition_at_offset(&root, 7);
        assert!(range.is_none());
    }

    #[test]
    fn test_extract_citation_key_from_citation() {
        let root = parse("Text @woodward1952 more text");
        let citation = root
            .descendants()
            .find(|n| n.kind() == SyntaxKind::CITATION)
            .expect("Should find CITATION");

        let key = extract_citation_key(&citation).expect("Should extract citation key");
        assert_eq!(key, "woodward1952");
    }

    #[test]
    fn test_extract_citation_key_walks_up_tree() {
        let root = parse("Text @woodward1952 more text");

        // Find the CITATION node
        let citation = root
            .descendants()
            .find(|n| n.kind() == SyntaxKind::CITATION)
            .expect("Should find CITATION node");

        let key = extract_citation_key(&citation).expect("Should extract citation key");
        assert_eq!(key, "woodward1952");
    }

    #[test]
    fn test_find_definition_whitespace_normalization() {
        let input = "[text][foo  bar]\n\n[foo bar]: /url";
        let root = parse(input);

        // Offset 7: at "foo  bar" reference
        let range = find_definition_at_offset(&root, 7);
        assert!(range.is_some());
    }

    #[test]
    fn test_extract_example_label_target_at_offset() {
        let config = crate::config::Config {
            flavor: crate::config::Flavor::Pandoc,
            extensions: crate::config::Extensions::for_flavor(crate::config::Flavor::Pandoc),
            ..Default::default()
        };
        let input = "(@good) Example.\n\nAs (@good) illustrates.\n";
        let root = crate::parse(input, Some(config));
        let offset = input.rfind("good").expect("reference label");
        let label = extract_example_label_target_at_offset(&root, offset);
        assert_eq!(label, Some("good".to_string()));
    }

    #[test]
    fn test_extract_example_label_target_at_offset_ignores_citation() {
        let root = parse("See @good for details.\n");
        let offset = 5;
        let label = extract_example_label_target_at_offset(&root, offset);
        assert_eq!(label, None);
    }

    #[test]
    fn test_example_label_range_at_offset() {
        let config = crate::config::Config {
            flavor: crate::config::Flavor::Pandoc,
            extensions: crate::config::Extensions::for_flavor(crate::config::Flavor::Pandoc),
            ..Default::default()
        };
        let input = "As (@good) illustrates.\n";
        let root = crate::parse(input, Some(config));
        let offset = input.find("good").expect("label offset");
        let range = example_label_range_at_offset(&root, offset).expect("label range");
        let start: usize = range.start().into();
        let end: usize = range.end().into();
        assert_eq!(&input[start..end], "good");
    }
}

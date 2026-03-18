use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_lsp_server::ls_types::{Location, Range, Uri};

use crate::Config;
use crate::lsp::DocumentState;
use crate::parser::utils::attributes::try_parse_trailing_attributes;
use crate::salsa::Db;
use crate::syntax::{
    AstNode, ChunkOption, Citation, Crossref, ParsedYamlRegionSnapshot, SyntaxKind, SyntaxNode,
};
use crate::utils::pandoc_slugify;
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

/// Normalize a label for case-insensitive matching (collapses whitespace, lowercases)
fn normalize_label(label: &str) -> String {
    label
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn normalize_heading_text(text: &str) -> String {
    normalize_label(text)
}

fn implicit_header_id(text: &str) -> String {
    pandoc_slugify(text)
}

/// Extract the reference label from a LinkRef or FootnoteReference node
pub(crate) fn extract_reference_label(node: &SyntaxNode) -> Option<(String, bool)> {
    match node.kind() {
        SyntaxKind::LINK_REF => {
            // LinkRef contains TEXT child with the label
            let text = node
                .children_with_tokens()
                .filter_map(|child| child.into_token())
                .filter(|token| token.kind() == SyntaxKind::TEXT)
                .map(|token| token.text().to_string())
                .collect::<String>();
            Some((normalize_label(&text), false))
        }
        SyntaxKind::FOOTNOTE_REFERENCE => {
            // FootnoteReference has TEXT children: "[^", "id", "]"
            // Extract the middle TEXT token (the ID)
            let tokens: Vec<_> = node
                .children_with_tokens()
                .filter_map(|child| child.into_token())
                .filter(|token| token.kind() == SyntaxKind::TEXT)
                .map(|token| token.text().to_string())
                .collect();

            if tokens.len() >= 2 && tokens[0] == "[^" {
                // The ID is in the second token
                let id = &tokens[1];
                Some((normalize_label(id), true))
            } else {
                None
            }
        }
        _ => None,
    }
}

pub(crate) fn extract_citation_key(node: &SyntaxNode) -> Option<String> {
    // Try to cast the node itself as a Citation
    if let Some(citation) = Citation::cast(node.clone()) {
        // Return the first key (citations can have multiple keys)
        return citation.keys().first().map(|key| key.text());
    }

    // If the node is a CITATION_KEY token's parent, walk up to find CITATION
    let mut current = node.clone();
    while let Some(parent) = current.parent() {
        if let Some(citation) = Citation::cast(parent.clone()) {
            // Check if any of the citation's keys match the position we're at
            // For simplicity, return the first key
            return citation.keys().first().map(|key| key.text());
        }
        current = parent;
    }

    None
}

pub(crate) fn extract_crossref_key(node: &SyntaxNode) -> Option<String> {
    if let Some(crossref) = Crossref::cast(node.clone()) {
        return crossref.keys().first().map(|key| key.text());
    }

    let mut current = node.clone();
    while let Some(parent) = current.parent() {
        if let Some(crossref) = Crossref::cast(parent.clone()) {
            return crossref.keys().first().map(|key| key.text());
        }
        current = parent;
    }

    None
}

pub(crate) fn extract_chunk_label_key(node: &SyntaxNode) -> Option<String> {
    if let Some(option) = ChunkOption::cast(node.clone())
        && let (Some(key), Some(value)) = (option.key(), option.value())
        && key.eq_ignore_ascii_case("label")
    {
        return Some(value);
    }

    let mut current = node.clone();
    while let Some(parent) = current.parent() {
        if let Some(option) = ChunkOption::cast(parent.clone())
            && let (Some(key), Some(value)) = (option.key(), option.value())
            && key.eq_ignore_ascii_case("label")
        {
            return Some(value);
        }
        current = parent;
    }

    None
}

pub(crate) fn find_crossref_definition_node(root: &SyntaxNode, label: &str) -> Option<SyntaxNode> {
    let target = normalize_label(label);
    if let Some(node) = root.descendants().find(|node| {
        if node.kind() != SyntaxKind::ATTRIBUTE {
            return false;
        }
        let text = node.text().to_string();
        if let Some(attrs) = try_parse_trailing_attributes(&text).map(|(attrs, _)| attrs)
            && let Some(id) = attrs.identifier
        {
            return normalize_label(&id) == target;
        }
        false
    }) {
        return Some(node);
    }

    root.descendants().find(|node| {
        if let Some(option) = ChunkOption::cast(node.clone())
            && let (Some(key), Some(value)) = (option.key(), option.value())
            && key.eq_ignore_ascii_case("label")
        {
            return normalize_label(&value) == target;
        }
        false
    })
}

pub(crate) fn find_implicit_header_definition_node(
    root: &SyntaxNode,
    label: &str,
) -> Option<SyntaxNode> {
    let target = normalize_label(label);
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

    for node in root.descendants() {
        if node.kind() != SyntaxKind::HEADING {
            continue;
        }
        let Some(content) = node
            .children()
            .find(|child| child.kind() == SyntaxKind::HEADING_CONTENT)
        else {
            continue;
        };
        let raw_text = content
            .descendants_with_tokens()
            .filter_map(|it| it.into_token())
            .filter(|token| token.kind() == SyntaxKind::TEXT)
            .map(|token| token.text().to_string())
            .collect::<String>();
        let cleaned = normalize_heading_text(&raw_text);
        if cleaned.is_empty() {
            continue;
        }
        let base = implicit_header_id(&cleaned);
        if base.is_empty() {
            continue;
        }
        let count = seen.entry(base.clone()).or_insert(0);
        let id = if *count == 0 {
            base.clone()
        } else {
            format!("{}-{}", base, *count)
        };
        *count += 1;

        if normalize_label(&id) == target {
            return Some(node.clone());
        }
    }
    None
}

/// Extract the label from a definition node (ReferenceDefinition or FootnoteDefinition)
fn extract_definition_label(node: &SyntaxNode) -> Option<String> {
    match node.kind() {
        SyntaxKind::REFERENCE_DEFINITION => {
            // ReferenceDefinition has a Link child with LinkText containing the label
            node.children()
                .find(|child| child.kind() == SyntaxKind::LINK)
                .and_then(|link| {
                    link.children()
                        .find(|child| child.kind() == SyntaxKind::LINK_TEXT)
                })
                .map(|link_text| {
                    let text = link_text
                        .children_with_tokens()
                        .filter_map(|child| child.into_token())
                        .filter(|token| token.kind() == SyntaxKind::TEXT)
                        .map(|token| token.text().to_string())
                        .collect::<String>();
                    normalize_label(&text)
                })
        }
        SyntaxKind::FOOTNOTE_DEFINITION => {
            // FootnoteDefinition has a FootnoteReference token with text like "[^1]: "
            node.children_with_tokens()
                .filter_map(|child| child.into_token())
                .find(|token| token.kind() == SyntaxKind::FOOTNOTE_REFERENCE)
                .and_then(|token| {
                    let text = token.text();
                    // Extract ID from "[^id]: " format
                    if text.starts_with("[^") && text.contains("]:") {
                        let id = text.trim_start_matches("[^").split(']').next()?;
                        Some(normalize_label(id))
                    } else {
                        None
                    }
                })
        }
        _ => None,
    }
}

/// Find a definition node matching the given label
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
        if let Some((label, is_footnote)) = extract_reference_label(&node) {
            // Found a reference - now find its definition
            let definition = find_definition_node(root, &label, is_footnote)?;
            return Some(definition.text_range());
        }

        // Check if this is a Link that might contain a LinkRef
        if node.kind() == SyntaxKind::LINK
            && let Some(link_ref) = node
                .children()
                .find(|child| child.kind() == SyntaxKind::LINK_REF)
            && let Some((label, is_footnote)) = extract_reference_label(&link_ref)
        {
            let definition = find_definition_node(root, &label, is_footnote)?;
            return Some(definition.text_range());
        }

        // Check if this is an ImageLink that might contain a LinkRef
        if node.kind() == SyntaxKind::IMAGE_LINK
            && let Some(link_ref) = node
                .children()
                .find(|child| child.kind() == SyntaxKind::LINK_REF)
            && let Some((label, is_footnote)) = extract_reference_label(&link_ref)
        {
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
        assert_eq!(normalize_label("Foo"), "foo");
        assert_eq!(normalize_label("foo bar"), "foo bar");
        assert_eq!(normalize_label("foo  bar"), "foo bar");
        assert_eq!(normalize_label(" foo bar "), "foo bar");
    }

    #[test]
    fn test_extract_reference_label_from_link_ref() {
        let root = parse("[text][ref]");
        let link_ref = root
            .descendants()
            .find(|n| n.kind() == SyntaxKind::LINK_REF)
            .expect("Should find LinkRef");

        let (label, is_footnote) =
            extract_reference_label(&link_ref).expect("Should extract label");
        assert_eq!(label, "ref");
        assert!(!is_footnote);
    }

    #[test]
    fn test_extract_reference_label_from_footnote() {
        let root = parse("[^1]");
        let footnote_ref = root
            .descendants()
            .find(|n| n.kind() == SyntaxKind::FOOTNOTE_REFERENCE)
            .expect("Should find FootnoteReference");

        let (label, is_footnote) =
            extract_reference_label(&footnote_ref).expect("Should extract label");
        assert_eq!(label, "1");
        assert!(is_footnote);
    }

    #[test]
    fn test_extract_definition_label_from_reference() {
        let root = parse("[ref]: /url");
        let def = root
            .descendants()
            .find(|n| n.kind() == SyntaxKind::REFERENCE_DEFINITION)
            .expect("Should find ReferenceDefinition");

        let label = extract_definition_label(&def).expect("Should extract label");
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
    fn test_find_crossref_definition_node() {
        let input = "See @eq-test.\n\n$$\nE = mc^2\n$$ {#eq-test}\n";
        let mut config = Config::default();
        config.extensions.quarto_crossrefs = true;
        let root = crate::parser::parse(input, Some(config));

        let def = find_crossref_definition_node(&root, "eq-test");
        assert!(def.is_some());
        assert_eq!(def.unwrap().kind(), SyntaxKind::ATTRIBUTE);
    }

    #[test]
    fn test_find_crossref_definition_node_chunk_label() {
        let input = "See @fig-plot.\n\n```{r}\n#| label: fig-plot\nx <- 1\n```\n";
        let mut config = Config::default();
        config.flavor = crate::config::Flavor::Quarto;
        config.extensions.quarto_crossrefs = true;
        let root = crate::parser::parse(input, Some(config));

        let def = find_crossref_definition_node(&root, "fig-plot");
        assert!(def.is_some());
        assert_eq!(def.unwrap().kind(), SyntaxKind::CHUNK_OPTION);
    }

    #[test]
    fn test_find_implicit_header_definition_node() {
        let input = "# Implicit Header\n\nSee \\@ref(implicit-header).\n";
        let root = parse(input);

        let def = find_implicit_header_definition_node(&root, "implicit-header");
        assert!(def.is_some());
        assert_eq!(def.unwrap().kind(), SyntaxKind::HEADING);
    }

    #[test]
    fn test_find_implicit_header_definition_node_duplicates() {
        let input = "# Implicit Header\n\n# Implicit Header\n";
        let root = parse(input);

        let def = find_implicit_header_definition_node(&root, "implicit-header-1");
        assert!(def.is_some());
        assert_eq!(def.unwrap().kind(), SyntaxKind::HEADING);
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
}

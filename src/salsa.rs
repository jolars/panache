use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::metadata::DocumentMetadata;
use crate::parser::utils::attributes::try_parse_trailing_attributes;
use crate::syntax::{AstNode, FootnoteDefinition, ReferenceDefinition, SyntaxKind, SyntaxNode};
use crate::utils::normalize_label;

#[salsa::input]
pub struct FileText {
    #[returns(ref)]
    pub text: String,
}

#[salsa::input]
pub struct FileConfig {
    #[returns(ref)]
    pub config: Config,
}

#[salsa::tracked(returns(ref), no_eq, unsafe(non_update_types))]
pub fn metadata(
    db: &dyn salsa::Database,
    file: FileText,
    config: FileConfig,
    path: PathBuf,
) -> DocumentMetadata {
    let tree = crate::parse(file.text(db), Some(config.config(db).clone()));
    crate::metadata::extract_project_metadata(&tree, &path).unwrap_or_else(|_| {
        crate::metadata::DocumentMetadata {
            bibliography: None,
            metadata_files: Vec::new(),
            bibliography_parse: None,
            inline_references: Vec::new(),
            citations: crate::metadata::CitationInfo { keys: Vec::new() },
            title: None,
            raw_yaml: String::new(),
        }
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncludeOccurrence {
    pub path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncludeResolution {
    pub includes: Vec<IncludeOccurrence>,
}

#[salsa::tracked(returns(ref))]
pub fn includes(
    db: &dyn salsa::Database,
    file: FileText,
    config: FileConfig,
    path: PathBuf,
) -> IncludeResolution {
    let tree = crate::parse(file.text(db), Some(config.config(db).clone()));
    let base_dir = path.parent().unwrap_or_else(|| Path::new("."));
    let project_root = crate::includes::find_quarto_root(&path)
        .or_else(|| crate::includes::find_bookdown_root(&path));
    let mut resolution = IncludeResolution {
        includes: Vec::new(),
    };

    if !config.config(db).extensions.quarto_shortcodes {
        return resolution;
    }

    for node in tree.descendants() {
        if node.kind() != SyntaxKind::SHORTCODE {
            continue;
        }
        if crate::includes::is_escaped_shortcode(&node) {
            continue;
        }
        let Some(content) = crate::includes::extract_shortcode_content(&node) else {
            continue;
        };
        let args = crate::includes::split_shortcode_args(&content);
        if args.first().map(String::as_str) != Some("include") {
            continue;
        }
        let Some(raw_path) = args.get(1) else {
            continue;
        };
        let resolved =
            crate::includes::resolve_include_path(raw_path, base_dir, project_root.as_deref());
        if resolved.exists() && resolved.is_file() {
            resolution
                .includes
                .push(IncludeOccurrence { path: resolved });
        }
    }

    resolution
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefinitionLocation {
    pub path: PathBuf,
    pub range: rowan::TextRange,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DefinitionIndex {
    references: HashMap<String, DefinitionLocation>,
    footnotes: HashMap<String, DefinitionLocation>,
    crossrefs: HashMap<String, DefinitionLocation>,
}

#[salsa::tracked(returns(ref))]
pub fn definition_index(
    db: &dyn salsa::Database,
    file: FileText,
    config: FileConfig,
    path: PathBuf,
) -> DefinitionIndex {
    let tree = crate::parse(file.text(db), Some(config.config(db).clone()));
    let mut index = DefinitionIndex::default();

    for def in tree.descendants().filter_map(ReferenceDefinition::cast) {
        let label = def.label();
        if label.is_empty() {
            continue;
        }
        let location = DefinitionLocation {
            path: path.clone(),
            range: def.syntax().text_range(),
        };
        insert_reference(&mut index, &label, location);
    }

    for def in tree.descendants().filter_map(FootnoteDefinition::cast) {
        let id = def.id();
        if id.is_empty() {
            continue;
        }
        let location = DefinitionLocation {
            path: path.clone(),
            range: def.syntax().text_range(),
        };
        insert_footnote(&mut index, &id, location);
    }

    for node in tree.descendants() {
        if node.kind() != SyntaxKind::ATTRIBUTE {
            continue;
        }
        let text = node.text().to_string();
        if let Some(attrs) = try_parse_trailing_attributes(&text).map(|(attrs, _)| attrs)
            && let Some(id) = attrs.identifier
        {
            let location = DefinitionLocation {
                path: path.clone(),
                range: node.text_range(),
            };
            insert_crossref(&mut index, &id, location);
        }
    }

    if config.config(db).extensions.bookdown_references {
        collect_bookdown_definitions(&mut index, &tree, &path);
    }

    index
}

fn insert_reference(index: &mut DefinitionIndex, label: &str, location: DefinitionLocation) {
    let key = normalize_label(label);
    index.references.entry(key).or_insert(location);
}

fn insert_footnote(index: &mut DefinitionIndex, id: &str, location: DefinitionLocation) {
    let key = normalize_label(id);
    index.footnotes.entry(key).or_insert(location);
}

fn insert_crossref(index: &mut DefinitionIndex, id: &str, location: DefinitionLocation) {
    let key = normalize_label(id);
    index.crossrefs.entry(key).or_insert(location);
}

fn collect_bookdown_definitions(index: &mut DefinitionIndex, tree: &SyntaxNode, path: &Path) {
    use crate::parser::inlines::bookdown::{
        try_parse_bookdown_definition, try_parse_bookdown_text_reference,
    };

    for element in tree.descendants_with_tokens() {
        let Some(token) = element.into_token() else {
            continue;
        };
        if token.kind() != SyntaxKind::TEXT {
            continue;
        }
        let text = token.text();
        let mut offset = 0usize;
        let bytes = text.as_bytes();
        while offset < bytes.len() {
            if bytes[offset] != b'(' {
                offset += 1;
                continue;
            }
            let slice = &text[offset..];
            if let Some((len, label)) = try_parse_bookdown_definition(slice) {
                let start: usize = token.text_range().start().into();
                let range = rowan::TextRange::new(
                    rowan::TextSize::from((start + offset) as u32),
                    rowan::TextSize::from((start + offset + len) as u32),
                );
                let location = DefinitionLocation {
                    path: path.to_path_buf(),
                    range,
                };
                insert_crossref(index, label, location);
                offset += len;
                continue;
            }
            if let Some((len, label)) = try_parse_bookdown_text_reference(slice) {
                let start: usize = token.text_range().start().into();
                let range = rowan::TextRange::new(
                    rowan::TextSize::from((start + offset) as u32),
                    rowan::TextSize::from((start + offset + len) as u32),
                );
                let location = DefinitionLocation {
                    path: path.to_path_buf(),
                    range,
                };
                insert_crossref(index, label, location);
                offset += len;
                continue;
            }
            offset += 1;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EdgeKind {
    Include,
    Bibliography,
    MetadataFile,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectGraph {
    documents: HashSet<PathBuf>,
    edges: HashMap<PathBuf, HashSet<(PathBuf, EdgeKind)>>,
    reverse_edges: HashMap<PathBuf, HashSet<(PathBuf, EdgeKind)>>,
}

impl ProjectGraph {
    pub fn documents(&self) -> &HashSet<PathBuf> {
        &self.documents
    }

    pub fn dependents(&self, path: &Path, kind: Option<EdgeKind>) -> Vec<PathBuf> {
        self.reverse_edges
            .get(path)
            .map(|edges| {
                edges
                    .iter()
                    .filter(|(_, edge_kind)| kind.is_none_or(|k| k == *edge_kind))
                    .map(|(from, _)| from.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn add_edge(&mut self, from: &Path, to: &Path, kind: EdgeKind) {
        let from = from.to_path_buf();
        let to = to.to_path_buf();
        self.edges
            .entry(from.clone())
            .or_default()
            .insert((to.clone(), kind));
        self.reverse_edges
            .entry(to)
            .or_default()
            .insert((from, kind));
    }
}

#[salsa::tracked(returns(ref))]
pub fn project_graph(
    db: &dyn salsa::Database,
    root_file: FileText,
    config: FileConfig,
    root_path: PathBuf,
) -> ProjectGraph {
    let mut graph = ProjectGraph::default();
    let mut visited = HashSet::new();
    let _project_root = crate::includes::find_quarto_root(&root_path)
        .or_else(|| crate::includes::find_bookdown_root(&root_path));
    visit_document(db, &root_file, config, &root_path, &mut graph, &mut visited);
    graph
}

fn visit_document(
    db: &dyn salsa::Database,
    file: &FileText,
    config: FileConfig,
    path: &Path,
    graph: &mut ProjectGraph,
    visited: &mut HashSet<PathBuf>,
) {
    if !visited.insert(path.to_path_buf()) {
        return;
    }
    graph.documents.insert(path.to_path_buf());
    let resolution = includes(db, *file, config, path.to_path_buf());
    for include in resolution.includes.iter() {
        graph.add_edge(path, &include.path, EdgeKind::Include);
        if include.path == *path {
            continue;
        }
        if let Ok(include_input) = std::fs::read_to_string(&include.path) {
            let _include_base = include.path.parent().unwrap_or_else(|| Path::new("."));
            let include_file = FileText::new(db, include_input);
            visit_document(db, &include_file, config, &include.path, graph, visited);
        }
    }
}
#[salsa::db]
#[derive(Default, Clone)]
pub struct SalsaDb {
    storage: salsa::Storage<Self>,
}

#[salsa::db]
impl salsa::Database for SalsaDb {}

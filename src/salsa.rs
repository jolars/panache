use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::config::Config;
use crate::linter::diagnostics::Diagnostic;
use crate::metadata::DocumentMetadata;
use crate::parser::utils::attributes::try_parse_trailing_attributes;
use crate::syntax::{AstNode, FootnoteDefinition, ReferenceDefinition, SyntaxKind, SyntaxNode};
use crate::utils::normalize_label;
use salsa::{Accumulator, Setter};

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
    db: &dyn Db,
    file: FileText,
    config: FileConfig,
    path: PathBuf,
) -> DocumentMetadata {
    let tree = crate::parse(file.text(db), Some(config.config(db).clone()));
    let mut metadata =
        crate::metadata::extract_project_metadata_without_bibliography_parse(&tree, &path)
            .unwrap_or_else(|_| crate::metadata::DocumentMetadata {
                bibliography: None,
                metadata_files: Vec::new(),
                bibliography_parse: None,
                inline_references: Vec::new(),
                citations: crate::metadata::CitationInfo { keys: Vec::new() },
                title: None,
                raw_yaml: String::new(),
            });

    // Route bibliography parsing through salsa so each bibliography file is cached and
    // invalidated via `Db::file_text` updates.
    if let Some(info) = metadata.bibliography.as_ref() {
        let mut index = crate::bib::BibIndex {
            entries: HashMap::new(),
            duplicates: Vec::new(),
            errors: Vec::new(),
            load_errors: Vec::new(),
        };
        let mut seen_paths = HashSet::new();

        for bib_path in &info.paths {
            db.unwind_if_revision_cancelled();
            if !seen_paths.insert(bib_path.clone()) {
                continue;
            }
            let Some(bib_file) = db.file_text(bib_path.clone()) else {
                index.load_errors.push(crate::bib::BibLoadError {
                    path: bib_path.clone(),
                    message: "Failed to read file".to_string(),
                });
                continue;
            };

            index.merge_from(bibliography_index(db, bib_file, bib_path.clone()).clone());
        }

        let parse_errors = index.errors.iter().map(|e| e.message.clone()).collect();
        metadata.bibliography_parse = Some(crate::metadata::BibliographyParse {
            index,
            parse_errors,
        });
    }

    metadata
}

#[salsa::tracked(returns(ref), no_eq, unsafe(non_update_types))]
pub fn yaml_metadata_parse_result(
    db: &dyn Db,
    file: FileText,
    config: FileConfig,
    path: PathBuf,
) -> Result<(), crate::metadata::YamlError> {
    let tree = crate::parse(file.text(db), Some(config.config(db).clone()));
    crate::metadata::extract_project_metadata_without_bibliography_parse(&tree, &path).map(|_| ())
}

#[salsa::tracked(returns(ref), no_eq, unsafe(non_update_types))]
pub fn bibliography_index(db: &dyn Db, file: FileText, path: PathBuf) -> crate::bib::BibIndex {
    crate::bib::load_bibliography_from_text(file.text(db), &path)
}

// includes resolution logic lives in crate::includes.

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

#[salsa::tracked(returns(ref), lru = 64)]
pub fn definition_index(
    db: &dyn Db,
    file: FileText,
    config: FileConfig,
    path: PathBuf,
) -> DefinitionIndex {
    let tree = crate::parse(file.text(db), Some(config.config(db).clone()));
    let mut index = DefinitionIndex::default();

    for def in tree.descendants().filter_map(ReferenceDefinition::cast) {
        db.unwind_if_revision_cancelled();
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
        db.unwind_if_revision_cancelled();
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
        db.unwind_if_revision_cancelled();
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
        collect_bookdown_definitions(db, &mut index, &tree, &path);
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

impl DefinitionIndex {
    pub fn is_empty(&self) -> bool {
        self.references.is_empty() && self.footnotes.is_empty() && self.crossrefs.is_empty()
    }

    pub fn find_reference(&self, label: &str) -> Option<&DefinitionLocation> {
        let key = normalize_label(label);
        self.references.get(&key)
    }

    pub fn find_footnote(&self, id: &str) -> Option<&DefinitionLocation> {
        let key = normalize_label(id);
        self.footnotes.get(&key)
    }

    pub fn find_crossref(&self, id: &str) -> Option<&DefinitionLocation> {
        let key = normalize_label(id);
        self.crossrefs.get(&key)
    }

    pub fn merge_from(&mut self, other: &DefinitionIndex) {
        for (key, value) in &other.references {
            self.references
                .entry(key.clone())
                .or_insert_with(|| value.clone());
        }
        for (key, value) in &other.footnotes {
            self.footnotes
                .entry(key.clone())
                .or_insert_with(|| value.clone());
        }
        for (key, value) in &other.crossrefs {
            self.crossrefs
                .entry(key.clone())
                .or_insert_with(|| value.clone());
        }
    }
}

impl DefinitionLocation {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn range(&self) -> rowan::TextRange {
        self.range
    }
}

fn collect_bookdown_definitions(
    db: &dyn Db,
    index: &mut DefinitionIndex,
    tree: &SyntaxNode,
    path: &Path,
) {
    use crate::parser::inlines::bookdown::{
        try_parse_bookdown_definition, try_parse_bookdown_text_reference,
    };

    for element in tree.descendants_with_tokens() {
        db.unwind_if_revision_cancelled();
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
            db.unwind_if_revision_cancelled();
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

    pub fn dependencies(&self, path: &Path, kind: Option<EdgeKind>) -> Vec<PathBuf> {
        self.edges
            .get(path)
            .map(|edges| {
                edges
                    .iter()
                    .filter(|(_, edge_kind)| kind.is_none_or(|k| k == *edge_kind))
                    .map(|(to, _)| to.clone())
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

#[derive(Debug, Clone)]
pub struct GraphDiagnosticEntry {
    pub path: PathBuf,
    pub diagnostic: Diagnostic,
}

#[salsa::accumulator]
pub struct GraphDiagnostic(pub GraphDiagnosticEntry);

#[salsa::tracked(returns(ref), lru = 32)]
pub fn project_graph(
    db: &dyn Db,
    root_file: FileText,
    config: FileConfig,
    root_path: PathBuf,
) -> ProjectGraph {
    let mut graph = ProjectGraph::default();
    let mut visited = HashSet::new();
    let mut definitions = crate::includes::DefinitionIndex::default();
    let _project_root = crate::includes::find_quarto_root(&root_path)
        .or_else(|| crate::includes::find_bookdown_root(&root_path));
    visit_document(
        db,
        &root_file,
        config,
        &root_path,
        &mut graph,
        &mut visited,
        &mut definitions,
    );
    if let Some(project_root) = crate::includes::find_quarto_root(&root_path)
        .or_else(|| crate::includes::find_bookdown_root(&root_path))
    {
        let is_bookdown = crate::includes::find_bookdown_root(&root_path).is_some();
        for path in
            crate::includes::find_project_documents(&project_root, config.config(db), is_bookdown)
        {
            db.unwind_if_revision_cancelled();
            if visited.contains(&path) {
                continue;
            }
            if let Some(include_file) = db.file_text(path.clone()) {
                visit_document(
                    db,
                    &include_file,
                    config,
                    &path,
                    &mut graph,
                    &mut visited,
                    &mut definitions,
                );
            }
        }
    }
    graph
}

fn visit_document(
    db: &dyn Db,
    file: &FileText,
    config: FileConfig,
    path: &Path,
    graph: &mut ProjectGraph,
    visited: &mut HashSet<PathBuf>,
    definitions: &mut crate::includes::DefinitionIndex,
) {
    if !visited.insert(path.to_path_buf()) {
        return;
    }
    graph.documents.insert(path.to_path_buf());
    let text = file.text(db);
    let tree = crate::parse(text, Some(config.config(db).clone()));
    let base_dir = path.parent().unwrap_or_else(|| Path::new("."));
    let project_root = crate::includes::find_quarto_root(path)
        .or_else(|| crate::includes::find_bookdown_root(path));
    let resolution = crate::includes::collect_includes(
        &tree,
        text,
        base_dir,
        project_root.as_deref(),
        config.config(db),
    );
    for include in resolution.includes.iter() {
        db.unwind_if_revision_cancelled();
        graph.add_edge(path, &include.path, EdgeKind::Include);
        if include.path == *path {
            continue;
        }
        if let Some(include_file) = db.file_text(include.path.clone()) {
            visit_document(
                db,
                &include_file,
                config,
                &include.path,
                graph,
                visited,
                definitions,
            );
        }
    }
    if !resolution.diagnostics.is_empty() {
        for diagnostic in resolution.diagnostics {
            GraphDiagnostic(GraphDiagnosticEntry {
                path: path.to_path_buf(),
                diagnostic,
            })
            .accumulate(db);
        }
    }

    let duplicate_diagnostics = crate::includes::collect_cross_doc_duplicates(
        definitions,
        &tree,
        text,
        path,
        config.config(db),
    );
    if !duplicate_diagnostics.is_empty() {
        for diagnostic in duplicate_diagnostics {
            db.unwind_if_revision_cancelled();
            GraphDiagnostic(GraphDiagnosticEntry {
                path: path.to_path_buf(),
                diagnostic,
            })
            .accumulate(db);
        }
    }
    if let Ok(metadata) = crate::metadata::extract_project_metadata(&tree, path) {
        for metadata_file in &metadata.metadata_files {
            graph.add_edge(path, metadata_file, EdgeKind::MetadataFile);
        }
        if let Some(bibliography) = metadata.bibliography {
            for bib in bibliography.paths {
                graph.add_edge(path, &bib, EdgeKind::Bibliography);
            }
        }
    }
}
#[salsa::db]
pub trait Db: salsa::Database {
    fn file_text(&self, path: PathBuf) -> Option<FileText>;
}

#[salsa::db]
#[derive(Clone)]
pub struct SalsaDb {
    storage: salsa::Storage<Self>,
    file_cache: Arc<Mutex<HashMap<PathBuf, FileText>>>,
}

impl Default for SalsaDb {
    fn default() -> Self {
        Self {
            storage: salsa::Storage::default(),
            file_cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl SalsaDb {
    fn get_or_load_file_text(&self, path: PathBuf) -> Option<FileText> {
        let mut cache = self.file_cache.lock().ok()?;
        if let Some(file) = cache.get(&path) {
            return Some(*file);
        }
        let contents = std::fs::read_to_string(&path).ok()?;
        let file = FileText::new(self, contents);
        cache.insert(path, file);
        Some(file)
    }

    pub fn file_text_if_cached(&self, path: &Path) -> Option<FileText> {
        let cache = self.file_cache.lock().expect("file cache lock poisoned");
        cache.get(path).copied()
    }

    pub fn update_file_text(&mut self, path: PathBuf, text: String) -> FileText {
        let existing = {
            let cache = self.file_cache.lock().expect("file cache lock poisoned");
            cache.get(&path).copied()
        };
        if let Some(file) = existing {
            file.set_text(self).to(text);
            return file;
        }
        let file = FileText::new(self, text);
        let mut cache = self.file_cache.lock().expect("file cache lock poisoned");
        cache.insert(path, file);
        file
    }

    pub fn update_file_text_if_cached(&mut self, path: &Path, text: String) -> bool {
        let file = {
            let cache = self.file_cache.lock().expect("file cache lock poisoned");
            cache.get(path).copied()
        };
        let Some(file) = file else {
            return false;
        };
        file.set_text(self).to(text);
        true
    }

    pub fn ensure_file_text_cached(&mut self, path: PathBuf) -> bool {
        {
            let cache = self.file_cache.lock().expect("file cache lock poisoned");
            if cache.contains_key(&path) {
                return true;
            }
        }
        let Ok(contents) = std::fs::read_to_string(&path) else {
            return false;
        };
        let file = FileText::new(self, contents);
        let mut cache = self.file_cache.lock().expect("file cache lock poisoned");
        cache.insert(path, file);
        true
    }

    pub fn cached_file_paths(&self) -> Vec<PathBuf> {
        let cache = self.file_cache.lock().expect("file cache lock poisoned");
        cache.keys().cloned().collect()
    }

    pub fn evict_file_text(&mut self, path: &Path) -> bool {
        let mut cache = self.file_cache.lock().expect("file cache lock poisoned");
        cache.remove(path).is_some()
    }
}

#[salsa::db]
impl salsa::Database for SalsaDb {}

#[salsa::db]
impl Db for SalsaDb {
    fn file_text(&self, path: PathBuf) -> Option<FileText> {
        self.get_or_load_file_text(path)
    }
}

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use rowan::{NodeOrToken, TextRange};

use crate::config::Config;
use crate::linter::diagnostics::{Diagnostic, Location};
use crate::parser::utils::attributes::try_parse_trailing_attributes;
use crate::syntax::{AstNode, FootnoteDefinition, ReferenceDefinition, SyntaxKind, SyntaxNode};
use crate::utils::normalize_label;

#[derive(Debug, Clone)]
pub struct IncludeOccurrence {
    pub path: PathBuf,
    pub range: TextRange,
}

#[derive(Debug, Default)]
pub struct IncludeResolution {
    pub includes: Vec<IncludeOccurrence>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone)]
pub struct DefinitionLocation {
    pub path: PathBuf,
    pub range: TextRange,
    pub line: usize,
}

#[derive(Debug, Default)]
pub struct DefinitionIndex {
    references: HashMap<String, DefinitionLocation>,
    footnotes: HashMap<String, DefinitionLocation>,
    crossrefs: HashMap<String, DefinitionLocation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EdgeKind {
    Include,
    Bibliography,
    MetadataFile,
}

#[derive(Debug, Default)]
pub struct ProjectGraph {
    definitions: DefinitionIndex,
    diagnostics: HashMap<PathBuf, Vec<Diagnostic>>,
    documents: HashSet<PathBuf>,
    edges: HashMap<PathBuf, HashSet<(PathBuf, EdgeKind)>>,
    reverse_edges: HashMap<PathBuf, HashSet<(PathBuf, EdgeKind)>>,
}

impl DefinitionIndex {
    pub fn is_empty(&self) -> bool {
        self.references.is_empty() && self.footnotes.is_empty() && self.crossrefs.is_empty()
    }
}

impl DefinitionLocation {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn range(&self) -> TextRange {
        self.range
    }
}

impl ProjectGraph {
    pub fn build(root_path: &Path, root_text: &str, config: &Config) -> Self {
        let mut graph = ProjectGraph::default();
        let mut visited = HashSet::new();
        let base_dir = root_path.parent().unwrap_or_else(|| Path::new("."));
        let project_root = find_quarto_root(root_path);
        visit_document(
            root_path,
            root_text,
            base_dir,
            project_root.as_deref(),
            config,
            &mut graph,
            &mut visited,
        );
        graph
    }

    pub fn definitions(&self) -> &DefinitionIndex {
        &self.definitions
    }

    pub fn diagnostics(&self) -> &HashMap<PathBuf, Vec<Diagnostic>> {
        &self.diagnostics
    }

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
}

pub fn collect_includes(
    tree: &SyntaxNode,
    input: &str,
    base_dir: &Path,
    project_root: Option<&Path>,
    config: &Config,
) -> IncludeResolution {
    if !config.extensions.quarto_shortcodes {
        return IncludeResolution::default();
    }

    let mut resolution = IncludeResolution::default();

    for node in tree.descendants() {
        if node.kind() != SyntaxKind::SHORTCODE {
            continue;
        }

        if is_escaped_shortcode(&node) {
            continue;
        }

        let Some(content) = extract_shortcode_content(&node) else {
            continue;
        };

        let args = split_shortcode_args(&content);
        if args.first().map(String::as_str) != Some("include") {
            continue;
        }
        let Some(raw_path) = args.get(1) else {
            continue;
        };

        let resolved = resolve_include_path(raw_path, base_dir, project_root);
        if !resolved.exists() || !resolved.is_file() {
            resolution.diagnostics.push(include_not_found_diagnostic(
                input,
                node.text_range(),
                &resolved,
            ));
            continue;
        }

        resolution.includes.push(IncludeOccurrence {
            path: resolved,
            range: node.text_range(),
        });
    }

    resolution
}

pub fn collect_cross_doc_duplicates(
    index: &mut DefinitionIndex,
    tree: &SyntaxNode,
    input: &str,
    doc_path: &Path,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for def in tree.descendants().filter_map(ReferenceDefinition::cast) {
        let label = def.label();
        if label.is_empty() {
            continue;
        }
        let location = DefinitionLocation::new(doc_path, def.syntax().text_range(), input);
        if let Some(first) = index.insert_reference(&label, location.clone())
            && first.path != doc_path
        {
            diagnostics.push(Diagnostic::warning(
                Location::from_range(location.range, input),
                "duplicate-reference-labels",
                format!(
                    "Duplicate reference label '[{}]' (first defined at {}:{})",
                    label,
                    first.path.display(),
                    first.line
                ),
            ));
        }
    }

    for def in tree.descendants().filter_map(FootnoteDefinition::cast) {
        let id = def.id();
        if id.is_empty() {
            continue;
        }
        let location = DefinitionLocation::new(doc_path, def.syntax().text_range(), input);
        if let Some(first) = index.insert_footnote(&id, location.clone())
            && first.path != doc_path
        {
            diagnostics.push(Diagnostic::warning(
                Location::from_range(location.range, input),
                "duplicate-reference-labels",
                format!(
                    "Duplicate footnote ID '[^{}]' (first defined at {}:{})",
                    id,
                    first.path.display(),
                    first.line
                ),
            ));
        }
    }

    for node in tree.descendants() {
        if node.kind() != SyntaxKind::ATTRIBUTE {
            continue;
        }
        let text = node.text().to_string();
        if let Some(attrs) = try_parse_trailing_attributes(&text).map(|(attrs, _)| attrs)
            && let Some(id) = attrs.identifier
        {
            let location = DefinitionLocation::new(doc_path, node.text_range(), input);
            index.insert_crossref(&id, location);
        }
    }

    diagnostics
}

pub fn include_cycle_diagnostic(input: &str, range: TextRange, path: &Path) -> Diagnostic {
    Diagnostic::error(
        Location::from_range(range, input),
        "include-cycle",
        format!("Include cycle detected: {}", path.display()),
    )
}

pub fn include_read_error_diagnostic(
    input: &str,
    range: TextRange,
    path: &Path,
    error: &str,
) -> Diagnostic {
    Diagnostic::error(
        Location::from_range(range, input),
        "include-read-error",
        format!("Failed to read included file {}: {}", path.display(), error),
    )
}

pub fn find_quarto_root(doc_path: &Path) -> Option<PathBuf> {
    let mut current = doc_path.parent()?;
    loop {
        let quarto = current.join("_quarto.yml");
        if quarto.exists() {
            return Some(current.to_path_buf());
        }
        current = current.parent()?;
    }
}

fn include_not_found_diagnostic(input: &str, range: TextRange, path: &Path) -> Diagnostic {
    Diagnostic::error(
        Location::from_range(range, input),
        "include-not-found",
        format!("Included file not found: {}", path.display()),
    )
}

fn resolve_include_path(raw: &str, base_dir: &Path, project_root: Option<&Path>) -> PathBuf {
    let trimmed = raw.trim();
    if let Some(path) = trimmed.strip_prefix('/')
        && let Some(root) = project_root
    {
        return root.join(path);
    }
    base_dir.join(trimmed)
}

impl DefinitionLocation {
    fn new(path: &Path, range: TextRange, input: &str) -> Self {
        let location = Location::from_range(range, input);
        Self {
            path: path.to_path_buf(),
            range,
            line: location.line,
        }
    }
}

impl DefinitionIndex {
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

    fn insert_reference(
        &mut self,
        label: &str,
        location: DefinitionLocation,
    ) -> Option<DefinitionLocation> {
        let key = normalize_label(label);
        if let Some(existing) = self.references.get(&key) {
            return Some(existing.clone());
        }
        self.references.insert(key, location);
        None
    }

    fn insert_footnote(
        &mut self,
        id: &str,
        location: DefinitionLocation,
    ) -> Option<DefinitionLocation> {
        let key = normalize_label(id);
        if let Some(existing) = self.footnotes.get(&key) {
            return Some(existing.clone());
        }
        self.footnotes.insert(key, location);
        None
    }

    fn insert_crossref(&mut self, id: &str, location: DefinitionLocation) {
        let key = normalize_label(id);
        self.crossrefs.entry(key).or_insert(location);
    }
}

fn visit_document(
    path: &Path,
    input: &str,
    base_dir: &Path,
    project_root: Option<&Path>,
    config: &Config,
    graph: &mut ProjectGraph,
    visited: &mut HashSet<PathBuf>,
) {
    if !visited.insert(path.to_path_buf()) {
        return;
    }
    graph.documents.insert(path.to_path_buf());

    let tree = crate::parse(input, Some(config.clone()));
    let diagnostics = collect_cross_doc_duplicates(&mut graph.definitions, &tree, input, path);
    if !diagnostics.is_empty() {
        graph
            .diagnostics
            .entry(path.to_path_buf())
            .or_default()
            .extend(diagnostics);
    }

    let resolution = collect_includes(&tree, input, base_dir, project_root, config);
    if !resolution.diagnostics.is_empty() {
        graph
            .diagnostics
            .entry(path.to_path_buf())
            .or_default()
            .extend(resolution.diagnostics);
    }
    for include in resolution.includes {
        graph.add_edge(path, &include.path, EdgeKind::Include);
        if let Ok(include_input) = std::fs::read_to_string(&include.path) {
            let include_base = include.path.parent().unwrap_or_else(|| Path::new("."));
            visit_document(
                &include.path,
                &include_input,
                include_base,
                project_root,
                config,
                graph,
                visited,
            );
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

impl ProjectGraph {
    fn add_edge(&mut self, from: &Path, to: &Path, kind: EdgeKind) {
        self.edges
            .entry(from.to_path_buf())
            .or_default()
            .insert((to.to_path_buf(), kind));
        self.reverse_edges
            .entry(to.to_path_buf())
            .or_default()
            .insert((from.to_path_buf(), kind));
    }
}

fn is_escaped_shortcode(node: &SyntaxNode) -> bool {
    node.children_with_tokens().any(|child| match child {
        NodeOrToken::Token(token) => {
            token.kind() == SyntaxKind::SHORTCODE_MARKER_OPEN && token.text() == "{{{<"
        }
        _ => false,
    })
}

fn extract_shortcode_content(node: &SyntaxNode) -> Option<String> {
    node.children().find_map(|child| {
        if child.kind() == SyntaxKind::SHORTCODE_CONTENT {
            Some(child.text().to_string())
        } else {
            None
        }
    })
}

fn split_shortcode_args(content: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut quote_char = None;

    for ch in content.trim().chars() {
        match ch {
            '"' | '\'' if !in_quotes => {
                in_quotes = true;
                quote_char = Some(ch);
            }
            c if Some(c) == quote_char && in_quotes => {
                in_quotes = false;
                quote_char = None;
            }
            c if c.is_whitespace() && !in_quotes => {
                if !current.is_empty() {
                    args.push(current.clone());
                    current.clear();
                }
            }
            c => current.push(c),
        }
    }

    if !current.is_empty() {
        args.push(current);
    }

    args
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn project_graph_tracks_metadata_and_bibliography_edges() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();
        let doc_path = root.join("doc.qmd");

        fs::write(
            root.join("_quarto.yml"),
            "metadata-files:\n  - _site.yml\nbibliography: proj.bib\n",
        )
        .unwrap();
        fs::write(root.join("_site.yml"), "title: Site\n").unwrap();
        fs::write(root.join("proj.bib"), "@book{proj,}\n").unwrap();
        fs::write(&doc_path, "---\n---\n\nText").unwrap();

        let input = fs::read_to_string(&doc_path).unwrap();
        let config = Config::default();
        let graph = ProjectGraph::build(&doc_path, &input, &config);

        let metadata_deps = graph.dependencies(&doc_path, Some(EdgeKind::MetadataFile));
        assert!(metadata_deps.contains(&root.join("_site.yml")));

        let bib_deps = graph.dependencies(&doc_path, Some(EdgeKind::Bibliography));
        assert!(bib_deps.contains(&root.join("proj.bib")));
    }
}

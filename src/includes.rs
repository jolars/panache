use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::metadata::project::{BookdownFiles, read_bookdown_files};
use regex::Regex;
use rowan::TextRange;

use crate::config::Config;
use crate::linter::diagnostics::{Diagnostic, Location};
use crate::syntax::{
    AstNode, AttributeNode, ChunkOption, FootnoteDefinition, ReferenceDefinition, Shortcode,
    SyntaxKind, SyntaxNode,
};
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

#[derive(Debug, Clone, Default)]
pub struct DefinitionIndex {
    references: HashMap<String, DefinitionLocation>,
    footnotes: HashMap<String, DefinitionLocation>,
    crossrefs: HashMap<String, DefinitionLocation>,
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

    for shortcode in tree.descendants().filter_map(Shortcode::cast) {
        if shortcode.is_escaped() {
            continue;
        }

        if shortcode.name().as_deref() != Some("include") {
            continue;
        }
        let args = shortcode.args();
        let Some(raw_path) = args.get(1) else {
            continue;
        };

        let resolved = resolve_include_path(raw_path, base_dir, project_root);
        if !resolved.exists() || !resolved.is_file() {
            resolution.diagnostics.push(include_not_found_diagnostic(
                input,
                shortcode.syntax().text_range(),
                &resolved,
            ));
            continue;
        }

        resolution.includes.push(IncludeOccurrence {
            path: resolved,
            range: shortcode.syntax().text_range(),
        });
    }

    resolution
}

pub fn collect_cross_doc_duplicates(
    index: &mut DefinitionIndex,
    tree: &SyntaxNode,
    input: &str,
    doc_path: &Path,
    config: &Config,
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

    for attribute in tree.descendants().filter_map(AttributeNode::cast) {
        if let Some(id) = attribute.id() {
            let location =
                DefinitionLocation::new(doc_path, attribute.syntax().text_range(), input);
            index.insert_crossref(&id, location);
        }
    }

    for option in tree.descendants().filter_map(ChunkOption::cast) {
        let Some(key) = option.key() else {
            continue;
        };
        if !key.eq_ignore_ascii_case("label") {
            continue;
        }
        let Some(value) = option.value() else {
            continue;
        };
        if value.is_empty() {
            continue;
        }
        let location = DefinitionLocation::new(doc_path, option.syntax().text_range(), input);
        index.insert_crossref(&value, location);
    }

    if config.extensions.bookdown_references {
        collect_bookdown_definitions(index, tree, input, doc_path);
    }

    diagnostics
}

fn collect_bookdown_definitions(
    index: &mut DefinitionIndex,
    tree: &SyntaxNode,
    input: &str,
    doc_path: &Path,
) {
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
                let location = DefinitionLocation::new(doc_path, range, input);
                index.insert_crossref(label, location);
                offset += len;
                continue;
            }
            if let Some((len, label)) = try_parse_bookdown_text_reference(slice) {
                let start: usize = token.text_range().start().into();
                let range = rowan::TextRange::new(
                    rowan::TextSize::from((start + offset) as u32),
                    rowan::TextSize::from((start + offset + len) as u32),
                );
                let location = DefinitionLocation::new(doc_path, range, input);
                index.insert_crossref(label, location);
                offset += len;
                continue;
            }
            offset += 1;
        }
    }
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

pub fn find_bookdown_root(doc_path: &Path) -> Option<PathBuf> {
    let mut current = doc_path.parent()?;
    loop {
        let bookdown = current.join("_bookdown.yml");
        if bookdown.exists() {
            return Some(current.to_path_buf());
        }
        current = current.parent()?;
    }
}

pub fn find_project_documents(
    project_root: &Path,
    config: &Config,
    is_bookdown: bool,
) -> Vec<PathBuf> {
    let mut docs = Vec::new();
    let mut seen = HashSet::new();
    let bookdown_files = if is_bookdown {
        read_bookdown_files(project_root)
    } else {
        None
    };
    let quarto_render = if is_bookdown {
        None
    } else {
        read_quarto_render(project_root)
    };
    let walker = ignore::WalkBuilder::new(project_root).build();

    for entry in walker.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if let Some(files) = &bookdown_files {
            let contains = match files {
                BookdownFiles::List(files) => files.contains(&path.to_path_buf()),
                BookdownFiles::ByFormat(formats) => {
                    formats.values().flatten().any(|value| value == path)
                }
            };
            if !contains {
                continue;
            }
        }
        let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
            continue;
        };
        if !crate::all_document_extensions().contains(&ext) {
            continue;
        }
        if ext == "md" && !config.extensions.quarto_shortcodes {
            continue;
        }
        if !is_bookdown && !is_quarto_render_target(path, project_root, quarto_render.as_deref()) {
            continue;
        }
        if seen.insert(path.to_path_buf()) {
            docs.push(path.to_path_buf());
        }
    }

    docs
}

fn read_quarto_render(project_root: &Path) -> Option<Vec<String>> {
    let quarto_config = project_root.join("_quarto.yml");
    if !quarto_config.exists() {
        return None;
    }
    let yaml = std::fs::read_to_string(quarto_config).ok()?;
    let root = yaml_parser::ast::Root::cast(yaml_parser::parse(&yaml).ok()?)?;
    let document = root.documents().next()?;
    let top_level = document.block()?.block_map()?;
    let project_entry = top_level
        .entries()
        .find(|entry| block_map_entry_key(entry).as_deref() == Some("project"))?;
    let project_map = project_entry.value()?.block()?.block_map()?;
    let render_entry = project_map
        .entries()
        .find(|entry| block_map_entry_key(entry).as_deref() == Some("render"))?;
    block_map_value_to_string_vec(render_entry.value()?)
}

fn block_map_entry_key(entry: &yaml_parser::ast::BlockMapEntry) -> Option<String> {
    let key = entry.key()?;
    if let Some(flow) = key.flow() {
        return flow_scalar_text(&flow);
    }
    let block = key.block()?;
    let flow = block_to_flow_scalar(&block)?;
    flow_scalar_text(&flow)
}

fn block_map_value_to_string_vec(value: yaml_parser::ast::BlockMapValue) -> Option<Vec<String>> {
    if let Some(flow) = value.flow()
        && let Some(single) = flow_scalar_text(&flow)
    {
        return Some(vec![single]);
    }
    let block = value.block()?;
    if let Some(sequence) = block.block_seq() {
        let mut out = Vec::new();
        for entry in sequence.entries() {
            let item = if let Some(flow) = entry.flow() {
                flow_scalar_text(&flow)
            } else if let Some(block) = entry.block() {
                block_to_flow_scalar(&block).and_then(|flow| flow_scalar_text(&flow))
            } else {
                None
            }?;
            out.push(item);
        }
        return Some(out);
    }
    None
}

fn block_to_flow_scalar(block: &yaml_parser::ast::Block) -> Option<yaml_parser::ast::Flow> {
    block
        .syntax()
        .children()
        .find_map(yaml_parser::ast::Flow::cast)
}

fn flow_scalar_text(flow: &yaml_parser::ast::Flow) -> Option<String> {
    if let Some(token) = flow.plain_scalar() {
        return Some(token.text().to_string());
    }
    if let Some(token) = flow.single_quoted_scalar() {
        return Some(token.text().trim_matches('\'').to_string());
    }
    if let Some(token) = flow.double_qouted_scalar() {
        return Some(token.text().trim_matches('"').to_string());
    }
    None
}

fn is_quarto_render_target(path: &Path, root: &Path, render: Option<&[String]>) -> bool {
    let Some(rel_path) = relative_path_string(path, root) else {
        return false;
    };
    if let Some(patterns) = render {
        return matches_render_patterns(&rel_path, patterns);
    }
    is_default_quarto_target(path, &rel_path)
}

fn is_default_quarto_target(path: &Path, rel_path: &str) -> bool {
    if matches!(
        path.file_name().and_then(|name| name.to_str()),
        Some("README.md") | Some("README.qmd")
    ) {
        return false;
    }
    !rel_path
        .split('/')
        .any(|component| component.starts_with('.') || component.starts_with('_'))
}

fn relative_path_string(path: &Path, root: &Path) -> Option<String> {
    let relative = path.strip_prefix(root).ok()?;
    let components: Vec<String> = relative
        .iter()
        .map(|component| component.to_string_lossy().to_string())
        .collect();
    if components.is_empty() {
        None
    } else {
        Some(components.join("/"))
    }
}

fn matches_render_patterns(rel_path: &str, patterns: &[String]) -> bool {
    let mut included = false;
    for raw_pattern in patterns {
        let trimmed = raw_pattern.trim();
        if trimmed.is_empty() {
            continue;
        }
        let (is_exclusion, pattern) = if let Some(pattern) = trimmed.strip_prefix('!') {
            (true, pattern.trim())
        } else {
            (false, trimmed)
        };
        if pattern.is_empty() {
            continue;
        }
        if quarto_render_pattern_matches(rel_path, pattern) {
            included = !is_exclusion;
        }
    }
    included
}

fn quarto_render_pattern_matches(rel_path: &str, pattern: &str) -> bool {
    let normalized = pattern.trim_start_matches("./");
    if let Some(dir) = normalized.strip_suffix('/') {
        if dir.is_empty() {
            return false;
        }
        return rel_path == dir || rel_path.starts_with(&format!("{dir}/"));
    }
    wildcard_match(rel_path, normalized)
}

fn wildcard_match(path: &str, pattern: &str) -> bool {
    let mut regex = String::from("^");
    let mut chars = pattern.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '*' => {
                if chars.peek() == Some(&'*') {
                    chars.next();
                    regex.push_str(".*");
                } else {
                    regex.push_str("[^/]*");
                }
            }
            '?' => regex.push_str("[^/]"),
            '.' | '+' | '(' | ')' | '|' | '^' | '$' | '{' | '}' | '[' | ']' | '\\' => {
                regex.push('\\');
                regex.push(ch);
            }
            _ => regex.push(ch),
        }
    }
    regex.push('$');
    Regex::new(&regex)
        .map(|compiled| compiled.is_match(path))
        .unwrap_or(false)
}

fn include_not_found_diagnostic(input: &str, range: TextRange, path: &Path) -> Diagnostic {
    Diagnostic::error(
        Location::from_range(range, input),
        "include-not-found",
        format!("Included file not found: {}", path.display()),
    )
}

pub fn resolve_include_path(raw: &str, base_dir: &Path, project_root: Option<&Path>) -> PathBuf {
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

pub fn is_escaped_shortcode(node: &SyntaxNode) -> bool {
    Shortcode::cast(node.clone()).is_some_and(|shortcode| shortcode.is_escaped())
}

pub fn extract_shortcode_content(node: &SyntaxNode) -> Option<String> {
    Shortcode::cast(node.clone()).and_then(|shortcode| shortcode.content())
}

pub fn split_shortcode_args(content: &str) -> Vec<String> {
    crate::syntax::split_shortcode_args(content)
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
        let graph = {
            let db = crate::salsa::SalsaDb::default();
            let file = crate::salsa::FileText::new(&db, input.clone());
            let config_input = crate::salsa::FileConfig::new(&db, config.clone());
            crate::salsa::project_graph(&db, file, config_input, doc_path.clone()).clone()
        };

        let metadata_deps =
            graph.dependencies(&doc_path, Some(crate::salsa::EdgeKind::MetadataFile));
        assert!(metadata_deps.contains(&root.join("_site.yml")));

        let bib_deps = graph.dependencies(&doc_path, Some(crate::salsa::EdgeKind::Bibliography));
        assert!(bib_deps.contains(&root.join("proj.bib")));
    }

    #[test]
    fn project_graph_builds_from_project_root() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();
        let doc_path = root.join("doc.qmd");
        let other_path = root.join("other.qmd");

        fs::write(root.join("_quarto.yml"), "project: default\n").unwrap();
        fs::write(&doc_path, "See [link][ref].\n").unwrap();
        fs::write(&other_path, "[ref]: https://example.com\n").unwrap();

        let input = fs::read_to_string(&doc_path).unwrap();
        let config = Config::default();
        let graph = {
            let db = crate::salsa::SalsaDb::default();
            let file = crate::salsa::FileText::new(&db, input.clone());
            let config_input = crate::salsa::FileConfig::new(&db, config.clone());
            crate::salsa::project_graph(&db, file, config_input, doc_path.clone()).clone()
        };

        assert!(graph.documents().contains(&doc_path));
        assert!(graph.documents().contains(&other_path));
        let mut definitions = DefinitionIndex::default();
        for path in graph.documents() {
            let Ok(text) = fs::read_to_string(path) else {
                continue;
            };
            let tree = crate::parse(&text, Some(config.clone()));
            crate::includes::collect_cross_doc_duplicates(
                &mut definitions,
                &tree,
                &text,
                path,
                &config,
            );
        }
        assert!(definitions.find_reference("ref").is_some());
    }

    #[test]
    fn find_project_documents_applies_quarto_default_render_rules() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();
        fs::write(root.join("_quarto.yml"), "project: default\n").unwrap();
        fs::create_dir_all(root.join("_partials")).unwrap();
        fs::create_dir_all(root.join(".hidden")).unwrap();
        fs::write(root.join("doc.qmd"), "# Doc\n").unwrap();
        fs::write(root.join("README.qmd"), "# Readme\n").unwrap();
        fs::write(root.join("_partials").join("part.qmd"), "# Part\n").unwrap();
        fs::write(root.join(".hidden").join("hidden.qmd"), "# Hidden\n").unwrap();

        let docs = find_project_documents(root, &Config::default(), false);
        assert!(docs.contains(&root.join("doc.qmd")));
        assert!(!docs.contains(&root.join("README.qmd")));
        assert!(!docs.contains(&root.join("_partials").join("part.qmd")));
        assert!(!docs.contains(&root.join(".hidden").join("hidden.qmd")));
    }

    #[test]
    fn find_project_documents_applies_quarto_render_patterns() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();
        fs::write(
            root.join("_quarto.yml"),
            "project:\n  render:\n    - \"*.qmd\"\n    - \"!ignored.qmd\"\n    - \"!ignored-dir/\"\n    - \"nested/kept.qmd\"\n",
        )
        .unwrap();
        fs::create_dir_all(root.join("nested")).unwrap();
        fs::create_dir_all(root.join("ignored-dir")).unwrap();
        fs::write(root.join("doc.qmd"), "# Doc\n").unwrap();
        fs::write(root.join("ignored.qmd"), "# Ignored\n").unwrap();
        fs::write(root.join("ignored-dir").join("child.qmd"), "# Child\n").unwrap();
        fs::write(root.join("nested").join("kept.qmd"), "# Kept\n").unwrap();

        let docs = find_project_documents(root, &Config::default(), false);
        assert!(docs.contains(&root.join("doc.qmd")));
        assert!(docs.contains(&root.join("nested").join("kept.qmd")));
        assert!(!docs.contains(&root.join("ignored.qmd")));
        assert!(!docs.contains(&root.join("ignored-dir").join("child.qmd")));
    }

    #[test]
    fn project_graph_excludes_non_render_targets_from_project_documents() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();
        let doc_path = root.join("doc.qmd");
        let non_rendered = root.join("non-rendered.qmd");

        fs::write(
            root.join("_quarto.yml"),
            "project:\n  render:\n    - doc.qmd\n",
        )
        .unwrap();
        fs::write(&doc_path, "See [link][ref].\n").unwrap();
        fs::write(&non_rendered, "[ref]: https://example.com\n").unwrap();

        let input = fs::read_to_string(&doc_path).unwrap();
        let config = Config::default();
        let graph = {
            let db = crate::salsa::SalsaDb::default();
            let file = crate::salsa::FileText::new(&db, input.clone());
            let config_input = crate::salsa::FileConfig::new(&db, config.clone());
            crate::salsa::project_graph(&db, file, config_input, doc_path.clone()).clone()
        };

        assert!(graph.documents().contains(&doc_path));
        assert!(!graph.documents().contains(&non_rendered));
    }

    #[test]
    fn project_graph_uses_bookdown_file_list() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();
        let doc_path = root.join("index.Rmd");
        let other_path = root.join("chapter.Rmd");
        let ignored_path = root.join("ignored.Rmd");

        fs::write(
            root.join("_bookdown.yml"),
            "rmd_files: [\"index.Rmd\", \"chapter.Rmd\"]\n",
        )
        .unwrap();
        fs::write(&doc_path, "[ref]: https://example.com\n").unwrap();
        fs::write(&other_path, "See [link][ref].\n").unwrap();
        fs::write(&ignored_path, "[ignored]: https://example.org\n").unwrap();

        let input = fs::read_to_string(&other_path).unwrap();
        let config = Config::default();
        let graph = {
            let db = crate::salsa::SalsaDb::default();
            let file = crate::salsa::FileText::new(&db, input.clone());
            let config_input = crate::salsa::FileConfig::new(&db, config.clone());
            crate::salsa::project_graph(&db, file, config_input, other_path.clone()).clone()
        };

        assert!(graph.documents().contains(&doc_path));
        assert!(graph.documents().contains(&other_path));
        assert!(!graph.documents().contains(&ignored_path));
    }

    #[test]
    fn project_graph_collects_bookdown_definitions() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();
        let doc_path = root.join("doc.Rmd");

        fs::write(
            &doc_path,
            "See \\@ref(fig:plot).\n\n(\\#fig:plot)\n\n(ref:caption)\n",
        )
        .unwrap();

        let input = fs::read_to_string(&doc_path).unwrap();
        let mut config = Config::default();
        config.extensions.bookdown_references = true;
        let _graph = {
            let db = crate::salsa::SalsaDb::default();
            let file = crate::salsa::FileText::new(&db, input.clone());
            let config_input = crate::salsa::FileConfig::new(&db, config.clone());
            crate::salsa::project_graph(&db, file, config_input, doc_path.clone()).clone()
        };

        let mut definitions = DefinitionIndex::default();
        let tree = crate::parse(&input, Some(config.clone()));
        crate::includes::collect_cross_doc_duplicates(
            &mut definitions,
            &tree,
            &input,
            &doc_path,
            &config,
        );
        assert!(definitions.find_crossref("fig:plot").is_some());
        assert!(definitions.find_crossref("caption").is_some());
    }
}

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use crate::config::Config;
use crate::linter::diagnostics::Diagnostic;
use crate::metadata::DocumentMetadata;
use crate::syntax::{
    AstNode, AttributeNode, Citation, CodeBlock, Crossref, FootnoteDefinition, FootnoteReference,
    Heading, Link, ListItem, ParsedYamlRegionSnapshot, ReferenceDefinition, SyntaxKind, SyntaxNode,
    YamlRegion, collect_parsed_yaml_region_snapshots,
};
use crate::utils::{implicit_heading_ids, normalize_anchor_label, normalize_label};
use salsa::{Accumulator, Durability, Setter};

/// Per-file text input. The value is `Option<Arc<str>>` so the writer can
/// distinguish a file it has *referenced but not loaded* (`None` --- a missing
/// include or unreadable bibliography) from a file that is *present but empty*
/// (`Some("")`). That distinction backs the bibliography "failed to read"
/// diagnostic (audit §3.3 / G3). `Arc<str>` lets worker reads share text
/// without cloning.
#[salsa::input]
pub struct FileText {
    #[returns(ref)]
    pub text: Option<Arc<str>>,
}

impl FileText {
    /// Create a *loaded* `FileText` from owned or borrowed text.
    pub fn from_str(db: &dyn Db, text: impl Into<Arc<str>>) -> FileText {
        FileText::new(db, Some(text.into()))
    }

    /// The file's text, or `""` when absent (`None`). Readers that treat an
    /// unloaded file as empty use this; callers that must distinguish absent
    /// from present-but-empty read [`FileText::text`] directly.
    pub fn content_or_empty(self, db: &dyn Db) -> &str {
        self.text(db).as_deref().unwrap_or("")
    }
}

#[salsa::input]
pub struct FileConfig {
    #[returns(ref)]
    pub config: Config,
}

/// The set of [`FileId`]s the writer has interned. `project_graph` reads it to
/// take a dependency on *which files exist*; the writer adds an id on first
/// reference of a path (a real input write), which re-runs `project_graph` so
/// it can resolve and recurse into the newly-referenced file.
///
/// This replaces the former global `CacheGeneration` counter with an in-graph,
/// *structural-only* signal: per-file **content** changes flow through each
/// file's [`FileText`] input and never touch this set, so a sibling load no
/// longer invalidates unrelated documents' `metadata` memos (audit §3.3 / G3).
#[salsa::input]
pub struct FileSet {
    #[returns(ref)]
    pub ids: Arc<HashSet<FileId>>,
}

#[salsa::interned]
pub struct InternedPath<'db> {
    #[returns(ref)]
    pub path: PathBuf,
}

#[salsa::interned]
pub struct InternedLabel<'db> {
    #[returns(ref)]
    pub label: String,
}

pub fn intern_path<'db>(db: &'db dyn Db, path: &Path) -> InternedPath<'db> {
    InternedPath::new(db, path.to_path_buf())
}

pub fn intern_label<'db>(db: &'db dyn Db, label: &str) -> InternedLabel<'db> {
    InternedLabel::new(db, label.to_owned())
}

pub fn intern_normalized_label<'db>(db: &'db dyn Db, label: &str) -> InternedLabel<'db> {
    InternedLabel::new(db, normalize_label(label))
}

pub fn resolve_path(db: &dyn Db, path: InternedPath<'_>) -> PathBuf {
    path.path(db).clone()
}

pub fn resolve_label(db: &dyn Db, label: InternedLabel<'_>) -> String {
    label.label(db).clone()
}

/// Document-scoped reference-definition label set for `(file, config)`.
///
/// Lifted out of [`parsed_tree`] so downstream semantic queries can
/// invalidate independently from CST recomputation. The dialect comes
/// from the config (Pandoc and CommonMark agree on the document-scoped
/// lookup rule, but normalization details may differ in the future).
///
/// Salsa value-equality on `Arc<HashSet<String>>` is set-equality
/// (order-independent), so a paragraph edit that doesn't change refdefs
/// short-circuits at this query and downstream consumers don't see an
/// invalidation pulse.
#[salsa::tracked(returns(ref), lru = 64)]
pub fn refdef_set(db: &dyn Db, file: FileText, config: FileConfig) -> crate::parser::RefdefMap {
    let dialect = panache_parser::Dialect::for_flavor(config.config(db).flavor);
    crate::parser::collect_refdef_labels(file.content_or_empty(db), dialect)
}

/// Parse a `(file, config)` pair to a CST exactly once per `SalsaDb`. All
/// salsa-tracked queries below funnel their parses through this entry point so
/// a single document's lint pipeline (built-in plan, project graph, metadata,
/// definition/usage indexes, ...) shares one parse instead of repeating it
/// per query. The host (`lint_loaded_document_with_includes`) reads the same
/// cached tree directly to avoid an additional standalone parse.
///
/// We cache `GreenNode` (Arc-backed, `Send + Sync`) rather than `SyntaxNode`
/// (which holds non-Send cursor state). Callers wrap the returned green tree
/// in a fresh `SyntaxNode` via [`parsed_tree_root`] — that is cheap (a single
/// atomic clone) and gives each caller its own cursor without leaking the
/// salsa cell.
///
/// The refdef set is consumed via the [`refdef_set`] query so that
/// edits which don't change refdefs short-circuit at the refdef layer
/// without re-scanning the document inside `parse`.
/// A cached parse: the green tree plus the embedded-sublanguage syntax errors
/// (host-ranged malformed YAML) the parser surfaced. Parsed once and cached
/// together so both the tree and the diagnostics are available without a second
/// pass.
#[derive(Debug, Clone)]
pub struct ParsedDocument {
    pub green: rowan::GreenNode,
    pub errors: Vec<crate::parser::SyntaxError>,
}

#[salsa::tracked(returns(ref), lru = 64, no_eq, unsafe(non_update_types))]
pub fn parsed_document(db: &dyn Db, file: FileText, config: FileConfig) -> ParsedDocument {
    let refdefs = refdef_set(db, file, config).clone();
    let (tree, errors) = crate::parser::parse_with_refdefs_and_errors(
        file.content_or_empty(db),
        Some(config.config(db).clone()),
        refdefs,
    );
    ParsedDocument {
        green: tree.green().into_owned(),
        errors,
    }
}

/// The cached green tree for `(file, config)`.
pub fn parsed_tree(db: &dyn Db, file: FileText, config: FileConfig) -> &rowan::GreenNode {
    &parsed_document(db, file, config).green
}

/// The embedded-sublanguage syntax errors (malformed YAML) for `(file, config)`,
/// with host-aligned ranges — ready to turn into diagnostics.
pub fn parse_syntax_errors(
    db: &dyn Db,
    file: FileText,
    config: FileConfig,
) -> &[crate::parser::SyntaxError] {
    &parsed_document(db, file, config).errors
}

/// Materialize the cached parse for `(file, config)` as a fresh `SyntaxNode`.
pub fn parsed_tree_root(db: &dyn Db, file: FileText, config: FileConfig) -> SyntaxNode {
    SyntaxNode::new_root(parsed_tree(db, file, config).clone())
}

#[salsa::tracked(returns(ref), no_eq, unsafe(non_update_types))]
pub fn metadata(db: &dyn Db, file: FileText, config: FileConfig) -> DocumentMetadata {
    // Resolve the document's path from its `FileText` identity; an in-memory
    // buffer has no path, so relative bibliography/metadata paths simply don't
    // resolve (audit §3.3 / G3).
    let path = db.path_of(file).unwrap_or_default();
    let tree = parsed_tree_root(db, file, config);
    let mut metadata =
        crate::metadata::extract_project_metadata_without_bibliography_parse(&tree, &path)
            .unwrap_or_else(|_| crate::metadata::DocumentMetadata {
                source_path: path.clone(),
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
            // Resolve the bibliography to its per-file input and read its
            // content. Taking a dependency on that input's value (`None` when
            // the writer has referenced but not loaded the file) is what re-runs
            // this query once the file loads --- no global firewall needed. A
            // `None` input *or* an absent path is "failed to read"; a present
            // file (even empty, `Some("")`) parses normally, preserving the
            // absent-vs-empty distinction (audit §3.3 / G3).
            let loaded = db
                .file_text(bib_path.clone())
                .filter(|bib_file| bib_file.text(db).is_some());
            let Some(bib_file) = loaded else {
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
) -> Result<(), crate::metadata::YamlError> {
    let path = db.path_of(file).unwrap_or_default();
    let tree = parsed_tree_root(db, file, config);
    crate::metadata::extract_project_metadata_without_bibliography_parse(&tree, &path).map(|_| ())
}

#[salsa::tracked(returns(ref), no_eq, unsafe(non_update_types))]
pub fn yaml_regions_for_file(db: &dyn Db, file: FileText, config: FileConfig) -> Vec<YamlRegion> {
    parsed_yaml_regions_for_file(db, file, config)
        .iter()
        .map(ParsedYamlRegionSnapshot::to_region)
        .collect()
}

#[salsa::tracked(returns(ref), no_eq, unsafe(non_update_types))]
pub fn parsed_yaml_regions_for_file(
    db: &dyn Db,
    file: FileText,
    config: FileConfig,
) -> Vec<ParsedYamlRegionSnapshot> {
    let tree = parsed_tree_root(db, file, config);
    collect_parsed_yaml_region_snapshots(&tree)
}

#[salsa::tracked(returns(ref), no_eq, unsafe(non_update_types))]
pub fn yaml_embedded_regions_in_host_range(
    db: &dyn Db,
    file: FileText,
    config: FileConfig,
    start_offset: usize,
    end_offset: usize,
) -> Vec<YamlRegion> {
    if start_offset >= end_offset {
        return Vec::new();
    }
    yaml_regions_for_file(db, file, config)
        .iter()
        .filter(|region| {
            region.host_range.start < end_offset && start_offset < region.host_range.end
        })
        .cloned()
        .collect()
}

#[salsa::tracked(returns(ref), no_eq, unsafe(non_update_types))]
pub fn yaml_frontmatter_is_valid(db: &dyn Db, file: FileText, config: FileConfig) -> bool {
    let frontmatter = parsed_yaml_regions_for_file(db, file, config)
        .iter()
        .find(|region| region.is_frontmatter())
        .cloned();
    let Some(frontmatter) = frontmatter else {
        // No in-document frontmatter to validate; allow project-file metadata flows.
        return true;
    };
    if !frontmatter.is_valid() {
        return false;
    }
    yaml_metadata_parse_result(db, file, config).is_ok()
}

#[salsa::tracked(returns(ref), no_eq, unsafe(non_update_types), lru = 64)]
pub fn built_in_lint_plan(db: &dyn Db, file: FileText, config: FileConfig) -> BuiltInLintPlan {
    let text = file.content_or_empty(db);
    let cfg = config.config(db).clone();
    let tree = parsed_tree_root(db, file, config);
    let parsed_yaml_regions: Vec<_> = parsed_yaml_regions_for_file(db, file, config).to_vec();
    let frontmatter = parsed_yaml_regions
        .iter()
        .find(|parsed| parsed.is_frontmatter())
        .cloned();
    let frontmatter = frontmatter.as_ref();
    let has_frontmatter = frontmatter.is_some();
    let frontmatter_parse_ok = frontmatter.as_ref().is_none_or(|parsed| parsed.is_valid());
    let yaml = if has_frontmatter && frontmatter_parse_ok {
        Some(yaml_metadata_parse_result(db, file, config).clone())
    } else {
        None
    };
    let metadata = if frontmatter_parse_ok && yaml.as_ref().is_none_or(Result::is_ok) {
        Some(metadata(db, file, config).clone())
    } else {
        None
    };

    let mut diagnostics = Vec::new();
    // YAML *syntax* errors (frontmatter + hashpipe) come straight from the
    // parser's syntax-error channel with host-aligned ranges — no re-parse, no
    // offset remapping. The parser computed these while deciding CST shape.
    diagnostics.extend(
        parse_syntax_errors(db, file, config)
            .iter()
            .filter(|err| err.source == crate::parser::SyntaxErrorSource::Yaml)
            .map(|err| {
                let host_offset: usize = err.range.start().into();
                crate::linter::metadata_diagnostics::yaml_parse_error_at_offset_diagnostic(
                    text,
                    host_offset,
                    Some(err.message.as_str()),
                )
            }),
    );
    // Frontmatter that parses cleanly but whose *metadata extraction* fails is a
    // separate, semantic error (not a YAML syntax error), so it stays on its own
    // path. When frontmatter has a syntax error, `yaml` is `None` (extraction is
    // skipped), so this never double-reports.
    if let Some(Err(yaml_error)) = yaml
        && let Some(diag) =
            crate::linter::metadata_diagnostics::yaml_error_diagnostic(&yaml_error, text)
    {
        diagnostics.push(diag);
    }

    diagnostics.extend(crate::linter::lint_with_metadata(
        &tree,
        text,
        &cfg,
        metadata.as_ref(),
    ));
    diagnostics.sort_by_key(|d| (d.location.line, d.location.column));

    let mut external_jobs = Vec::new();
    if !cfg.linters.is_empty() {
        let code_blocks = crate::utils::collect_code_blocks(&tree, text);
        for (language, linter_name) in &cfg.linters {
            let Some(blocks) = code_blocks.get(language) else {
                continue;
            };
            if blocks.is_empty() {
                continue;
            }
            let concatenated =
                crate::linter::code_block_collector::concatenate_with_blanks_and_mapping(blocks);
            external_jobs.push(ExternalLintJob {
                linter_name: linter_name.clone(),
                language: language.clone(),
                content: concatenated.content,
                mappings: concatenated.mappings,
            });
        }
    }

    BuiltInLintPlan {
        diagnostics,
        external_jobs,
    }
}

#[derive(Debug, Clone, Default)]
pub struct ExternalLintJob {
    pub linter_name: String,
    pub language: String,
    pub content: String,
    pub mappings: Vec<crate::linter::code_block_collector::BlockMapping>,
}

#[derive(Debug, Clone, Default)]
pub struct BuiltInLintPlan {
    pub diagnostics: Vec<crate::linter::Diagnostic>,
    pub external_jobs: Vec<ExternalLintJob>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SymbolUsageIndex {
    citation_usages: HashMap<String, Vec<rowan::TextRange>>,
    citation_references: HashMap<String, Vec<rowan::TextRange>>,
    crossref_usages: HashMap<String, Vec<rowan::TextRange>>,
    example_label_usages: HashMap<String, Vec<rowan::TextRange>>,
    crossref_declarations: HashMap<String, Vec<rowan::TextRange>>,
    crossref_declaration_value_ranges: HashMap<String, Vec<rowan::TextRange>>,
    chunk_label_declaration_ranges: HashMap<String, Vec<rowan::TextRange>>,
    chunk_label_value_ranges: HashMap<String, Vec<rowan::TextRange>>,
    heading_id_value_ranges: HashMap<String, Vec<rowan::TextRange>>,
    heading_link_usages: HashMap<String, Vec<rowan::TextRange>>,
    implicit_heading_insert_ranges: HashMap<String, Vec<rowan::TextRange>>,
    heading_explicit_definition_ranges: HashMap<String, Vec<rowan::TextRange>>,
    heading_implicit_definition_ranges: HashMap<String, Vec<rowan::TextRange>>,
    reference_definitions: HashMap<String, Vec<rowan::TextRange>>,
    footnote_definitions: HashMap<String, Vec<rowan::TextRange>>,
    footnote_references: HashMap<String, Vec<rowan::TextRange>>,
    footnote_definition_id_ranges: HashMap<String, Vec<rowan::TextRange>>,
    example_label_definitions: HashMap<String, Vec<rowan::TextRange>>,
    heading_labels: HashMap<String, Vec<rowan::TextRange>>,
    heading_sequence: Vec<(rowan::TextRange, usize)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeadingOutlineEntry {
    pub title: String,
    pub level: usize,
    pub range: rowan::TextRange,
}

pub(crate) fn is_structural_heading_node(node: &SyntaxNode) -> bool {
    !node.ancestors().skip(1).any(|ancestor| {
        matches!(
            ancestor.kind(),
            SyntaxKind::LIST_ITEM
                | SyntaxKind::BLOCK_QUOTE
                | SyntaxKind::DEFINITION_ITEM
                | SyntaxKind::DEFINITION
                | SyntaxKind::TERM
                | SyntaxKind::FOOTNOTE_DEFINITION
                | SyntaxKind::TABLE_CELL
        )
    })
}

impl SymbolUsageIndex {
    pub fn citation_usages(&self, key: &str) -> Option<&Vec<rowan::TextRange>> {
        self.citation_usages.get(&normalize_label(key))
    }

    pub fn citation_references(&self, key: &str) -> Option<&Vec<rowan::TextRange>> {
        self.citation_references.get(&normalize_label(key))
    }

    pub fn crossref_usages(&self, key: &str) -> Option<&Vec<rowan::TextRange>> {
        self.crossref_usages.get(&normalize_anchor_label(key))
    }

    pub fn example_label_usages(&self, key: &str) -> Option<&Vec<rowan::TextRange>> {
        self.example_label_usages.get(&normalize_label(key))
    }

    pub fn crossref_declarations(&self, key: &str) -> Option<&Vec<rowan::TextRange>> {
        self.crossref_declarations.get(&normalize_anchor_label(key))
    }

    pub fn chunk_label_value_ranges(&self, key: &str) -> Option<&Vec<rowan::TextRange>> {
        self.chunk_label_value_ranges
            .get(&normalize_anchor_label(key))
    }

    pub fn chunk_label_declaration_ranges(&self, key: &str) -> Option<&Vec<rowan::TextRange>> {
        self.chunk_label_declaration_ranges
            .get(&normalize_anchor_label(key))
    }

    pub fn crossref_declaration_value_ranges(&self, key: &str) -> Option<&Vec<rowan::TextRange>> {
        self.crossref_declaration_value_ranges
            .get(&normalize_anchor_label(key))
    }

    pub fn heading_id_value_ranges(&self, key: &str) -> Option<&Vec<rowan::TextRange>> {
        self.heading_id_value_ranges
            .get(&normalize_anchor_label(key))
    }

    pub fn heading_link_usages(&self, key: &str) -> Option<&Vec<rowan::TextRange>> {
        self.heading_link_usages.get(&normalize_label(key))
    }

    pub fn implicit_heading_insert_ranges(&self, key: &str) -> Option<&Vec<rowan::TextRange>> {
        self.implicit_heading_insert_ranges
            .get(&normalize_label(key))
    }

    pub fn crossref_declaration_entries(
        &self,
    ) -> impl Iterator<Item = (&String, &Vec<rowan::TextRange>)> {
        self.crossref_declarations.iter()
    }

    pub fn reference_definitions(&self, key: &str) -> Option<&Vec<rowan::TextRange>> {
        self.reference_definitions.get(&normalize_label(key))
    }

    pub fn footnote_definitions(&self, key: &str) -> Option<&Vec<rowan::TextRange>> {
        self.footnote_definitions.get(&normalize_label(key))
    }

    pub fn footnote_rename_ranges(&self, key: &str) -> Vec<rowan::TextRange> {
        let normalized = normalize_label(key);
        let mut ranges = self
            .footnote_references
            .get(&normalized)
            .cloned()
            .unwrap_or_default();
        if let Some(id_ranges) = self.footnote_definition_id_ranges.get(&normalized) {
            ranges.extend(id_ranges.iter().copied());
        }
        ranges.sort_by_key(|range| range.start());
        ranges.dedup();
        ranges
    }

    pub fn example_label_definitions(&self, key: &str) -> Option<&Vec<rowan::TextRange>> {
        self.example_label_definitions.get(&normalize_label(key))
    }

    pub fn reference_definition_entries(
        &self,
    ) -> impl Iterator<Item = (&String, &Vec<rowan::TextRange>)> {
        self.reference_definitions.iter()
    }

    pub fn footnote_definition_entries(
        &self,
    ) -> impl Iterator<Item = (&String, &Vec<rowan::TextRange>)> {
        self.footnote_definitions.iter()
    }

    pub fn heading_label_entries(&self) -> impl Iterator<Item = (&String, &Vec<rowan::TextRange>)> {
        self.heading_labels.iter()
    }

    pub fn heading_reference_ranges(
        &self,
        key: &str,
        include_declaration: bool,
    ) -> Vec<rowan::TextRange> {
        let anchor_normalized = normalize_anchor_label(key);
        let mut ranges = self
            .heading_link_usages
            .get(&anchor_normalized)
            .cloned()
            .unwrap_or_default();

        if include_declaration
            && let Some(id_ranges) = self.heading_id_value_ranges(&anchor_normalized)
        {
            ranges.extend(id_ranges.iter().copied());
        }

        ranges.sort_by_key(|range| range.start());
        ranges.dedup();
        ranges
    }

    pub fn heading_rename_ranges(&self, key: &str) -> Vec<rowan::TextRange> {
        self.heading_reference_ranges(key, true)
    }

    pub fn heading_explicit_definition_ranges(&self, key: &str) -> Option<&Vec<rowan::TextRange>> {
        self.heading_explicit_definition_ranges
            .get(&normalize_anchor_label(key))
    }

    pub fn heading_implicit_definition_ranges(&self, key: &str) -> Option<&Vec<rowan::TextRange>> {
        self.heading_implicit_definition_ranges
            .get(&normalize_label(key))
    }

    pub fn heading_label_ranges(&self, key: &str) -> Option<&Vec<rowan::TextRange>> {
        self.heading_labels.get(&normalize_label(key))
    }

    pub fn heading_sequence(&self) -> &[(rowan::TextRange, usize)] {
        &self.heading_sequence
    }
}

#[salsa::tracked(returns(ref), lru = 64)]
pub fn symbol_usage_index(db: &dyn Db, file: FileText, config: FileConfig) -> SymbolUsageIndex {
    let tree = parsed_tree_root(db, file, config);
    symbol_usage_index_from_tree(db, &tree, &config.config(db).extensions)
}

#[salsa::tracked(returns(ref), lru = 64)]
pub fn heading_outline(
    db: &dyn Db,
    file: FileText,
    config: FileConfig,
) -> Vec<HeadingOutlineEntry> {
    let tree = parsed_tree_root(db, file, config);
    tree.descendants()
        .filter_map(crate::syntax::Heading::cast)
        .filter(|heading| is_structural_heading_node(heading.syntax()))
        .filter_map(|heading| {
            let level = heading.level();
            if level == 0 {
                return None;
            }

            let title = heading.text();
            Some(HeadingOutlineEntry {
                title: if title.is_empty() {
                    "(empty)".to_string()
                } else {
                    title
                },
                level,
                range: heading.syntax().text_range(),
            })
        })
        .collect()
}

pub fn symbol_usage_index_from_tree(
    db: &dyn Db,
    tree: &SyntaxNode,
    extensions: &crate::config::Extensions,
) -> SymbolUsageIndex {
    let mut index = SymbolUsageIndex::default();

    for def in tree.descendants().filter_map(ReferenceDefinition::cast) {
        db.unwind_if_revision_cancelled();
        let label = normalize_label(&def.label());
        if label.is_empty() {
            continue;
        }
        index
            .reference_definitions
            .entry(label)
            .or_default()
            .push(def.syntax().text_range());
    }

    for def in tree.descendants().filter_map(FootnoteDefinition::cast) {
        db.unwind_if_revision_cancelled();
        let id = normalize_label(&def.id());
        if id.is_empty() {
            continue;
        }
        index
            .footnote_definitions
            .entry(id)
            .or_default()
            .push(def.syntax().text_range());
        if let Some(id_range) = def.id_value_range() {
            index
                .footnote_definition_id_ranges
                .entry(normalize_label(&def.id()))
                .or_default()
                .push(id_range);
        }
    }

    for footnote in tree.descendants().filter_map(FootnoteReference::cast) {
        db.unwind_if_revision_cancelled();
        let id = normalize_label(&footnote.id());
        if id.is_empty() {
            continue;
        }
        if let Some(id_range) = footnote.id_value_range() {
            index
                .footnote_references
                .entry(id)
                .or_default()
                .push(id_range);
        }
    }

    for item in tree.descendants().filter_map(ListItem::cast) {
        db.unwind_if_revision_cancelled();
        if let Some((label, range)) = extract_example_label_definition(&item) {
            index
                .example_label_definitions
                .entry(normalize_label(&label))
                .or_default()
                .push(range);
        }
    }

    for heading in tree.descendants().filter_map(crate::syntax::Heading::cast) {
        db.unwind_if_revision_cancelled();
        let label = normalize_label(&heading.text());
        if label.is_empty() {
            continue;
        }
        index
            .heading_labels
            .entry(label)
            .or_default()
            .push(heading.syntax().text_range());
        let level = heading.level();
        if level > 0 && is_structural_heading_node(heading.syntax()) {
            index
                .heading_sequence
                .push((heading.syntax().text_range(), level));
        }
    }

    for link in tree.descendants().filter_map(Link::cast) {
        db.unwind_if_revision_cancelled();
        if let Some(dest) = link.dest() {
            let Some(id) = dest.hash_anchor_id() else {
                continue;
            };
            let Some(range) = dest.hash_anchor_id_range() else {
                continue;
            };
            index
                .heading_link_usages
                .entry(normalize_anchor_label(&id))
                .or_default()
                .push(range);
            continue;
        }

        if link.reference().is_none()
            && let Some(text) = link.text()
        {
            let label = normalize_label(&text.text_content());
            if label.is_empty() {
                continue;
            }
            index
                .heading_link_usages
                .entry(label)
                .or_default()
                .push(text.syntax().text_range());
        }
    }

    // Implicit-heading shortcut links may also surface as
    // `UNRESOLVED_REFERENCE` (Pandoc dialect with no matching refdef).
    // Index their inner text range so cross-file rename and
    // goto-definition cover both wrappers uniformly.
    for unresolved in tree
        .descendants()
        .filter_map(crate::syntax::UnresolvedReference::cast)
    {
        db.unwind_if_revision_cancelled();
        if unresolved.is_image() || unresolved.label().is_some() {
            continue;
        }
        let label = normalize_label(&unresolved.text());
        if label.is_empty() {
            continue;
        }
        let Some(text_node) = unresolved
            .syntax()
            .children()
            .find(|c| c.kind() == SyntaxKind::LINK_TEXT)
        else {
            continue;
        };
        index
            .heading_link_usages
            .entry(label)
            .or_default()
            .push(text_node.text_range());
    }

    for node in tree
        .descendants()
        .filter(|node| node.kind() == SyntaxKind::CITATION)
    {
        db.unwind_if_revision_cancelled();
        let Some(citation) = Citation::cast(node) else {
            continue;
        };
        for key in citation.keys() {
            index
                .citation_usages
                .entry(normalize_label(&key.text()))
                .or_default()
                .push(key.text_range());
            index
                .citation_references
                .entry(normalize_label(&key.text()))
                .or_default()
                .push(citation.syntax().text_range());
        }
    }

    for node in tree
        .descendants()
        .filter(|node| node.kind() == SyntaxKind::CROSSREF)
    {
        db.unwind_if_revision_cancelled();
        let Some(crossref) = Crossref::cast(node) else {
            continue;
        };
        for key in crossref.keys() {
            index
                .crossref_usages
                .entry(normalize_anchor_label(&key.text()))
                .or_default()
                .push(key.text_range());
        }
    }

    for element in tree.descendants_with_tokens() {
        db.unwind_if_revision_cancelled();
        let Some(token) = element.into_token() else {
            continue;
        };
        match token.kind() {
            SyntaxKind::TEXT => {
                collect_bookdown_declarations_from_text_token(&token, &mut index, extensions);
                collect_example_label_usages_from_text_token(&token, &mut index);
            }
            // Bookdown equation labels `(\#eq:label)` inside math are parsed
            // into a dedicated token; its text is exactly one declaration, so
            // the same scanner registers it (with full + value ranges).
            SyntaxKind::MATH_EQUATION_LABEL => {
                collect_bookdown_declarations_from_text_token(&token, &mut index, extensions);
            }
            _ => {}
        }
    }

    for attribute in tree.descendants().filter_map(AttributeNode::cast) {
        db.unwind_if_revision_cancelled();
        if let Some(id) = attribute.id() {
            index
                .crossref_declarations
                .entry(normalize_anchor_label(&id))
                .or_default()
                .push(attribute.syntax().text_range());
            if let Some(id_range) = attribute.id_value_range() {
                index
                    .crossref_declaration_value_ranges
                    .entry(normalize_anchor_label(&id))
                    .or_default()
                    .push(id_range);
                if attribute
                    .syntax()
                    .ancestors()
                    .any(|ancestor| ancestor.kind() == SyntaxKind::HEADING)
                {
                    index
                        .heading_id_value_ranges
                        .entry(normalize_anchor_label(&id))
                        .or_default()
                        .push(id_range);
                    if let Some(heading) = attribute
                        .syntax()
                        .ancestors()
                        .find(|ancestor| ancestor.kind() == SyntaxKind::HEADING)
                    {
                        index
                            .heading_explicit_definition_ranges
                            .entry(normalize_anchor_label(&id))
                            .or_default()
                            .push(heading.text_range());
                    }
                }
            }
        }
    }

    for span_attrs in tree
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::SPAN_ATTRIBUTES)
    {
        db.unwind_if_revision_cancelled();
        let text = span_attrs.text().to_string();
        let inner = text
            .strip_prefix('{')
            .and_then(|s| s.strip_suffix('}'))
            .unwrap_or(text.as_str());
        let Some(parsed) = crate::parser::utils::attributes::parse_attribute_content(inner) else {
            continue;
        };
        let Some(id) = parsed.identifier.filter(|s| !s.is_empty()) else {
            continue;
        };
        index
            .crossref_declarations
            .entry(normalize_anchor_label(&id))
            .or_default()
            .push(span_attrs.text_range());
    }

    // Pandoc-dialect <div id="..."> attribute regions are exposed
    // structurally as `SyntaxKind::HTML_ATTRS` and recognized by
    // `AttributeNode::cast`, so the descendants walk above already
    // registers their ids in `crossref_declarations`. No dedicated
    // walk needed here.

    for block in tree.descendants().filter_map(CodeBlock::cast) {
        db.unwind_if_revision_cancelled();
        for label in block.chunk_label_entries() {
            let value = label.value().to_string();
            if value.is_empty() {
                continue;
            }
            let normalized_anchor = normalize_anchor_label(&value);

            index
                .crossref_declarations
                .entry(normalized_anchor.clone())
                .or_default()
                .push(label.declaration_range());
            index
                .chunk_label_declaration_ranges
                .entry(normalized_anchor.clone())
                .or_default()
                .push(label.declaration_range());
            index
                .chunk_label_value_ranges
                .entry(normalized_anchor.clone())
                .or_default()
                .push(label.value_range());
            index
                .crossref_declaration_value_ranges
                .entry(normalized_anchor)
                .or_default()
                .push(label.value_range());
        }
    }

    for entry in implicit_heading_ids(tree, extensions) {
        db.unwind_if_revision_cancelled();
        index
            .heading_implicit_definition_ranges
            .entry(normalize_label(&entry.id))
            .or_default()
            .push(entry.heading.text_range());

        if heading_has_explicit_id(&entry.heading) {
            continue;
        }
        let Some(heading) = Heading::cast(entry.heading.clone()) else {
            continue;
        };
        let Some(content) = heading.content() else {
            continue;
        };
        let pos = content.syntax().text_range().end();
        let range = rowan::TextRange::new(pos, pos);
        index
            .implicit_heading_insert_ranges
            .entry(normalize_label(&entry.id))
            .or_default()
            .push(range);
    }

    index
}

fn heading_has_explicit_id(heading: &SyntaxNode) -> bool {
    heading
        .children()
        .filter_map(AttributeNode::cast)
        .any(|attribute| attribute.id().is_some())
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CitationDefinitionLocation {
    pub path: PathBuf,
    pub range: rowan::TextRange,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CitationDefinitionIndex {
    by_key: HashMap<String, Vec<CitationDefinitionLocation>>,
}

impl CitationDefinitionIndex {
    pub fn by_key(&self, key: &str) -> Option<&Vec<CitationDefinitionLocation>> {
        self.by_key.get(&normalize_label(key))
    }
}

#[salsa::tracked(returns(ref), no_eq, unsafe(non_update_types), lru = 64)]
pub fn citation_definition_index(
    db: &dyn Db,
    file: FileText,
    config: FileConfig,
) -> CitationDefinitionIndex {
    let metadata = metadata(db, file, config).clone();
    let mut out = CitationDefinitionIndex::default();

    if let Some(parse) = metadata.bibliography_parse.as_ref() {
        for entry in parse.index.entries.values() {
            out.by_key
                .entry(normalize_label(&entry.key))
                .or_default()
                .push(CitationDefinitionLocation {
                    path: entry.source_file.clone(),
                    range: rowan::TextRange::new(
                        rowan::TextSize::from(entry.span.start as u32),
                        rowan::TextSize::from(entry.span.end as u32),
                    ),
                });
        }
    }

    for inline in &metadata.inline_references {
        out.by_key
            .entry(normalize_label(&inline.id))
            .or_default()
            .push(CitationDefinitionLocation {
                path: inline.path.clone(),
                range: inline.range,
            });
    }

    for values in out.by_key.values_mut() {
        values.sort_by(|a, b| {
            a.path
                .cmp(&b.path)
                .then(a.range.start().cmp(&b.range.start()))
        });
        values.dedup_by(|a, b| a.path == b.path && a.range == b.range);
    }

    out
}

#[salsa::tracked(returns(ref), no_eq, unsafe(non_update_types))]
pub fn bibliography_index(db: &dyn Db, file: FileText, path: PathBuf) -> crate::bib::BibIndex {
    crate::bib::load_bibliography_from_text(file.content_or_empty(db), &path)
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
    example_labels: HashMap<String, DefinitionLocation>,
}

#[derive(Default)]
struct InternedDefinitionIndex<'db> {
    references: HashMap<InternedLabel<'db>, DefinitionLocation>,
    footnotes: HashMap<InternedLabel<'db>, DefinitionLocation>,
    crossrefs: HashMap<InternedLabel<'db>, DefinitionLocation>,
    example_labels: HashMap<InternedLabel<'db>, DefinitionLocation>,
}

#[salsa::tracked(returns(ref), lru = 64)]
pub fn definition_index(db: &dyn Db, file: FileText, config: FileConfig) -> DefinitionIndex {
    // The definitions' source path is the document's own path, resolved from its
    // `FileText` identity (empty for an in-memory buffer) (audit §3.3 / G3).
    let path = db.path_of(file).unwrap_or_default();
    let tree = parsed_tree_root(db, file, config);
    let mut index = InternedDefinitionIndex::default();

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
        insert_reference(db, &mut index, &label, location);
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
        insert_footnote(db, &mut index, &id, location);
    }

    for item in tree.descendants().filter_map(ListItem::cast) {
        db.unwind_if_revision_cancelled();
        let Some((label, range)) = extract_example_label_definition(&item) else {
            continue;
        };
        let location = DefinitionLocation {
            path: path.clone(),
            range,
        };
        insert_example_label(db, &mut index, &label, location);
    }

    for attribute in tree.descendants().filter_map(AttributeNode::cast) {
        db.unwind_if_revision_cancelled();
        if let Some(id) = attribute.id() {
            let location = DefinitionLocation {
                path: path.clone(),
                range: attribute.syntax().text_range(),
            };
            insert_crossref(db, &mut index, &id, location);
        }
    }

    for block in tree.descendants().filter_map(CodeBlock::cast) {
        db.unwind_if_revision_cancelled();
        for label in block.chunk_label_entries() {
            let value = label.value();
            if value.is_empty() {
                continue;
            }
            let location = DefinitionLocation {
                path: path.clone(),
                range: label.declaration_range(),
            };
            insert_crossref(db, &mut index, value, location);
        }
    }

    if config.config(db).extensions.bookdown_references {
        collect_bookdown_definitions(
            db,
            &mut index,
            &tree,
            &path,
            config.config(db).extensions.bookdown_equation_references,
        );
    }

    index.into_owned(db)
}

fn insert_reference<'db>(
    db: &'db dyn Db,
    index: &mut InternedDefinitionIndex<'db>,
    label: &str,
    location: DefinitionLocation,
) {
    let key = intern_normalized_label(db, label);
    index.references.entry(key).or_insert(location);
}

fn insert_footnote<'db>(
    db: &'db dyn Db,
    index: &mut InternedDefinitionIndex<'db>,
    id: &str,
    location: DefinitionLocation,
) {
    let key = intern_normalized_label(db, id);
    index.footnotes.entry(key).or_insert(location);
}

fn insert_crossref<'db>(
    db: &'db dyn Db,
    index: &mut InternedDefinitionIndex<'db>,
    id: &str,
    location: DefinitionLocation,
) {
    let key = intern_label(db, &normalize_anchor_label(id));
    index.crossrefs.entry(key).or_insert(location);
}

fn insert_example_label<'db>(
    db: &'db dyn Db,
    index: &mut InternedDefinitionIndex<'db>,
    label: &str,
    location: DefinitionLocation,
) {
    let key = intern_normalized_label(db, label);
    index.example_labels.entry(key).or_insert(location);
}

impl InternedDefinitionIndex<'_> {
    fn into_owned(self, db: &dyn Db) -> DefinitionIndex {
        DefinitionIndex {
            references: self
                .references
                .into_iter()
                .map(|(label, location)| (resolve_label(db, label), location))
                .collect(),
            footnotes: self
                .footnotes
                .into_iter()
                .map(|(label, location)| (resolve_label(db, label), location))
                .collect(),
            crossrefs: self
                .crossrefs
                .into_iter()
                .map(|(label, location)| (resolve_label(db, label), location))
                .collect(),
            example_labels: self
                .example_labels
                .into_iter()
                .map(|(label, location)| (resolve_label(db, label), location))
                .collect(),
        }
    }
}

impl DefinitionIndex {
    pub fn is_empty(&self) -> bool {
        self.references.is_empty()
            && self.footnotes.is_empty()
            && self.crossrefs.is_empty()
            && self.example_labels.is_empty()
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
        let key = normalize_anchor_label(id);
        self.crossrefs.get(&key)
    }

    pub fn find_example_label(&self, label: &str) -> Option<&DefinitionLocation> {
        let key = normalize_label(label);
        self.example_labels.get(&key)
    }

    pub fn find_crossref_resolved(
        &self,
        id: &str,
        bookdown_references: bool,
    ) -> Option<&DefinitionLocation> {
        for candidate in crate::utils::crossref_resolution_labels(id, bookdown_references) {
            if let Some(location) = self.crossrefs.get(&candidate) {
                return Some(location);
            }
        }
        None
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
        for (key, value) in &other.example_labels {
            self.example_labels
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

fn collect_bookdown_definitions<'db>(
    db: &'db dyn Db,
    index: &mut InternedDefinitionIndex<'db>,
    tree: &SyntaxNode,
    path: &Path,
    collect_equation_definitions: bool,
) {
    // Prose bookdown declarations / text references live in `TEXT` tokens.
    // Bookdown *equation* labels `(\#eq:label)` inside math are parsed into a
    // dedicated `MATH_EQUATION_LABEL` token (gated on the same extension), so
    // we read them straight off the CST rather than re-scanning math text.
    for element in tree.descendants_with_tokens() {
        db.unwind_if_revision_cancelled();
        let Some(token) = element.into_token() else {
            continue;
        };
        match token.kind() {
            SyntaxKind::TEXT => {
                scan_bookdown_definitions_in_text(
                    db,
                    index,
                    path,
                    collect_equation_definitions,
                    token.text(),
                    token.text_range().start().into(),
                );
            }
            SyntaxKind::MATH_EQUATION_LABEL if collect_equation_definitions => {
                // Token text is the whole `(\#eq:label)`.
                if let Some((_len, label)) =
                    crate::parser::inlines::bookdown::try_parse_bookdown_equation_definition(
                        token.text(),
                    )
                {
                    let location = DefinitionLocation {
                        path: path.to_path_buf(),
                        range: token.text_range(),
                    };
                    insert_crossref(db, index, label, location);
                }
            }
            _ => {}
        }
    }
}

/// Scan a single text span for bookdown `(\#...)` declarations and text
/// references, inserting any found into `index`. `base_start` is the document
/// byte offset of `text[0]` so emitted ranges are document-absolute.
fn scan_bookdown_definitions_in_text<'db>(
    db: &'db dyn Db,
    index: &mut InternedDefinitionIndex<'db>,
    path: &Path,
    collect_equation_definitions: bool,
    text: &str,
    base_start: usize,
) {
    use crate::parser::inlines::bookdown::{
        try_parse_bookdown_definition, try_parse_bookdown_equation_definition,
        try_parse_bookdown_text_reference,
    };

    let mut offset = 0usize;
    let bytes = text.as_bytes();
    while offset < bytes.len() {
        db.unwind_if_revision_cancelled();
        if bytes[offset] != b'(' {
            offset += 1;
            continue;
        }
        let slice = &text[offset..];
        if collect_equation_definitions
            && let Some((len, label)) = try_parse_bookdown_equation_definition(slice)
        {
            let range = rowan::TextRange::new(
                rowan::TextSize::from((base_start + offset) as u32),
                rowan::TextSize::from((base_start + offset + len) as u32),
            );
            let location = DefinitionLocation {
                path: path.to_path_buf(),
                range,
            };
            insert_crossref(db, index, label, location);
            offset += len;
            continue;
        }
        if let Some((len, label)) = try_parse_bookdown_definition(slice) {
            if label.starts_with("eq:") && !collect_equation_definitions {
                offset += len;
                continue;
            }
            let range = rowan::TextRange::new(
                rowan::TextSize::from((base_start + offset) as u32),
                rowan::TextSize::from((base_start + offset + len) as u32),
            );
            let location = DefinitionLocation {
                path: path.to_path_buf(),
                range,
            };
            insert_crossref(db, index, label, location);
            offset += len;
            continue;
        }
        if let Some((len, label)) = try_parse_bookdown_text_reference(slice) {
            let range = rowan::TextRange::new(
                rowan::TextSize::from((base_start + offset) as u32),
                rowan::TextSize::from((base_start + offset + len) as u32),
            );
            let location = DefinitionLocation {
                path: path.to_path_buf(),
                range,
            };
            insert_crossref(db, index, label, location);
            offset += len;
            continue;
        }
        offset += 1;
    }
}

fn collect_bookdown_declarations_from_text_token(
    token: &crate::syntax::SyntaxToken,
    index: &mut SymbolUsageIndex,
    extensions: &crate::config::Extensions,
) {
    if !extensions.bookdown_references {
        return;
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
        let Some((len, label)) =
            crate::parser::inlines::bookdown::try_parse_bookdown_definition(slice)
        else {
            offset += 1;
            continue;
        };
        // `(\#eq:...)` declarations are gated on the separate
        // `bookdown_equation_references` extension. Other prefixed
        // declarations (`tab:`, `fig:`, theorem-family, …) and the
        // section-id shorthand follow the generic toggle above.
        if label.starts_with("eq:") && !extensions.bookdown_equation_references {
            offset += len;
            continue;
        }
        let token_start: usize = token.text_range().start().into();
        let full_start = token_start + offset;
        let full_end = full_start + len;
        let value_start = full_start + "(\\#".len();
        let value_end = value_start + label.len();

        index
            .crossref_declarations
            .entry(normalize_anchor_label(label))
            .or_default()
            .push(rowan::TextRange::new(
                rowan::TextSize::from(full_start as u32),
                rowan::TextSize::from(full_end as u32),
            ));
        index
            .crossref_declaration_value_ranges
            .entry(normalize_anchor_label(label))
            .or_default()
            .push(rowan::TextRange::new(
                rowan::TextSize::from(value_start as u32),
                rowan::TextSize::from(value_end as u32),
            ));
        offset += len;
    }
}

fn collect_example_label_usages_from_text_token(
    token: &crate::syntax::SyntaxToken,
    index: &mut SymbolUsageIndex,
) {
    let text = token.text();
    let token_start: usize = token.text_range().start().into();
    for (start, label) in example_label_spans(text) {
        let normalized = normalize_label(label);
        if normalized.is_empty() {
            continue;
        }
        let label_start = rowan::TextSize::from((token_start + start + 2) as u32);
        let label_end = rowan::TextSize::from((token_start + start + 2 + label.len()) as u32);
        let range = rowan::TextRange::new(label_start, label_end);
        index
            .example_label_usages
            .entry(normalized)
            .or_default()
            .push(range);
    }
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

fn parse_example_label(marker: &str) -> Option<&str> {
    let rest = marker.strip_prefix("(@")?;
    let label = rest.strip_suffix(')')?;
    if label.is_empty()
        || !label
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return None;
    }
    Some(label)
}

fn extract_example_label_definition(item: &ListItem) -> Option<(String, rowan::TextRange)> {
    let token = item.syntax().children_with_tokens().find_map(|element| {
        element
            .into_token()
            .filter(|token| token.kind() == SyntaxKind::LIST_MARKER)
    })?;
    let marker = token.text();
    let label = parse_example_label(marker)?;
    let token_start: usize = token.text_range().start().into();
    let start = rowan::TextSize::from((token_start + 2) as u32);
    let end = rowan::TextSize::from((token_start + 2 + label.len()) as u32);
    Some((label.to_string(), rowan::TextRange::new(start, end)))
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

#[derive(Default)]
struct InternedProjectGraph<'db> {
    documents: HashSet<InternedPath<'db>>,
    edges: HashMap<InternedPath<'db>, HashSet<(InternedPath<'db>, EdgeKind)>>,
    reverse_edges: HashMap<InternedPath<'db>, HashSet<(InternedPath<'db>, EdgeKind)>>,
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
}

impl<'db> InternedProjectGraph<'db> {
    fn add_document(&mut self, db: &'db dyn Db, path: &Path) {
        self.documents.insert(intern_path(db, path));
    }

    fn add_edge(&mut self, db: &'db dyn Db, from: &Path, to: &Path, kind: EdgeKind) {
        let from = intern_path(db, from);
        let to = intern_path(db, to);
        self.edges.entry(from).or_default().insert((to, kind));
        self.reverse_edges
            .entry(to)
            .or_default()
            .insert((from, kind));
    }

    fn into_owned(self, db: &dyn Db) -> ProjectGraph {
        ProjectGraph {
            documents: self
                .documents
                .into_iter()
                .map(|path| resolve_path(db, path))
                .collect(),
            edges: self
                .edges
                .into_iter()
                .map(|(from, targets)| {
                    (
                        resolve_path(db, from),
                        targets
                            .into_iter()
                            .map(|(to, kind)| (resolve_path(db, to), kind))
                            .collect(),
                    )
                })
                .collect(),
            reverse_edges: self
                .reverse_edges
                .into_iter()
                .map(|(to, sources)| {
                    (
                        resolve_path(db, to),
                        sources
                            .into_iter()
                            .map(|(from, kind)| (resolve_path(db, from), kind))
                            .collect(),
                    )
                })
                .collect(),
        }
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
pub fn project_graph(db: &dyn Db, root_file: FileText, config: FileConfig) -> ProjectGraph {
    // Depend on the set of interned files so that the writer interning a
    // newly-referenced include/sibling (adding its id to the set) re-runs this
    // query and lets it resolve the new path. Per-file *content* arrival is
    // tracked separately, via each file's `FileText` value read below (audit
    // §3.3 / G3).
    let _ = db.file_set().ids(db);
    let mut graph = InternedProjectGraph::default();
    // A pathless in-memory buffer has no project root and no resolvable
    // includes, so its project graph is empty (audit §3.3 / G3).
    let Some(root_path) = db.path_of(root_file) else {
        return graph.into_owned(db);
    };
    let mut visited = HashSet::new();
    let mut definitions = crate::includes::DefinitionIndex::default();
    visit_document(
        db,
        &root_file,
        config,
        &root_path,
        &mut graph,
        &mut visited,
        &mut definitions,
    );
    let roots = crate::includes::find_project_roots(&root_path);
    if let Some(project_root) = roots.quarto_first() {
        let is_bookdown = roots.bookdown.is_some();
        for path in
            crate::includes::find_project_documents(&project_root, config.config(db), is_bookdown)
        {
            db.unwind_if_revision_cancelled();
            if visited.contains(&path) {
                continue;
            }
            // Record the project document even when it isn't loaded yet, so the
            // writer's `load_project_files` fixpoint can see it in the graph,
            // load it, and re-run (mirrors how includes record an edge before
            // the `file_text` check). Without this, an unloaded sibling would
            // vanish from the graph and never get discovered (audit §3.2).
            graph.add_document(db, &path);
            // Resolve the sibling to its input and read its content (taking a
            // per-file dependency). Recurse only when it is actually loaded; an
            // interned-but-absent file (`None`) records the dependency so a
            // later load re-runs this query and recurses then.
            let loaded = db
                .file_text(path.clone())
                .filter(|include_file| include_file.text(db).is_some());
            if let Some(include_file) = loaded {
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
    graph.into_owned(db)
}

fn visit_document<'db>(
    db: &'db dyn Db,
    file: &FileText,
    config: FileConfig,
    path: &Path,
    graph: &mut InternedProjectGraph<'db>,
    visited: &mut HashSet<PathBuf>,
    definitions: &mut crate::includes::DefinitionIndex,
) {
    if !visited.insert(path.to_path_buf()) {
        return;
    }
    graph.add_document(db, path);
    let text = file.content_or_empty(db);
    let tree = parsed_tree_root(db, *file, config);
    let base_dir = path.parent().unwrap_or_else(|| Path::new("."));
    let project_root = crate::includes::find_project_roots(path).quarto_first();
    let resolution = crate::includes::collect_includes(
        &tree,
        text,
        base_dir,
        project_root.as_deref(),
        config.config(db),
    );
    for include in resolution.includes.iter() {
        db.unwind_if_revision_cancelled();
        graph.add_edge(db, path, &include.path, EdgeKind::Include);
        if include.path == *path {
            continue;
        }
        // Read the include's content (per-file dependency); recurse only when
        // loaded. An interned-but-absent include records the dependency so a
        // later load re-runs `project_graph` and recurses then (audit §3.3).
        let loaded = db
            .file_text(include.path.clone())
            .filter(|include_file| include_file.text(db).is_some());
        if let Some(include_file) = loaded {
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
            graph.add_edge(db, path, metadata_file, EdgeKind::MetadataFile);
        }
        if let Some(bibliography) = metadata.bibliography {
            for bib in bibliography.paths {
                graph.add_edge(db, path, &bib, EdgeKind::Bibliography);
            }
        }
    }
}
#[salsa::db]
pub trait Db: salsa::Database {
    /// Pure lookup of a previously-loaded file. Returns `None` for any path the
    /// writer has not loaded; it never touches the filesystem. Loading is the
    /// writer's responsibility (`crate::lsp::documents::load_project_files`).
    fn file_text(&self, path: PathBuf) -> Option<FileText>;

    /// The immutable backing path for a document's [`FileText`], or `None` for
    /// an in-memory buffer. Path-keyed queries resolve their document path this
    /// way instead of taking a `PathBuf` parameter, so `PathBuf` stops leaking
    /// into analysis and the `<memory>` sentinel is retired (audit §3.3 / G3).
    fn path_of(&self, file: FileText) -> Option<PathBuf>;

    /// The shared [`FileSet`] input. `project_graph` reads it to depend on the
    /// set of interned files; the writer adds ids as it discovers references.
    fn file_set(&self) -> FileSet;
}

/// Opaque, process-stable identity for a file (mirrors rust-analyzer's
/// `vfs::FileId`). A plain newtype --- not a salsa interned struct --- because
/// the LSP boundary must convert URI -> `FileId` synchronously on the main
/// thread, outside any salsa query. Intra-query path interning still goes
/// through [`InternedPath`].
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct FileId(u32);

/// The interior of [`Vfs`]: the path<->id bimap plus the id->input table. Only
/// the writer mutates it (`alloc_id`/`insert`/`remove`); cloned worker handles
/// share the same `Arc<Mutex<_>>` and only read.
#[derive(Default)]
struct VfsInner {
    next_id: u32,
    path_to_id: HashMap<PathBuf, FileId>,
    /// Backing path for each id; `None` for an in-memory buffer with no file on
    /// disk (retires the `<memory>` sentinel).
    id_to_path: HashMap<FileId, Option<PathBuf>>,
    id_to_input: HashMap<FileId, FileText>,
    /// Reverse map: a [`FileText`] input back to its (immutable) backing path.
    /// Lets path-keyed queries resolve a document's path from its `FileText`
    /// identity rather than threading a `PathBuf` parameter (audit §3.3 / G3).
    /// `None` for an in-memory buffer.
    input_to_path: HashMap<FileText, Option<PathBuf>>,
}

/// A `vfs`-style path<->id map that subsumes the former `file_cache`
/// (audit §3.3 / G3). Owned by [`SalsaDb`] behind an `Arc<Mutex<_>>` so cloned
/// worker handles observe the same table.
#[derive(Clone, Default)]
struct Vfs {
    inner: Arc<Mutex<VfsInner>>,
}

impl Vfs {
    fn lock(&self) -> std::sync::MutexGuard<'_, VfsInner> {
        self.inner.lock().expect("vfs lock poisoned")
    }

    fn id_for_path(&self, path: &Path) -> Option<FileId> {
        self.lock().path_to_id.get(path).copied()
    }

    fn input_for_id(&self, id: FileId) -> Option<FileText> {
        self.lock().id_to_input.get(&id).copied()
    }

    fn input_for_path(&self, path: &Path) -> Option<FileText> {
        let inner = self.lock();
        let id = inner.path_to_id.get(path)?;
        inner.id_to_input.get(id).copied()
    }

    fn path_for_id(&self, id: FileId) -> Option<PathBuf> {
        self.lock().id_to_path.get(&id).cloned().flatten()
    }

    /// The immutable backing path for a [`FileText`] input, or `None` for an
    /// in-memory buffer / unregistered input.
    fn path_for_input(&self, input: FileText) -> Option<PathBuf> {
        self.lock().input_to_path.get(&input).cloned().flatten()
    }

    fn cached_paths(&self) -> Vec<PathBuf> {
        self.lock().path_to_id.keys().cloned().collect()
    }

    /// Allocate a fresh id. Called only by the single writer.
    fn alloc_id(&self) -> FileId {
        let mut inner = self.lock();
        let id = FileId(inner.next_id);
        inner.next_id += 1;
        id
    }

    /// Register an id's path and salsa input. Called only by the writer.
    fn insert(&self, id: FileId, path: Option<PathBuf>, input: FileText) {
        let mut inner = self.lock();
        if let Some(path) = path.clone() {
            inner.path_to_id.insert(path, id);
        }
        inner.id_to_path.insert(id, path.clone());
        inner.id_to_input.insert(id, input);
        inner.input_to_path.insert(input, path);
    }

    /// Forget a path's id/input mapping. Returns the removed [`FileId`], if any.
    fn remove_path(&self, path: &Path) -> Option<FileId> {
        let mut inner = self.lock();
        let id = inner.path_to_id.remove(path)?;
        inner.id_to_path.remove(&id);
        if let Some(input) = inner.id_to_input.remove(&id) {
            inner.input_to_path.remove(&input);
        }
        Some(id)
    }
}

#[salsa::db]
#[derive(Clone)]
pub struct SalsaDb {
    storage: salsa::Storage<Self>,
    vfs: Vfs,
    /// The single [`FileSet`] input, shared across cloned handles. Created once
    /// at construction (below) on the writer, so worker reads only ever observe
    /// it, never mint it.
    file_set: Arc<OnceLock<FileSet>>,
}

impl Default for SalsaDb {
    fn default() -> Self {
        let db = Self {
            storage: salsa::Storage::default(),
            vfs: Vfs::default(),
            file_set: Arc::new(OnceLock::new()),
        };
        // Mint the input now (on the constructing thread) so the `OnceLock` is
        // populated before any cloned worker handle reads it.
        db.file_set();
        db
    }
}

impl SalsaDb {
    pub fn file_text_if_cached(&self, path: &Path) -> Option<FileText> {
        self.vfs.input_for_path(path)
    }

    /// Register a brand-new file: allocate a [`FileId`], store its input, and
    /// add the id to the [`FileSet`] so `project_graph` (which depends on the
    /// set) re-runs and can resolve the new path. Writer-only.
    fn register_new(&mut self, path: Option<PathBuf>, input: FileText) -> FileId {
        let id = self.vfs.alloc_id();
        self.vfs.insert(id, path, input);
        self.add_file_to_set(id);
        id
    }

    /// Add `id` to the shared [`FileSet`] input (no-op if already present). The
    /// set is structural-only, so it carries `MEDIUM` durability: a `LOW`
    /// per-keystroke edit never rewrites it.
    fn add_file_to_set(&mut self, id: FileId) {
        let set = self.file_set();
        let next = {
            let current = set.ids(self);
            if current.contains(&id) {
                return;
            }
            let mut next = (**current).clone();
            next.insert(id);
            next
        };
        set.set_ids(self)
            .with_durability(Durability::MEDIUM)
            .to(Arc::new(next));
    }

    /// Return the [`FileId`] for `path`, minting it on first reference. A freshly
    /// minted id gets an *absent* (`None`) text input and is added to the
    /// [`FileSet`]; the writer fills in contents later via
    /// [`SalsaDb::load_file_from_disk`] or a `set_text` update. Pass `None` for
    /// an in-memory buffer with no backing file. Writer-only.
    pub fn intern_file(&mut self, path: Option<PathBuf>) -> FileId {
        if let Some(existing) = path.as_deref().and_then(|p| self.vfs.id_for_path(p)) {
            return existing;
        }
        let input = FileText::new(self, None);
        self.register_new(path, input)
    }

    /// Register an in-memory buffer (no backing path) with initial `text`,
    /// returning its [`FileText`] input. The buffer gets a real [`FileId`] with
    /// `path_of == None`, so it never collides with another untitled buffer and
    /// never needs the `<memory>` sentinel (audit §3.3 / G3). Writer-only.
    pub fn create_in_memory_file(&mut self, text: String, durability: Durability) -> FileText {
        let id = self.intern_file(None);
        let input = self
            .vfs
            .input_for_id(id)
            .expect("input exists for a just-interned id");
        input
            .set_text(self)
            .with_durability(durability)
            .to(Some(Arc::from(text)));
        input
    }

    pub fn load_file_from_disk(&mut self, id: FileId) -> bool {
        self.load_file_from_disk_with_durability(id, Durability::HIGH)
    }

    /// Read `id`'s backing file from disk into its input. Returns `true` only
    /// when this newly populates an absent (`None`) input (`None` -> `Some`);
    /// `false` if `id` has no backing path, is already loaded, or the read
    /// fails. A missing file therefore stays `None`, keeping "absent"
    /// distinguishable from "present but empty". Writer-only.
    pub fn load_file_from_disk_with_durability(
        &mut self,
        id: FileId,
        durability: Durability,
    ) -> bool {
        let Some(path) = self.vfs.path_for_id(id) else {
            return false;
        };
        let Some(input) = self.vfs.input_for_id(id) else {
            return false;
        };
        if input.text(self).is_some() {
            return false;
        }
        let Ok(contents) = std::fs::read_to_string(&path) else {
            return false;
        };
        input
            .set_text(self)
            .with_durability(durability)
            .to(Some(Arc::from(contents)));
        true
    }

    pub fn update_file_text(&mut self, path: PathBuf, text: String) -> FileText {
        self.update_file_text_with_durability(path, text, Durability::LOW)
    }

    pub fn update_file_text_with_durability(
        &mut self,
        path: PathBuf,
        text: String,
        durability: Durability,
    ) -> FileText {
        let text: Arc<str> = Arc::from(text);
        if let Some(file) = self.vfs.input_for_path(&path) {
            file.set_text(self)
                .with_durability(durability)
                .to(Some(text));
            return file;
        }
        let file = FileText::new(self, Some(text.clone()));
        file.set_text(self)
            .with_durability(durability)
            .to(Some(text));
        self.register_new(Some(path), file);
        file
    }

    pub fn update_file_text_if_cached(&mut self, path: &Path, text: String) -> bool {
        self.update_file_text_if_cached_with_durability(path, text, Durability::LOW)
    }

    pub fn update_file_text_if_cached_with_durability(
        &mut self,
        path: &Path,
        text: String,
        durability: Durability,
    ) -> bool {
        let Some(file) = self.vfs.input_for_path(path) else {
            return false;
        };
        file.set_text(self)
            .with_durability(durability)
            .to(Some(Arc::from(text)));
        true
    }

    pub fn ensure_file_text_cached(&mut self, path: PathBuf) -> bool {
        self.ensure_file_text_cached_with_durability(path, Durability::HIGH)
    }

    pub fn ensure_file_text_cached_with_durability(
        &mut self,
        path: PathBuf,
        durability: Durability,
    ) -> bool {
        if self.vfs.input_for_path(&path).is_some() {
            return true;
        }
        let Ok(contents) = std::fs::read_to_string(&path) else {
            return false;
        };
        let contents: Arc<str> = Arc::from(contents);
        let file = FileText::new(self, Some(contents.clone()));
        file.set_text(self)
            .with_durability(durability)
            .to(Some(contents));
        self.register_new(Some(path), file);
        true
    }

    pub fn cached_file_paths(&self) -> Vec<PathBuf> {
        self.vfs.cached_paths()
    }

    pub fn evict_file_text(&mut self, path: &Path) -> bool {
        let Some(id) = self.vfs.remove_path(path) else {
            return false;
        };
        self.remove_file_from_set(id);
        true
    }

    /// Drop `id` from the shared [`FileSet`] input (no-op if absent).
    fn remove_file_from_set(&mut self, id: FileId) {
        let set = self.file_set();
        let next = {
            let current = set.ids(self);
            if !current.contains(&id) {
                return;
            }
            let mut next = (**current).clone();
            next.remove(&id);
            next
        };
        set.set_ids(self)
            .with_durability(Durability::MEDIUM)
            .to(Arc::new(next));
    }

    /// Discover and load every file `project_graph` references for `root_file`,
    /// on the writer, until the referenced set reaches a fixpoint.
    ///
    /// `Db::file_text` is a pure lookup (audit §3.2), so a query only sees files
    /// already loaded. Each pass runs `project_graph` (which records the root,
    /// its included/sibling documents, and bibliography/metadata edges even when
    /// a file is unloaded), then for every referenced path **interns** it (which
    /// mints a `None` input and adds its id to the [`FileSet`] on first
    /// reference --- re-running `project_graph`) and **loads** it from disk. A
    /// fresh `None`->`Some` load flips that file's per-file dependency, again
    /// re-running `project_graph` so it recurses into the freshly-loaded file's
    /// own references. Both convergence channels live inside salsa's dependency
    /// graph; no `CacheGeneration` counter is needed (audit §3.3 / G3).
    ///
    /// Terminates once a pass loads no new content: the referenced set is the
    /// finite transitive closure of `root_path`, each pass only adds inputs, and
    /// a file missing on disk stays `None` (interned once, not retried).
    ///
    /// Returns the final tracked path set (the caller uses it for retention).
    pub fn load_referenced_files(
        &mut self,
        root_file: FileText,
        config: FileConfig,
        root_path: PathBuf,
    ) -> HashSet<PathBuf> {
        loop {
            let tracked = {
                let graph = project_graph(self, root_file, config);
                let mut tracked = HashSet::new();
                tracked.insert(root_path.clone());
                for document in graph.documents() {
                    tracked.insert(document.clone());
                    for dependency in graph.dependencies(document, None) {
                        tracked.insert(dependency);
                    }
                }
                tracked
            };
            let mut progress = false;
            for path in &tracked {
                let id = self.intern_file(Some(path.clone()));
                if self.load_file_from_disk(id) {
                    progress = true;
                }
            }
            if !progress {
                return tracked;
            }
        }
    }
}

/// A read-only view of [`SalsaDb`] handed to worker threads.
///
/// Wraps the salsa handle and exposes only shared (`&dyn Db`) access, so a
/// worker can run read queries but cannot reach the `&mut` setters / input
/// updates that mutate state. This encodes the single-writer invariant the
/// [`StateSnapshot`] doc comment relies on: the main loop's owned `SalsaDb` is
/// the sole writer. Mirrors rust-analyzer's `Analysis` / `AnalysisHost` split.
///
/// [`StateSnapshot`]: crate::lsp::global_state::StateSnapshot
#[derive(Clone)]
pub struct Analysis {
    db: SalsaDb,
}

impl Analysis {
    pub(crate) fn new(db: SalsaDb) -> Self {
        Self { db }
    }

    /// Shared database handle for read queries. Never yields `&mut`.
    pub(crate) fn db(&self) -> &dyn Db {
        &self.db
    }
}

#[salsa::db]
impl salsa::Database for SalsaDb {}

#[salsa::db]
impl Db for SalsaDb {
    // A pure lookup: queries and worker threads observe only files that the
    // writer has already loaded. Discovery-and-load of includes/bibliography is
    // the writer's job (see `crate::lsp::documents::load_project_files`), so
    // this never reads `std::fs` and never creates an input off a `&self` path
    // --- restoring query purity and the single-writer invariant (audit §3.2).
    fn file_text(&self, path: PathBuf) -> Option<FileText> {
        self.file_text_if_cached(&path)
    }

    fn path_of(&self, file: FileText) -> Option<PathBuf> {
        self.vfs.path_for_input(file)
    }

    fn file_set(&self) -> FileSet {
        // Created once on the writer (in `Default`); cloned worker handles
        // share the same `OnceLock` and only ever read it back.
        *self
            .file_set
            .get_or_init(|| FileSet::new(self, Arc::new(HashSet::new())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static STABLE_QUERY_RUNS: AtomicUsize = AtomicUsize::new(0);

    #[salsa::input]
    struct VolatileInput {
        value: u32,
    }

    #[salsa::tracked]
    fn stable_file_len(db: &dyn Db, file: FileText) -> usize {
        STABLE_QUERY_RUNS.fetch_add(1, Ordering::Relaxed);
        file.content_or_empty(db).len()
    }

    #[salsa::tracked]
    fn volatile_probe(db: &dyn Db, volatile: VolatileInput) -> u32 {
        volatile.value(db)
    }

    static PROBE_WITH_SET_RUNS: AtomicUsize = AtomicUsize::new(0);
    static PROBE_WITHOUT_SET_RUNS: AtomicUsize = AtomicUsize::new(0);

    /// Mirrors `project_graph`'s dependency shape: reads the [`FileSet`] (the
    /// structural signal) *and* the file's content.
    #[salsa::tracked]
    fn probe_with_file_set(db: &dyn Db, file: FileText) -> usize {
        PROBE_WITH_SET_RUNS.fetch_add(1, Ordering::Relaxed);
        let _ = db.file_set().ids(db);
        file.content_or_empty(db).len()
    }

    /// Mirrors `metadata`'s dependency shape: reads only the file's content, no
    /// `FileSet` (its bibliography dependency is a separate per-file input).
    #[salsa::tracked]
    fn probe_without_file_set(db: &dyn Db, file: FileText) -> usize {
        PROBE_WITHOUT_SET_RUNS.fetch_add(1, Ordering::Relaxed);
        file.content_or_empty(db).len()
    }

    fn unique_temp_path(stem: &str, suffix: &str) -> PathBuf {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time before unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "panache-{stem}-{}-{now}{suffix}",
            std::process::id()
        ))
    }

    #[test]
    fn intern_normalized_label_collapses_and_lowercases() {
        let db = SalsaDb::default();
        let a = intern_normalized_label(&db, "Foo  Bar");
        let b = intern_normalized_label(&db, "foo bar");
        assert!(a == b);
    }

    #[test]
    fn intern_path_roundtrips_to_owned_path() {
        let db = SalsaDb::default();
        let path = PathBuf::from("/tmp/example.qmd");
        let interned = intern_path(&db, &path);
        assert_eq!(resolve_path(&db, interned), path);
    }

    #[test]
    fn symbol_usage_index_collects_citations_and_crossrefs() {
        let mut db = SalsaDb::default();
        let path = PathBuf::from("/tmp/symbols.qmd");
        let file = db.update_file_text(
            path.clone(),
            "See @fig-plot and [@cite] and [ref].\n\n# Heading\n\n[ref]: https://example.com\n[^a]: footnote\n\n```{r}\n#| label: fig-plot\n1 + 1\n```\n".to_string(),
        );
        let mut cfg = crate::Config {
            flavor: crate::config::Flavor::Quarto,
            extensions: crate::config::Extensions::for_flavor(crate::config::Flavor::Quarto),
            ..Default::default()
        };
        cfg.extensions.quarto_crossrefs = true;
        let config = FileConfig::new(&db, cfg);
        let index = symbol_usage_index(&db, file, config);

        assert_eq!(index.crossref_usages("fig-plot").map(|v| v.len()), Some(1));
        assert_eq!(
            index.crossref_declarations("fig-plot").map(|v| v.len()),
            Some(1)
        );
        assert_eq!(
            index.chunk_label_value_ranges("fig-plot").map(|v| v.len()),
            Some(1)
        );
        assert_eq!(
            index.reference_definition_entries().count(),
            1,
            "expected one reference definition label"
        );
        assert_eq!(
            index.footnote_definition_entries().count(),
            1,
            "expected one footnote definition id"
        );
        assert_eq!(
            index.heading_label_entries().count(),
            1,
            "expected one heading label"
        );
        assert_eq!(index.citation_usages("cite").map(|v| v.len()), Some(1));
    }

    #[test]
    fn symbol_usage_index_collects_example_label_definitions() {
        let db = SalsaDb::default();
        let config = crate::Config {
            flavor: crate::config::Flavor::Pandoc,
            extensions: crate::config::Extensions::for_flavor(crate::config::Flavor::Pandoc),
            ..Default::default()
        };
        let tree = crate::parse(
            "(@good) Good example.\n\n(@bad) Bad example.\n\nAs (@good) illustrates.\n",
            Some(config.clone()),
        );
        let index = symbol_usage_index_from_tree(&db, &tree, &config.extensions);
        assert_eq!(
            index
                .example_label_definitions("good")
                .map(|ranges| ranges.len()),
            Some(1)
        );
        assert_eq!(
            index
                .example_label_definitions("bad")
                .map(|ranges| ranges.len()),
            Some(1)
        );
    }

    #[test]
    fn symbol_usage_index_collects_table_caption_id_for_crossref() {
        let db = SalsaDb::default();
        let mut cfg = crate::Config {
            flavor: crate::config::Flavor::Quarto,
            extensions: crate::config::Extensions::for_flavor(crate::config::Flavor::Quarto),
            ..Default::default()
        };
        cfg.extensions.quarto_crossrefs = true;
        let input = "@tbl-glm\n\n  | Model |\n  | :---- |\n  | A     |\n\n  : {#tbl-glm}\n";
        let tree = crate::parse(input, Some(cfg.clone()));
        let index = symbol_usage_index_from_tree(&db, &tree, &cfg.extensions);

        assert_eq!(
            index.crossref_declarations("tbl-glm").map(|v| v.len()),
            Some(1),
            "table caption attribute should register a crossref declaration"
        );
        let value_ranges = index
            .crossref_declaration_value_ranges("tbl-glm")
            .expect("crossref declaration value range");
        assert_eq!(value_ranges.len(), 1);
        let range = value_ranges[0];
        let start: usize = range.start().into();
        let end: usize = range.end().into();
        assert_eq!(&input[start..end], "tbl-glm");
        assert_eq!(index.crossref_usages("tbl-glm").map(|v| v.len()), Some(1));
    }

    #[test]
    fn symbol_usage_index_collects_display_math_id_no_blank_line() {
        let db = SalsaDb::default();
        let mut cfg = crate::Config {
            flavor: crate::config::Flavor::Quarto,
            extensions: crate::config::Extensions::for_flavor(crate::config::Flavor::Quarto),
            ..Default::default()
        };
        cfg.extensions.quarto_crossrefs = true;
        let input = "$$\na = b\n$$ {#eq-primal-problem}\n@eq-primal-problem\n";
        let tree = crate::parse(input, Some(cfg.clone()));
        let index = symbol_usage_index_from_tree(&db, &tree, &cfg.extensions);

        assert_eq!(
            index
                .crossref_declarations("eq-primal-problem")
                .map(|v| v.len()),
            Some(1)
        );
        let value_ranges = index
            .crossref_declaration_value_ranges("eq-primal-problem")
            .expect("crossref declaration value range");
        assert_eq!(value_ranges.len(), 1);
        let range = value_ranges[0];
        let start: usize = range.start().into();
        let end: usize = range.end().into();
        assert_eq!(&input[start..end], "eq-primal-problem");
        assert_eq!(
            index.crossref_usages("eq-primal-problem").map(|v| v.len()),
            Some(1)
        );
    }

    #[test]
    fn symbol_usage_index_collects_heading_ranges_for_links_and_ids() {
        let db = SalsaDb::default();
        let tree = crate::parse(
            "# Heading {#heading}\n\nSee [heading].\n\nSee [label](#heading).\n",
            None,
        );
        let index = symbol_usage_index_from_tree(&db, &tree, &crate::config::Extensions::default());

        assert_eq!(
            index
                .heading_id_value_ranges("heading")
                .map(|ranges| ranges.len()),
            Some(1)
        );
        assert_eq!(
            index
                .heading_link_usages("heading")
                .map(|ranges| ranges.len()),
            Some(2)
        );
        assert_eq!(index.heading_reference_ranges("heading", true).len(), 3);
        assert_eq!(index.heading_rename_ranges("heading").len(), 3);
    }

    #[test]
    fn symbol_usage_index_collects_footnote_rename_ranges() {
        let db = SalsaDb::default();
        let tree = crate::parse(
            "Text with footnote[^note] and another[^note].\n\n[^note]: Footnote text.\n",
            None,
        );
        let index = symbol_usage_index_from_tree(&db, &tree, &crate::config::Extensions::default());

        assert_eq!(
            index
                .footnote_definitions("note")
                .map(|ranges| ranges.len()),
            Some(1)
        );
        assert_eq!(index.footnote_rename_ranges("note").len(), 3);
    }

    #[test]
    fn symbol_usage_index_collects_implicit_heading_insert_ranges() {
        let db = SalsaDb::default();
        let mut config = crate::Config {
            flavor: crate::config::Flavor::RMarkdown,
            extensions: crate::config::Extensions::for_flavor(crate::config::Flavor::RMarkdown),
            ..Default::default()
        };
        config.extensions.bookdown_references = true;
        let tree = crate::parse(
            "# Heading\n\n## Heading 2\n\nA ref to \\@ref(heading-2).\n",
            Some(config),
        );
        let mut extensions =
            crate::config::Extensions::for_flavor(crate::config::Flavor::RMarkdown);
        extensions.bookdown_references = true;
        let index = symbol_usage_index_from_tree(&db, &tree, &extensions);

        assert_eq!(
            index
                .implicit_heading_insert_ranges("heading-2")
                .map(|ranges| ranges.len()),
            Some(1)
        );
    }

    #[test]
    fn symbol_usage_index_collects_bookdown_equation_declarations_when_enabled() {
        let db = SalsaDb::default();
        let input = "\\begin{align}\n  a (\\#eq:solveG)\n\\end{align}\n\n\\@ref(eq:solveG)\n";
        let mut config = crate::Config {
            flavor: crate::config::Flavor::RMarkdown,
            extensions: crate::config::Extensions::for_flavor(crate::config::Flavor::RMarkdown),
            ..Default::default()
        };
        config.extensions.bookdown_references = true;
        config.extensions.bookdown_equation_references = true;
        let tree = crate::parse(input, Some(config.clone()));
        let index = symbol_usage_index_from_tree(&db, &tree, &config.extensions);

        assert_eq!(index.crossref_usages("eq:solveG").map(|v| v.len()), Some(1));
        assert_eq!(
            index.crossref_declarations("eq:solveG").map(|v| v.len()),
            Some(1)
        );
        assert_eq!(
            index
                .crossref_declaration_value_ranges("eq:solveG")
                .map(|v| v.len()),
            Some(1)
        );
        assert_eq!(index.crossref_declarations("eq:solveg"), None);
    }

    #[test]
    fn symbol_usage_index_skips_bookdown_equation_declarations_when_disabled() {
        let db = SalsaDb::default();
        let input = "\\begin{align}\n  a (\\#eq:foo)\n\\end{align}\n\n\\@ref(eq:foo)\n";
        let mut config = crate::Config {
            flavor: crate::config::Flavor::RMarkdown,
            extensions: crate::config::Extensions::for_flavor(crate::config::Flavor::RMarkdown),
            ..Default::default()
        };
        config.extensions.bookdown_references = true;
        config.extensions.bookdown_equation_references = false;
        let tree = crate::parse(input, Some(config.clone()));
        let index = symbol_usage_index_from_tree(&db, &tree, &config.extensions);

        assert_eq!(index.crossref_usages("eq:foo").map(|v| v.len()), Some(1));
        assert_eq!(index.crossref_declarations("eq:foo"), None);
    }

    #[test]
    fn symbol_usage_index_collects_heading_definition_ranges() {
        let db = SalsaDb::default();
        let tree = crate::parse("# A\n\n# B {#beta}\n", None);
        let index = symbol_usage_index_from_tree(&db, &tree, &crate::config::Extensions::default());

        assert_eq!(
            index
                .heading_implicit_definition_ranges("a")
                .map(|ranges| ranges.len()),
            Some(1)
        );
        assert_eq!(
            index
                .heading_explicit_definition_ranges("beta")
                .map(|ranges| ranges.len()),
            Some(1)
        );
    }

    #[test]
    fn symbol_usage_index_preserves_case_for_anchor_based_crossrefs() {
        let db = SalsaDb::default();
        let tree = crate::parse(
            "# Heading {#em}\n\nSee [a](#em).\n\n# Heading {#EM}\n\nSee [b](#EM).\n",
            None,
        );
        let index = symbol_usage_index_from_tree(&db, &tree, &crate::config::Extensions::default());

        assert_eq!(
            index.crossref_declarations("em").map(|ranges| ranges.len()),
            Some(1)
        );
        assert_eq!(
            index.crossref_declarations("EM").map(|ranges| ranges.len()),
            Some(1)
        );
        assert_eq!(
            index
                .heading_id_value_ranges("em")
                .map(|ranges| ranges.len()),
            Some(1)
        );
        assert_eq!(
            index
                .heading_id_value_ranges("EM")
                .map(|ranges| ranges.len()),
            Some(1)
        );
        assert_eq!(index.heading_reference_ranges("em", true).len(), 2);
        assert_eq!(index.heading_reference_ranges("EM", true).len(), 2);
        assert_eq!(index.heading_reference_ranges("Em", true).len(), 0);
    }

    #[test]
    fn heading_outline_collects_heading_title_level_and_range() {
        let mut db = SalsaDb::default();
        let path = PathBuf::from("/tmp/heading_outline.qmd");
        let file = db.update_file_text(path.clone(), "# Top\n\n## Child\n".to_string());
        let config = FileConfig::new(&db, crate::Config::default());

        let outline = heading_outline(&db, file, config).clone();

        assert_eq!(outline.len(), 2);
        assert_eq!(outline[0].title, "Top");
        assert_eq!(outline[0].level, 1);
        assert_eq!(outline[1].title, "Child");
        assert_eq!(outline[1].level, 2);
    }

    #[test]
    fn symbol_usage_index_heading_sequence_excludes_container_headings() {
        let db = SalsaDb::default();
        let tree = crate::parse(
            "# Top\n\n- # Item Heading\n\nTerm\n: # Definition Heading\n\n> # Quote Heading\n\n## Child\n",
            None,
        );
        let index = symbol_usage_index_from_tree(&db, &tree, &crate::config::Extensions::default());

        let levels: Vec<usize> = index
            .heading_sequence()
            .iter()
            .map(|(_, level)| *level)
            .collect();
        assert_eq!(levels, vec![1, 2]);
    }

    #[test]
    fn heading_outline_excludes_container_headings() {
        let mut db = SalsaDb::default();
        let path = PathBuf::from("/tmp/heading_outline_structural.qmd");
        let file = db.update_file_text(
            path.clone(),
            "# Top\n\n- # Item Heading\n\nTerm\n: # Definition Heading\n\n> # Quote Heading\n\n## Child\n"
                .to_string(),
        );
        let config = FileConfig::new(&db, crate::Config::default());

        let outline = heading_outline(&db, file, config).clone();
        let levels: Vec<usize> = outline.iter().map(|entry| entry.level).collect();
        let titles: Vec<String> = outline.iter().map(|entry| entry.title.clone()).collect();

        assert_eq!(levels, vec![1, 2]);
        assert_eq!(titles, vec!["Top".to_string(), "Child".to_string()]);
    }

    #[test]
    fn yaml_metadata_parse_result_recomputes_after_file_update() {
        let mut db = SalsaDb::default();
        let path = PathBuf::from("/tmp/yaml_recompute.qmd");
        let cfg = crate::Config {
            flavor: crate::config::Flavor::Quarto,
            extensions: crate::config::Extensions::for_flavor(crate::config::Flavor::Quarto),
            ..Default::default()
        };
        let config = FileConfig::new(&db, cfg);

        let file = db.update_file_text(path.clone(), "---\ntitle: [\n---\n\n# Test\n".to_string());
        let first = yaml_metadata_parse_result(&db, file, config).clone();
        assert!(first.is_err(), "expected initial YAML parse failure");

        let fixed = crate::format(
            "---\necho:    false\nlist:\n  -  a\n  -     b\n---\n\n# Test\n",
            None,
            None,
        );
        let file = db.update_file_text(path.clone(), fixed);
        let second = yaml_metadata_parse_result(&db, file, config).clone();
        assert!(second.is_ok(), "expected YAML parse success after update");
    }

    #[test]
    fn yaml_regions_for_file_recomputes_after_file_update() {
        let mut db = SalsaDb::default();
        let cfg = crate::Config {
            flavor: crate::config::Flavor::Quarto,
            extensions: crate::config::Extensions::for_flavor(crate::config::Flavor::Quarto),
            ..Default::default()
        };
        let config = FileConfig::new(&db, cfg);

        let file = db.update_file_text(
            PathBuf::from("/tmp/yaml_regions.qmd"),
            "# Test\n".to_string(),
        );
        let first = yaml_regions_for_file(&db, file, config).clone();
        assert!(
            first.is_empty(),
            "expected no YAML regions in plain markdown input"
        );

        let updated = "---\ntitle: Test\n---\n\n```{r}\n#| echo: false\n1 + 1\n```\n".to_string();
        let file = db.update_file_text(PathBuf::from("/tmp/yaml_regions.qmd"), updated);
        let second = yaml_regions_for_file(&db, file, config).clone();

        assert_eq!(second.len(), 2, "expected frontmatter + hashpipe regions");
        assert!(
            second
                .iter()
                .any(|region| matches!(region.kind, crate::syntax::YamlRegionKind::Frontmatter))
        );
        assert!(
            second
                .iter()
                .any(|region| matches!(region.kind, crate::syntax::YamlRegionKind::Hashpipe))
        );
    }

    #[test]
    fn yaml_embedded_regions_in_host_range_recomputes_after_file_update() {
        let mut db = SalsaDb::default();
        let cfg = crate::Config {
            flavor: crate::config::Flavor::Quarto,
            extensions: crate::config::Extensions::for_flavor(crate::config::Flavor::Quarto),
            ..Default::default()
        };
        let config = FileConfig::new(&db, cfg);

        let file = db.update_file_text(
            PathBuf::from("/tmp/yaml_embedded_regions_update.qmd"),
            "# Test\n".to_string(),
        );
        let first = yaml_embedded_regions_in_host_range(&db, file, config, 0, 6).clone();
        assert!(
            first.is_empty(),
            "expected no YAML regions in plain markdown"
        );

        let updated = "---\ntitle: Test\n---\n\n```{r}\n#| echo: false\n1 + 1\n```\n".to_string();
        let file = db.update_file_text(
            PathBuf::from("/tmp/yaml_embedded_regions_update.qmd"),
            updated.clone(),
        );
        let second =
            yaml_embedded_regions_in_host_range(&db, file, config, 0, updated.len()).clone();

        assert_eq!(
            second.len(),
            2,
            "expected regions for frontmatter + hashpipe"
        );
        assert!(
            second
                .iter()
                .any(|region| matches!(region.kind, crate::syntax::YamlRegionKind::Frontmatter))
        );
        assert!(
            second
                .iter()
                .any(|region| matches!(region.kind, crate::syntax::YamlRegionKind::Hashpipe))
        );
    }

    #[test]
    fn yaml_frontmatter_is_valid_depends_on_region_and_parse_state() {
        let mut db = SalsaDb::default();
        let path = PathBuf::from("/tmp/yaml_validity.qmd");
        let cfg = crate::Config {
            flavor: crate::config::Flavor::Quarto,
            extensions: crate::config::Extensions::for_flavor(crate::config::Flavor::Quarto),
            ..Default::default()
        };
        let config = FileConfig::new(&db, cfg);

        let file = db.update_file_text(path.clone(), "# Test\n".to_string());
        assert!(
            *yaml_frontmatter_is_valid(&db, file, config),
            "no frontmatter should be treated as valid for project metadata flows"
        );

        let file = db.update_file_text(path.clone(), "---\nbibliography: [\n---\n".to_string());
        assert!(
            !*yaml_frontmatter_is_valid(&db, file, config),
            "invalid frontmatter YAML should be invalid"
        );

        let file = db.update_file_text(
            path.clone(),
            "---\nbibliography: refs.bib\n---\n".to_string(),
        );
        assert!(
            *yaml_frontmatter_is_valid(&db, file, config),
            "valid frontmatter YAML should be valid"
        );
    }

    #[test]
    fn built_in_lint_plan_uses_project_bibliography_without_frontmatter() {
        let temp_dir = tempfile::TempDir::new().expect("temp dir");
        let root = temp_dir.path();
        let doc_path = root.join("doc.qmd");
        let bib_path = root.join("refs.bib");
        std::fs::write(root.join("_quarto.yml"), "bibliography: refs.bib\n")
            .expect("project config");
        std::fs::write(&bib_path, "@article{known,\n  title = {Known}\n}\n").expect("bib file");

        let mut db = SalsaDb::default();
        let cfg = crate::Config {
            flavor: crate::config::Flavor::Quarto,
            extensions: crate::config::Extensions::for_flavor(crate::config::Flavor::Quarto),
            ..Default::default()
        };
        let config = FileConfig::new(&db, cfg);

        let _bib_file = db.update_file_text(
            bib_path.clone(),
            "@article{known,\n  title = {Known}\n}\n".to_string(),
        );
        let file = db.update_file_text(doc_path.clone(), "See [@known].\n".to_string());

        let plan = built_in_lint_plan(&db, file, config).clone();
        assert!(
            plan.diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code != "missing-bibliography-key"),
            "project bibliography should satisfy citation key lint without frontmatter"
        );
    }

    #[test]
    fn built_in_lint_plan_reports_frontmatter_yaml_parse_error() {
        let mut db = SalsaDb::default();
        let cfg = crate::Config {
            flavor: crate::config::Flavor::Quarto,
            extensions: crate::config::Extensions::for_flavor(crate::config::Flavor::Quarto),
            ..Default::default()
        };
        let config = FileConfig::new(&db, cfg);
        let path = PathBuf::from("/tmp/lint_yaml_summary_error.qmd");
        let file = db.update_file_text(path.clone(), "---\ntitle: [\n---\n".to_string());

        let plan = built_in_lint_plan(&db, file, config).clone();
        assert!(
            plan.diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "yaml-parse-error"),
            "expected yaml parse diagnostic from invalid frontmatter YAML"
        );
    }

    #[test]
    fn built_in_lint_plan_reports_hashpipe_yaml_parse_error() {
        let mut db = SalsaDb::default();
        let cfg = crate::Config {
            flavor: crate::config::Flavor::Quarto,
            extensions: crate::config::Extensions::for_flavor(crate::config::Flavor::Quarto),
            ..Default::default()
        };
        let config = FileConfig::new(&db, cfg);
        let path = PathBuf::from("/tmp/lint_hashpipe_yaml_error.qmd");
        let input = "```{r}\n#| echo: [\n1 + 1\n```\n".to_string();
        let file = db.update_file_text(path.clone(), input);

        let plan = built_in_lint_plan(&db, file, config).clone();
        assert!(
            plan.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == "yaml-parse-error"
                    && diagnostic.message.contains("YAML parse error")
            }),
            "expected yaml parse diagnostic from invalid hashpipe YAML"
        );
    }

    #[test]
    fn built_in_lint_plan_reports_hashpipe_yaml_parse_error_for_prefixed_continuation_line() {
        let mut db = SalsaDb::default();
        let cfg = crate::Config {
            flavor: crate::config::Flavor::Quarto,
            extensions: crate::config::Extensions::for_flavor(crate::config::Flavor::Quarto),
            ..Default::default()
        };
        let config = FileConfig::new(&db, cfg);
        let path = PathBuf::from("/tmp/lint_hashpipe_yaml_error_continuation.qmd");
        let input = "```{r}\n#| fig-subcap: - \"A\"\n#|   - \"B\"\n1 + 1\n```\n".to_string();
        let file = db.update_file_text(path.clone(), input);

        let plan = built_in_lint_plan(&db, file, config).clone();
        assert!(
            plan.diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "yaml-parse-error"),
            "expected yaml parse diagnostic from invalid hashpipe YAML continuation line"
        );
    }

    #[test]
    fn yaml_embedded_regions_in_host_range_resolves_regions_with_stable_ids() {
        let mut db = SalsaDb::default();
        let cfg = crate::Config {
            flavor: crate::config::Flavor::Quarto,
            extensions: crate::config::Extensions::for_flavor(crate::config::Flavor::Quarto),
            ..Default::default()
        };
        let config = FileConfig::new(&db, cfg);
        let path = PathBuf::from("/tmp/yaml_embedded_regions.qmd");
        let input = "---\ntitle: Test\n---\n\n```{r}\n#| echo: false\n1 + 1\n```\n".to_string();
        let file = db.update_file_text(path, input.clone());

        let regions =
            yaml_embedded_regions_in_host_range(&db, file, config, 0, input.len()).clone();
        assert_eq!(regions.len(), 2, "expected frontmatter + hashpipe regions");
        assert!(regions.iter().any(|region| !region.id.is_empty()));
        assert!(
            regions
                .iter()
                .any(|region| matches!(region.kind, crate::syntax::YamlRegionKind::Frontmatter))
        );
        assert!(
            regions
                .iter()
                .any(|region| matches!(region.kind, crate::syntax::YamlRegionKind::Hashpipe))
        );
    }

    #[test]
    fn high_durability_file_is_not_revalidated_by_low_updates() {
        let mut db = SalsaDb::default();
        STABLE_QUERY_RUNS.store(0, Ordering::Relaxed);

        let stable_path = unique_temp_path("durability-stable-high", ".qmd");
        std::fs::write(&stable_path, "stable high durability").expect("write high durability file");

        assert!(db.ensure_file_text_cached(stable_path.clone()));
        let stable_file = db
            .file_text(stable_path.clone())
            .expect("stable file should be cached");
        let volatile = VolatileInput::new(&db, 0);
        let noisy_path = unique_temp_path("durability-noisy-high", ".qmd");

        let baseline = stable_file_len(&db, stable_file);
        let baseline_runs = STABLE_QUERY_RUNS.load(Ordering::Relaxed);
        assert!(baseline_runs >= 1);

        for i in 1..=20 {
            db.update_file_text(noisy_path.clone(), format!("noisy-{i}"));
            volatile.set_value(&mut db).to(i);
            assert_eq!(volatile_probe(&db, volatile), i);
            assert_eq!(stable_file_len(&db, stable_file), baseline);
        }

        assert_eq!(
            STABLE_QUERY_RUNS.load(Ordering::Relaxed),
            baseline_runs,
            "HIGH durability inputs should not be revalidated on LOW updates"
        );

        let _ = std::fs::remove_file(stable_path);
    }

    /// The core G3 granularity win (audit §3.3): interning an unrelated sibling
    /// (a `FileSet` change) re-runs queries that read the set --- like
    /// `project_graph` --- but NOT per-file readers like `metadata`, whose
    /// bibliography dependency is a per-file input rather than a global firewall.
    /// Under the former global `CacheGeneration` counter, *both* would re-run.
    #[test]
    fn interning_a_sibling_reruns_file_set_readers_but_not_per_file_readers() {
        let mut db = SalsaDb::default();
        let file = db.update_file_text(PathBuf::from("/tmp/g3-granularity-a.md"), "a".to_string());

        PROBE_WITH_SET_RUNS.store(0, Ordering::Relaxed);
        PROBE_WITHOUT_SET_RUNS.store(0, Ordering::Relaxed);

        // Prime both memos.
        probe_with_file_set(&db, file);
        probe_without_file_set(&db, file);
        assert_eq!(PROBE_WITH_SET_RUNS.load(Ordering::Relaxed), 1);
        assert_eq!(PROBE_WITHOUT_SET_RUNS.load(Ordering::Relaxed), 1);

        // Intern an unrelated sibling: this adds an id to the `FileSet` but
        // touches no existing file's content.
        db.intern_file(Some(PathBuf::from("/tmp/g3-granularity-c.md")));

        probe_with_file_set(&db, file);
        probe_without_file_set(&db, file);

        assert_eq!(
            PROBE_WITH_SET_RUNS.load(Ordering::Relaxed),
            2,
            "a FileSet reader (project_graph-shaped) re-runs when a sibling is interned"
        );
        assert_eq!(
            PROBE_WITHOUT_SET_RUNS.load(Ordering::Relaxed),
            1,
            "a per-file reader (metadata-shaped) is NOT re-run by an unrelated sibling load"
        );
    }

    #[test]
    fn file_text_is_a_pure_lookup_and_never_reads_disk() {
        let db = SalsaDb::default();

        // A real file exists on disk, but it was never loaded through a writer
        // method. `file_text` must NOT read it --- it is a pure cache lookup
        // (audit §3.2). Loading is the writer's responsibility.
        let path = unique_temp_path("file-text-purity", ".qmd");
        std::fs::write(&path, "on disk but not loaded").expect("write probe file");

        assert!(
            db.file_text(path.clone()).is_none(),
            "file_text must return None for an unloaded path even when the file exists on disk"
        );

        let _ = std::fs::remove_file(path);
    }
}

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use crate::config::Config;
use crate::incremental::{PrevParse, ReparseAdmission, ReparseCache};
use crate::linter::diagnostics::Diagnostic;
use crate::metadata::DocumentMetadata;
use crate::syntax::{
    AstNode, AttributeNode, Citation, CodeBlock, Crossref, FootnoteDefinition, FootnoteReference,
    Heading, Link, ListItem, ParsedYamlRegionSnapshot, ReferenceDefinition, SyntaxKind, SyntaxNode,
    SyntaxToken, UnresolvedReference, YamlRegion, collect_parsed_yaml_region_snapshots,
};
use crate::utils::{implicit_heading_ids, normalize_anchor_label, normalize_label};
use crate::vfs::Vfs;
use salsa::{Accumulator, Durability, Setter};

// The path<->id<->input map lives in [`crate::vfs`]; re-exported so existing
// `crate::salsa::FileId` paths keep resolving.
pub use crate::vfs::FileId;

// LRU cap shared by every lint-pipeline tracked query below (each `lru = 512`).
// The LSP re-lints *all* open documents per quiescent settle (rust-analyzer's
// model), so the cap must stay comfortably above any realistic open-doc count:
// an edit in a session with more open docs than the cap forces re-validation of
// LRU-evicted memos, turning each settle into a tens-to-hundreds-of-ms storm
// (see the `lsp_relint` bench). 512 keeps every open doc's memo resident for
// sessions far larger than observed in practice. salsa's `lru =` attribute
// requires an integer literal, so the value is repeated rather than a const.

/// Per-file text input. The value is `Option<Arc<str>>` so the writer can
/// distinguish a file it has *referenced but not loaded* (`None` --- a missing
/// include or unreadable bibliography) from a file that is *present but empty*
/// (`Some("")`). That distinction backs the bibliography "failed to read"
/// diagnostic. `Arc<str>` lets worker reads share text
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
/// Content changes flow through each file's [`FileText`] input and do not touch
/// this structural set, avoiding invalidation of unrelated document metadata.
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

pub fn intern_path<'db>(db: &'db dyn Db, path: &Path) -> InternedPath<'db> {
    InternedPath::new(db, path.to_path_buf())
}

pub fn resolve_path(db: &dyn Db, path: InternedPath<'_>) -> PathBuf {
    path.path(db).clone()
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
#[salsa::tracked(returns(ref), lru = 512)]
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
/// Full parses consume the refdef set via [`refdef_set`]. An admitted
/// incremental parse first attempts reuse with the previous parse's set: every
/// successful reparse has already proved that its edit cannot change a
/// definition, so the whole-document scan is deferred until fallback.
/// A cached parse: the green tree plus the embedded-sublanguage syntax errors
/// (host-ranged malformed YAML) the parser surfaced. Parsed once and cached
/// together so both the tree and the diagnostics are available without a second
/// pass.
#[derive(Debug, Clone)]
pub struct ParsedDocument {
    pub green: rowan::GreenNode,
    pub errors: Vec<crate::parser::SyntaxError>,
}

/// Whether a spliced tree spans exactly the text it was spliced from.
///
/// The governing invariant is checked in full by the oracles, but both are
/// `cfg(debug_assertions)` and the host one additionally wants
/// `PANACHE_REPARSE_ORACLE=1` -- they cost a full parse each, which is the
/// thing the feature exists to avoid. This is the one part of the invariant
/// cheap enough to check unconditionally: it is `O(1)`, and it catches the
/// class that does real damage, a splice that drops or duplicates bytes.
/// `parsed_document` feeds every consumer including LSP formatting, which
/// writes the user's file.
///
/// A mismatch falls back to the full parse rather than panicking: refusal is
/// the contract everywhere else in this path, and a release build should
/// degrade to the behavior it had before the feature existed. Debug builds
/// still assert, so a test cannot pass over one.
fn splice_length_agrees(reparsed: &crate::parser::Reparsed, text: &str) -> bool {
    let spliced_len = usize::from(reparsed.green.text_len());
    if spliced_len == text.len() {
        return true;
    }
    log::error!(
        "incremental reparse produced {spliced_len} bytes for {} bytes of text \
         ({} splice over {:?}); falling back to a full parse",
        text.len(),
        reparsed.strategy.as_str(),
        reparsed.reparse_range,
    );
    debug_assert!(
        false,
        "spliced tree disagrees with the text it was spliced from"
    );
    false
}

/// The document's parse, incrementally when the side channel can supply a
/// usable base and a full parse otherwise.
///
/// The result is the same either way: a successful reparse is byte-identical
/// to a full parse of the same text, tree *and* errors (the governing
/// invariant, asserted by the parser crate's debug oracle on every splice and,
/// under `PANACHE_REPARSE_ORACLE=1`, again here against a real full parse
/// before anything is stored, plus [`splice_length_agrees`] in every build).
/// So this stays a pure function of `(file, config)` and salsa still sees only
/// input changes.
///
/// Reuse is refused unless the base was recorded under the *same* config and
/// the *same* refdef set: retained blocks keep the reference resolution and
/// parser options they were parsed with, so either changing invalidates the
/// whole prefix at a distance.
#[salsa::tracked(returns(ref), lru = 512, no_eq, unsafe(non_salsa_values))]
pub fn parsed_document(db: &dyn Db, file: FileText, config: FileConfig) -> ParsedDocument {
    let text = file.text(db).clone().unwrap_or_else(|| Arc::from(""));
    let cfg = config.config(db);

    let admission = db.reparse_base(file, config);
    let (prev, supplied_edit) = match &admission {
        ReparseAdmission::Refused => (None, None),
        ReparseAdmission::Admitted { prev, edit } => (prev.clone(), edit.clone()),
    };

    // Text unchanged: the base already holds what a full parse would return.
    // Salsa usually spares us the call entirely, so this is the cold path --
    // an LRU eviction, or a write that set the text back to what the base
    // holds. Nothing is stored; the base has not moved.
    if let Some(prev) = prev.as_ref().filter(|prev| {
        // The base stores the very `Arc` salsa holds, so an unmoved text proves
        // itself by pointer rather than by walking the document.
        (Arc::ptr_eq(&prev.text, &text) || prev.text == text) && prev.config == *cfg
    }) {
        return ParsedDocument {
            green: prev.green.clone(),
            errors: prev.errors.clone(),
        };
    }

    // Try the cached set before asking salsa to scan the new text. The parser's
    // reparse guard declines every edit that can add, remove, or alter a
    // document-scoped definition; therefore a successful splice proves that
    // `prev.refdefs` is still the set a fresh scan would produce.
    let reused = prev
        .and_then(|prev| {
            if prev.config != *cfg {
                return None;
            }
            let edit = supplied_edit
                .filter(|supplied| Arc::ptr_eq(&supplied.target, &text))
                .and_then(|supplied| supplied.materialize())
                .unwrap_or_else(|| crate::parser::diff_edit(&prev.text, &text));
            crate::parser::reparse_with_refdefs(
                &prev.green,
                &prev.errors,
                &edit,
                &text,
                Some(cfg.clone()),
                prev.refdefs.clone(),
            )
            .map(|reparsed| (reparsed, prev.refdefs.clone()))
        })
        .filter(|(reparsed, _)| splice_length_agrees(reparsed, &text));

    let (parsed, refdefs) = match reused {
        Some((reparsed, refdefs)) => {
            let parsed = ParsedDocument {
                green: reparsed.green,
                errors: reparsed.errors,
            };
            crate::incremental::assert_reuse_matches_full_parse(&parsed, &text, cfg, &refdefs);
            log::trace!(
                "parsed_document: {} splice over {:?}",
                reparsed.strategy.as_str(),
                reparsed.reparse_range
            );
            (parsed, refdefs)
        }
        None => {
            let refdefs = refdef_set(db, file, config).clone();
            let (tree, errors) = crate::parser::parse_with_refdefs_and_errors(
                &text,
                Some(cfg.clone()),
                refdefs.clone(),
            );
            (
                ParsedDocument {
                    green: tree.green().to_owned(),
                    errors,
                },
                refdefs,
            )
        }
    };

    // Store last, after every fallible step, so a panic or a salsa `Cancelled`
    // unwind mid-query can never leave a base whose text and tree disagree. A
    // cancelled query simply stores nothing and the next one diffs from the
    // older base -- a wider edit, still a correct one.
    //
    // Stores are *not* generally ordered across revisions. A staged LSP edit
    // rejects an older store so its exact source remains available; unstaged
    // callers retain the older behavior, where a stale base merely widens the
    // next `diff_edit`.
    if matches!(admission, ReparseAdmission::Admitted { .. }) {
        db.reparse_store(
            file,
            config,
            PrevParse {
                text,
                green: parsed.green.clone(),
                errors: parsed.errors.clone(),
                refdefs,
                config: cfg.clone(),
            },
        );
    }

    parsed
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

#[salsa::tracked(returns(ref), no_eq, unsafe(non_salsa_values))]
pub fn metadata(db: &dyn Db, file: FileText, config: FileConfig) -> DocumentMetadata {
    // Resolve the document's path from its `FileText` identity; an in-memory
    // buffer has no path, so relative bibliography/metadata paths simply don't
    // resolve.
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
            // absent-vs-empty distinction.
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

#[salsa::tracked(returns(ref), no_eq, unsafe(non_salsa_values))]
pub fn yaml_metadata_parse_result(
    db: &dyn Db,
    file: FileText,
    config: FileConfig,
) -> Result<(), crate::metadata::YamlError> {
    let path = db.path_of(file).unwrap_or_default();
    let tree = parsed_tree_root(db, file, config);
    crate::metadata::extract_project_metadata_without_bibliography_parse(&tree, &path).map(|_| ())
}

/// Like [`yaml_metadata_parse_result`], but validates ONLY the document's own
/// frontmatter — no project-manifest (`_quarto.yml` etc.) reads. Project-file
/// errors are surfaced on the manifest's own URI via
/// [`project_manifest_diagnostics`], not misattributed to the open document.
#[salsa::tracked(returns(ref), no_eq, unsafe(non_salsa_values))]
pub fn doc_frontmatter_metadata_result(
    db: &dyn Db,
    file: FileText,
    config: FileConfig,
) -> Result<(), crate::metadata::YamlError> {
    let tree = parsed_tree_root(db, file, config);
    let ctx = crate::parser::yaml::YamlValidationContext::frontmatter(config.config(db).flavor);
    crate::metadata::project::validate_doc_frontmatter(&tree, ctx)
}

/// Per-file YAML parse errors in the project-manifest files reachable from this
/// document's project: `_quarto.yml`, the `_metadata.yml` chain,
/// `_bookdown.yml`/`_output.yml` (`EdgeKind::ProjectConfig`), and
/// `metadata-files:` includes (`EdgeKind::MetadataFile`). Each entry pairs the
/// manifest's path with its parse error so the LSP can publish a diagnostic on
/// the manifest's own URI — rust-analyzer's `Cargo.toml` model.
///
/// Manifest text is read through `db.file_text` (a tracked input loaded by
/// `load_referenced_files`), so editing a manifest re-runs this query — the same
/// invalidation path the bibliography reads use.
#[salsa::tracked(returns(ref), no_eq, unsafe(non_salsa_values))]
pub fn project_manifest_diagnostics(
    db: &dyn Db,
    file: FileText,
    config: FileConfig,
) -> Vec<(PathBuf, crate::metadata::YamlError)> {
    let graph = project_structure(db, file, config);
    let mut manifests: Vec<PathBuf> = Vec::new();
    let mut seen = HashSet::new();
    for document in graph.documents() {
        for kind in [EdgeKind::ProjectConfig, EdgeKind::MetadataFile] {
            for path in graph.dependencies(document, Some(kind)) {
                if seen.insert(path.clone()) {
                    manifests.push(path);
                }
            }
        }
    }

    let mut diagnostics = Vec::new();
    for path in manifests {
        db.unwind_if_revision_cancelled();
        // Tracked read: depending on the input's value re-runs this query when
        // the manifest changes or loads. An interned-but-absent file (`None`)
        // simply contributes no diagnostic.
        let Some(file_text) = db.file_text(path.clone()) else {
            continue;
        };
        let Some(text) = file_text.text(db).as_deref() else {
            continue;
        };
        if let Err(err) = crate::yaml_engine::validate_yaml(text) {
            let offset = err.offset();
            let (line, column) =
                crate::metadata::project::byte_offset_to_line_col_1based(text, offset);
            diagnostics.push((
                path,
                crate::metadata::YamlError::ParseError {
                    message: err.message().to_string(),
                    line: line as u64,
                    column: column as u64,
                    byte_offset: Some(offset),
                },
            ));
        }
    }
    diagnostics
}

/// `quarto-schema` diagnostics for the project-manifest files reachable from this
/// document's project: each `_quarto.yml` (validated against `project-config`)
/// and `_metadata.yml` (validated against `front-matter`). Each entry pairs the
/// manifest's path with its schema diagnostics so the LSP can publish them on the
/// manifest's own URI alongside any parse error.
///
/// Gated on the triggering document's flavor being Quarto and the
/// `quarto-schema` rule being enabled — returns empty otherwise. A `.qmd`
/// document detects as Quarto unless explicitly overridden, so this is the same
/// "Quarto unless overridden" gate the CLI applies to an explicit manifest
/// target (see `lint_quarto_manifest`); the two paths agree on when manifests
/// are validated. Bookdown manifests have no Quarto schema and are skipped (no
/// root). Manifest text is read through `db.file_text`, so editing a manifest
/// re-runs this query.
#[salsa::tracked(returns(ref), no_eq, unsafe(non_salsa_values))]
pub fn project_manifest_schema_diagnostics(
    db: &dyn Db,
    file: FileText,
    config: FileConfig,
) -> Vec<(PathBuf, Vec<Diagnostic>)> {
    let cfg = config.config(db);
    // Type/enum ride the on-by-default `quarto-schema` rule; unknown-key is the
    // opt-in `quarto-schema-unknown-key` rule. Validate when either is on, then
    // filter per code so CLI and LSP gate identically.
    let quarto = cfg.flavor == crate::config::Flavor::Quarto;
    let type_enum_enabled = quarto && cfg.lint.is_rule_enabled("quarto-schema");
    let unknown_key_enabled = quarto
        && cfg
            .lint
            .is_rule_explicitly_enabled("quarto-schema-unknown-key");
    if !type_enum_enabled && !unknown_key_enabled {
        return Vec::new();
    }

    let graph = project_structure(db, file, config);
    let mut manifests: Vec<PathBuf> = Vec::new();
    let mut seen = HashSet::new();
    for document in graph.documents() {
        for kind in [EdgeKind::ProjectConfig, EdgeKind::MetadataFile] {
            for path in graph.dependencies(document, Some(kind)) {
                if seen.insert(path.clone()) {
                    manifests.push(path);
                }
            }
        }
    }

    let mut diagnostics = Vec::new();
    for path in manifests {
        db.unwind_if_revision_cancelled();
        let Some(root) = crate::linter::quarto_schema::manifest_schema_root(&path) else {
            continue;
        };
        let Some(file_text) = db.file_text(path.clone()) else {
            continue;
        };
        let Some(text) = file_text.text(db).as_deref() else {
            continue;
        };
        let diags = crate::linter::quarto_schema::retain_enabled_codes(
            crate::linter::quarto_schema::validate_standalone_yaml(text, root),
            type_enum_enabled,
            unknown_key_enabled,
        );
        if !diags.is_empty() {
            diagnostics.push((path, diags));
        }
    }
    diagnostics
}

#[salsa::tracked(returns(ref), no_eq, unsafe(non_salsa_values))]
pub fn yaml_regions_for_file(db: &dyn Db, file: FileText, config: FileConfig) -> Vec<YamlRegion> {
    parsed_yaml_regions_for_file(db, file, config)
        .iter()
        .map(ParsedYamlRegionSnapshot::to_region)
        .collect()
}

#[salsa::tracked(returns(ref), no_eq, unsafe(non_salsa_values))]
pub fn parsed_yaml_regions_for_file(
    db: &dyn Db,
    file: FileText,
    config: FileConfig,
) -> Vec<ParsedYamlRegionSnapshot> {
    let tree = parsed_tree_root(db, file, config);
    collect_parsed_yaml_region_snapshots(&tree)
}

#[salsa::tracked(returns(ref), no_eq, unsafe(non_salsa_values))]
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

#[salsa::tracked(returns(ref), no_eq, unsafe(non_salsa_values))]
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
    // Document-only: a broken project manifest (`_quarto.yml`) must not mark the
    // document's own frontmatter invalid (its errors surface on the manifest URI).
    doc_frontmatter_metadata_result(db, file, config).is_ok()
}

#[salsa::tracked(returns(ref), no_eq, unsafe(non_salsa_values), lru = 512)]
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
    // The diagnostic below uses a *document-only* parse result: a broken project
    // manifest (`_quarto.yml` etc.) must NOT be misattributed to the open
    // document. Manifest errors are published on the manifest's own URI via
    // `project_manifest_diagnostics`. (`yaml` above stays the full result so a
    // broken manifest still gates metadata-dependent lints.)
    let doc_frontmatter = if has_frontmatter && frontmatter_parse_ok {
        Some(doc_frontmatter_metadata_result(db, file, config).clone())
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
    // Doc frontmatter that parses cleanly but whose *metadata extraction* fails
    // is a separate, semantic error (not a YAML syntax error), so it stays on its
    // own path. When frontmatter has a syntax error, `doc_frontmatter` is `None`
    // (extraction is skipped), so this never double-reports. Project-manifest
    // errors are intentionally excluded here (see `doc_frontmatter`).
    if let Some(Err(yaml_error)) = doc_frontmatter
        && let Some(diag) =
            crate::linter::metadata_diagnostics::yaml_error_diagnostic(&yaml_error, text)
    {
        diagnostics.push(diag);
    }

    // Thread the per-block-memoized symbol index into the rules so
    // symbol-index-backed rules (duplicate/undefined references, undefined
    // anchors, unused definitions, citation keys, heading hierarchy) reuse it
    // instead of each rebuilding one off a throwaway database. Unchanged blocks
    // stay memoized across edits.
    let symbols = symbol_usage_index(db, file, config);
    // Cross-file rules resolve labels/anchors against every document in the same
    // Quarto/bookdown project. Compute the project-wide aggregate once per
    // project (salsa memoizes it across every file in the batch) instead of
    // letting each rule re-parse the siblings. Standalone documents get `None`
    // and resolve locally.
    let project_symbols = project_symbol_index(db, file, config);
    diagnostics.extend(crate::linter::lint_with_metadata_and_symbols(
        &tree,
        text,
        &cfg,
        metadata.as_ref(),
        Some(symbols),
        project_symbols,
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
    /// Bare key text ranges --- what a rename edit replaces.
    citation_usages: HashMap<String, Vec<rowan::TextRange>>,
    /// Key ranges including the `@` marker and any braces --- what a
    /// diagnostic underlines, so a key inside `[@a; @b]` points at itself
    /// rather than the whole bracketed group.
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
    /// Merge `other`'s block-relative ranges into `self`, shifting every range
    /// by `offset` (the block's absolute start in the document).
    ///
    /// Append-only and order-preserving: because blocks are merged in document
    /// order and each block's buckets are already in intra-block document order,
    /// every bucket ends up byte-identical to a single whole-tree walk. The two
    /// implicit-heading buckets are left untouched here — the merge fills them
    /// separately from the one cross-block pass.
    fn merge_offset(&mut self, other: &SymbolUsageIndex, offset: rowan::TextSize) {
        fn merge_map(
            dst: &mut HashMap<String, Vec<rowan::TextRange>>,
            src: &HashMap<String, Vec<rowan::TextRange>>,
            offset: rowan::TextSize,
        ) {
            for (key, ranges) in src {
                dst.entry(key.clone())
                    .or_default()
                    .extend(ranges.iter().map(|range| *range + offset));
            }
        }

        merge_map(&mut self.citation_usages, &other.citation_usages, offset);
        merge_map(
            &mut self.citation_references,
            &other.citation_references,
            offset,
        );
        merge_map(&mut self.crossref_usages, &other.crossref_usages, offset);
        merge_map(
            &mut self.example_label_usages,
            &other.example_label_usages,
            offset,
        );
        merge_map(
            &mut self.crossref_declarations,
            &other.crossref_declarations,
            offset,
        );
        merge_map(
            &mut self.crossref_declaration_value_ranges,
            &other.crossref_declaration_value_ranges,
            offset,
        );
        merge_map(
            &mut self.chunk_label_declaration_ranges,
            &other.chunk_label_declaration_ranges,
            offset,
        );
        merge_map(
            &mut self.chunk_label_value_ranges,
            &other.chunk_label_value_ranges,
            offset,
        );
        merge_map(
            &mut self.heading_id_value_ranges,
            &other.heading_id_value_ranges,
            offset,
        );
        merge_map(
            &mut self.heading_link_usages,
            &other.heading_link_usages,
            offset,
        );
        merge_map(
            &mut self.heading_explicit_definition_ranges,
            &other.heading_explicit_definition_ranges,
            offset,
        );
        merge_map(
            &mut self.reference_definitions,
            &other.reference_definitions,
            offset,
        );
        merge_map(
            &mut self.footnote_definitions,
            &other.footnote_definitions,
            offset,
        );
        merge_map(
            &mut self.footnote_references,
            &other.footnote_references,
            offset,
        );
        merge_map(
            &mut self.footnote_definition_id_ranges,
            &other.footnote_definition_id_ranges,
            offset,
        );
        merge_map(
            &mut self.example_label_definitions,
            &other.example_label_definitions,
            offset,
        );
        merge_map(&mut self.heading_labels, &other.heading_labels, offset);

        self.heading_sequence.extend(
            other
                .heading_sequence
                .iter()
                .map(|(range, level)| (*range + offset, *level)),
        );
    }

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

/// Ordered top-level blocks (`DOCUMENT` children) as owned green subtrees.
///
/// Content-addressed: an edit that changes one block leaves the other blocks'
/// `GreenNode`s structurally equal (locked in by the parser's
/// `incremental_identity` tests), so per-block queries keyed on these values
/// memo-hit for every unchanged block — even when the parse fell back to a full
/// reparse. `no_eq` because the `Vec` itself changes on every edit; the gating
/// happens at the per-block layer.
///
/// `unsafe(non_salsa_values)`: `GreenNode` is not a salsa value. It is an
/// immutable Arc-backed tree carrying no salsa ids, so it is sound to return.
#[salsa::tracked(returns(ref), lru = 512, no_eq, unsafe(non_salsa_values))]
pub fn document_blocks(db: &dyn Db, file: FileText, config: FileConfig) -> Vec<rowan::GreenNode> {
    parsed_tree(db, file, config)
        .children()
        .filter_map(|child| child.into_node().map(|node| node.to_owned()))
        .collect()
}

/// Block-relative symbol index for a single top-level block.
///
/// Ranges are relative to the block start (the walk runs on a fresh root), so
/// the result is position-independent: an edit that merely shifts a block's
/// absolute offset still memo-hits here. [`symbol_usage_index`] offsets these
/// back to absolute ranges when merging. Only the node-local buckets are
/// populated; the cross-block implicit-heading pass runs once in the merge.
///
/// `unsafe(non_salsa_values)`: the interned `GreenNode` argument is not a salsa
/// value. It is an immutable Arc-backed tree carrying no salsa ids, so the
/// interning contract holds. The output backdates on `SymbolUsageIndex`
/// equality (no `no_eq`), giving a second memoization layer.
#[salsa::tracked(returns(ref), unsafe(non_salsa_values))]
pub fn symbol_usage_index_block(
    db: &dyn Db,
    block: rowan::GreenNode,
    config: FileConfig,
) -> SymbolUsageIndex {
    let tree = SyntaxNode::new_root(block);
    let mut index = SymbolUsageIndex::default();
    collect_block_local_symbols(db, &tree, &config.config(db).extensions, &mut index);
    index
}

#[salsa::tracked(returns(ref), lru = 512)]
pub fn symbol_usage_index(db: &dyn Db, file: FileText, config: FileConfig) -> SymbolUsageIndex {
    // Merge the memoized per-block node-local indexes, shifting each block's
    // relative ranges to absolute by prefix-summing block lengths. Unchanged
    // blocks are cache hits, so only edited blocks are re-walked.
    let mut index = SymbolUsageIndex::default();
    let mut offset = rowan::TextSize::from(0);
    for block in document_blocks(db, file, config) {
        let block_index = symbol_usage_index_block(db, block.clone(), config);
        index.merge_offset(block_index, offset);
        offset += block.text_len();
    }

    // The per-block offset math assumes `document_blocks` (which keeps only node
    // children) drops nothing, i.e. `DOCUMENT` has no direct token children. If a
    // future parser change breaks that invariant, `offset` undercounts and every
    // range past the stray token silently desyncs — assert it loudly in debug.
    debug_assert_eq!(
        offset,
        parsed_tree(db, file, config).text_len(),
        "document_blocks dropped a top-level token child; per-block offsets desynced"
    );

    // Implicit heading ids dedup slugs across the whole document, so they cannot
    // be computed per block. Run that one cross-block pass over the full tree;
    // its ranges are already absolute.
    let tree = parsed_tree_root(db, file, config);
    collect_implicit_heading_symbols(db, &tree, &config.config(db).extensions, &mut index);
    index
}

/// One document's contribution to its project's symbol aggregate: the
/// definition labels, anchors, and usage labels the cross-file reference rules
/// (`undefined-references`, `unused-definitions`, `undefined-anchor`) resolve
/// against. Memoized per `(file, config)`, so each project document is folded
/// once regardless of how many sibling documents aggregate it or how many
/// times the batch re-lints.
#[salsa::tracked(returns(ref), lru = 512)]
pub fn project_document_contribution(
    db: &dyn Db,
    file: FileText,
    config: FileConfig,
) -> crate::linter::project_index::ProjectSymbolIndex {
    let tree = parsed_tree_root(db, file, config);
    let symbols = symbol_usage_index(db, file, config);
    let mut contribution = crate::linter::project_index::ProjectSymbolIndex::default();
    contribution.fold_document(&tree, symbols, config.config(db));
    contribution
}

/// The project-wide symbol aggregate for the project rooted at `root`: the
/// union of every project document's [`project_document_contribution`].
///
/// Keyed on the interned project *root* (not the current file), so every
/// document in a project shares one memo — the project's files are enumerated
/// and folded once for the whole batch, not once per file (the O(n^2) fix). The
/// heavy per-document work lives in the memoized contribution query, so the
/// aggregation itself is just cheap set unions.
#[salsa::tracked(returns(ref), lru = 512)]
pub fn project_symbol_index_for<'db>(
    db: &'db dyn Db,
    root: InternedPath<'db>,
    is_bookdown: bool,
    config: FileConfig,
) -> crate::linter::project_index::ProjectSymbolIndex {
    // Depend on the set of interned files so loading a newly-referenced project
    // document re-runs this aggregate (mirrors `project_structure`).
    let _ = db.file_set().ids(db);
    let project_root = root.path(db);
    let mut aggregate = crate::linter::project_index::ProjectSymbolIndex::default();
    for path in
        crate::includes::find_project_documents(project_root, config.config(db), is_bookdown)
    {
        db.unwind_if_revision_cancelled();
        let Some(doc_file) = db.file_text(path.clone()) else {
            continue;
        };
        if !file_is_present(db, doc_file) {
            continue;
        }
        aggregate.extend(project_document_contribution(db, doc_file, config));
    }
    aggregate
}

/// The project-wide symbol aggregate for the project that `file` belongs to, or
/// `None` when `file` is standalone (not under a Quarto/bookdown manifest).
///
/// A thin resolver over [`project_symbol_index_for`]: it maps the document to
/// its project root and delegates to the project-keyed memo, so all files in one
/// project hit the same cached aggregate.
pub fn project_symbol_index(
    db: &dyn Db,
    file: FileText,
    config: FileConfig,
) -> Option<&crate::linter::project_index::ProjectSymbolIndex> {
    // Use the interned path as-is: loading (`load_referenced_files`) and
    // `project_graph` key files on this exact form, never a canonical one.
    // Canonicalizing here diverged from them whenever the path ran through a
    // symlink (e.g. macOS's `/var` -> `/private/var`), so the aggregate looked
    // siblings up by a form no loaded file matched and folded nothing.
    let doc_path = db.path_of(file)?;
    let roots = crate::includes::find_project_roots(&doc_path);
    let project_root = roots.quarto_first()?;
    let is_bookdown = roots.bookdown.is_some();
    let root = intern_path(db, &project_root);
    Some(project_symbol_index_for(db, root, is_bookdown, config))
}

#[salsa::tracked(returns(ref), lru = 512)]
pub fn heading_outline(
    db: &dyn Db,
    file: FileText,
    config: FileConfig,
) -> Vec<HeadingOutlineEntry> {
    // Merge the memoized per-block outlines, shifting each block's relative
    // heading ranges to absolute by prefix-summing block lengths. The outline is
    // config-independent, so per-block memos hit for every unchanged block.
    let mut outline = Vec::new();
    let mut offset = rowan::TextSize::from(0);
    for block in document_blocks(db, file, config) {
        for entry in heading_outline_block(db, block.clone(), config) {
            outline.push(HeadingOutlineEntry {
                title: entry.title.clone(),
                level: entry.level,
                range: entry.range + offset,
            });
        }
        offset += block.text_len();
    }
    // Guard the same invariant as `symbol_usage_index`: summed block lengths must
    // equal the full document length, else `document_blocks` dropped a top-level
    // token child and the absolute ranges above are desynced.
    debug_assert_eq!(
        offset,
        parsed_tree(db, file, config).text_len(),
        "document_blocks dropped a top-level token child; per-block offsets desynced"
    );
    outline
}

/// Block-relative structural-heading outline for a single top-level block.
///
/// The outline is config-independent; `config` only rides along to give salsa a
/// third argument to intern (a non-salsa `GreenNode` alone can't be a query
/// key). `unsafe(non_salsa_values)`: the interned `GreenNode` argument is an
/// immutable Arc-backed tree carrying no salsa ids.
#[salsa::tracked(returns(ref), unsafe(non_salsa_values))]
pub fn heading_outline_block(
    db: &dyn Db,
    block: rowan::GreenNode,
    config: FileConfig,
) -> Vec<HeadingOutlineEntry> {
    let _ = (db, config);
    let tree = SyntaxNode::new_root(block);
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
    collect_block_local_symbols(db, tree, extensions, &mut index);
    collect_implicit_heading_symbols(db, tree, extensions, &mut index);
    index
}

/// Collect every node-local symbol bucket for `tree` into `index`.
///
/// "Node-local" means each range derives from a single node or token with no
/// cross-node state, so this is safe to run per top-level block and merge with
/// offsets — the basis of the salsa per-block memoization in
/// [`symbol_usage_index`]. The one cross-block pass (implicit heading ids, which
/// dedups slugs document-wide) lives in [`collect_implicit_heading_symbols`].
fn collect_block_local_symbols(
    db: &dyn Db,
    tree: &SyntaxNode,
    extensions: &crate::config::Extensions,
    index: &mut SymbolUsageIndex,
) {
    // Single pre-order walk buckets every node/token kind the passes below
    // consume, replacing the ~12 separate `tree.descendants()` traversals that
    // used to dominate this query. The replay loops are otherwise unchanged and
    // run in their original order, so each `SymbolUsageIndex` field's push order
    // stays byte-identical — value-equality (and the salsa short-circuiting that
    // depends on it) is preserved. Covered by `folded_index_matches_reference`.
    let mut reference_definitions: Vec<ReferenceDefinition> = Vec::new();
    let mut footnote_definitions: Vec<FootnoteDefinition> = Vec::new();
    let mut footnote_references: Vec<FootnoteReference> = Vec::new();
    let mut list_items: Vec<ListItem> = Vec::new();
    let mut headings: Vec<Heading> = Vec::new();
    let mut links: Vec<Link> = Vec::new();
    let mut unresolved_references: Vec<UnresolvedReference> = Vec::new();
    let mut citation_nodes: Vec<SyntaxNode> = Vec::new();
    let mut crossref_nodes: Vec<SyntaxNode> = Vec::new();
    let mut text_tokens: Vec<SyntaxToken> = Vec::new();
    let mut attribute_nodes: Vec<AttributeNode> = Vec::new();
    let mut span_attribute_nodes: Vec<SyntaxNode> = Vec::new();
    let mut code_blocks: Vec<CodeBlock> = Vec::new();

    for element in tree.descendants_with_tokens() {
        if let Some(node) = element.as_node() {
            db.unwind_if_revision_cancelled();
            if let Some(cast) = ReferenceDefinition::cast(node.clone()) {
                reference_definitions.push(cast);
            }
            if let Some(cast) = FootnoteDefinition::cast(node.clone()) {
                footnote_definitions.push(cast);
            }
            if let Some(cast) = FootnoteReference::cast(node.clone()) {
                footnote_references.push(cast);
            }
            if let Some(cast) = ListItem::cast(node.clone()) {
                list_items.push(cast);
            }
            if let Some(cast) = Heading::cast(node.clone()) {
                headings.push(cast);
            }
            if let Some(cast) = Link::cast(node.clone()) {
                links.push(cast);
            }
            if let Some(cast) = UnresolvedReference::cast(node.clone()) {
                unresolved_references.push(cast);
            }
            if node.kind() == SyntaxKind::CITATION {
                citation_nodes.push(node.clone());
            }
            if node.kind() == SyntaxKind::CROSSREF {
                crossref_nodes.push(node.clone());
            }
            if let Some(cast) = AttributeNode::cast(node.clone()) {
                attribute_nodes.push(cast);
            }
            if node.kind() == SyntaxKind::SPAN_ATTRIBUTES {
                span_attribute_nodes.push(node.clone());
            }
            if let Some(cast) = CodeBlock::cast(node.clone()) {
                code_blocks.push(cast);
            }
        } else if let Some(token) = element.as_token()
            && matches!(
                token.kind(),
                SyntaxKind::TEXT | SyntaxKind::MATH_EQUATION_LABEL
            )
        {
            text_tokens.push(token.clone());
        }
    }

    for def in reference_definitions {
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

    for def in footnote_definitions {
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

    for footnote in footnote_references {
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

    for item in list_items {
        db.unwind_if_revision_cancelled();
        if let Some((label, range)) = extract_example_label_definition(&item) {
            index
                .example_label_definitions
                .entry(normalize_label(&label))
                .or_default()
                .push(range);
        }
    }

    for heading in headings {
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

    for link in links {
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
    for unresolved in unresolved_references {
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

    for node in citation_nodes {
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
                .push(key.marked_range());
        }
    }

    for node in crossref_nodes {
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

    for token in text_tokens {
        db.unwind_if_revision_cancelled();
        match token.kind() {
            SyntaxKind::TEXT => {
                collect_bookdown_declarations_from_text_token(&token, index, extensions);
                collect_example_label_usages_from_text_token(&token, index);
            }
            // Host-owned Bookdown equation-label tokens contain exactly one
            // declaration, so the same scanner registers full and value ranges.
            SyntaxKind::MATH_EQUATION_LABEL => {
                collect_bookdown_declarations_from_text_token(&token, index, extensions);
            }
            _ => {}
        }
    }

    for attribute in attribute_nodes {
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

    for span_attrs in span_attribute_nodes {
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

    for block in code_blocks {
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
}

/// Cross-block pass: implicit heading ids dedup slugs document-wide (pandoc's
/// `id`, `id-1`, `id-2` suffixing), so this must run once over the whole tree,
/// never per block. Ranges are absolute; [`symbol_usage_index`] runs it after
/// merging the per-block node-local buckets.
fn collect_implicit_heading_symbols(
    db: &dyn Db,
    tree: &SyntaxNode,
    extensions: &crate::config::Extensions,
    index: &mut SymbolUsageIndex,
) {
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

#[salsa::tracked(returns(ref), no_eq, unsafe(non_salsa_values), lru = 512)]
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

#[salsa::tracked(returns(ref), no_eq, unsafe(non_salsa_values))]
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

#[salsa::tracked(returns(ref), lru = 512)]
pub fn definition_index(db: &dyn Db, file: FileText, config: FileConfig) -> DefinitionIndex {
    // The definitions' source path is the document's own path, resolved from its
    // `FileText` identity (empty for an in-memory buffer).
    let path = db.path_of(file).unwrap_or_default();
    let tree = parsed_tree_root(db, file, config);
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

    for item in tree.descendants().filter_map(ListItem::cast) {
        db.unwind_if_revision_cancelled();
        let Some((label, range)) = extract_example_label_definition(&item) else {
            continue;
        };
        let location = DefinitionLocation {
            path: path.clone(),
            range,
        };
        insert_example_label(&mut index, &label, location);
    }

    for attribute in tree.descendants().filter_map(AttributeNode::cast) {
        db.unwind_if_revision_cancelled();
        if let Some(id) = attribute.id() {
            let location = DefinitionLocation {
                path: path.clone(),
                range: attribute.syntax().text_range(),
            };
            insert_crossref(&mut index, &id, location);
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
            insert_crossref(&mut index, value, location);
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
    let key = normalize_anchor_label(id);
    index.crossrefs.entry(key).or_insert(location);
}

fn insert_example_label(index: &mut DefinitionIndex, label: &str, location: DefinitionLocation) {
    let key = normalize_label(label);
    index.example_labels.entry(key).or_insert(location);
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

fn collect_bookdown_definitions(
    db: &dyn Db,
    index: &mut DefinitionIndex,
    tree: &SyntaxNode,
    path: &Path,
    collect_equation_definitions: bool,
) {
    // Prose bookdown declarations / text references live in `TEXT` tokens.
    // Bookdown equation labels are host-owned `MATH_EQUATION_LABEL` siblings of
    // TeX content (gated on the same extension), so read them straight off the
    // CST rather than re-scanning math text.
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
                    insert_crossref(index, label, location);
                }
            }
            _ => {}
        }
    }
}

/// Scan a single text span for bookdown `(\#...)` declarations and text
/// references, inserting any found into `index`. `base_start` is the document
/// byte offset of `text[0]` so emitted ranges are document-absolute.
fn scan_bookdown_definitions_in_text(
    db: &dyn Db,
    index: &mut DefinitionIndex,
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
            insert_crossref(index, label, location);
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
            insert_crossref(index, label, location);
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
            insert_crossref(index, label, location);
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
    /// A project-manifest config file (`_quarto.yml`, `_metadata.yml`,
    /// `_bookdown.yml`, `_output.yml`) the document inherits metadata from.
    /// Distinct from `MetadataFile` (a `metadata-files:` include): these are the
    /// project-root/ancestor configs resolved by directory walk.
    ProjectConfig,
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

#[salsa::tracked(returns(ref), lru = 512)]
pub fn project_graph(db: &dyn Db, root_file: FileText, config: FileConfig) -> ProjectGraph {
    // Depend on the set of interned files so that the writer interning a
    // newly-referenced include/sibling (adding its id to the set) re-runs this
    // query and lets it resolve the new path. Per-file *content* arrival is
    // tracked separately through each file's `FileText` value below.
    let _ = db.file_set().ids(db);
    let mut graph = InternedProjectGraph::default();
    // A pathless in-memory buffer has no project root and no resolvable
    // includes, so its project graph is empty.
    let Some(root_path) = db.path_of(root_file) else {
        return graph.into_owned(db);
    };
    let mut visited = HashSet::new();
    let roots = crate::includes::find_project_roots(&root_path);
    // Footnote and reference-link labels collide across files only within a
    // single merged Pandoc pass. Bookdown concatenates all chapters into one
    // such pass, so the whole project shares one `DefinitionIndex`. Quarto
    // renders each document separately, so every render target gets its own
    // index (its `{{< include >}}` children share it, but sibling render
    // targets do not cross-contaminate). See `collect_cross_doc_duplicates`.
    let is_bookdown = roots.bookdown.is_some();
    let mut shared_definitions = crate::includes::DefinitionIndex::default();
    let mut root_definitions = crate::includes::DefinitionIndex::default();
    let root_index = if is_bookdown {
        &mut shared_definitions
    } else {
        &mut root_definitions
    };
    visit_document(
        db,
        &root_file,
        config,
        &root_path,
        &mut graph,
        &mut visited,
        root_index,
    );
    if let Some(project_root) = roots.quarto_first() {
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
            // vanish from the graph and never get discovered.
            graph.add_document(db, &path);
            // Resolve the sibling to its input and read its content (taking a
            // per-file dependency). Recurse only when it is actually loaded; an
            // interned-but-absent file (`None`) records the dependency so a
            // later load re-runs this query and recurses then.
            let loaded = db
                .file_text(path.clone())
                .filter(|include_file| include_file.text(db).is_some());
            if let Some(include_file) = loaded {
                // Bookdown shares one index across the whole project; each Quarto
                // render target starts from a fresh index so its labels are
                // scoped to itself plus its own includes.
                let mut sibling_definitions = crate::includes::DefinitionIndex::default();
                let sibling_index = if is_bookdown {
                    &mut shared_definitions
                } else {
                    &mut sibling_definitions
                };
                visit_document(
                    db,
                    &include_file,
                    config,
                    &path,
                    &mut graph,
                    &mut visited,
                    sibling_index,
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
        // later load re-runs `project_graph` and recurses then.
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

/// The range-free project edges a single document contributes: the include,
/// metadata-file, and bibliography paths that wire it into the project graph.
///
/// Lifted out of the parse so [`project_structure`] can backdate the same way
/// [`refdef_set`] isolates the parse from unrelated changes. These are paths only
/// — none of the byte ranges that include/duplicate diagnostics carry. A
/// paragraph-body edit shifts those ranges but leaves the path set unchanged, so
/// salsa value-equality on `ProjectEdges` lets the structural graph short-circuit
/// while [`project_graph`] (the diagnostics source, which *does* need current
/// ranges) re-runs as before.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectEdges {
    pub includes: Vec<PathBuf>,
    pub metadata_files: Vec<PathBuf>,
    pub bibliographies: Vec<PathBuf>,
    /// Project-manifest configs (`_quarto.yml`/`_metadata.yml`/`_bookdown.yml`/
    /// `_output.yml`) resolved by directory walk. Path-only (existence-gated),
    /// so the firewall holds on content edits.
    pub project_configs: Vec<PathBuf>,
}

#[salsa::tracked(returns(ref), lru = 512)]
pub fn project_edges(db: &dyn Db, file: FileText, config: FileConfig) -> ProjectEdges {
    // `collect_includes` probes the filesystem directly:
    // an include edge only forms when the target exists on disk), so depend on
    // the interned `FileSet` the way `project_graph` does --- interning a
    // newly-created include (a watcher event) re-runs this query and re-resolves
    // the probe. A content edit leaves the set unchanged, so the firewall holds.
    let _ = db.file_set().ids(db);
    // A pathless in-memory buffer has no project root and no resolvable
    // includes, so it contributes no edges (mirrors `project_graph`).
    let Some(path) = db.path_of(file) else {
        return ProjectEdges::default();
    };
    let text = file.content_or_empty(db);
    let tree = parsed_tree_root(db, file, config);
    let base_dir = path.parent().unwrap_or_else(|| Path::new("."));
    let project_root = crate::includes::find_project_roots(&path).quarto_first();
    let resolution = crate::includes::collect_includes(
        &tree,
        text,
        base_dir,
        project_root.as_deref(),
        config.config(db),
    );
    let includes = resolution
        .includes
        .into_iter()
        .map(|occ| occ.path)
        .collect();
    let (metadata_files, bibliographies) =
        match crate::metadata::extract_project_metadata(&tree, &path) {
            Ok(metadata) => {
                let bibliographies = metadata
                    .bibliography
                    .map(|bibliography| bibliography.paths)
                    .unwrap_or_default();
                (metadata.metadata_files, bibliographies)
            }
            Err(_) => (Vec::new(), Vec::new()),
        };
    let project_configs = crate::metadata::project::project_config_paths(&path);
    ProjectEdges {
        includes,
        metadata_files,
        bibliographies,
        project_configs,
    }
}

/// Whether a file's input is loaded (`Some`) versus interned-but-absent (`None`).
///
/// A thin `Eq`-returning firewall over the raw text input: reading
/// `file.text(db)` directly takes a dependency on the *content value*, so an
/// edit (`Some("a")` -> `Some("b")`) would re-run every reader. Returning the
/// `bool` presence flag backdates instead — only an actual load/unload
/// (`None` <-> `Some`) flips it — which is exactly what [`project_structure`]
/// needs to decide whether to recurse into a referenced file.
#[salsa::tracked]
pub fn file_is_present(db: &dyn Db, file: FileText) -> bool {
    file.text(db).is_some()
}

/// The structural project graph (documents + include/metadata/bibliography
/// edges), with no diagnostics.
///
/// This is the backdating sibling of [`project_graph`]: it walks the project the
/// same way, but reads each member's range-free [`project_edges`] and
/// [`file_is_present`] instead of the member's full parse, so a paragraph-body
/// edit in any member reuses this memo. Every structural
/// consumer — the writer's load fixpoint, `definition_index_with_includes`, and
/// the navigation/workspace-symbol handlers — reads this query. `project_graph`
/// remains the source of the `GraphDiagnostic` accumulator (include + cross-doc
/// duplicate diagnostics) because those carry byte ranges that must track edits.
#[salsa::tracked(returns(ref), lru = 512)]
pub fn project_structure(db: &dyn Db, root_file: FileText, config: FileConfig) -> ProjectGraph {
    // Depend on the set of interned files so interning a newly-referenced
    // include or sibling re-runs this query, mirroring `project_graph`.
    let _ = db.file_set().ids(db);
    let mut graph = InternedProjectGraph::default();
    let Some(root_path) = db.path_of(root_file) else {
        return graph.into_owned(db);
    };
    let mut visited = HashSet::new();
    visit_structure(db, root_file, config, &root_path, &mut graph, &mut visited);
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
            // Record the project document even when unloaded so the writer's
            // fixpoint can see it, load it, and re-run (mirrors `project_graph`).
            graph.add_document(db, &path);
            let loaded = db
                .file_text(path.clone())
                .filter(|include_file| *file_is_present(db, *include_file));
            if let Some(include_file) = loaded {
                visit_structure(db, include_file, config, &path, &mut graph, &mut visited);
            }
        }
    }
    graph.into_owned(db)
}

fn visit_structure<'db>(
    db: &'db dyn Db,
    file: FileText,
    config: FileConfig,
    path: &Path,
    graph: &mut InternedProjectGraph<'db>,
    visited: &mut HashSet<PathBuf>,
) {
    if !visited.insert(path.to_path_buf()) {
        return;
    }
    graph.add_document(db, path);
    let edges = project_edges(db, file, config);
    for include in &edges.includes {
        db.unwind_if_revision_cancelled();
        graph.add_edge(db, path, include, EdgeKind::Include);
        if include == path {
            continue;
        }
        let loaded = db
            .file_text(include.clone())
            .filter(|include_file| *file_is_present(db, *include_file));
        if let Some(include_file) = loaded {
            visit_structure(db, include_file, config, include, graph, visited);
        }
    }
    for metadata_file in &edges.metadata_files {
        graph.add_edge(db, path, metadata_file, EdgeKind::MetadataFile);
    }
    for bibliography in &edges.bibliographies {
        graph.add_edge(db, path, bibliography, EdgeKind::Bibliography);
    }
    for project_config in &edges.project_configs {
        graph.add_edge(db, path, project_config, EdgeKind::ProjectConfig);
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
    /// into analysis and the `<memory>` sentinel is unnecessary.
    fn path_of(&self, file: FileText) -> Option<PathBuf>;

    /// The immutable backing path for a [`FileId`], or `None` for an in-memory
    /// buffer / evicted id. The [`FileId`]-keyed counterpart of [`Db::path_of`],
    /// so the LSP can key a document on its stable id and still recover the path
    /// on demand (from a worker's `&dyn Db` snapshot).
    fn path_of_id(&self, id: FileId) -> Option<PathBuf>;

    /// The shared [`FileSet`] input. `project_graph` reads it to depend on the
    /// set of interned files; the writer adds ids as it discovers references.
    fn file_set(&self) -> FileSet;

    /// The incremental-reparse side channel's view of `(file, config)`.
    ///
    /// The default refuses, which is what makes every throwaway or test
    /// database (and every `Db` impl that never opts in) full-parse with no
    /// side-channel activity at all. Only [`SalsaDb`] overrides it, and only
    /// for documents the LSP has admitted.
    ///
    /// This does not make [`parsed_document`] impure: a successful reparse is
    /// byte-identical to a full parse, so the channel changes computation time
    /// and nothing else. See [`crate::incremental`].
    fn reparse_base(&self, _file: FileText, _config: FileConfig) -> ReparseAdmission {
        ReparseAdmission::Refused
    }

    /// Record `prev` as the reparse base for `(file, config)`. No-op by
    /// default, and no-op in [`SalsaDb`] for a pair that was never admitted.
    fn reparse_store(&self, _file: FileText, _config: FileConfig, _prev: PrevParse) {}
}

#[salsa::db]
#[derive(Clone)]
pub struct SalsaDb {
    storage: salsa::Storage<Self>,
    /// The single owner of file identity: the path<->id<->input interner and the
    /// structural [`FileSet`] input handle (see [`crate::vfs::Vfs`]).
    vfs: Vfs,
    /// The incremental-reparse side channel, shared across cloned handles the
    /// same way [`Vfs`] is, so a worker's snapshot splices against the same
    /// bases the writer records. See [`crate::incremental`].
    reparse: Arc<Mutex<ReparseCache>>,
}

impl Default for SalsaDb {
    fn default() -> Self {
        let db = Self {
            storage: salsa::Storage::default(),
            vfs: Vfs::default(),
            reparse: Arc::default(),
        };
        // Mint the `FileSet` input now (on the constructing thread) so the VFS's
        // `OnceLock` is populated before any cloned worker handle reads it.
        db.file_set();
        db
    }
}

impl SalsaDb {
    /// The reparse side channel, recovering from a poisoned lock rather than
    /// propagating the panic: losing the cache costs a full parse, never a
    /// wrong answer.
    ///
    /// Callers must never hold this guard across a salsa call. The lock order
    /// is strictly salsa -> side channel, so a `salsa::Cancelled` unwind can
    /// never pass through a held lock and no deadlock is reachable.
    fn reparse_state(&self) -> MutexGuard<'_, ReparseCache> {
        self.reparse.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Admit `(file, config)` to the reparse side channel, so its parses start
    /// keeping a base. Writer-only, and the LSP's only gate on incremental
    /// parsing: nothing else in the process admits anything.
    pub fn reparse_admit(&mut self, file: FileText, config: FileConfig) {
        self.reparse_state().admit((file, config));
    }

    /// Drop every base for `file`. Writer-only; called when a document closes.
    pub fn reparse_retire_file(&mut self, file: FileText) {
        self.reparse_state().retire_file(file);
    }

    /// Empty the side channel. Writer-only; called when incremental parsing is
    /// switched off at runtime.
    pub fn reparse_clear(&mut self) {
        self.reparse_state().clear();
    }

    fn reparse_invalidate_file_edits(&mut self, file: FileText) {
        self.reparse_state().invalidate_file_edits(file);
    }

    pub fn file_text_if_cached(&self, path: &Path) -> Option<FileText> {
        self.vfs.input_for_path(path)
    }

    /// The stable [`FileId`] backing a [`FileText`] input, if it is registered.
    /// The LSP keys an open document on this id instead of duplicating its path.
    pub fn file_id_for_input(&self, input: FileText) -> Option<FileId> {
        self.vfs.id_for_input(input)
    }

    /// Register a brand-new file: allocate a [`FileId`], store its input, and
    /// add the id to the [`FileSet`] so `project_graph` (which depends on the
    /// set) re-runs and can resolve the new path. Writer-only.
    fn register_new(&mut self, path: Option<PathBuf>, input: FileText) -> FileId {
        let id = self.vfs.register(path, input);
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
    /// needs no shared sentinel. Writer-only.
    pub fn create_in_memory_file(
        &mut self,
        text: impl Into<Arc<str>>,
        durability: Durability,
    ) -> FileText {
        let id = self.intern_file(None);
        let input = self
            .vfs
            .input_for_id(id)
            .expect("input exists for a just-interned id");
        input
            .set_text(self)
            .with_durability(durability)
            .to(Some(text.into()));
        input
    }

    /// Set `file`'s text, skipping the write when it would store what is already
    /// there. Returns whether the input actually moved.
    ///
    /// Salsa's setter stamps the current revision unconditionally --- it never
    /// compares --- so without this an identical write invalidates every
    /// dependent query, and `parsed_document` is `no_eq`, so nothing downstream
    /// can backdate its way out. Identical writes are not hypothetical: a
    /// watcher event re-reads a file whose bytes did not change on every atomic
    /// save, every duplicate event, and every save of an already-synced file.
    ///
    /// A shared allocation proves equality without reading a byte, which is what
    /// makes this cheap on the paths that hand over the `Arc` they were given.
    ///
    /// Note that skipping the write also skips the durability update. That is
    /// deliberate: the durability of unchanged content is not a reason to
    /// invalidate the world.
    fn set_text_if_changed(
        &mut self,
        file: FileText,
        text: Arc<str>,
        durability: Durability,
    ) -> bool {
        let unchanged = match file.text(self) {
            Some(current) => Arc::ptr_eq(current, &text) || **current == *text,
            None => false,
        };
        if unchanged {
            return false;
        }
        file.set_text(self)
            .with_durability(durability)
            .to(Some(text));
        self.reparse_invalidate_file_edits(file);
        true
    }

    /// Update an admitted LSP input and retain the exact edit for reparsing.
    pub fn update_input_text_with_reparse_edit(
        &mut self,
        file: FileText,
        config: FileConfig,
        text: Arc<str>,
        durability: Durability,
        span: crate::incremental::EditSpan,
    ) -> bool {
        let source = file.text(self).clone().unwrap_or_else(|| Arc::from(""));
        if !self.set_text_if_changed(file, Arc::clone(&text), durability) {
            return false;
        }
        self.reparse_state()
            .stage((file, config), source, text, span);
        true
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

    /// Set the text of the input for `path`, registering it if it is new.
    ///
    /// Takes anything that becomes an `Arc<str>`, so a caller that already holds
    /// one (the LSP write phase does) hands it over with a refcount bump instead
    /// of a copy, while a caller holding a `String` pays the same conversion it
    /// always did.
    pub fn update_file_text(&mut self, path: PathBuf, text: impl Into<Arc<str>>) -> FileText {
        self.update_file_text_with_durability(path, text, Durability::LOW)
    }

    pub fn update_file_text_with_durability(
        &mut self,
        path: PathBuf,
        text: impl Into<Arc<str>>,
        durability: Durability,
    ) -> FileText {
        let text = text.into();
        if let Some(file) = self.vfs.input_for_path(&path) {
            self.set_text_if_changed(file, text, durability);
            return file;
        }
        // The builder carries the durability, so a new file is one write rather
        // than a create followed by a set (which also forced a second `Arc`).
        let file = FileText::builder(Some(text))
            .text_durability(durability)
            .new(self);
        self.register_new(Some(path), file);
        file
    }

    /// Set the text of an already-registered input directly, for a document
    /// with no backing path (an untitled buffer). Returns whether the input
    /// actually moved. Writer-only.
    pub fn update_input_text(
        &mut self,
        file: FileText,
        text: impl Into<Arc<str>>,
        durability: Durability,
    ) -> bool {
        self.set_text_if_changed(file, text.into(), durability)
    }

    pub fn update_file_text_if_cached(&mut self, path: &Path, text: impl Into<Arc<str>>) -> bool {
        self.update_file_text_if_cached_with_durability(path, text, Durability::LOW)
    }

    /// Returns whether `path` was cached at all --- not whether the text moved.
    /// Callers use this to decide "was this file one we track", and a watcher
    /// event for a file whose bytes are unchanged still answers yes.
    pub fn update_file_text_if_cached_with_durability(
        &mut self,
        path: &Path,
        text: impl Into<Arc<str>>,
        durability: Durability,
    ) -> bool {
        let Some(file) = self.vfs.input_for_path(path) else {
            return false;
        };
        self.set_text_if_changed(file, text.into(), durability);
        true
    }

    /// Re-read `path` from disk and refresh its cached text input, but only if
    /// the file is already cached and its on-disk content differs from the
    /// cached value. Returns `true` when the input was actually updated.
    ///
    /// This is the self-heal path for clients that don't deliver
    /// `didChangeWatchedFiles` for every referenced-file change. Neovim, for
    /// example, emits no watch event for a bibliography that is open in a buffer
    /// (it routes `didChange`/`didSave` only to LSPs that own that file type),
    /// so without this an out-of-band edit stays frozen in salsa until the
    /// document is reloaded. Called from the settle write-phase over each open
    /// document's referenced set so the change is picked up on the next document
    /// activity.
    ///
    /// Skips uncached paths (a brand-new file is loaded by
    /// [`SalsaDb::load_file_from_disk`], not here), unreadable paths (a missing
    /// file keeps its last-known content rather than being wiped), and unchanged
    /// content (compare-then-skip, so an unchanged file causes no revision bump
    /// and no downstream invalidation). Writer-only.
    pub fn resync_cached_file_from_disk(&mut self, path: &Path, durability: Durability) -> bool {
        let Some(file) = self.vfs.input_for_path(path) else {
            return false;
        };
        let Ok(contents) = std::fs::read_to_string(path) else {
            return false;
        };
        self.set_text_if_changed(file, Arc::from(contents), durability)
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
        let file = FileText::builder(Some(Arc::from(contents)))
            .text_durability(durability)
            .new(self);
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
    /// `Db::file_text` is a pure lookup, so a query only sees files
    /// already loaded. Each pass runs `project_graph` (which records the root,
    /// its included/sibling documents, and bibliography/metadata edges even when
    /// a file is unloaded), then for every referenced path **interns** it (which
    /// mints a `None` input and adds its id to the [`FileSet`] on first
    /// reference --- re-running `project_graph`) and **loads** it from disk. A
    /// fresh `None`->`Some` load flips that file's per-file dependency, again
    /// re-running `project_graph` so it recurses into the freshly-loaded file's
    /// own references. Both convergence channels live inside salsa's dependency
    /// graph, without a global generation counter.
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
                let graph = project_structure(self, root_file, config);
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
    // and preserves the single-writer invariant.
    fn file_text(&self, path: PathBuf) -> Option<FileText> {
        self.file_text_if_cached(&path)
    }

    fn path_of(&self, file: FileText) -> Option<PathBuf> {
        self.vfs.path_for_input(file)
    }

    fn path_of_id(&self, id: FileId) -> Option<PathBuf> {
        self.vfs.path_for_id(id)
    }

    fn file_set(&self) -> FileSet {
        // Owned by the VFS: created once on the writer (in `Default`); cloned
        // worker handles share the same `OnceLock` and only ever read it back.
        self.vfs.file_set(self)
    }

    fn reparse_base(&self, file: FileText, config: FileConfig) -> ReparseAdmission {
        self.reparse_state().base((file, config))
    }

    fn reparse_store(&self, file: FileText, config: FileConfig, prev: PrevParse) {
        self.reparse_state().store((file, config), prev);
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
        *volatile.value(db)
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

    /// `<div id="a&amp;b">` declares the HTML id `a&b`, so that -- not the
    /// encoded source spelling -- is what lands in the anchor index. Matches
    /// pandoc's lifted `Div` attr.
    #[test]
    fn symbol_usage_index_decodes_entities_in_html_div_anchor() {
        let db = SalsaDb::default();
        let tree = crate::parse(
            "<div id=\"a&amp;b\">\n\ntext\n\n</div>\n",
            Some(crate::Config {
                flavor: crate::config::Flavor::Pandoc,
                ..Default::default()
            }),
        );
        let index = symbol_usage_index_from_tree(&db, &tree, &crate::config::Extensions::default());

        assert_eq!(
            index
                .crossref_declarations("a&b")
                .map(|ranges| ranges.len()),
            Some(1),
            "decoded id should be declared"
        );
        assert!(
            index.crossref_declarations("a&amp;b").is_none(),
            "encoded source spelling should not be declared"
        );
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
    fn symbol_usage_index_block_memoizes_unchanged_blocks() {
        // The perf win: editing one top-level block re-walks only that block.
        // Every other block's `symbol_usage_index_block` memo is reused because
        // its `GreenNode` is structurally equal across the reparse.
        let (mut db, log) = db_with_exec_log();
        let path = PathBuf::from("/tmp/block_memo.qmd");
        let source = "# Alpha\n\n# Beta\n\n# Gamma\n\n# Delta\n\n# Epsilon\n\n# Zeta\n".to_string();
        let file = db.update_file_text(path.clone(), source);
        let config = FileConfig::new(&db, crate::Config::default());

        // Prime the per-block memos, then clear the log.
        let before = symbol_usage_index(&db, file, config).clone();
        let primed_blocks = executed(&log, "symbol_usage_index_block");
        assert!(
            primed_blocks >= 5,
            "priming should walk every block, walked {primed_blocks}"
        );
        log.lock().unwrap().clear();

        // Edit exactly one heading block (changing its length shifts later
        // blocks' absolute offsets, which must not force them to re-walk).
        db.update_file_text(
            path,
            "# Alpha\n\n# Beta\n\n# GammaX\n\n# Delta\n\n# Epsilon\n\n# Zeta\n".to_string(),
        );
        let after = symbol_usage_index(&db, file, config).clone();

        assert_eq!(
            executed(&log, "symbol_usage_index_block"),
            1,
            "only the edited block should be re-walked; every other block memo-hits"
        );
        // Sanity: the merged index still equals a full whole-tree walk.
        let tree = parsed_tree_root(&db, file, config);
        let reference =
            symbol_usage_index_from_tree(&db, &tree, &crate::config::Extensions::default());
        assert_eq!(after, reference);
        assert_ne!(before, after, "the edit should change the index");
    }

    #[test]
    fn built_in_lint_plan_reuses_block_symbol_memos_after_edit() {
        // End to end: the lint pass now consumes the per-block-memoized symbol
        // index, so a single-block edit re-walks only the edited block's symbols
        // instead of rebuilding the whole index for every symbol-backed rule.
        let (mut db, log) = db_with_exec_log();
        let path = PathBuf::from("/tmp/lint_block_memo.qmd");
        let source = "# Alpha\n\n# Beta\n\n# Gamma\n\n# Delta\n\n# Epsilon\n\n# Zeta\n".to_string();
        let file = db.update_file_text(path.clone(), source);
        let config = FileConfig::new(&db, crate::Config::default());

        // Prime the lint plan (builds every block's symbol memo once).
        let _ = built_in_lint_plan(&db, file, config);
        assert!(executed(&log, "symbol_usage_index_block") >= 5);
        log.lock().unwrap().clear();

        db.update_file_text(
            path,
            "# Alpha\n\n# Beta\n\n# GammaX\n\n# Delta\n\n# Epsilon\n\n# Zeta\n".to_string(),
        );
        let _ = built_in_lint_plan(&db, file, config);

        assert_eq!(
            executed(&log, "symbol_usage_index_block"),
            1,
            "relint should re-walk only the edited block's symbols"
        );
    }

    #[test]
    fn heading_outline_block_memoizes_unchanged_blocks() {
        let (mut db, log) = db_with_exec_log();
        let path = PathBuf::from("/tmp/outline_memo.qmd");
        let source = "# Alpha\n\n# Beta\n\n# Gamma\n\n# Delta\n\n# Epsilon\n\n# Zeta\n".to_string();
        let file = db.update_file_text(path.clone(), source);
        let config = FileConfig::new(&db, crate::Config::default());

        let before = heading_outline(&db, file, config).clone();
        assert!(executed(&log, "heading_outline_block") >= 5);
        log.lock().unwrap().clear();

        db.update_file_text(
            path,
            "# Alpha\n\n# Beta\n\n# GammaX\n\n# Delta\n\n# Epsilon\n\n# Zeta\n".to_string(),
        );
        let after = heading_outline(&db, file, config).clone();

        assert_eq!(
            executed(&log, "heading_outline_block"),
            1,
            "only the edited heading block should be re-walked"
        );
        assert_ne!(before, after, "the edit should change the outline");
        // Later blocks' absolute ranges must still shift correctly.
        assert_eq!(after.len(), 6);
        assert_eq!(after[2].title, "GammaX");
    }

    #[test]
    fn folded_index_matches_reference() {
        // The decomposed, per-block `symbol_usage_index` must be byte-identical
        // to the whole-tree `symbol_usage_index_from_tree` reference across a
        // corpus. The corpus deliberately spans every cross-block hazard:
        // duplicate heading slugs (implicit-id dedup runs document-wide), and
        // symbols spread across multiple top-level sections so the offset merge
        // is exercised.
        let corpus = [
            "# Intro\n\nSee @fig-plot and [@cite] and [ref].\n\n# Intro\n\nDuplicate slug.\n\n[ref]: https://example.com\n[^a]: footnote\n\n```{r}\n#| label: fig-plot\n1 + 1\n```\n",
            "# A\n\nalpha [text](#b)\n\n## B {#b}\n\nbody\n\n# A\n\nsecond A section\n\n# A\n\nthird A section\n",
            "para one [^n]\n\npara two\n\n[^n]: note\n\n[link][r]\n\n[r]: https://example.org\n",
            "---\ntitle: Demo\n---\n\n# H1\n\ntext\n\n# H1\n\nmore\n\n# H1\n\neven more\n",
            "no headings here\n\njust paragraphs\n\nand [ref] uses\n\n[ref]: https://x.test\n",
        ];

        for (flavor, exts) in [
            (crate::config::Flavor::Quarto, {
                let mut e = crate::config::Extensions::for_flavor(crate::config::Flavor::Quarto);
                e.quarto_crossrefs = true;
                e
            }),
            (
                crate::config::Flavor::Pandoc,
                crate::config::Extensions::for_flavor(crate::config::Flavor::Pandoc),
            ),
        ] {
            for (i, source) in corpus.iter().enumerate() {
                let mut db = SalsaDb::default();
                let path = PathBuf::from(format!("/tmp/folded_{i}.qmd"));
                let file = db.update_file_text(path, source.to_string());
                let cfg = crate::Config {
                    flavor,
                    extensions: exts.clone(),
                    ..Default::default()
                };
                let config = FileConfig::new(&db, cfg);

                let decomposed = symbol_usage_index(&db, file, config).clone();
                let tree = parsed_tree_root(&db, file, config);
                let reference = symbol_usage_index_from_tree(&db, &tree, &exts);

                assert_eq!(
                    decomposed, reference,
                    "decomposed index diverged from whole-tree reference for corpus case {i} ({flavor:?})"
                );
            }
        }
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
    fn bibliography_load_error_range_updates_after_path_edit() {
        let temp_dir = tempfile::TempDir::new().expect("temp dir");
        let root = temp_dir.path();
        let doc_path = root.join("doc.qmd");
        // references.bib intentionally missing on disk -> always a load error.

        let mut db = SalsaDb::default();
        let cfg = crate::Config {
            flavor: crate::config::Flavor::Quarto,
            extensions: crate::config::Extensions::for_flavor(crate::config::Flavor::Quarto),
            ..Default::default()
        };
        let config = FileConfig::new(&db, cfg);

        // Mirror the LSP write phase (`reload_open_documents_referenced_files`)
        // followed by the read phase (`built_in_lint_plan`), returning the text
        // covered by the bibliography-load-error span.
        fn load_error_span(db: &SalsaDb, file: FileText, config: FileConfig, text: &str) -> String {
            let plan = built_in_lint_plan(db, file, config).clone();
            let diag = plan
                .diagnostics
                .iter()
                .find(|d| d.code == "bibliography-load-error")
                .expect("bibliography-load-error diagnostic");
            let start: usize = diag.location.range.start().into();
            let end: usize = diag.location.range.end().into();
            text[start..end].to_string()
        }

        let with_r = "---\nbibliography: references.bib\n---\n\nSee [@known].\n";
        let without_r = "---\nbibliography: eferences.bib\n---\n\nSee [@known].\n";

        // Initial open.
        let file = db.update_file_text(doc_path.clone(), with_r.to_string());
        db.load_referenced_files(file, config, doc_path.clone());
        assert_eq!(
            load_error_span(&db, file, config, with_r),
            "references.bib",
            "initial span must cover the full value"
        );

        // Edit: delete the leading `r` -> `eferences.bib`.
        let file = db.update_file_text(doc_path.clone(), without_r.to_string());
        db.load_referenced_files(file, config, doc_path.clone());
        assert_eq!(
            load_error_span(&db, file, config, without_r),
            "eferences.bib",
            "span must follow the edited (shorter) value"
        );

        // Restore: bring the `r` back -> `references.bib`.
        let file = db.update_file_text(doc_path.clone(), with_r.to_string());
        db.load_referenced_files(file, config, doc_path.clone());
        assert_eq!(
            load_error_span(&db, file, config, with_r),
            "references.bib",
            "span must update back to the full value after restoring the path"
        );
    }

    #[test]
    fn project_manifest_diagnostics_reports_and_clears_broken_quarto_yml() {
        let temp_dir = tempfile::TempDir::new().expect("temp dir");
        let root = temp_dir.path();
        let doc_path = root.join("doc.qmd");
        let quarto_path = root.join("_quarto.yml");
        // Must exist on disk so `find_project_root` resolves the ProjectConfig edge.
        std::fs::write(&quarto_path, "title: [\n").expect("project config");

        let mut db = SalsaDb::default();
        let cfg = crate::Config {
            flavor: crate::config::Flavor::Quarto,
            extensions: crate::config::Extensions::for_flavor(crate::config::Flavor::Quarto),
            ..Default::default()
        };
        let config = FileConfig::new(&db, cfg);

        // Load the manifest as a tracked input (mirrors `load_referenced_files`).
        let _quarto_file = db.update_file_text(quarto_path.clone(), "title: [\n".to_string());
        let file = db.update_file_text(doc_path.clone(), "# Doc\n".to_string());

        let diags = project_manifest_diagnostics(&db, file, config).clone();
        assert_eq!(diags.len(), 1, "expected one manifest diagnostic");
        assert_eq!(
            diags[0].0, quarto_path,
            "diagnostic attributed to _quarto.yml"
        );
        assert!(matches!(
            diags[0].1,
            crate::metadata::YamlError::ParseError { .. }
        ));

        // Fixing the manifest input re-runs the query and clears the diagnostic.
        let _ = db.update_file_text(quarto_path.clone(), "title: ok\n".to_string());
        let diags = project_manifest_diagnostics(&db, file, config).clone();
        assert!(
            diags.is_empty(),
            "valid manifest should produce no diagnostics, got {diags:?}"
        );
    }

    #[test]
    fn project_manifest_schema_diagnostics_flags_quarto_yml_typo() {
        let temp_dir = tempfile::TempDir::new().expect("temp dir");
        let root = temp_dir.path();
        let doc_path = root.join("doc.qmd");
        let quarto_path = root.join("_quarto.yml");
        // Valid YAML but a frontmatter-key typo the schema can decide.
        std::fs::write(&quarto_path, "forrmat: html\n").expect("project config");

        let mut db = SalsaDb::default();
        let mut cfg = crate::Config {
            flavor: crate::config::Flavor::Quarto,
            extensions: crate::config::Extensions::for_flavor(crate::config::Flavor::Quarto),
            ..Default::default()
        };
        // unknown-key is opt-in; enable it so the manifest path surfaces the typo.
        cfg.lint
            .rules
            .insert("quarto-schema-unknown-key".to_string(), true);
        let config = FileConfig::new(&db, cfg);

        let _quarto_file = db.update_file_text(quarto_path.clone(), "forrmat: html\n".to_string());
        let file = db.update_file_text(doc_path.clone(), "# Doc\n".to_string());

        let diags = project_manifest_schema_diagnostics(&db, file, config).clone();
        assert_eq!(diags.len(), 1, "expected one manifest with schema diags");
        assert_eq!(diags[0].0, quarto_path);
        assert!(
            diags[0]
                .1
                .iter()
                .any(|d| d.code == "quarto-schema-unknown-key"),
            "expected unknown-key diagnostic, got {:?}",
            diags[0].1
        );

        // Fixing the manifest clears the diagnostic.
        let _ = db.update_file_text(quarto_path.clone(), "format: html\n".to_string());
        let diags = project_manifest_schema_diagnostics(&db, file, config).clone();
        assert!(
            diags.is_empty(),
            "valid manifest should be clean, got {diags:?}"
        );
    }

    #[test]
    fn project_manifest_schema_diagnostics_off_under_pandoc() {
        let temp_dir = tempfile::TempDir::new().expect("temp dir");
        let root = temp_dir.path();
        let doc_path = root.join("doc.qmd");
        let quarto_path = root.join("_quarto.yml");
        std::fs::write(&quarto_path, "forrmat: html\n").expect("project config");

        let mut db = SalsaDb::default();
        // Non-Quarto flavor: the rule (and this query) must stay silent.
        let cfg = crate::Config {
            flavor: crate::config::Flavor::Pandoc,
            ..Default::default()
        };
        let config = FileConfig::new(&db, cfg);

        let _quarto_file = db.update_file_text(quarto_path.clone(), "forrmat: html\n".to_string());
        let file = db.update_file_text(doc_path.clone(), "# Doc\n".to_string());

        let diags = project_manifest_schema_diagnostics(&db, file, config).clone();
        assert!(
            diags.is_empty(),
            "schema query must be Quarto-only, got {diags:?}"
        );
    }

    #[test]
    fn built_in_lint_plan_does_not_misattribute_manifest_error_to_document() {
        let temp_dir = tempfile::TempDir::new().expect("temp dir");
        let root = temp_dir.path();
        let doc_path = root.join("doc.qmd");
        let quarto_path = root.join("_quarto.yml");
        std::fs::write(&quarto_path, "title: [\n").expect("project config");

        let mut db = SalsaDb::default();
        let cfg = crate::Config {
            flavor: crate::config::Flavor::Quarto,
            extensions: crate::config::Extensions::for_flavor(crate::config::Flavor::Quarto),
            ..Default::default()
        };
        let config = FileConfig::new(&db, cfg);

        let _quarto_file = db.update_file_text(quarto_path.clone(), "title: [\n".to_string());
        // Document has perfectly valid frontmatter; only the manifest is broken.
        let file = db.update_file_text(doc_path.clone(), "---\ntitle: Doc\n---\n".to_string());

        let plan = built_in_lint_plan(&db, file, config).clone();
        assert!(
            plan.diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code != "yaml-parse-error"),
            "broken _quarto.yml must NOT surface a yaml-parse-error on the document"
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
    fn frontmatter_tab_is_consumer_aware_and_not_double_reported() {
        // A tab indenting a mapping value: pandoc/libyaml accepts it (its
        // markdown reader expands tabs), js-yaml/R yaml reject it. The lint must
        // mirror the active consumer — and never double-report (the parser
        // channel + the metadata-extraction gate must agree).
        let doc = "---\nfoo:\n\tbar: 1\n---\n\n# Hi\n";

        let count_yaml_errors = |flavor: crate::config::Flavor| {
            let mut db = SalsaDb::default();
            let cfg = crate::Config {
                flavor,
                extensions: crate::config::Extensions::for_flavor(flavor),
                ..Default::default()
            };
            let config = FileConfig::new(&db, cfg);
            let path = PathBuf::from("/tmp/frontmatter_tab.md");
            let file = db.update_file_text(path, doc.to_string());
            built_in_lint_plan(&db, file, config)
                .diagnostics
                .iter()
                .filter(|d| d.code == "yaml-parse-error")
                .count()
        };

        assert_eq!(
            count_yaml_errors(crate::config::Flavor::Pandoc),
            0,
            "pandoc accepts a tab as indentation; lint must not flag it",
        );
        assert_eq!(
            count_yaml_errors(crate::config::Flavor::Quarto),
            1,
            "js-yaml rejects the tab; lint must flag it exactly once (no double-report)",
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

        let baseline = *stable_file_len(&db, stable_file);
        let baseline_runs = STABLE_QUERY_RUNS.load(Ordering::Relaxed);
        assert!(baseline_runs >= 1);

        for i in 1..=20 {
            db.update_file_text(noisy_path.clone(), format!("noisy-{i}"));
            volatile.set_value(&mut db).to(i);
            assert_eq!(*volatile_probe(&db, volatile), i);
            assert_eq!(*stable_file_len(&db, stable_file), baseline);
        }

        assert_eq!(
            STABLE_QUERY_RUNS.load(Ordering::Relaxed),
            baseline_runs,
            "HIGH durability inputs should not be revalidated on LOW updates"
        );

        let _ = std::fs::remove_file(stable_path);
    }

    /// Interning an unrelated sibling
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

    type ExecLog = Arc<Mutex<Vec<String>>>;

    /// A `SalsaDb` that records the `database_key` of every tracked query salsa
    /// *executes* (as opposed to validating from memo). Lets tests assert that a
    /// paragraph-body edit in one project member reuses other files' memos
    /// using the same event-callback hook Salsa's own test
    /// suite uses.
    fn db_with_exec_log() -> (SalsaDb, ExecLog) {
        let log: ExecLog = Arc::new(Mutex::new(Vec::new()));
        let sink = log.clone();
        let storage = salsa::Storage::new(Some(Box::new(move |event: salsa::Event| {
            if let salsa::EventKind::WillExecute { database_key } = event.kind {
                sink.lock().unwrap().push(format!("{database_key:?}"));
            }
        })));
        let db = SalsaDb {
            storage,
            vfs: Vfs::default(),
            reparse: Arc::default(),
        };
        db.file_set();
        (db, log)
    }

    /// How many times a tracked query named `query` executed in the log. The
    /// recorded key renders as `query(Id(..))`, so match on the `query(` prefix.
    fn executed(log: &ExecLog, query: &str) -> usize {
        let needle = format!("{query}(");
        log.lock()
            .unwrap()
            .iter()
            .filter(|key| key.starts_with(&needle))
            .count()
    }

    #[test]
    fn definition_index_does_not_intern_labels() {
        let db = SalsaDb::default();
        let file = FileText::from_str(&db, "[Transient Label]: /target\n");
        let config = FileConfig::new(&db, Config::default());

        let index = definition_index(&db, file, config);
        assert!(index.find_reference("transient label").is_some());

        let memory = (&db as &dyn salsa::Database).memory_usage();
        assert!(
            memory
                .structs
                .iter()
                .all(|ingredient| ingredient.debug_name() != "InternedLabel"),
            "definition indexing must not create a database-wide label interner"
        );
    }

    #[test]
    fn successful_reparse_does_not_execute_refdef_set() {
        let (mut db, log) = db_with_exec_log();
        let path = PathBuf::from("/virtual/deferred-refdefs.md");
        let before = "# One\n\nAlpha paragraph.\n\n# Two\n\nBeta paragraph.\n";
        let after = "# One\n\nAlpha paragraph.\n\n# Two\n\nBeta paragraph edited.\n";
        let file = db.update_file_text(path.clone(), before.to_owned());
        let config = FileConfig::new(&db, Config::default());
        db.reparse_admit(file, config);
        parsed_document(&db, file, config);
        log.lock().unwrap().clear();

        db.update_file_text(path, after.to_owned());
        let parsed = parsed_document(&db, file, config);

        assert_eq!(executed(&log, "parsed_document"), 1);
        assert_eq!(
            executed(&log, "refdef_set"),
            0,
            "an accepted reparse must retain the previous refdef set without scanning"
        );
        assert_eq!(
            SyntaxNode::new_root(parsed.green.clone()).to_string(),
            after
        );
    }

    #[test]
    fn refdef_edit_scans_on_reparse_fallback() {
        let (mut db, log) = db_with_exec_log();
        let path = PathBuf::from("/virtual/changed-refdefs.md");
        let before = "A [link][ref].\n\n[ref]: https://example.com\n";
        let after = "A [link][ref].\n\n[other]: https://example.com\n";
        let file = db.update_file_text(path.clone(), before.to_owned());
        let config = FileConfig::new(&db, Config::default());
        db.reparse_admit(file, config);
        parsed_document(&db, file, config);
        log.lock().unwrap().clear();

        db.update_file_text(path, after.to_owned());
        let parsed = parsed_document(&db, file, config);

        assert_eq!(executed(&log, "refdef_set"), 1);
        let fresh = crate::parser::parse(after, Some(Config::default()));
        assert_eq!(
            panache_parser::parser::fingerprint(&SyntaxNode::new_root(parsed.green.clone())),
            panache_parser::parser::fingerprint(&fresh),
        );
    }

    #[test]
    fn unchanged_duplicate_refdef_set_still_scans_after_guard_decline() {
        let (mut db, log) = db_with_exec_log();
        let path = PathBuf::from("/virtual/duplicate-refdefs.md");
        let before = "[ref]: /one\n[ref]: /two\n\nA [link][ref].\n";
        let after = "[ref]: /one-edited\n[ref]: /two\n\nA [link][ref].\n";
        let file = db.update_file_text(path.clone(), before.to_owned());
        let config = FileConfig::new(&db, Config::default());
        db.reparse_admit(file, config);
        parsed_document(&db, file, config);
        log.lock().unwrap().clear();

        db.update_file_text(path, after.to_owned());
        let parsed = parsed_document(&db, file, config);

        assert_eq!(
            executed(&log, "refdef_set"),
            1,
            "a guard decline must scan even when set equality later backdates the map"
        );
        assert_eq!(
            SyntaxNode::new_root(parsed.green.clone()).to_string(),
            after
        );
    }

    /// A two-document Quarto project (`root.qmd` + `child.qmd`, both loaded) on
    /// an execution-logging db. Returns the db, root's `FileText`, config, child
    /// path, the exec log, and the `TempDir` guard (keep it alive for disk reads).
    fn two_doc_project_logging() -> (
        SalsaDb,
        FileText,
        FileConfig,
        PathBuf,
        ExecLog,
        tempfile::TempDir,
    ) {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let root = temp_dir.path();
        std::fs::write(root.join("_quarto.yml"), "project: default\n").unwrap();
        let root_path = root.join("root.qmd");
        let child_path = root.join("child.qmd");
        std::fs::write(&root_path, "# Root\n\nRoot body.\n").unwrap();
        std::fs::write(&child_path, "# Child\n\nChild body paragraph.\n").unwrap();

        let (mut db, log) = db_with_exec_log();
        let root_file =
            db.update_file_text(root_path.clone(), "# Root\n\nRoot body.\n".to_string());
        // Load the sibling so `project_structure` takes a (firewall-able)
        // dependency on its `project_edges` / `file_is_present`.
        db.update_file_text(
            child_path.clone(),
            "# Child\n\nChild body paragraph.\n".to_string(),
        );
        let config = FileConfig::new(&db, Config::default());
        (db, root_file, config, child_path, log, temp_dir)
    }

    /// Writing the bytes a file already holds must execute nothing. Salsa's
    /// setter never compares, so the guard in `set_text_if_changed` is the only
    /// thing standing between a redundant write and a full invalidation cascade
    /// --- and redundant writes are routine: an atomic save, a duplicate watcher
    /// event, a save of an already-synced file.
    ///
    /// Both routes must hold: the same allocation (the pointer fast path) and an
    /// equal one from a fresh `String` (the content compare behind it).
    #[test]
    fn an_identical_write_executes_nothing() {
        const DOC: &str = "# Root\n\nRoot body.\n";
        let (mut db, root_file, config, _child_path, log, _temp_dir) = two_doc_project_logging();

        parsed_document(&db, root_file, config);
        refdef_set(&db, root_file, config);
        let id = db.file_id_for_input(root_file).expect("root is registered");
        let path = Db::path_of_id(&db, id).expect("root has a path");
        log.lock().unwrap().clear();

        let shared = root_file.text(&db).clone().expect("root has text");
        db.update_file_text(path.clone(), shared);
        db.update_file_text(path, DOC.to_string());

        parsed_document(&db, root_file, config);
        refdef_set(&db, root_file, config);

        // `line_index` was a third witness here until it stopped being a tracked
        // query (it is now `crate::lsp::line_index::LineIndexCache`, which salsa
        // does not know about). Its half of this claim is asserted directly, and
        // more sharply, by `an_identical_write_leaves_no_stale_index` in
        // `tests/lsp/test_line_index_reuse.rs`.
        for query in ["parsed_document", "refdef_set"] {
            assert_eq!(
                executed(&log, query),
                0,
                "an identical write must not re-execute {query}"
            );
        }
    }

    /// The guard must not swallow a real write.
    #[test]
    fn a_differing_write_still_invalidates() {
        let (mut db, root_file, config, _child_path, log, _temp_dir) = two_doc_project_logging();

        parsed_document(&db, root_file, config);
        let id = db.file_id_for_input(root_file).expect("root is registered");
        let path = Db::path_of_id(&db, id).expect("root has a path");
        log.lock().unwrap().clear();

        db.update_file_text(path, "# Root\n\nA different body.\n".to_string());
        parsed_document(&db, root_file, config);

        assert_eq!(
            executed(&log, "parsed_document"),
            1,
            "a real change must still invalidate"
        );
    }

    /// A paragraph-body edit in a
    /// project member must NOT re-execute the structural `project_structure`
    /// memo, nor the *other* file's `definition_index` / `heading_outline` /
    /// `metadata` memos. Pre-firewall (`project_structure` read the member's full
    /// parse) the `project_structure` assertion fails.
    #[test]
    fn body_edit_in_member_reuses_structural_and_sibling_memos() {
        let (mut db, root_file, config, child_path, log, _temp_dir) = two_doc_project_logging();

        // Prime every memo we care about for the root document, then clear.
        project_structure(&db, root_file, config);
        definition_index(&db, root_file, config);
        heading_outline(&db, root_file, config);
        metadata(&db, root_file, config);
        log.lock().unwrap().clear();

        // Edit ONLY the child's paragraph body: no heading, no definition, no
        // include/metadata/bibliography edge changes.
        db.update_file_text(child_path, "# Child\n\nA different body.\n".to_string());

        project_structure(&db, root_file, config);
        definition_index(&db, root_file, config);
        heading_outline(&db, root_file, config);
        metadata(&db, root_file, config);

        assert_eq!(
            executed(&log, "project_structure"),
            0,
            "a member body edit must not re-run the structural project graph"
        );
        assert_eq!(
            executed(&log, "definition_index"),
            0,
            "the root's definition_index must be reused across a sibling body edit"
        );
        assert_eq!(
            executed(&log, "heading_outline"),
            0,
            "the root's heading_outline must be reused across a sibling body edit"
        );
        assert_eq!(
            executed(&log, "metadata"),
            0,
            "the root's metadata must be reused across a sibling body edit"
        );
        // The firewall really engaged: the child's edges were re-checked (and
        // backdated) rather than skipped.
        assert!(
            executed(&log, "project_edges") >= 1,
            "the edited child's project_edges should re-run and backdate"
        );
    }

    /// The firewall must not over-suppress: a structural edit (adding a
    /// bibliography edge to the child) changes `project_edges`, so it does NOT
    /// backdate and `project_structure` re-runs.
    #[test]
    fn structural_edit_in_member_reexecutes_project_structure() {
        let (mut db, root_file, config, child_path, log, _temp_dir) = two_doc_project_logging();

        project_structure(&db, root_file, config);
        log.lock().unwrap().clear();

        // Add a bibliography edge to the child via frontmatter.
        db.update_file_text(
            child_path,
            "---\nbibliography: refs.bib\n---\n\n# Child\n\nChild body paragraph.\n".to_string(),
        );

        project_structure(&db, root_file, config);

        assert!(
            executed(&log, "project_structure") >= 1,
            "adding a bibliography edge to a member must re-run the structural graph"
        );
    }

    #[test]
    fn file_text_is_a_pure_lookup_and_never_reads_disk() {
        let db = SalsaDb::default();

        // A real file exists on disk, but it was never loaded through a writer
        // method. `file_text` must NOT read it --- it is a pure cache lookup
        // Loading is the writer's responsibility.
        let path = unique_temp_path("file-text-purity", ".qmd");
        std::fs::write(&path, "on disk but not loaded").expect("write probe file");

        assert!(
            db.file_text(path.clone()).is_none(),
            "file_text must return None for an unloaded path even when the file exists on disk"
        );

        let _ = std::fs::remove_file(path);
    }

    fn quarto_config(db: &SalsaDb) -> FileConfig {
        let cfg = Config {
            flavor: crate::config::Flavor::Quarto,
            extensions: crate::config::Extensions::for_flavor(crate::config::Flavor::Quarto),
            ..Default::default()
        };
        FileConfig::new(db, cfg)
    }

    #[test]
    fn project_symbol_index_aggregates_sibling_definitions() {
        let temp = tempfile::TempDir::new().unwrap();
        let root = temp.path();
        std::fs::write(root.join("_quarto.yml"), "project:\n  type: book\n").unwrap();
        let defs_path = root.join("defs.qmd");
        let body_path = root.join("body.qmd");
        std::fs::write(
            &defs_path,
            "# Intro {#shared}\n\n[ref-def]: https://example.com\n",
        )
        .unwrap();
        std::fs::write(&body_path, "See [ref-def] and [here](#shared).\n").unwrap();

        let mut db = SalsaDb::default();
        let config = quarto_config(&db);
        let body_file = db.update_file_text(
            body_path.clone(),
            std::fs::read_to_string(&body_path).unwrap(),
        );
        db.load_referenced_files(body_file, config, body_path.clone());

        let aggregate =
            project_symbol_index(&db, body_file, config).expect("document is in a project");
        assert!(
            aggregate.definitions.reference_labels.contains("ref-def"),
            "reference definition from a sibling must be aggregated: {:?}",
            aggregate.definitions
        );
        assert!(
            aggregate.anchors.contains("shared"),
            "explicit anchor from a sibling must be aggregated: {:?}",
            aggregate.anchors
        );
    }

    #[test]
    fn project_document_contribution_memoized_across_project_files() {
        // The core O(n^2) fix: aggregating the project symbols for every file in
        // a project must fold each project document exactly once, not once per
        // aggregating file. `project_document_contribution` carries the per-doc
        // parse + index work, so it must execute once per document total.
        let temp = tempfile::TempDir::new().unwrap();
        let root = temp.path();
        std::fs::write(root.join("_quarto.yml"), "project:\n  type: book\n").unwrap();
        let a_path = root.join("a.qmd");
        let b_path = root.join("b.qmd");
        std::fs::write(&a_path, "# A {#anchor-a}\n\n[def-a]: https://a.example\n").unwrap();
        std::fs::write(&b_path, "See [def-a] and [x](#anchor-a).\n").unwrap();

        let (mut db, log) = db_with_exec_log();
        let config = quarto_config(&db);
        let a_file = db.update_file_text(a_path.clone(), std::fs::read_to_string(&a_path).unwrap());
        db.load_referenced_files(a_file, config, a_path.clone());
        let b_file = db
            .file_text(b_path.clone())
            .expect("sibling loaded by load_referenced_files");

        // Aggregate the project symbols for BOTH project files.
        let _ = project_symbol_index(&db, a_file, config);
        let _ = project_symbol_index(&db, b_file, config);

        assert_eq!(
            executed(&log, "project_document_contribution"),
            2,
            "each project document's contribution must be folded once, not per aggregating file"
        );
    }

    /// Regression: on macOS the tempdir prefix (`/var/...`) is a symlink to
    /// `/private/var/...`, so canonicalizing the document path inside
    /// `project_symbol_index` diverged from the raw path the files were loaded
    /// under, and the aggregate looked up siblings by the canonical form and
    /// found nothing. Loading and aggregation must agree on path form: neither
    /// side canonicalizes. Reproduced portably by loading through an explicit
    /// symlink whose canonical target differs from the path used at load time.
    #[cfg(unix)]
    #[test]
    fn project_symbol_index_aggregates_through_symlinked_root() {
        let temp = tempfile::TempDir::new().unwrap();
        let real_root = temp.path().join("real");
        std::fs::create_dir(&real_root).unwrap();
        std::fs::write(real_root.join("_quarto.yml"), "project:\n  type: book\n").unwrap();
        std::fs::write(
            real_root.join("defs.qmd"),
            "# Intro {#shared}\n\n[ref-def]: https://example.com\n",
        )
        .unwrap();
        std::fs::write(
            real_root.join("body.qmd"),
            "See [ref-def] and [here](#shared).\n",
        )
        .unwrap();

        // Load everything through a symlinked prefix, mirroring macOS's
        // `/var` -> `/private/var` indirection. `canonicalize()` would resolve
        // the link away and no longer match the interned paths.
        let link_root = temp.path().join("link");
        std::os::unix::fs::symlink(&real_root, &link_root).unwrap();
        let body_path = link_root.join("body.qmd");

        let mut db = SalsaDb::default();
        let config = quarto_config(&db);
        let body_file = db.update_file_text(
            body_path.clone(),
            std::fs::read_to_string(&body_path).unwrap(),
        );
        db.load_referenced_files(body_file, config, body_path.clone());

        let aggregate =
            project_symbol_index(&db, body_file, config).expect("document is in a project");
        assert!(
            aggregate.definitions.reference_labels.contains("ref-def"),
            "sibling definition must aggregate through a symlinked root: {:?}",
            aggregate.definitions
        );
        assert!(
            aggregate.anchors.contains("shared"),
            "sibling anchor must aggregate through a symlinked root: {:?}",
            aggregate.anchors
        );
    }
}

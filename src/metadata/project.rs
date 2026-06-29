use std::path::{Path, PathBuf};

use rowan::TextRange;
use std::collections::HashMap;

use super::bibliography::{BibliographyInfo, BibliographyParse};
use super::yaml::{YamlError, strip_yaml_delimiters};
use super::{DocumentMetadata, InlineReference, extract_citations};
use crate::bib;
use crate::syntax::{
    SyntaxNode, YamlBlockMapValue, YamlBlockSequence, YamlFlowSequence, parse_yaml_document,
};

enum ProjectRoot {
    Quarto(PathBuf),
    Bookdown(BookdownProject),
}

struct BookdownProject {
    root: PathBuf,
    first_file: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct MergeMetadata {
    title: Option<String>,
    bibliography: Vec<String>,
    metadata_files: Vec<String>,
    references: Vec<String>,
}

impl MergeMetadata {
    fn merge_from(&mut self, other: MergeMetadata) {
        if other.title.is_some() {
            self.title = other.title;
        }
        self.bibliography.extend(other.bibliography);
        self.metadata_files.extend(other.metadata_files);
        self.references.extend(other.references);
    }
}

pub fn extract_project_metadata(
    tree: &SyntaxNode,
    doc_path: &Path,
) -> Result<DocumentMetadata, YamlError> {
    extract_project_metadata_impl(tree, doc_path, true)
}

pub fn extract_project_metadata_without_bibliography_parse(
    tree: &SyntaxNode,
    doc_path: &Path,
) -> Result<DocumentMetadata, YamlError> {
    extract_project_metadata_impl(tree, doc_path, false)
}

fn extract_project_metadata_impl(
    tree: &SyntaxNode,
    doc_path: &Path,
    parse_bibliography: bool,
) -> Result<DocumentMetadata, YamlError> {
    let yaml_node = super::find_yaml_metadata_node(tree).map(|node| node.text().to_string());
    let yaml_offset = super::find_yaml_metadata_node(tree)
        .map(|node| node.text_range().start())
        .unwrap_or_default();

    let (doc_meta, doc_yaml, doc_yaml_offset) = if let Some(yaml_text) = yaml_node {
        let doc_yaml = strip_yaml_delimiters(&yaml_text);
        let doc_yaml_offset = yaml_offset + text_start_offset(&yaml_text);
        (parse_metadata_text(&doc_yaml)?, doc_yaml, doc_yaml_offset)
    } else {
        (
            MergeMetadata::default(),
            String::new(),
            rowan::TextSize::from(0),
        )
    };

    let mut sources: Vec<(MergeMetadata, PathBuf)> = Vec::new();
    let doc_dir = doc_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();

    if let Some(project_root) = find_project_root(doc_path) {
        match project_root {
            ProjectRoot::Quarto(project_root) => {
                let quarto_file = project_root.join("_quarto.yml");
                if quarto_file.exists() {
                    let project_meta = parse_metadata_file(&quarto_file)?;
                    push_with_includes(&mut sources, &project_root, project_meta)?;
                }

                let mut dirs = Vec::new();
                let mut dir = doc_dir.as_path();
                while dir.starts_with(&project_root) {
                    dirs.push(dir.to_path_buf());
                    if dir == project_root {
                        break;
                    }
                    dir = dir.parent().unwrap_or(project_root.as_path());
                }

                for dir in dirs.into_iter().rev() {
                    if let Some(source) = load_metadata_file(&dir, "_metadata.yml")? {
                        push_with_includes(&mut sources, &dir, source)?;
                    }
                }
            }
            ProjectRoot::Bookdown(project) => {
                for bookdown_file in ["_bookdown.yml", "_output.yml"] {
                    let path = project.root.join(bookdown_file);
                    if path.exists() {
                        let meta = parse_metadata_file(&path)?;
                        sources.push((strip_bibliography(meta), project.root.clone()));
                    }
                }

                let first_file = project
                    .first_file
                    .map(|file| project.root.join(file))
                    .unwrap_or_else(|| project.root.join("index.Rmd"));
                if let Some(index_meta) = load_frontmatter_file(&first_file)? {
                    push_with_includes(&mut sources, &project.root, index_meta)?;
                }
            }
        }
    }

    push_with_includes(&mut sources, &doc_dir, doc_meta)?;

    let mut merged = MergeMetadata::default();
    for (source, _) in &sources {
        merged.merge_from(source.clone());
    }

    let bibliography = extract_bibliography_from_sources(&sources, &doc_yaml, doc_yaml_offset);

    let bibliography_parse = if parse_bibliography {
        bibliography.as_ref().map(|info| {
            let index = bib::load_bibliography(&info.paths);
            BibliographyParse {
                parse_errors: index
                    .errors
                    .iter()
                    .map(|error| error.message.clone())
                    .collect(),
                index,
            }
        })
    } else {
        None
    };
    let mut inline_references =
        extract_doc_inline_references(&doc_yaml, doc_yaml_offset, doc_path)?;
    let project_references = merged
        .references
        .iter()
        .map(|id| InlineReference {
            id: id.clone(),
            range: TextRange::new(rowan::TextSize::from(0), rowan::TextSize::from(0)),
            path: doc_path.to_path_buf(),
        })
        .collect::<Vec<_>>();
    inline_references.extend(project_references);

    let metadata_files = collect_metadata_files(doc_path, &merged.metadata_files);
    let mut metadata = DocumentMetadata {
        source_path: doc_path.to_path_buf(),
        bibliography,
        metadata_files,
        bibliography_parse,
        inline_references,
        citations: super::CitationInfo { keys: Vec::new() },
        title: merged.title,
        raw_yaml: doc_yaml,
    };
    metadata.citations = extract_citations(tree);
    Ok(metadata)
}

/// Validate ONLY the document's own YAML frontmatter, with no project-file
/// reads. The LSP uses this so that project-manifest (`_quarto.yml` etc.) parse
/// errors stay off the document's own diagnostics — those are surfaced on the
/// manifest file's own URI instead (see `project_manifest_diagnostics`).
pub(crate) fn validate_doc_frontmatter(
    tree: &SyntaxNode,
    ctx: crate::parser::yaml::YamlValidationContext,
) -> Result<(), YamlError> {
    let Some(yaml_text) = super::find_yaml_metadata_node(tree).map(|node| node.text().to_string())
    else {
        return Ok(());
    };
    let doc_yaml = strip_yaml_delimiters(&yaml_text);
    // Validate against the document's real YAML consumers (per flavor), in
    // agreement with the parser's context-aware syntax-error channel — not the
    // 1.2 substrate. Otherwise a pandoc-accepted tab would be re-reported here
    // as a frontmatter parse error. Field extraction (below, in
    // `parse_metadata_text`) is infallible once the YAML validates, so this gate
    // is the only error source.
    crate::yaml_engine::validate_yaml_with_context(&doc_yaml, ctx).map_err(|err| {
        let (line, column) = byte_offset_to_line_col_1based(&doc_yaml, err.offset());
        YamlError::ParseError {
            message: err.message().to_string(),
            line: line as u64,
            column: column as u64,
            byte_offset: Some(err.offset()),
        }
    })
}

/// The project-manifest config files that contribute metadata to `doc_path`:
/// `_quarto.yml` at the project root, every `_metadata.yml` from the root down
/// to the document's directory (Quarto), or `_bookdown.yml`/`_output.yml`
/// (bookdown). Only existing files are returned. Mirrors the resolution order in
/// [`extract_project_metadata_impl`]; used to wire `EdgeKind::ProjectConfig`
/// edges so these files become tracked salsa inputs.
pub(crate) fn project_config_paths(doc_path: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let Some(project_root) = find_project_root(doc_path) else {
        return paths;
    };
    let doc_dir = doc_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();
    match project_root {
        ProjectRoot::Quarto(project_root) => {
            let quarto_file = project_root.join("_quarto.yml");
            if quarto_file.exists() {
                paths.push(quarto_file);
            }
            let mut dir = doc_dir.as_path();
            while dir.starts_with(&project_root) {
                let metadata_file = dir.join("_metadata.yml");
                if metadata_file.exists() {
                    paths.push(metadata_file);
                }
                if dir == project_root {
                    break;
                }
                dir = dir.parent().unwrap_or(project_root.as_path());
            }
        }
        ProjectRoot::Bookdown(project) => {
            for bookdown_file in ["_bookdown.yml", "_output.yml"] {
                let path = project.root.join(bookdown_file);
                if path.exists() {
                    paths.push(path);
                }
            }
        }
    }
    paths
}

fn parse_metadata_text(yaml_text: &str) -> Result<MergeMetadata, YamlError> {
    if yaml_text.trim().is_empty() {
        return Ok(MergeMetadata::default());
    }
    let parsed = parse_yaml_metadata_fields(yaml_text)?;
    Ok(MergeMetadata {
        title: parsed.title,
        bibliography: parsed
            .bibliography
            .into_iter()
            .map(|item| item.value)
            .collect(),
        metadata_files: parsed.metadata_files,
        references: parsed
            .references
            .into_iter()
            .map(|item| item.value)
            .collect(),
    })
}

fn extract_doc_inline_references(
    yaml_text: &str,
    yaml_offset: rowan::TextSize,
    doc_path: &Path,
) -> Result<Vec<super::InlineReference>, YamlError> {
    if yaml_text.trim().is_empty() {
        return Ok(Vec::new());
    }
    let parsed = parse_yaml_metadata_fields(yaml_text)?;
    Ok(parsed
        .references
        .into_iter()
        .map(|entry| InlineReference {
            id: entry.value,
            range: TextRange::new(
                yaml_offset + rowan::TextSize::from(entry.range.start as u32),
                yaml_offset + rowan::TextSize::from(entry.range.end as u32),
            ),
            path: doc_path.to_path_buf(),
        })
        .collect())
}

#[derive(Debug, Clone)]
struct MetadataScalar {
    value: String,
    range: std::ops::Range<usize>,
}

#[derive(Debug, Clone, Default)]
struct ParsedMetadataFields {
    title: Option<String>,
    bibliography: Vec<MetadataScalar>,
    metadata_files: Vec<String>,
    references: Vec<MetadataScalar>,
}

fn parse_yaml_metadata_fields(yaml_text: &str) -> Result<ParsedMetadataFields, YamlError> {
    crate::yaml_engine::validate_yaml(yaml_text).map_err(|err| {
        let (line, column) = byte_offset_to_line_col_1based(yaml_text, err.offset());
        YamlError::ParseError {
            message: err.message().to_string(),
            line: line as u64,
            column: column as u64,
            byte_offset: Some(err.offset()),
        }
    })?;
    let Some(map) = parse_yaml_document(yaml_text).and_then(|doc| doc.block_map()) else {
        return Ok(ParsedMetadataFields::default());
    };
    let title = map
        .value_of("title")
        .and_then(|value| block_map_value_to_scalar(&value))
        .map(|entry| entry.value);
    let bibliography = map
        .value_of("bibliography")
        .map(|value| block_map_value_to_scalar_list(&value))
        .unwrap_or_default();
    let metadata_files = map
        .value_of("metadata-files")
        .map(|value| block_map_value_to_scalar_list(&value))
        .unwrap_or_default()
        .into_iter()
        .map(|entry| entry.value)
        .collect();
    let references = map
        .value_of("references")
        .map(|value| extract_reference_ids(&value))
        .unwrap_or_default();
    Ok(ParsedMetadataFields {
        title,
        bibliography,
        metadata_files,
        references,
    })
}

fn extract_reference_ids(value: &YamlBlockMapValue) -> Vec<MetadataScalar> {
    let Some(seq) = value.as_block_sequence() else {
        return Vec::new();
    };
    seq.items()
        .filter_map(|item| {
            let map = item.as_block_map()?;
            let id = map.value_of("id")?;
            block_map_value_to_scalar(&id)
        })
        .collect()
}

fn block_map_value_to_scalar(value: &YamlBlockMapValue) -> Option<MetadataScalar> {
    value.as_scalar().map(metadata_scalar)
}

fn block_map_value_to_scalar_list(value: &YamlBlockMapValue) -> Vec<MetadataScalar> {
    if let Some(single) = value.as_scalar() {
        return vec![metadata_scalar(single)];
    }
    if let Some(seq) = value.as_flow_sequence() {
        return seq
            .items()
            .filter_map(|i| i.as_scalar())
            .map(metadata_scalar)
            .collect();
    }
    if let Some(seq) = value.as_block_sequence() {
        return seq
            .items()
            .filter_map(|i| i.as_scalar())
            .map(metadata_scalar)
            .collect();
    }
    Vec::new()
}

fn metadata_scalar(scalar: crate::syntax::YamlScalar) -> MetadataScalar {
    let range = scalar.text_range();
    MetadataScalar {
        value: scalar.value(),
        range: range.start().into()..range.end().into(),
    }
}

pub(crate) fn byte_offset_to_line_col_1based(input: &str, offset: usize) -> (usize, usize) {
    let mut line = 1usize;
    let mut line_start = 0usize;
    let bytes = input.as_bytes();
    let mut i = 0usize;
    let target = offset.min(input.len());
    while i < target {
        if bytes[i] == b'\n' {
            line += 1;
            line_start = i + 1;
        }
        i += 1;
    }
    let col = input[line_start..target].chars().count() + 1;
    (line, col)
}

fn parse_metadata_file(path: &Path) -> Result<MergeMetadata, YamlError> {
    let yaml =
        std::fs::read_to_string(path).map_err(|err| YamlError::StructureError(err.to_string()))?;
    parse_metadata_text(&yaml)
}

fn find_project_root(doc_path: &Path) -> Option<ProjectRoot> {
    let mut current = doc_path.parent()?;
    loop {
        let quarto = current.join("_quarto.yml");
        if quarto.exists() {
            return Some(ProjectRoot::Quarto(current.to_path_buf()));
        }
        let bookdown = current.join("_bookdown.yml");
        if bookdown.exists() {
            return Some(ProjectRoot::Bookdown(BookdownProject {
                root: current.to_path_buf(),
                first_file: read_bookdown_first_file(&bookdown, current),
            }));
        }
        current = current.parent()?;
    }
}

fn read_bookdown_first_file(path: &Path, root: &Path) -> Option<String> {
    let yaml = std::fs::read_to_string(path).ok()?;
    let index_exists = root.join("index.Rmd").exists();
    match parse_bookdown_rmd_files(&yaml) {
        None => default_bookdown_first_file(root, index_exists),
        Some(BookdownFiles::List(files)) => {
            let as_strings: Vec<String> = files
                .iter()
                .map(|path| path.to_string_lossy().to_string())
                .collect();
            select_first_bookdown_file(&as_strings, index_exists)
        }
        Some(BookdownFiles::ByFormat(formats)) => {
            let files = formats
                .get("html")
                .or_else(|| formats.get("latex"))
                .or_else(|| formats.values().next());
            files
                .map(|files| {
                    files
                        .iter()
                        .map(|path| path.to_string_lossy().to_string())
                        .collect::<Vec<_>>()
                })
                .and_then(|files| select_first_bookdown_file(&files, index_exists))
                .or_else(|| {
                    if index_exists {
                        Some("index.Rmd".to_string())
                    } else {
                        None
                    }
                })
        }
    }
}

pub(crate) enum BookdownFiles {
    List(Vec<PathBuf>),
    ByFormat(HashMap<String, Vec<PathBuf>>),
}

pub(crate) fn read_bookdown_files(root: &Path) -> Option<BookdownFiles> {
    let bookdown = root.join("_bookdown.yml");
    if !bookdown.exists() {
        return None;
    }
    let yaml = std::fs::read_to_string(&bookdown).ok()?;
    let files = parse_bookdown_rmd_files(&yaml)?;
    let files = match files {
        BookdownFiles::List(files) => {
            BookdownFiles::List(files.into_iter().map(|path| root.join(path)).collect())
        }
        BookdownFiles::ByFormat(formats) => BookdownFiles::ByFormat(
            formats
                .into_iter()
                .map(|(key, values)| {
                    (
                        key,
                        values.into_iter().map(|path| root.join(path)).collect(),
                    )
                })
                .collect(),
        ),
    };
    Some(files)
}

fn parse_bookdown_rmd_files(yaml: &str) -> Option<BookdownFiles> {
    let value = parse_yaml_document(yaml)?
        .block_map()?
        .value_of("rmd_files")?;
    if let Some(seq) = value.as_flow_sequence() {
        return Some(BookdownFiles::List(flow_seq_to_strings(&seq)));
    }
    if let Some(seq) = value.as_block_sequence() {
        return Some(BookdownFiles::List(block_seq_to_strings(&seq)));
    }
    if let Some(map) = value.as_block_map() {
        let formats = map
            .entries()
            .filter_map(|entry| {
                let key = entry.key_text()?;
                let value = entry.value()?;
                let files = if let Some(seq) = value.as_flow_sequence() {
                    flow_seq_to_strings(&seq)
                } else if let Some(seq) = value.as_block_sequence() {
                    block_seq_to_strings(&seq)
                } else {
                    return None;
                };
                Some((key, files))
            })
            .collect();
        return Some(BookdownFiles::ByFormat(formats));
    }
    None
}

fn block_seq_to_strings(seq: &YamlBlockSequence) -> Vec<PathBuf> {
    seq.items()
        .filter_map(|item| item.as_scalar().map(|s| PathBuf::from(s.value())))
        .collect()
}

fn flow_seq_to_strings(seq: &YamlFlowSequence) -> Vec<PathBuf> {
    seq.items()
        .filter_map(|item| item.as_scalar().map(|s| PathBuf::from(s.value())))
        .collect()
}

fn select_first_bookdown_file(files: &[String], index_exists: bool) -> Option<String> {
    if index_exists {
        return Some("index.Rmd".to_string());
    }
    files.iter().find(|file| !file.starts_with('_')).cloned()
}

fn default_bookdown_first_file(root: &Path, index_exists: bool) -> Option<String> {
    if index_exists {
        return Some("index.Rmd".to_string());
    }
    let mut candidates = Vec::new();
    let entries = std::fs::read_dir(root).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = path.file_name()?.to_string_lossy().to_string();
        if name.starts_with('_') {
            continue;
        }
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if matches!(ext, "Rmd" | "rmd") {
            candidates.push(name);
        }
    }
    candidates.sort();
    candidates.first().cloned()
}

fn load_metadata_file(base_dir: &Path, relative: &str) -> Result<Option<MergeMetadata>, YamlError> {
    let path = base_dir.join(relative);
    if !path.exists() {
        return Ok(None);
    }
    Ok(Some(parse_metadata_file(&path)?))
}

fn load_frontmatter_file(path: &Path) -> Result<Option<MergeMetadata>, YamlError> {
    if !path.exists() {
        return Ok(None);
    }
    let content =
        std::fs::read_to_string(path).map_err(|err| YamlError::StructureError(err.to_string()))?;
    let frontmatter = extract_frontmatter(&content);
    Ok(Some(parse_metadata_text(&frontmatter)?))
}

fn extract_frontmatter(input: &str) -> String {
    let mut lines = input.lines();
    let Some(first) = lines.next() else {
        return String::new();
    };
    if first.trim() != "---" {
        return String::new();
    }
    let mut yaml_lines = Vec::new();
    for line in lines {
        let trimmed = line.trim();
        if trimmed == "---" || trimmed == "..." {
            break;
        }
        yaml_lines.push(line);
    }
    yaml_lines.join("\n")
}

fn push_with_includes(
    sources: &mut Vec<(MergeMetadata, PathBuf)>,
    base_dir: &Path,
    meta: MergeMetadata,
) -> Result<(), YamlError> {
    let includes = meta.metadata_files.clone();
    sources.push((meta, base_dir.to_path_buf()));
    for include in includes {
        if let Some(source) = load_metadata_file(base_dir, &include)? {
            push_with_includes(sources, base_dir, source)?;
        }
    }
    Ok(())
}

fn strip_bibliography(mut meta: MergeMetadata) -> MergeMetadata {
    meta.bibliography.clear();
    meta
}

fn extract_bibliography_from_sources(
    sources: &[(MergeMetadata, PathBuf)],
    yaml_text: &str,
    yaml_offset: rowan::TextSize,
) -> Option<BibliographyInfo> {
    let mut resolved_paths = Vec::new();
    let mut ranges = Vec::new();

    for (source, base_dir) in sources {
        for path_str in &source.bibliography {
            resolved_paths.push(base_dir.join(path_str));
            let range = find_yaml_value_range(yaml_text, path_str, yaml_offset)
                .map(|(start, end)| TextRange::new(start, end))
                .unwrap_or_default();
            ranges.push(range);
        }
    }

    if resolved_paths.is_empty() {
        return None;
    }

    Some(BibliographyInfo {
        paths: resolved_paths,
        source_ranges: ranges,
    })
}

/// A bibliography file declared directly in a project manifest, paired with the
/// byte range of its path value within the manifest text. The range anchors
/// diagnostics back onto the manifest (e.g. an internal duplicate in a
/// project-level bibliography is reported against `_quarto.yml`, not against
/// every document that inherits it).
#[derive(Debug, Clone)]
pub struct ManifestBibliography {
    /// Bibliography path resolved relative to the manifest's directory.
    pub resolved_path: PathBuf,
    /// Byte range of the path value within the manifest text.
    pub value_range: std::ops::Range<usize>,
}

/// Bibliographies declared directly in a project manifest (`_quarto.yml`,
/// `_metadata.yml`, …), each paired with the byte range of its value in
/// `manifest_text`. Paths resolve relative to the manifest's directory. Returns
/// an empty list when the manifest declares no `bibliography`.
pub fn manifest_declared_bibliographies(
    manifest_path: &Path,
    manifest_text: &str,
) -> Result<Vec<ManifestBibliography>, YamlError> {
    let base_dir = manifest_path.parent().unwrap_or_else(|| Path::new("."));
    let parsed = parse_yaml_metadata_fields(manifest_text)?;
    Ok(parsed
        .bibliography
        .into_iter()
        .map(|scalar| ManifestBibliography {
            resolved_path: base_dir.join(&scalar.value),
            value_range: scalar.range,
        })
        .collect())
}

fn collect_metadata_files(doc_path: &Path, metadata_files: &[String]) -> Vec<PathBuf> {
    let doc_dir = doc_path.parent().unwrap_or_else(|| Path::new("."));
    metadata_files
        .iter()
        .map(|path| doc_dir.join(path))
        .collect()
}

fn find_yaml_value_range(
    yaml_text: &str,
    value: &str,
    yaml_offset: rowan::TextSize,
) -> Option<(rowan::TextSize, rowan::TextSize)> {
    let start = yaml_text.find(value)?;
    let start_offset = rowan::TextSize::from(start as u32) + yaml_offset;
    let end_offset = rowan::TextSize::from((start + value.len()) as u32) + yaml_offset;
    Some((start_offset, end_offset))
}

/// Offset of the first content byte of `input` relative to its own start,
/// i.e. the byte just past the opening `---` delimiter line. Used to map ranges
/// found in the delimiter-stripped YAML body back onto the full document.
fn text_start_offset(input: &str) -> rowan::TextSize {
    let Some(first) = input.lines().next() else {
        return rowan::TextSize::from(0);
    };
    if first.trim() != "---" {
        return rowan::TextSize::from(0);
    }
    rowan::TextSize::from(first.len() as u32 + 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    #[test]
    fn text_start_offset_points_past_opening_delimiter() {
        // The body's first byte sits right after the opening `---\n`, regardless
        // of how many content lines follow.
        assert_eq!(
            text_start_offset("---\nbibliography: references.bib\n---\n"),
            rowan::TextSize::from(4)
        );
        assert_eq!(
            text_start_offset("---\ntitle: x\nbibliography: refs.bib\n---"),
            rowan::TextSize::from(4)
        );
    }

    #[test]
    fn text_start_offset_without_delimiter_is_zero() {
        assert_eq!(
            text_start_offset("bibliography: refs.bib"),
            rowan::TextSize::from(0)
        );
    }

    #[test]
    fn bibliography_source_range_covers_the_value() {
        let input = "---\nbibliography: references.bib\n---\n";
        let tree = parse(input, None);
        let metadata =
            extract_project_metadata_without_bibliography_parse(&tree, Path::new("doc.qmd"))
                .unwrap();
        let bib = metadata.bibliography.expect("bibliography");
        let range = bib.source_ranges[0];
        assert_eq!(&input[range], "references.bib");
    }

    #[test]
    fn manifest_bibliographies_resolve_paths_and_ranges() {
        // A manifest's `bibliography` value resolves relative to the manifest
        // directory, and its byte range points back at the value in the text.
        let manifest = "project:\n  type: website\nbibliography: assets/bibliography.bib\n";
        let bibs = manifest_declared_bibliographies(Path::new("/proj/_quarto.yml"), manifest)
            .expect("parse");
        assert_eq!(bibs.len(), 1);
        assert_eq!(
            bibs[0].resolved_path,
            PathBuf::from("/proj/assets/bibliography.bib")
        );
        assert_eq!(
            &manifest[bibs[0].value_range.clone()],
            "assets/bibliography.bib"
        );
    }

    #[test]
    fn manifest_without_bibliography_is_empty() {
        let manifest = "project:\n  type: website\n";
        let bibs =
            manifest_declared_bibliographies(Path::new("/proj/_quarto.yml"), manifest).expect("ok");
        assert!(bibs.is_empty());
    }
}

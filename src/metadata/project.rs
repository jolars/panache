use std::path::{Path, PathBuf};

use rowan::TextRange;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::HashMap;

use super::bibliography::{BibliographyInfo, BibliographyParse};
use super::references::extract_inline_references;
use super::yaml::{YamlError, strip_yaml_delimiters};
use super::{DocumentMetadata, ReferenceEntry, extract_citations};
use crate::bibtex;
use crate::syntax::SyntaxNode;

enum ProjectRoot {
    Quarto(PathBuf),
    Bookdown(BookdownProject),
}

struct BookdownProject {
    root: PathBuf,
    first_file: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
enum StringOrArray {
    Single(String),
    Multiple(Vec<String>),
}

impl StringOrArray {
    fn into_vec(self) -> Vec<String> {
        match self {
            Self::Single(value) => vec![value],
            Self::Multiple(values) => values,
        }
    }
}

fn deserialize_bibliography<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<StringOrArray>::deserialize(deserializer)?;
    Ok(value.map(StringOrArray::into_vec).unwrap_or_default())
}

fn serialize_bibliography<S>(value: &Vec<String>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    match value.len() {
        0 => serializer.serialize_none(),
        1 => serializer.serialize_str(&value[0]),
        _ => value.serialize(serializer),
    }
}

fn deserialize_metadata_files<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<StringOrArray>::deserialize(deserializer)?;
    Ok(value.map(StringOrArray::into_vec).unwrap_or_default())
}

fn serialize_metadata_files<S>(value: &Vec<String>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    match value.len() {
        0 => serializer.serialize_none(),
        1 => serializer.serialize_str(&value[0]),
        _ => value.serialize(serializer),
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
struct MergeMetadata {
    title: Option<String>,
    #[serde(
        default,
        deserialize_with = "deserialize_bibliography",
        serialize_with = "serialize_bibliography",
        skip_serializing_if = "Vec::is_empty"
    )]
    bibliography: Vec<String>,
    #[serde(
        default,
        rename = "metadata-files",
        deserialize_with = "deserialize_metadata_files",
        serialize_with = "serialize_metadata_files",
        skip_serializing_if = "Vec::is_empty"
    )]
    metadata_files: Vec<String>,
    #[serde(default)]
    references: Vec<ReferenceEntry>,
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

    let mut sources = Vec::new();
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
                        sources.push(strip_bibliography(meta));
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
    for source in sources {
        merged.merge_from(source);
    }

    let bibliography = if merged.bibliography.is_empty() {
        None
    } else {
        Some(extract_bibliography_from_strings(
            &merged.bibliography,
            &doc_yaml,
            doc_yaml_offset,
            doc_path,
        ))
    };

    let bibliography_parse = bibliography.as_ref().map(|info| BibliographyParse {
        index: bibtex::load_bibliography(&info.paths),
    });
    let mut inline_references =
        extract_doc_inline_references(&doc_yaml, doc_yaml_offset, doc_path)?;
    let project_references =
        extract_inline_references(merged.references, rowan::TextSize::from(0), doc_path);
    inline_references.extend(project_references);

    let metadata_files = collect_metadata_files(doc_path, &merged.metadata_files);
    let mut metadata = DocumentMetadata {
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

fn parse_metadata_text(yaml_text: &str) -> Result<MergeMetadata, YamlError> {
    if yaml_text.trim().is_empty() {
        Ok(MergeMetadata::default())
    } else {
        Ok(serde_saphyr::from_str(yaml_text)?)
    }
}

#[derive(Debug, Deserialize)]
struct DocFrontmatter {
    references: Option<Vec<ReferenceEntry>>,
}

fn extract_doc_inline_references(
    yaml_text: &str,
    yaml_offset: rowan::TextSize,
    doc_path: &Path,
) -> Result<Vec<super::InlineReference>, YamlError> {
    if yaml_text.trim().is_empty() {
        return Ok(Vec::new());
    }
    let frontmatter: DocFrontmatter = serde_saphyr::from_str(yaml_text)?;
    Ok(frontmatter
        .references
        .map(|refs| extract_inline_references(refs, yaml_offset, doc_path))
        .unwrap_or_default())
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
    let doc: BookdownConfig = serde_saphyr::from_str(&yaml).ok()?;
    let index_exists = root.join("index.Rmd").exists();
    match doc.rmd_files {
        Some(RmdFiles::List(files)) => select_first_bookdown_file(&files, index_exists),
        Some(RmdFiles::ByFormat(formats)) => {
            let files = formats
                .get("html")
                .or_else(|| formats.get("latex"))
                .or_else(|| formats.values().next());
            files
                .and_then(|files| select_first_bookdown_file(files, index_exists))
                .or_else(|| {
                    if index_exists {
                        Some("index.Rmd".to_string())
                    } else {
                        None
                    }
                })
        }
        None => default_bookdown_first_file(root, index_exists),
    }
}

#[derive(Deserialize)]
struct BookdownConfig {
    rmd_files: Option<RmdFiles>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RmdFiles {
    List(Vec<String>),
    ByFormat(HashMap<String, Vec<String>>),
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
    let doc: BookdownConfig = serde_saphyr::from_str(&yaml).ok()?;
    let rmd_files = doc.rmd_files?;
    let files = match rmd_files {
        RmdFiles::List(files) => {
            BookdownFiles::List(files.into_iter().map(|f| root.join(f)).collect())
        }
        RmdFiles::ByFormat(formats) => BookdownFiles::ByFormat(
            formats
                .into_iter()
                .map(|(key, values)| (key, values.into_iter().map(|f| root.join(f)).collect()))
                .collect(),
        ),
    };
    Some(files)
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
    sources: &mut Vec<MergeMetadata>,
    base_dir: &Path,
    meta: MergeMetadata,
) -> Result<(), YamlError> {
    let includes = meta.metadata_files.clone();
    sources.push(meta);
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

fn extract_bibliography_from_strings(
    paths: &[String],
    yaml_text: &str,
    yaml_offset: rowan::TextSize,
    doc_path: &Path,
) -> BibliographyInfo {
    let doc_dir = doc_path.parent().unwrap_or_else(|| Path::new("."));
    let mut resolved_paths = Vec::new();
    let mut ranges = Vec::new();

    for path_str in paths {
        resolved_paths.push(doc_dir.join(path_str));
        let range = find_yaml_value_range(yaml_text, path_str, yaml_offset)
            .map(|(start, end)| TextRange::new(start, end))
            .unwrap_or_default();
        ranges.push(range);
    }

    BibliographyInfo {
        paths: resolved_paths,
        source_ranges: ranges,
    }
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

fn text_start_offset(input: &str) -> rowan::TextSize {
    let mut offset = rowan::TextSize::from(0);
    let mut lines = input.lines();
    let Some(first) = lines.next() else {
        return offset;
    };
    if first.trim() != "---" {
        return offset;
    }
    offset += rowan::TextSize::from(first.len() as u32 + 1);
    for line in lines {
        let trimmed = line.trim();
        if trimmed == "---" || trimmed == "..." {
            break;
        }
        offset += rowan::TextSize::from(line.len() as u32 + 1);
    }
    offset
}

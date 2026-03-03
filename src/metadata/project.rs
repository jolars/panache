use std::path::{Path, PathBuf};

use rowan::TextRange;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::bibliography::{BibliographyInfo, BibliographyParse};
use super::yaml::{YamlError, strip_yaml_delimiters};
use super::{DocumentMetadata, extract_citations};
use crate::bibtex;
use crate::syntax::SyntaxNode;

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
        skip_serializing_if = "Vec::is_empty"
    )]
    metadata_files: Vec<String>,
}

impl MergeMetadata {
    fn merge_from(&mut self, other: MergeMetadata) {
        if other.title.is_some() {
            self.title = other.title;
        }
        self.bibliography.extend(other.bibliography);
        self.metadata_files.extend(other.metadata_files);
    }
}

pub fn extract_project_metadata(
    tree: &SyntaxNode,
    doc_path: &Path,
) -> Result<DocumentMetadata, YamlError> {
    let yaml_node = super::find_yaml_metadata_node(tree).map(|node| node.text().to_string());

    let doc_meta = if let Some(yaml_text) = yaml_node {
        let doc_yaml = strip_yaml_delimiters(&yaml_text);
        parse_metadata_text(&doc_yaml)?
    } else {
        MergeMetadata::default()
    };

    let mut sources = Vec::new();
    let doc_dir = doc_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();

    if let Some(project_root) = find_project_root(doc_path) {
        let project_file = project_root.join("_quarto.yml");
        if project_file.exists() {
            let project_meta = parse_metadata_file(&project_file)?;
            let project_includes = project_meta.metadata_files.clone();
            sources.push(project_meta);

            for include in project_includes {
                if let Some(source) = load_metadata_file(&project_root, &include)? {
                    sources.push(source);
                }
            }
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
                sources.push(source);
            }
        }
    }

    let doc_includes = doc_meta.metadata_files.clone();
    sources.push(doc_meta);

    for include in doc_includes {
        if let Some(source) = load_metadata_file(&doc_dir, &include)? {
            sources.push(source);
        }
    }

    let mut merged = MergeMetadata::default();
    for source in sources {
        merged.merge_from(source);
    }

    let merged_yaml = serde_saphyr::to_string(&merged)
        .map_err(|err| YamlError::StructureError(err.to_string()))?;

    let bibliography = if merged.bibliography.is_empty() {
        None
    } else {
        Some(extract_bibliography_from_strings(
            &merged.bibliography,
            &merged_yaml,
            doc_path,
        ))
    };

    let bibliography_parse = bibliography.as_ref().map(|info| BibliographyParse {
        index: bibtex::load_bibliography(&info.paths),
    });

    let mut metadata = DocumentMetadata {
        bibliography,
        bibliography_parse,
        citations: super::CitationInfo { keys: Vec::new() },
        title: merged.title,
        raw_yaml: merged_yaml,
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

fn parse_metadata_file(path: &Path) -> Result<MergeMetadata, YamlError> {
    let yaml =
        std::fs::read_to_string(path).map_err(|err| YamlError::StructureError(err.to_string()))?;
    parse_metadata_text(&yaml)
}

fn find_project_root(doc_path: &Path) -> Option<PathBuf> {
    let mut current = doc_path.parent()?;
    loop {
        let candidate = current.join("_quarto.yml");
        if candidate.exists() {
            return Some(current.to_path_buf());
        }
        current = current.parent()?;
    }
}

fn load_metadata_file(base_dir: &Path, relative: &str) -> Result<Option<MergeMetadata>, YamlError> {
    let path = base_dir.join(relative);
    if !path.exists() {
        return Ok(None);
    }
    Ok(Some(parse_metadata_file(&path)?))
}

fn extract_bibliography_from_strings(
    paths: &[String],
    yaml_text: &str,
    doc_path: &Path,
) -> BibliographyInfo {
    let doc_dir = doc_path.parent().unwrap_or_else(|| Path::new("."));
    let mut resolved_paths = Vec::new();
    let mut ranges = Vec::new();

    for path_str in paths {
        resolved_paths.push(doc_dir.join(path_str));
        let range = find_yaml_value_range(yaml_text, path_str)
            .map(|(start, end)| TextRange::new(start, end))
            .unwrap_or_default();
        ranges.push(range);
    }

    BibliographyInfo {
        paths: resolved_paths,
        source_ranges: ranges,
    }
}

fn find_yaml_value_range(
    yaml_text: &str,
    value: &str,
) -> Option<(rowan::TextSize, rowan::TextSize)> {
    let start = yaml_text.find(value)?;
    let start_offset = rowan::TextSize::from(start as u32);
    let end_offset = rowan::TextSize::from((start + value.len()) as u32);
    Some((start_offset, end_offset))
}

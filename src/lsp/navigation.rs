use std::path::{Path, PathBuf};

use crate::lsp::uri_ext::UriExt;
use lsp_types::{Location, Range, Uri};

use crate::salsa::Db;

use super::conversions;

#[derive(Clone)]
pub(crate) struct IndexedDocument {
    pub(crate) uri: Uri,
    pub(crate) text: String,
    pub(crate) symbol_index: crate::salsa::SymbolUsageIndex,
}

#[derive(Clone)]
pub(crate) struct ProjectDocumentBundle {
    pub(crate) inputs: Vec<(PathBuf, String)>,
    pub(crate) parse_config: crate::config::Config,
}

pub(crate) fn project_document_inputs(
    db: &dyn Db,
    salsa_file: crate::salsa::FileText,
    salsa_config: crate::salsa::FileConfig,
    doc_path: &Path,
    current_content: &str,
) -> Vec<(PathBuf, String)> {
    let doc_paths = project_document_paths(db, salsa_file, salsa_config, doc_path);
    document_inputs_for_paths(db, doc_path, current_content, doc_paths)
}

pub(crate) fn project_document_bundle(
    db: &dyn Db,
    salsa_file: crate::salsa::FileText,
    salsa_config: crate::salsa::FileConfig,
    doc_path: &Path,
    current_content: &str,
) -> ProjectDocumentBundle {
    let inputs = project_document_inputs(db, salsa_file, salsa_config, doc_path, current_content);
    let parse_config = salsa_config.config(db).clone();

    ProjectDocumentBundle {
        inputs,
        parse_config,
    }
}

pub(crate) fn parse_with_config(
    input: &str,
    parse_config: &crate::config::Config,
) -> crate::syntax::SyntaxNode {
    crate::parse(input, Some(parse_config.clone()))
}

pub(crate) fn project_document_paths(
    db: &dyn Db,
    salsa_file: crate::salsa::FileText,
    salsa_config: crate::salsa::FileConfig,
    doc_path: &Path,
) -> Vec<PathBuf> {
    let mut doc_paths = crate::salsa::project_graph(db, salsa_file, salsa_config)
        .documents()
        .iter()
        .cloned()
        .collect::<Vec<_>>();
    if !doc_paths.contains(&doc_path.to_path_buf()) {
        doc_paths.push(doc_path.to_path_buf());
    }
    doc_paths.sort();
    doc_paths.dedup();

    doc_paths
}

pub(crate) fn document_inputs_for_paths(
    db: &dyn Db,
    doc_path: &Path,
    current_content: &str,
    mut doc_paths: Vec<PathBuf>,
) -> Vec<(PathBuf, String)> {
    if !doc_paths.contains(&doc_path.to_path_buf()) {
        doc_paths.push(doc_path.to_path_buf());
    }
    doc_paths.sort();
    doc_paths.dedup();

    let mut inputs = Vec::new();
    for path in doc_paths {
        let text = if path == doc_path {
            current_content.to_string()
        } else if let Some(file) = db.file_text(path.clone()) {
            file.content_or_empty(db).to_string()
        } else {
            continue;
        };
        inputs.push((path, text));
    }

    inputs
}

pub(crate) fn project_symbol_documents(
    db: &dyn Db,
    salsa_file: crate::salsa::FileText,
    salsa_config: crate::salsa::FileConfig,
    doc_path: &Path,
    current_uri: &Uri,
    current_content: &str,
) -> Vec<IndexedDocument> {
    let inputs = project_document_inputs(db, salsa_file, salsa_config, doc_path, current_content);
    indexed_documents_from_inputs(db, salsa_file, salsa_config, doc_path, current_uri, inputs)
}

pub(crate) fn indexed_documents_from_inputs(
    db: &dyn Db,
    salsa_file: crate::salsa::FileText,
    salsa_config: crate::salsa::FileConfig,
    doc_path: &Path,
    current_uri: &Uri,
    inputs: Vec<(PathBuf, String)>,
) -> Vec<IndexedDocument> {
    let mut docs = Vec::new();
    for (path, text) in inputs {
        let file = if path == doc_path {
            salsa_file
        } else if let Some(file) = crate::salsa::Db::file_text(db, path.clone()) {
            file
        } else {
            continue;
        };

        let symbol_index = crate::salsa::symbol_usage_index(db, file, salsa_config).clone();
        let uri = if path == doc_path {
            current_uri.clone()
        } else {
            Uri::from_file_path(&path).unwrap_or_else(|| current_uri.clone())
        };
        docs.push(IndexedDocument {
            uri,
            text,
            symbol_index,
        });
    }

    docs
}

pub(crate) fn location_from_range(uri: &Uri, text: &str, range: rowan::TextRange) -> Location {
    Location {
        uri: uri.clone(),
        range: Range {
            start: conversions::offset_to_position(text, range.start().into()),
            end: conversions::offset_to_position(text, range.end().into()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::parse_with_config;

    #[test]
    fn parse_with_config_uses_provided_configuration() {
        let input = "A ref to \\@ref(heading-2).";

        let mut disabled = crate::Config {
            flavor: crate::config::Flavor::RMarkdown,
            ..Default::default()
        };
        disabled.extensions.bookdown_references = false;

        let mut enabled = crate::Config {
            flavor: crate::config::Flavor::RMarkdown,
            ..Default::default()
        };
        enabled.extensions.bookdown_references = true;

        let disabled_tree = parse_with_config(input, &disabled);
        let enabled_tree = parse_with_config(input, &enabled);

        let disabled_has_crossref = disabled_tree
            .descendants()
            .any(|node| node.kind() == crate::syntax::SyntaxKind::CROSSREF);
        let enabled_has_crossref = enabled_tree
            .descendants()
            .any(|node| node.kind() == crate::syntax::SyntaxKind::CROSSREF);

        assert!(!disabled_has_crossref);
        assert!(enabled_has_crossref);
    }
}

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use rowan::GreenNode;
use tokio::sync::Mutex;
use tower_lsp_server::Client;
use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::{
    Location, OneOf, Position, Range, SymbolKind, Uri, WorkspaceSymbol, WorkspaceSymbolParams,
    WorkspaceSymbolResponse,
};

use crate::lsp::DocumentState;
use crate::lsp::conversions::offset_to_position;
use crate::salsa::{Db, HeadingOutlineEntry};
use crate::syntax::{AstNode, Heading, SyntaxKind, SyntaxNode};

pub(crate) async fn workspace_symbol(
    _client: &Client,
    document_map: Arc<Mutex<HashMap<String, DocumentState>>>,
    salsa_db: Arc<Mutex<crate::salsa::SalsaDb>>,
    _workspace_root: Arc<Mutex<Option<PathBuf>>>,
    params: WorkspaceSymbolParams,
) -> Result<Option<WorkspaceSymbolResponse>> {
    let open_documents: Vec<(String, DocumentState)> = {
        let map = document_map.lock().await;
        map.iter()
            .map(|(uri, state)| (uri.clone(), state.clone()))
            .collect()
    };

    if open_documents.is_empty() {
        return Ok(None);
    }

    let mut candidate_paths: HashSet<PathBuf> = HashSet::new();
    let mut path_configs: HashMap<PathBuf, crate::salsa::FileConfig> = HashMap::new();
    let mut path_uris: HashMap<PathBuf, Uri> = HashMap::new();
    let mut memory_docs: Vec<(Uri, String, GreenNode)> = Vec::new();

    {
        let db = salsa_db.lock().await;
        for (uri_str, state) in &open_documents {
            if let Some(path) = &state.path {
                candidate_paths.insert(path.clone());
                path_configs
                    .entry(path.clone())
                    .or_insert(state.salsa_config);
                if let Ok(uri) = uri_str.parse::<Uri>() {
                    path_uris.entry(path.clone()).or_insert(uri);
                }

                let graph = crate::salsa::project_graph(
                    &*db,
                    state.salsa_file,
                    state.salsa_config,
                    path.clone(),
                )
                .clone();

                for graph_path in graph.documents().iter().cloned() {
                    candidate_paths.insert(graph_path.clone());
                    path_configs.entry(graph_path).or_insert(state.salsa_config);
                }
            } else if let Ok(uri) = uri_str.parse::<Uri>() {
                let content = state.salsa_file.text(&*db).clone();
                memory_docs.push((uri, content, state.tree.clone()));
            }
        }
    }

    let query = params.query.trim().to_lowercase();
    let mut symbols = Vec::new();

    {
        let db = salsa_db.lock().await;
        for path in candidate_paths {
            let Some(file) = db.file_text(path.clone()) else {
                continue;
            };
            let Some(config) = path_configs.get(&path).copied() else {
                continue;
            };
            let uri = Uri::from_file_path(&path).or_else(|| path_uris.get(&path).cloned());
            let Some(uri) = uri else {
                continue;
            };

            let content = file.text(&*db).clone();
            let outline = crate::salsa::heading_outline(&*db, file, config, path).clone();
            symbols.extend(symbols_for_document(&uri, &content, &outline, &query));
        }
    }

    for (uri, content, green) in memory_docs {
        let root = SyntaxNode::new_root(green);
        let outline = heading_outline_from_root(&root);
        symbols.extend(symbols_for_document(&uri, &content, &outline, &query));
    }

    symbols.sort_by(compare_workspace_symbol);
    symbols.dedup_by(|a, b| {
        a.name == b.name
            && a.kind == b.kind
            && workspace_symbol_uri(a) == workspace_symbol_uri(b)
            && workspace_symbol_range(a) == workspace_symbol_range(b)
    });

    if symbols.is_empty() {
        Ok(None)
    } else {
        Ok(Some(WorkspaceSymbolResponse::Nested(symbols)))
    }
}

fn heading_outline_from_root(root: &SyntaxNode) -> Vec<HeadingOutlineEntry> {
    if root.kind() != SyntaxKind::DOCUMENT {
        return Vec::new();
    }

    root.children()
        .filter_map(Heading::cast)
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

fn symbols_for_document(
    uri: &Uri,
    content: &str,
    outline: &[HeadingOutlineEntry],
    query: &str,
) -> Vec<WorkspaceSymbol> {
    let mut symbols = Vec::new();
    let mut heading_stack: Vec<(usize, String)> = Vec::new();

    for entry in outline {
        while let Some((stack_level, _)) = heading_stack.last() {
            if *stack_level < entry.level {
                break;
            }
            heading_stack.pop();
        }

        let container_name = heading_stack.last().map(|(_, title)| title.clone());
        heading_stack.push((entry.level, entry.title.clone()));

        if !query.is_empty() && !entry.title.to_lowercase().contains(query) {
            continue;
        }

        symbols.push(make_workspace_symbol(
            entry.title.clone(),
            SymbolKind::NAMESPACE,
            Location {
                uri: uri.clone(),
                range: range_from_text_range(content, entry.range),
            },
            container_name,
        ));
    }

    symbols
}

fn make_workspace_symbol(
    name: String,
    kind: SymbolKind,
    location: Location,
    container_name: Option<String>,
) -> WorkspaceSymbol {
    WorkspaceSymbol {
        name,
        kind,
        tags: None,
        container_name,
        location: OneOf::Left(location),
        data: None,
    }
}

fn range_from_text_range(content: &str, range: rowan::TextRange) -> Range {
    Range {
        start: offset_to_position(content, range.start().into()),
        end: offset_to_position(content, range.end().into()),
    }
}

fn compare_workspace_symbol(a: &WorkspaceSymbol, b: &WorkspaceSymbol) -> std::cmp::Ordering {
    compare_locations(workspace_symbol_location(a), workspace_symbol_location(b))
        .then(a.name.cmp(&b.name))
        .then(a.container_name.cmp(&b.container_name))
}

fn workspace_symbol_location(symbol: &WorkspaceSymbol) -> Option<&Location> {
    match &symbol.location {
        OneOf::Left(location) => Some(location),
        OneOf::Right(_) => None,
    }
}

fn workspace_symbol_uri(symbol: &WorkspaceSymbol) -> &Uri {
    match &symbol.location {
        OneOf::Left(location) => &location.uri,
        OneOf::Right(location) => &location.uri,
    }
}

fn workspace_symbol_range(symbol: &WorkspaceSymbol) -> Option<&Range> {
    workspace_symbol_location(symbol).map(|location| &location.range)
}

fn compare_locations(a: Option<&Location>, b: Option<&Location>) -> std::cmp::Ordering {
    let a_uri = a.map(|location| location.uri.as_str()).unwrap_or("");
    let b_uri = b.map(|location| location.uri.as_str()).unwrap_or("");

    a_uri
        .cmp(b_uri)
        .then_with(|| {
            compare_positions(
                a.map(|location| &location.range.start),
                b.map(|location| &location.range.start),
            )
        })
        .then_with(|| {
            compare_positions(
                a.map(|location| &location.range.end),
                b.map(|location| &location.range.end),
            )
        })
}

fn compare_positions(a: Option<&Position>, b: Option<&Position>) -> std::cmp::Ordering {
    match (a, b) {
        (Some(a), Some(b)) => a.line.cmp(&b.line).then(a.character.cmp(&b.character)),
        (None, Some(_)) => std::cmp::Ordering::Less,
        (Some(_), None) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    }
}

#[cfg(test)]
mod tests {
    use super::{heading_outline_from_root, symbols_for_document};
    use std::env;
    use tower_lsp_server::ls_types::Uri;

    #[test]
    fn extracts_heading_symbols_with_container_names() {
        let content = "# Top\n\n## Child\n\n### Grandchild\n\n## Sibling\n";
        let uri = Uri::from_file_path(env::temp_dir().join("test.qmd")).expect("path uri");
        let root = crate::parse(content, None);
        let outline = heading_outline_from_root(&root);
        let symbols = symbols_for_document(&uri, content, &outline, "");

        assert_eq!(symbols.len(), 4);
        assert_eq!(symbols[0].name, "Top");
        assert_eq!(symbols[1].container_name.as_deref(), Some("Top"));
        assert_eq!(symbols[2].container_name.as_deref(), Some("Child"));
        assert_eq!(symbols[3].container_name.as_deref(), Some("Top"));
    }

    #[test]
    fn filters_heading_symbols_by_query() {
        let content = "# Intro\n\n## Methods\n\n## Results\n";
        let uri = Uri::from_file_path(env::temp_dir().join("test.qmd")).expect("path uri");
        let root = crate::parse(content, None);
        let outline = heading_outline_from_root(&root);
        let symbols = symbols_for_document(&uri, content, &outline, "intro");

        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "Intro");
    }
}

use std::fs;

use super::helpers::*;
use serde_json::json;
use tempfile::TempDir;
use tower_lsp_server::ls_types::Uri;

#[tokio::test]
async fn test_document_links_for_inline_and_image_links() {
    let server = TestLspServer::new();
    let content = "[site](https://example.com) ![img](images/photo.png)";
    server
        .open_document("file:///test.qmd", content, "quarto")
        .await;

    let links = server.document_links("file:///test.qmd").await;
    let Some(links) = links else {
        panic!("Expected document links");
    };

    assert!(
        links.iter().any(|link| {
            link.target
                .as_ref()
                .is_some_and(|uri| uri.as_str().starts_with("https://example.com"))
                && link.tooltip.is_none()
        }),
        "Expected external inline link target"
    );
    assert!(
        links.iter().any(|link| {
            link.target
                .as_ref()
                .is_some_and(|uri| uri.as_str().starts_with("https://example.com"))
                && link
                    .data
                    .as_ref()
                    .and_then(|data| data.get("tooltip"))
                    .and_then(|value| value.as_str())
                    == Some("Open link target")
        }),
        "Expected inline link tooltip in data for resolve"
    );
}

#[tokio::test]
async fn test_document_links_for_relative_image_path() {
    let temp_dir = TempDir::new().unwrap();
    let doc_path = temp_dir.path().join("doc.qmd");
    fs::write(&doc_path, "![img](images/photo.png)\n").unwrap();

    let server = TestLspServer::new();
    let uri = Uri::from_file_path(&doc_path).expect("doc uri");
    server
        .open_document(
            uri.as_str(),
            &fs::read_to_string(&doc_path).unwrap(),
            "quarto",
        )
        .await;

    let links = server.document_links(uri.as_str()).await;
    let Some(links) = links else {
        panic!("Expected document links");
    };

    assert!(
        links.iter().any(|link| {
            link.target
                .as_ref()
                .is_some_and(|uri| uri.as_str().ends_with("/images/photo.png"))
        }),
        "Expected image file target"
    );
}

#[tokio::test]
async fn test_document_links_for_autolinks() {
    let server = TestLspServer::new();
    let content = "Visit <https://example.com> or <person@example.com>.";
    server
        .open_document("file:///test.qmd", content, "quarto")
        .await;

    let links = server.document_links("file:///test.qmd").await;
    let Some(links) = links else {
        panic!("Expected document links");
    };

    assert!(
        links.iter().any(|link| {
            link.target
                .as_ref()
                .is_some_and(|uri| uri.as_str().starts_with("https://example.com"))
        }),
        "Expected autolink URL target"
    );
    assert!(
        links.iter().any(|link| {
            link.target
                .as_ref()
                .is_some_and(|uri| uri.as_str() == "mailto:person@example.com")
        }),
        "Expected autolink email target"
    );
}

#[tokio::test]
async fn test_document_links_include_shortcode_resolves_file() {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();
    let doc_path = root.join("doc.qmd");
    let include_path = root.join("chapters").join("part 1.qmd");
    fs::create_dir_all(include_path.parent().unwrap()).unwrap();
    fs::write(&include_path, "# Included\n").unwrap();
    fs::write(&doc_path, "{{< include \"chapters/part 1.qmd\" >}}\n").unwrap();

    let server = TestLspServer::new();
    let root_uri = Uri::from_file_path(root).expect("root uri");
    let doc_uri = Uri::from_file_path(&doc_path).expect("doc uri");
    server.initialize(root_uri.as_str()).await;
    server
        .open_document(
            doc_uri.as_str(),
            &fs::read_to_string(&doc_path).unwrap(),
            "quarto",
        )
        .await;

    let links = server.document_links(doc_uri.as_str()).await;
    let Some(links) = links else {
        panic!("Expected document links");
    };

    let expected = Uri::from_file_path(&include_path).expect("include uri");
    let include_link = links
        .into_iter()
        .find(|link| {
            link.target.is_none()
                && link
                    .data
                    .as_ref()
                    .and_then(|data| data.get("kind"))
                    .and_then(|value| value.as_str())
                    == Some("include")
        })
        .expect("Expected include shortcode document link");

    let resolved = server.resolve_document_link(include_link).await;
    assert_eq!(resolved.target, Some(expected));
}

#[tokio::test]
async fn test_document_links_ignore_escaped_shortcode() {
    let server = TestLspServer::new();
    let content = "{{{< include chapter.qmd >}}}";
    server
        .open_document("file:///test.qmd", content, "quarto")
        .await;

    let links = server.document_links("file:///test.qmd").await;
    assert!(
        links.is_none_or(|items| items.is_empty()),
        "Escaped shortcode should not produce document links"
    );
}

#[tokio::test]
async fn test_document_links_for_internal_anchor_destination() {
    let server = TestLspServer::new();
    let content = "[jump](#overview)\n\n# Overview {#overview}\n";
    server
        .open_document("file:///test.qmd", content, "quarto")
        .await;

    let links = server.document_links("file:///test.qmd").await;
    let Some(links) = links else {
        panic!("Expected document links");
    };

    assert!(
        links.iter().any(|link| {
            link.target
                .as_ref()
                .is_some_and(|uri| uri.as_str() == "file:///test.qmd#overview")
        }),
        "Expected same-document anchor target"
    );
}

#[tokio::test]
async fn test_document_links_for_reference_style_link() {
    let server = TestLspServer::new();
    let content = "See [docs][ref].\n\n[ref]: https://example.com/path\n";
    server
        .open_document("file:///test.qmd", content, "quarto")
        .await;

    let links = server.document_links("file:///test.qmd").await;
    let Some(links) = links else {
        panic!("Expected document links");
    };

    let reference_link = links
        .into_iter()
        .find(|link| {
            link.target.is_none()
                && link
                    .data
                    .as_ref()
                    .and_then(|data| data.get("kind"))
                    .and_then(|value| value.as_str())
                    == Some("reference")
        })
        .expect("Expected reference-style link from definition");

    let resolved = server.resolve_document_link(reference_link).await;
    assert!(
        resolved
            .target
            .as_ref()
            .is_some_and(|uri| uri.as_str().starts_with("https://example.com/path")),
        "Expected resolved reference-style link target"
    );
}

#[tokio::test]
async fn test_document_links_for_shortcut_reference_link() {
    let server = TestLspServer::new();
    let content = "See [guide].\n\n[guide]: https://example.com/guide\n";
    server
        .open_document("file:///test.qmd", content, "quarto")
        .await;

    let links = server.document_links("file:///test.qmd").await;
    let Some(links) = links else {
        panic!("Expected document links");
    };

    let reference_link = links
        .into_iter()
        .find(|link| {
            link.target.is_none()
                && link
                    .data
                    .as_ref()
                    .and_then(|data| data.get("kind"))
                    .and_then(|value| value.as_str())
                    == Some("reference")
        })
        .expect("Expected lazy reference link requiring resolve");

    let resolved = server.resolve_document_link(reference_link).await;
    assert!(
        resolved
            .target
            .as_ref()
            .is_some_and(|uri| uri.as_str().starts_with("https://example.com/guide")),
        "Expected resolved shortcut reference link target"
    );
}

#[tokio::test]
async fn test_document_link_resolve_backfills_tooltip_from_data() {
    let server = TestLspServer::new();

    let unresolved = tower_lsp_server::ls_types::DocumentLink {
        range: tower_lsp_server::ls_types::Range {
            start: tower_lsp_server::ls_types::Position {
                line: 0,
                character: 0,
            },
            end: tower_lsp_server::ls_types::Position {
                line: 0,
                character: 5,
            },
        },
        target: None,
        tooltip: None,
        data: Some(json!({
            "tooltip": "Open link target"
        })),
    };

    let resolved = server.resolve_document_link(unresolved).await;
    assert_eq!(resolved.tooltip.as_deref(), Some("Open link target"));
}

#[tokio::test]
async fn test_document_link_resolve_backfills_target_from_data() {
    let server = TestLspServer::new();

    let unresolved = tower_lsp_server::ls_types::DocumentLink {
        range: tower_lsp_server::ls_types::Range {
            start: tower_lsp_server::ls_types::Position {
                line: 0,
                character: 0,
            },
            end: tower_lsp_server::ls_types::Position {
                line: 0,
                character: 5,
            },
        },
        target: None,
        tooltip: None,
        data: Some(json!({
            "target": "https://example.com"
        })),
    };

    let resolved = server.resolve_document_link(unresolved).await;
    assert_eq!(resolved.target.unwrap().as_str(), "https://example.com");
}

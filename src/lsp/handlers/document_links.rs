use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use tokio::sync::Mutex;
use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::{DocumentLink, DocumentLinkParams, Range, Uri};

use crate::lsp::DocumentState;
use crate::syntax::{AstNode, AutoLink, ImageLink, Link, Shortcode};
use crate::utils::normalize_label;
use serde_json::json;

use super::super::conversions;

pub(crate) async fn document_links(
    document_map: Arc<Mutex<HashMap<String, DocumentState>>>,
    salsa_db: Arc<Mutex<crate::salsa::SalsaDb>>,
    params: DocumentLinkParams,
) -> Result<Option<Vec<DocumentLink>>> {
    let uri = params.text_document.uri;

    let Some(ctx) =
        crate::lsp::context::get_open_document_context(&document_map, &salsa_db, &uri).await
    else {
        return Ok(None);
    };
    let content = ctx.content.clone();
    let doc_path = ctx.path.clone();
    let salsa_file = ctx.salsa_file;
    let salsa_config = ctx.salsa_config;

    let reference_targets = if let Some(path) = doc_path.as_deref() {
        build_reference_targets(&salsa_db, salsa_file, salsa_config, path, &content, &uri).await
    } else {
        HashMap::new()
    };
    let root = ctx.syntax_root();
    let mut links = Vec::new();

    for node in root.descendants() {
        if let Some(link) = Link::cast(node.clone()) {
            if let Some(dest) = link.dest() {
                let dest_url = dest.url();
                let raw_target = extract_first_destination_token(&dest_url);
                if let Some(target) =
                    resolve_link_target(raw_target, doc_path.as_deref(), Some(&uri))
                {
                    links.push(build_document_link(
                        text_range_to_lsp_range(&content, dest.syntax().text_range()),
                        target,
                        "link",
                        "Open link target",
                        false,
                    ));
                }
            }

            if let Some(link_ref) = link.reference() {
                let mut label = link_ref.label();
                if label.is_empty()
                    && let Some(text) = link.text()
                {
                    label = text.text_content();
                }
                let key = normalize_label(&label);
                if let Some(target_info) = reference_targets.get(&key)
                    && let Some(target) = resolve_link_target(
                        &target_info.raw_target,
                        Some(&target_info.base_path),
                        target_info.base_uri.as_ref(),
                    )
                {
                    links.push(build_document_link(
                        text_range_to_lsp_range(&content, link_ref.syntax().text_range()),
                        target,
                        "reference",
                        "Open reference target",
                        true,
                    ));
                }
            } else if link.dest().is_none()
                && let Some(text) = link.text()
            {
                let key = normalize_label(&text.text_content());
                if let Some(target_info) = reference_targets.get(&key)
                    && let Some(target) = resolve_link_target(
                        &target_info.raw_target,
                        Some(&target_info.base_path),
                        target_info.base_uri.as_ref(),
                    )
                {
                    links.push(build_document_link(
                        text_range_to_lsp_range(&content, text.syntax().text_range()),
                        target,
                        "reference",
                        "Open reference target",
                        true,
                    ));
                }
            }
            continue;
        }

        if let Some(image) = ImageLink::cast(node.clone()) {
            if let Some(dest) = image.dest() {
                let dest_url = dest.url();
                let raw_target = extract_first_destination_token(&dest_url);
                if let Some(target) =
                    resolve_link_target(raw_target, doc_path.as_deref(), Some(&uri))
                {
                    links.push(build_document_link(
                        text_range_to_lsp_range(&content, dest.syntax().text_range()),
                        target,
                        "image",
                        "Open image target",
                        false,
                    ));
                }
            }
            continue;
        }

        if let Some(autolink) = AutoLink::cast(node.clone()) {
            let target_text = autolink.target();
            if let Some(target) = resolve_link_target(&target_text, doc_path.as_deref(), Some(&uri))
            {
                links.push(build_document_link(
                    text_range_to_lsp_range(&content, autolink.syntax().text_range()),
                    target,
                    "link",
                    "Open link target",
                    false,
                ));
            }
            continue;
        }

        if let Some(shortcode) = Shortcode::cast(node.clone()) {
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

            let Some(doc_path) = doc_path.as_deref() else {
                continue;
            };
            let base_dir = doc_path.parent().unwrap_or_else(|| Path::new("."));
            let project_root = crate::includes::find_quarto_root(doc_path)
                .or_else(|| crate::includes::find_bookdown_root(doc_path));
            let resolved =
                crate::includes::resolve_include_path(raw_path, base_dir, project_root.as_deref());

            if let Some(target) = Uri::from_file_path(&resolved) {
                links.push(build_document_link(
                    text_range_to_lsp_range(&content, shortcode.syntax().text_range()),
                    target,
                    "include",
                    "Open included file",
                    true,
                ));
            }
        }
    }

    if links.is_empty() {
        return Ok(None);
    }

    Ok(Some(links))
}

fn build_document_link(
    range: Range,
    target: Uri,
    kind: &str,
    tooltip: &str,
    lazy_target: bool,
) -> DocumentLink {
    DocumentLink {
        range,
        target: (!lazy_target).then_some(target.clone()),
        tooltip: None,
        data: Some(json!({
            "kind": kind,
            "tooltip": tooltip,
            "target": target.as_str(),
        })),
    }
}

pub(crate) async fn document_link_resolve(mut link: DocumentLink) -> Result<DocumentLink> {
    if link.tooltip.is_none()
        && let Some(data) = &link.data
        && let Some(tooltip) = data.get("tooltip").and_then(|value| value.as_str())
    {
        link.tooltip = Some(tooltip.to_string());
    }

    if link.target.is_none()
        && let Some(data) = &link.data
        && let Some(target) = data.get("target").and_then(|value| value.as_str())
        && let Ok(uri) = target.parse::<Uri>()
    {
        link.target = Some(uri);
    }

    Ok(link)
}

fn text_range_to_lsp_range(content: &str, range: rowan::TextRange) -> Range {
    let start = conversions::offset_to_position(content, range.start().into());
    let end = conversions::offset_to_position(content, range.end().into());
    Range { start, end }
}

pub(crate) fn resolve_link_target(
    raw_target: &str,
    doc_path: Option<&Path>,
    doc_uri: Option<&Uri>,
) -> Option<Uri> {
    let target = raw_target.trim();
    if target.is_empty() {
        return None;
    }

    if let Some(fragment) = target.strip_prefix('#') {
        return with_fragment(doc_uri?.clone(), fragment);
    }

    let with_mailto = if looks_like_email(target) {
        format!("mailto:{target}")
    } else {
        target.to_string()
    };

    if looks_like_uri_scheme(&with_mailto) {
        return with_mailto.parse::<Uri>().ok();
    }

    let doc_path = doc_path?;

    let (path_target, fragment) = split_fragment(target);

    let path = Path::new(path_target);
    let resolved = if path.is_absolute() {
        path.to_path_buf()
    } else {
        let base = doc_path.parent().unwrap_or_else(|| Path::new("."));
        base.join(path)
    };

    let uri = Uri::from_file_path(resolved)?;
    if let Some(fragment) = fragment {
        with_fragment(uri, fragment)
    } else {
        Some(uri)
    }
}

fn split_fragment(target: &str) -> (&str, Option<&str>) {
    if let Some((path, fragment)) = target.split_once('#') {
        (path, (!fragment.is_empty()).then_some(fragment))
    } else {
        (target, None)
    }
}

fn with_fragment(uri: Uri, fragment: &str) -> Option<Uri> {
    let escaped = fragment.replace(' ', "%20");
    format!("{}#{escaped}", uri.as_str()).parse::<Uri>().ok()
}

#[derive(Clone)]
pub(crate) struct ReferenceTarget {
    pub(crate) raw_target: String,
    pub(crate) base_path: std::path::PathBuf,
    pub(crate) base_uri: Option<Uri>,
}

pub(crate) async fn build_reference_targets(
    salsa_db: &Arc<Mutex<crate::salsa::SalsaDb>>,
    salsa_file: crate::salsa::FileText,
    salsa_config: crate::salsa::FileConfig,
    doc_path: &Path,
    doc_content: &str,
    fallback_uri: &Uri,
) -> HashMap<String, ReferenceTarget> {
    let bundle = crate::lsp::navigation::project_document_bundle(
        salsa_db,
        salsa_file,
        salsa_config,
        doc_path,
        doc_content,
    )
    .await;

    let mut out = HashMap::new();
    for (path, input) in bundle.inputs {
        let tree = crate::lsp::navigation::parse_with_config(&input, &bundle.parse_config);
        for def in tree
            .descendants()
            .filter_map(crate::syntax::ReferenceDefinition::cast)
        {
            let label = normalize_label(&def.label());
            if label.is_empty() || out.contains_key(&label) {
                continue;
            }
            let Some(raw_destination) = def.destination() else {
                continue;
            };
            let raw_target = extract_first_destination_token(&raw_destination);
            if raw_target.is_empty() {
                continue;
            }
            out.insert(
                label,
                ReferenceTarget {
                    raw_target: raw_target.to_string(),
                    base_uri: Uri::from_file_path(&path).or_else(|| Some(fallback_uri.clone())),
                    base_path: path.clone(),
                },
            );
        }
    }

    out
}

pub(crate) fn extract_first_destination_token(raw_dest: &str) -> &str {
    let trimmed = raw_dest.trim();
    if let Some(rest) = trimmed.strip_prefix('<')
        && let Some(end) = rest.find('>')
    {
        return &rest[..end];
    }

    trimmed.split_whitespace().next().unwrap_or("")
}

fn looks_like_email(text: &str) -> bool {
    text.contains('@') && !text.contains(':')
}

fn looks_like_uri_scheme(text: &str) -> bool {
    let Some(colon_idx) = text.find(':') else {
        return false;
    };

    if colon_idx == 1 {
        let bytes = text.as_bytes();
        if bytes.get(2).is_some_and(|b| *b == b'/' || *b == b'\\') {
            return false;
        }
    }

    let scheme = &text[..colon_idx];
    if scheme.is_empty() {
        return false;
    }

    let mut chars = scheme.chars();
    if !chars.next().is_some_and(|ch| ch.is_ascii_alphabetic()) {
        return false;
    }
    chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '+' || ch == '-' || ch == '.')
}

#[cfg(test)]
mod tests {
    use super::{
        extract_first_destination_token, looks_like_uri_scheme, resolve_link_target,
        split_fragment, with_fragment,
    };
    use tempfile::TempDir;
    use tower_lsp_server::ls_types::Uri;

    #[test]
    fn extracts_first_destination_token_before_title() {
        let token = extract_first_destination_token(r#"https://example.com \"Title\""#);
        assert_eq!(token, "https://example.com");
    }

    #[test]
    fn extracts_bracketed_destination_token() {
        let token = extract_first_destination_token("<docs/with spaces.md>");
        assert_eq!(token, "docs/with spaces.md");
    }

    #[test]
    fn detects_schemes_without_windows_drive_letters() {
        assert!(looks_like_uri_scheme("https://example.com"));
        assert!(looks_like_uri_scheme("mailto:user@example.com"));
        assert!(!looks_like_uri_scheme(r"C:\\docs\\chapter.md"));
    }

    #[test]
    fn resolves_relative_file_target() {
        let temp_dir = TempDir::new().expect("temp dir");
        let path = temp_dir.path().join("doc.qmd");
        let uri = Uri::from_file_path(&path).expect("doc uri");
        let target =
            resolve_link_target("notes/child.md", Some(&path), Some(&uri)).expect("file uri");
        let expected = Uri::from_file_path(temp_dir.path().join("notes").join("child.md"))
            .expect("expected uri");
        assert_eq!(target, expected);
    }

    #[test]
    fn resolves_same_document_anchor() {
        let temp_dir = TempDir::new().expect("temp dir");
        let path = temp_dir.path().join("doc.qmd");
        let uri = Uri::from_file_path(&path).expect("doc uri");
        let target = resolve_link_target("#sec-a", Some(&path), Some(&uri)).expect("anchor uri");
        assert_eq!(target.as_str(), format!("{}#sec-a", uri.as_str()));
    }

    #[test]
    fn resolves_relative_file_anchor_target() {
        let temp_dir = TempDir::new().expect("temp dir");
        let path = temp_dir.path().join("doc.qmd");
        let uri = Uri::from_file_path(&path).expect("doc uri");
        let target =
            resolve_link_target("notes/child.md#sec-b", Some(&path), Some(&uri)).expect("file uri");
        let base =
            Uri::from_file_path(temp_dir.path().join("notes").join("child.md")).expect("base uri");
        assert_eq!(target.as_str(), format!("{}#sec-b", base.as_str()));
    }

    #[test]
    fn splits_fragment_from_path() {
        let (path, fragment) = split_fragment("child.qmd#sec");
        assert_eq!(path, "child.qmd");
        assert_eq!(fragment, Some("sec"));
    }

    #[test]
    fn appends_fragment_to_uri() {
        let uri = "file:///workspace/doc.qmd".parse::<Uri>().expect("uri");
        let with = with_fragment(uri, "section title").expect("uri with fragment");
        assert_eq!(with.as_str(), "file:///workspace/doc.qmd#section%20title");
    }
}

use super::helpers::*;
use std::fs;
use tower_lsp_server::ls_types::Uri;

#[tokio::test]
async fn test_rename_citation_updates_bib_and_dependents() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let root = temp_dir.path();
    fs::write(root.join("_quarto.yml"), "project: default\n").unwrap();

    let bib_path = root.join("refs.bib");
    fs::write(&bib_path, "@article{oldkey,\n  title = {Old}\n}\n").unwrap();

    let doc1_path = root.join("doc1.qmd");
    let doc2_path = root.join("doc2.qmd");
    fs::write(
        &doc1_path,
        "---\nbibliography: refs.bib\n---\nSee [@oldkey].\n",
    )
    .unwrap();
    fs::write(
        &doc2_path,
        "---\nbibliography: refs.bib\n---\nAlso [@oldkey].\n",
    )
    .unwrap();

    let doc1_uri = Uri::from_file_path(&doc1_path).unwrap();
    let doc2_uri = Uri::from_file_path(&doc2_path).unwrap();
    let bib_uri = Uri::from_file_path(&bib_path).unwrap();
    let root_uri = Uri::from_file_path(root).unwrap();

    let server = TestLspServer::new();
    server.initialize(root_uri.as_str()).await;
    server
        .open_document(
            doc1_uri.as_str(),
            &fs::read_to_string(&doc1_path).unwrap(),
            "quarto",
        )
        .await;
    server
        .open_document(
            doc2_uri.as_str(),
            &fs::read_to_string(&doc2_path).unwrap(),
            "quarto",
        )
        .await;

    let edit = server
        .rename(doc1_uri.as_str(), 3, 7, "newkey")
        .await
        .expect("rename edit");
    let changes = edit.changes.expect("changes");

    assert!(changes.contains_key(&doc1_uri));
    assert!(changes.contains_key(&doc2_uri));
    assert!(changes.contains_key(&bib_uri));
}

#[tokio::test]
async fn test_rename_citation_updates_inline_references() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let root = temp_dir.path();
    std::fs::write(root.join("_quarto.yml"), "project: default\n").unwrap();

    let doc_path = root.join("doc.qmd");
    std::fs::write(
        &doc_path,
        "---\nreferences:\n  - id: oldkey\n    title: Inline\n---\n\nSee [@oldkey].\n",
    )
    .unwrap();

    let doc_uri = Uri::from_file_path(&doc_path).unwrap();
    let root_uri = Uri::from_file_path(root).unwrap();

    let server = TestLspServer::new();
    server.initialize(root_uri.as_str()).await;
    server
        .open_document(
            doc_uri.as_str(),
            &std::fs::read_to_string(&doc_path).unwrap(),
            "quarto",
        )
        .await;

    let edit = server
        .rename(doc_uri.as_str(), 6, 7, "newkey")
        .await
        .expect("rename edit");
    let changes = edit.changes.expect("changes");

    assert!(changes.contains_key(&doc_uri));
}

#[tokio::test]
async fn test_rename_citation_updates_csl_yaml() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let root = temp_dir.path();
    std::fs::write(root.join("_quarto.yml"), "project: default\n").unwrap();

    let bib_path = root.join("refs.yaml");
    std::fs::write(&bib_path, "- id: oldkey\n  title: Sample\n").unwrap();

    let doc_path = root.join("doc.qmd");
    std::fs::write(
        &doc_path,
        "---\nbibliography: refs.yaml\n---\n\nSee [@oldkey].\n",
    )
    .unwrap();

    let doc_uri = Uri::from_file_path(&doc_path).unwrap();
    let bib_uri = Uri::from_file_path(&bib_path).unwrap();
    let root_uri = Uri::from_file_path(root).unwrap();

    let server = TestLspServer::new();
    server.initialize(root_uri.as_str()).await;
    server
        .open_document(
            doc_uri.as_str(),
            &std::fs::read_to_string(&doc_path).unwrap(),
            "quarto",
        )
        .await;

    let edit = server
        .rename(doc_uri.as_str(), 4, 7, "newkey")
        .await
        .expect("rename edit");
    let changes = edit.changes.expect("changes");

    assert!(changes.contains_key(&doc_uri));
    assert!(changes.contains_key(&bib_uri));
}

#[tokio::test]
async fn test_rename_citation_updates_csl_json() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let root = temp_dir.path();
    std::fs::write(root.join("_quarto.yml"), "project: default\n").unwrap();

    let bib_path = root.join("refs.json");
    std::fs::write(&bib_path, "[{\"id\":\"oldkey\",\"title\":\"Sample\"}]").unwrap();

    let doc_path = root.join("doc.qmd");
    std::fs::write(
        &doc_path,
        "---\nbibliography: refs.json\n---\n\nSee [@oldkey].\n",
    )
    .unwrap();

    let doc_uri = Uri::from_file_path(&doc_path).unwrap();
    let bib_uri = Uri::from_file_path(&bib_path).unwrap();
    let root_uri = Uri::from_file_path(root).unwrap();

    let server = TestLspServer::new();
    server.initialize(root_uri.as_str()).await;
    server
        .open_document(
            doc_uri.as_str(),
            &std::fs::read_to_string(&doc_path).unwrap(),
            "quarto",
        )
        .await;

    let edit = server
        .rename(doc_uri.as_str(), 4, 7, "newkey")
        .await
        .expect("rename edit");
    let changes = edit.changes.expect("changes");

    assert!(changes.contains_key(&doc_uri));
    assert!(changes.contains_key(&bib_uri));
}

#[tokio::test]
async fn test_rename_citation_updates_ris() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let root = temp_dir.path();
    std::fs::write(root.join("_quarto.yml"), "project: default\n").unwrap();

    let bib_path = root.join("refs.ris");
    std::fs::write(&bib_path, "TY  - JOUR\nID  - oldkey\nER  - \n").unwrap();

    let doc_path = root.join("doc.qmd");
    std::fs::write(
        &doc_path,
        "---\nbibliography: refs.ris\n---\n\nSee [@oldkey].\n",
    )
    .unwrap();

    let doc_uri = Uri::from_file_path(&doc_path).unwrap();
    let bib_uri = Uri::from_file_path(&bib_path).unwrap();
    let root_uri = Uri::from_file_path(root).unwrap();

    let server = TestLspServer::new();
    server.initialize(root_uri.as_str()).await;
    server
        .open_document(
            doc_uri.as_str(),
            &std::fs::read_to_string(&doc_path).unwrap(),
            "quarto",
        )
        .await;

    let edit = server
        .rename(doc_uri.as_str(), 4, 7, "newkey")
        .await
        .expect("rename edit");
    let changes = edit.changes.expect("changes");

    assert!(changes.contains_key(&doc_uri));
    assert!(changes.contains_key(&bib_uri));
}

#[tokio::test]
async fn test_rename_chunk_label_updates_crossref_and_definition() {
    let server = TestLspServer::new();
    let content = r#"See @fig-plot.

```{r}
#| label: fig-plot
plot(1:10)
```
"#;
    server
        .open_document("file:///test.qmd", content, "quarto")
        .await;

    let edit = server
        .rename("file:///test.qmd", 0, 7, "fig-renamed")
        .await
        .expect("rename edit");
    let changes = edit.changes.expect("changes");
    let doc_uri: Uri = "file:///test.qmd".parse().unwrap();
    let edits = changes.get(&doc_uri).expect("doc edits");

    assert!(
        edits.iter().any(|e| e.new_text == "fig-renamed"),
        "expected rename edits to use new key"
    );
    assert!(
        edits.iter().any(|e| e.range.start.line == 0),
        "expected crossref reference edit"
    );
    assert!(
        edits.iter().any(|e| e.range.start.line == 3),
        "expected chunk label definition edit"
    );
}

#[tokio::test]
async fn test_rename_bookdown_chunk_label_updates_crossrefs_and_definition() {
    let server = TestLspServer::new();
    let content = r#"Figure \@ref(fig:a-label).

```{r}
#| label: a-label
#| fig-cap: "A caption."
plot(1, 1)
```
"#;
    server
        .open_document("file:///test.Rmd", content, "rmarkdown")
        .await;

    let edit = server
        .rename("file:///test.Rmd", 0, 16, "renamed-label")
        .await
        .expect("rename edit");
    let changes = edit.changes.expect("changes");
    let doc_uri: Uri = "file:///test.Rmd".parse().unwrap();
    let edits = changes.get(&doc_uri).expect("doc edits");

    assert!(
        edits.iter().any(|e| e.new_text == "renamed-label"),
        "expected rename edits to use new key"
    );
    assert!(
        edits.iter().any(|e| e.range.start.line == 0),
        "expected bookdown crossref reference edit"
    );
    assert!(
        edits.iter().any(|e| e.range.start.line == 3),
        "expected chunk label definition edit"
    );
}

#[tokio::test]
async fn test_rename_bookdown_theorem_crossref_updates_div_id() {
    let server = TestLspServer::new();
    let content = r#"Exercise \@ref(exr:mu).

::: {#mu .exercise}
foobar
:::
"#;
    server
        .open_document("file:///test.Rmd", content, "rmarkdown")
        .await;

    let edit = server
        .rename("file:///test.Rmd", 0, 18, "renamed-label")
        .await
        .expect("rename edit");
    let changes = edit.changes.expect("changes");
    let doc_uri: Uri = "file:///test.Rmd".parse().unwrap();
    let edits = changes.get(&doc_uri).expect("doc edits");

    assert!(
        edits.iter().any(|e| e.new_text == "renamed-label"),
        "expected rename edits to use new key"
    );
    assert!(
        edits.iter().any(|e| e.range.start.line == 0),
        "expected theorem crossref reference edit"
    );
    assert!(
        edits.iter().any(|e| e.range.start.line == 2),
        "expected fenced div id edit"
    );
}

#[tokio::test]
async fn test_rename_returns_none_inside_yaml_frontmatter() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let root = temp_dir.path();
    fs::write(root.join("_quarto.yml"), "project: default\n").unwrap();
    fs::write(
        root.join("refs.bib"),
        "@article{known,\n  title = {Known}\n}\n",
    )
    .unwrap();

    let doc_path = root.join("doc.qmd");
    fs::write(
        &doc_path,
        "---\ntitle: \"@known\"\nbibliography: refs.bib\n---\n\nSee [@known].\n",
    )
    .unwrap();

    let doc_uri = Uri::from_file_path(&doc_path).unwrap();
    let root_uri = Uri::from_file_path(root).unwrap();

    let server = TestLspServer::new();
    server.initialize(root_uri.as_str()).await;
    server
        .open_document(
            doc_uri.as_str(),
            &fs::read_to_string(&doc_path).unwrap(),
            "quarto",
        )
        .await;

    let edit = server.rename(doc_uri.as_str(), 1, 10, "renamed").await;
    assert!(
        edit.is_none(),
        "Expected no rename edits when cursor is inside YAML frontmatter"
    );
}

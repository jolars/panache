use super::helpers::*;
use std::fs;
use tower_lsp_server::ls_types::Uri;

#[tokio::test]
async fn test_references_crossref_chunk_label_without_declaration() {
    let server = TestLspServer::new();
    let content = r#"See @fig-plot and again @fig-plot.

```{r}
#| label: fig-plot
plot(1:10)
```
"#;
    server
        .open_document("file:///test.qmd", content, "quarto")
        .await;

    let refs = server
        .references("file:///test.qmd", 0, 6, false)
        .await
        .expect("references");

    assert_eq!(refs.len(), 2);
    assert!(refs.iter().all(|loc| loc.range.start.line == 0));
}

#[tokio::test]
async fn test_references_crossref_chunk_label_with_declaration() {
    let server = TestLspServer::new();
    let content = r#"See @fig-plot and again @fig-plot.

```{r}
#| label: fig-plot
plot(1:10)
```
"#;
    server
        .open_document("file:///test.qmd", content, "quarto")
        .await;

    let refs = server
        .references("file:///test.qmd", 3, 12, true)
        .await
        .expect("references");

    assert!(refs.iter().any(|loc| loc.range.start.line == 0));
    assert!(refs.iter().any(|loc| loc.range.start.line == 3));
}

#[tokio::test]
async fn test_references_bookdown_crossref_chunk_label_with_declaration() {
    let server = TestLspServer::new();
    let content = r#"Figure \@ref(fig:a-label)
Figure \@ref(fig:a-label)

```{r}
#| label: a-label
#| fig-cap: "A caption."
plot(1, 1)
```
"#;
    server
        .open_document("file:///test.Rmd", content, "rmarkdown")
        .await;

    let refs = server
        .references("file:///test.Rmd", 0, 16, true)
        .await
        .expect("references");

    assert!(refs.iter().filter(|loc| loc.range.start.line == 0).count() == 1);
    assert!(refs.iter().filter(|loc| loc.range.start.line == 1).count() == 1);
    assert!(refs.iter().any(|loc| loc.range.start.line == 4));
}

#[tokio::test]
async fn test_references_bookdown_theorem_crossref_with_declaration() {
    let server = TestLspServer::new();
    let content = r#"Exercise \@ref(exr:mu)
Again \@ref(exr:mu)

::: {#mu .exercise}
foobar
:::
"#;
    server
        .open_document("file:///test.Rmd", content, "rmarkdown")
        .await;

    let refs = server
        .references("file:///test.Rmd", 0, 18, true)
        .await
        .expect("references");

    assert!(refs.iter().filter(|loc| loc.range.start.line == 0).count() == 1);
    assert!(refs.iter().filter(|loc| loc.range.start.line == 1).count() == 1);
    assert!(refs.iter().any(|loc| loc.range.start.line == 3));
}

#[tokio::test]
async fn test_references_bookdown_theorem_from_div_id_with_declaration() {
    let server = TestLspServer::new();
    let content = r#"Exercise \@ref(exr:mu)
Again \@ref(exr:mu)

::: {#mu .exercise}
foobar
:::
"#;
    server
        .open_document("file:///test.Rmd", content, "rmarkdown")
        .await;

    let refs = server
        .references("file:///test.Rmd", 3, 7, true)
        .await
        .expect("references");

    assert!(refs.iter().filter(|loc| loc.range.start.line == 0).count() == 1);
    assert!(refs.iter().filter(|loc| loc.range.start.line == 1).count() == 1);
    assert!(refs.iter().any(|loc| loc.range.start.line == 3));
}

#[tokio::test]
async fn test_references_citation_without_declaration() {
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
            Uri::from_file_path(&doc2_path).unwrap().as_str(),
            &fs::read_to_string(&doc2_path).unwrap(),
            "quarto",
        )
        .await;

    let refs = server
        .references(doc1_uri.as_str(), 3, 7, false)
        .await
        .expect("references");

    assert_eq!(refs.len(), 2);
    assert!(
        refs.iter()
            .all(|loc| loc.uri != Uri::from_file_path(&bib_path).unwrap())
    );
}

#[tokio::test]
async fn test_references_citation_with_declaration() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let root = temp_dir.path();
    fs::write(root.join("_quarto.yml"), "project: default\n").unwrap();

    let bib_path = root.join("refs.bib");
    fs::write(&bib_path, "@article{oldkey,\n  title = {Old}\n}\n").unwrap();

    let doc_path = root.join("doc.qmd");
    fs::write(
        &doc_path,
        "---\nbibliography: refs.bib\n---\nSee [@oldkey].\n",
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
            &fs::read_to_string(&doc_path).unwrap(),
            "quarto",
        )
        .await;

    let refs = server
        .references(doc_uri.as_str(), 3, 7, true)
        .await
        .expect("references");

    assert!(refs.iter().any(|loc| loc.uri == bib_uri));
    assert!(refs.iter().any(|loc| loc.uri == doc_uri));
}

#[tokio::test]
async fn test_references_citation_skips_bibliography_declaration_for_invalid_yaml() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let root = temp_dir.path();
    fs::write(root.join("_quarto.yml"), "project: default\n").unwrap();

    let bib_path = root.join("refs.bib");
    fs::write(&bib_path, "@article{oldkey,\n  title = {Old}\n}\n").unwrap();

    let doc_path = root.join("doc.qmd");
    fs::write(&doc_path, "---\nbibliography: [\n---\nSee [@oldkey].\n").unwrap();

    let doc_uri = Uri::from_file_path(&doc_path).unwrap();
    let bib_uri = Uri::from_file_path(&bib_path).unwrap();
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

    let refs = server
        .references(doc_uri.as_str(), 3, 7, true)
        .await
        .expect("references");

    assert!(
        refs.iter().all(|loc| loc.uri != bib_uri),
        "Invalid YAML should suppress bibliography declaration references"
    );
    assert!(
        refs.iter().any(|loc| loc.uri == doc_uri),
        "Document citation usage should still be reported"
    );
}

#[tokio::test]
async fn test_references_returns_none_inside_yaml_frontmatter() {
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

    let refs = server.references(doc_uri.as_str(), 1, 10, true).await;
    assert!(
        refs.is_none(),
        "Expected no references when cursor is inside YAML frontmatter"
    );
}

#[tokio::test]
async fn test_references_heading_hash_link_and_id_are_consistent() {
    let server = TestLspServer::new();
    let content = "# Heading {#heading}\n\nSee [label](#heading).\n";
    server
        .open_document("file:///test.md", content, "markdown")
        .await;

    let hash_locations = server
        .references("file:///test.md", 2, 14, true)
        .await
        .expect("references from hash link");
    let id_locations = server
        .references("file:///test.md", 0, 12, true)
        .await
        .expect("references from heading id");

    assert_eq!(hash_locations, id_locations);
    assert!(hash_locations.iter().any(|loc| loc.range.start.line == 0));
    assert!(hash_locations.iter().any(|loc| loc.range.start.line == 2));
}

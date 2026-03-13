use super::helpers::*;

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

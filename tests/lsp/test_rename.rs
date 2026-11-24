use super::helpers::*;
use lsp_types::{Position, Range, TextEdit, Uri};
use std::fs;

#[test]
fn test_rename_reference_link_updates_links_images_and_definition() {
    let mut server = TestLspServer::new();
    let content = "[text][label]\n![alt][label]\n\n[label]: https://example.com \"Title\"\n";
    server.open_document("file:///test.md", content, "markdown");
    let doc_uri: Uri = "file:///test.md".parse().unwrap();
    let expected = vec![
        TextEdit::new(
            Range::new(Position::new(0, 7), Position::new(0, 12)),
            "renamed".into(),
        ),
        TextEdit::new(
            Range::new(Position::new(1, 7), Position::new(1, 12)),
            "renamed".into(),
        ),
        TextEdit::new(
            Range::new(Position::new(3, 1), Position::new(3, 6)),
            "renamed".into(),
        ),
    ];

    for (line, character) in [(0, 9), (1, 9), (3, 3)] {
        let edit = server
            .rename(doc_uri.as_str(), line, character, "renamed")
            .expect("rename from a link, image, or definition label");
        let changes = edit.changes.expect("changes");
        let mut edits = changes.get(&doc_uri).expect("doc edits").clone();
        edits.sort_by_key(|edit| edit.range.start);
        assert_eq!(edits, expected);
    }
}

#[test]
fn test_rename_reference_link_matches_case_and_whitespace_variants() {
    let mut server = TestLspServer::new();
    let content = "[text][Some Label]\n[more][some   label]\n\n[SOME LABEL]: https://example.com\n";
    server.open_document("file:///test.md", content, "markdown");

    let edit = server
        .rename("file:///test.md", 0, 9, "renamed")
        .expect("reference rename edit");
    let changes = edit.changes.expect("changes");
    let doc_uri: Uri = "file:///test.md".parse().unwrap();
    let mut edits = changes.get(&doc_uri).expect("doc edits").clone();
    edits.sort_by_key(|edit| edit.range.start);
    assert_eq!(
        edits,
        vec![
            TextEdit::new(
                Range::new(Position::new(0, 7), Position::new(0, 17)),
                "renamed".into()
            ),
            TextEdit::new(
                Range::new(Position::new(1, 7), Position::new(1, 19)),
                "renamed".into()
            ),
            TextEdit::new(
                Range::new(Position::new(3, 1), Position::new(3, 11)),
                "renamed".into()
            ),
        ]
    );
}

#[test]
fn test_rename_reference_link_uses_utf16_ranges() {
    let mut server = TestLspServer::new();
    let content = "😀 [text][café]\n\n[café]: https://example.com\n";
    server.open_document("file:///test.md", content, "markdown");

    let edit = server
        .rename("file:///test.md", 0, 12, "résumé")
        .expect("reference rename edit");
    let changes = edit.changes.expect("changes");
    let doc_uri: Uri = "file:///test.md".parse().unwrap();
    let mut edits = changes.get(&doc_uri).expect("doc edits").clone();
    edits.sort_by_key(|edit| edit.range.start);
    assert_eq!(
        edits,
        vec![
            TextEdit::new(
                Range::new(Position::new(0, 10), Position::new(0, 14)),
                "résumé".into()
            ),
            TextEdit::new(
                Range::new(Position::new(2, 1), Position::new(2, 5)),
                "résumé".into()
            ),
        ]
    );
}

#[test]
fn test_rename_reference_link_is_scoped_to_matching_links_in_current_document() {
    let mut server = TestLspServer::new();
    let content = "[text][label]\n\n[label]: https://example.com\n\n[other][keep]\n\n[keep]: https://keep.example\n\nSee[^label].\n\n[^label]: Footnote.\n";
    server.open_document("file:///test.md", content, "markdown");
    server.open_document("file:///other.md", content, "markdown");

    let edit = server
        .rename("file:///test.md", 0, 9, "renamed")
        .expect("reference rename edit");
    let changes = edit.changes.expect("changes");
    let doc_uri: Uri = "file:///test.md".parse().unwrap();
    assert_eq!(changes.len(), 1);
    let mut edits = changes.get(&doc_uri).expect("doc edits").clone();
    edits.sort_by_key(|edit| edit.range.start);
    assert_eq!(
        edits,
        vec![
            TextEdit::new(
                Range::new(Position::new(0, 7), Position::new(0, 12)),
                "renamed".into()
            ),
            TextEdit::new(
                Range::new(Position::new(2, 1), Position::new(2, 6)),
                "renamed".into()
            ),
        ]
    );
}

#[test]
fn test_rename_citation_updates_bib_and_dependents() {
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

    let mut server = TestLspServer::new();
    server.initialize(root_uri.as_str());
    server.open_document(
        doc1_uri.as_str(),
        &fs::read_to_string(&doc1_path).unwrap(),
        "quarto",
    );
    server.open_document(
        doc2_uri.as_str(),
        &fs::read_to_string(&doc2_path).unwrap(),
        "quarto",
    );

    let edit = server
        .rename(doc1_uri.as_str(), 3, 7, "newkey")
        .expect("rename edit");
    let changes = edit.changes.expect("changes");

    assert!(changes.contains_key(&doc1_uri));
    assert!(changes.contains_key(&doc2_uri));
    assert!(changes.contains_key(&bib_uri));
}

#[test]
fn test_rename_citation_updates_inline_references() {
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

    let mut server = TestLspServer::new();
    server.initialize(root_uri.as_str());
    server.open_document(
        doc_uri.as_str(),
        &std::fs::read_to_string(&doc_path).unwrap(),
        "quarto",
    );

    let edit = server
        .rename(doc_uri.as_str(), 6, 7, "newkey")
        .expect("rename edit");
    let changes = edit.changes.expect("changes");

    assert!(changes.contains_key(&doc_uri));
}

#[test]
fn test_rename_citation_updates_csl_yaml() {
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

    let mut server = TestLspServer::new();
    server.initialize(root_uri.as_str());
    server.open_document(
        doc_uri.as_str(),
        &std::fs::read_to_string(&doc_path).unwrap(),
        "quarto",
    );

    let edit = server
        .rename(doc_uri.as_str(), 4, 7, "newkey")
        .expect("rename edit");
    let changes = edit.changes.expect("changes");

    assert!(changes.contains_key(&doc_uri));
    assert!(changes.contains_key(&bib_uri));
}

#[test]
fn test_rename_citation_updates_csl_json() {
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

    let mut server = TestLspServer::new();
    server.initialize(root_uri.as_str());
    server.open_document(
        doc_uri.as_str(),
        &std::fs::read_to_string(&doc_path).unwrap(),
        "quarto",
    );

    let edit = server
        .rename(doc_uri.as_str(), 4, 7, "newkey")
        .expect("rename edit");
    let changes = edit.changes.expect("changes");

    assert!(changes.contains_key(&doc_uri));
    assert!(changes.contains_key(&bib_uri));
}

#[test]
fn test_rename_citation_updates_ris() {
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

    let mut server = TestLspServer::new();
    server.initialize(root_uri.as_str());
    server.open_document(
        doc_uri.as_str(),
        &std::fs::read_to_string(&doc_path).unwrap(),
        "quarto",
    );

    let edit = server
        .rename(doc_uri.as_str(), 4, 7, "newkey")
        .expect("rename edit");
    let changes = edit.changes.expect("changes");

    assert!(changes.contains_key(&doc_uri));
    assert!(changes.contains_key(&bib_uri));
}

#[test]
fn test_rename_chunk_label_updates_crossref_and_definition() {
    let mut server = TestLspServer::new();
    let content = r#"See @fig-plot.

```{r}
#| label: fig-plot
plot(1:10)
```
"#;
    server.open_document("file:///test.qmd", content, "quarto");

    let edit = server
        .rename("file:///test.qmd", 0, 7, "fig-renamed")
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

#[test]
fn test_rename_example_counter_reset_preserves_number() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let root = temp_dir.path();
    let doc_path = root.join("doc.md");
    std::fs::write(root.join("panache.toml"), "flavor = \"pandoc\"\n").unwrap();
    let content = "(12@good) First example.\n\nSee (@good).\n";
    std::fs::write(&doc_path, content).unwrap();
    let mut server = TestLspServer::new();
    let root_uri = Uri::from_file_path(root).unwrap();
    let doc_uri = Uri::from_file_path(&doc_path).unwrap();
    server.initialize(root_uri.as_str());
    server.open_document(doc_uri.as_str(), content, "markdown");

    for (line, character) in [(0, 5), (2, 7)] {
        let edit = server
            .rename(doc_uri.as_str(), line, character, "better")
            .expect("rename from definition or reference");
        let changes = edit.changes.unwrap();
        let edits = changes.get(&doc_uri).unwrap();
        assert_eq!(edits.len(), 2);
        let definition = edits
            .iter()
            .find(|edit| edit.range.start.line == 0)
            .unwrap();
        assert_eq!(definition.range.start.character, 4);
        assert_eq!(definition.range.end.character, 8);
        assert_eq!(definition.new_text, "better");
    }
}

#[test]
fn test_rename_numbered_example_label_updates_definition_and_reference() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let root = temp_dir.path();
    let doc_path = root.join("doc.qmd");
    std::fs::write(root.join("panache.toml"), "flavor = \"pandoc\"\n").unwrap();
    let mut server = TestLspServer::new();
    let content = "(@good) First example.\n\nAs (@good) illustrates, details follow.\n";
    std::fs::write(&doc_path, content).unwrap();
    let root_uri = Uri::from_file_path(root).expect("root uri");
    let doc_uri = Uri::from_file_path(&doc_path).expect("doc uri");
    server.initialize(root_uri.as_str());
    server.open_document(doc_uri.as_str(), content, "quarto");

    let edit = server
        .rename(doc_uri.as_str(), 2, 7, "better")
        .expect("rename edit");
    let changes = edit.changes.expect("changes");
    let edits = changes.get(&doc_uri).expect("doc edits");

    assert!(
        edits.iter().any(|e| e.new_text == "better"),
        "expected rename edits to use new example label"
    );
    assert!(
        edits.iter().any(|e| e.range.start.line == 0),
        "expected example definition edit"
    );
    assert!(
        edits.iter().any(|e| e.range.start.line == 2),
        "expected example reference edit"
    );
}

#[test]
fn test_rename_numbered_example_label_extension_off_returns_none() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let root = temp_dir.path();
    let doc_path = root.join("doc.qmd");
    std::fs::write(
        root.join("panache.toml"),
        "flavor = \"pandoc\"\n[extensions]\nexample-lists = false\n",
    )
    .unwrap();
    let mut server = TestLspServer::new();
    let content = "(@good) First example.\n\nAs (@good) illustrates.\n";
    std::fs::write(&doc_path, content).unwrap();
    let root_uri = Uri::from_file_path(root).expect("root uri");
    let doc_uri = Uri::from_file_path(&doc_path).expect("doc uri");
    server.initialize(root_uri.as_str());
    server.open_document(doc_uri.as_str(), content, "quarto");

    let edit = server.rename(doc_uri.as_str(), 2, 7, "better");
    assert!(
        edit.is_none(),
        "expected no rename edit when example_lists is disabled"
    );
}

#[test]
fn test_rename_bookdown_chunk_label_updates_crossrefs_and_definition() {
    let mut server = TestLspServer::new();
    let content = r#"Figure \@ref(fig:a-label).

```{r}
#| label: a-label
#| fig-cap: "A caption."
plot(1, 1)
```
"#;
    server.open_document("file:///test.Rmd", content, "rmarkdown");

    let edit = server
        .rename("file:///test.Rmd", 0, 16, "renamed-label")
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

#[test]
fn test_rename_bookdown_theorem_crossref_updates_div_id() {
    let mut server = TestLspServer::new();
    let content = r#"Exercise \@ref(exr:mu).

::: {#mu .exercise}
foobar
:::
"#;
    server.open_document("file:///test.Rmd", content, "rmarkdown");

    let edit = server
        .rename("file:///test.Rmd", 0, 18, "renamed-label")
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

#[test]
fn vitepress_renames_named_heading_id_from_the_declaration() {
    let project = tempfile::TempDir::new().unwrap();
    fs::write(
        project.path().join("panache.toml"),
        "flavor = 'vitepress'\n",
    )
    .unwrap();
    let root = Uri::from_file_path(project.path()).unwrap();
    let doc = Uri::from_file_path(project.path().join("guide.md")).unwrap();
    let mut server = TestLspServer::new();
    server.initialize(root.as_str());
    server.open_document(
        doc.as_str(),
        "# Setup {id=\"install\"}\n\n[Link](#install)\n",
        "markdown",
    );

    let declaration = Range::new(Position::new(0, 13), Position::new(0, 20));
    let prepared = server.prepare_rename(doc.as_str(), 0, 15).unwrap();
    assert_eq!(
        prepared,
        lsp_types::PrepareRenameResponse::RangeWithPlaceholder {
            range: declaration,
            placeholder: "install".to_owned(),
        }
    );
    let edit = server.rename(doc.as_str(), 0, 15, "configure").unwrap();
    let changes = edit.changes.unwrap();
    let mut edits = changes[&doc].clone();
    edits.sort_by_key(|edit| edit.range.start);
    assert_eq!(
        edits,
        [
            TextEdit::new(declaration, "configure".to_owned()),
            TextEdit::new(
                Range::new(Position::new(2, 8), Position::new(2, 15)),
                "configure".to_owned(),
            ),
        ]
    );
}

#[test]
fn test_rename_heading_id_is_case_sensitive() {
    let mut server = TestLspServer::new();
    let content = r#"# Heading {#em}

A reference to [Heading](#em).

# Heading {#EM}

A reference to [Heading](#EM).
"#;
    server.open_document("file:///test.md", content, "markdown");

    let edit = server
        .rename("file:///test.md", 6, 27, "renamed")
        .expect("rename edit");
    let changes = edit.changes.expect("changes");
    let doc_uri: Uri = "file:///test.md".parse().unwrap();
    let edits = changes.get(&doc_uri).expect("doc edits");

    assert_eq!(edits.len(), 2);
    assert!(edits.iter().all(|e| e.new_text == "renamed"));
    assert!(edits.iter().any(|e| e.range.start.line == 4));
    assert!(edits.iter().any(|e| e.range.start.line == 6));
}

#[test]
fn test_rename_bookdown_section_crossref_with_hyphenated_slug() {
    let mut server = TestLspServer::new();
    let content = r#"# Heading

A ref to \@ref(heading).

## Heading 2

A ref to \@ref(heading-2).
"#;
    server.open_document("file:///test.Rmd", content, "rmarkdown");

    let edit = server
        .rename("file:///test.Rmd", 6, 16, "renamed-section")
        .expect("rename edit");
    let changes = edit.changes.expect("changes");
    let doc_uri: Uri = "file:///test.Rmd".parse().unwrap();
    let edits = changes.get(&doc_uri).expect("doc edits");

    assert!(
        edits.iter().any(|e| e.new_text == "renamed-section"),
        "expected rename edits to use new key"
    );
    assert!(
        edits.iter().any(|e| e.range.start.line == 6),
        "expected bookdown section crossref edit"
    );
    assert!(
        edits
            .iter()
            .any(|e| e.range.start.line == 4 && e.new_text == " {#renamed-section}"),
        "expected explicit heading id insertion for implicit heading slug"
    );
}

#[test]
fn test_rename_executable_chunk_label_updates_crossref_and_definition() {
    let mut server = TestLspServer::new();
    let content = r#"See @fig-my-label.

```{r fig-my-label}
plot(1, 1)
```
"#;
    server.open_document("file:///test.qmd", content, "quarto");

    let edit = server
        .rename("file:///test.qmd", 2, 10, "fig-renamed")
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
        edits.iter().any(|e| e.range.start.line == 2),
        "expected chunk label definition edit"
    );
}

#[test]
fn test_rename_hashpipe_label_with_hyphen_updates_from_value_cursor() {
    let mut server = TestLspServer::new();
    let content = r#"See @fig-my-other-label.

```{r}
#| label: fig-my-other-label
plot(1, 1)
```
"#;
    server.open_document("file:///test.qmd", content, "quarto");

    let edit = server
        .rename("file:///test.qmd", 3, 15, "fig-renamed")
        .expect("rename edit");
    let changes = edit.changes.expect("changes");
    let doc_uri: Uri = "file:///test.qmd".parse().unwrap();
    let edits = changes.get(&doc_uri).expect("doc edits");

    assert!(
        edits.iter().any(|e| e.range.start.line == 0),
        "expected crossref usage edit"
    );
    assert!(
        edits.iter().any(|e| e.range.start.line == 3),
        "expected chunk label declaration edit"
    );
}

#[test]
fn test_rename_chunk_option_label_with_hyphen_updates_from_value_cursor() {
    let mut server = TestLspServer::new();
    let content = r#"See @fig-my-other-label.

```{r, label = "fig-my-other-label"}
plot(1, 1)
```
"#;
    server.open_document("file:///test.qmd", content, "quarto");

    let edit = server
        .rename("file:///test.qmd", 2, 18, "fig-renamed")
        .expect("rename edit");
    let changes = edit.changes.expect("changes");
    let doc_uri: Uri = "file:///test.qmd".parse().unwrap();
    let edits = changes.get(&doc_uri).expect("doc edits");

    assert!(
        edits.iter().any(|e| e.range.start.line == 0),
        "expected crossref usage edit"
    );
    assert!(
        edits.iter().any(|e| e.range.start.line == 2),
        "expected chunk option declaration edit"
    );
}

#[test]
fn test_rename_returns_none_inside_yaml_frontmatter() {
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

    let mut server = TestLspServer::new();
    server.initialize(root_uri.as_str());
    server.open_document(
        doc_uri.as_str(),
        &fs::read_to_string(&doc_path).unwrap(),
        "quarto",
    );

    let edit = server.rename(doc_uri.as_str(), 1, 10, "renamed");
    assert!(
        edit.is_none(),
        "Expected no rename edits when cursor is inside YAML frontmatter"
    );
}

#[test]
fn test_rename_heading_reference_updates_shortcut_and_hash_links() {
    let mut server = TestLspServer::new();
    let content = "# Heading {#heading}\n\nA ref to [heading].\n\nA ref to [foobar](#heading).\n";
    server.open_document("file:///test.md", content, "markdown");

    let edit = server
        .rename("file:///test.md", 2, 11, "renamed-heading")
        .expect("rename edit");
    let changes = edit.changes.expect("changes");
    let doc_uri: Uri = "file:///test.md".parse().unwrap();
    let edits = changes.get(&doc_uri).expect("doc edits");

    assert!(
        edits.iter().any(|e| e.range.start.line == 0),
        "expected heading id declaration edit"
    );
    assert!(
        edits
            .iter()
            .any(|e| e.range.start.line == 2 && e.new_text == "renamed-heading"),
        "expected shortcut heading reference edit"
    );
    assert!(
        edits
            .iter()
            .any(|e| e.range.start.line == 4 && e.new_text == "renamed-heading"),
        "expected hash heading reference edit"
    );
}

#[test]
fn test_rename_footnote_updates_references_and_definition_id() {
    let mut server = TestLspServer::new();
    // Blank line after the footnote def is required so `Another[^1].` is a
    // sibling paragraph rather than a lazy-continuation line of the def body
    // (pandoc-native: `[^1]` inside a footnote-def body is literal text).
    let content =
        "Simple footnote[^1] in text.\n\n[^1]: This is a simple footnote.\n\nAnother[^1].\n";
    server.open_document("file:///test.md", content, "markdown");

    let edit = server
        .rename("file:///test.md", 0, 17, "note")
        .expect("rename edit");
    let changes = edit.changes.expect("changes");
    let doc_uri: Uri = "file:///test.md".parse().unwrap();
    let edits = changes.get(&doc_uri).expect("doc edits");

    assert_eq!(
        edits.iter().filter(|e| e.new_text == "note").count(),
        3,
        "expected two references plus one definition id edit"
    );
    assert!(edits.iter().any(|e| e.range.start.line == 0));
    assert!(edits.iter().any(|e| e.range.start.line == 2));
    assert!(edits.iter().any(|e| e.range.start.line == 4));
}

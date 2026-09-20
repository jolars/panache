//! Tests for hover previews and reference definitions.

use super::helpers::*;
use lsp_types::*;

#[test]
fn test_hover_on_included_footnote() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let child_path = temp_dir.path().join("_child.qmd");
    let parent_path = temp_dir.path().join("parent.qmd");

    std::fs::write(&child_path, "[^1]: Included footnote content.\n").unwrap();
    std::fs::write(&parent_path, "{{< include _child.qmd >}}\nRef[^1].\n").unwrap();

    let mut server = TestLspServer::new();
    let root_uri = Uri::from_file_path(temp_dir.path()).expect("root uri");
    let parent_uri = Uri::from_file_path(&parent_path).expect("parent uri");
    server.initialize(root_uri.as_str());
    server.open_document(
        parent_uri.as_str(),
        &std::fs::read_to_string(&parent_path).unwrap(),
        "quarto",
    );

    let hover = server.hover(parent_uri.as_str(), 1, 4);

    let Some(h) = hover else {
        panic!("Expected hover content");
    };
    if let HoverContents::Markup(markup) = h.contents {
        assert!(markup.value.contains("Included footnote content"));
    } else {
        panic!("Expected markup hover content");
    }
}

#[test]
fn test_hover_included_updates_after_watcher_change() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let child_path = temp_dir.path().join("_child.qmd");
    let parent_path = temp_dir.path().join("parent.qmd");

    std::fs::write(&child_path, "[^1]: Included footnote content.\n").unwrap();
    std::fs::write(&parent_path, "{{< include _child.qmd >}}\nRef[^1].\n").unwrap();

    let mut server = TestLspServer::new();
    let root_uri = Uri::from_file_path(temp_dir.path()).expect("root uri");
    let parent_uri = Uri::from_file_path(&parent_path).expect("parent uri");
    server.initialize(root_uri.as_str());
    server.open_document(
        parent_uri.as_str(),
        &std::fs::read_to_string(&parent_path).unwrap(),
        "quarto",
    );

    // First hover caches the included file and its definition index.
    assert!(
        server.hover(parent_uri.as_str(), 1, 4).is_some(),
        "Sanity check: should resolve hover before edit"
    );

    // Change the included file on disk so the footnote no longer exists.
    std::fs::write(&child_path, "[^2]: Included footnote content.\n").unwrap();
    server.did_change_watched_files(vec![FileEvent {
        uri: Uri::from_file_path(&child_path).expect("child uri"),
        typ: FileChangeType::CHANGED,
    }]);

    let hover = server.hover(parent_uri.as_str(), 1, 4);
    assert!(
        hover.is_none(),
        "After watcher update, hover should no longer resolve"
    );
}

#[test]
fn test_hover_loads_include_added_during_edit() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let child_path = temp_dir.path().join("_new.qmd");
    let parent_path = temp_dir.path().join("parent.qmd");

    std::fs::write(&child_path, "[^9]: New footnote content.\n").unwrap();
    // Parent references the footnote but does NOT include the child yet.
    std::fs::write(&parent_path, "Ref[^9].\n").unwrap();

    let mut server = TestLspServer::new();
    let root_uri = Uri::from_file_path(temp_dir.path()).expect("root uri");
    let parent_uri = Uri::from_file_path(&parent_path).expect("parent uri");
    server.initialize(root_uri.as_str());
    server.open_document(
        parent_uri.as_str(),
        &std::fs::read_to_string(&parent_path).unwrap(),
        "quarto",
    );

    // Not referenced yet: the writer never loaded the child, and `file_text` no
    // longer lazy-loads on the read path, so hover cannot resolve.
    assert_eq!(server.get_cached_file_text(&child_path), None);
    assert!(server.hover(parent_uri.as_str(), 0, 4).is_none());

    // Add the include. The newly-referenced file must be loaded on the writer
    // during the debounced pass that `pump` drives.
    server.edit_document(
        parent_uri.as_str(),
        vec![full_document_change("{{< include _new.qmd >}}\nRef[^9].\n")],
    );
    server.pump(std::time::Duration::from_millis(500));

    assert!(
        server.get_cached_file_text(&child_path).is_some(),
        "an include added during editing should be loaded on the writer"
    );

    let hover = server.hover(parent_uri.as_str(), 1, 4);
    let Some(h) = hover else {
        panic!("Expected hover to resolve the included footnote after the edit");
    };
    if let HoverContents::Markup(markup) = h.contents {
        assert!(markup.value.contains("New footnote content"));
    } else {
        panic!("Expected markup hover content");
    }
}

#[test]
fn test_hover_on_footnote_reference() {
    let mut server = TestLspServer::new();

    // Open a document with footnote
    let content = r#"Text with footnote[^1] here.

[^1]: This is the footnote content with details.
"#;
    server.open_document("file:///test.md", content, "markdown");

    // Request hover on the footnote reference [^1]
    let hover = server.hover(
        "file:///test.md",
        0,  // Line with footnote[^1]
        20, // Character position inside [^1]
    );

    assert!(hover.is_some(), "Should have hover info for footnote");

    if let Some(h) = hover {
        // Check that it contains the footnote content
        if let HoverContents::Markup(markup) = h.contents {
            assert_eq!(markup.kind, MarkupKind::Markdown);
            assert!(
                markup.value.contains("footnote content"),
                "Should show footnote content"
            );
        } else {
            panic!("Expected markup hover content");
        }
    }
}

#[test]
fn test_hover_on_plain_text() {
    let mut server = TestLspServer::new();

    // Open a document without any special elements
    let content = "Just plain text without footnotes.";
    server.open_document("file:///test.md", content, "markdown");

    // Request hover in plain text
    let hover = server.hover("file:///test.md", 0, 10);

    assert!(hover.is_none(), "Should not have hover for plain text");
}

#[test]
fn test_hover_on_footnote_with_formatting() {
    let mut server = TestLspServer::new();

    // Open a document with formatted footnote
    let content = r#"Reference[^note] in text.

[^note]: Footnote with *emphasis* and `code`.
"#;
    server.open_document("file:///test.md", content, "markdown");

    // Request hover on footnote
    let hover = server.hover(
        "file:///test.md",
        0,  // Line with [^note]
        10, // Inside [^note]
    );

    assert!(hover.is_some(), "Should have hover for formatted footnote");

    if let Some(h) = hover
        && let HoverContents::Markup(markup) = h.contents
    {
        let content = markup.value;
        assert!(content.contains("*emphasis*"));
        assert!(content.contains("`code`"));
    }
}

#[test]
fn test_hover_on_citation_preview() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let root = temp_dir.path();
    let bib_path = root.join("refs.bib");
    let doc_path = root.join("doc.qmd");

    std::fs::write(
        &bib_path,
        "@article{citekey,\n  author = {Doe, Jane},\n  year = {2020},\n  title = {Sample Title},\n  journal = {Journal Name},\n  volume = {12},\n  number = {3},\n  pages = {45-67}\n}\n",
    )
    .unwrap();

    std::fs::write(
        &doc_path,
        "---\nbibliography: refs.bib\n---\n\nSee [@citekey].\n",
    )
    .unwrap();

    let mut server = TestLspServer::new();
    let root_uri = Uri::from_file_path(root).expect("root uri");
    let doc_uri = Uri::from_file_path(&doc_path).expect("doc uri");
    server.initialize(root_uri.as_str());
    server.open_document(
        doc_uri.as_str(),
        &std::fs::read_to_string(&doc_path).unwrap(),
        "quarto",
    );

    let result = server.hover(doc_uri.as_str(), 4, 7);
    let Some(Hover { contents, .. }) = result else {
        panic!("Expected hover content");
    };
    let content = match contents {
        HoverContents::Markup(markup) => markup.value,
        HoverContents::Scalar(scalar) => match scalar {
            MarkedString::String(text) => text,
            MarkedString::LanguageString(lang) => lang.value,
        },
        HoverContents::Array(array) => array
            .iter()
            .map(|item| match item {
                MarkedString::String(text) => text.clone(),
                MarkedString::LanguageString(lang) => lang.value.clone(),
            })
            .collect::<Vec<_>>()
            .join("\n"),
    };
    assert!(content.contains("Doe"));
    assert!(content.contains("2020"));
    assert!(content.contains("Sample Title"));
    assert!(content.contains("Journal Name"));
}

#[test]
fn test_hover_on_undefined_footnote() {
    let mut server = TestLspServer::new();

    // Open a document with footnote reference but no definition
    let content = "Text with undefined[^missing] footnote.";
    server.open_document("file:///test.md", content, "markdown");

    // Request hover on undefined footnote
    let hover = server.hover(
        "file:///test.md",
        0,
        25, // Inside [^missing]
    );

    // Should return None when footnote definition doesn't exist
    assert!(
        hover.is_none(),
        "Should not have hover for undefined footnote"
    );
}

#[test]
fn test_hover_returns_none_inside_yaml_frontmatter() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let root = temp_dir.path();
    let bib_path = root.join("refs.bib");
    let doc_path = root.join("doc.qmd");

    std::fs::write(&bib_path, "@article{known,\n  title = {Known}\n}\n").unwrap();
    std::fs::write(
        &doc_path,
        "---\ntitle: \"@known\"\nbibliography: refs.bib\n---\n\nSee [@known].\n",
    )
    .unwrap();

    let mut server = TestLspServer::new();
    let root_uri = Uri::from_file_path(root).expect("root uri");
    let doc_uri = Uri::from_file_path(&doc_path).expect("doc uri");
    server.initialize(root_uri.as_str());
    server.open_document(
        doc_uri.as_str(),
        &std::fs::read_to_string(&doc_path).unwrap(),
        "quarto",
    );

    let hover = server.hover(doc_uri.as_str(), 1, 10);
    assert!(
        hover.is_none(),
        "Expected no hover when cursor is inside YAML frontmatter"
    );
}

#[test]
fn test_hover_on_heading_reference_shows_section_preview() {
    let mut server = TestLspServer::new();
    let content = "# Intro {#intro}\n\nFirst paragraph in intro section.\n\n## Next\n\nTail.\n\nSee [go](#intro).\n";
    server.open_document("file:///test.md", content, "markdown");

    let hover = server.hover(
        "file:///test.md",
        8,  // See [go](#intro).
        10, // Inside intro anchor text
    );

    let Some(h) = hover else {
        panic!("Expected hover content for heading reference");
    };
    let content = match h.contents {
        HoverContents::Markup(markup) => markup.value,
        _ => panic!("Expected markdown hover content"),
    };
    assert!(content.contains("Section"));
    assert!(content.contains("Intro"));
    assert!(content.contains("First paragraph in intro section."));
}

#[test]
fn test_hover_on_heading_declaration_returns_none() {
    let mut server = TestLspServer::new();
    let content = "# Intro {#intro}\n\nBody.\n\nSee [go](#intro).\n";
    server.open_document("file:///test.md", content, "markdown");

    let hover = server.hover("file:///test.md", 0, 3);
    assert!(
        hover.is_none(),
        "Heading declaration should not produce section preview hover"
    );
}

#[test]
fn test_hover_on_heading_reference_with_empty_section_shows_title_only() {
    let mut server = TestLspServer::new();
    let content = "# Intro {#intro}\n\n## Next\n\nSee [go](#intro).\n";
    server.open_document("file:///test.md", content, "markdown");

    let hover = server.hover(
        "file:///test.md",
        4, // See [go](#intro).
        10,
    );

    let Some(h) = hover else {
        panic!("Expected hover content for heading reference");
    };
    let content = match h.contents {
        HoverContents::Markup(markup) => markup.value,
        _ => panic!("Expected markdown hover content"),
    };
    assert!(content.contains("Section"));
    assert!(content.contains("Intro"));
    assert!(!content.contains("..."));
}

#[test]
fn test_hover_on_heading_reference_crops_preview() {
    let mut server = TestLspServer::new();
    let long_body = "Lorem ipsum dolor sit amet, consectetur adipiscing elit. ".repeat(8);
    let content = format!(
        "# Intro {{#intro}}\n\n{}\n\n## Next\n\nSee [go](#intro).\n",
        long_body
    );
    server.open_document("file:///test.md", &content, "markdown");

    let hover = server.hover("file:///test.md", 6, 10);
    let Some(h) = hover else {
        panic!("Expected hover content for heading reference");
    };
    let content = match h.contents {
        HoverContents::Markup(markup) => markup.value,
        _ => panic!("Expected markdown hover content"),
    };
    assert!(
        content.ends_with("..."),
        "Expected cropped preview to end with ellipsis"
    );
}

#[test]
fn test_hover_on_reference_link_definition_to_heading_shows_section_preview() {
    let mut server = TestLspServer::new();
    let content = "# Intro {#bar}\n\nSection body here.\n\nSee [foo][myref].\n\n[myref]: #bar\n";
    server.open_document("file:///test.md", content, "markdown");

    let hover = server.hover(
        "file:///test.md",
        4,  // See [foo][myref].
        11, // inside [myref]
    );

    let Some(h) = hover else {
        panic!("Expected hover content for reference-style heading link");
    };
    let content = match h.contents {
        HoverContents::Markup(markup) => markup.value,
        _ => panic!("Expected markdown hover content"),
    };
    assert!(content.contains("Section"));
    assert!(content.contains("Intro"));
    assert!(content.contains("Section body here."));
}

#[test]
fn test_hover_on_reference_link_to_external_destination_shows_definition() {
    let mut server = TestLspServer::new();
    let content = "# Intro {#bar}\n\nSection body here.\n\nSee [foo][myref].\n\n[myref]: https://example.com\n";
    server.open_document("file:///test.md", content, "markdown");

    assert_eq!(
        markdown_hover(server.hover("file:///test.md", 4, 11)),
        "```markdown\n[myref]: https://example.com\n```"
    );
}

fn markdown_hover(hover: Option<Hover>) -> String {
    let hover = hover.expect("Expected hover content");
    assert_eq!(hover.range, None);
    let HoverContents::Markup(markup) = hover.contents else {
        panic!("Expected markup hover content");
    };
    assert_eq!(markup.kind, MarkupKind::Markdown);
    markup.value
}

#[test]
fn test_hover_reference_forms() {
    for flavor in ["commonmark", "pandoc", "quarto"] {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("panache.toml"),
            format!("flavor = \"{flavor}\"\n"),
        )
        .unwrap();
        let mut server = TestLspServer::new();
        server.initialize(Uri::from_file_path(dir.path()).unwrap().as_str());

        for (i, usage) in [
            "[text][ref]",
            "[ref][]",
            "[ref]",
            "![alt][ref]",
            "![ref][]",
            "![ref]",
        ]
        .iter()
        .enumerate()
        {
            let uri = Uri::from_file_path(dir.path().join(format!("doc{i}.md"))).unwrap();
            let line = format!("😀 See {usage}.");
            let content = format!("{line}\n\n[ref]: https://example.com \"Title\"\n");
            server.open_document(uri.as_str(), &content, "markdown");
            let offset = line.find("ref").unwrap() + 1;
            let column = line[..offset].encode_utf16().count() as u32;
            assert_eq!(
                markdown_hover(server.hover(uri.as_str(), 0, column)),
                "```markdown\n[ref]: https://example.com \"Title\"\n```",
                "{flavor}: {usage}"
            );
        }
    }
}

#[test]
fn test_hover_reference_label_normalization_preserves_inline_markup() {
    for (usage, definition) in [
        ("[text][MY  ref]", "[my ref]: https://example.com"),
        ("[`code`][]", "[`code`]: https://example.com"),
        ("[*bold*]", "[*bold*]: https://example.com"),
        ("![`code`][]", "[`code`]: https://example.com"),
        ("![*bold*]", "[*bold*]: https://example.com"),
    ] {
        let mut server = TestLspServer::new();
        server.open_document(
            "file:///test.md",
            &format!("{usage}\n\n{definition}\n"),
            "markdown",
        );
        assert_eq!(
            markdown_hover(server.hover("file:///test.md", 0, 3)),
            format!("```markdown\n{definition}\n```"),
            "{usage}"
        );
    }
}

#[test]
fn test_hover_reference_preserves_definition_source() {
    for definition in [
        "[Ref]: <https://example.com/a%20b> 'A title'".to_string(),
        "[Ref]:\n  https://example.com\n  \"A title\n  on two lines\"".to_string(),
        format!("[Ref]: https://example.com/{}", "long-path/".repeat(30)),
    ] {
        let mut server = TestLspServer::new();
        server.open_document(
            "file:///test.md",
            &format!("[text][ref]\n\n{definition}\n"),
            "markdown",
        );
        assert_eq!(
            markdown_hover(server.hover("file:///test.md", 0, 8)),
            format!("```markdown\n{definition}\n```")
        );
    }
}

#[test]
fn test_hover_reference_fence_contains_backticks_in_title() {
    let mut server = TestLspServer::new();
    let definition = "[ref]: https://example.com \"A title\n```\nwith a fence\"";
    server.open_document(
        "file:///test.md",
        &format!("[ref]\n\n{definition}\n"),
        "markdown",
    );
    assert_eq!(
        markdown_hover(server.hover("file:///test.md", 0, 2)),
        format!("````markdown\n{definition}\n````")
    );
}

#[test]
fn test_hover_shortcut_definition_takes_precedence_over_heading() {
    let mut server = TestLspServer::new();
    server.open_document(
        "file:///test.md",
        "# ref\n\nHeading body.\n\n[ref]\n\n[ref]: https://example.com\n",
        "markdown",
    );
    assert_eq!(
        markdown_hover(server.hover("file:///test.md", 4, 2)),
        "```markdown\n[ref]: https://example.com\n```"
    );
}

#[test]
fn test_hover_reference_forms_to_heading_keep_section_preview() {
    for usage in [
        "[text][ref]",
        "[ref][]",
        "[ref]",
        "![alt][ref]",
        "![ref][]",
        "![ref]",
    ] {
        let mut server = TestLspServer::new();
        let content = format!("{usage}\n\n[ref]: #intro\n\n# Intro\n\nSection body.\n");
        server.open_document("file:///test.md", &content, "markdown");
        let hover = markdown_hover(server.hover("file:///test.md", 0, 3));
        assert!(hover.contains("**Section:** Intro"), "{usage}: {hover}");
        assert!(hover.contains("Section body."));
    }
}

#[test]
fn test_hover_reference_falls_back_when_destination_has_no_preview() {
    for destination in [
        "./does-not-exist.md",
        "./image.svg",
        "#missing",
        "mailto:me@example.com",
    ] {
        let mut server = TestLspServer::new();
        let definition = format!("[ref]: {destination}");
        server.open_document(
            "file:///test.md",
            &format!("[ref]\n\n{definition}\n"),
            "markdown",
        );
        assert_eq!(
            markdown_hover(server.hover("file:///test.md", 0, 2)),
            format!("```markdown\n{definition}\n```")
        );
    }
}

#[test]
fn test_hover_reference_uses_first_definition() {
    let mut server = TestLspServer::new();
    server.open_document(
        "file:///test.md",
        "[text][ref]\n\n[ref]: https://first.example\n[ref]: #intro\n\n# Intro\n\nBody.\n",
        "markdown",
    );
    assert_eq!(
        markdown_hover(server.hover("file:///test.md", 0, 8)),
        "```markdown\n[ref]: https://first.example\n```"
    );
}

#[test]
fn test_hover_reference_uses_project_definition_order() {
    let dir = tempfile::TempDir::new().unwrap();
    let child_path = dir.path().join("_refs.qmd");
    let parent_path = dir.path().join("parent.qmd");
    let content = "{{< include _refs.qmd >}}\n[text][ref]\n\n[ref]: https://parent.example\n";
    std::fs::write(&child_path, "[ref]: https://child.example\n").unwrap();
    std::fs::write(&parent_path, content).unwrap();
    let uri = Uri::from_file_path(&parent_path).unwrap();
    let mut server = TestLspServer::new();
    server.initialize(Uri::from_file_path(dir.path()).unwrap().as_str());
    server.open_document(uri.as_str(), content, "quarto");
    assert_eq!(
        markdown_hover(server.hover(uri.as_str(), 1, 8)),
        "```markdown\n[ref]: https://child.example\n```"
    );
}

#[test]
fn test_hover_reference_preserves_multimarkdown_attributes() {
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::write(
        dir.path().join("panache.toml"),
        "flavor = \"multimarkdown\"\n",
    )
    .unwrap();
    let uri = Uri::from_file_path(dir.path().join("doc.md")).unwrap();
    let definition = "[ref]: image.png \"Title\" width=20px\n    height=30px";
    let mut server = TestLspServer::new();
    server.initialize(Uri::from_file_path(dir.path()).unwrap().as_str());
    server.open_document(
        uri.as_str(),
        &format!("![ref]\n\n{definition}\n"),
        "markdown",
    );
    assert_eq!(
        markdown_hover(server.hover(uri.as_str(), 0, 3)),
        format!("```markdown\n{definition}\n```")
    );
}

#[test]
fn test_hover_footnote_inside_reference_link_keeps_footnote_preview() {
    let content = "[text[^1]][ref]\n\n[ref]: https://example.com\n\n[^1]: Footnote content.\n";
    let mut server = TestLspServer::new();
    server.open_document("file:///test.md", content, "markdown");
    assert_eq!(
        markdown_hover(server.hover("file:///test.md", 0, 7)),
        "Footnote content."
    );
}

#[test]
fn test_hover_reference_tracks_unsaved_definition_edits() {
    let mut server = TestLspServer::new();
    server.open_document(
        "file:///test.md",
        "[ref]\n\n[ref]: https://old.example\n",
        "markdown",
    );
    assert!(markdown_hover(server.hover("file:///test.md", 0, 2)).contains("https://old.example"));
    server.edit_document(
        "file:///test.md",
        vec![full_document_change(
            "[ref]\n\n[ref]: https://new.example\n",
        )],
    );
    assert_eq!(
        markdown_hover(server.hover("file:///test.md", 0, 2)),
        "```markdown\n[ref]: https://new.example\n```"
    );
}

#[test]
fn test_hover_included_reference_forms_track_watcher_updates() {
    let dir = tempfile::TempDir::new().unwrap();
    let child_path = dir.path().join("_refs.qmd");
    let parent_path = dir.path().join("parent.qmd");
    let content =
        "{{< include _refs.qmd >}}\n[text][ref]\n[ref][]\n[ref]\n![alt][ref]\n![ref][]\n![ref]\n";
    std::fs::write(&child_path, "[ref]: https://old.example\n").unwrap();
    std::fs::write(&parent_path, content).unwrap();
    let parent_uri = Uri::from_file_path(&parent_path).unwrap();
    let child_uri = Uri::from_file_path(&child_path).unwrap();
    let mut server = TestLspServer::new();
    server.initialize(Uri::from_file_path(dir.path()).unwrap().as_str());
    server.open_document(parent_uri.as_str(), content, "quarto");

    for (line, usage) in content.lines().enumerate().skip(1) {
        let column = usage.rfind("ref").unwrap() as u32 + 1;
        assert_eq!(
            markdown_hover(server.hover(parent_uri.as_str(), line as u32, column)),
            "```markdown\n[ref]: https://old.example\n```",
            "{usage}"
        );
    }
    std::fs::write(&child_path, "[ref]: https://new.example\n").unwrap();
    server.did_change_watched_files(vec![FileEvent {
        uri: child_uri.clone(),
        typ: FileChangeType::CHANGED,
    }]);
    assert_eq!(
        markdown_hover(server.hover(parent_uri.as_str(), 1, 8)),
        "```markdown\n[ref]: https://new.example\n```"
    );
    std::fs::write(&child_path, "").unwrap();
    server.did_change_watched_files(vec![FileEvent {
        uri: child_uri,
        typ: FileChangeType::CHANGED,
    }]);
    assert!(server.hover(parent_uri.as_str(), 1, 8).is_none());
}

#[test]
fn test_hover_reference_source_requires_a_resolved_usage() {
    for (content, column) in [
        ("[text][missing]\n", 9),
        ("[missing][]\n", 3),
        ("[missing]\n", 3),
        ("![alt][missing]\n", 9),
        ("![missing][]\n", 3),
        ("![missing]\n", 3),
        ("`[ref]`\n\n[ref]: https://example.com\n", 3),
        ("[ref]: https://example.com\n", 2),
    ] {
        let mut server = TestLspServer::new();
        server.open_document("file:///test.md", content, "markdown");
        assert!(
            server.hover("file:///test.md", 0, column).is_none(),
            "{content}"
        );
    }
}

#[test]
fn test_hover_on_equation_reference_shows_equation_preview() {
    let mut server = TestLspServer::new();
    let content = "$$\na = b + c\n$$ {#eq-foo}\n\n@eq-foo\n";
    server.open_document("file:///test.qmd", content, "quarto");

    let hover = server.hover("file:///test.qmd", 4, 4);
    let Some(h) = hover else {
        panic!("Expected hover content for equation reference");
    };
    let content = match h.contents {
        HoverContents::Markup(markup) => markup.value,
        _ => panic!("Expected markdown hover content"),
    };
    assert!(content.contains("Equation"));
    assert!(content.contains("eq-foo"));
    assert!(content.contains("```tex"));
    assert!(content.contains("a = b + c"));
}

#[test]
fn test_hover_on_equation_reference_crops_preview_lines() {
    let mut server = TestLspServer::new();
    let content =
        "$$\nline1\nline2\nline3\nline4\nline5\nline6\nline7\n$$ {#eq-long}\n\n@eq-long\n";
    server.open_document("file:///test.qmd", content, "quarto");

    let hover = server.hover("file:///test.qmd", 10, 4);
    let Some(h) = hover else {
        panic!("Expected hover content for equation reference");
    };
    let content = match h.contents {
        HoverContents::Markup(markup) => markup.value,
        _ => panic!("Expected markdown hover content"),
    };
    assert!(content.contains("line6"));
    assert!(!content.contains("line7"));
    assert!(content.contains("\n...\n```"));
}

#[test]
fn test_hover_on_direct_local_markdown_link_shows_linked_doc_preview() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let doc_path = temp_dir.path().join("doc.qmd");
    let linked_path = temp_dir.path().join("linked.md");

    std::fs::write(
        &linked_path,
        "# Linked title\n\nLinked paragraph preview text.\n",
    )
    .unwrap();
    std::fs::write(&doc_path, "See [linked](./linked.md).\n").unwrap();

    let mut server = TestLspServer::new();
    let root_uri = Uri::from_file_path(temp_dir.path()).expect("root uri");
    let doc_uri = Uri::from_file_path(&doc_path).expect("doc uri");
    server.initialize(root_uri.as_str());
    server.open_document(
        doc_uri.as_str(),
        &std::fs::read_to_string(&doc_path).unwrap(),
        "quarto",
    );

    let hover = server.hover(doc_uri.as_str(), 0, 17);
    let Some(h) = hover else {
        panic!("Expected hover content for direct local markdown link");
    };
    let content = match h.contents {
        HoverContents::Markup(markup) => markup.value,
        _ => panic!("Expected markdown hover content"),
    };
    assert!(content.contains("Linked document"));
    assert!(content.contains("linked.md"));
    assert!(content.contains("Linked title"));
    assert!(content.contains("Linked paragraph preview text."));
}

#[test]
fn test_hover_on_reference_forms_to_local_markdown_shows_linked_doc_preview() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let doc_path = temp_dir.path().join("doc.qmd");
    let linked_path = temp_dir.path().join("linked.qmd");

    std::fs::write(
        &linked_path,
        "# Ref linked title\n\nRef linked paragraph preview text.\n",
    )
    .unwrap();
    std::fs::write(
        &doc_path,
        "See [linked][ref].\nSee [ref][].\nSee [ref].\n\n[ref]: <./linked.qmd>\n",
    )
    .unwrap();

    let mut server = TestLspServer::new();
    let root_uri = Uri::from_file_path(temp_dir.path()).expect("root uri");
    let doc_uri = Uri::from_file_path(&doc_path).expect("doc uri");
    server.initialize(root_uri.as_str());
    server.open_document(
        doc_uri.as_str(),
        &std::fs::read_to_string(&doc_path).unwrap(),
        "quarto",
    );

    for (line, column) in [(0, 14), (1, 6), (2, 6)] {
        let content = markdown_hover(server.hover(doc_uri.as_str(), line, column));
        assert!(content.contains("Linked document"));
        assert!(content.contains("linked.qmd"));
        assert!(content.contains("Ref linked title"));
        assert!(content.contains("Ref linked paragraph preview text."));
    }
}

#[test]
fn test_hover_on_missing_local_markdown_link_returns_none() {
    let mut server = TestLspServer::new();
    let content = "See [missing](./does-not-exist.md).\n";
    server.open_document("file:///test.md", content, "markdown");

    let hover = server.hover("file:///test.md", 0, 20);
    assert!(
        hover.is_none(),
        "Missing local linked document should not produce hover preview"
    );
}

#[test]
fn test_hover_included_reference_uses_definition_directory() {
    let dir = tempfile::TempDir::new().unwrap();
    let parts = dir.path().join("parts");
    std::fs::create_dir(&parts).unwrap();
    std::fs::write(parts.join("_refs.qmd"), "[ref]: <./linked.md>\n").unwrap();
    std::fs::write(
        parts.join("linked.md"),
        "# Correct\n\nThe linked document.\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("linked.md"), "# Wrong directory\n").unwrap();
    let content = "{{< include parts/_refs.qmd >}}\n[ref]\n";
    let path = dir.path().join("doc.qmd");
    std::fs::write(&path, content).unwrap();
    let uri = Uri::from_file_path(&path).unwrap();
    let mut server = TestLspServer::new();
    server.initialize(Uri::from_file_path(dir.path()).unwrap().as_str());
    server.open_document(uri.as_str(), content, "quarto");

    let hover = markdown_hover(server.hover(uri.as_str(), 1, 2));
    assert!(hover.contains("**Title:** Correct"));
    assert!(hover.contains("The linked document."));
    assert!(!hover.contains("Wrong directory"));
}

#[test]
fn test_hover_on_external_url_link_returns_none_for_linked_doc_preview() {
    let mut server = TestLspServer::new();
    let content = "See [site](https://example.com).\n";
    server.open_document("file:///test.md", content, "markdown");

    let hover = server.hover("file:///test.md", 0, 13);
    assert!(
        hover.is_none(),
        "External URL links should not produce linked-document previews"
    );
}

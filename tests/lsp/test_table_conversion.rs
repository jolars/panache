use super::helpers::*;
use lsp_types::*;
use panache::syntax::{SyntaxKind, Table};

fn actions(server: &TestLspServer, uri: &str, line: u32, character: u32) -> Vec<CodeAction> {
    server
        .get_code_actions(uri, line, character, line, character)
        .unwrap()
        .into_iter()
        .filter_map(|action| match action {
            CodeActionOrCommand::CodeAction(action)
                if action.title == "Convert to simple table"
                    || action.title == "Convert to pipe table"
                    || action.title == "Convert to multiline table"
                    || action.title == "Convert to grid table" =>
            {
                Some(action)
            }
            _ => None,
        })
        .collect()
}

fn byte_offset(text: &str, position: Position) -> usize {
    let start: usize = text
        .split_inclusive('\n')
        .take(position.line as usize)
        .map(str::len)
        .sum();
    let mut utf16 = 0;
    for (offset, ch) in text[start..].char_indices() {
        if utf16 == position.character {
            return start + offset;
        }
        utf16 += ch.len_utf16() as u32;
    }
    text.len()
}

fn apply(text: &str, action: &CodeAction) -> String {
    let edits = action
        .edit
        .as_ref()
        .unwrap()
        .changes
        .as_ref()
        .unwrap()
        .values()
        .next()
        .unwrap();
    assert_eq!(edits.len(), 1);
    let edit = &edits[0];
    let mut result = text.to_string();
    result.replace_range(
        byte_offset(text, edit.range.start)..byte_offset(text, edit.range.end),
        &edit.new_text,
    );
    result
}

#[test]
fn offers_three_refactors_and_preserves_surroundings() {
    let mut server = TestLspServer::new();
    let text = "Before 😀.\n\n| A | B |\n|---|---|\n| one | two |\n\nAfter.\n";
    server.open_document("file:///table.qmd", text, "quarto");
    let actions = actions(&server, "file:///table.qmd", 4, 4);
    assert_eq!(actions.len(), 3);
    for action in actions {
        assert_eq!(
            action.kind.as_ref(),
            Some(&CodeActionKind::REFACTOR_REWRITE)
        );
        let result = apply(text, &action);
        assert!(result.starts_with("Before 😀.\n\n"));
        assert!(result.ends_with("\nAfter.\n"));
        let tree = panache::parse(&result, None);
        let table = tree.descendants().find_map(Table::cast).unwrap();
        assert_eq!(
            table.syntax().kind(),
            if action.title.contains("multiline") {
                SyntaxKind::MULTILINE_TABLE
            } else if action.title.contains("grid") {
                SyntaxKind::GRID_TABLE
            } else {
                SyntaxKind::SIMPLE_TABLE
            }
        );
    }
}

#[test]
fn preserves_nested_prefixes_and_crlf() {
    let mut server = TestLspServer::new();
    let text = "- > | A | B |\r\n  > |---|---|\r\n  > | 😀 | 界 |\r\n\nAfter.\n";
    server.open_document("file:///table.qmd", text, "quarto");
    let actions = actions(&server, "file:///table.qmd", 2, 8);
    assert_eq!(actions.len(), 3);
    for action in actions {
        let result = apply(text, &action);
        assert!(result.starts_with("- > "));
        assert!(result.contains("\r\n  > "));
        assert!(result.ends_with("\nAfter.\n"));
        let tree = panache::parse(&result, None);
        let table = tree.descendants().find_map(Table::cast).unwrap();
        assert!(
            table
                .syntax()
                .ancestors()
                .any(|node| node.kind() == SyntaxKind::LIST_ITEM)
        );
        assert!(
            table
                .syntax()
                .ancestors()
                .any(|node| node.kind() == SyntaxKind::BLOCK_QUOTE)
        );
    }
}

#[test]
fn omits_unsupported_actions_and_current_style() {
    let mut server = TestLspServer::new();
    for (text, expected) in [
        ("| A | B |\n|---|---|\n| x |\n", 0),
        ("A     B\n----- -----\none   two\n", 3),
    ] {
        server.open_document("file:///table.qmd", text, "quarto");
        assert_eq!(actions(&server, "file:///table.qmd", 0, 1).len(), expected);
        server.close_document("file:///table.qmd");
    }
}

#[test]
fn converts_simple_and_multiline_tables_to_pipe() {
    for source in [
        "A     B\n----- -----\none   two\n",
        "-------------\nA      B\n------ ------\none    two\nthree\n\n-------------\n",
        "----- -----\none   two\n----- -----\n",
    ] {
        let text = format!("Before 😀.\n\n{source}\n: Caption {{#tbl-id}}\n\nAfter.\n");
        let mut server = TestLspServer::new();
        server.open_document("file:///table.qmd", &text, "quarto");
        let line = text
            .lines()
            .position(|line| line.starts_with(": Caption"))
            .unwrap();
        let actions = actions(&server, "file:///table.qmd", line as u32, 3);
        assert_eq!(actions.len(), 3);
        let action = actions
            .iter()
            .find(|action| action.title == "Convert to pipe table")
            .unwrap();
        assert_eq!(action.kind, Some(CodeActionKind::REFACTOR_REWRITE));
        let result = apply(&text, action);
        assert!(result.starts_with("Before 😀.\n\n|"));
        assert!(result.ends_with("\n: Caption {#tbl-id}\n\nAfter.\n"));
        assert!(result.contains("two"));
        if source.contains("three") {
            assert!(result.contains("one three"));
        }
        let tree = panache::parse(&result, None);
        assert_eq!(
            tree.descendants()
                .find_map(Table::cast)
                .unwrap()
                .syntax()
                .kind(),
            SyntaxKind::PIPE_TABLE
        );
    }
}

#[test]
fn pipe_conversion_preserves_nested_prefixes_crlf_and_final_newline() {
    let source = "A     B\n----- -----\n😀    界\n";
    let nested = source
        .lines()
        .map(|line| format!("  > {line}\r\n"))
        .collect::<String>();
    for text in [
        format!("- Item.\r\n\r\n{nested}\r\n- Sibling.\r\n"),
        source.trim_end().to_string(),
        format!(": Caption 😀 {{#tbl-id}}\n\n{source}"),
    ] {
        let mut server = TestLspServer::new();
        server.open_document("file:///table.qmd", &text, "quarto");
        let (line, content) = text
            .lines()
            .enumerate()
            .find(|(_, line)| line.contains("界"))
            .unwrap();
        let col = content[..content.find("界").unwrap()]
            .encode_utf16()
            .count();
        let action = actions(&server, "file:///table.qmd", line as u32, col as u32)
            .into_iter()
            .find(|action| action.title == "Convert to pipe table")
            .unwrap();
        let result = apply(&text, &action);
        assert_eq!(text.ends_with('\n'), result.ends_with('\n'));
        assert!(result.contains("😀"));
        assert!(result.contains("界"));
        if text.contains("\r\n") {
            assert!(result.contains("\r\n  > |"));
            assert!(result.ends_with("\r\n- Sibling.\r\n"));
            assert!(!result.replace("\r\n", "").contains('\n'));
        }
        if text.starts_with(": Caption") {
            assert!(result.starts_with(": Caption 😀 {#tbl-id}\n\n|"));
        }
    }
}

#[test]
fn pipe_conversion_reports_disabled_extension() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("panache.toml"),
        "flavor = \"pandoc\"\n[extensions]\npipe-tables = false\n",
    )
    .unwrap();
    let uri = Uri::from_file_path(dir.path().join("table.md")).unwrap();
    for disabled_support in [false, true] {
        let mut server = TestLspServer::new();
        if disabled_support {
            server.initialize_disabled_code_actions(
                Uri::from_file_path(dir.path()).unwrap().as_str(),
            );
        } else {
            server.initialize(Uri::from_file_path(dir.path()).unwrap().as_str());
        }
        server.open_document(
            uri.as_str(),
            "A     B\n----- -----\none   two\n",
            "markdown",
        );
        let action = actions(&server, uri.as_str(), 0, 1)
            .into_iter()
            .find(|action| action.title == "Convert to pipe table");
        if disabled_support {
            let action = action.unwrap();
            assert!(action.edit.is_none());
            assert!(
                action
                    .disabled
                    .unwrap()
                    .reason
                    .contains("extension is disabled")
            );
        } else {
            assert!(action.is_none());
        }
    }
}

#[test]
fn no_actions_for_selection_spanning_tables_or_cursor_outside() {
    let mut server = TestLspServer::new();
    let text = "| A | B |\n|---|---|\n| one | two |\n\n| C | D |\n|---|---|\n| three | four |\n";
    server.open_document("file:///table.qmd", text, "quarto");
    let response = server
        .get_code_actions("file:///table.qmd", 0, 0, 6, 0)
        .unwrap();
    assert!(response.iter().all(|action| !matches!(action, CodeActionOrCommand::CodeAction(action) if action.title.contains("table"))));
    assert!(actions(&server, "file:///table.qmd", 3, 0).is_empty());
}

#[test]
fn reports_unsupported_reasons_only_to_capable_clients() {
    let text = "| A | B |\n|---|---|\n| x |\n";
    let mut server = TestLspServer::new();
    server.initialize_disabled_code_actions("file:///workspace");
    server.open_document("file:///table.qmd", text, "quarto");
    let actions = actions(&server, "file:///table.qmd", 2, 1);
    assert_eq!(actions.len(), 3);
    for action in actions {
        assert!(action.edit.is_none());
        assert!(
            action
                .disabled
                .unwrap()
                .reason
                .contains("number of columns")
        );
    }
}

#[test]
fn reports_disabled_extensions_in_gfm() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("panache.toml"), "flavor = \"gfm\"\n").unwrap();
    let uri = Uri::from_file_path(dir.path().join("table.md")).unwrap();
    let mut server = TestLspServer::new();
    server.initialize_disabled_code_actions(Uri::from_file_path(dir.path()).unwrap().as_str());
    server.open_document(
        uri.as_str(),
        "| A | B |\n|---|---|\n| x | y |\n",
        "markdown",
    );
    let actions = actions(&server, uri.as_str(), 0, 2);
    assert_eq!(actions.len(), 3);
    for action in actions {
        assert!(action.edit.is_none());
        assert!(
            action
                .disabled
                .unwrap()
                .reason
                .contains("extension is disabled")
        );
    }
}

#[test]
fn converts_at_caption_and_document_boundaries() {
    let samples = [
        (
            ": Caption 😀 {#tbl-id}\n\n| A | B |\n|---|---|\n| one | two |\n",
            0,
            2,
        ),
        (
            "| A | B |\n|---|---|\n| one | two |\n\n: Caption 😀 {#tbl-id}",
            4,
            2,
        ),
        ("| A | B |\n|---|---|\n| one | two |", 0, 0),
    ];
    for (text, line, col) in samples {
        let mut server = TestLspServer::new();
        server.open_document("file:///table.qmd", text, "quarto");
        let actions = actions(&server, "file:///table.qmd", line, col);
        assert_eq!(actions.len(), 3, "{text}");
        for action in actions {
            let result = apply(text, &action);
            assert_eq!(result.ends_with('\n'), text.ends_with('\n'));
            if text.contains("Caption") {
                assert!(result.contains("Caption 😀 {#tbl-id}"));
            }
        }
    }
}

#[test]
fn keeps_tables_in_their_containers() {
    let samples = [
        "- Item.\n\n  | A | B |\n  |---|---|\n  | one | two |\n\n- Sibling.\n",
        "> | A | B |\n> |---|---|\n> | one | two |\n\nAfter.\n",
        "::: {.box}\n\n| A | B |\n|---|---|\n| one | two |\n\n:::\n",
        "[^note]:\n    | A | B |\n    |---|---|\n    | one | two |\n\nAfter.\n",
        "Term\n:   Description.\n\n    | A | B |\n    |---|---|\n    | one | two |\n\nAfter.\n",
        "- Item.\n\n  +-----+-----+\n  | A   | B   |\n  +=====+=====+\n  | one | two |\n  +-----+-----+\n\n- Sibling.\n",
        "> +-----+-----+\n> | A   | B   |\n> +=====+=====+\n> | one | two |\n> +-----+-----+\n\nAfter.\n",
        "::: {.box}\n\n+-----+-----+\n| A   | B   |\n+=====+=====+\n| one | two |\n+-----+-----+\n\n:::\n",
        "[^note]:\n    +-----+-----+\n    | A   | B   |\n    +=====+=====+\n    | one | two |\n    +-----+-----+\n\nAfter.\n",
        "Term\n:   Description.\n\n    +-----+-----+\n    | A   | B   |\n    +=====+=====+\n    | one | two |\n    +-----+-----+\n\nAfter.\n",
    ];
    for text in samples {
        let mut server = TestLspServer::new();
        server.open_document("file:///table.qmd", text, "quarto");
        let (line, content) = text
            .lines()
            .enumerate()
            .find(|(_, line)| line.contains("one"))
            .unwrap();
        let actions = actions(
            &server,
            "file:///table.qmd",
            line as u32,
            content.find("one").unwrap() as u32,
        );
        assert_eq!(actions.len(), 3, "{text}");
        for action in actions {
            let result = apply(text, &action);
            let original = panache::parse(text, None);
            let parsed = panache::parse(&result, None);
            let parent_kinds = |tree: &panache::syntax::SyntaxNode| {
                tree.descendants()
                    .find_map(Table::cast)
                    .unwrap()
                    .syntax()
                    .ancestors()
                    .skip(1)
                    .map(|node| node.kind())
                    .collect::<Vec<_>>()
            };
            assert_eq!(parent_kinds(&original), parent_kinds(&parsed), "{result}");
        }
    }
}

#[test]
fn converts_grid_at_captions_preserving_crlf_and_document_boundaries() {
    let grid = "+-----+-----+\n| A   | B   |\n+=====+=====+\n| 😀  | 界  |\n+-----+-----+\n";
    for text in [
        format!(": Caption 😀 {{#tbl-id}}\n\n{grid}"),
        format!("Before.\n\n{grid}\n: Caption 😀 {{#tbl-id}}"),
    ] {
        let text = text.replace('\n', "\r\n");
        let line = text
            .lines()
            .position(|line| line.starts_with(": Caption"))
            .unwrap() as u32;
        let mut server = TestLspServer::new();
        server.open_document("file:///table.qmd", &text, "quarto");
        let actions = actions(&server, "file:///table.qmd", line, 12);
        assert_eq!(actions.len(), 3);
        assert!(
            actions
                .iter()
                .all(|action| action.title != "Convert to grid table")
        );
        for action in actions {
            let output = apply(&text, &action);
            assert_eq!(output.ends_with('\n'), text.ends_with('\n'));
            assert!(!output.replace("\r\n", "").contains('\n'));
            assert!(output.contains(": Caption 😀 {#tbl-id}"));
            assert!(output.contains('😀') && output.contains('界'));
            assert_eq!(
                output.starts_with(": Caption"),
                text.starts_with(": Caption")
            );
        }
    }
}

#[test]
fn reports_grid_restrictions_only_to_capable_clients() {
    for supported in [false, true] {
        let mut server = TestLspServer::new();
        if supported {
            server.initialize_disabled_code_actions("file:///workspace");
        }
        for (source, reason) in [
            (
                "+-------+\n| A B   |\n+===+===+\n| x | y |\n+---+---+\n",
                "merged cells",
            ),
            (
                "+--------+\n| A      |\n+========+\n| - item |\n+--------+\n",
                "block structure",
            ),
            (
                "+---+\n| A |\n+===+\n| x |\n+===+\n| f |\n+---+\n",
                "footers",
            ),
        ] {
            server.open_document("file:///table.qmd", source, "quarto");
            let actions = actions(&server, "file:///table.qmd", 0, 1);
            assert_eq!(actions.len(), if supported { 3 } else { 0 });
            for action in actions {
                assert!(action.edit.is_none());
                assert!(action.disabled.unwrap().reason.contains(reason));
            }
            server.close_document("file:///table.qmd");
        }
    }
}

#[test]
fn grid_conversion_honors_document_width_without_table_indent() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("panache.toml"),
        "[format]\nline-width = 24\ntable-indent = 3\n",
    )
    .unwrap();
    let uri = Uri::from_file_path(dir.path().join("table.qmd")).unwrap();
    let mut server = TestLspServer::new();
    server.initialize(Uri::from_file_path(dir.path()).unwrap().as_str());
    let text = "| A | B |\n|---|---|\n| one two three four five | six seven eight nine ten |\n";
    server.open_document(uri.as_str(), text, "quarto");
    let action = actions(&server, uri.as_str(), 0, 1)
        .into_iter()
        .find(|action| action.title == "Convert to grid table")
        .unwrap();
    let output = apply(text, &action);
    assert_eq!(output.lines().next().unwrap().len(), 24);
    assert!(output.lines().all(|line| line.len() <= 24));
}

#[test]
fn offers_grid_conversion_for_joined_emoji() {
    let mut server = TestLspServer::new();
    let source = "| A | B |\n|---|---|\n| 👩‍💻 | ok |\n";
    server.open_document("file:///table.qmd", source, "quarto");
    let action = actions(&server, "file:///table.qmd", 2, 4)
        .into_iter()
        .find(|action| action.title == "Convert to grid table")
        .expect("grid conversion should be available");
    assert!(action.disabled.is_none());
    let output = apply(source, &action);
    assert!(output.contains("| 👩‍💻 | ok |"));
    assert_eq!(panache::format(&output, None, None), output);
}

#[test]
fn honors_code_action_kind_filter() {
    let mut server = TestLspServer::new();
    server.open_document(
        "file:///table.qmd",
        "| A | B |\n|---|---|\n| x | y |\n",
        "quarto",
    );
    for (kind, expected) in [
        (CodeActionKind::QUICKFIX, 0),
        (CodeActionKind::REFACTOR, 3),
        (CodeActionKind::REFACTOR_REWRITE, 3),
    ] {
        let response = server
            .get_code_actions_with_context(
                "file:///table.qmd",
                Range::default(),
                CodeActionContext {
                    diagnostics: vec![],
                    only: Some(vec![kind]),
                    trigger_kind: None,
                },
            )
            .unwrap();
        assert_eq!(response.iter().filter(|action| matches!(action, CodeActionOrCommand::CodeAction(action) if action.title.contains("table"))).count(), expected);
    }
}

#[test]
fn declines_marker_line_layouts_that_cannot_preserve_the_container() {
    let mut server = TestLspServer::new();
    server.initialize_disabled_code_actions("file:///workspace");
    let text = "- | A | B |\n  |---|---|\n  | one | two |\n\n- Sibling.\n";
    server.open_document("file:///table.qmd", text, "quarto");
    let actions = actions(&server, "file:///table.qmd", 0, 4);
    assert_eq!(actions.len(), 3);
    for action in actions {
        if action.title == "Convert to grid table" {
            let output = apply(text, &action);
            assert!(output.starts_with("- +"));
            assert!(output.ends_with("\n- Sibling.\n"));
        } else {
            assert!(action.edit.is_none());
            assert!(action.disabled.unwrap().reason.contains("container"));
        }
    }
}

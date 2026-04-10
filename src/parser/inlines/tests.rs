// Tests for inline parser functionality
// These tests will be expanded as we implement inline parsing features

#[cfg(test)]
mod emphasis_tests {
    use crate::syntax::SyntaxKind;

    fn parse_inline(input: &str) -> crate::syntax::SyntaxNode {
        // Use main parse function - now includes inline parsing during block parsing
        crate::parser::parse(input, None)
    }

    fn find_emphasis(node: &crate::syntax::SyntaxNode) -> Vec<String> {
        let mut emphasis = Vec::new();
        for child in node.descendants() {
            if child.kind() == SyntaxKind::EMPHASIS {
                emphasis.push(child.to_string());
            }
        }
        emphasis
    }

    fn find_strong(node: &crate::syntax::SyntaxNode) -> Vec<String> {
        let mut strong = Vec::new();
        for child in node.descendants() {
            if child.kind() == SyntaxKind::STRONG {
                strong.push(child.to_string());
            }
        }
        strong
    }

    #[test]
    fn test_simple_emphasis() {
        let input = "This is *italic* text.";
        let inline_tree = parse_inline(input);

        let emphasis = find_emphasis(&inline_tree);
        assert_eq!(emphasis.len(), 1);
        assert_eq!(emphasis[0], "*italic*");
    }

    #[test]
    fn test_simple_strong() {
        let input = "This is **bold** text.";
        let inline_tree = parse_inline(input);

        let strong = find_strong(&inline_tree);
        assert_eq!(strong.len(), 1);
        assert_eq!(strong[0], "**bold**");
    }

    #[test]
    fn test_multiple_emphasis() {
        let input = "Both *foo* and *bar* are italic.";
        let inline_tree = parse_inline(input);

        let emphasis = find_emphasis(&inline_tree);
        assert_eq!(emphasis.len(), 2);
        assert_eq!(emphasis[0], "*foo*");
        assert_eq!(emphasis[1], "*bar*");
    }

    #[test]
    fn test_mixed_emphasis_and_strong() {
        let input = "Mix *italic* and **bold** together.";
        let inline_tree = parse_inline(input);

        let emphasis = find_emphasis(&inline_tree);
        let strong = find_strong(&inline_tree);

        assert_eq!(emphasis.len(), 1);
        assert_eq!(emphasis[0], "*italic*");
        assert_eq!(strong.len(), 1);
        assert_eq!(strong[0], "**bold**");
    }

    #[test]
    fn test_triple_emphasis() {
        let input = "This is ***both*** text.";
        let inline_tree = parse_inline(input);

        // Triple emphasis creates nested Strong and Emphasis
        let strong = find_strong(&inline_tree);
        let emphasis = find_emphasis(&inline_tree);

        assert_eq!(strong.len(), 1);
        assert_eq!(emphasis.len(), 1);
    }

    #[test]
    fn test_underscore_emphasis() {
        let input = "This is _italic_ text.";
        let inline_tree = parse_inline(input);

        let emphasis = find_emphasis(&inline_tree);
        assert_eq!(emphasis.len(), 1);
        assert_eq!(emphasis[0], "_italic_");
    }

    #[test]
    fn test_intraword_underscore_no_emphasis() {
        let input = "This is feas_ible text.";
        let inline_tree = parse_inline(input);

        let emphasis = find_emphasis(&inline_tree);
        assert_eq!(
            emphasis.len(),
            0,
            "Underscores within words should not create emphasis"
        );
    }

    #[test]
    fn test_emphasis_with_spaces() {
        // Spaces around delimiters should prevent emphasis
        let input = "This is * not * italic.";
        let inline_tree = parse_inline(input);

        let emphasis = find_emphasis(&inline_tree);
        assert_eq!(emphasis.len(), 0);
    }

    #[test]
    fn test_no_emphasis() {
        let input = "Plain text with no emphasis.";
        let inline_tree = parse_inline(input);

        let emphasis = find_emphasis(&inline_tree);
        assert_eq!(emphasis.len(), 0);
    }
}

#[cfg(test)]
mod link_tests {
    // TODO: Add tests for link parsing ([text](url), [text][ref])
}

#[cfg(test)]
mod citation_tests {
    use crate::config::Config;
    use crate::syntax::SyntaxKind;

    fn parse_inline(input: &str) -> crate::syntax::SyntaxNode {
        crate::parser::parse(input, None)
    }

    fn parse_inline_with_bookdown(input: &str) -> crate::syntax::SyntaxNode {
        let mut config = Config::default();
        config.extensions.bookdown_references = true;
        crate::parser::parse(input, Some(config))
    }

    fn find_keys(node: &crate::syntax::SyntaxNode, kind: SyntaxKind) -> Vec<String> {
        let mut keys = Vec::new();
        for element in node.descendants_with_tokens() {
            if let Some(token) = element.into_token()
                && token.kind() == kind
            {
                keys.push(token.text().to_string());
            }
        }
        keys
    }

    #[test]
    fn test_bracketed_citation_keys() {
        let input = "Text [@doe99; @smith2000].";
        let inline_tree = parse_inline(input);

        let keys = find_keys(&inline_tree, SyntaxKind::CITATION_KEY);
        assert_eq!(keys, vec!["doe99", "smith2000"]);
    }

    #[test]
    fn test_bare_citation_key() {
        let input = "See @doe99 for details.";
        let inline_tree = parse_inline(input);

        let keys = find_keys(&inline_tree, SyntaxKind::CITATION_KEY);
        assert_eq!(keys, vec!["doe99"]);
    }

    #[test]
    fn bookdown_ref_with_prefix() {
        let input = "See \\@ref(fig:plot).";
        let inline_tree = parse_inline_with_bookdown(input);

        let keys = find_keys(&inline_tree, SyntaxKind::CROSSREF_KEY);
        assert_eq!(keys, vec!["fig:plot"]);
    }

    #[test]
    fn bookdown_ref_section_without_prefix() {
        let input = "See \\@ref(introduction).";
        let inline_tree = parse_inline_with_bookdown(input);

        let keys = find_keys(&inline_tree, SyntaxKind::CROSSREF_KEY);
        assert_eq!(keys, vec!["introduction"]);
    }

    #[test]
    fn bookdown_ref_rejects_unknown_prefix() {
        let input = "See \\@ref(bad:label).";
        let inline_tree = parse_inline_with_bookdown(input);

        let keys = find_keys(&inline_tree, SyntaxKind::CROSSREF_KEY);
        assert!(keys.is_empty());
    }
}

#[cfg(test)]
mod code_tests {

    use crate::syntax::SyntaxKind;

    fn parse_inline(input: &str) -> crate::syntax::SyntaxNode {
        crate::parser::parse(input, None)
    }

    fn find_code_spans(node: &crate::syntax::SyntaxNode) -> Vec<String> {
        let mut code_spans = Vec::new();
        for child in node.descendants() {
            if child.kind() == SyntaxKind::CODE_SPAN {
                code_spans.push(child.to_string());
            }
        }
        code_spans
    }

    #[test]
    fn test_simple_code_span() {
        let input = "This has `code` in it.";
        let inline_tree = parse_inline(input);

        let code_spans = find_code_spans(&inline_tree);
        assert_eq!(code_spans.len(), 1);
        assert_eq!(code_spans[0], "`code`");
    }

    #[test]
    fn test_multiple_code_spans() {
        let input = "Both `foo` and `bar` are code.";
        let inline_tree = parse_inline(input);

        let code_spans = find_code_spans(&inline_tree);
        assert_eq!(code_spans.len(), 2);
        assert_eq!(code_spans[0], "`foo`");
        assert_eq!(code_spans[1], "`bar`");
    }

    #[test]
    fn test_code_span_with_backticks() {
        let input = "Use `` `backtick` `` for literal backticks.";
        let inline_tree = parse_inline(input);

        let code_spans = find_code_spans(&inline_tree);
        assert_eq!(code_spans.len(), 1);
        assert_eq!(code_spans[0], "`` `backtick` ``");
    }

    #[test]
    fn test_no_code_spans() {
        let input = "Plain text with no code.";
        let inline_tree = parse_inline(input);

        let code_spans = find_code_spans(&inline_tree);
        assert_eq!(code_spans.len(), 0);
    }
}

#[cfg(test)]
mod math_tests {

    use crate::syntax::SyntaxKind;

    fn parse_inline(input: &str) -> crate::syntax::SyntaxNode {
        crate::parser::parse(input, None)
    }

    fn find_inline_math(node: &crate::syntax::SyntaxNode) -> Vec<String> {
        let mut math = Vec::new();
        for child in node.descendants() {
            if child.kind() == SyntaxKind::INLINE_MATH {
                math.push(child.to_string());
            }
        }
        math
    }

    fn find_display_math(node: &crate::syntax::SyntaxNode) -> Vec<String> {
        let mut math = Vec::new();
        for child in node.descendants() {
            if child.kind() == SyntaxKind::DISPLAY_MATH {
                math.push(child.to_string());
            }
        }
        math
    }

    #[test]
    fn test_simple_inline_math() {
        let input = "This has $x = y$ in it.";
        let inline_tree = parse_inline(input);

        let math = find_inline_math(&inline_tree);
        assert_eq!(math.len(), 1);
        assert_eq!(math[0], "$x = y$");
    }

    #[test]
    fn test_multiple_inline_math() {
        let input = "Both $a$ and $b$ are variables.";
        let inline_tree = parse_inline(input);

        let math = find_inline_math(&inline_tree);
        assert_eq!(math.len(), 2);
        assert_eq!(math[0], "$a$");
        assert_eq!(math[1], "$b$");
    }

    #[test]
    fn test_inline_math_complex() {
        let input = r"The formula $\frac{1}{2}$ is simple.";
        let inline_tree = parse_inline(input);

        let math = find_inline_math(&inline_tree);
        assert_eq!(math.len(), 1);
        assert_eq!(math[0], r"$\frac{1}{2}$");
    }

    #[test]
    fn test_no_inline_math() {
        let input = "Plain text with no math.";
        let inline_tree = parse_inline(input);

        let math = find_inline_math(&inline_tree);
        assert_eq!(math.len(), 0);
    }

    #[test]
    fn test_mixed_code_and_math() {
        let input = "Code `x` and math $y$ together.";
        let inline_tree = parse_inline(input);

        let math = find_inline_math(&inline_tree);
        assert_eq!(math.len(), 1);
        assert_eq!(math[0], "$y$");
    }

    #[test]
    fn test_math_environment_inline_display() {
        let input = "\\begin{equation}\n  x = y\n\\end{equation}\n";
        let inline_tree = parse_inline(input);

        let math = find_display_math(&inline_tree);
        assert_eq!(math.len(), 1);
        assert_eq!(math[0], input);
    }

    #[test]
    fn test_math_environment_with_indented_end_marker_stays_single_display_math() {
        let input = "\\begin{align*}\n    x = y\n  \n  \\end{align*}\n";
        let inline_tree = parse_inline(input);

        let math = find_display_math(&inline_tree);
        assert_eq!(math.len(), 1);
        assert_eq!(math[0], input);
    }
}

#[cfg(test)]
mod escape_tests {

    use crate::syntax::SyntaxKind;

    fn parse_inline(input: &str) -> crate::syntax::SyntaxNode {
        crate::parser::parse(input, None)
    }

    fn count_nodes_of_kind(node: &crate::syntax::SyntaxNode, kind: SyntaxKind) -> usize {
        node.descendants_with_tokens()
            .filter(|n| n.kind() == kind)
            .count()
    }

    #[test]
    fn test_escaped_asterisk() {
        let input = r"This is \*not emphasis\*.";
        let tree = parse_inline(input);

        let escaped = count_nodes_of_kind(&tree, SyntaxKind::ESCAPED_CHAR);
        assert_eq!(escaped, 2, "Should have two escaped asterisks");
    }

    #[test]
    fn test_escaped_backtick() {
        let input = r"This is \`not code\`.";
        let tree = parse_inline(input);

        let escaped = count_nodes_of_kind(&tree, SyntaxKind::ESCAPED_CHAR);
        let code_spans = count_nodes_of_kind(&tree, SyntaxKind::CODE_SPAN);

        assert_eq!(escaped, 2, "Should have two escaped backticks");
        assert_eq!(code_spans, 0, "Should not create code span");
    }

    #[test]
    fn test_escaped_dollar() {
        let input = r"Price is \$5.";
        let tree = parse_inline(input);

        let escaped = count_nodes_of_kind(&tree, SyntaxKind::ESCAPED_CHAR);
        let math = count_nodes_of_kind(&tree, SyntaxKind::INLINE_MATH);

        assert_eq!(escaped, 1, "Should have one escaped dollar");
        assert_eq!(math, 0, "Should not create math");
    }

    #[test]
    fn test_nonbreaking_space() {
        let input = r"word1\ word2";
        let tree = parse_inline(input);

        let nbsp = count_nodes_of_kind(&tree, SyntaxKind::NONBREAKING_SPACE);
        assert_eq!(nbsp, 1, "Should have one nonbreaking space");
    }

    #[test]
    fn test_hard_line_break() {
        let input = "line1\\\nline2";
        let tree = parse_inline(input);

        let hard_break = count_nodes_of_kind(&tree, SyntaxKind::HARD_LINE_BREAK);
        assert_eq!(hard_break, 1, "Should have one hard line break");
    }

    #[test]
    fn test_hard_line_break_disabled() {
        let input = "line1\\\nline2";
        let mut config = crate::Config::default();
        config.extensions.escaped_line_breaks = false;

        let tree = crate::parser::parse(input, Some(config));

        let hard_break = count_nodes_of_kind(&tree, SyntaxKind::HARD_LINE_BREAK);
        assert_eq!(
            hard_break, 0,
            "Should not have hard line break when extension disabled"
        );
    }

    #[test]
    fn test_escape_prevents_code_span() {
        let input = r"\`not code\`";
        let tree = parse_inline(input);

        let code_spans = count_nodes_of_kind(&tree, SyntaxKind::CODE_SPAN);
        assert_eq!(code_spans, 0, "Escaped backticks should prevent code span");
    }

    #[test]
    fn test_escape_prevents_math() {
        let input = r"\$not math\$";
        let tree = parse_inline(input);

        let math = count_nodes_of_kind(&tree, SyntaxKind::INLINE_MATH);
        assert_eq!(math, 0, "Escaped dollars should prevent math");
    }

    #[test]
    fn test_escape_inside_code_span_not_processed() {
        // Per spec: "Backslash escapes do not work in verbatim contexts"
        let input = r"`\*code\*`";
        let tree = parse_inline(input);

        let code_spans = count_nodes_of_kind(&tree, SyntaxKind::CODE_SPAN);
        assert_eq!(code_spans, 1, "Should create code span");

        // The backslashes should be preserved as-is inside the code span
        let output = tree.to_string();
        assert!(
            output.contains(r"`\*code\*`"),
            "Escapes should not be processed in code"
        );
    }

    #[test]
    fn test_multiple_escapes() {
        let input = r"Escape \* and \$ and \[";
        let tree = parse_inline(input);

        let escaped = count_nodes_of_kind(&tree, SyntaxKind::ESCAPED_CHAR);
        assert_eq!(escaped, 3, "Should have three escaped characters");
    }

    #[test]
    fn test_backslash_not_before_escapable() {
        // Backslash before non-escapable character stays as-is
        let input = r"\a normal text";
        let tree = parse_inline(input);

        let escaped = count_nodes_of_kind(&tree, SyntaxKind::ESCAPED_CHAR);
        assert_eq!(escaped, 0, "Should not escape letter 'a'");

        // The backslash should remain in output
        let output = tree.to_string();
        assert!(
            output.contains(r"\a"),
            "Backslash before letter should remain"
        );
    }
}

#[cfg(test)]
mod footnote_tests {

    use crate::syntax::SyntaxKind;

    fn parse_inline(input: &str) -> crate::syntax::SyntaxNode {
        crate::parser::parse(input, None)
    }

    fn find_footnotes(node: &crate::syntax::SyntaxNode) -> Vec<String> {
        let mut footnotes = Vec::new();
        for child in node.descendants() {
            if child.kind() == SyntaxKind::INLINE_FOOTNOTE {
                footnotes.push(child.to_string());
            }
        }
        footnotes
    }

    #[test]
    fn test_simple_inline_footnote() {
        let input = "Here is some text^[This is a footnote] with more text.";
        let tree = parse_inline(input);

        let footnotes = find_footnotes(&tree);
        assert_eq!(footnotes.len(), 1);
        assert_eq!(footnotes[0], "^[This is a footnote]");
    }

    #[test]
    fn test_multiple_inline_footnotes() {
        let input = "First^[footnote 1] and second^[footnote 2] notes.";
        let tree = parse_inline(input);

        let footnotes = find_footnotes(&tree);
        assert_eq!(footnotes.len(), 2);
        assert_eq!(footnotes[0], "^[footnote 1]");
        assert_eq!(footnotes[1], "^[footnote 2]");
    }

    #[test]
    fn test_footnote_with_inline_elements() {
        let input = "Text^[Note with *emphasis* and `code`] end.";
        let tree = parse_inline(input);

        let footnotes = find_footnotes(&tree);
        assert_eq!(footnotes.len(), 1);
        // The footnote should contain the inline elements
        assert!(footnotes[0].contains("*emphasis*"));
        assert!(footnotes[0].contains("`code`"));
    }

    #[test]
    fn test_footnote_empty() {
        let input = "Text with empty^[] footnote.";
        let tree = parse_inline(input);

        let footnotes = find_footnotes(&tree);
        assert_eq!(footnotes.len(), 1);
        assert_eq!(footnotes[0], "^[]");
    }

    #[test]
    fn test_no_footnote_without_bracket() {
        let input = "Text with ^ caret but no bracket.";
        let tree = parse_inline(input);

        let footnotes = find_footnotes(&tree);
        assert_eq!(footnotes.len(), 0);
    }

    #[test]
    fn test_footnote_with_link() {
        let input = "Text^[See [link](http://example.com) for more] end.";
        let tree = parse_inline(input);

        let footnotes = find_footnotes(&tree);
        assert_eq!(footnotes.len(), 1);
        assert!(footnotes[0].contains("[link](http://example.com)"));
    }
}

#[cfg(test)]
mod bracketed_span_tests {

    use crate::syntax::SyntaxKind;

    fn parse_inline(input: &str) -> crate::syntax::SyntaxNode {
        crate::parser::parse(input, None)
    }

    fn assert_has_kind(tree: &crate::syntax::SyntaxNode, kind: SyntaxKind) {
        assert!(
            tree.descendants().any(|n| n.kind() == kind),
            "Expected to find {:?} in tree",
            kind
        );
    }

    fn assert_has_text(tree: &crate::syntax::SyntaxNode, kind: SyntaxKind, expected: &str) {
        let node = tree
            .descendants()
            .find(|n| n.kind() == kind)
            .unwrap_or_else(|| panic!("Expected to find {:?}", kind));
        assert_eq!(node.text().to_string(), expected);
    }

    #[test]
    fn simple_span() {
        let tree = parse_inline("[text]{.class}");
        assert_has_kind(&tree, SyntaxKind::BRACKETED_SPAN);
        assert_has_text(&tree, SyntaxKind::SPAN_CONTENT, "text");
    }

    #[test]
    fn span_with_emphasis() {
        let tree = parse_inline("[**bold** text]{.highlight}");
        assert_has_kind(&tree, SyntaxKind::BRACKETED_SPAN);
        assert_has_kind(&tree, SyntaxKind::STRONG);
    }

    #[test]
    fn span_with_code() {
        let tree = parse_inline("[`code` text]{.mono}");
        assert_has_kind(&tree, SyntaxKind::BRACKETED_SPAN);
        assert_has_kind(&tree, SyntaxKind::CODE_SPAN);
    }

    #[test]
    fn span_in_paragraph() {
        let tree = parse_inline("Before [span]{.class} after");
        assert_has_kind(&tree, SyntaxKind::BRACKETED_SPAN);
        let text = tree.text().to_string();
        assert!(text.contains("Before"));
        assert!(text.contains("after"));
    }

    #[test]
    fn multiple_spans() {
        let tree = parse_inline("[first]{.a} and [second]{.b}");
        let spans: Vec<_> = tree
            .descendants()
            .filter(|n| n.kind() == SyntaxKind::BRACKETED_SPAN)
            .collect();
        assert_eq!(spans.len(), 2);
    }

    #[test]
    fn nested_brackets_in_span() {
        let tree = parse_inline("[[nested]]{.class}");
        assert_has_kind(&tree, SyntaxKind::BRACKETED_SPAN);
        assert_has_text(&tree, SyntaxKind::SPAN_CONTENT, "[nested]");
    }
}

// ========================================
// Reference Images Tests
// ========================================

#[cfg(test)]
mod reference_tests {

    use crate::syntax::SyntaxKind;

    fn parse_with_refs(input: &str) -> crate::syntax::SyntaxNode {
        crate::parser::parse(input, None)
    }

    #[test]
    fn test_reference_image_explicit() {
        let input = "Text with ![alt text][img-ref] image.

[img-ref]: image.jpg \"Image Title\"";

        let parsed = crate::parser::parse(input, None);

        let para = parsed.first_child().expect("paragraph");

        let image = para
            .descendants()
            .find(|n| n.kind() == SyntaxKind::IMAGE_LINK)
            .expect("image node");

        // Should preserve reference syntax
        let text = image.text().to_string();
        assert!(text.contains("![alt text][img-ref]"));
    }

    #[test]
    fn test_reference_image_implicit() {
        let input = "Text with ![image ref][] image.

[image ref]: /path/to/image.png";

        let parsed = parse_with_refs(input);
        let para = parsed.first_child().expect("paragraph");
        let image = para
            .descendants()
            .find(|n| n.kind() == SyntaxKind::IMAGE_LINK)
            .expect("image node");

        // Should preserve implicit reference syntax
        let text = image.text().to_string();
        assert!(text.contains("![image ref][]"));
    }

    #[test]
    fn test_reference_image_unresolved() {
        let input = "Text with ![alt][missing-ref] image.";

        let parsed = parse_with_refs(input);
        let para = parsed.first_child().expect("paragraph");

        // Should still parse as an image, but keep original reference syntax
        let text = para.text().to_string();
        assert!(text.contains("![alt][missing-ref]"));
    }

    #[test]
    fn test_reference_image_case_insensitive() {
        let input = "Image: ![ALT][MyRef]

[myref]: image.jpg";

        let parsed = parse_with_refs(input);
        let para = parsed.first_child().expect("paragraph");
        let image = para
            .descendants()
            .find(|n| n.kind() == SyntaxKind::IMAGE_LINK)
            .expect("image node");

        // Should preserve reference syntax (case insensitivity is for lookup, not formatting)
        let text = image.text().to_string();
        assert!(text.contains("![ALT][MyRef]"));
    }
}

#[cfg(test)]
mod raw_inline_tests {

    use crate::syntax::SyntaxKind;

    fn parse_inline(input: &str) -> crate::syntax::SyntaxNode {
        crate::parser::parse(input, None)
    }

    fn parse_inline_with_config(input: &str, config: crate::Config) -> crate::syntax::SyntaxNode {
        crate::parser::parse(input, Some(config))
    }

    fn find_raw_inline(node: &crate::syntax::SyntaxNode) -> Vec<String> {
        let mut raw_inlines = Vec::new();
        for child in node.descendants() {
            if child.kind() == SyntaxKind::RAW_INLINE {
                raw_inlines.push(child.to_string());
            }
        }
        raw_inlines
    }

    fn find_code_spans(node: &crate::syntax::SyntaxNode) -> Vec<String> {
        let mut code_spans = Vec::new();
        for child in node.descendants() {
            if child.kind() == SyntaxKind::CODE_SPAN {
                code_spans.push(child.to_string());
            }
        }
        code_spans
    }

    #[test]
    fn test_raw_inline_html() {
        let input = "This is `<a>html</a>`{=html} text.";
        let inline_tree = parse_inline(input);

        let raw_inlines = find_raw_inline(&inline_tree);
        assert_eq!(raw_inlines.len(), 1);
        assert_eq!(raw_inlines[0], "`<a>html</a>`{=html}");
    }

    #[test]
    fn test_raw_inline_latex() {
        let input = r"This is `\LaTeX`{=latex} formatted.";
        let inline_tree = parse_inline(input);

        let raw_inlines = find_raw_inline(&inline_tree);
        assert_eq!(raw_inlines.len(), 1);
        assert_eq!(raw_inlines[0], r"`\LaTeX`{=latex}");
    }

    #[test]
    fn test_raw_inline_openxml() {
        let input = "This is `<w:br/>`{=openxml} a pagebreak.";
        let inline_tree = parse_inline(input);

        let raw_inlines = find_raw_inline(&inline_tree);
        assert_eq!(raw_inlines.len(), 1);
        assert_eq!(raw_inlines[0], "`<w:br/>`{=openxml}");
    }

    #[test]
    fn test_raw_inline_with_double_backticks() {
        let input = "This is `` `backtick` ``{=html} text.";
        let inline_tree = parse_inline(input);

        let raw_inlines = find_raw_inline(&inline_tree);
        assert_eq!(raw_inlines.len(), 1);
        assert_eq!(raw_inlines[0], "`` `backtick` ``{=html}");
    }

    #[test]
    fn test_raw_inline_disabled() {
        let input = "This is `<a>html</a>`{=html} text.";
        let mut config = crate::Config::default();
        config.extensions.raw_attribute = false;

        let inline_tree = parse_inline_with_config(input, config);

        // Should be treated as regular code span with attributes
        let raw_inlines = find_raw_inline(&inline_tree);
        assert_eq!(raw_inlines.len(), 0);

        let code_spans = find_code_spans(&inline_tree);
        assert_eq!(code_spans.len(), 1);
        assert_eq!(code_spans[0], "`<a>html</a>`{=html}");
    }

    #[test]
    fn test_code_span_with_regular_class() {
        // Regular code span with .class should not be treated as raw inline
        let input = "This is `code`{.python} text.";
        let inline_tree = parse_inline(input);

        let raw_inlines = find_raw_inline(&inline_tree);
        assert_eq!(raw_inlines.len(), 0);

        let code_spans = find_code_spans(&inline_tree);
        assert_eq!(code_spans.len(), 1);
        assert_eq!(code_spans[0], "`code`{.python}");
    }

    #[test]
    fn test_raw_inline_mixed_with_code_spans() {
        let input = "Regular `code` and raw `<html>`{=html} in one sentence.";
        let inline_tree = parse_inline(input);

        let raw_inlines = find_raw_inline(&inline_tree);
        assert_eq!(raw_inlines.len(), 1);
        assert_eq!(raw_inlines[0], "`<html>`{=html}");

        let code_spans = find_code_spans(&inline_tree);
        assert_eq!(code_spans.len(), 1);
        assert_eq!(code_spans[0], "`code`");
    }

    #[test]
    fn test_raw_inline_multiple_formats() {
        let input = "HTML `<a>`{=html} and LaTeX `\\cmd`{=latex} together.";
        let inline_tree = parse_inline(input);

        let raw_inlines = find_raw_inline(&inline_tree);
        assert_eq!(raw_inlines.len(), 2);
        assert_eq!(raw_inlines[0], "`<a>`{=html}");
        assert_eq!(raw_inlines[1], r"`\cmd`{=latex}");
    }

    #[test]
    fn test_raw_inline_with_id_not_raw() {
        // If attributes include ID, it's not a raw inline
        let input = "This is `code`{#myid =html} text.";
        let inline_tree = parse_inline(input);

        // Should be code span, not raw inline (because it has an ID)
        let raw_inlines = find_raw_inline(&inline_tree);
        assert_eq!(raw_inlines.len(), 0);

        let code_spans = find_code_spans(&inline_tree);
        assert_eq!(code_spans.len(), 1);
    }

    #[test]
    fn test_raw_inline_with_key_value_not_raw() {
        // If attributes include key=value, it's not a raw inline
        let input = "This is `code`{=html key=val} text.";
        let inline_tree = parse_inline(input);

        // Should be code span, not raw inline
        let raw_inlines = find_raw_inline(&inline_tree);
        assert_eq!(raw_inlines.len(), 0);

        let code_spans = find_code_spans(&inline_tree);
        assert_eq!(code_spans.len(), 1);
    }
}

#[cfg(test)]
mod extension_guard_tests {
    use crate::config::Config;
    use crate::syntax::SyntaxKind;

    fn parse_with_config(input: &str, config: Config) -> crate::syntax::SyntaxNode {
        crate::parser::parse(input, Some(config))
    }

    fn count_kind(tree: &crate::syntax::SyntaxNode, kind: SyntaxKind) -> usize {
        tree.descendants_with_tokens()
            .filter(|element| element.kind() == kind)
            .count()
    }

    #[test]
    fn strikeout_disabled_treats_text_literal() {
        let mut config = Config::default();
        config.extensions.strikeout = false;
        let tree = parse_with_config("~~strike~~", config);
        assert_eq!(count_kind(&tree, SyntaxKind::STRIKEOUT), 0);
    }

    #[test]
    fn superscript_disabled_treats_text_literal() {
        let mut config = Config::default();
        config.extensions.superscript = false;
        let tree = parse_with_config("^sup^", config);
        assert_eq!(count_kind(&tree, SyntaxKind::SUPERSCRIPT), 0);
    }

    #[test]
    fn subscript_disabled_treats_text_literal() {
        let mut config = Config::default();
        config.extensions.subscript = false;
        let tree = parse_with_config("~sub~", config);
        assert_eq!(count_kind(&tree, SyntaxKind::SUBSCRIPT), 0);
    }

    #[test]
    fn bracketed_spans_disabled_do_not_parse_span() {
        let mut config = Config::default();
        config.extensions.bracketed_spans = false;
        let tree = parse_with_config("[text]{.class}", config);
        assert_eq!(count_kind(&tree, SyntaxKind::BRACKETED_SPAN), 0);
    }

    #[test]
    fn inline_code_attributes_disabled_leaves_attrs_outside_code_span() {
        let mut config = Config::default();
        config.extensions.inline_code_attributes = false;
        let tree = parse_with_config("`code`{.lang}", config);
        let code_span = tree
            .descendants()
            .find(|n| n.kind() == SyntaxKind::CODE_SPAN)
            .expect("code span");
        assert_eq!(code_span.to_string(), "`code`");
        assert!(tree.to_string().contains("{.lang}"));
    }

    #[test]
    fn tex_math_dollars_disabled_keeps_dollars_literal() {
        let mut config = Config::default();
        config.extensions.tex_math_dollars = false;
        let tree = parse_with_config("$x$", config);
        assert_eq!(count_kind(&tree, SyntaxKind::INLINE_MATH), 0);
        assert_eq!(count_kind(&tree, SyntaxKind::DISPLAY_MATH), 0);
    }

    #[test]
    fn tex_math_gfm_enabled_parses_backtick_dollar_inline_math() {
        let mut config = Config::default();
        config.extensions.tex_math_dollars = false;
        config.extensions.tex_math_gfm = true;
        let tree = parse_with_config("$`x^2`$", config);
        assert_eq!(count_kind(&tree, SyntaxKind::INLINE_MATH), 1);
    }

    #[test]
    fn tex_math_gfm_disabled_keeps_backtick_dollar_inline_literal() {
        let mut config = Config::default();
        config.extensions.tex_math_dollars = false;
        config.extensions.tex_math_gfm = false;
        let tree = parse_with_config("$`x^2`$", config);
        assert_eq!(count_kind(&tree, SyntaxKind::INLINE_MATH), 0);
    }

    #[test]
    fn all_symbols_escapable_disabled_stops_symbol_escapes() {
        let mut config = Config::default();
        config.extensions.all_symbols_escapable = false;
        let tree = parse_with_config(r"\?", config);
        assert_eq!(count_kind(&tree, SyntaxKind::ESCAPED_CHAR), 0);
    }

    #[test]
    fn escaped_line_breaks_still_work_when_symbol_escapes_disabled() {
        let mut config = Config::default();
        config.extensions.all_symbols_escapable = false;
        config.extensions.escaped_line_breaks = true;
        let tree = parse_with_config("a\\\nb", config);
        assert_eq!(count_kind(&tree, SyntaxKind::HARD_LINE_BREAK), 1);
    }

    #[test]
    fn hard_line_breaks_enabled_turns_single_newline_into_hard_break() {
        let mut config = Config::default();
        config.extensions.hard_line_breaks = true;
        let tree = parse_with_config("a\nb", config);
        assert_eq!(count_kind(&tree, SyntaxKind::HARD_LINE_BREAK), 1);
        assert_eq!(count_kind(&tree, SyntaxKind::NEWLINE), 0);
    }

    #[test]
    fn hard_line_breaks_disabled_keeps_single_newline_token() {
        let mut config = Config::default();
        config.extensions.hard_line_breaks = false;
        let tree = parse_with_config("a\nb", config);
        assert_eq!(count_kind(&tree, SyntaxKind::HARD_LINE_BREAK), 0);
        assert_eq!(count_kind(&tree, SyntaxKind::NEWLINE), 1);
    }

    #[test]
    fn raw_tex_disabled_blocks_latex_command_node() {
        let mut config = Config::default();
        config.extensions.raw_tex = false;
        let tree = parse_with_config(r"\alpha", config);
        assert_eq!(count_kind(&tree, SyntaxKind::LATEX_COMMAND), 0);
    }

    #[test]
    fn inline_footnotes_disabled_keeps_note_literal() {
        let mut config = Config::default();
        config.extensions.inline_footnotes = false;
        let tree = parse_with_config("A^[note]", config);
        assert_eq!(count_kind(&tree, SyntaxKind::INLINE_FOOTNOTE), 0);
    }

    #[test]
    fn footnotes_disabled_keeps_reference_literal() {
        let mut config = Config::default();
        config.extensions.footnotes = false;
        let tree = parse_with_config("[^id]", config);
        assert_eq!(count_kind(&tree, SyntaxKind::FOOTNOTE_REFERENCE), 0);
    }

    #[test]
    fn footnote_reference_emits_structural_tokens() {
        let tree = parse_with_config("[^id]", Config::default());
        assert_eq!(count_kind(&tree, SyntaxKind::FOOTNOTE_REFERENCE), 1);
        assert_eq!(count_kind(&tree, SyntaxKind::FOOTNOTE_LABEL_START), 1);
        assert_eq!(count_kind(&tree, SyntaxKind::FOOTNOTE_LABEL_ID), 1);
        assert_eq!(count_kind(&tree, SyntaxKind::FOOTNOTE_LABEL_END), 1);
    }

    #[test]
    fn autolinks_disabled_keeps_angle_link_literal() {
        let mut config = Config::default();
        config.extensions.autolinks = false;
        let tree = parse_with_config("<https://example.com>", config);
        assert_eq!(count_kind(&tree, SyntaxKind::AUTO_LINK), 0);
    }

    #[test]
    fn inline_links_disabled_keeps_inline_link_literal() {
        let mut config = Config::default();
        config.extensions.inline_links = false;
        let tree = parse_with_config("[text](url)", config);
        assert_eq!(count_kind(&tree, SyntaxKind::LINK), 0);
    }

    #[test]
    fn reference_links_disabled_keeps_reference_link_literal() {
        let mut config = Config::default();
        config.extensions.reference_links = false;
        let tree = parse_with_config("[text][ref]", config);
        assert_eq!(count_kind(&tree, SyntaxKind::LINK), 0);
    }

    #[test]
    fn citations_disabled_keeps_bare_citation_literal() {
        let mut config = Config::default();
        config.extensions.citations = false;
        let tree = parse_with_config("@doe99", config);
        assert_eq!(count_kind(&tree, SyntaxKind::CITATION), 0);
    }

    #[test]
    fn emoji_enabled_parses_colon_alias() {
        let mut config = Config::default();
        config.extensions.emoji = true;
        let tree = parse_with_config("Hello :smile: world", config);
        assert_eq!(count_kind(&tree, SyntaxKind::EMOJI), 1);
    }

    #[test]
    fn emoji_disabled_keeps_colon_alias_literal() {
        let mut config = Config::default();
        config.extensions.emoji = false;
        let tree = parse_with_config("Hello :smile: world", config);
        assert_eq!(count_kind(&tree, SyntaxKind::EMOJI), 0);
    }

    #[test]
    fn crossrefs_enabled_without_citations_still_parse_crossref() {
        let mut config = Config::default();
        config.extensions.citations = false;
        config.extensions.quarto_crossrefs = true;
        let tree = parse_with_config("@fig-plot", config);
        assert_eq!(count_kind(&tree, SyntaxKind::CROSSREF), 1);
        assert_eq!(count_kind(&tree, SyntaxKind::CITATION), 0);
    }

    #[test]
    fn crossrefs_disabled_parse_quarto_key_as_citation() {
        let mut config = Config::default();
        config.extensions.citations = true;
        config.extensions.quarto_crossrefs = false;
        let tree = parse_with_config("@fig-plot", config);
        assert_eq!(count_kind(&tree, SyntaxKind::CROSSREF), 0);
        assert_eq!(count_kind(&tree, SyntaxKind::CITATION), 1);
    }

    #[test]
    fn rmarkdown_inline_code_enabled_parses_classic_form() {
        let mut config = Config::default();
        config.extensions.rmarkdown_inline_code = true;
        config.extensions.quarto_inline_code = false;
        let tree = parse_with_config("`3 == `r 2 + 1``", config);
        assert_eq!(count_kind(&tree, SyntaxKind::INLINE_EXECUTABLE_CODE), 1);
    }

    #[test]
    fn rmarkdown_inline_code_disabled_keeps_classic_form_literal() {
        let mut config = Config::default();
        config.extensions.rmarkdown_inline_code = false;
        config.extensions.quarto_inline_code = false;
        let tree = parse_with_config("`3 == `r 2 + 1``", config);
        assert_eq!(count_kind(&tree, SyntaxKind::INLINE_EXECUTABLE_CODE), 0);
        assert_eq!(count_kind(&tree, SyntaxKind::CODE_SPAN), 1);
    }

    #[test]
    fn quarto_inline_code_enabled_parses_braced_form() {
        let mut config = Config::default();
        config.extensions.rmarkdown_inline_code = false;
        config.extensions.quarto_inline_code = true;
        let tree = parse_with_config("`3 == `{r} 2 + 1``", config);
        assert_eq!(count_kind(&tree, SyntaxKind::INLINE_EXECUTABLE_CODE), 1);
    }

    #[test]
    fn quarto_inline_code_disabled_keeps_braced_form_literal() {
        let mut config = Config::default();
        config.extensions.rmarkdown_inline_code = false;
        config.extensions.quarto_inline_code = false;
        let tree = parse_with_config("`3 == `{r} 2 + 1``", config);
        assert_eq!(count_kind(&tree, SyntaxKind::INLINE_EXECUTABLE_CODE), 0);
        assert_eq!(count_kind(&tree, SyntaxKind::CODE_SPAN), 1);
    }

    #[test]
    fn classic_form_not_parsed_when_only_quarto_inline_enabled() {
        let mut config = Config::default();
        config.extensions.rmarkdown_inline_code = false;
        config.extensions.quarto_inline_code = true;
        let tree = parse_with_config("`3 == `r 2 + 1``", config);
        assert_eq!(count_kind(&tree, SyntaxKind::INLINE_EXECUTABLE_CODE), 0);
    }

    #[test]
    fn braced_form_not_parsed_when_only_rmarkdown_inline_enabled() {
        let mut config = Config::default();
        config.extensions.rmarkdown_inline_code = true;
        config.extensions.quarto_inline_code = false;
        let tree = parse_with_config("`3 == `{r} 2 + 1``", config);
        assert_eq!(count_kind(&tree, SyntaxKind::INLINE_EXECUTABLE_CODE), 0);
    }
}

#[cfg(test)]
mod complex_emphasis_tests {

    use crate::syntax::SyntaxKind;

    fn parse_inline(input: &str) -> crate::syntax::SyntaxNode {
        crate::parser::parse(input, None)
    }

    fn count_node_type(node: &crate::syntax::SyntaxNode, kind: SyntaxKind) -> usize {
        node.descendants().filter(|n| n.kind() == kind).count()
    }

    fn find_text_nodes(node: &crate::syntax::SyntaxNode) -> Vec<String> {
        let mut texts = Vec::new();
        for child in node.descendants_with_tokens() {
            if let rowan::NodeOrToken::Token(token) = child
                && token.kind() == SyntaxKind::TEXT
            {
                texts.push(token.text().to_string());
            }
        }
        texts
    }

    #[test]
    fn test_triple_emphasis_with_nested_strong() {
        // Issue: ***foo **bar** baz***
        // Should parse as: EMPH containing STRONG("foo"), text(" bar "), STRONG("baz")
        // Currently: Fails to parse triple emphasis, treats *** as literal text
        let input = "***foo **bar** baz***";
        let tree = parse_inline(input);

        println!("Tree:\n{:#?}", tree);

        // Should have 1 EMPHASIS node
        let emph_count = count_node_type(&tree, SyntaxKind::EMPHASIS);
        assert_eq!(
            emph_count, 1,
            "Expected 1 EMPHASIS node, found {}",
            emph_count
        );

        // Should have 2 STRONG nodes inside the EMPHASIS
        let strong_count = count_node_type(&tree, SyntaxKind::STRONG);
        assert_eq!(
            strong_count, 2,
            "Expected 2 STRONG nodes, found {}",
            strong_count
        );

        // Should NOT have TEXT nodes containing ***
        let text_nodes = find_text_nodes(&tree);
        for text in &text_nodes {
            assert!(
                !text.contains("***"),
                "Found TEXT node with ***: {:?}",
                text
            );
        }
    }

    #[test]
    fn test_adjacent_strong_delimiters() {
        // Input: **foo****bar**
        // Parser behavior: Two separate STRONG nodes (this is correct for CST)
        // Formatter should merge them to **foobar**
        //
        // Note: Pandoc's AST shows Strong[Str "foo", Str "bar"] but that's after
        // AST normalization. The parser naturally produces two STRONG nodes when
        // `****` acts as closer + opener.
        let input = "**foo****bar**";
        let tree = parse_inline(input);

        println!("Tree:\n{:#?}", tree);

        // Parser produces 2 STRONG nodes - this is correct CST behavior
        // The formatter is responsible for merging adjacent emphasis
        let strong_count = count_node_type(&tree, SyntaxKind::STRONG);
        assert_eq!(
            strong_count, 2,
            "Expected 2 STRONG nodes (formatter merges them), found {}",
            strong_count
        );
    }

    #[test]
    fn test_triple_emphasis_simple() {
        // Simple case: ***text***
        // Should parse as: STRONG > EMPH > text
        let input = "***simple***";
        let tree = parse_inline(input);

        println!("Tree:\n{:#?}", tree);

        let emph_count = count_node_type(&tree, SyntaxKind::EMPHASIS);
        let strong_count = count_node_type(&tree, SyntaxKind::STRONG);

        // Should have 1 of each (nested)
        assert_eq!(emph_count, 1, "Expected 1 EMPHASIS node");
        assert_eq!(strong_count, 1, "Expected 1 STRONG node");
    }

    #[test]
    fn test_overlapping_delimiters_with_escapes() {
        // Issue: *foo **bar* baz**
        // This is a complex case with overlapping boundaries
        let input = "*foo **bar* baz**";
        let tree = parse_inline(input);

        println!("Tree:\n{:#?}", tree);

        // Need to verify Pandoc's exact parsing here
        // Likely: *foo **bar* (emphasis closes first) + remaining baz**
    }

    #[test]
    fn test_emphasis_after_escaped_delimiter() {
        // Test: \**not bold\**
        // After \*, should still be able to parse *not bold*
        // Currently works but formats with extra escaping
        let input = r"\**not bold\**";
        let tree = parse_inline(input);

        println!("Tree:\n{:#?}", tree);

        // Should have ESCAPED_CHAR nodes for \*
        let escaped_count = tree
            .descendants_with_tokens()
            .filter(|n| {
                if let rowan::NodeOrToken::Token(t) = n {
                    t.kind() == SyntaxKind::ESCAPED_CHAR
                } else {
                    false
                }
            })
            .count();

        assert_eq!(escaped_count, 2, "Expected 2 ESCAPED_CHAR nodes");

        // Note: Our current parse is actually reasonable, just formats differently than Pandoc
    }

    #[test]
    fn test_triple_emphasis_with_embedded_double() {
        // More specific test for the triple emphasis bug
        // Input: ***a **b** c***
        // The ** around "b" should be parsed as nested STRONG
        // The *** should find the closing *** at the end
        let input = "***a **b** c***";
        let tree = parse_inline(input);

        println!("Tree:\n{:#?}", tree);

        // Debug: print all node kinds
        for node in tree.descendants() {
            println!("Node: {:?} = {}", node.kind(), node);
        }

        // Should have EMPHASIS wrapping everything
        let emph_count = count_node_type(&tree, SyntaxKind::EMPHASIS);
        assert!(emph_count >= 1, "Should have at least 1 EMPHASIS node");

        // Should have STRONG for "b"
        let strong_count = count_node_type(&tree, SyntaxKind::STRONG);
        assert!(strong_count >= 1, "Should have at least 1 STRONG node");

        // The opening *** should not be treated as TEXT
        let text_nodes = find_text_nodes(&tree);
        let has_triple_star_text = text_nodes.iter().any(|t| t.starts_with("***"));
        assert!(
            !has_triple_star_text,
            "Opening *** should not be TEXT, found: {:?}",
            text_nodes
        );
    }

    #[test]
    fn test_triple_emphasis_pandoc_structure() {
        // Input: ***foo **bar** baz***
        // Pandoc parses as: Emph[Strong["foo "], "bar", Strong[" baz"]]
        // NOT as: Strong[Emph["foo ", Strong["bar"], " baz"]]
        //
        // The key is that `**` at position 7 acts as an ender for the `***` opener,
        // triggering the `ender c 2 >> one c (B.strong <$> contents)` fallback.
        let input = "***foo **bar** baz***";
        let tree = parse_inline(input);

        println!("Tree:\n{:#?}", tree);

        // Find the top-level emphasis/strong structure
        let paragraph = tree.children().find(|n| n.kind() == SyntaxKind::PARAGRAPH);
        assert!(paragraph.is_some(), "Should have PARAGRAPH");

        let para = paragraph.unwrap();
        let first_child = para.children().next();
        assert!(first_child.is_some(), "PARAGRAPH should have children");

        // According to Pandoc, the outermost element should be EMPHASIS (not STRONG)
        // because the `***` opener matches with `ender c 2` fallback which produces
        // `one c (B.strong <$> contents)` = Emph[Strong[...], ...]
        let first_kind = first_child.as_ref().unwrap().kind();
        assert_eq!(
            first_kind,
            SyntaxKind::EMPHASIS,
            "Outermost element should be EMPHASIS (Pandoc: Emph[Strong[...], ...]), got {:?}",
            first_kind
        );

        // Count nested STRONG nodes inside the EMPHASIS
        let emph_node = first_child.unwrap();
        let strong_count: usize = emph_node
            .descendants()
            .filter(|n| n.kind() == SyntaxKind::STRONG)
            .count();

        assert_eq!(
            strong_count, 2,
            "Should have 2 STRONG nodes inside EMPHASIS (for 'foo ' and ' baz')"
        );
    }

    #[test]
    fn test_nested_emphasis_and_strong() {
        // Test: **foo *bar* baz**
        // Should parse as STRONG containing EMPH
        let input = "**foo *bar* baz**";
        let tree = parse_inline(input);

        let strong_count = count_node_type(&tree, SyntaxKind::STRONG);
        let emph_count = count_node_type(&tree, SyntaxKind::EMPHASIS);

        assert_eq!(strong_count, 1, "Should have 1 STRONG node");
        assert_eq!(
            emph_count, 1,
            "Should have 1 EMPH node (nested inside STRONG)"
        );
    }

    #[test]
    fn test_nested_strong_and_emphasis() {
        // Test: *foo **bar** baz*
        // Should parse as EMPH containing STRONG
        let input = "*foo **bar** baz*";
        let tree = parse_inline(input);

        let strong_count = count_node_type(&tree, SyntaxKind::STRONG);
        let emph_count = count_node_type(&tree, SyntaxKind::EMPHASIS);

        assert_eq!(emph_count, 1, "Should have 1 EMPH node");
        assert_eq!(
            strong_count, 1,
            "Should have 1 STRONG node (nested inside EMPH)"
        );
    }

    #[test]
    fn test_deeply_nested_emphasis() {
        // Test: **foo *bar **nested** baz* qux**
        // Complex nesting: STRONG > EMPH > STRONG
        let input = "**foo *bar **nested** baz* qux**";
        let tree = parse_inline(input);

        println!("Tree:\n{:#?}", tree);

        // Should have 2 STRONG nodes and 1 EMPH node
        let strong_count = count_node_type(&tree, SyntaxKind::STRONG);
        let emph_count = count_node_type(&tree, SyntaxKind::EMPHASIS);

        assert!(strong_count >= 2, "Should have at least 2 STRONG nodes");
        assert!(emph_count >= 1, "Should have at least 1 EMPH node");
    }
}

// Tests for inline parser functionality
// These tests will be expanded as we implement inline parsing features

#[cfg(test)]
mod emphasis_tests {
    use crate::config::Config;
    use crate::parser::block_parser::BlockParser;
    use crate::parser::inline_parser::InlineParser;
    use crate::syntax::SyntaxKind;

    fn parse_inline(input: &str) -> crate::syntax::SyntaxNode {
        let config = Config::default();
        let (block_tree, registry) = BlockParser::new(input, &config).parse();
        InlineParser::new(block_tree, config, registry).parse()
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
mod code_tests {
    use crate::config::Config;
    use crate::parser::block_parser::BlockParser;
    use crate::parser::inline_parser::InlineParser;
    use crate::syntax::SyntaxKind;

    fn parse_inline(input: &str) -> crate::syntax::SyntaxNode {
        let config = Config::default();
        let (block_tree, registry) = BlockParser::new(input, &config).parse();
        InlineParser::new(block_tree, config, registry).parse()
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
    use crate::config::Config;
    use crate::parser::block_parser::BlockParser;
    use crate::parser::inline_parser::InlineParser;
    use crate::syntax::SyntaxKind;

    fn parse_inline(input: &str) -> crate::syntax::SyntaxNode {
        let config = Config::default();
        let (block_tree, registry) = BlockParser::new(input, &config).parse();
        InlineParser::new(block_tree, config, registry).parse()
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
}

#[cfg(test)]
mod escape_tests {
    use crate::config::Config;
    use crate::parser::block_parser::BlockParser;
    use crate::parser::inline_parser::InlineParser;
    use crate::syntax::SyntaxKind;

    fn parse_inline(input: &str) -> crate::syntax::SyntaxNode {
        let config = Config::default();
        let (block_tree, registry) = BlockParser::new(input, &config).parse();
        InlineParser::new(block_tree, config, registry).parse()
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
        let mut config = Config::default();
        config.extensions.escaped_line_breaks = false;

        let (block_tree, registry) = BlockParser::new(input, &config).parse();
        let tree = InlineParser::new(block_tree, config, registry).parse();

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
    use crate::config::Config;
    use crate::parser::block_parser::BlockParser;
    use crate::parser::inline_parser::InlineParser;
    use crate::syntax::SyntaxKind;

    fn parse_inline(input: &str) -> crate::syntax::SyntaxNode {
        let config = Config::default();
        let (block_tree, registry) = BlockParser::new(input, &config).parse();
        InlineParser::new(block_tree, config, registry).parse()
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
    use crate::config::Config;
    use crate::parser::block_parser::BlockParser;
    use crate::parser::inline_parser::InlineParser;
    use crate::syntax::SyntaxKind;

    fn parse_inline(input: &str) -> crate::syntax::SyntaxNode {
        let config = Config::default();
        let (block_tree, registry) = BlockParser::new(input, &config).parse();
        InlineParser::new(block_tree, config, registry).parse()
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
    use crate::config::Config;
    use crate::parser::block_parser::BlockParser;
    use crate::parser::inline_parser::InlineParser;
    use crate::syntax::SyntaxKind;

    fn parse_with_refs(input: &str) -> crate::syntax::SyntaxNode {
        let config = Config::default();
        let (block_tree, registry) = BlockParser::new(input, &config).parse();
        InlineParser::new(block_tree, config, registry).parse()
    }

    #[test]
    fn test_reference_image_explicit() {
        let input = "Text with ![alt text][img-ref] image.

[img-ref]: image.jpg \"Image Title\"";

        let config = Config::default();
        let (block_tree, registry) = BlockParser::new(input, &config).parse();

        eprintln!("Has img-ref: {}", registry.get("img-ref").is_some());

        let para = block_tree.first_child().unwrap().first_child().unwrap();
        eprintln!("\nParagraph children:");
        for child in para.children_with_tokens() {
            match child {
                rowan::NodeOrToken::Node(n) => {
                    eprintln!("  Node {:?}: '{}'", n.kind(), n.text());
                }
                rowan::NodeOrToken::Token(t) => {
                    eprintln!("  Token {:?}: '{}'", t.kind(), t.text());
                }
            }
        }

        let parsed = InlineParser::new(block_tree, config, registry).parse();

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
    use crate::config::Config;
    use crate::parser::block_parser::BlockParser;
    use crate::parser::inline_parser::InlineParser;
    use crate::syntax::SyntaxKind;

    fn parse_inline(input: &str) -> crate::syntax::SyntaxNode {
        let config = Config::default();
        let (block_tree, registry) = BlockParser::new(input, &config).parse();
        InlineParser::new(block_tree, config, registry).parse()
    }

    fn parse_inline_with_config(input: &str, config: Config) -> crate::syntax::SyntaxNode {
        let (block_tree, registry) = BlockParser::new(input, &config).parse();
        InlineParser::new(block_tree, config, registry).parse()
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
        let mut config = Config::default();
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

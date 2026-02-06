// Tests for inline parser functionality
// These tests will be expanded as we implement inline parsing features

#[cfg(test)]
mod emphasis_tests {
    use crate::block_parser::BlockParser;
    use crate::inline_parser::InlineParser;
    use crate::syntax::SyntaxKind;

    fn find_emphasis(node: &crate::syntax::SyntaxNode) -> Vec<String> {
        let mut emphasis = Vec::new();
        for child in node.descendants() {
            if child.kind() == SyntaxKind::Emphasis {
                emphasis.push(child.to_string());
            }
        }
        emphasis
    }

    fn find_strong(node: &crate::syntax::SyntaxNode) -> Vec<String> {
        let mut strong = Vec::new();
        for child in node.descendants() {
            if child.kind() == SyntaxKind::Strong {
                strong.push(child.to_string());
            }
        }
        strong
    }

    #[test]
    fn test_simple_emphasis() {
        let input = "This is *italic* text.";
        let block_tree = BlockParser::new(input).parse();
        let inline_tree = InlineParser::new(block_tree).parse();

        let emphasis = find_emphasis(&inline_tree);
        assert_eq!(emphasis.len(), 1);
        assert_eq!(emphasis[0], "*italic*");
    }

    #[test]
    fn test_simple_strong() {
        let input = "This is **bold** text.";
        let block_tree = BlockParser::new(input).parse();
        let inline_tree = InlineParser::new(block_tree).parse();

        let strong = find_strong(&inline_tree);
        assert_eq!(strong.len(), 1);
        assert_eq!(strong[0], "**bold**");
    }

    #[test]
    fn test_multiple_emphasis() {
        let input = "Both *foo* and *bar* are italic.";
        let block_tree = BlockParser::new(input).parse();
        let inline_tree = InlineParser::new(block_tree).parse();

        let emphasis = find_emphasis(&inline_tree);
        assert_eq!(emphasis.len(), 2);
        assert_eq!(emphasis[0], "*foo*");
        assert_eq!(emphasis[1], "*bar*");
    }

    #[test]
    fn test_mixed_emphasis_and_strong() {
        let input = "Mix *italic* and **bold** together.";
        let block_tree = BlockParser::new(input).parse();
        let inline_tree = InlineParser::new(block_tree).parse();

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
        let block_tree = BlockParser::new(input).parse();
        let inline_tree = InlineParser::new(block_tree).parse();

        // Triple emphasis creates nested Strong and Emphasis
        let strong = find_strong(&inline_tree);
        let emphasis = find_emphasis(&inline_tree);

        assert_eq!(strong.len(), 1);
        assert_eq!(emphasis.len(), 1);
    }

    #[test]
    fn test_underscore_emphasis() {
        let input = "This is _italic_ text.";
        let block_tree = BlockParser::new(input).parse();
        let inline_tree = InlineParser::new(block_tree).parse();

        let emphasis = find_emphasis(&inline_tree);
        assert_eq!(emphasis.len(), 1);
        assert_eq!(emphasis[0], "_italic_");
    }

    #[test]
    fn test_intraword_underscore_no_emphasis() {
        let input = "This is feas_ible text.";
        let block_tree = BlockParser::new(input).parse();
        let inline_tree = InlineParser::new(block_tree).parse();

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
        let block_tree = BlockParser::new(input).parse();
        let inline_tree = InlineParser::new(block_tree).parse();

        let emphasis = find_emphasis(&inline_tree);
        assert_eq!(emphasis.len(), 0);
    }

    #[test]
    fn test_no_emphasis() {
        let input = "Plain text with no emphasis.";
        let block_tree = BlockParser::new(input).parse();
        let inline_tree = InlineParser::new(block_tree).parse();

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
    use crate::block_parser::BlockParser;
    use crate::inline_parser::InlineParser;
    use crate::syntax::SyntaxKind;

    fn find_code_spans(node: &crate::syntax::SyntaxNode) -> Vec<String> {
        let mut code_spans = Vec::new();
        for child in node.descendants() {
            if child.kind() == SyntaxKind::CodeSpan {
                code_spans.push(child.to_string());
            }
        }
        code_spans
    }

    #[test]
    fn test_simple_code_span() {
        let input = "This has `code` in it.";
        let block_tree = BlockParser::new(input).parse();
        let inline_tree = InlineParser::new(block_tree).parse();

        let code_spans = find_code_spans(&inline_tree);
        assert_eq!(code_spans.len(), 1);
        assert_eq!(code_spans[0], "`code`");
    }

    #[test]
    fn test_multiple_code_spans() {
        let input = "Both `foo` and `bar` are code.";
        let block_tree = BlockParser::new(input).parse();
        let inline_tree = InlineParser::new(block_tree).parse();

        let code_spans = find_code_spans(&inline_tree);
        assert_eq!(code_spans.len(), 2);
        assert_eq!(code_spans[0], "`foo`");
        assert_eq!(code_spans[1], "`bar`");
    }

    #[test]
    fn test_code_span_with_backticks() {
        let input = "Use `` `backtick` `` for literal backticks.";
        let block_tree = BlockParser::new(input).parse();
        let inline_tree = InlineParser::new(block_tree).parse();

        let code_spans = find_code_spans(&inline_tree);
        assert_eq!(code_spans.len(), 1);
        assert_eq!(code_spans[0], "`` `backtick` ``");
    }

    #[test]
    fn test_no_code_spans() {
        let input = "Plain text with no code.";
        let block_tree = BlockParser::new(input).parse();
        let inline_tree = InlineParser::new(block_tree).parse();

        let code_spans = find_code_spans(&inline_tree);
        assert_eq!(code_spans.len(), 0);
    }
}

#[cfg(test)]
mod math_tests {
    use crate::block_parser::BlockParser;
    use crate::inline_parser::InlineParser;
    use crate::syntax::SyntaxKind;

    fn find_inline_math(node: &crate::syntax::SyntaxNode) -> Vec<String> {
        let mut math = Vec::new();
        for child in node.descendants() {
            if child.kind() == SyntaxKind::InlineMath {
                math.push(child.to_string());
            }
        }
        math
    }

    #[test]
    fn test_simple_inline_math() {
        let input = "This has $x = y$ in it.";
        let block_tree = BlockParser::new(input).parse();
        let inline_tree = InlineParser::new(block_tree).parse();

        let math = find_inline_math(&inline_tree);
        assert_eq!(math.len(), 1);
        assert_eq!(math[0], "$x = y$");
    }

    #[test]
    fn test_multiple_inline_math() {
        let input = "Both $a$ and $b$ are variables.";
        let block_tree = BlockParser::new(input).parse();
        let inline_tree = InlineParser::new(block_tree).parse();

        let math = find_inline_math(&inline_tree);
        assert_eq!(math.len(), 2);
        assert_eq!(math[0], "$a$");
        assert_eq!(math[1], "$b$");
    }

    #[test]
    fn test_inline_math_complex() {
        let input = r"The formula $\frac{1}{2}$ is simple.";
        let block_tree = BlockParser::new(input).parse();
        let inline_tree = InlineParser::new(block_tree).parse();

        let math = find_inline_math(&inline_tree);
        assert_eq!(math.len(), 1);
        assert_eq!(math[0], r"$\frac{1}{2}$");
    }

    #[test]
    fn test_no_inline_math() {
        let input = "Plain text with no math.";
        let block_tree = BlockParser::new(input).parse();
        let inline_tree = InlineParser::new(block_tree).parse();

        let math = find_inline_math(&inline_tree);
        assert_eq!(math.len(), 0);
    }

    #[test]
    fn test_mixed_code_and_math() {
        let input = "Code `x` and math $y$ together.";
        let block_tree = BlockParser::new(input).parse();
        let inline_tree = InlineParser::new(block_tree).parse();

        let math = find_inline_math(&inline_tree);
        assert_eq!(math.len(), 1);
        assert_eq!(math[0], "$y$");
    }
}

#[cfg(test)]
mod escape_tests {
    use crate::block_parser::BlockParser;
    use crate::inline_parser::InlineParser;
    use crate::syntax::SyntaxKind;

    fn parse_inline(input: &str) -> crate::syntax::SyntaxNode {
        let block_tree = BlockParser::new(input).parse();
        InlineParser::new(block_tree).parse()
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

        let escaped = count_nodes_of_kind(&tree, SyntaxKind::EscapedChar);
        assert_eq!(escaped, 2, "Should have two escaped asterisks");
    }

    #[test]
    fn test_escaped_backtick() {
        let input = r"This is \`not code\`.";
        let tree = parse_inline(input);

        let escaped = count_nodes_of_kind(&tree, SyntaxKind::EscapedChar);
        let code_spans = count_nodes_of_kind(&tree, SyntaxKind::CodeSpan);

        assert_eq!(escaped, 2, "Should have two escaped backticks");
        assert_eq!(code_spans, 0, "Should not create code span");
    }

    #[test]
    fn test_escaped_dollar() {
        let input = r"Price is \$5.";
        let tree = parse_inline(input);

        let escaped = count_nodes_of_kind(&tree, SyntaxKind::EscapedChar);
        let math = count_nodes_of_kind(&tree, SyntaxKind::InlineMath);

        assert_eq!(escaped, 1, "Should have one escaped dollar");
        assert_eq!(math, 0, "Should not create math");
    }

    #[test]
    fn test_nonbreaking_space() {
        let input = r"word1\ word2";
        let tree = parse_inline(input);

        let nbsp = count_nodes_of_kind(&tree, SyntaxKind::NonbreakingSpace);
        assert_eq!(nbsp, 1, "Should have one nonbreaking space");
    }

    #[test]
    #[ignore = "Hard line breaks span token boundaries - needs block parser support"]
    fn test_hard_line_break() {
        // TODO: Backslash-newline escapes require coordination with block parser
        // The backslash is in TEXT token, newline is in NEWLINE token
        // Need to handle this at block parsing level or with token lookahead
        let input = "line1\\\nline2";
        let tree = parse_inline(input);

        let hard_break = count_nodes_of_kind(&tree, SyntaxKind::HardLineBreak);
        assert_eq!(hard_break, 1, "Should have one hard line break");
    }

    #[test]
    fn test_escape_prevents_code_span() {
        let input = r"\`not code\`";
        let tree = parse_inline(input);

        let code_spans = count_nodes_of_kind(&tree, SyntaxKind::CodeSpan);
        assert_eq!(code_spans, 0, "Escaped backticks should prevent code span");
    }

    #[test]
    fn test_escape_prevents_math() {
        let input = r"\$not math\$";
        let tree = parse_inline(input);

        let math = count_nodes_of_kind(&tree, SyntaxKind::InlineMath);
        assert_eq!(math, 0, "Escaped dollars should prevent math");
    }

    #[test]
    fn test_escape_inside_code_span_not_processed() {
        // Per spec: "Backslash escapes do not work in verbatim contexts"
        let input = r"`\*code\*`";
        let tree = parse_inline(input);

        let code_spans = count_nodes_of_kind(&tree, SyntaxKind::CodeSpan);
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

        let escaped = count_nodes_of_kind(&tree, SyntaxKind::EscapedChar);
        assert_eq!(escaped, 3, "Should have three escaped characters");
    }

    #[test]
    fn test_backslash_not_before_escapable() {
        // Backslash before non-escapable character stays as-is
        let input = r"\a normal text";
        let tree = parse_inline(input);

        let escaped = count_nodes_of_kind(&tree, SyntaxKind::EscapedChar);
        assert_eq!(escaped, 0, "Should not escape letter 'a'");

        // The backslash should remain in output
        let output = tree.to_string();
        assert!(
            output.contains(r"\a"),
            "Backslash before letter should remain"
        );
    }
}

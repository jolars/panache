// Tests for inline parser functionality
// These tests will be expanded as we implement inline parsing features

#[cfg(test)]
mod emphasis_tests {
    // TODO: Add tests for emphasis parsing (*text*, **text**, _text_, __text__)
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
    // TODO: Add tests for inline math parsing ($math$)
}

#[cfg(test)]
mod escape_tests {
    // TODO: Add tests for escape sequence parsing (\*)
}

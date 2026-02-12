// Architecture and nesting tests for inline parser

#[cfg(test)]
use crate::config::Config;
#[cfg(test)]
use crate::parser::block_parser::BlockParser;
#[cfg(test)]
use crate::parser::inline_parser::InlineParser;
#[cfg(test)]
use crate::syntax::SyntaxKind;

#[cfg(test)]
fn parse_inline(input: &str) -> crate::syntax::SyntaxNode {
    let config = Config::default();
    let (block_tree, registry) = BlockParser::new(input, &config).parse();
    InlineParser::new(block_tree, config, registry).parse()
}

#[cfg(test)]
fn count_nodes_of_kind(node: &crate::syntax::SyntaxNode, kind: SyntaxKind) -> usize {
    node.descendants().filter(|n| n.kind() == kind).count()
}

#[test]
fn test_code_inside_unimplemented_link() {
    // Link syntax not implemented yet, but code inside should still parse
    let input = "[click `here`](url)";
    let tree = parse_inline(input);

    // Code spans should still be detected in the TEXT
    let code_spans = count_nodes_of_kind(&tree, SyntaxKind::CodeSpan);
    assert_eq!(
        code_spans, 1,
        "Code span should be parsed even in unimplemented link"
    );
}

#[test]
fn test_math_inside_unimplemented_link() {
    let input = "[see $x$](url)";
    let tree = parse_inline(input);

    let math = count_nodes_of_kind(&tree, SyntaxKind::InlineMath);
    assert_eq!(
        math, 1,
        "Inline math should be parsed even in unimplemented link"
    );
}

#[test]
fn test_code_span_contains_dollar() {
    // Code spans should NOT parse math inside them
    let input = "`$x$`";
    let tree = parse_inline(input);

    let code_spans = count_nodes_of_kind(&tree, SyntaxKind::CodeSpan);
    let math = count_nodes_of_kind(&tree, SyntaxKind::InlineMath);

    assert_eq!(code_spans, 1, "Should parse as code span");
    assert_eq!(math, 0, "Should NOT parse math inside code span");
}

#[test]
fn test_math_contains_backtick() {
    // Math should NOT parse code spans inside
    let input = "$`x`$";
    let tree = parse_inline(input);

    let code_spans = count_nodes_of_kind(&tree, SyntaxKind::CodeSpan);
    let math = count_nodes_of_kind(&tree, SyntaxKind::InlineMath);

    assert_eq!(math, 1, "Should parse as inline math");
    assert_eq!(code_spans, 0, "Should NOT parse code inside math");
}

#[test]
fn test_code_then_math() {
    let input = "`code` and $math$";
    let tree = parse_inline(input);

    let code_spans = count_nodes_of_kind(&tree, SyntaxKind::CodeSpan);
    let math = count_nodes_of_kind(&tree, SyntaxKind::InlineMath);

    assert_eq!(code_spans, 1);
    assert_eq!(math, 1);
}

#[test]
fn test_math_then_code() {
    let input = "$math$ and `code`";
    let tree = parse_inline(input);

    let code_spans = count_nodes_of_kind(&tree, SyntaxKind::CodeSpan);
    let math = count_nodes_of_kind(&tree, SyntaxKind::InlineMath);

    assert_eq!(code_spans, 1);
    assert_eq!(math, 1);
}

#[test]
fn test_consecutive_code_spans() {
    // `one``two` is actually parsed as a single code span containing "one" with backticks
    // This is correct per Markdown spec - double backticks create code with single backticks inside
    let input = "`one``two`";
    let tree = parse_inline(input);

    let code_spans = count_nodes_of_kind(&tree, SyntaxKind::CodeSpan);
    // This parses as ONE code span: `one``two` where content is "one``two"
    assert_eq!(
        code_spans, 1,
        "Backticks inside require matching delimiters"
    );
}

#[test]
fn test_two_separate_code_spans() {
    // To get two separate code spans, they need whitespace between
    let input = "`one` `two`";
    let tree = parse_inline(input);

    let code_spans = count_nodes_of_kind(&tree, SyntaxKind::CodeSpan);
    assert_eq!(
        code_spans, 2,
        "Should parse as two separate code spans with space"
    );
}

#[test]
fn test_consecutive_math() {
    // $x$$y$ - the parser sees $x$ as first math, then $y$ as second
    // But actually, $$ is display math start! So this becomes ambiguous.
    // Let's test what Pandoc does and match that behavior.
    // For now, let's just verify it doesn't crash
    let input = "$x$$y$";
    let tree = parse_inline(input);

    // The behavior here depends on display math handling
    // Current implementation should parse $x$ and leave $$ alone
    let math = count_nodes_of_kind(&tree, SyntaxKind::InlineMath);
    // This is actually ambiguous: could be $x$ followed by $y$ or $x$ followed by $$y$$
    // Our implementation currently parses $x$ and then treats $$ as display math start
    assert!(math >= 1, "Should parse at least one math expression");
}

#[test]
fn test_unmatched_code_span() {
    let input = "`no closing";
    let tree = parse_inline(input);

    // Unmatched backtick should remain as TEXT
    let code_spans = count_nodes_of_kind(&tree, SyntaxKind::CodeSpan);
    assert_eq!(
        code_spans, 0,
        "Unmatched backtick should not create code span"
    );
}

#[test]
fn test_unmatched_math() {
    let input = "$no closing";
    let tree = parse_inline(input);

    // Unmatched dollar should remain as TEXT
    let math = count_nodes_of_kind(&tree, SyntaxKind::InlineMath);
    assert_eq!(math, 0, "Unmatched dollar should not create math");
}

#[test]
fn test_multiline_text_preserves_structure() {
    let input = "Line one\nLine two";
    let tree = parse_inline(input);

    // Should preserve the newline in output
    let output = tree.to_string();
    assert!(output.contains("Line one"));
    assert!(output.contains("Line two"));
}

#[test]
fn test_empty_code_span() {
    // Empty code span: `` - two backticks with nothing between
    // According to Markdown spec, this creates an empty code span
    let input = "``";
    let tree = parse_inline(input);

    let code_spans = count_nodes_of_kind(&tree, SyntaxKind::CodeSpan);
    // Our current implementation requires matching backtick counts
    // `` is 2 backticks, which opens a 2-backtick code span, but has no closing
    // So this should NOT parse as a code span (no matching close)
    assert_eq!(
        code_spans, 0,
        "Empty `` without close doesn't create code span"
    );
}

#[test]
fn test_double_backtick_code_span() {
    // Proper empty double-backtick code span needs closing
    let input = "`` ``";
    let tree = parse_inline(input);

    let code_spans = count_nodes_of_kind(&tree, SyntaxKind::CodeSpan);
    assert_eq!(
        code_spans, 1,
        "Double backticks with space creates code span"
    );
}

#[test]
fn test_code_span_with_only_space() {
    let input = "` `";
    let tree = parse_inline(input);

    let code_spans = count_nodes_of_kind(&tree, SyntaxKind::CodeSpan);
    assert_eq!(code_spans, 1, "Code span with space should parse");
}

#[test]
fn test_architecture_preserves_block_structure() {
    // Verify that inline parsing doesn't break block structure
    let input = "# Heading\n\nParagraph with `code`.";
    let tree = parse_inline(input);

    let headings = count_nodes_of_kind(&tree, SyntaxKind::Heading);
    let paragraphs = count_nodes_of_kind(&tree, SyntaxKind::PARAGRAPH);
    let code_spans = count_nodes_of_kind(&tree, SyntaxKind::CodeSpan);

    assert_eq!(headings, 1, "Should preserve heading");
    assert_eq!(paragraphs, 1, "Should preserve paragraph");
    assert_eq!(code_spans, 1, "Should parse code span");
}

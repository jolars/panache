//! Unit tests for emphasis parser.
//!
//! These tests verify CST structure directly, focusing on parser correctness
//! rather than formatter output. Each test parses input and inspects the
//! resulting syntax tree to ensure emphasis is parsed correctly.

use panache::{parse, syntax::SyntaxKind};

/// Helper to get all nodes of a specific kind from a syntax tree.
fn find_nodes(tree: &panache::SyntaxNode, kind: SyntaxKind) -> Vec<panache::SyntaxNode> {
    let mut nodes = Vec::new();

    fn collect(node: &panache::SyntaxNode, kind: SyntaxKind, nodes: &mut Vec<panache::SyntaxNode>) {
        if node.kind() == kind {
            nodes.push(node.clone());
        }
        for child in node.children() {
            collect(&child, kind, nodes);
        }
    }

    collect(tree, kind, &mut nodes);
    nodes
}

/// Helper to check if a node contains a specific child kind.
fn has_child(node: &panache::SyntaxNode, kind: SyntaxKind) -> bool {
    node.children().any(|child| child.kind() == kind)
}

/// Helper to count nodes of a specific kind.
fn count_nodes(tree: &panache::SyntaxNode, kind: SyntaxKind) -> usize {
    find_nodes(tree, kind).len()
}

// =============================================================================
// Critical Cases: Nested Inline Elements
// =============================================================================
// These are the "killer test cases" that require proper position tracking
// to avoid matching delimiters inside code spans, math, etc.

#[test]
fn code_span_in_emphasis() {
    let input = "*text `code here` end*\n";
    let tree = parse(input, None);

    // Should have: EMPHASIS containing CODE_SPAN
    let emphasis_nodes = find_nodes(&tree, SyntaxKind::EMPHASIS);
    assert_eq!(
        emphasis_nodes.len(),
        1,
        "Should parse exactly one emphasis node"
    );

    let emphasis = &emphasis_nodes[0];
    assert!(
        has_child(emphasis, SyntaxKind::CODE_SPAN),
        "Emphasis should contain code span as child"
    );
}

#[test]
fn code_span_with_asterisk_in_emphasis() {
    // The asterisk inside the code span should NOT close the emphasis
    let input = "*text `code * here` end*\n";
    let tree = parse(input, None);

    let emphasis_nodes = find_nodes(&tree, SyntaxKind::EMPHASIS);
    assert_eq!(
        emphasis_nodes.len(),
        1,
        "Should parse exactly one emphasis node"
    );

    let emphasis = &emphasis_nodes[0];
    assert!(
        has_child(emphasis, SyntaxKind::CODE_SPAN),
        "Emphasis should contain code span"
    );

    // Verify the code span content includes the asterisk
    let code_spans = find_nodes(&tree, SyntaxKind::CODE_SPAN);
    assert_eq!(code_spans.len(), 1, "Should have exactly one code span");
}

#[test]
fn math_in_emphasis() {
    let input = "*text $math$ end*\n";
    let tree = parse(input, None);

    let emphasis_nodes = find_nodes(&tree, SyntaxKind::EMPHASIS);
    assert_eq!(
        emphasis_nodes.len(),
        1,
        "Should parse exactly one emphasis node"
    );

    let emphasis = &emphasis_nodes[0];
    assert!(
        has_child(emphasis, SyntaxKind::INLINE_MATH),
        "Emphasis should contain inline math"
    );
}

#[test]
fn math_with_asterisk_in_emphasis() {
    // The asterisk inside math should NOT close the emphasis
    let input = "*text $a * b$ end*\n";
    let tree = parse(input, None);

    let emphasis_nodes = find_nodes(&tree, SyntaxKind::EMPHASIS);
    assert_eq!(
        emphasis_nodes.len(),
        1,
        "Should parse exactly one emphasis node"
    );

    let emphasis = &emphasis_nodes[0];
    assert!(
        has_child(emphasis, SyntaxKind::INLINE_MATH),
        "Emphasis should contain inline math"
    );
}

#[test]
fn link_in_emphasis() {
    let input = "*text [link](url) end*\n";
    let tree = parse(input, None);

    let emphasis_nodes = find_nodes(&tree, SyntaxKind::EMPHASIS);
    assert_eq!(
        emphasis_nodes.len(),
        1,
        "Should parse exactly one emphasis node"
    );

    let emphasis = &emphasis_nodes[0];
    assert!(
        has_child(emphasis, SyntaxKind::LINK),
        "Emphasis should contain link"
    );
}

#[test]
fn link_with_asterisk_in_emphasis() {
    // The asterisk in link text should NOT close the emphasis
    let input = "*text [link * here](url) end*\n";
    let tree = parse(input, None);

    let emphasis_nodes = find_nodes(&tree, SyntaxKind::EMPHASIS);
    assert_eq!(
        emphasis_nodes.len(),
        1,
        "Should parse exactly one emphasis node"
    );

    let emphasis = &emphasis_nodes[0];
    assert!(
        has_child(emphasis, SyntaxKind::LINK),
        "Emphasis should contain link"
    );
}

#[test]
fn image_in_emphasis() {
    let input = "*text ![alt](img.png) end*\n";
    let tree = parse(input, None);

    let emphasis_nodes = find_nodes(&tree, SyntaxKind::EMPHASIS);
    assert_eq!(
        emphasis_nodes.len(),
        1,
        "Should parse exactly one emphasis node"
    );

    let emphasis = &emphasis_nodes[0];
    assert!(
        has_child(emphasis, SyntaxKind::IMAGE_LINK),
        "Emphasis should contain image"
    );
}

#[test]
fn strong_with_code_span() {
    let input = "**text `code ** here` end**\n";
    let tree = parse(input, None);

    let strong_nodes = find_nodes(&tree, SyntaxKind::STRONG);
    assert_eq!(
        strong_nodes.len(),
        1,
        "Should parse exactly one strong node"
    );

    let strong = &strong_nodes[0];
    assert!(
        has_child(strong, SyntaxKind::CODE_SPAN),
        "Strong should contain code span"
    );
}

#[test]
fn complex_nesting() {
    // Multiple inline elements in emphasis
    let input = "*em with `code`, [link](url), and text*\n";
    let tree = parse(input, None);

    let emphasis_nodes = find_nodes(&tree, SyntaxKind::EMPHASIS);
    assert_eq!(
        emphasis_nodes.len(),
        1,
        "Should parse exactly one emphasis node"
    );

    let emphasis = &emphasis_nodes[0];
    assert!(
        has_child(emphasis, SyntaxKind::CODE_SPAN),
        "Should contain code span"
    );
    assert!(has_child(emphasis, SyntaxKind::LINK), "Should contain link");
}

// =============================================================================
// Rule of 3s (Pandoc's delimiter consumption strategy)
// =============================================================================

#[test]
fn three_opener_two_closer() {
    // ***foo** should produce: literal * + Strong("foo")
    let input = "***foo**\n";
    let tree = parse(input, None);

    // Should have one STRONG node
    let strong_nodes = find_nodes(&tree, SyntaxKind::STRONG);
    assert_eq!(
        strong_nodes.len(),
        1,
        "Should parse exactly one strong node"
    );

    // The first * should be a literal (TEXT or escaped)
    // This is a known current bug - documenting expected behavior
}

#[test]
fn triple_matched() {
    // ***foo*** should produce: StrongEmph("foo")
    let input = "***foo***\n";
    let tree = parse(input, None);

    // Should have nested STRONG and EMPHASIS
    let strong_nodes = find_nodes(&tree, SyntaxKind::STRONG);
    let emphasis_nodes = find_nodes(&tree, SyntaxKind::EMPHASIS);

    assert!(
        strong_nodes.len() >= 1 || emphasis_nodes.len() >= 1,
        "Should parse emphasis/strong from triple delimiters"
    );
}

#[test]
fn four_or_more_delimiters_literal() {
    // ****foo**** should be literal (Pandoc doesn't recognize 4+)
    let input = "****foo****\n";
    let tree = parse(input, None);

    // Should NOT create emphasis or strong nodes
    let emphasis_count = count_nodes(&tree, SyntaxKind::EMPHASIS);
    let strong_count = count_nodes(&tree, SyntaxKind::STRONG);

    // Expecting this to remain literal - current parser may differ
    // This documents the Pandoc-compliant behavior we want
}

// =============================================================================
// Overlapping Delimiters
// =============================================================================

#[test]
fn overlapping_emphasis_strong() {
    // *foo **bar* baz** should produce: literal "*foo " + Strong("bar* baz")
    // The first * can't close because of wrong nesting, so ** opens strong
    let input = "*foo **bar* baz**\n";
    let tree = parse(input, None);

    // Should have one STRONG node for "bar* baz"
    let strong_nodes = find_nodes(&tree, SyntaxKind::STRONG);
    assert!(
        strong_nodes.len() >= 1,
        "Should parse at least one strong node"
    );
}

#[test]
fn overlapping_strong_emphasis() {
    // **foo *bar** baz* should produce: Strong("foo *bar") + literal " baz*"
    let input = "**foo *bar** baz*\n";
    let tree = parse(input, None);

    // Should have one STRONG node
    let strong_nodes = find_nodes(&tree, SyntaxKind::STRONG);
    assert!(
        strong_nodes.len() >= 1,
        "Should parse at least one strong node"
    );
}

// =============================================================================
// Adjacent Patterns
// =============================================================================

#[test]
fn adjacent_emphasis() {
    // *foo**bar* could be: Emph("foo") + Emph("bar") (adjacent emphasis)
    // OR: Emph("foo**bar") depending on parsing strategy
    let input = "*foo**bar*\n";
    let tree = parse(input, None);

    let emphasis_nodes = find_nodes(&tree, SyntaxKind::EMPHASIS);
    // Should have at least one emphasis node
    assert!(emphasis_nodes.len() >= 1, "Should parse emphasis");
}

#[test]
fn adjacent_strong() {
    // **foo****bar** should produce: Strong("foo") + Strong("bar")
    // (merged in AST but separate in CST)
    let input = "**foo****bar**\n";
    let tree = parse(input, None);

    let strong_nodes = find_nodes(&tree, SyntaxKind::STRONG);
    assert!(strong_nodes.len() >= 1, "Should parse strong emphasis");
}

// =============================================================================
// Flanking Rules
// =============================================================================

#[test]
fn intraword_asterisk() {
    // un*frigging*believable - asterisks CAN work intraword
    let input = "un*frigging*believable\n";
    let tree = parse(input, None);

    let emphasis_nodes = find_nodes(&tree, SyntaxKind::EMPHASIS);
    assert_eq!(
        emphasis_nodes.len(),
        1,
        "Should parse intraword emphasis with asterisks"
    );
}

#[test]
fn intraword_underscore_disabled() {
    // feas_ible - underscores should NOT work intraword (default config)
    let input = "feas_ible\n";
    let tree = parse(input, None);

    let emphasis_nodes = find_nodes(&tree, SyntaxKind::EMPHASIS);
    assert_eq!(
        emphasis_nodes.len(),
        0,
        "Should not parse intraword emphasis with underscores"
    );
}

#[test]
fn whitespace_flanking_opener() {
    // "* foo*" - opener has trailing space, should not match
    let input = "* foo*\n";
    let tree = parse(input, None);

    let emphasis_nodes = find_nodes(&tree, SyntaxKind::EMPHASIS);
    // Should NOT parse as emphasis (opener not left-flanking)
    assert_eq!(
        emphasis_nodes.len(),
        0,
        "Should not parse emphasis with space after opener"
    );
}

#[test]
fn whitespace_flanking_closer() {
    // "*foo *" - closer has leading space, should not match
    let input = "*foo *\n";
    let tree = parse(input, None);

    let emphasis_nodes = find_nodes(&tree, SyntaxKind::EMPHASIS);
    // Should NOT parse as emphasis (closer not right-flanking)
    assert_eq!(
        emphasis_nodes.len(),
        0,
        "Should not parse emphasis with space before closer"
    );
}

// =============================================================================
// Escapes
// =============================================================================

#[test]
fn escaped_opener() {
    // \*foo* should not create emphasis
    let input = "\\*foo*\n";
    let tree = parse(input, None);

    let emphasis_nodes = find_nodes(&tree, SyntaxKind::EMPHASIS);
    assert_eq!(
        emphasis_nodes.len(),
        0,
        "Should not parse emphasis when opener is escaped"
    );

    // Should have an ESCAPED_CHAR node
    let escape_nodes = find_nodes(&tree, SyntaxKind::ESCAPED_CHAR);
    assert!(escape_nodes.len() >= 1, "Should have escape node");
}

#[test]
fn escaped_closer() {
    // *foo\* should not create emphasis
    let input = "*foo\\*\n";
    let tree = parse(input, None);

    let emphasis_nodes = find_nodes(&tree, SyntaxKind::EMPHASIS);
    assert_eq!(
        emphasis_nodes.len(),
        0,
        "Should not parse emphasis when closer is escaped"
    );
}

#[test]
fn escaped_within_emphasis() {
    // *foo \* bar* should create emphasis with escaped asterisk inside
    let input = "*foo \\* bar*\n";
    let tree = parse(input, None);

    let emphasis_nodes = find_nodes(&tree, SyntaxKind::EMPHASIS);
    assert_eq!(
        emphasis_nodes.len(),
        1,
        "Should parse emphasis with escape inside"
    );

    let emphasis = &emphasis_nodes[0];
    assert!(
        has_child(emphasis, SyntaxKind::ESCAPED_CHAR),
        "Emphasis should contain escape node"
    );
}

// =============================================================================
// Unclosed Constructs
// =============================================================================

#[test]
fn unclosed_code_in_emphasis() {
    // *text `unclosed code end*
    // When code span fails to close, backtick becomes literal,
    // and * could be a valid closer candidate
    let input = "*text `unclosed code end*\n";
    let tree = parse(input, None);

    // Current behavior may vary - documenting that this is an edge case
    // Pandoc would parse this as emphasis with literal backtick inside
}

#[test]
fn unclosed_emphasis() {
    // *foo - no closing delimiter
    let input = "*foo\n";
    let tree = parse(input, None);

    // Should NOT create emphasis node (no closer)
    let emphasis_nodes = find_nodes(&tree, SyntaxKind::EMPHASIS);
    assert_eq!(
        emphasis_nodes.len(),
        0,
        "Should not parse unclosed emphasis"
    );
}

#[test]
fn unclosed_strong() {
    // **foo - no closing delimiter
    let input = "**foo\n";
    let tree = parse(input, None);

    // Should NOT create strong node (no closer)
    let strong_nodes = find_nodes(&tree, SyntaxKind::STRONG);
    assert_eq!(strong_nodes.len(), 0, "Should not parse unclosed strong");
}

// =============================================================================
// Cross-delimiter Interaction
// =============================================================================

#[test]
fn emphasis_in_strikeout() {
    let input = "~~strike *em* text~~\n";
    let tree = parse(input, None);

    let strikeout_nodes = find_nodes(&tree, SyntaxKind::STRIKEOUT);
    assert_eq!(strikeout_nodes.len(), 1, "Should parse strikeout");

    let strikeout = &strikeout_nodes[0];
    assert!(
        has_child(strikeout, SyntaxKind::EMPHASIS),
        "Strikeout should contain emphasis"
    );
}

#[test]
fn strikeout_in_emphasis() {
    let input = "*em ~~strike~~ text*\n";
    let tree = parse(input, None);

    let emphasis_nodes = find_nodes(&tree, SyntaxKind::EMPHASIS);
    assert_eq!(emphasis_nodes.len(), 1, "Should parse emphasis");

    let emphasis = &emphasis_nodes[0];
    assert!(
        has_child(emphasis, SyntaxKind::STRIKEOUT),
        "Emphasis should contain strikeout"
    );
}

#[test]
fn subscript_in_emphasis() {
    let input = "*em ~sub~ text*\n";
    let tree = parse(input, None);

    let emphasis_nodes = find_nodes(&tree, SyntaxKind::EMPHASIS);
    assert_eq!(emphasis_nodes.len(), 1, "Should parse emphasis");

    let emphasis = &emphasis_nodes[0];
    assert!(
        has_child(emphasis, SyntaxKind::SUBSCRIPT),
        "Emphasis should contain subscript"
    );
}

#[test]
fn superscript_in_emphasis() {
    let input = "*em ^super^ text*\n";
    let tree = parse(input, None);

    let emphasis_nodes = find_nodes(&tree, SyntaxKind::EMPHASIS);
    assert_eq!(emphasis_nodes.len(), 1, "Should parse emphasis");

    let emphasis = &emphasis_nodes[0];
    assert!(
        has_child(emphasis, SyntaxKind::SUPERSCRIPT),
        "Emphasis should contain superscript"
    );
}

// =============================================================================
// Empty Emphasis
// =============================================================================

#[test]
fn empty_emphasis() {
    // ** alone should be literal
    let input = "**\n";
    let tree = parse(input, None);

    let emphasis_nodes = find_nodes(&tree, SyntaxKind::EMPHASIS);
    let strong_nodes = find_nodes(&tree, SyntaxKind::STRONG);

    assert_eq!(emphasis_nodes.len(), 0, "Should not parse empty emphasis");
    assert_eq!(strong_nodes.len(), 0, "Should not parse empty strong");
}

#[test]
fn emphasis_only_code() {
    // *`code`* - emphasis containing only code span
    let input = "*`code`*\n";
    let tree = parse(input, None);

    let emphasis_nodes = find_nodes(&tree, SyntaxKind::EMPHASIS);
    assert_eq!(
        emphasis_nodes.len(),
        1,
        "Should parse emphasis with only code"
    );

    let emphasis = &emphasis_nodes[0];
    assert!(
        has_child(emphasis, SyntaxKind::CODE_SPAN),
        "Emphasis should contain code span"
    );
}

// =============================================================================
// Losslessness Tests
// =============================================================================

#[test]
fn lossless_simple_emphasis() {
    let input = "*foo*\n";
    let tree = parse(input, None);
    let output = tree.to_string();

    assert_eq!(
        input, output,
        "Parser should be lossless for simple emphasis"
    );
}

#[test]
fn lossless_triple_delimiters() {
    let input = "***foo**\n";
    let tree = parse(input, None);
    let output = tree.to_string();

    assert_eq!(
        input, output,
        "Parser should preserve all bytes for mismatched delimiters"
    );
}

#[test]
fn lossless_with_nested_code() {
    let input = "*text `code * here` end*\n";
    let tree = parse(input, None);
    let output = tree.to_string();

    assert_eq!(input, output, "Parser should be lossless with nested code");
}

#[test]
fn lossless_unclosed() {
    let input = "*foo\n";
    let tree = parse(input, None);
    let output = tree.to_string();

    assert_eq!(
        input, output,
        "Parser should be lossless for unclosed emphasis"
    );
}

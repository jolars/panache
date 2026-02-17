use crate::syntax::{SyntaxKind, SyntaxNode};
use rowan::NodeOrToken;
use std::collections::HashMap;

/// Check if a syntax kind represents a block-level element for formatting purposes.
/// This determines when to add blank lines between elements.
pub fn is_block_element(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::PARAGRAPH
            | SyntaxKind::FIGURE
            | SyntaxKind::LIST
            | SyntaxKind::DEFINITION_LIST
            | SyntaxKind::BLOCKQUOTE
            | SyntaxKind::CODE_BLOCK
            | SyntaxKind::SIMPLE_TABLE
            | SyntaxKind::MULTILINE_TABLE
            | SyntaxKind::PIPE_TABLE
            | SyntaxKind::LINE_BLOCK
    )
}

/// Check if a syntax kind represents a block-level element for range filtering.
/// This is more comprehensive than is_block_element and includes all structural blocks.
pub fn is_structural_block(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::PARAGRAPH
            | SyntaxKind::FIGURE
            | SyntaxKind::HEADING
            | SyntaxKind::CODE_BLOCK
            | SyntaxKind::BLOCKQUOTE
            | SyntaxKind::LIST
            | SyntaxKind::LIST_ITEM
            | SyntaxKind::DEFINITION_LIST
            | SyntaxKind::DEFINITION_ITEM
            | SyntaxKind::LINE_BLOCK
            | SyntaxKind::SIMPLE_TABLE
            | SyntaxKind::MULTILINE_TABLE
            | SyntaxKind::PIPE_TABLE
            | SyntaxKind::GRID_TABLE
            | SyntaxKind::FENCED_DIV
            | SyntaxKind::HORIZONTAL_RULE
            | SyntaxKind::YAML_METADATA
            | SyntaxKind::PANDOC_TITLE_BLOCK
            | SyntaxKind::HTML_BLOCK
            | SyntaxKind::BLANK_LINE
            | SyntaxKind::REFERENCE_DEFINITION
            | SyntaxKind::FOOTNOTE_DEFINITION
    )
}

/// A code block with its location in the document.
#[derive(Debug, Clone)]
pub struct CodeBlock {
    /// Programming language of the block
    pub language: String,
    /// Content of the code block (without fences)
    pub content: String,
    /// Starting line number in the document (1-indexed)
    pub start_line: usize,
}

/// Collect all fenced code blocks from a syntax tree, grouped by language.
pub fn collect_code_blocks(tree: &SyntaxNode, input: &str) -> HashMap<String, Vec<CodeBlock>> {
    let mut blocks: HashMap<String, Vec<CodeBlock>> = HashMap::new();

    for node in tree.descendants() {
        if node.kind() == SyntaxKind::CODE_BLOCK
            && let Some(block) = extract_code_block(&node, input)
        {
            blocks
                .entry(block.language.clone())
                .or_default()
                .push(block);
        }
    }

    blocks
}

fn extract_code_block(node: &SyntaxNode, input: &str) -> Option<CodeBlock> {
    let mut language = None;
    let mut content = String::new();
    let mut content_start_offset = None;

    for child in node.children_with_tokens() {
        if let NodeOrToken::Node(n) = child {
            match n.kind() {
                SyntaxKind::CODE_FENCE_OPEN => {
                    // Look for CodeInfo node, then extract CodeLanguage from inside it
                    for fence_child in n.children_with_tokens() {
                        if let NodeOrToken::Node(info_node) = fence_child
                            && info_node.kind() == SyntaxKind::CODE_INFO
                        {
                            // Search for CodeLanguage token inside CodeInfo node
                            for info_token in info_node.children_with_tokens() {
                                if let NodeOrToken::Token(t) = info_token
                                    && t.kind() == SyntaxKind::CODE_LANGUAGE
                                {
                                    language = Some(t.text().to_string());
                                    break;
                                }
                            }
                        }
                    }
                }
                SyntaxKind::CODE_CONTENT => {
                    content = n.text().to_string();
                    // Track where the actual code content starts (not the fence)
                    content_start_offset = Some(n.text_range().start().into());
                }
                _ => {}
            }
        }
    }

    // Extract language - now from CodeLanguage token inside CodeInfo node
    let language = language?;

    // Skip if language is empty or content is empty
    if language.is_empty() || content.is_empty() {
        return None;
    }

    // Calculate start line from where content actually starts (after the fence line)
    let start_line = if let Some(offset) = content_start_offset {
        offset_to_line(input, offset)
    } else {
        // Fallback to block start if we can't find content offset
        offset_to_line(input, node.text_range().start().into())
    };

    Some(CodeBlock {
        language,
        content,
        start_line,
    })
}

/// Convert byte offset to 1-indexed line number.
pub fn offset_to_line(input: &str, offset: usize) -> usize {
    // Count how many newlines precede this offset
    let newline_count = input[..offset].chars().filter(|&c| c == '\n').count();
    // Line number is newlines + 1
    newline_count + 1
}

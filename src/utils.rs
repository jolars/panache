use crate::syntax::{SyntaxKind, SyntaxNode};
use rowan::NodeOrToken;
use std::collections::HashMap;

/// Check if a syntax kind represents a block-level element for formatting purposes.
/// This determines when to add blank lines between elements.
pub fn is_block_element(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::PARAGRAPH
            | SyntaxKind::Figure
            | SyntaxKind::List
            | SyntaxKind::DefinitionList
            | SyntaxKind::BlockQuote
            | SyntaxKind::MathBlock
            | SyntaxKind::CodeBlock
            | SyntaxKind::SimpleTable
            | SyntaxKind::MultilineTable
            | SyntaxKind::PipeTable
            | SyntaxKind::LineBlock
    )
}

/// Check if a syntax kind represents a block-level element for range filtering.
/// This is more comprehensive than is_block_element and includes all structural blocks.
pub fn is_structural_block(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::PARAGRAPH
            | SyntaxKind::Figure
            | SyntaxKind::Heading
            | SyntaxKind::CodeBlock
            | SyntaxKind::BlockQuote
            | SyntaxKind::List
            | SyntaxKind::ListItem
            | SyntaxKind::DefinitionList
            | SyntaxKind::DefinitionItem
            | SyntaxKind::LineBlock
            | SyntaxKind::SimpleTable
            | SyntaxKind::MultilineTable
            | SyntaxKind::PipeTable
            | SyntaxKind::GridTable
            | SyntaxKind::FencedDiv
            | SyntaxKind::HorizontalRule
            | SyntaxKind::YamlMetadata
            | SyntaxKind::PandocTitleBlock
            | SyntaxKind::HtmlBlock
            | SyntaxKind::MathBlock
            | SyntaxKind::BlankLine
            | SyntaxKind::ReferenceDefinition
            | SyntaxKind::FootnoteDefinition
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
        if node.kind() == SyntaxKind::CodeBlock
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
                SyntaxKind::CodeFenceOpen => {
                    // Look for CodeInfo node, then extract CodeLanguage from inside it
                    for fence_child in n.children_with_tokens() {
                        if let NodeOrToken::Node(info_node) = fence_child
                            && info_node.kind() == SyntaxKind::CodeInfo
                        {
                            // Search for CodeLanguage token inside CodeInfo node
                            for info_token in info_node.children_with_tokens() {
                                if let NodeOrToken::Token(t) = info_token
                                    && t.kind() == SyntaxKind::CodeLanguage
                                {
                                    language = Some(t.text().to_string());
                                    break;
                                }
                            }
                        }
                    }
                }
                SyntaxKind::CodeContent => {
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

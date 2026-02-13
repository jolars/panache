//! Code block collection and concatenation for external linter invocation.
//!
//! This module provides utilities to:
//! 1. Extract code blocks from a parsed document
//! 2. Group them by language
//! 3. Concatenate blocks with blank line preservation for accurate position mapping

use std::collections::HashMap;

use crate::syntax::{SyntaxKind, SyntaxNode};

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
    use rowan::NodeOrToken;

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
fn offset_to_line(input: &str, offset: usize) -> usize {
    // Count how many newlines precede this offset
    let newline_count = input[..offset].chars().filter(|&c| c == '\n').count();
    // Line number is newlines + 1
    newline_count + 1
}

/// Concatenate code blocks with blank line preservation.
///
/// Returns the concatenated string where each block appears at its original line number,
/// with blank lines filling the gaps.
pub fn concatenate_with_blanks(blocks: &[CodeBlock]) -> String {
    if blocks.is_empty() {
        return String::new();
    }

    let mut result = String::new();
    let mut current_line = 1;

    for block in blocks {
        // Add blank lines to reach the block's start line
        while current_line < block.start_line {
            result.push('\n');
            current_line += 1;
        }

        // Add the block content
        result.push_str(&block.content);

        // Update current line based on how many lines we just added
        let lines_added = block.content.lines().count().max(1);
        current_line += lines_added;

        // Add trailing newline if block doesn't end with one
        if !block.content.ends_with('\n') {
            result.push('\n');
            current_line += 1;
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    #[test]
    fn test_collect_single_r_block() {
        let input = r#"# Test

```r
x <- 1
y <- 2
```
"#;

        let tree = parse(input, None);
        let blocks = collect_code_blocks(&tree, input);

        assert_eq!(blocks.len(), 1);
        assert!(blocks.contains_key("r"));

        let r_blocks = &blocks["r"];
        assert_eq!(r_blocks.len(), 1);
        assert_eq!(r_blocks[0].language, "r");
        assert_eq!(r_blocks[0].content, "x <- 1\ny <- 2\n");
        assert_eq!(r_blocks[0].start_line, 4); // Content starts on line 4, not fence line 3
    }

    #[test]
    fn test_collect_multiple_blocks_same_language() {
        let input = r#"```r
x <- 1
```

Text in between.

```r
y <- 2
```
"#;

        let tree = parse(input, None);
        let blocks = collect_code_blocks(&tree, input);

        assert_eq!(blocks.len(), 1);
        let r_blocks = &blocks["r"];
        assert_eq!(r_blocks.len(), 2);
        assert_eq!(r_blocks[0].start_line, 2); // Content on line 2, fence on line 1
        assert_eq!(r_blocks[1].start_line, 8); // Content on line 8, fence on line 7
    }

    #[test]
    fn test_collect_multiple_languages() {
        let input = r#"```python
print("hello")
```

```r
print("hello")
```
"#;

        let tree = parse(input, None);
        let blocks = collect_code_blocks(&tree, input);

        assert_eq!(blocks.len(), 2);
        assert!(blocks.contains_key("python"));
        assert!(blocks.contains_key("r"));
    }

    #[test]
    fn test_concatenate_with_blanks_single_block() {
        let blocks = vec![CodeBlock {
            language: "r".to_string(),
            content: "x <- 1\n".to_string(),
            start_line: 5,
        }];

        let result = concatenate_with_blanks(&blocks);

        // Should have 4 blank lines (lines 1-4), then content at line 5
        let expected = "\n\n\n\nx <- 1\n";
        assert_eq!(result, expected);
    }

    #[test]
    fn test_concatenate_with_blanks_multiple_blocks() {
        let blocks = vec![
            CodeBlock {
                language: "r".to_string(),
                content: "x <- 1\n".to_string(),
                start_line: 2,
            },
            CodeBlock {
                language: "r".to_string(),
                content: "y <- 2\n".to_string(),
                start_line: 6,
            },
        ];

        let result = concatenate_with_blanks(&blocks);

        // Line 1: blank
        // Line 2: x <- 1
        // Lines 3-5: blank
        // Line 6: y <- 2
        let lines: Vec<&str> = result.lines().collect();
        assert_eq!(lines.len(), 6);
        assert_eq!(lines[0], ""); // Line 1 (blank)
        assert_eq!(lines[1], "x <- 1"); // Line 2
        assert_eq!(lines[2], ""); // Line 3
        assert_eq!(lines[3], ""); // Line 4
        assert_eq!(lines[4], ""); // Line 5
        assert_eq!(lines[5], "y <- 2"); // Line 6
    }

    #[test]
    fn test_concatenate_preserves_line_numbers() {
        let blocks = vec![
            CodeBlock {
                language: "r".to_string(),
                content: "a <- 1\n".to_string(),
                start_line: 10,
            },
            CodeBlock {
                language: "r".to_string(),
                content: "b <- 2\n".to_string(),
                start_line: 20,
            },
        ];

        let result = concatenate_with_blanks(&blocks);

        // Count total lines
        let line_count = result.lines().count();
        assert_eq!(line_count, 20);

        // Check that line 10 has "a <- 1"
        let line_10 = result.lines().nth(9).unwrap(); // 0-indexed
        assert_eq!(line_10, "a <- 1");

        // Check that line 20 has "b <- 2"
        let line_20 = result.lines().nth(19).unwrap();
        assert_eq!(line_20, "b <- 2");
    }

    #[test]
    fn test_offset_to_line() {
        let input = "line1\nline2\nline3\n";

        assert_eq!(offset_to_line(input, 0), 1); // Start of file
        assert_eq!(offset_to_line(input, 5), 1); // Before first \n
        assert_eq!(offset_to_line(input, 6), 2); // Start of line 2
        assert_eq!(offset_to_line(input, 12), 3); // Start of line 3
    }

    #[test]
    fn test_quarto_style_braces() {
        // Quarto uses {r} instead of just r
        let input = r#"```{r}
x <- 1
```
"#;

        let tree = parse(input, None);
        let blocks = collect_code_blocks(&tree, input);

        assert_eq!(blocks.len(), 1);
        assert!(blocks.contains_key("r"), "Should extract 'r' from '{{r}}'");

        let r_blocks = &blocks["r"];
        assert_eq!(r_blocks.len(), 1);
        assert_eq!(r_blocks[0].language, "r");
        assert_eq!(r_blocks[0].content, "x <- 1\n");
    }

    #[test]
    fn test_quarto_style_braces_with_options() {
        // Quarto supports {r label, echo=FALSE}
        let input = r#"```{r my-label, echo=FALSE}
x <- 1
```
"#;

        let tree = parse(input, None);
        let blocks = collect_code_blocks(&tree, input);

        assert_eq!(blocks.len(), 1);
        assert!(
            blocks.contains_key("r"),
            "Should extract 'r' from '{{r my-label, echo=FALSE}}'"
        );

        let r_blocks = &blocks["r"];
        assert_eq!(r_blocks.len(), 1);
        assert_eq!(r_blocks[0].language, "r");
    }

    #[test]
    fn test_quarto_various_syntaxes() {
        let input = r#"```{r}
a <- 1
```

```{python}
b = 2
```

```{r chunk-label}
c <- 3
```

```{r chunk2, echo=FALSE}
d <- 4
```
"#;

        let tree = parse(input, None);
        let blocks = collect_code_blocks(&tree, input);

        assert_eq!(blocks.len(), 2);
        assert!(blocks.contains_key("r"));
        assert!(blocks.contains_key("python"));

        let r_blocks = &blocks["r"];
        assert_eq!(r_blocks.len(), 3, "Should find all three R blocks");

        let py_blocks = &blocks["python"];
        assert_eq!(py_blocks.len(), 1);
    }
}

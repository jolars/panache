//! Code block concatenation for external linter invocation.
//!
//! This module provides utilities to concatenate code blocks with blank line
//! preservation for accurate position mapping in diagnostics.

use crate::utils::CodeBlock;

/// Mapping information for a code block in the concatenated file.
#[derive(Debug, Clone)]
pub struct BlockMapping {
    /// Byte offset range in the concatenated file
    pub concatenated_range: std::ops::Range<usize>,
    /// Byte offset range in the original document
    pub original_range: std::ops::Range<usize>,
    /// Starting line number in both files (preserved by blank line padding)
    pub start_line: usize,
}

/// Result of concatenating code blocks with mapping information.
#[derive(Debug, Clone)]
pub struct ConcatenatedBlocks {
    /// The concatenated content
    pub content: String,
    /// Mapping information for each block
    pub mappings: Vec<BlockMapping>,
}

/// Concatenate code blocks with blank line preservation and return mapping info.
///
/// Returns the concatenated string where each block appears at its original line number,
/// with blank lines filling the gaps, plus mapping information to convert offsets back.
pub fn concatenate_with_blanks_and_mapping(blocks: &[CodeBlock]) -> ConcatenatedBlocks {
    if blocks.is_empty() {
        return ConcatenatedBlocks {
            content: String::new(),
            mappings: Vec::new(),
        };
    }

    let mut content = String::new();
    let mut mappings = Vec::new();
    let mut current_line = 1;

    for block in blocks {
        // Add blank lines to reach the block's start line
        while current_line < block.start_line {
            content.push('\n');
            current_line += 1;
        }

        // Track the start of this block in the concatenated file
        let concat_start = content.len();

        // Add the block content
        content.push_str(&block.content);

        // Track the end of this block in the concatenated file
        let concat_end = content.len();

        // Record the mapping
        mappings.push(BlockMapping {
            concatenated_range: concat_start..concat_end,
            original_range: block.original_range.clone(),
            start_line: block.start_line,
        });

        // Update current line based on how many lines we just added
        let lines_added = block.content.lines().count().max(1);
        current_line += lines_added;

        // Add trailing newline if block doesn't end with one
        if !block.content.ends_with('\n') {
            content.push('\n');
            current_line += 1;
        }
    }

    ConcatenatedBlocks { content, mappings }
}

/// Concatenate code blocks with blank line preservation.
///
/// Returns the concatenated string where each block appears at its original line number,
/// with blank lines filling the gaps.
pub fn concatenate_with_blanks(blocks: &[CodeBlock]) -> String {
    concatenate_with_blanks_and_mapping(blocks).content
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, Flavor};
    use crate::parse;
    use crate::utils::{CodeBlock, collect_code_blocks, offset_to_line};

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
            original_range: 100..107, // Dummy range for test
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
                original_range: 50..57,
            },
            CodeBlock {
                language: "r".to_string(),
                content: "y <- 2\n".to_string(),
                start_line: 6,
                original_range: 150..157,
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
                original_range: 200..207,
            },
            CodeBlock {
                language: "r".to_string(),
                content: "b <- 2\n".to_string(),
                start_line: 20,
                original_range: 400..407,
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

        let config = Config {
            flavor: Flavor::Quarto,
            extensions: crate::config::Extensions::for_flavor(Flavor::Quarto),
            ..Default::default()
        };
        let tree = parse(input, Some(config));
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

        let config = Config {
            flavor: Flavor::Quarto,
            extensions: crate::config::Extensions::for_flavor(Flavor::Quarto),
            ..Default::default()
        };
        let tree = parse(input, Some(config));
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
    fn test_quarto_display_class_language_normalized() {
        let input = "```{.bash filename=\"Terminal\"}\necho hi\n```\n";
        let config = Config {
            flavor: Flavor::Quarto,
            extensions: crate::config::Extensions::for_flavor(Flavor::Quarto),
            ..Default::default()
        };
        let tree = parse(input, Some(config));
        let blocks = collect_code_blocks(&tree, input);

        assert!(blocks.contains_key("bash"));
        let bash_blocks = &blocks["bash"];
        assert_eq!(bash_blocks.len(), 1);
        assert_eq!(bash_blocks[0].language, "bash");
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

        let config = Config {
            flavor: Flavor::Quarto,
            extensions: crate::config::Extensions::for_flavor(Flavor::Quarto),
            ..Default::default()
        };
        let tree = parse(input, Some(config));
        let blocks = collect_code_blocks(&tree, input);

        assert_eq!(blocks.len(), 2);
        assert!(blocks.contains_key("r"));
        assert!(blocks.contains_key("python"));

        let r_blocks = &blocks["r"];
        assert_eq!(r_blocks.len(), 3, "Should find all three R blocks");

        let py_blocks = &blocks["python"];
        assert_eq!(py_blocks.len(), 1);
    }

    #[test]
    fn test_concatenate_with_mapping() {
        let blocks = vec![
            CodeBlock {
                language: "r".to_string(),
                content: "x <- 1\n".to_string(),
                start_line: 2,
                original_range: 10..17, // Hypothetical original positions
            },
            CodeBlock {
                language: "r".to_string(),
                content: "y <- 2\n".to_string(),
                start_line: 6,
                original_range: 50..57,
            },
        ];

        let result = concatenate_with_blanks_and_mapping(&blocks);

        // Check content is correct
        let lines: Vec<&str> = result.content.lines().collect();
        assert_eq!(lines.len(), 6);
        assert_eq!(lines[1], "x <- 1"); // Line 2
        assert_eq!(lines[5], "y <- 2"); // Line 6

        // Check mappings
        assert_eq!(result.mappings.len(), 2);

        // First block mapping
        assert_eq!(result.mappings[0].start_line, 2);
        assert_eq!(result.mappings[0].original_range, 10..17);
        // In concatenated: "\n" (line 1) + "x <- 1\n" = offset 1 to 8
        assert_eq!(result.mappings[0].concatenated_range.start, 1);
        assert_eq!(result.mappings[0].concatenated_range.end, 8);

        // Second block mapping
        assert_eq!(result.mappings[1].start_line, 6);
        assert_eq!(result.mappings[1].original_range, 50..57);
        // In concatenated: 8 (after first) + "\n\n\n" (lines 3-5) = 11, then "y <- 2\n" = 11 to 18
        assert_eq!(result.mappings[1].concatenated_range.start, 11);
        assert_eq!(result.mappings[1].concatenated_range.end, 18);
    }
}

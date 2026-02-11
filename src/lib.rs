pub mod block_parser;
pub mod config;
#[cfg(feature = "lsp")]
pub mod external_formatters;
mod external_formatters_common;
pub mod external_formatters_sync;
pub mod formatter;
pub mod inline_parser;
pub mod linter;
#[cfg(feature = "lsp")]
pub mod lsp;
pub mod range_utils;
pub mod syntax;

pub use config::BlankLines;
pub use config::Config;
pub use config::ConfigBuilder;
pub use formatter::format_tree;
#[cfg(feature = "lsp")]
pub use formatter::format_tree_async;
pub use syntax::SyntaxNode;

fn init_logger() {
    let _ = env_logger::builder().is_test(true).try_init();
}

fn detect_line_ending(input: &str) -> &str {
    // Check for first occurrence of \r\n or \n
    let rn_pos = input.find("\r\n");
    let n_pos = input.find('\n');

    if let (Some(rn), Some(n)) = (rn_pos, n_pos) {
        if rn < n {
            return "\r\n";
        }
    } else if rn_pos.is_some() {
        return "\r\n";
    }

    "\n"
}

/// Formats a Quarto document string with the specified configuration.
///
/// This is the primary formatting function. It runs synchronously and includes
/// external formatter support via threads.
///
/// # Examples
///
/// ```rust
/// use panache::format;
///
/// let cfg = panache::ConfigBuilder::default().line_width(80).build();
///
/// let input = "This is a very long line that should be wrapped.";
/// let formatted = format(input, Some(cfg), None);
/// ```
///
/// # Arguments
///
/// * `input` - The Quarto document content to format
/// * `config` - Optional configuration (defaults to default config)
/// * `range` - Optional line range (start_line, end_line) to format, 1-indexed and inclusive.
///   If None, formats entire document. Range will be expanded to complete block boundaries.
pub fn format(input: &str, config: Option<Config>, range: Option<(usize, usize)>) -> String {
    #[cfg(debug_assertions)]
    {
        init_logger();
    }

    let line_ending = detect_line_ending(input);

    let normalized_input = input.replace("\r\n", "\n");

    // Step 1: Parse blocks to create initial CST and extract reference definitions
    let config = config.unwrap_or_default();
    let (block_tree, reference_registry) =
        block_parser::BlockParser::new(&normalized_input, &config).parse();

    // Step 2: Run inline parser on block content to create final CST
    let tree =
        inline_parser::InlineParser::new(block_tree, config.clone(), reference_registry).parse();

    // Step 3: Expand line range to byte offsets and block boundaries if specified
    let expanded_range = range.and_then(|(start_line, end_line)| {
        let result = range_utils::expand_line_range_to_blocks(
            &tree,
            &normalized_input,
            start_line,
            end_line,
        );
        if let Some((start, end)) = result {
            log::info!(
                "Range lines {}:{} expanded to byte range {}:{} (text: {:?}...{:?})",
                start_line,
                end_line,
                start,
                end,
                &normalized_input[start..start.min(start + 20)],
                &normalized_input[end.saturating_sub(20).max(start)..end]
            );
        }
        result
    });

    // Step 4: Format the final CST (synchronously, no external formatters)
    let out = formatter::format_tree(&tree, &config, expanded_range);

    if line_ending == "\r\n" {
        out.replace("\n", "\r\n")
    } else {
        out
    }
}

/// Formats a Quarto document string using default configuration.
pub fn format_with_defaults(input: &str) -> String {
    format(input, None, None)
}

/// Async version for LSP contexts. Uses tokio for non-blocking formatter execution.
///
/// # Examples
///
/// ```no_run
/// # async {
/// use panache::format_async;
///
/// let cfg = panache::ConfigBuilder::default().line_width(80).build();
///
/// let input = "This is a very long line that should be wrapped.";
/// let formatted = format_async(input, Some(cfg), None).await;
/// # };
/// ```
///
/// # Arguments
///
/// * `input` - The Quarto document content to format
/// * `config` - Optional configuration (defaults to default config)
/// * `range` - Optional line range (start_line, end_line) to format, 1-indexed and inclusive.
///   If None, formats entire document. Range will be expanded to complete block boundaries.
#[cfg(feature = "lsp")]
pub async fn format_async(
    input: &str,
    config: Option<Config>,
    range: Option<(usize, usize)>,
) -> String {
    #[cfg(debug_assertions)]
    {
        init_logger();
    }

    let line_ending = detect_line_ending(input);

    let normalized_input = input.replace("\r\n", "\n");

    // Step 1: Parse blocks to create initial CST and extract reference definitions
    let config = config.unwrap_or_default();
    let (block_tree, reference_registry) =
        block_parser::BlockParser::new(&normalized_input, &config).parse();

    // Step 2: Run inline parser on block content to create final CST
    let tree =
        inline_parser::InlineParser::new(block_tree, config.clone(), reference_registry).parse();

    // Step 3: Expand line range to byte offsets and block boundaries if specified
    let expanded_range = range.and_then(|(start_line, end_line)| {
        range_utils::expand_line_range_to_blocks(&tree, &normalized_input, start_line, end_line)
    });

    // Step 4: Format the final CST (with external formatters if configured)
    let out = formatter::format_tree_async(&tree, &config, expanded_range).await;

    if line_ending == "\r\n" {
        out.replace("\n", "\r\n")
    } else {
        out
    }
}

#[cfg(feature = "lsp")]
pub async fn format_async_with_defaults(input: &str) -> String {
    format_async(input, None, None).await
}

/// Parses a Quarto document string into a syntax tree.
///
/// This function normalizes line endings and runs both the block parser
/// and inline parser to produce a complete concrete syntax tree (CST).
///
/// # Examples
///
/// ```rust
/// use panache::parse;
///
/// let input = "# Heading\n\nParagraph text.";
/// let tree = parse(input, None);
/// println!("{:#?}", tree);
/// ```
///
/// # Arguments
///
/// * `input` - The Quarto document content to parse
/// * `config` - Optional configuration. If None, uses default config.
pub fn parse(input: &str, config: Option<Config>) -> SyntaxNode {
    let normalized_input = input.replace("\r\n", "\n");
    let config = config.unwrap_or_default();
    let (block_tree, reference_registry) =
        block_parser::BlockParser::new(&normalized_input, &config).parse();
    inline_parser::InlineParser::new(block_tree, config, reference_registry).parse()
}

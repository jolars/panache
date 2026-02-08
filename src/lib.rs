pub mod block_parser;
pub mod config;
pub mod external_formatters;
pub mod formatter;
pub mod inline_parser;
pub mod syntax;

pub use config::BlankLines;
pub use config::Config;
pub use config::ConfigBuilder;
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

/// Formats a Quarto document string with the specified line width.
///
/// This function normalizes line endings, preserves code blocks and frontmatter,
/// and applies consistent paragraph wrapping.
///
/// # Examples
///
/// ```no_run
/// # async {
/// use panache::format;
///
/// let cfg = panache::ConfigBuilder::default().line_width(80).build();
///
/// let input = "This is a very long line that should be wrapped.";
/// let formatted = format(input, Some(cfg)).await;
/// # };
/// ```
///
/// # Arguments
///
/// * `input` - The Quarto document content to format
/// * `config` - Optional configuration (defaults to default config)
pub async fn format(input: &str, config: Option<Config>) -> String {
    #[cfg(debug_assertions)]
    {
        init_logger();
    }

    let line_ending = detect_line_ending(input);

    let normalized_input = input.replace("\r\n", "\n");

    // Step 1: Parse blocks to create initial CST
    let config = config.unwrap_or_default();
    let block_tree = block_parser::BlockParser::new(&normalized_input, &config).parse();

    // Step 2: Run inline parser on block content to create final CST
    let tree = inline_parser::InlineParser::new(block_tree, config.clone()).parse();

    // Step 3: Format the final CST (with external formatters if configured)
    let out = formatter::format_tree_async(&tree, &config).await;

    if line_ending == "\r\n" {
        out.replace("\n", "\r\n")
    } else {
        out
    }
}

pub async fn format_with_defaults(input: &str) -> String {
    format(input, None).await
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
/// let tree = parse(input);
/// println!("{:#?}", tree);
/// ```
///
/// # Arguments
///
/// * `input` - The Quarto document content to parse
pub fn parse(input: &str) -> SyntaxNode {
    let normalized_input = input.replace("\r\n", "\n");
    let config = Config::default();
    let block_tree = block_parser::BlockParser::new(&normalized_input, &config).parse();
    inline_parser::InlineParser::new(block_tree, config).parse()
}

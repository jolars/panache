pub mod bib;
pub mod config;
pub mod directives;
#[cfg(not(target_arch = "wasm32"))]
mod external_formatters_common;
#[cfg(not(target_arch = "wasm32"))]
pub mod external_formatters_sync;
#[cfg(any(feature = "lsp", not(target_arch = "wasm32")))]
mod external_tools_common;
pub mod formatter;
pub mod includes;
pub mod linter;
#[cfg(feature = "lsp")]
pub mod lsp;
pub mod metadata;
pub mod parser;
pub mod range_utils;
pub mod salsa;
pub mod syntax;
mod utils;
mod yaml_engine;
#[cfg(test)]
mod yaml_regions;

pub use config::BlankLines;
pub use config::Config;
pub use config::ConfigBuilder;
#[cfg(not(target_arch = "wasm32"))]
pub use external_tools_common::init_external_tool_budget;
#[cfg(any(feature = "lsp", not(target_arch = "wasm32")))]
pub use external_tools_common::set_warning_color_override;
pub use formatter::format_tree;
pub use parser::parse;
pub use syntax::SyntaxNode;

pub fn markdown_extensions() -> &'static [&'static str] {
    &["md", "markdown", "mdown", "mkd", "mkdn"]
}

pub fn all_document_extensions() -> &'static [&'static str] {
    &[
        "qmd",
        "Rmd",
        "rmd",
        "Rmarkdown",
        "rmarkdown",
        "md",
        "markdown",
        "mdown",
        "mkd",
        "mkdn",
        "svx",
    ]
}

#[cfg(debug_assertions)]
fn init_logger() {
    let _ = env_logger::builder().is_test(true).try_init();
}

fn detect_line_ending(input: &str) -> &str {
    // Detect first occurrence of \r\n or \n
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

/// Apply line ending normalization to formatted output.
/// Converts all line endings in the output to the target line ending.
fn apply_line_ending(text: &str, target: &str) -> String {
    if target == "\r\n" {
        // Convert LF to CRLF (but don't double-convert existing CRLF)
        text.replace("\r\n", "\n").replace("\n", "\r\n")
    } else {
        // Convert CRLF to LF
        text.replace("\r\n", "\n")
    }
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

    let config = config.unwrap_or_default();

    // Parse document into complete CST (parser preserves all bytes including
    // CRLF), then format that tree.
    let tree = parser::parse(input, Some(config.clone()));
    format_with_tree(input, &tree, &config, range)
}

/// Formats a document from an already-parsed CST, skipping the internal parse.
///
/// Behaves exactly like [`format`] but reuses a caller-owned `tree` instead of
/// parsing `input` again. The LSP routes formatting through this so it can reuse
/// its salsa-cached parse (matching hover/symbols) rather than parsing afresh on
/// every format request.
///
/// `tree` MUST be the result of parsing `input` under `config`; passing a tree
/// that doesn't correspond to `input`/`config` yields undefined output.
///
/// # Arguments
///
/// * `input` - The document content the `tree` was parsed from
/// * `tree` - The CST produced by parsing `input` with `config`
/// * `config` - The configuration used to parse `input`
/// * `range` - Optional line range (start_line, end_line), 1-indexed and
///   inclusive; see [`format`].
pub fn format_with_tree(
    input: &str,
    tree: &SyntaxNode,
    config: &Config,
    range: Option<(usize, usize)>,
) -> String {
    // Determine target line ending based on config
    let target_line_ending = match config.line_ending {
        Some(config::LineEnding::Lf) => "\n",
        Some(config::LineEnding::Crlf) => "\r\n",
        Some(config::LineEnding::Auto) | None => {
            // Auto-detect from input: use first line ending found
            detect_line_ending(input)
        }
    };

    // Expand line range to byte offsets and block boundaries if specified
    let expanded_range = range.and_then(|(start_line, end_line)| {
        let result = range_utils::expand_line_range_to_blocks(tree, input, start_line, end_line);
        if let Some((start, end)) = result {
            log::debug!(
                "Range lines {}:{} expanded to byte range {}:{} (text: {:?}...{:?})",
                start_line,
                end_line,
                start,
                end,
                &input[start..start.min(start + 20)],
                &input[end.saturating_sub(20).max(start)..end]
            );
        }
        result
    });

    // Format the final CST (synchronously, includes external formatter support)
    let out = formatter::format_tree(tree, config, expanded_range);

    // Apply line ending normalization if needed
    apply_line_ending(&out, target_line_ending)
}

/// Formats a Quarto document string using default configuration.
pub fn format_with_defaults(input: &str) -> String {
    format(input, None, None)
}

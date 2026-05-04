pub mod config;
pub mod directives;
pub mod formatter;
pub mod parser;
pub mod syntax;
pub mod utils;
pub mod yaml_engine;

pub use config::BlankLines;
pub use config::Config;
pub use config::ConfigBuilder;
pub use config::LineEnding;
pub use config::MathDelimiterStyle;
pub use config::ParserOptions;
pub use config::TabStopMode;
pub use config::WrapMode;
pub use formatter::ExternalCodeBlock;
pub use formatter::FormattedCodeMap;
pub use formatter::collect_code_blocks;
pub use formatter::format_tree;
pub use formatter::format_tree_with_formatted_code;
pub use syntax::SyntaxNode;

fn detect_line_ending(input: &str) -> &str {
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

fn apply_line_ending(text: &str, target: &str) -> String {
    if target == "\r\n" {
        text.replace("\r\n", "\n").replace("\n", "\r\n")
    } else {
        text.replace("\r\n", "\n")
    }
}

pub fn format(input: &str, config: Option<Config>, range: Option<(usize, usize)>) -> String {
    let config = config.unwrap_or_default();
    let target_line_ending = match config.line_ending {
        Some(LineEnding::Lf) => "\n",
        Some(LineEnding::Crlf) => "\r\n",
        Some(LineEnding::Auto) | None => detect_line_ending(input),
    };

    let tree = parser::parse(input, Some(config.parser_options()));
    let out = formatter::format_tree(&tree, &config, range);
    apply_line_ending(&out, target_line_ending)
}

pub fn format_with_defaults(input: &str) -> String {
    format(input, None, None)
}

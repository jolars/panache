//! LaTeX environment block parsing utilities.

use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;

use super::blockquotes::count_blockquote_markers;
use super::utils::{emit_line_tokens, strip_leading_spaces};

/// Information about a detected LaTeX environment opening.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LatexEnvInfo {
    pub env_name: String,
}

/// Try to detect a LaTeX environment opening from content.
/// Returns environment info if this is a valid `\begin{name}` line.
pub(crate) fn try_parse_latex_env_begin(content: &str) -> Option<LatexEnvInfo> {
    let trimmed = strip_leading_spaces(content);

    // Check for \begin{
    if !trimmed.starts_with("\\begin{") {
        return None;
    }

    // Extract environment name
    let after_begin = &trimmed[7..]; // Skip "\begin{"
    let close_brace = after_begin.find('}')?;
    let env_name = after_begin[..close_brace].to_string();

    // Environment name must not be empty
    if env_name.is_empty() {
        return None;
    }

    Some(LatexEnvInfo { env_name })
}

/// Try to detect a LaTeX environment closing from content.
/// Returns environment name if this is a valid `\end{name}` line.
fn try_parse_latex_env_end(content: &str) -> Option<String> {
    let trimmed = strip_leading_spaces(content);

    // Check for \end{
    if !trimmed.starts_with("\\end{") {
        return None;
    }

    // Extract environment name
    let after_end = &trimmed[5..]; // Skip "\end{"
    let close_brace = after_end.find('}')?;
    let env_name = after_end[..close_brace].to_string();

    // Environment name must not be empty
    if env_name.is_empty() {
        return None;
    }

    Some(env_name)
}

/// Parse a LaTeX environment block, consuming lines from the parser.
/// Returns the new position after the environment block.
/// Handles nested environments by tracking environment names on a stack.
pub(crate) fn parse_latex_environment(
    builder: &mut GreenNodeBuilder<'static>,
    lines: &[&str],
    start_pos: usize,
    env_info: LatexEnvInfo,
    bq_depth: usize,
) -> usize {
    // Start LaTeX environment block
    builder.start_node(SyntaxKind::LatexEnvironment.into());

    // Opening \begin{name}
    let first_line = lines[start_pos];
    builder.start_node(SyntaxKind::LatexEnvBegin.into());
    emit_line_tokens(builder, first_line);
    builder.finish_node(); // LatexEnvBegin

    let mut current_pos = start_pos + 1;
    let mut content_lines: Vec<&str> = Vec::new();
    let mut env_stack = vec![env_info.env_name.clone()];
    let mut found_closing = false;

    // Parse content until we find the matching \end{name}
    while current_pos < lines.len() {
        let line = lines[current_pos];
        let (line_bq_depth, inner_content) = count_blockquote_markers(line);

        // Only process lines at the same or deeper blockquote depth
        if line_bq_depth < bq_depth {
            break;
        }

        // Strip blockquote markers at our depth
        let stripped_line = if line_bq_depth == bq_depth {
            inner_content
        } else {
            // Deeper blockquote - preserve the extra markers
            line
        };

        // Check for nested \begin{} or closing \end{}
        if let Some(nested_env) = try_parse_latex_env_begin(stripped_line) {
            log::trace!(
                "Found nested LaTeX \\begin{{{}}} at line {}",
                nested_env.env_name,
                current_pos + 1
            );
            env_stack.push(nested_env.env_name);
            content_lines.push(line);
            current_pos += 1;
            continue;
        }

        if let Some(end_name) = try_parse_latex_env_end(stripped_line) {
            // Check if this closes our environment or a nested one
            if let Some(expected_name) = env_stack.last()
                && &end_name == expected_name
            {
                env_stack.pop();

                // If stack is empty, this closes our outermost environment
                if env_stack.is_empty() {
                    log::debug!(
                        "Found matching LaTeX \\end{{{}}} at line {}",
                        end_name,
                        current_pos + 1
                    );
                    found_closing = true;

                    // Emit content
                    if !content_lines.is_empty() {
                        builder.start_node(SyntaxKind::LatexEnvContent.into());
                        for content_line in &content_lines {
                            emit_line_tokens(builder, content_line);
                        }
                        builder.finish_node(); // LatexEnvContent
                    }

                    // Emit closing \end{name}
                    builder.start_node(SyntaxKind::LatexEnvEnd.into());
                    emit_line_tokens(builder, line);
                    builder.finish_node(); // LatexEnvEnd

                    current_pos += 1;
                    break;
                } else {
                    // This closes a nested environment, continue collecting content
                    log::trace!(
                        "Found nested LaTeX \\end{{{}}} at line {}",
                        end_name,
                        current_pos + 1
                    );
                    content_lines.push(line);
                    current_pos += 1;
                    continue;
                }
            }
        }

        // Regular content line
        content_lines.push(line);
        current_pos += 1;
    }

    // If we didn't find a closing tag, emit what we collected
    if !found_closing {
        log::debug!(
            "LaTeX environment \\begin{{{}}} at line {} has no matching \\end",
            env_info.env_name,
            start_pos + 1
        );
        if !content_lines.is_empty() {
            builder.start_node(SyntaxKind::LatexEnvContent.into());
            for content_line in &content_lines {
                emit_line_tokens(builder, content_line);
            }
            builder.finish_node(); // LatexEnvContent
        }
    }

    builder.finish_node(); // LatexEnvironment
    current_pos
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_try_parse_latex_env_begin() {
        assert_eq!(
            try_parse_latex_env_begin("\\begin{tabular}"),
            Some(LatexEnvInfo {
                env_name: "tabular".to_string()
            })
        );
        assert_eq!(
            try_parse_latex_env_begin("  \\begin{equation}"),
            Some(LatexEnvInfo {
                env_name: "equation".to_string()
            })
        );
        assert_eq!(try_parse_latex_env_begin("\\begin{}"), None);
        assert_eq!(try_parse_latex_env_begin("begin{tabular}"), None);
        assert_eq!(try_parse_latex_env_begin("\\Begin{tabular}"), None);
    }

    #[test]
    fn test_try_parse_latex_env_end() {
        assert_eq!(
            try_parse_latex_env_end("\\end{tabular}"),
            Some("tabular".to_string())
        );
        assert_eq!(
            try_parse_latex_env_end("  \\end{equation}"),
            Some("equation".to_string())
        );
        assert_eq!(try_parse_latex_env_end("\\end{}"), None);
        assert_eq!(try_parse_latex_env_end("end{tabular}"), None);
    }

    #[test]
    fn test_parse_basic_latex_environment() {
        let input = r"\begin{tabular}{|l|l|}
Age & Frequency \\
18--25 & 15 \\
\end{tabular}";
        let lines: Vec<&str> = input.lines().collect();
        let mut builder = GreenNodeBuilder::new();

        let env_info = try_parse_latex_env_begin(lines[0]).unwrap();
        let new_pos = parse_latex_environment(&mut builder, &lines, 0, env_info, 0);

        assert_eq!(new_pos, 4); // Lines 0-3 consumed, position is now 4
    }

    #[test]
    fn test_parse_nested_latex_environments() {
        let input = r"\begin{table}
\begin{tabular}{|l|l|}
Age & Frequency \\
\end{tabular}
\end{table}";
        let lines: Vec<&str> = input.lines().collect();
        let mut builder = GreenNodeBuilder::new();

        let env_info = try_parse_latex_env_begin(lines[0]).unwrap();
        let new_pos = parse_latex_environment(&mut builder, &lines, 0, env_info, 0);

        assert_eq!(new_pos, 5);
    }

    #[test]
    fn test_parse_latex_environment_no_closing() {
        let input = r"\begin{tabular}{|l|l|}
Age & Frequency \\
18--25 & 15 \\";
        let lines: Vec<&str> = input.lines().collect();
        let mut builder = GreenNodeBuilder::new();

        let env_info = try_parse_latex_env_begin(lines[0]).unwrap();
        let new_pos = parse_latex_environment(&mut builder, &lines, 0, env_info, 0);

        // Should consume all lines even without closing tag
        assert_eq!(new_pos, 3);
    }
}

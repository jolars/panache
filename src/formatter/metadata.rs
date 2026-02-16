use crate::syntax::{SyntaxKind, SyntaxNode};
use rowan::NodeOrToken;

/// Collect YAML frontmatter from the syntax tree for external formatting.
/// Returns the raw YAML content (without --- delimiters and the structural newline after opening ---) if present.
pub fn collect_yaml_metadata(tree: &SyntaxNode) -> Option<String> {
    // Find YamlMetadata node
    for node in tree.descendants() {
        if node.kind() == SyntaxKind::YamlMetadata {
            // Extract YAML content between --- delimiters
            let mut yaml_content = String::new();
            let mut in_content = false;
            let mut skip_first_newline = false;

            for child in node.children_with_tokens() {
                match child {
                    NodeOrToken::Token(token) => {
                        if token.kind() == SyntaxKind::YamlMetadataDelim {
                            if token.text() == "---" && !in_content {
                                // Opening delimiter - start collecting (but skip next newline)
                                in_content = true;
                                skip_first_newline = true;
                            } else if (token.text() == "---" || token.text() == "...") && in_content
                            {
                                // Closing delimiter - stop collecting
                                break;
                            }
                        } else if in_content
                            && (token.kind() == SyntaxKind::TEXT
                                || token.kind() == SyntaxKind::NEWLINE)
                        {
                            // Skip the first newline after opening ---
                            if skip_first_newline && token.kind() == SyntaxKind::NEWLINE {
                                skip_first_newline = false;
                                continue;
                            }
                            yaml_content.push_str(token.text());
                        }
                    }
                    NodeOrToken::Node(_) => {}
                }
            }

            // Trim trailing newlines for cleaner formatting
            return Some(yaml_content.trim_end().to_string());
        }
    }

    None
}

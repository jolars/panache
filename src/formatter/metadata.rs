use crate::syntax::{SyntaxKind, SyntaxNode};

pub fn collect_yaml_frontmatter_region(
    tree: &SyntaxNode,
) -> Option<crate::syntax::YamlFrontmatterRegion> {
    let frontmatter = tree
        .children()
        .find(|node| node.kind() != SyntaxKind::BLANK_LINE)
        .filter(|node| node.kind() == SyntaxKind::YAML_METADATA)?;

    let content = frontmatter
        .children()
        .find(|child| child.kind() == SyntaxKind::YAML_METADATA_CONTENT)?;

    let host_start: usize = frontmatter.text_range().start().into();
    let host_end: usize = frontmatter.text_range().end().into();
    let content_start: usize = content.text_range().start().into();
    let content_end: usize = content.text_range().end().into();

    Some(crate::syntax::YamlFrontmatterRegion {
        id: format!("frontmatter:{}:{}", content_start, content_end),
        host_range: host_start..host_end,
        content_range: content_start..content_end,
        content: content.text().to_string(),
    })
}

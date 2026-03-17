use crate::syntax::SyntaxNode;

pub fn collect_yaml_frontmatter_region(
    tree: &SyntaxNode,
) -> Option<crate::syntax::YamlFrontmatterRegion> {
    crate::syntax::collect_embedded_frontmatter_yaml_cst(tree).map(|embedding| {
        let region = embedding.parsed().to_region();
        crate::syntax::YamlFrontmatterRegion {
            id: region.id,
            host_range: region.host_range,
            content_range: region.content_range,
            content: region.content,
        }
    })
}

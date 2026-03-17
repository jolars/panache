#[cfg(test)]
use crate::syntax::SyntaxNode;

#[cfg(test)]
pub(crate) type YamlRegionKind = crate::syntax::YamlRegionKind;
#[cfg(test)]
pub(crate) type YamlRegion = crate::syntax::YamlRegion;

#[cfg(test)]
pub(crate) fn collect_yaml_regions(tree: &SyntaxNode) -> Vec<YamlRegion> {
    crate::syntax::collect_yaml_regions(tree)
}

#[cfg(test)]
pub(crate) fn collect_frontmatter_region(tree: &SyntaxNode) -> Option<YamlRegion> {
    crate::syntax::collect_frontmatter_yaml_region(tree)
}

#[cfg(test)]
pub(crate) fn collect_hashpipe_regions(tree: &SyntaxNode) -> Vec<YamlRegion> {
    crate::syntax::collect_hashpipe_regions(tree)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::SyntaxKind;

    #[test]
    fn frontmatter_region_tracks_content_and_ranges() {
        let input = "---\ntitle: Test\nlist:\n  - a\n---\n\n# H\n";
        let tree = crate::parser::parse(input, None);
        let region = collect_frontmatter_region(&tree).expect("expected frontmatter");
        assert_eq!(region.kind, YamlRegionKind::Frontmatter);
        assert_eq!(region.content, "title: Test\nlist:\n  - a\n");
        assert_eq!(&input[region.content_range.clone()], region.content);
        assert_eq!(
            &input[region.host_range.clone()],
            "---\ntitle: Test\nlist:\n  - a\n---\n"
        );
        assert!(region.region_range.start <= region.content_range.start);
        assert!(region.region_range.end >= region.content_range.end);
    }

    #[test]
    fn frontmatter_region_supports_dots_closer() {
        let input = "---\ntitle: Test\n...\n\n# H\n";
        let tree = crate::parser::parse(input, None);
        let region = collect_frontmatter_region(&tree).expect("expected frontmatter");
        assert_eq!(region.content, "title: Test\n");
    }

    #[test]
    fn frontmatter_region_range_matches_metadata_delimiters() {
        let input = "---\ntitle: Test\nlist:\n  - a\n---\n";
        let tree = crate::parser::parse(input, None);
        let region = collect_frontmatter_region(&tree).expect("expected frontmatter");
        let metadata = tree
            .descendants()
            .find(|n| n.kind() == SyntaxKind::YAML_METADATA)
            .expect("yaml metadata node");
        let mut first_delim_end = None;
        let mut content_start = None;
        let mut second_delim_start = None;
        for el in metadata.children_with_tokens() {
            let Some(token) = el.as_token() else {
                continue;
            };
            match token.kind() {
                SyntaxKind::YAML_METADATA_DELIM => {
                    if first_delim_end.is_none() {
                        first_delim_end = Some(token.text_range().end());
                    } else {
                        second_delim_start = Some(token.text_range().start());
                        break;
                    }
                }
                SyntaxKind::NEWLINE => {
                    if content_start.is_none() && first_delim_end.is_some() {
                        content_start = Some(token.text_range().end());
                    }
                }
                _ => {}
            }
        }
        let start: usize = content_start
            .or(first_delim_end)
            .expect("frontmatter start")
            .into();
        let end: usize = second_delim_start.expect("frontmatter end").into();
        assert_eq!(region.content_range, start..end);
    }

    #[test]
    fn frontmatter_region_range_matches_parser_content_node() {
        let input = "---\ntitle: Test\nlist:\n  - a\n---\n";
        let tree = crate::parser::parse(input, None);
        let region = collect_frontmatter_region(&tree).expect("expected frontmatter");
        let content_node = tree
            .descendants()
            .find(|n| n.kind() == SyntaxKind::YAML_METADATA_CONTENT)
            .expect("yaml metadata content node");
        let content_range: std::ops::Range<usize> =
            content_node.text_range().start().into()..content_node.text_range().end().into();
        assert_eq!(region.content_range, content_range);
    }

    #[test]
    fn hashpipe_region_tracks_header_content() {
        let input = "```{r}\n#| echo: false\n#| fig-cap: |\n#|   A caption\nx <- 1\n```\n";
        let config = crate::config::Config {
            flavor: crate::config::Flavor::Quarto,
            ..Default::default()
        };
        let tree = crate::parser::parse(input, Some(config));
        let regions = collect_hashpipe_regions(&tree);
        assert_eq!(regions.len(), 1);
        let region = &regions[0];
        assert_eq!(region.kind, YamlRegionKind::Hashpipe);
        assert_eq!(region.content, "echo: false\nfig-cap: |\n  A caption\n");
        assert_eq!(
            &input[region.region_range.clone()],
            "#| echo: false\n#| fig-cap: |\n#|   A caption\n"
        );
        assert_eq!(
            &input[region.host_range.clone()],
            "```{r}\n#| echo: false\n#| fig-cap: |\n#|   A caption\nx <- 1\n```\n"
        );
    }

    #[test]
    fn yaml_regions_include_frontmatter_and_hashpipe() {
        let input = "---\ntitle: Test\n---\n\n```{r}\n#| echo: false\nx <- 1\n```\n";
        let config = crate::config::Config {
            flavor: crate::config::Flavor::Quarto,
            ..Default::default()
        };
        let tree = crate::parser::parse(input, Some(config));
        let regions = collect_yaml_regions(&tree);
        assert_eq!(regions.len(), 2);
        assert!(
            regions
                .iter()
                .any(|region| matches!(region.kind, YamlRegionKind::Frontmatter))
        );
        assert!(
            regions
                .iter()
                .any(|region| matches!(region.kind, YamlRegionKind::Hashpipe))
        );
    }

    #[test]
    fn hashpipe_region_requires_option_header_line() {
        let input = "```{r}\n#|   not-an-option\nx <- 1\n```\n";
        let config = crate::config::Config {
            flavor: crate::config::Flavor::Quarto,
            ..Default::default()
        };
        let tree = crate::parser::parse(input, Some(config));
        let regions = collect_hashpipe_regions(&tree);
        assert!(
            regions.is_empty(),
            "continuation-only hashpipe lines should not form YAML regions"
        );
    }

    #[test]
    fn hashpipe_region_id_uses_host_and_region_offsets() {
        let input = "```{r}\n#| echo: false\nx <- 1\n```\n";
        let config = crate::config::Config {
            flavor: crate::config::Flavor::Quarto,
            ..Default::default()
        };
        let tree = crate::parser::parse(input, Some(config));
        let regions = collect_hashpipe_regions(&tree);
        assert_eq!(regions.len(), 1);
        let region = &regions[0];
        assert_eq!(
            region.id,
            format!(
                "hashpipe:r:{}:{}",
                region.host_range.start, region.region_range.start
            )
        );
    }

    #[test]
    fn hashpipe_region_range_matches_parser_preamble_node() {
        let input = "```{r}\n#| echo: false\n#| fig-cap: |\n#|   A caption\nx <- 1\n```\n";
        let config = crate::config::Config {
            flavor: crate::config::Flavor::Quarto,
            ..Default::default()
        };
        let tree = crate::parser::parse(input, Some(config));
        let regions = collect_hashpipe_regions(&tree);
        let region = regions.first().expect("hashpipe region");
        let preamble = tree
            .descendants()
            .find(|n| n.kind() == SyntaxKind::HASHPIPE_YAML_PREAMBLE)
            .expect("hashpipe preamble node");
        let preamble_range: std::ops::Range<usize> =
            preamble.text_range().start().into()..preamble.text_range().end().into();
        assert_eq!(region.region_range, preamble_range);
    }

    #[test]
    fn hashpipe_region_range_matches_parser_preamble_content_node() {
        let input = "```{r}\n#| echo: false\n#| fig-cap: |\n#|   A caption\nx <- 1\n```\n";
        let config = crate::config::Config {
            flavor: crate::config::Flavor::Quarto,
            ..Default::default()
        };
        let tree = crate::parser::parse(input, Some(config));
        let regions = collect_hashpipe_regions(&tree);
        let region = regions.first().expect("hashpipe region");
        let preamble_content = tree
            .descendants()
            .find(|n| n.kind() == SyntaxKind::HASHPIPE_YAML_CONTENT)
            .expect("hashpipe preamble content node");
        let preamble_content_range: std::ops::Range<usize> =
            preamble_content.text_range().start().into()
                ..preamble_content.text_range().end().into();
        assert_eq!(region.region_range, preamble_content_range);
    }

    #[test]
    fn yaml_regions_parse_as_yaml_roots_for_frontmatter_and_hashpipe() {
        let input = "---\ntitle: Test\n---\n\n```{r}\n#| echo: false\nx <- 1\n```\n";
        let config = crate::config::Config {
            flavor: crate::config::Flavor::Quarto,
            ..Default::default()
        };
        let tree = crate::parser::parse(input, Some(config));
        let regions = crate::syntax::collect_parsed_yaml_regions(&tree);
        assert_eq!(
            regions.len(),
            2,
            "expected YAML regions for frontmatter and hashpipe"
        );
        assert!(
            regions
                .iter()
                .all(|region| region.root_kind() == Some(crate::syntax::YamlAstRootKind::Root))
        );
    }

    #[test]
    fn frontmatter_region_maps_yaml_offsets_back_to_host_offsets() {
        let input = "---\ntitle: Test\n---\n";
        let tree = crate::parser::parse(input, None);
        let frontmatter = collect_frontmatter_region(&tree).expect("frontmatter yaml region");
        let yaml_local_offset = frontmatter
            .content
            .find("title")
            .expect("title in yaml content");
        let host_offset = frontmatter.content_range.start + yaml_local_offset;
        assert_eq!(&input[host_offset..host_offset + 5], "title");
    }

    #[test]
    fn frontmatter_yaml_parse_error_maps_to_host_offset() {
        let input = "---\ntitle: [\n---\n";
        let tree = crate::parser::parse(input, None);
        let frontmatter = collect_frontmatter_region(&tree).expect("frontmatter yaml region");
        let err = crate::syntax::validate_yaml_text(&frontmatter.content)
            .expect_err("expected parse err");
        let host_offset = frontmatter.content_range.start + err.offset();
        assert!(host_offset >= frontmatter.content_range.start);
    }
}

use std::ops::Range;

use crate::parser::utils::yaml_regions::hashpipe_language_and_prefix;
use crate::syntax::{AstNode, PanacheLanguage, SyntaxKind, SyntaxNode};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YamlFrontmatterRegion {
    pub id: String,
    pub host_range: Range<usize>,
    pub content_range: Range<usize>,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum YamlRegionKind {
    Frontmatter,
    Hashpipe,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YamlRegion {
    pub id: String,
    pub kind: YamlRegionKind,
    pub host_range: Range<usize>,
    pub region_range: Range<usize>,
    pub content_range: Range<usize>,
    pub content: String,
    pub yaml_to_host_offsets: Vec<usize>,
}

#[derive(Debug, Clone)]
pub struct ParsedYamlRegion {
    region: YamlRegion,
    parse_result: Result<yaml_parser::SyntaxNode, yaml_parser::SyntaxError>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedYamlRegionSnapshot {
    region: YamlRegion,
    parse_ok: bool,
    error: Option<YamlParseError>,
    document_shape_summary: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum YamlEmbeddingHostKind {
    FrontmatterMetadata,
    HashpipePreamble,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct YamlMetadata(SyntaxNode);

impl AstNode for YamlMetadata {
    type Language = PanacheLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::YAML_METADATA
    }

    fn cast(node: SyntaxNode) -> Option<Self> {
        Self::can_cast(node.kind()).then(|| Self(node))
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HashpipeYamlPreamble(SyntaxNode);

impl AstNode for HashpipeYamlPreamble {
    type Language = PanacheLanguage;

    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::HASHPIPE_YAML_PREAMBLE
    }

    fn cast(node: SyntaxNode) -> Option<Self> {
        Self::can_cast(node.kind()).then(|| Self(node))
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

#[derive(Debug, Clone)]
pub enum YamlEmbeddingHost {
    FrontmatterMetadata(YamlMetadata),
    HashpipePreamble(HashpipeYamlPreamble),
}

#[derive(Debug, Clone)]
pub struct YamlEmbeddedCst {
    host: YamlEmbeddingHost,
    parsed: ParsedYamlRegion,
}

impl YamlEmbeddedCst {
    pub fn host_kind(&self) -> YamlEmbeddingHostKind {
        match self.host {
            YamlEmbeddingHost::FrontmatterMetadata(_) => YamlEmbeddingHostKind::FrontmatterMetadata,
            YamlEmbeddingHost::HashpipePreamble(_) => YamlEmbeddingHostKind::HashpipePreamble,
        }
    }

    pub fn host_node(&self) -> &SyntaxNode {
        match &self.host {
            YamlEmbeddingHost::FrontmatterMetadata(host) => host.syntax(),
            YamlEmbeddingHost::HashpipePreamble(host) => host.syntax(),
        }
    }

    pub fn frontmatter_host(&self) -> Option<&YamlMetadata> {
        match &self.host {
            YamlEmbeddingHost::FrontmatterMetadata(host) => Some(host),
            _ => None,
        }
    }

    pub fn hashpipe_host(&self) -> Option<&HashpipeYamlPreamble> {
        match &self.host {
            YamlEmbeddingHost::HashpipePreamble(host) => Some(host),
            _ => None,
        }
    }

    pub fn parsed(&self) -> &ParsedYamlRegion {
        &self.parsed
    }

    pub fn yaml_content(&self) -> &str {
        self.parsed.content()
    }

    pub fn host_offset_for_yaml_offset(&self, yaml_offset: usize) -> Option<usize> {
        self.parsed.host_offset_for_yaml_offset(yaml_offset)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum YamlAstRootKind {
    Root,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum YamlDocumentKind {
    BlockMap,
    BlockSeq,
    BlockScalar,
    Flow,
    Empty,
}

#[derive(Debug, Clone, Copy)]
pub struct YamlAstRoot<'a> {
    node: &'a yaml_parser::SyntaxNode,
}

impl<'a> YamlAstRoot<'a> {
    pub fn kind(&self) -> YamlAstRootKind {
        debug_assert_eq!(self.node.kind(), yaml_parser::SyntaxKind::ROOT);
        YamlAstRootKind::Root
    }

    pub fn document_count(&self) -> usize {
        yaml_parser::ast::Root::cast(self.node.clone())
            .map(|root| root.documents().count())
            .unwrap_or(0)
    }

    pub fn first_document_kind(&self) -> Option<YamlDocumentKind> {
        let root = yaml_parser::ast::Root::cast(self.node.clone())?;
        let doc = root.documents().next()?;
        if let Some(block) = doc.block() {
            if block.block_map().is_some() {
                return Some(YamlDocumentKind::BlockMap);
            }
            if block.block_seq().is_some() {
                return Some(YamlDocumentKind::BlockSeq);
            }
            if block.block_scalar().is_some() {
                return Some(YamlDocumentKind::BlockScalar);
            }
            return Some(YamlDocumentKind::Empty);
        }
        if doc.flow().is_some() {
            return Some(YamlDocumentKind::Flow);
        }
        Some(YamlDocumentKind::Empty)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YamlParseError {
    offset: usize,
    message: String,
}

impl YamlParseError {
    pub fn offset(&self) -> usize {
        self.offset
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    fn from_parser(err: &yaml_parser::SyntaxError) -> Self {
        Self {
            offset: err.offset(),
            message: err.message().to_string(),
        }
    }
}

impl ParsedYamlRegion {
    pub fn id(&self) -> &str {
        &self.region.id
    }

    pub fn kind(&self) -> &YamlRegionKind {
        &self.region.kind
    }

    pub fn is_frontmatter(&self) -> bool {
        matches!(self.region.kind, YamlRegionKind::Frontmatter)
    }

    pub fn is_hashpipe(&self) -> bool {
        matches!(self.region.kind, YamlRegionKind::Hashpipe)
    }

    pub fn root(&self) -> Option<YamlAstRoot<'_>> {
        self.parse_result
            .as_ref()
            .ok()
            .map(|node| YamlAstRoot { node })
    }

    pub fn error(&self) -> Option<YamlParseError> {
        self.parse_result.as_ref().err().map(|err| YamlParseError {
            offset: err.offset(),
            message: err.message().to_string(),
        })
    }

    pub fn root_kind(&self) -> Option<YamlAstRootKind> {
        self.root().map(|root| root.kind())
    }

    pub fn is_valid(&self) -> bool {
        self.parse_result.is_ok()
    }

    pub fn host_range(&self) -> Range<usize> {
        self.region.host_range.clone()
    }

    pub fn content_range(&self) -> Range<usize> {
        self.region.content_range.clone()
    }

    pub fn region_range(&self) -> Range<usize> {
        self.region.region_range.clone()
    }

    pub fn to_region(&self) -> YamlRegion {
        self.region.clone()
    }

    pub fn content(&self) -> &str {
        &self.region.content
    }

    pub fn host_offset_for_yaml_offset(&self, yaml_offset: usize) -> Option<usize> {
        self.region.yaml_to_host_offsets.get(yaml_offset).copied()
    }

    pub fn parse_error_host_offset(&self) -> Option<usize> {
        self.error()
            .and_then(|err| self.host_offset_for_yaml_offset(err.offset()))
    }

    pub fn document_shape_summary(&self) -> Option<String> {
        let root = self.root()?;
        let doc_count = root.document_count();
        let first_kind = root.first_document_kind();
        Some(match first_kind {
            Some(kind) => format!("{:?} docs={} first={:?}", root.kind(), doc_count, kind),
            None => format!("{:?} docs={}", root.kind(), doc_count),
        })
    }

    pub fn to_snapshot(&self) -> ParsedYamlRegionSnapshot {
        ParsedYamlRegionSnapshot {
            region: self.region.clone(),
            parse_ok: self.is_valid(),
            error: self.error(),
            document_shape_summary: self.document_shape_summary(),
        }
    }
}

impl ParsedYamlRegionSnapshot {
    pub fn id(&self) -> &str {
        &self.region.id
    }

    pub fn is_frontmatter(&self) -> bool {
        matches!(self.region.kind, YamlRegionKind::Frontmatter)
    }

    pub fn is_hashpipe(&self) -> bool {
        matches!(self.region.kind, YamlRegionKind::Hashpipe)
    }

    pub fn is_valid(&self) -> bool {
        self.parse_ok
    }

    pub fn error(&self) -> Option<&YamlParseError> {
        self.error.as_ref()
    }

    pub fn host_range(&self) -> Range<usize> {
        self.region.host_range.clone()
    }

    pub fn parse_error_host_offset(&self) -> Option<usize> {
        let err = self.error()?;
        self.region.yaml_to_host_offsets.get(err.offset()).copied()
    }

    pub fn document_shape_summary(&self) -> Option<&str> {
        self.document_shape_summary.as_deref()
    }

    pub fn to_region(&self) -> YamlRegion {
        self.region.clone()
    }
}

pub fn collect_frontmatter_region(tree: &SyntaxNode) -> Option<YamlFrontmatterRegion> {
    let metadata = tree
        .descendants()
        .find(|node| node.kind() == SyntaxKind::YAML_METADATA)?;
    let content_node = metadata
        .children()
        .find(|child| child.kind() == SyntaxKind::YAML_METADATA_CONTENT)?;

    let host_start: usize = metadata.text_range().start().into();
    let host_end: usize = metadata.text_range().end().into();
    let content_start: usize = content_node.text_range().start().into();
    let content_end: usize = content_node.text_range().end().into();

    Some(YamlFrontmatterRegion {
        id: format!("frontmatter:{}:{}", content_start, content_end),
        host_range: host_start..host_end,
        content_range: content_start..content_end,
        content: content_node.text().to_string(),
    })
}

pub fn collect_frontmatter_yaml_region(tree: &SyntaxNode) -> Option<YamlRegion> {
    let frontmatter = collect_frontmatter_region(tree)?;
    let content_range = frontmatter.content_range.clone();
    Some(YamlRegion {
        id: frontmatter.id,
        kind: YamlRegionKind::Frontmatter,
        host_range: frontmatter.host_range.clone(),
        region_range: frontmatter.host_range,
        content_range: content_range.clone(),
        yaml_to_host_offsets: (0..=frontmatter.content.len())
            .map(|offset| content_range.start + offset)
            .collect(),
        content: frontmatter.content,
    })
}

pub fn collect_hashpipe_regions(tree: &SyntaxNode) -> Vec<YamlRegion> {
    let mut regions = Vec::new();
    for node in tree
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::CODE_BLOCK)
    {
        let mut info_text: Option<String> = None;
        let mut content_node: Option<SyntaxNode> = None;
        for child in node.children() {
            match child.kind() {
                SyntaxKind::CODE_FENCE_OPEN => {
                    for nested in child.children() {
                        if nested.kind() == SyntaxKind::CODE_INFO {
                            info_text = Some(nested.text().to_string());
                        }
                    }
                }
                SyntaxKind::CODE_CONTENT => content_node = Some(child),
                _ => {}
            }
        }
        let (Some(info_text), Some(content_node)) = (info_text, content_node) else {
            continue;
        };
        let Some((language, prefix)) = hashpipe_language_and_prefix(&info_text) else {
            continue;
        };

        let host_start: usize = node.text_range().start().into();
        let host_end: usize = node.text_range().end().into();
        let Some(preamble) = content_node
            .children()
            .find(|n| n.kind() == SyntaxKind::HASHPIPE_YAML_PREAMBLE)
        else {
            continue;
        };
        let Some(preamble_content) = preamble
            .children()
            .find(|n| n.kind() == SyntaxKind::HASHPIPE_YAML_CONTENT)
        else {
            continue;
        };
        let preamble_text = preamble_content.text().to_string();
        let preamble_start: usize = preamble_content.text_range().start().into();
        if let Some(region) = extract_hashpipe_region(
            &preamble_text,
            host_start,
            host_end,
            preamble_start,
            prefix,
            language.as_str(),
        ) {
            regions.push(region);
        }
    }
    regions
}

pub fn collect_yaml_regions(tree: &SyntaxNode) -> Vec<YamlRegion> {
    let mut regions = Vec::new();
    if let Some(frontmatter) = collect_frontmatter_yaml_region(tree) {
        regions.push(frontmatter);
    }
    regions.extend(collect_hashpipe_regions(tree));
    regions
}

pub fn collect_parsed_yaml_regions(tree: &SyntaxNode) -> Vec<ParsedYamlRegion> {
    collect_yaml_regions(tree)
        .into_iter()
        .map(|region| ParsedYamlRegion {
            parse_result: yaml_parser::parse(&region.content),
            region,
        })
        .collect()
}

pub fn collect_parsed_frontmatter_region(tree: &SyntaxNode) -> Option<ParsedYamlRegion> {
    collect_parsed_yaml_regions(tree)
        .into_iter()
        .find(|region| region.is_frontmatter())
}

pub fn collect_parsed_yaml_region_snapshots(tree: &SyntaxNode) -> Vec<ParsedYamlRegionSnapshot> {
    collect_parsed_yaml_regions(tree)
        .iter()
        .map(ParsedYamlRegion::to_snapshot)
        .collect()
}

pub fn validate_yaml_text(input: &str) -> Result<(), YamlParseError> {
    yaml_parser::parse(input)
        .map(|_| ())
        .map_err(|err| YamlParseError::from_parser(&err))
}

pub fn collect_embedded_yaml_cst(tree: &SyntaxNode) -> Vec<YamlEmbeddedCst> {
    let parsed_regions = collect_parsed_yaml_regions(tree);
    let frontmatter_node = tree.descendants().find_map(YamlMetadata::cast);
    let hashpipe_preambles: Vec<HashpipeYamlPreamble> = tree
        .descendants()
        .filter_map(HashpipeYamlPreamble::cast)
        .collect();

    let mut embedded = Vec::new();
    for parsed in parsed_regions {
        match parsed.kind() {
            YamlRegionKind::Frontmatter => {
                if let Some(node) = frontmatter_node.clone() {
                    embedded.push(YamlEmbeddedCst {
                        host: YamlEmbeddingHost::FrontmatterMetadata(node),
                        parsed,
                    });
                }
            }
            YamlRegionKind::Hashpipe => {
                if let Some(node) = hashpipe_preambles.iter().find(|node| {
                    let range: Range<usize> = node.syntax().text_range().start().into()
                        ..node.syntax().text_range().end().into();
                    range == parsed.region_range()
                }) {
                    embedded.push(YamlEmbeddedCst {
                        host: YamlEmbeddingHost::HashpipePreamble(node.clone()),
                        parsed,
                    });
                }
            }
        }
    }
    embedded
}

pub fn collect_embedded_frontmatter_yaml_cst(tree: &SyntaxNode) -> Option<YamlEmbeddedCst> {
    collect_embedded_yaml_cst(tree)
        .into_iter()
        .find(|embedded| embedded.frontmatter_host().is_some())
}

fn extract_hashpipe_region(
    content: &str,
    host_start: usize,
    host_end: usize,
    content_start: usize,
    prefix: &str,
    language: &str,
) -> Option<YamlRegion> {
    let lines: Vec<&str> = content.split_inclusive('\n').collect();
    if lines.is_empty() {
        return None;
    }
    let mut collected = String::new();
    let mut yaml_to_host_offsets = Vec::new();
    let mut offset = 0usize;
    for line in &lines {
        let line = *line;
        let line_core = line.strip_suffix('\n').unwrap_or(line);
        let line_core = line_core.strip_suffix('\r').unwrap_or(line_core);
        let eol = &line[line_core.len()..];
        let indent_len = line_core
            .chars()
            .take_while(|ch| *ch == ' ' || *ch == '\t')
            .map(char::len_utf8)
            .sum::<usize>();
        let trimmed = &line_core[indent_len..];
        let after_prefix = trimmed.strip_prefix(prefix)?;
        let payload = after_prefix
            .strip_prefix(' ')
            .or_else(|| after_prefix.strip_prefix('\t'))
            .unwrap_or(after_prefix);
        let after_prefix_start = indent_len + (trimmed.len() - after_prefix.len());
        let payload_start = after_prefix_start + (after_prefix.len() - payload.len());
        let line_host_start = content_start + offset;
        yaml_to_host_offsets
            .extend((0..payload.len()).map(|i| line_host_start + payload_start + i));
        yaml_to_host_offsets.extend((0..eol.len()).map(|i| line_host_start + line_core.len() + i));
        collected.push_str(payload);
        collected.push_str(eol);
        offset += line.len();
    }
    let start = content_start;
    let region_end = content_start + offset;
    yaml_to_host_offsets.push(region_end);
    let id = format!("hashpipe:{}:{}:{}", language, host_start, start);
    Some(YamlRegion {
        id,
        kind: YamlRegionKind::Hashpipe,
        host_range: host_start..host_end,
        region_range: start..region_end,
        content_range: start..region_end,
        content: collected,
        yaml_to_host_offsets,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsed_yaml_regions_include_frontmatter_and_hashpipe_cst_roots() {
        let input = "---\ntitle: Test\n---\n\n```{r}\n#| echo: false\n1 + 1\n```\n";
        let config = crate::config::Config {
            flavor: crate::config::Flavor::Quarto,
            ..Default::default()
        };
        let tree = crate::parser::parse(input, Some(config));
        let regions = collect_parsed_yaml_regions(&tree);
        assert_eq!(regions.len(), 2);
        assert!(regions.iter().any(|parsed| {
            parsed.is_frontmatter() && parsed.root_kind() == Some(YamlAstRootKind::Root)
        }));
        assert!(regions.iter().any(|parsed| {
            parsed.is_hashpipe() && parsed.root_kind() == Some(YamlAstRootKind::Root)
        }));
    }

    #[test]
    fn parsed_yaml_region_maps_parse_error_to_host_offset() {
        let input = "```{r}\n#| echo: [\n1 + 1\n```\n";
        let config = crate::config::Config {
            flavor: crate::config::Flavor::Quarto,
            ..Default::default()
        };
        let tree = crate::parser::parse(input, Some(config));
        let parsed = collect_parsed_yaml_regions(&tree);
        let hashpipe = parsed
            .iter()
            .find(|region| region.is_hashpipe())
            .expect("hashpipe region");
        let host_offset = hashpipe
            .parse_error_host_offset()
            .expect("expected parse error host offset");
        let expected = input.find('[').expect("expected '[' in input");
        assert_eq!(host_offset, expected);
    }

    #[test]
    fn yaml_ast_root_reports_document_shape() {
        let input = "---\ntitle: Test\n---\n";
        let tree = crate::parser::parse(input, None);
        let parsed = collect_parsed_frontmatter_region(&tree).expect("frontmatter");
        let root = parsed.root().expect("yaml root");
        assert_eq!(root.document_count(), 1);
        assert_eq!(root.first_document_kind(), Some(YamlDocumentKind::BlockMap));
    }

    #[test]
    fn embedded_yaml_cst_attaches_to_frontmatter_and_hashpipe_hosts() {
        let input = "---\ntitle: Test\n---\n\n```{r}\n#| echo: false\nx <- 1\n```\n";
        let config = crate::config::Config {
            flavor: crate::config::Flavor::Quarto,
            ..Default::default()
        };
        let tree = crate::parser::parse(input, Some(config));
        let embedded = collect_embedded_yaml_cst(&tree);
        assert_eq!(embedded.len(), 2);
        assert!(
            embedded
                .iter()
                .any(|item| item.frontmatter_host().is_some())
        );
        assert!(embedded.iter().any(|item| item.hashpipe_host().is_some()));
    }

    #[test]
    fn embedded_yaml_cst_exposes_frontmatter_and_hashpipe_payloads() {
        let input = "---\ntitle: Test\n---\n\n```{r}\n#| fig-cap: |\n#|   A caption\nx <- 1\n```\n";
        let config = crate::config::Config {
            flavor: crate::config::Flavor::Quarto,
            ..Default::default()
        };
        let tree = crate::parser::parse(input, Some(config));
        let embedded = collect_embedded_yaml_cst(&tree);
        assert_eq!(embedded.len(), 2);

        let frontmatter = embedded
            .iter()
            .find(|item| item.frontmatter_host().is_some())
            .expect("frontmatter embedding");
        assert!(frontmatter.parsed().is_valid());
        assert_eq!(
            frontmatter.parsed().document_shape_summary().as_deref(),
            Some("Root docs=1 first=BlockMap")
        );

        let hashpipe = embedded
            .iter()
            .find(|item| item.hashpipe_host().is_some())
            .expect("hashpipe embedding");
        assert!(hashpipe.parsed().is_valid());
        assert!(hashpipe.parsed().to_region().content.contains("fig-cap: |"));
    }

    #[test]
    fn embedded_frontmatter_query_returns_typed_host_wrapper() {
        let input = "---\ntitle: Test\n---\n\nBody\n";
        let tree = crate::parser::parse(input, None);
        let embedded = collect_embedded_frontmatter_yaml_cst(&tree).expect("frontmatter embedding");
        let _host = embedded.frontmatter_host().expect("frontmatter host");
        assert!(embedded.hashpipe_host().is_none());
    }

    #[test]
    fn yaml_offset_map_includes_eof_position() {
        let input = "---\ntitle: Test\n---\n";
        let tree = crate::parser::parse(input, None);
        let parsed = collect_parsed_frontmatter_region(&tree).expect("frontmatter");
        let eof_yaml_offset = parsed.content().len();
        assert_eq!(
            parsed.host_offset_for_yaml_offset(eof_yaml_offset),
            Some(parsed.content_range().end)
        );
    }
}

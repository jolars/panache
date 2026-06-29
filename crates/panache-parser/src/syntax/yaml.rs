use std::ops::Range;

use crate::parser::utils::yaml_regions::hashpipe_language_and_prefix;
use crate::parser::yaml::YamlDiagnostic;
use crate::syntax::{
    AstNode, PanacheLanguage, SyntaxKind, SyntaxNode, YamlDocument, YamlScalarStyle,
};

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
}

#[derive(Debug, Clone)]
pub struct ParsedYamlRegion {
    region: YamlRegion,
    /// The host content node (`YAML_METADATA_CONTENT` / `HASHPIPE_YAML_CONTENT`)
    /// carrying the embedded `YAML_DOCUMENT` subtree, when the parser embedded a
    /// valid one. `None` for malformed YAML (opaque fallback). Validity and
    /// document shape derive from its presence — no standalone re-parse.
    embedded: Option<SyntaxNode>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedYamlRegionSnapshot {
    region: YamlRegion,
    parse_ok: bool,
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
    node: &'a SyntaxNode,
}

impl YamlAstRoot<'_> {
    pub fn kind(&self) -> YamlAstRootKind {
        YamlAstRootKind::Root
    }

    pub fn document_count(&self) -> usize {
        self.documents().count()
    }

    pub fn first_document_kind(&self) -> Option<YamlDocumentKind> {
        let doc = self.documents().next()?;
        if doc.block_map().is_some() {
            return Some(YamlDocumentKind::BlockMap);
        }
        if doc.block_sequence().is_some() {
            return Some(YamlDocumentKind::BlockSeq);
        }
        if let Some(scalar) = doc.scalar() {
            return Some(match scalar.style() {
                YamlScalarStyle::Literal | YamlScalarStyle::Folded => YamlDocumentKind::BlockScalar,
                _ => YamlDocumentKind::Flow,
            });
        }
        if doc.flow_map().is_some() || doc.flow_sequence().is_some() {
            return Some(YamlDocumentKind::Flow);
        }
        Some(YamlDocumentKind::Empty)
    }

    fn documents(&self) -> impl Iterator<Item = YamlDocument> + '_ {
        self.node.descendants().filter_map(YamlDocument::cast)
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

    fn from_diagnostic(diag: &YamlDiagnostic) -> Self {
        Self {
            offset: diag.byte_start,
            message: diag.message.to_string(),
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

    /// The embedded YAML document root, when the parser embedded a valid
    /// subtree. The host content node carries `YAML_DOCUMENT` children directly,
    /// which [`YamlAstRoot`] walks.
    pub fn root(&self) -> Option<YamlAstRoot<'_>> {
        self.embedded.as_ref().map(|node| YamlAstRoot { node })
    }

    pub fn root_kind(&self) -> Option<YamlAstRootKind> {
        self.root().map(|root| root.kind())
    }

    /// Whether the region's YAML is well-formed — i.e. the parser embedded a
    /// structured subtree rather than falling back to opaque tokens. This is the
    /// parser's own verdict, so it cannot diverge from the syntax-error channel.
    pub fn is_valid(&self) -> bool {
        self.embedded.is_some()
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

    pub fn host_range(&self) -> Range<usize> {
        self.region.host_range.clone()
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
        content_range,
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
    let embedded_frontmatter = embedded_frontmatter_stream(tree);
    collect_yaml_regions(tree)
        .into_iter()
        .map(|region| {
            // Validity and document shape come from the host-embedded subtree the
            // parser produced — no standalone re-parse. `None` (malformed YAML,
            // opaque fallback) means invalid; the parser's syntax-error channel
            // carries the diagnostic for those.
            let embedded = match &region.kind {
                YamlRegionKind::Frontmatter => embedded_frontmatter.clone(),
                YamlRegionKind::Hashpipe => embedded_hashpipe_stream(tree, &region.region_range),
            };
            ParsedYamlRegion { embedded, region }
        })
        .collect()
}

/// Locate the embedded YAML subtree under the frontmatter's
/// YAML_METADATA_CONTENT node, if the host parser embedded one (valid
/// frontmatter). The content node plays the stream container role for the
/// singleton-stream embedding, so we return it directly when the parser
/// embedded YAML. Returns `None` for malformed frontmatter, where the content
/// node holds opaque line tokens and the syntax-error channel carries the
/// diagnostic.
fn embedded_frontmatter_stream(tree: &SyntaxNode) -> Option<SyntaxNode> {
    let metadata = tree
        .descendants()
        .find(|node| node.kind() == SyntaxKind::YAML_METADATA)?;
    let content_node = metadata
        .children()
        .find(|child| child.kind() == SyntaxKind::YAML_METADATA_CONTENT)?;
    (!is_opaque_yaml_fallback(&content_node)).then_some(content_node)
}

/// Locate the embedded YAML subtree under the hashpipe preamble's
/// `HASHPIPE_YAML_CONTENT` node whose range matches `region_range`, when the
/// host parser embedded one (valid hashpipe YAML). Mirrors
/// [`embedded_frontmatter_stream`]. Returns `None` for malformed YAML (opaque
/// fallback).
fn embedded_hashpipe_stream(tree: &SyntaxNode, region_range: &Range<usize>) -> Option<SyntaxNode> {
    tree.descendants()
        .filter(|node| node.kind() == SyntaxKind::HASHPIPE_YAML_CONTENT)
        .find(|node| {
            let start: usize = node.text_range().start().into();
            let end: usize = node.text_range().end().into();
            start == region_range.start && end == region_range.end
        })
        .filter(|node| !is_opaque_yaml_fallback(node))
}

/// Whether a host YAML content node holds the parser's *opaque fallback* — raw
/// `TEXT` line tokens emitted when the YAML failed to validate — rather than an
/// embedded YAML subtree. Valid embeddings carry `YAML_*` nodes (or, for empty
/// content, nothing) and never a raw `TEXT` token, so its presence is the
/// reliable malformed-YAML fingerprint. Empty content (valid empty YAML) is not
/// opaque.
fn is_opaque_yaml_fallback(content_node: &SyntaxNode) -> bool {
    content_node
        .children_with_tokens()
        .any(|element| element.kind() == SyntaxKind::TEXT)
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
    match crate::parser::yaml::parse_yaml_report(input)
        .diagnostics
        .first()
    {
        Some(diag) => Err(YamlParseError::from_diagnostic(diag)),
        None => Ok(()),
    }
}

/// Like [`validate_yaml_text`], but validates against the real consumers of a
/// given (flavor, location) rather than the abstract YAML 1.2 substrate. Use
/// this for in-document frontmatter / hashpipe regions so the verdict matches
/// the parser's own context-aware syntax-error channel (e.g. pandoc accepts a
/// tab as indentation that the 1.2 substrate rejects). See
/// `tests/yaml/consumer-matrix.md`.
pub fn validate_yaml_text_with_context(
    input: &str,
    ctx: crate::parser::yaml::YamlValidationContext,
) -> Result<(), YamlParseError> {
    match crate::parser::yaml::locate_yaml_diagnostic_ctx(input, "", ctx) {
        Some((diag, _start, _end)) => Err(YamlParseError::from_diagnostic(&diag)),
        None => Ok(()),
    }
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
    // Rebuild the prefix-stripped YAML payload (used for the region's `content`
    // shape view). Host↔stripped offset mapping is no longer needed here: the
    // parser embeds a host-aligned YAML subtree and surfaces malformed-YAML
    // diagnostics through its own syntax-error channel.
    let mut collected = String::new();
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
        collected.push_str(payload);
        collected.push_str(eol);
        offset += line.len();
    }
    let start = content_start;
    let region_end = content_start + offset;
    let id = format!("hashpipe:{}:{}:{}", language, host_start, start);
    Some(YamlRegion {
        id,
        kind: YamlRegionKind::Hashpipe,
        host_range: host_start..host_end,
        region_range: start..region_end,
        content_range: start..region_end,
        content: collected,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsed_yaml_regions_include_frontmatter_and_hashpipe_cst_roots() {
        let input = "---\ntitle: Test\n---\n\n```{r}\n#| echo: false\n1 + 1\n```\n";
        let config = crate::options::ParserOptions {
            flavor: crate::options::Flavor::Quarto,
            extensions: crate::options::Extensions::for_flavor(crate::options::Flavor::Quarto),
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
    fn parsed_hashpipe_region_validity_derives_from_embedded_subtree() {
        let config = crate::options::ParserOptions {
            flavor: crate::options::Flavor::Quarto,
            extensions: crate::options::Extensions::for_flavor(crate::options::Flavor::Quarto),
            ..Default::default()
        };
        // Malformed hashpipe YAML: no embedded subtree → invalid, no root.
        let bad = crate::parser::parse("```{r}\n#| echo: [\n1 + 1\n```\n", Some(config.clone()));
        let bad_region = collect_parsed_yaml_regions(&bad)
            .into_iter()
            .find(|region| region.is_hashpipe())
            .expect("hashpipe region");
        assert!(!bad_region.is_valid());
        assert!(bad_region.root().is_none());

        // Well-formed hashpipe YAML: embedded subtree → valid, root present.
        let good = crate::parser::parse("```{r}\n#| echo: false\n1 + 1\n```\n", Some(config));
        let good_region = collect_parsed_yaml_regions(&good)
            .into_iter()
            .find(|region| region.is_hashpipe())
            .expect("hashpipe region");
        assert!(good_region.is_valid());
        assert!(good_region.root().is_some());
    }

    #[test]
    fn empty_frontmatter_is_valid() {
        // Valid empty YAML embeds an empty content node (no document, no opaque
        // TEXT). It must still count as valid — not malformed.
        let tree = crate::parser::parse("---\n---\n\nbody\n", None);
        let parsed = collect_parsed_frontmatter_region(&tree).expect("frontmatter");
        assert!(parsed.is_valid());
    }

    #[test]
    fn malformed_frontmatter_is_invalid() {
        let tree = crate::parser::parse("---\ntitle: [\n---\n", None);
        let parsed = collect_parsed_frontmatter_region(&tree).expect("frontmatter");
        assert!(!parsed.is_valid());
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
        let config = crate::options::ParserOptions {
            flavor: crate::options::Flavor::Quarto,
            extensions: crate::options::Extensions::for_flavor(crate::options::Flavor::Quarto),
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
        let config = crate::options::ParserOptions {
            flavor: crate::options::Flavor::Quarto,
            extensions: crate::options::Extensions::for_flavor(crate::options::Flavor::Quarto),
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
}

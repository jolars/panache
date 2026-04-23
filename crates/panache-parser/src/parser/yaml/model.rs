#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum YamlInputKind {
    #[default]
    Plain,
    Hashpipe,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShadowYamlOptions {
    pub enabled: bool,
    pub input_kind: YamlInputKind,
}

impl Default for ShadowYamlOptions {
    fn default() -> Self {
        Self {
            enabled: false,
            input_kind: YamlInputKind::Plain,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShadowYamlOutcome {
    SkippedDisabled,
    PrototypeParsed,
    PrototypeRejected,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShadowYamlReport {
    pub outcome: ShadowYamlOutcome,
    pub shadow_reason: &'static str,
    pub input_kind: YamlInputKind,
    pub input_len_bytes: usize,
    pub line_count: usize,
    pub normalized_input: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YamlDiagnostic {
    pub code: &'static str,
    pub message: &'static str,
    pub byte_start: usize,
    pub byte_end: usize,
}

#[derive(Debug, Clone)]
pub struct YamlParseReport {
    pub tree: Option<crate::syntax::SyntaxNode>,
    pub diagnostics: Vec<YamlDiagnostic>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum YamlToken {
    Indent,
    Dedent,
    DocumentStart,
    DocumentEnd,
    Directive,
    Anchor,
    Alias,
    Key,
    Colon,
    FlowMapStart,
    FlowMapEnd,
    FlowSeqStart,
    FlowSeqEnd,
    Comma,
    Whitespace,
    Tag,
    BlockScalarHeader,
    BlockScalarContent,
    Scalar,
    Comment,
    Newline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct YamlTokenSpan<'a> {
    pub kind: YamlToken,
    pub text: &'a str,
    pub byte_start: usize,
    pub byte_end: usize,
}

impl<'a> YamlTokenSpan<'a> {
    pub fn new(kind: YamlToken, text: &'a str) -> Self {
        Self {
            kind,
            text,
            byte_start: 0,
            byte_end: 0,
        }
    }
}

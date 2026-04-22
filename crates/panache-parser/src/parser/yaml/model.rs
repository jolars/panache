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
pub struct BasicYamlEntry<'a> {
    pub key: &'a str,
    pub value: &'a str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum YamlShadowTokenKind {
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
pub struct YamlShadowToken<'a> {
    pub kind: YamlShadowTokenKind,
    pub text: &'a str,
}

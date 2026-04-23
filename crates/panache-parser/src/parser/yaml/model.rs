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

pub mod diagnostic_codes {
    pub const LEX_ERROR: &str = "YAML_LEX_ERROR";
    pub const LEX_TRAILING_CONTENT_AFTER_DOCUMENT_START: &str =
        "YAML_LEX_TRAILING_CONTENT_AFTER_DOCUMENT_START";
    pub const LEX_TRAILING_CONTENT_AFTER_DOCUMENT_END: &str =
        "YAML_LEX_TRAILING_CONTENT_AFTER_DOCUMENT_END";
    pub const LEX_INVALID_DOUBLE_QUOTED_ESCAPE: &str = "YAML_LEX_INVALID_DOUBLE_QUOTED_ESCAPE";
    pub const LEX_WRONG_INDENTED_FLOW: &str = "YAML_LEX_WRONG_INDENTED_FLOW";

    pub const PARSE_EXPECTED_FLOW_SEQUENCE_START: &str = "YAML_PARSE_EXPECTED_FLOW_SEQUENCE_START";
    pub const PARSE_TRAILING_CONTENT_AFTER_FLOW_END: &str =
        "YAML_PARSE_TRAILING_CONTENT_AFTER_FLOW_END";
    pub const PARSE_INVALID_FLOW_SEQUENCE_COMMA: &str = "YAML_PARSE_INVALID_FLOW_SEQUENCE_COMMA";
    pub const PARSE_UNTERMINATED_FLOW_SEQUENCE: &str = "YAML_PARSE_UNTERMINATED_FLOW_SEQUENCE";
    pub const PARSE_EXPECTED_FLOW_MAP_START: &str = "YAML_PARSE_EXPECTED_FLOW_MAP_START";
    pub const PARSE_UNTERMINATED_FLOW_MAP: &str = "YAML_PARSE_UNTERMINATED_FLOW_MAP";
    pub const PARSE_UNEXPECTED_FLOW_CLOSER: &str = "YAML_PARSE_UNEXPECTED_FLOW_CLOSER";
    pub const PARSE_UNEXPECTED_INDENT: &str = "YAML_PARSE_UNEXPECTED_INDENT";
    pub const PARSE_UNEXPECTED_DEDENT: &str = "YAML_PARSE_UNEXPECTED_DEDENT";
    pub const PARSE_INVALID_KEY_TOKEN: &str = "YAML_PARSE_INVALID_KEY_TOKEN";
    pub const PARSE_MISSING_COLON: &str = "YAML_PARSE_MISSING_COLON";
    pub const PARSE_UNTERMINATED_BLOCK_MAP: &str = "YAML_PARSE_UNTERMINATED_BLOCK_MAP";
    pub const PARSE_DIRECTIVE_AFTER_CONTENT: &str = "YAML_PARSE_DIRECTIVE_AFTER_CONTENT";
    pub const PARSE_DIRECTIVE_WITHOUT_DOCUMENT_START: &str =
        "YAML_PARSE_DIRECTIVE_WITHOUT_DOCUMENT_START";
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
    BlockSeqEntry,
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

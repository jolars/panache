use rowan::{GreenNodeBuilder, Language, SyntaxNode};

use crate::bibtex::Span;

#[allow(non_camel_case_types)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u16)]
pub enum RisSyntaxKind {
    WHITESPACE = 0,
    NEWLINE,
    TEXT,
    DASH,

    RIS_FILE,
    RECORD,
    TAG,
    TAG_NAME,
    TAG_VALUE,
}

impl From<RisSyntaxKind> for rowan::SyntaxKind {
    fn from(kind: RisSyntaxKind) -> Self {
        Self(kind as u16)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RisLanguage {}

impl Language for RisLanguage {
    type Kind = RisSyntaxKind;

    fn kind_from_raw(raw: rowan::SyntaxKind) -> Self::Kind {
        unsafe { std::mem::transmute::<u16, RisSyntaxKind>(raw.0) }
    }

    fn kind_to_raw(kind: Self::Kind) -> rowan::SyntaxKind {
        kind.into()
    }
}

pub type RisNode = SyntaxNode<RisLanguage>;

pub fn parse_ris_cst(input: &str) -> RisNode {
    let parser = RisCstParser::new(input);
    parser.parse()
}

pub(crate) fn parse_ris_entries(input: &str) -> Result<Vec<(String, Span)>, String> {
    let root = parse_ris_cst(input);
    let mut entries = Vec::new();

    for record in root
        .children()
        .filter(|node| node.kind() == RisSyntaxKind::RECORD)
    {
        if let Some((id, span)) = extract_record_id(&record) {
            entries.push((id, span));
        }
    }

    Ok(entries)
}

fn extract_record_id(record: &RisNode) -> Option<(String, Span)> {
    for tag in record
        .children()
        .filter(|node| node.kind() == RisSyntaxKind::TAG)
    {
        let name = tag
            .children()
            .find(|node| node.kind() == RisSyntaxKind::TAG_NAME)
            .and_then(|node| first_text(&node))
            .unwrap_or_default();
        if name != "ID" {
            continue;
        }

        if let Some((value, span)) = tag
            .children()
            .find(|node| node.kind() == RisSyntaxKind::TAG_VALUE)
            .and_then(|node| extract_text_span(&node))
        {
            if !value.is_empty() {
                return Some((value, span));
            }
        }
    }
    None
}

fn first_text(node: &RisNode) -> Option<String> {
    node.children_with_tokens()
        .filter_map(|element| element.into_token())
        .find(|token| token.kind() == RisSyntaxKind::TEXT)
        .map(|token| token.text().to_string())
}

fn extract_text_span(node: &RisNode) -> Option<(String, Span)> {
    let token = node
        .children_with_tokens()
        .filter_map(|element| element.into_token())
        .find(|token| token.kind() == RisSyntaxKind::TEXT)?;
    let raw = token.text();
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let leading = raw.find(trimmed).unwrap_or(0);
    let start = usize::from(token.text_range().start()) + leading;
    let end = start + trimmed.len();
    Some((trimmed.to_string(), Span { start, end }))
}

struct RisCstParser<'a> {
    input: &'a str,
    bytes: &'a [u8],
    pos: usize,
    builder: GreenNodeBuilder<'a>,
}

impl<'a> RisCstParser<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            input,
            bytes: input.as_bytes(),
            pos: 0,
            builder: GreenNodeBuilder::new(),
        }
    }

    fn parse(mut self) -> RisNode {
        self.builder.start_node(RisSyntaxKind::RIS_FILE.into());
        while self.pos < self.bytes.len() {
            self.emit_record();
        }
        self.builder.finish_node();
        RisNode::new_root(self.builder.finish())
    }

    fn emit_record(&mut self) {
        self.builder.start_node(RisSyntaxKind::RECORD.into());
        let mut at_line_start = true;
        while self.pos < self.bytes.len() {
            if at_line_start && self.is_tag_start() {
                let name = self.parse_tag_name();
                let is_end = self.emit_tag(&name);
                at_line_start = true;
                if is_end {
                    break;
                }
                continue;
            }

            if self.emit_newline() {
                at_line_start = true;
                continue;
            }
            if self.emit_whitespace(at_line_start) {
                continue;
            }
            self.emit_text_run();
            at_line_start = false;
        }
        self.builder.finish_node();
    }

    fn emit_tag(&mut self, name: &str) -> bool {
        self.builder.start_node(RisSyntaxKind::TAG.into());
        self.builder.start_node(RisSyntaxKind::TAG_NAME.into());
        self.builder.token(RisSyntaxKind::TEXT.into(), name);
        self.builder.finish_node();

        self.emit_whitespace(false);
        if self.peek_byte() == Some(b'-') {
            self.builder.token(RisSyntaxKind::DASH.into(), "-");
            self.pos += 1;
        }

        let (value_start, value_end, newline) = self.parse_tag_value();
        self.builder.start_node(RisSyntaxKind::TAG_VALUE.into());
        if value_end > value_start {
            self.builder.token(
                RisSyntaxKind::TEXT.into(),
                &self.input[value_start..value_end],
            );
        }
        self.builder.finish_node();

        self.builder.finish_node();

        if let Some(text) = newline {
            self.builder.token(RisSyntaxKind::NEWLINE.into(), text);
        }

        name == "ER"
    }

    fn emit_whitespace(&mut self, _preserve_line_start: bool) -> bool {
        let start = self.pos;
        while let Some(b) = self.peek_byte() {
            if b == b' ' || b == b'\t' {
                self.pos += 1;
            } else {
                break;
            }
        }
        if self.pos == start {
            return false;
        }
        self.builder.token(
            RisSyntaxKind::WHITESPACE.into(),
            &self.input[start..self.pos],
        );
        true
    }

    fn emit_newline(&mut self) -> bool {
        match self.peek_byte() {
            Some(b'\n') => {
                self.builder.token(RisSyntaxKind::NEWLINE.into(), "\n");
                self.pos += 1;
                true
            }
            Some(b'\r') => {
                if self.peek_next_byte() == Some(b'\n') {
                    self.builder.token(RisSyntaxKind::NEWLINE.into(), "\r\n");
                    self.pos += 2;
                } else {
                    self.builder.token(RisSyntaxKind::NEWLINE.into(), "\r");
                    self.pos += 1;
                }
                true
            }
            _ => false,
        }
    }

    fn emit_text_run(&mut self) {
        let start = self.pos;
        while let Some(b) = self.peek_byte() {
            if b == b'\n' || b == b'\r' || b == b' ' || b == b'\t' {
                break;
            }
            self.pos += 1;
        }
        if self.pos > start {
            self.builder
                .token(RisSyntaxKind::TEXT.into(), &self.input[start..self.pos]);
        } else if let Some(ch) = self.input[self.pos..].chars().next() {
            let mut buf = [0u8; 4];
            let text = ch.encode_utf8(&mut buf);
            self.pos += ch.len_utf8();
            self.builder.token(RisSyntaxKind::TEXT.into(), text);
        }
    }

    fn is_tag_start(&self) -> bool {
        if self.pos + 3 > self.bytes.len() {
            return false;
        }
        let first = self.bytes[self.pos];
        let second = self.bytes[self.pos + 1];
        if !is_tag_char(first) || !is_tag_char(second) {
            return false;
        }
        let mut idx = self.pos + 2;
        let mut saw_space = false;
        while let Some(b) = self.bytes.get(idx).copied() {
            if b == b' ' || b == b'\t' {
                saw_space = true;
                idx += 1;
                continue;
            }
            break;
        }
        saw_space && self.bytes.get(idx) == Some(&b'-')
    }

    fn parse_tag_name(&mut self) -> String {
        if self.pos + 2 > self.bytes.len() {
            return String::new();
        }
        let first = self.bytes[self.pos];
        let second = self.bytes[self.pos + 1];
        if !is_tag_char(first) || !is_tag_char(second) {
            return String::new();
        }
        self.pos += 2;
        self.input[self.pos - 2..self.pos].to_string()
    }

    fn parse_tag_value(&mut self) -> (usize, usize, Option<&'a str>) {
        let start = self.pos;
        while let Some(b) = self.peek_byte() {
            if b == b'\n' || b == b'\r' {
                break;
            }
            self.pos += 1;
        }
        let end = self.pos;
        let newline = match self.peek_byte() {
            Some(b'\n') => {
                self.pos += 1;
                Some("\n")
            }
            Some(b'\r') => {
                if self.peek_next_byte() == Some(b'\n') {
                    self.pos += 2;
                    Some("\r\n")
                } else {
                    self.pos += 1;
                    Some("\r")
                }
            }
            _ => None,
        };
        (start, end, newline)
    }

    fn peek_byte(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn peek_next_byte(&self) -> Option<u8> {
        self.bytes.get(self.pos + 1).copied()
    }
}

fn is_tag_char(b: u8) -> bool {
    (b'A'..=b'Z').contains(&b) || (b'0'..=b'9').contains(&b)
}

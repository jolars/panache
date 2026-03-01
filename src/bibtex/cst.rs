//! Rowan-based BibTeX CST parser.

use rowan::{GreenNodeBuilder, Language};

#[allow(non_camel_case_types)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u16)]
pub enum BibTexSyntaxKind {
    WHITESPACE = 0,
    NEWLINE,
    TEXT,
    COMMENT,
    AT_MARK,
    LBRACE,
    RBRACE,
    LPAREN,
    RPAREN,
    COMMA,
    EQUALS,
    QUOTE,

    BIBTEX_FILE,
    ENTRY,
    ENTRY_TYPE,
    ENTRY_KEY,
    FIELD,
    FIELD_NAME,
    FIELD_VALUE,
    STRING,
}

impl From<BibTexSyntaxKind> for rowan::SyntaxKind {
    fn from(kind: BibTexSyntaxKind) -> Self {
        Self(kind as u16)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BibTexLanguage {}

impl Language for BibTexLanguage {
    type Kind = BibTexSyntaxKind;

    fn kind_from_raw(raw: rowan::SyntaxKind) -> Self::Kind {
        unsafe { std::mem::transmute::<u16, BibTexSyntaxKind>(raw.0) }
    }

    fn kind_to_raw(kind: Self::Kind) -> rowan::SyntaxKind {
        kind.into()
    }
}

pub type BibTexNode = rowan::SyntaxNode<BibTexLanguage>;

pub fn parse_bibtex_cst(input: &str) -> BibTexNode {
    let parser = CstParser::new(input);
    parser.parse()
}

struct CstParser<'a> {
    input: &'a str,
    bytes: &'a [u8],
    pos: usize,
    builder: GreenNodeBuilder<'a>,
}

impl<'a> CstParser<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            input,
            bytes: input.as_bytes(),
            pos: 0,
            builder: GreenNodeBuilder::new(),
        }
    }

    fn parse(mut self) -> BibTexNode {
        self.builder
            .start_node(BibTexSyntaxKind::BIBTEX_FILE.into());
        while self.pos < self.bytes.len() {
            if self.peek_byte() == Some(b'@') {
                self.emit_entry();
            } else {
                self.emit_trivia_or_text();
            }
        }
        self.builder.finish_node();
        BibTexNode::new_root(self.builder.finish())
    }

    fn emit_entry(&mut self) {
        self.builder.start_node(BibTexSyntaxKind::ENTRY.into());

        self.builder.token(BibTexSyntaxKind::AT_MARK.into(), "@");
        self.pos += 1;

        self.emit_whitespace();
        let entry_type = self.parse_identifier().to_string();
        if !entry_type.is_empty() {
            self.builder.start_node(BibTexSyntaxKind::ENTRY_TYPE.into());
            self.builder
                .token(BibTexSyntaxKind::TEXT.into(), &entry_type);
            self.builder.finish_node();
        }

        self.emit_whitespace();
        let open = self.peek_byte();
        match open {
            Some(b'{') => {
                self.builder.token(BibTexSyntaxKind::LBRACE.into(), "{");
                self.pos += 1;
            }
            Some(b'(') => {
                self.builder.token(BibTexSyntaxKind::LPAREN.into(), "(");
                self.pos += 1;
            }
            _ => {}
        }

        self.emit_whitespace();
        if entry_type.eq_ignore_ascii_case("string") {
            let key = self.parse_until(|b| b == b'}' || b == b')').to_string();
            if !key.is_empty() {
                self.builder.start_node(BibTexSyntaxKind::ENTRY_KEY.into());
                self.builder.token(BibTexSyntaxKind::TEXT.into(), &key);
                self.builder.finish_node();
            }

            if self.peek_byte() == Some(b'}') {
                self.builder.token(BibTexSyntaxKind::RBRACE.into(), "}");
                self.pos += 1;
            } else if self.peek_byte() == Some(b')') {
                self.builder.token(BibTexSyntaxKind::RPAREN.into(), ")");
                self.pos += 1;
            }
        } else {
            let key = self
                .parse_until(|b| b == b',' || b == b'}' || b == b')')
                .to_string();
            if !key.is_empty() {
                self.builder.start_node(BibTexSyntaxKind::ENTRY_KEY.into());
                self.builder.token(BibTexSyntaxKind::TEXT.into(), &key);
                self.builder.finish_node();
            }

            self.emit_whitespace();
            if self.peek_byte() == Some(b',') {
                self.builder.token(BibTexSyntaxKind::COMMA.into(), ",");
                self.pos += 1;
            }

            loop {
                self.emit_whitespace();
                match self.peek_byte() {
                    Some(b'}') => {
                        self.builder.token(BibTexSyntaxKind::RBRACE.into(), "}");
                        self.pos += 1;
                        break;
                    }
                    Some(b')') => {
                        self.builder.token(BibTexSyntaxKind::RPAREN.into(), ")");
                        self.pos += 1;
                        break;
                    }
                    Some(_) => {
                        self.emit_field();
                    }
                    None => break,
                }
            }
        }

        self.builder.finish_node();
    }

    fn emit_field(&mut self) {
        self.builder.start_node(BibTexSyntaxKind::FIELD.into());
        let name = self.parse_identifier().to_string();
        if !name.is_empty() {
            self.builder.start_node(BibTexSyntaxKind::FIELD_NAME.into());
            self.builder.token(BibTexSyntaxKind::TEXT.into(), &name);
            self.builder.finish_node();
        }

        self.emit_whitespace();
        if self.peek_byte() == Some(b'=') {
            self.builder.token(BibTexSyntaxKind::EQUALS.into(), "=");
            self.pos += 1;
        }

        self.emit_whitespace();
        self.emit_field_value();
        self.emit_whitespace();
        if self.peek_byte() == Some(b',') {
            self.builder.token(BibTexSyntaxKind::COMMA.into(), ",");
            self.pos += 1;
        }
        self.builder.finish_node();
    }

    fn emit_field_value(&mut self) {
        self.builder
            .start_node(BibTexSyntaxKind::FIELD_VALUE.into());
        match self.peek_byte() {
            Some(b'"') => {
                self.builder.token(BibTexSyntaxKind::QUOTE.into(), "\"");
                self.pos += 1;
                let value = self.parse_quoted().to_string();
                if !value.is_empty() {
                    self.builder.start_node(BibTexSyntaxKind::STRING.into());
                    self.builder.token(BibTexSyntaxKind::TEXT.into(), &value);
                    self.builder.finish_node();
                }
                if self.peek_byte() == Some(b'"') {
                    self.builder.token(BibTexSyntaxKind::QUOTE.into(), "\"");
                    self.pos += 1;
                }
            }
            Some(b'{') => {
                self.builder.token(BibTexSyntaxKind::LBRACE.into(), "{");
                self.pos += 1;
                let value = self.parse_braced().to_string();
                if !value.is_empty() {
                    self.builder.start_node(BibTexSyntaxKind::STRING.into());
                    self.builder.token(BibTexSyntaxKind::TEXT.into(), &value);
                    self.builder.finish_node();
                }
                if self.peek_byte() == Some(b'}') {
                    self.builder.token(BibTexSyntaxKind::RBRACE.into(), "}");
                    self.pos += 1;
                }
            }
            Some(_) => {
                let value = self
                    .parse_until(|b| b == b',' || b == b'}' || b == b')')
                    .to_string();
                if !value.is_empty() {
                    self.builder.start_node(BibTexSyntaxKind::STRING.into());
                    self.builder
                        .token(BibTexSyntaxKind::TEXT.into(), value.trim());
                    self.builder.finish_node();
                }
            }
            None => {}
        }
        self.builder.finish_node();
    }

    fn emit_trivia_or_text(&mut self) {
        let byte = self.peek_byte().unwrap();
        if byte == b'%' {
            let comment = self.parse_until(|b| b == b'\n').to_string();
            self.builder
                .token(BibTexSyntaxKind::COMMENT.into(), &comment);
            return;
        }

        if (byte as char).is_whitespace() {
            self.emit_whitespace();
            return;
        }

        let text = self
            .parse_until(|b| b == b'@' || (b as char).is_whitespace())
            .to_string();
        if !text.is_empty() {
            self.builder.token(BibTexSyntaxKind::TEXT.into(), &text);
        }
    }

    fn emit_whitespace(&mut self) {
        while let Some(byte) = self.peek_byte() {
            let ch = byte as char;
            if ch == '\n' {
                self.builder.token(BibTexSyntaxKind::NEWLINE.into(), "\n");
                self.pos += 1;
                continue;
            }
            if ch.is_whitespace() {
                let start = self.pos;
                self.pos += 1;
                while let Some(next) = self.peek_byte() {
                    let next_ch = next as char;
                    if next_ch.is_whitespace() && next_ch != '\n' {
                        self.pos += 1;
                    } else {
                        break;
                    }
                }
                let text = &self.input[start..self.pos];
                self.builder
                    .token(BibTexSyntaxKind::WHITESPACE.into(), text);
                continue;
            }
            break;
        }
    }

    fn parse_identifier(&mut self) -> &str {
        let start = self.pos;
        while let Some(byte) = self.peek_byte() {
            let ch = byte as char;
            if ch.is_alphanumeric() || ch == '_' || ch == '-' {
                self.pos += 1;
            } else {
                break;
            }
        }
        &self.input[start..self.pos]
    }

    fn parse_until<F>(&mut self, stop: F) -> &str
    where
        F: Fn(u8) -> bool,
    {
        let start = self.pos;
        while let Some(byte) = self.peek_byte() {
            if stop(byte) {
                break;
            }
            self.pos += 1;
        }
        &self.input[start..self.pos]
    }

    fn parse_quoted(&mut self) -> &str {
        let start = self.pos;
        while let Some(byte) = self.peek_byte() {
            if byte == b'"' {
                break;
            }
            if byte == b'\\' {
                self.pos += 2;
                continue;
            }
            self.pos += 1;
        }
        &self.input[start..self.pos]
    }

    fn parse_braced(&mut self) -> &str {
        let start = self.pos;
        let mut depth = 1usize;
        while let Some(byte) = self.peek_byte() {
            if byte == b'{' {
                depth += 1;
                self.pos += 1;
                continue;
            }
            if byte == b'}' {
                depth -= 1;
                if depth == 0 {
                    break;
                }
                self.pos += 1;
                continue;
            }
            if byte == b'\\' {
                self.pos += 2;
                continue;
            }
            self.pos += 1;
        }
        &self.input[start..self.pos]
    }

    fn peek_byte(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_entry() {
        let input = "@article{doe2020, title={Title}}";
        let node = parse_bibtex_cst(input);
        let text = node.text().to_string();
        assert_eq!(text, input);
    }
}

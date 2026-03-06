//! Minimal BibTeX parser with span tracking.

use std::collections::HashMap;

use crate::bib::{BibError, BibtexDatabase, ParsedEntry, Span};

#[derive(Debug, Clone)]
pub struct BibtexField {
    pub name: String,
    pub value: String,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct BibtexEntry {
    pub entry_type: String,
    pub key: String,
    pub key_span: Span,
    pub fields: Vec<BibtexField>,
    pub span: Span,
}

pub fn parse_bibtex(input: &str) -> BibtexDatabase {
    let mut parser = Parser::new(input);
    parser.parse_database()
}

/// Parse BibTeX file and return unified entry format.
///
/// Returns (entries, errors) where entries is Vec<ParsedEntry>.
pub fn parse_bibtex_full(input: &str) -> (Vec<ParsedEntry>, Vec<BibError>) {
    let database = parse_bibtex(input);
    let mut entries = Vec::new();

    for entry in &database.entries {
        let mut fields = HashMap::new();
        for field in &entry.fields {
            fields.insert(field.name.clone(), field.value.clone());
        }

        entries.push((
            entry.key.clone(),
            Some(entry.entry_type.clone()),
            fields,
            entry.key_span,
        ));
    }

    (entries, database.errors)
}

struct Parser<'a> {
    input: &'a str,
    bytes: &'a [u8],
    pos: usize,
    errors: Vec<BibError>,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            input,
            bytes: input.as_bytes(),
            pos: 0,
            errors: Vec::new(),
        }
    }

    fn parse_database(&mut self) -> BibtexDatabase {
        let mut entries = Vec::new();
        let mut entry_index = HashMap::new();

        while self.pos < self.bytes.len() {
            self.skip_whitespace_and_comments();
            if self.pos >= self.bytes.len() {
                break;
            }

            if self.bytes[self.pos] == b'@' {
                if let Some(entry) = self.parse_entry() {
                    let key_lower = entry.key.to_lowercase();
                    entry_index
                        .entry(key_lower)
                        .or_insert_with(|| entries.len());
                    entries.push(entry);
                } else {
                    self.advance_to_next_entry();
                }
            } else {
                self.pos += 1;
            }
        }

        BibtexDatabase {
            entries,
            entry_index,
            errors: std::mem::take(&mut self.errors),
        }
    }

    fn parse_entry(&mut self) -> Option<BibtexEntry> {
        let entry_start = self.pos;
        self.pos += 1;

        let entry_type = self.parse_identifier();
        if entry_type.is_empty() {
            self.push_error(
                "Expected entry type after '@'",
                Some(Span {
                    start: entry_start,
                    end: self.pos,
                }),
            );
            return None;
        }

        self.skip_whitespace_and_comments();
        let open = self.peek_byte()?;
        if open != b'{' && open != b'(' {
            self.push_error(
                "Expected '{' or '(' after entry type",
                Some(Span {
                    start: entry_start,
                    end: self.pos,
                }),
            );
            return None;
        }
        self.pos += 1;

        self.skip_whitespace_and_comments();
        let key_start = self.pos;
        let key = self.parse_until(|b| b == b',' || b == b'}' || b == b')');
        let key = key.trim();
        let key_end = key_start + key.len();
        let key_span = Span {
            start: key_start,
            end: key_end,
        };

        if key.is_empty() {
            self.push_error(
                "Missing citation key in entry",
                Some(Span {
                    start: key_start,
                    end: self.pos,
                }),
            );
        }

        self.skip_whitespace_and_comments();
        if self.peek_byte() == Some(b',') {
            self.pos += 1;
        }

        let mut fields = Vec::new();
        loop {
            self.skip_whitespace_and_comments();
            match self.peek_byte() {
                Some(b'}') | Some(b')') => {
                    self.pos += 1;
                    break;
                }
                Some(_) => {
                    if let Some(field) = self.parse_field() {
                        fields.push(field);
                    } else {
                        self.advance_to_next_field_or_end();
                    }
                }
                None => break,
            }
        }

        let entry_end = self.pos;
        Some(BibtexEntry {
            entry_type,
            key: key.to_string(),
            key_span,
            fields,
            span: Span {
                start: entry_start,
                end: entry_end,
            },
        })
    }

    fn parse_field(&mut self) -> Option<BibtexField> {
        let start = self.pos;
        let name = self.parse_identifier();
        if name.is_empty() {
            self.push_error(
                "Expected field name",
                Some(Span {
                    start,
                    end: self.pos,
                }),
            );
            return None;
        }

        self.skip_whitespace_and_comments();
        if self.peek_byte() != Some(b'=') {
            self.push_error(
                "Expected '=' after field name",
                Some(Span {
                    start,
                    end: self.pos,
                }),
            );
            return None;
        }
        self.pos += 1;

        self.skip_whitespace_and_comments();
        let value_start = self.pos;
        let value = self.parse_value();
        let value_end = self.pos;

        self.skip_whitespace_and_comments();
        if self.peek_byte() == Some(b',') {
            self.pos += 1;
        }

        Some(BibtexField {
            name,
            value,
            span: Span {
                start: value_start,
                end: value_end,
            },
        })
    }

    fn parse_value(&mut self) -> String {
        match self.peek_byte() {
            Some(b'"') => self.parse_quoted_string(),
            Some(b'{') => self.parse_braced_string(),
            Some(_) => self
                .parse_until(|b| b == b',' || b == b'}' || b == b')')
                .trim()
                .to_string(),
            None => String::new(),
        }
    }

    fn parse_quoted_string(&mut self) -> String {
        let mut result = String::new();
        if self.peek_byte() != Some(b'"') {
            return result;
        }
        self.pos += 1;
        while let Some(b) = self.peek_byte() {
            if b == b'"' {
                self.pos += 1;
                break;
            }
            if b == b'\\' {
                self.pos += 1;
                if let Some(next) = self.peek_byte() {
                    result.push(next as char);
                    self.pos += 1;
                }
                continue;
            }
            result.push(b as char);
            self.pos += 1;
        }
        result
    }

    fn parse_braced_string(&mut self) -> String {
        let mut result = String::new();
        if self.peek_byte() != Some(b'{') {
            return result;
        }
        self.pos += 1;
        let mut depth = 1usize;
        while let Some(b) = self.peek_byte() {
            if b == b'{' {
                depth += 1;
                result.push('{');
                self.pos += 1;
                continue;
            }
            if b == b'}' {
                depth -= 1;
                self.pos += 1;
                if depth == 0 {
                    break;
                }
                result.push('}');
                continue;
            }
            if b == b'\\' {
                self.pos += 1;
                if let Some(next) = self.peek_byte() {
                    result.push(next as char);
                    self.pos += 1;
                }
                continue;
            }
            result.push(b as char);
            self.pos += 1;
        }
        result
    }

    fn parse_identifier(&mut self) -> String {
        let start = self.pos;
        while let Some(b) = self.peek_byte() {
            let ch = b as char;
            if ch.is_alphanumeric() || ch == '_' || ch == '-' {
                self.pos += 1;
            } else {
                break;
            }
        }
        self.input[start..self.pos].to_string()
    }

    fn parse_until<F>(&mut self, stop: F) -> String
    where
        F: Fn(u8) -> bool,
    {
        let start = self.pos;
        while let Some(b) = self.peek_byte() {
            if stop(b) {
                break;
            }
            self.pos += 1;
        }
        self.input[start..self.pos].to_string()
    }

    fn skip_whitespace_and_comments(&mut self) {
        loop {
            self.skip_whitespace();
            if self.peek_byte() == Some(b'%') {
                while let Some(b) = self.peek_byte() {
                    self.pos += 1;
                    if b == b'\n' {
                        break;
                    }
                }
                continue;
            }
            break;
        }
    }

    fn skip_whitespace(&mut self) {
        while let Some(b) = self.peek_byte() {
            if (b as char).is_whitespace() {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn advance_to_next_entry(&mut self) {
        while let Some(b) = self.peek_byte() {
            if b == b'@' {
                break;
            }
            self.pos += 1;
        }
    }

    fn advance_to_next_field_or_end(&mut self) {
        while let Some(b) = self.peek_byte() {
            if b == b',' || b == b'}' || b == b')' {
                break;
            }
            self.pos += 1;
        }
    }

    fn peek_byte(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn push_error(&mut self, message: &str, span: Option<Span>) {
        self.errors.push(BibError {
            message: message.to_string(),
            span,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_entry() {
        let input = "@article{doe2020, title=\"Title\", author={Doe, Jane}}";
        let db = parse_bibtex(input);
        assert_eq!(db.entries.len(), 1);
        let entry = &db.entries[0];
        assert_eq!(entry.entry_type, "article");
        assert_eq!(entry.key, "doe2020");
        assert_eq!(entry.fields.len(), 2);
    }

    #[test]
    fn parse_multiple_entries() {
        let input = "@book{key1, title=\"A\"}\n@misc{key2, note={B}}";
        let db = parse_bibtex(input);
        assert_eq!(db.entries.len(), 2);
        assert!(db.entry_index.contains_key("key1"));
        assert!(db.entry_index.contains_key("key2"));
    }

    #[test]
    fn parse_braced_value_with_nested() {
        let input = "@misc{key, note={A {nested} value}}";
        let db = parse_bibtex(input);
        assert_eq!(db.entries.len(), 1);
        let note = &db.entries[0].fields[0];
        assert!(note.value.contains("nested"));
    }
}

//! Parser for the CommonMark `spec.txt` example format.
//!
//! Format (each example):
//! ```text
//! ```````````````````````````````` example
//! <markdown input>
//! .
//! <expected HTML output>
//! ````````````````````````````````
//! ```
//!
//! Section names are tracked from the most recent ATX heading. Examples are
//! numbered sequentially starting at 1. The character `→` (U+2192) stands in
//! for a literal tab in spec.txt and is substituted before parsing, matching
//! commonmark-hs (`T.replace "→" "\t"`).

use std::path::Path;

const FENCE: &str = "````````````````````````````````";
const FENCE_OPEN_TAG: &str = "```````````````````````````````` example";

#[derive(Debug, Clone)]
pub struct SpecExample {
    pub number: u32,
    pub section: String,
    #[allow(dead_code)]
    pub start_line: u32,
    #[allow(dead_code)]
    pub end_line: u32,
    pub markdown: String,
    pub expected_html: String,
}

pub fn parse_spec(content: &str) -> Vec<SpecExample> {
    let content = content.replace('\u{2192}', "\t");
    let lines: Vec<&str> = content.lines().collect();

    let mut out = Vec::new();
    let mut section = String::new();
    let mut counter: u32 = 1;
    let mut i = 0usize;

    while i < lines.len() {
        let line = lines[i];
        if line == FENCE_OPEN_TAG {
            let start_line = (i + 1) as u32;
            i += 1;

            let mut markdown = String::new();
            while i < lines.len() && lines[i] != "." {
                markdown.push_str(lines[i]);
                markdown.push('\n');
                i += 1;
            }
            assert!(
                i < lines.len(),
                "unterminated example starting at line {start_line}: missing `.` separator"
            );
            i += 1;

            let mut expected_html = String::new();
            while i < lines.len() && lines[i] != FENCE {
                expected_html.push_str(lines[i]);
                expected_html.push('\n');
                i += 1;
            }
            assert!(
                i < lines.len(),
                "unterminated example starting at line {start_line}: missing closing fence"
            );
            let end_line = (i + 1) as u32;
            i += 1;

            out.push(SpecExample {
                number: counter,
                section: section.clone(),
                start_line,
                end_line,
                markdown,
                expected_html,
            });
            counter += 1;
            continue;
        }

        if let Some(rest) = line.strip_prefix('#') {
            section = rest.trim_start_matches('#').trim().to_string();
        }
        i += 1;
    }

    out
}

pub fn read_spec(path: &Path) -> Vec<SpecExample> {
    let content = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("failed to read spec file {}: {e}", path.display()));
    parse_spec(&content)
}

/// CommonMark-hs's HTML normalization for fair comparison: collapse the
/// `<li>\n` and `\n</li>` whitespace that some implementations emit.
pub fn normalize_html(html: &str) -> String {
    html.replace("<li>\n", "<li>").replace("\n</li>", "</li>")
}

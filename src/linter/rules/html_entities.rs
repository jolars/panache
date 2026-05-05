use std::collections::HashSet;
use std::sync::OnceLock;

use rowan::TextRange;

use crate::config::Config;
use crate::linter::diagnostics::{Diagnostic, DiagnosticNoteKind, Location};
use crate::linter::rules::Rule;
use crate::syntax::{SyntaxKind, SyntaxNode};

use panache_parser::entities::ENTITIES;

pub struct HtmlEntitiesRule;

impl Rule for HtmlEntitiesRule {
    fn name(&self) -> &str {
        "html-entities"
    }

    fn check(
        &self,
        tree: &SyntaxNode,
        input: &str,
        _config: &Config,
        _metadata: Option<&crate::metadata::DocumentMetadata>,
    ) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        for elem in tree.descendants_with_tokens() {
            let Some(token) = elem.into_token() else {
                continue;
            };
            if token.kind() != SyntaxKind::TEXT {
                continue;
            }
            if has_excluded_ancestor(&token) {
                continue;
            }

            let token_text = token.text();
            let token_start: u32 = token.text_range().start().into();

            for hit in scan_entity_candidates(token_text) {
                let abs_start = token_start + hit.start as u32;
                let abs_end = token_start + hit.end as u32;
                let range = TextRange::new(abs_start.into(), abs_end.into());
                let location = Location::from_range(range, input);

                let diag = match hit.kind {
                    Verdict::UnknownNamed => {
                        let mut d = Diagnostic::warning(
                            location,
                            "html-entities",
                            format!("Unknown HTML entity '{}'", &token_text[hit.start..hit.end]),
                        );
                        if let Some(suggestion) = nearest_named_entity(hit.name) {
                            d = d.with_note(
                                DiagnosticNoteKind::Help,
                                format!("did you mean '&{};'?", suggestion),
                            );
                        }
                        d
                    }
                    Verdict::MissingSemicolon => Diagnostic::warning(
                        location,
                        "html-entities",
                        format!("HTML entity '&{}' is missing a trailing ';'", hit.name),
                    )
                    .with_note(
                        DiagnosticNoteKind::Help,
                        format!("write '&{};' to encode the character", hit.name),
                    ),
                };

                diagnostics.push(diag);
            }
        }

        diagnostics
    }
}

#[derive(Debug)]
struct EntityHit<'a> {
    start: usize,
    end: usize,
    name: &'a str,
    kind: Verdict,
}

#[derive(Debug, Clone, Copy)]
enum Verdict {
    UnknownNamed,
    MissingSemicolon,
}

fn scan_entity_candidates(text: &str) -> Vec<EntityHit<'_>> {
    let mut hits = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'&' {
            i += 1;
            continue;
        }
        let name_start = i + 1;
        if name_start >= bytes.len() || !bytes[name_start].is_ascii_alphabetic() {
            i += 1;
            continue;
        }
        let mut j = name_start;
        while j < bytes.len() && bytes[j].is_ascii_alphanumeric() {
            j += 1;
            if j - name_start > MAX_ENTITY_NAME_LEN {
                break;
            }
        }
        let name = &text[name_start..j];
        let has_semi = j < bytes.len() && bytes[j] == b';';

        if has_semi {
            let end = j + 1;
            if !is_known_with_semi(name) {
                hits.push(EntityHit {
                    start: i,
                    end,
                    name,
                    kind: Verdict::UnknownNamed,
                });
            }
            i = end;
        } else {
            if is_known_with_semi(name) && !is_known_without_semi(name) {
                hits.push(EntityHit {
                    start: i,
                    end: j,
                    name,
                    kind: Verdict::MissingSemicolon,
                });
            }
            i = j.max(i + 1);
        }
    }
    hits
}

const MAX_ENTITY_NAME_LEN: usize = 31;

const EXCLUDED_ANCESTOR_KINDS: &[SyntaxKind] = &[
    SyntaxKind::INLINE_CODE,
    SyntaxKind::CODE_BLOCK,
    SyntaxKind::CODE_CONTENT,
    SyntaxKind::INLINE_HTML,
    SyntaxKind::HTML_BLOCK,
    SyntaxKind::HTML_BLOCK_TAG,
    SyntaxKind::HTML_BLOCK_CONTENT,
    SyntaxKind::TEX_BLOCK,
    SyntaxKind::INLINE_MATH,
    SyntaxKind::DISPLAY_MATH,
    SyntaxKind::MATH_CONTENT,
    SyntaxKind::RAW_INLINE,
    SyntaxKind::RAW_INLINE_CONTENT,
    SyntaxKind::AUTO_LINK,
    SyntaxKind::LINK_DEST,
    SyntaxKind::REFERENCE_DEFINITION,
    SyntaxKind::REFERENCE_URL,
    SyntaxKind::REFERENCE_TITLE,
    SyntaxKind::ATTRIBUTE,
    SyntaxKind::SPAN_ATTRIBUTES,
    SyntaxKind::CHUNK_OPTIONS,
    SyntaxKind::CHUNK_OPTION,
    SyntaxKind::CHUNK_OPTION_KEY,
    SyntaxKind::CHUNK_OPTION_VALUE,
    SyntaxKind::CODE_INFO,
    SyntaxKind::COMMENT,
    SyntaxKind::YAML_METADATA,
    SyntaxKind::YAML_METADATA_CONTENT,
    SyntaxKind::INLINE_EXEC,
    SyntaxKind::INLINE_EXEC_CONTENT,
    SyntaxKind::SHORTCODE,
    SyntaxKind::SHORTCODE_CONTENT,
];

fn has_excluded_ancestor(token: &panache_parser::syntax::SyntaxToken) -> bool {
    let mut node = token.parent();
    while let Some(n) = node {
        if EXCLUDED_ANCESTOR_KINDS.contains(&n.kind()) {
            return true;
        }
        node = n.parent();
    }
    false
}

fn entity_sets() -> &'static (HashSet<&'static str>, HashSet<&'static str>) {
    static SETS: OnceLock<(HashSet<&'static str>, HashSet<&'static str>)> = OnceLock::new();
    SETS.get_or_init(|| {
        let mut with_semi: HashSet<&'static str> = HashSet::new();
        let mut without_semi: HashSet<&'static str> = HashSet::new();
        for entry in ENTITIES.iter() {
            let raw = entry.entity;
            if let Some(rest) = raw.strip_prefix('&') {
                if let Some(name) = rest.strip_suffix(';') {
                    with_semi.insert(name);
                } else {
                    without_semi.insert(rest);
                }
            }
        }
        (with_semi, without_semi)
    })
}

fn is_known_with_semi(name: &str) -> bool {
    entity_sets().0.contains(name)
}

fn is_known_without_semi(name: &str) -> bool {
    entity_sets().1.contains(name)
}

fn nearest_named_entity(name: &str) -> Option<&'static str> {
    if name.is_empty() {
        return None;
    }
    let target = name;
    let max_distance = if name.len() <= 4 { 1 } else { 2 };
    let mut best: Option<(usize, &'static str)> = None;
    for candidate in entity_sets().0.iter() {
        if candidate.len().abs_diff(target.len()) > max_distance {
            continue;
        }
        let d = levenshtein(target, candidate);
        if d == 0 || d > max_distance {
            continue;
        }
        match best {
            Some((bd, _)) if d > bd => {}
            Some((bd, bc)) if d == bd && *candidate >= bc => {}
            _ => best = Some((d, candidate)),
        }
    }
    best.map(|(_, c)| c)
}

fn levenshtein(a: &str, b: &str) -> usize {
    let a_bytes = a.as_bytes();
    let b_bytes = b.as_bytes();
    let n = a_bytes.len();
    let m = b_bytes.len();
    if n == 0 {
        return m;
    }
    if m == 0 {
        return n;
    }
    let mut prev: Vec<usize> = (0..=m).collect();
    let mut curr = vec![0usize; m + 1];
    for i in 1..=n {
        curr[0] = i;
        for j in 1..=m {
            let cost = if a_bytes[i - 1] == b_bytes[j - 1] {
                0
            } else {
                1
            };
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[m]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_and_lint(input: &str) -> Vec<Diagnostic> {
        let config = Config::default();
        let tree = crate::parser::parse(input, Some(config.clone()));
        let rule = HtmlEntitiesRule;
        rule.check(&tree, input, &config, None)
    }

    #[test]
    fn flags_unknown_named_entity() {
        let diagnostics = parse_and_lint("This is &ellips; wrong.");
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "html-entities");
        assert!(diagnostics[0].message.contains("&ellips;"));
    }

    #[test]
    fn flags_missing_semicolon_when_name_resolves() {
        let diagnostics = parse_and_lint("Section &numero 5 of the report.");
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "html-entities");
        assert!(diagnostics[0].message.contains("&numero"));
    }

    #[test]
    fn does_not_flag_valid_entity() {
        let diagnostics = parse_and_lint("Use &hellip; for a horizontal ellipsis.");
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn does_not_flag_plain_ampersand_in_prose() {
        let diagnostics = parse_and_lint("Tom & Jerry are friends.");
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn does_not_flag_at_and_t() {
        let diagnostics = parse_and_lint("AT&T is a company.");
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn does_not_flag_inside_code_span() {
        let diagnostics = parse_and_lint("Use `&ellips;` inline.");
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn does_not_flag_inside_fenced_code_block() {
        let input = "```\n&ellips;\n```\n";
        let diagnostics = parse_and_lint(input);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn does_not_flag_inside_url() {
        let diagnostics = parse_and_lint("[link](http://example.com/?a=1&ellips=2&b=3)");
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn suggests_close_match_in_note() {
        let diagnostics = parse_and_lint("Try &hellp;.");
        assert_eq!(diagnostics.len(), 1);
        let d = &diagnostics[0];
        assert!(
            d.notes.iter().any(|n| n.message.contains("&hellip;")),
            "expected suggestion note pointing to &hellip;, got: {:?}",
            d.notes
        );
    }

    #[test]
    fn suggestion_breaks_ties_alphabetically() {
        // &hellip; and &vellip; are both at distance 2 from "ellips";
        // tie-break must pick the alphabetically earliest so output stays
        // deterministic regardless of HashSet iteration order.
        assert_eq!(nearest_named_entity("ellips"), Some("hellip"));
    }
}

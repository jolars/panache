//! Keeps `docs/reference/linter-rules.qmd` honest against the built-in rule
//! registry.
//!
//! The reference page is hand-written prose, but several of its facts (the set
//! of rules, each rule's diagnostic codes, severities, auto-fix support,
//! default-on/off state, and config requirements) have an authoritative source
//! in code via [`RuleMeta`]. This test asserts the docs reflect that metadata
//! and never document a rule or code that no longer exists, so the catalogue
//! cannot silently drift from the linter.

use panache::linter::{Requirement, Severity, builtin_rule_metadata};

const DOC: &str = include_str!("../docs/reference/linter-rules.qmd");

/// The body of the `## Rules` section: everything between the `## Rules`
/// heading and the next top-level section (`## YAML diagnostics`). The YAML
/// codes are emitted by the parser, not the rule registry, so they live in
/// their own section and are out of scope here.
fn rules_section() -> &'static str {
    let start = DOC
        .find("\n## Rules\n")
        .expect("docs must have a `## Rules` section");
    let after = &DOC[start + "\n## Rules\n".len()..];
    let end = after
        .find("\n## ")
        .expect("`## Rules` must be followed by another `## ` section");
    &after[..end]
}

/// Returns the rule name when `line` is a rule heading of the exact form
/// ``### `name` {#anchor}``. Markdown `### ` headings inside example code
/// blocks have no backtick-wrapped name and no `{#…}` anchor, so they are
/// skipped.
fn rule_heading_name(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("### `")?;
    let name = rest.split('`').next()?;
    line.contains("{#").then_some(name)
}

/// Per-rule chunks keyed by rule name: each rule's heading through the line
/// before the next rule heading, so its `#### code` subsections stay inside its
/// own chunk.
fn rule_chunks() -> Vec<(String, &'static str)> {
    let section = rules_section();
    let starts: Vec<(usize, &str)> = section
        .match_indices('\n')
        .filter_map(|(nl, _)| {
            let line_start = nl + 1;
            let line = section[line_start..].lines().next()?;
            rule_heading_name(line).map(|name| (line_start, name))
        })
        .collect();

    starts
        .iter()
        .enumerate()
        .map(|(i, &(start, name))| {
            let end = starts.get(i + 1).map_or(section.len(), |&(s, _)| s);
            (name.to_string(), &section[start..end])
        })
        .collect()
}

/// Value of a pandoc definition-list field (`Term\n:   value...`), with
/// continuation lines joined by spaces. Returns `None` when the term is absent.
fn field(chunk: &str, term: &str) -> Option<String> {
    let lines: Vec<&str> = chunk.lines().collect();
    let idx = lines.iter().position(|l| l.trim() == term)?;
    let mut out = String::new();
    let mut i = idx + 1;
    while i < lines.len() && lines[i].trim().is_empty() {
        i += 1; // pandoc allows a blank line between the term and its `:` body
    }
    while i < lines.len() && !lines[i].trim().is_empty() {
        let line = lines[i].trim_start();
        let line = line.strip_prefix(':').unwrap_or(line);
        out.push_str(line.trim());
        out.push(' ');
        i += 1;
    }
    Some(out.trim().to_string())
}

/// A lowercase substring the docs' "Requirements" field must mention for a
/// given requirement, so the documented precondition matches the gating.
fn requirement_token(req: Requirement) -> Option<&'static str> {
    match req {
        Requirement::Always => None,
        Requirement::HeaderAttributes => Some("header-attributes"),
        Requirement::Footnotes => Some("footnotes"),
        Requirement::Citations => Some("citations"),
        Requirement::FencedCodeAttributes => Some("fenced-code-attributes"),
        Requirement::FencedDivs => Some("fenced-divs"),
        Requirement::Emoji => Some("emoji"),
        Requirement::TexMath => Some("tex-math"),
        Requirement::ChunkFlavor => Some("chunk"),
        Requirement::Quarto => Some("quarto"),
    }
}

fn severity_word(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "Error",
        Severity::Warning => "Warning",
        Severity::Info => "Info",
    }
}

#[test]
fn every_rule_is_documented_and_nothing_extra() {
    let meta_names: std::collections::BTreeSet<String> = builtin_rule_metadata()
        .iter()
        .map(|m| m.name.to_string())
        .collect();
    let doc_names: std::collections::BTreeSet<String> =
        rule_chunks().into_iter().map(|(name, _)| name).collect();

    let missing: Vec<_> = meta_names.difference(&doc_names).collect();
    assert!(
        missing.is_empty(),
        "rules registered in code but missing from docs/reference/linter-rules.qmd: {missing:?}"
    );

    let phantom: Vec<_> = doc_names.difference(&meta_names).collect();
    assert!(
        phantom.is_empty(),
        "rules documented but not registered in code: {phantom:?}"
    );
}

#[test]
fn documented_facts_match_metadata() {
    let chunks: std::collections::HashMap<String, &'static str> =
        rule_chunks().into_iter().collect();

    for meta in builtin_rule_metadata() {
        let chunk = chunks
            .get(meta.name)
            .unwrap_or_else(|| panic!("rule `{}` has no docs section", meta.name));

        // Every diagnostic code must appear in the rule's section.
        for code in meta.codes {
            assert!(
                chunk.contains(code.code),
                "rule `{}`: diagnostic code `{}` is not documented",
                meta.name,
                code.code
            );
        }

        // The Severity field must mention every distinct severity the rule emits.
        let severity = field(chunk, "Severity")
            .unwrap_or_else(|| panic!("rule `{}` has no Severity field", meta.name));
        let mut severities: Vec<Severity> = meta.codes.iter().map(|c| c.severity).collect();
        severities.dedup();
        for sev in severities {
            assert!(
                severity.contains(severity_word(sev)),
                "rule `{}`: Severity field {severity:?} does not mention {:?}",
                meta.name,
                severity_word(sev)
            );
        }

        // Auto-fix field must agree with `auto_fix`.
        let auto_fix = field(chunk, "Auto-fix")
            .unwrap_or_else(|| panic!("rule `{}` has no Auto-fix field", meta.name));
        if meta.auto_fix {
            assert!(
                auto_fix.starts_with("Yes"),
                "rule `{}`: auto_fix=true but docs say {auto_fix:?}",
                meta.name
            );
        } else {
            assert!(
                auto_fix.starts_with("No"),
                "rule `{}`: auto_fix=false but docs say {auto_fix:?}",
                meta.name
            );
        }

        // Opt-in rules must be flagged "Default: Off"; default-on rules must not.
        let default_field = field(chunk, "Default");
        if meta.default_on {
            assert!(
                default_field.is_none(),
                "rule `{}`: default_on=true but docs declare a Default field ({:?})",
                meta.name,
                default_field
            );
        } else {
            let value = default_field.unwrap_or_else(|| {
                panic!(
                    "rule `{}`: default_on=false but docs have no Default field",
                    meta.name
                )
            });
            assert!(
                value.contains("Off"),
                "rule `{}`: opt-in rule's Default field should say Off, got {value:?}",
                meta.name
            );
        }

        // Gated rules must document a matching Requirements field.
        if let Some(token) = requirement_token(meta.requires) {
            let req = field(chunk, "Requirements").unwrap_or_else(|| {
                panic!(
                    "rule `{}`: requires {:?} but docs have no Requirements field",
                    meta.name, meta.requires
                )
            });
            assert!(
                req.to_lowercase().contains(token),
                "rule `{}`: Requirements field {req:?} does not mention `{token}`",
                meta.name
            );
        }
    }
}

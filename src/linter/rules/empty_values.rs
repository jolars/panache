//! `empty-values`: flag YAML block-mapping keys whose value is an implicit
//! null (`title:` with nothing after the colon).
//!
//! Modeled on yamllint's `empty-values`. A missing value is parsed as YAML
//! null, which is occasionally intentional but far more often a forgotten
//! value. The rule fires on document frontmatter and on hashpipe cell options
//! alike, since both embed the same YAML CST; an *explicit* null (`title: null`
//! or `title: ~`) is a real scalar and is never flagged.
//!
//! The auto-fix deletes the empty key's whole line. It is marked **unsafe**
//! (applied only under `--unsafe-fixes`) because dropping a key changes the
//! document's data: the right resolution—supply a value, delete the key, or
//! write an explicit `null`—is an author-intent decision the rule can't make.

use crate::linter::diagnostics::{Diagnostic, DiagnosticNoteKind, Edit, Fix, Location};
use crate::linter::rules::{DiagnosticCode, LintContext, Requirement, Rule, RuleMeta};
use crate::syntax::{AstNode, SyntaxKind, SyntaxNode, YamlBlockMapEntry};
use rowan::{TextRange, TextSize};

pub struct EmptyValuesRule;

impl Rule for EmptyValuesRule {
    fn name(&self) -> &str {
        "empty-values"
    }

    fn metadata(&self) -> RuleMeta {
        RuleMeta {
            name: "empty-values",
            default_on: true,
            requires: Requirement::Always,
            auto_fix: true,
            codes: const { &[DiagnosticCode::warning("empty-values")] },
        }
    }

    fn node_interests(&self) -> &'static [SyntaxKind] {
        &[SyntaxKind::YAML_BLOCK_MAP_ENTRY]
    }

    fn check(&self, cx: &LintContext) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        for node in cx.nodes(SyntaxKind::YAML_BLOCK_MAP_ENTRY) {
            if let Some(diag) = classify(node, cx.input) {
                diagnostics.push(diag);
            }
        }
        diagnostics
    }
}

fn classify(node: &SyntaxNode, input: &str) -> Option<Diagnostic> {
    let entry = YamlBlockMapEntry::cast(node.clone())?;
    // An absent value node and a present-but-empty one both model an implicit
    // null. An explicit scalar (`null`, `~`, ...) or a nested container is a
    // real value and resolves `is_empty()` to `false`.
    let is_empty = entry.value().is_none_or(|value| value.is_empty());
    if !is_empty {
        return None;
    }

    let key = entry.key()?;
    // Point the caret at the key name (excluding the trailing colon) when the
    // scalar is available, falling back to the whole key wrapper.
    let range = key
        .scalar()
        .map(|s| s.text_range())
        .unwrap_or_else(|| key.syntax().text_range());
    let location = Location::from_range(range, input);

    let message = match entry.key_text() {
        Some(key_text) => format!("Key `{key_text}` has an empty value (implicit null)"),
        None => "Mapping key has an empty value (implicit null)".to_string(),
    };

    let fix_message = match entry.key_text() {
        Some(key_text) => format!("Remove the empty key `{key_text}`"),
        None => "Remove the empty key".to_string(),
    };
    let fix = Fix::unsafe_fix(
        fix_message,
        vec![Edit {
            range: line_deletion_range(node, input),
            replacement: String::new(),
        }],
    );

    Some(
        Diagnostic::warning(location, "empty-values", message)
            .with_note(
                DiagnosticNoteKind::Help,
                "Provide a value, remove the key, or write an explicit `null` if the empty value is intentional",
            )
            .with_fix(fix),
    )
}

/// The byte range of the empty key's whole line, from the start of its leading
/// indentation through the trailing newline (inclusive). Deleting this range
/// removes the key cleanly; starting at the entry node would leave the line's
/// indentation dangling, merging the next line into the orphaned whitespace.
///
/// The end is taken from the first newline after the line start, not from the
/// entry node's own end: an implicit-null entry's range already swallows its
/// trailing newline, so searching forward from it would over-delete the
/// following line. An empty key is always a single physical line, so the line's
/// own newline is the correct terminator.
fn line_deletion_range(node: &SyntaxNode, input: &str) -> TextRange {
    let entry_start: usize = node.text_range().start().into();

    // Back up over the leading indentation to the start of the line.
    let line_start = input[..entry_start].rfind('\n').map_or(0, |nl| nl + 1);

    // Extend forward through the newline that terminates the line, if any.
    let line_end = match input[line_start..].find('\n') {
        Some(offset) => line_start + offset + 1,
        None => input.len(),
    };

    TextRange::new(
        TextSize::new(line_start as u32),
        TextSize::new(line_end as u32),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn lint(input: &str) -> Vec<Diagnostic> {
        let config = Config::default();
        let tree = crate::parser::parse(input, Some(config.clone()));
        EmptyValuesRule.check_tree(&tree, input, &config, None)
    }

    fn span<'a>(d: &Diagnostic, input: &'a str) -> &'a str {
        let r = d.location.range;
        let start: usize = r.start().into();
        let end: usize = r.end().into();
        &input[start..end]
    }

    /// Apply a single diagnostic's fix to `input`.
    fn apply_fix(d: &Diagnostic, input: &str) -> String {
        let fix = d.fix.as_ref().expect("diagnostic should carry a fix");
        let mut edits: Vec<&Edit> = fix.edits.iter().collect();
        edits.sort_by_key(|e| e.range.start());
        let mut out = String::new();
        let mut last = 0;
        for edit in edits {
            let start: usize = edit.range.start().into();
            let end: usize = edit.range.end().into();
            out.push_str(&input[last..start]);
            out.push_str(&edit.replacement);
            last = end;
        }
        out.push_str(&input[last..]);
        out
    }

    #[test]
    fn flags_empty_frontmatter_value() {
        let input = "---\ntitle:\n---\n";
        let diags = lint(input);
        assert_eq!(diags.len(), 1, "got: {diags:?}");
        assert_eq!(diags[0].code, "empty-values");
        assert!(diags[0].message.contains("title"));
        // Caret points at the key name, not the whole entry.
        assert_eq!(span(&diags[0], input), "title");
    }

    #[test]
    fn offers_unsafe_removal_fix() {
        let input = "---\ntitle:\n---\n";
        let diags = lint(input);
        assert_eq!(diags.len(), 1, "got: {diags:?}");
        let fix = diags[0].fix.as_ref().expect("fix should be present");
        assert_eq!(fix.safety, crate::linter::FixSafety::Unsafe);
        assert_eq!(fix.edits.len(), 1);
        assert!(fix.edits[0].replacement.is_empty());
        // The whole `title:\n` line is removed, leaving valid frontmatter.
        assert_eq!(apply_fix(&diags[0], input), "---\n---\n");
    }

    #[test]
    fn fix_removes_indented_key_with_its_indentation() {
        // The deletion must include the leading `  ` of `  echo:`, or the
        // indentation would be orphaned onto the next line.
        let input = "---\nexecute:\n  echo:\n---\n";
        let diags = lint(input);
        assert_eq!(diags.len(), 1, "got: {diags:?}");
        assert!(diags[0].message.contains("echo"));
        assert_eq!(apply_fix(&diags[0], input), "---\nexecute:\n---\n");
    }

    #[test]
    fn flags_every_empty_key() {
        let diags = lint("---\ntitle:\ntags:\nauthor: Jane\n---\n");
        assert_eq!(diags.len(), 2, "got: {diags:?}");
        assert!(diags.iter().all(|d| d.code == "empty-values"));
        assert!(diags.iter().any(|d| d.message.contains("title")));
        assert!(diags.iter().any(|d| d.message.contains("tags")));
    }

    #[test]
    fn does_not_flag_explicit_null() {
        // Explicit nulls are a deliberate value, not a forgotten one.
        assert!(lint("---\ntitle: null\n---\n").is_empty());
        assert!(lint("---\ntitle: ~\n---\n").is_empty());
    }

    #[test]
    fn does_not_flag_non_empty_value() {
        assert!(lint("---\nauthor: Jane\n---\n").is_empty());
    }

    #[test]
    fn parent_with_nested_children_is_not_empty() {
        // `execute:` holds a nested block map, so it is not empty; only the
        // genuinely-empty nested `echo:` is flagged.
        let diags = lint("---\nexecute:\n  echo:\n---\n");
        assert_eq!(diags.len(), 1, "got: {diags:?}");
        assert!(diags[0].message.contains("echo"));
    }

    #[test]
    fn carries_help_note() {
        let diags = lint("---\ntitle:\n---\n");
        assert_eq!(diags.len(), 1);
        assert!(
            diags[0]
                .notes
                .iter()
                .any(|n| n.kind == DiagnosticNoteKind::Help)
        );
    }
}

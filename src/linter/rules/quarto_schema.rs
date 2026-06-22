//! `quarto-schema`: validate document YAML against Quarto's machine-readable
//! schema (Quarto flavor only).
//!
//! Pandoc treats metadata as an arbitrary mapping, so the ceiling there is
//! `yaml-parse-error`. Quarto ships a schema and validates against it, flagging
//! unknown/misspelled keys and type mismatches. This rule reproduces that for
//! the cases the distilled schema can decide (see
//! [`crate::linter::quarto_schema`]).
//!
//! Validates document **frontmatter** (against the `front-matter` root) and
//! **hashpipe cell options** (`#| ...`, against the per-engine root —
//! `engine-knitr` for R cells, `engine-jupyter` otherwise). Project config
//! files (`_quarto.yml`, `_metadata.yml`) are standalone YAML, not part of the
//! document CST; they are validated separately via
//! [`crate::linter::quarto_schema::validate_standalone_yaml`] on the CLI and LSP
//! manifest paths.

use crate::config::Flavor;
use crate::linter::diagnostics::Diagnostic;
use crate::linter::quarto_schema::interp::validate;
use crate::linter::quarto_schema::model::QuartoSchema;
use crate::linter::quarto_schema::report::{
    INVALID_ENUM, TYPE_MISMATCH, UNKNOWN_KEY, to_diagnostic,
};
use crate::linter::quarto_schema::schema;
use crate::linter::quarto_schema::value::bridge_yaml_content;
use crate::linter::rules::{DiagnosticCode, LintContext, Requirement, Rule, RuleMeta};
use crate::syntax::{AstNode, CodeBlock, SyntaxKind};

pub struct QuartoSchemaRule;

impl Rule for QuartoSchemaRule {
    fn name(&self) -> &str {
        "quarto-schema"
    }

    fn metadata(&self) -> RuleMeta {
        RuleMeta {
            name: "quarto-schema",
            default_on: true,
            requires: Requirement::Quarto,
            auto_fix: false,
            codes: const {
                &[
                    DiagnosticCode::warning(UNKNOWN_KEY),
                    DiagnosticCode::warning(TYPE_MISMATCH),
                    DiagnosticCode::warning(INVALID_ENUM),
                ]
            },
        }
    }

    fn check(&self, cx: &LintContext) -> Vec<Diagnostic> {
        // Registration already gates on `Requirement::Quarto`; this also guards
        // the `check_tree` test entry point, which bypasses the registry.
        if !matches!(cx.config.flavor, Flavor::Quarto) {
            return Vec::new();
        }
        let schema = schema();
        let mut diagnostics = Vec::new();
        // Walk the embedded YAML content nodes directly: frontmatter and each
        // cell's hashpipe options. Working from the in-tree CST keeps spans in
        // host coordinates (the `#|` prefix is trivia), so no offset shifting.
        for node in cx.tree.descendants() {
            let root = match node.kind() {
                SyntaxKind::YAML_METADATA_CONTENT => Some(schema.roots.frontmatter.as_str()),
                SyntaxKind::HASHPIPE_YAML_CONTENT => node
                    .ancestors()
                    .find_map(CodeBlock::cast)
                    .and_then(|block| cell_root(&block, schema)),
                _ => continue,
            };
            let Some(root) = root else {
                continue;
            };
            // `None` means malformed YAML (opaque tokens) — already reported.
            let Some(value) = bridge_yaml_content(&node) else {
                continue;
            };
            for err in validate(schema, root, &value) {
                diagnostics.push(to_diagnostic(err, cx.input));
            }
        }
        diagnostics
    }
}

/// The cell-options engine root for an executable code cell: `engine-knitr` for
/// R, `engine-jupyter` otherwise. `None` for non-executable fences.
fn cell_root<'s>(block: &CodeBlock, schema: &'s QuartoSchema) -> Option<&'s str> {
    if !block.is_executable_chunk() {
        return None;
    }
    let is_r = block
        .language()
        .is_some_and(|lang| lang.eq_ignore_ascii_case("r"));
    if is_r {
        schema.roots.cell_knitr.as_deref()
    } else {
        schema.roots.cell_jupyter.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, Extensions, Flavor};
    use crate::linter::diagnostics::Severity;

    fn lint(input: &str, flavor: Flavor) -> Vec<Diagnostic> {
        // Mirror real config loading, which applies flavor-default extensions —
        // executable chunks (and thus hashpipe options) need them parsed.
        let config = Config {
            flavor,
            extensions: Extensions::for_flavor(flavor),
            ..Config::default()
        };
        let tree = crate::parser::parse(input, Some(config.clone()));
        QuartoSchemaRule.check_tree(&tree, input, &config, None)
    }

    #[test]
    fn flags_unknown_frontmatter_key_with_suggestion() {
        let diags = lint("---\nforrmat: html\ntitle: Hi\n---\n", Flavor::Quarto);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, UNKNOWN_KEY);
        assert_eq!(diags[0].severity, Severity::Warning);
        assert!(diags[0].message.contains("forrmat"));
        assert!(diags[0].message.contains("format"));
        // The diagnostic points at the key, not the whole frontmatter.
        let r = diags[0].location.range;
        assert_eq!(
            &input_at(&r, "---\nforrmat: html\ntitle: Hi\n---\n"),
            "forrmat"
        );
    }

    fn input_at(r: &rowan::TextRange, input: &str) -> String {
        let start: usize = r.start().into();
        let end: usize = r.end().into();
        input[start..end].to_string()
    }

    #[test]
    fn flags_type_mismatch() {
        let diags = lint("---\ntoc: maybe\n---\n", Flavor::Quarto);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, TYPE_MISMATCH);
        assert!(diags[0].message.contains("boolean"));
    }

    #[test]
    fn accepts_valid_frontmatter() {
        let diags = lint(
            "---\ntitle: Hello\nauthor: Jane\nformat: html\ntoc: true\n---\n",
            Flavor::Quarto,
        );
        assert!(diags.is_empty(), "unexpected: {diags:?}");
    }

    #[test]
    fn ignores_custom_passthrough_keys() {
        let diags = lint(
            "---\nmy-custom-field: 42\nx-extra: [1, 2, 3]\n---\n",
            Flavor::Quarto,
        );
        assert!(diags.is_empty(), "unexpected: {diags:?}");
    }

    #[test]
    fn does_not_run_under_pandoc() {
        let diags = lint("---\nforrmat: html\n---\n", Flavor::Pandoc);
        assert!(diags.is_empty());
    }

    #[test]
    fn merged_enum_and_type_message() {
        let diags = lint("---\nexecute:\n  echo: maybe\n---\n", Flavor::Quarto);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, TYPE_MISMATCH);
        assert!(diags[0].message.contains("boolean"));
        assert!(diags[0].message.contains("fenced"));
    }

    #[test]
    fn flags_cell_option_type_mismatch() {
        // `echo` is `anyOf[boolean, enum[fenced]]` in the cell schema.
        let diags = lint("```{python}\n#| echo: maybe\n1\n```\n", Flavor::Quarto);
        assert_eq!(diags.len(), 1, "got: {diags:?}");
        assert_eq!(diags[0].code, TYPE_MISMATCH);
        assert!(diags[0].message.contains("boolean"));
        assert!(diags[0].message.contains("fenced"));
    }

    #[test]
    fn flags_cell_option_typo() {
        let diags = lint("```{python}\n#| eccho: false\n1\n```\n", Flavor::Quarto);
        assert_eq!(diags.len(), 1, "got: {diags:?}");
        assert_eq!(diags[0].code, UNKNOWN_KEY);
        assert!(diags[0].message.contains("eccho"));
        assert!(diags[0].message.contains("echo"));
    }

    #[test]
    fn accepts_valid_cell_options() {
        let diags = lint(
            "```{python}\n#| echo: false\n#| label: fig-1\n1\n```\n",
            Flavor::Quarto,
        );
        assert!(diags.is_empty(), "unexpected: {diags:?}");
    }

    #[test]
    fn validates_r_cells_against_knitr_engine() {
        // `cache` is a plain boolean in the knitr engine schema.
        let diags = lint("```{r}\n#| cache: maybe\n1\n```\n", Flavor::Quarto);
        assert_eq!(diags.len(), 1, "got: {diags:?}");
        assert_eq!(diags[0].code, TYPE_MISMATCH);
    }

    #[test]
    fn reports_every_issue_with_trailing_body() {
        // A map with multiple issues must report each one, not collapse to a
        // single "expected null" via the frontmatter's `anyOf[null, ...]`.
        let input = "---\nforrmat: html\ntoc: maybe\ntitle: My Doc\n---\n\n# Hello\n";
        let diags = lint(input, Flavor::Quarto);
        assert_eq!(diags.len(), 2, "got: {diags:?}");
        assert!(diags.iter().any(|d| d.code == UNKNOWN_KEY));
        assert!(diags.iter().any(|d| d.code == TYPE_MISMATCH));
        // None of the diagnostics should span the whole block.
        assert!(diags.iter().all(|d| d.message != "value should be null"));
    }
}

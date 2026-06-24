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
    INVALID_ENUM, TYPE_MISMATCH, UNKNOWN_KEY, to_diagnostic, validation_disabled,
};
use crate::linter::quarto_schema::schema;
use crate::linter::quarto_schema::value::bridge_yaml_content;
use crate::linter::rules::{DiagnosticCode, LintContext, Requirement, Rule, RuleMeta};
use crate::syntax::{AstNode, CodeBlock, SyntaxKind, SyntaxNode};

/// Schema validation split across two rules so each carries its own
/// default-on state (the docs/metadata model declares `default_on` per rule):
///
/// - [`QuartoSchemaRule`] (`quarto-schema`, on by default) reports type
///   mismatches and invalid enum values. These mirror Quarto's *hard* render
///   errors — `quarto render` fails YAML validation on them — so they are
///   high-signal.
/// - [`QuartoSchemaUnknownKeyRule`] (`quarto-schema-unknown-key`, opt-in)
///   reports unknown/misspelled keys. Quarto itself tolerates unknown keys at
///   render time (its schema objects are open for pandoc passthrough and custom
///   template metadata), so flagging them is stricter than the reference tool
///   and is left off by default.
pub struct QuartoSchemaRule;
pub struct QuartoSchemaUnknownKeyRule;

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
                    DiagnosticCode::warning(TYPE_MISMATCH),
                    DiagnosticCode::warning(INVALID_ENUM),
                ]
            },
        }
    }

    fn check(&self, cx: &LintContext) -> Vec<Diagnostic> {
        schema_diagnostics(cx)
            .into_iter()
            .filter(|d| d.code != UNKNOWN_KEY)
            .collect()
    }
}

impl Rule for QuartoSchemaUnknownKeyRule {
    fn name(&self) -> &str {
        "quarto-schema-unknown-key"
    }

    fn metadata(&self) -> RuleMeta {
        RuleMeta {
            name: "quarto-schema-unknown-key",
            default_on: false,
            requires: Requirement::Quarto,
            auto_fix: false,
            codes: const { &[DiagnosticCode::warning(UNKNOWN_KEY)] },
        }
    }

    fn check(&self, cx: &LintContext) -> Vec<Diagnostic> {
        schema_diagnostics(cx)
            .into_iter()
            .filter(|d| d.code == UNKNOWN_KEY)
            .collect()
    }
}

/// Validate every embedded YAML node (frontmatter and each cell's hashpipe
/// options) against the schema, returning all diagnostics regardless of code.
/// The two rules above filter this by code. Each rule runs its own walk; the
/// unknown-key rule is opt-in, so in the common case only one walk runs.
fn schema_diagnostics(cx: &LintContext) -> Vec<Diagnostic> {
    // Registration already gates on `Requirement::Quarto`; this also guards
    // the `check_tree` test entry point, which bypasses the registry.
    if !matches!(cx.config.flavor, Flavor::Quarto) {
        return Vec::new();
    }
    // Honor `validate-yaml: false` in the document's frontmatter: Quarto then
    // performs no schema validation for the document, so neither do we (this
    // covers the frontmatter and every cell's options).
    if frontmatter_opts_out(cx.tree) {
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

/// Whether the document's frontmatter sets `validate-yaml: false`, Quarto's
/// opt-out from YAML schema validation.
fn frontmatter_opts_out(tree: &SyntaxNode) -> bool {
    tree.descendants()
        .find(|n| n.kind() == SyntaxKind::YAML_METADATA_CONTENT)
        .and_then(|n| bridge_yaml_content(&n))
        .is_some_and(|v| validation_disabled(&v))
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

    fn config_for(flavor: Flavor) -> Config {
        // Mirror real config loading, which applies flavor-default extensions —
        // executable chunks (and thus hashpipe options) need them parsed.
        Config {
            flavor,
            extensions: Extensions::for_flavor(flavor),
            ..Config::default()
        }
    }

    /// Type-mismatch / invalid-enum diagnostics (the on-by-default rule).
    fn lint(input: &str, flavor: Flavor) -> Vec<Diagnostic> {
        let config = config_for(flavor);
        let tree = crate::parser::parse(input, Some(config.clone()));
        QuartoSchemaRule.check_tree(&tree, input, &config, None)
    }

    /// Unknown-key diagnostics (the opt-in rule).
    fn lint_unknown(input: &str, flavor: Flavor) -> Vec<Diagnostic> {
        let config = config_for(flavor);
        let tree = crate::parser::parse(input, Some(config.clone()));
        QuartoSchemaUnknownKeyRule.check_tree(&tree, input, &config, None)
    }

    #[test]
    fn flags_unknown_frontmatter_key_with_suggestion() {
        let diags = lint_unknown("---\nforrmat: html\ntitle: Hi\n---\n", Flavor::Quarto);
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
        // Custom top-level metadata with no near-miss of a known key is allowed
        // even by the opt-in unknown-key rule (open frontmatter object).
        let diags = lint_unknown(
            "---\nmy-custom-field: 42\nx-extra: [1, 2, 3]\n---\n",
            Flavor::Quarto,
        );
        assert!(diags.is_empty(), "unexpected: {diags:?}");
    }

    #[test]
    fn validate_yaml_false_disables_schema() {
        // `validate-yaml: false` is Quarto's opt-out; it skips schema validation
        // for the whole document — frontmatter and cell options alike.
        let doc =
            "---\nvalidate-yaml: false\ntoc: maybe\n---\n\n```{python}\n#| echo: maybe\n1\n```\n";
        assert!(
            lint(doc, Flavor::Quarto).is_empty(),
            "type/enum must be suppressed: {:?}",
            lint(doc, Flavor::Quarto)
        );
        let key = "---\nvalidate-yaml: false\nforrmat: html\n---\n";
        assert!(
            lint_unknown(key, Flavor::Quarto).is_empty(),
            "unknown-key must be suppressed: {:?}",
            lint_unknown(key, Flavor::Quarto)
        );
        // Sanity: without the opt-out the type mismatch still fires.
        assert!(!lint("---\ntoc: maybe\n---\n", Flavor::Quarto).is_empty());
    }

    #[test]
    fn does_not_run_under_pandoc() {
        assert!(lint("---\ntoc: maybe\n---\n", Flavor::Pandoc).is_empty());
        assert!(lint_unknown("---\nforrmat: html\n---\n", Flavor::Pandoc).is_empty());
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
        let diags = lint_unknown("```{python}\n#| eccho: false\n1\n```\n", Flavor::Quarto);
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
        let type_enum = lint(input, Flavor::Quarto);
        let unknown = lint_unknown(input, Flavor::Quarto);
        assert_eq!(type_enum.len(), 1, "got: {type_enum:?}");
        assert_eq!(type_enum[0].code, TYPE_MISMATCH);
        assert_eq!(unknown.len(), 1, "got: {unknown:?}");
        assert_eq!(unknown[0].code, UNKNOWN_KEY);
        // None of the diagnostics should span the whole block.
        assert!(
            type_enum
                .iter()
                .chain(&unknown)
                .all(|d| d.message != "value should be null")
        );
    }
}

//! Shared reporting helpers: map the interpreter's [`SchemaError`]s to linter
//! [`Diagnostic`]s, and validate standalone YAML files (project manifests)
//! against a schema root.
//!
//! Both the `quarto-schema` rule (embedded document YAML) and the manifest
//! validation paths (CLI + LSP, standalone `_quarto.yml` / `_metadata.yml`)
//! share this mapping so diagnostic codes and messages stay identical.

use std::path::Path;

use crate::linter::diagnostics::{Diagnostic, Location};
use crate::syntax::SyntaxNode;

use super::interp::{ErrorKind, SchemaError, validate};
use super::schema;
use super::value::bridge_yaml_content;

pub const UNKNOWN_KEY: &str = "quarto-schema-unknown-key";
pub const TYPE_MISMATCH: &str = "quarto-schema-type-mismatch";
pub const INVALID_ENUM: &str = "quarto-schema-invalid-enum";

/// Map a single [`SchemaError`] (span in `input` coordinates) to a warning
/// [`Diagnostic`].
pub fn to_diagnostic(err: SchemaError, input: &str) -> Diagnostic {
    let location = Location::from_range(err.span, input);
    match err.kind {
        ErrorKind::UnknownKey {
            key, suggestion, ..
        } => {
            let message = match &suggestion {
                Some(s) => format!("unknown key `{key}`; did you mean `{s}`?"),
                None => format!("unknown key `{key}`"),
            };
            Diagnostic::warning(location, UNKNOWN_KEY, message)
        }
        ErrorKind::TypeMismatch { expected } => Diagnostic::warning(
            location,
            TYPE_MISMATCH,
            format!("value should be {expected}"),
        ),
        ErrorKind::InvalidEnum { allowed } => Diagnostic::warning(
            location,
            INVALID_ENUM,
            format!("invalid value; expected one of: {}", allowed.join(", ")),
        ),
    }
}

/// Schema-validate an already-parsed YAML `tree` (offsets in host coordinates)
/// against `root_id`. Shared by the standalone and CLI entry points so they
/// bridge and validate identically. Empty when the tree carries no YAML
/// document.
fn schema_diagnostics_for_tree(tree: &SyntaxNode, text: &str, root_id: &str) -> Vec<Diagnostic> {
    let Some(value) = bridge_yaml_content(tree) else {
        return Vec::new();
    };
    validate(schema(), root_id, &value)
        .into_iter()
        .map(|err| to_diagnostic(err, text))
        .collect()
}

/// Validate a **standalone** YAML document (a project manifest such as
/// `_quarto.yml` or `_metadata.yml`) against the schema definition `root_id`.
///
/// Reparses `text` as YAML — the resulting tree's offsets are 0-based over the
/// file, i.e. already in host coordinates — and bridges it to the interpreter's
/// value model. Returns an empty vector when `text` is malformed YAML (the
/// parser yields no tree); that case is reported separately as a
/// `yaml-parse-error`.
pub fn validate_standalone_yaml(text: &str, root_id: &str) -> Vec<Diagnostic> {
    match crate::parser::yaml::parse_yaml_tree(text) {
        Some(tree) => schema_diagnostics_for_tree(&tree, text, root_id),
        None => Vec::new(),
    }
}

/// Full lint for a standalone Quarto manifest's text: a `yaml-parse-error` when
/// the YAML is malformed, otherwise the schema diagnostics against `root_id`
/// when `schema_enabled`.
///
/// Used by the CLI `lint` command for explicit `_quarto.yml` / `_metadata.yml`
/// targets. (The LSP composes the same two halves from separate salsa queries —
/// `project_manifest_diagnostics` for the parse error,
/// `project_manifest_schema_diagnostics` for the schema part — so the parse
/// error stays flavor-agnostic there.) Parse errors are always reported; only
/// the schema half is gated, matching the rule's enablement.
///
/// A single `parse_yaml_report` carries both the parse-error diagnostic and the
/// tree, so the manifest is parsed once: `validate_yaml` and `parse_yaml_tree`
/// each re-run that same pass, and calling both would parse the file twice.
pub fn lint_manifest_text(text: &str, root_id: &str, schema_enabled: bool) -> Vec<Diagnostic> {
    let report = crate::parser::yaml::parse_yaml_report(text);
    if let Some(diag) = report.diagnostics.first() {
        let (line, column) =
            crate::metadata::project::byte_offset_to_line_col_1based(text, diag.byte_start);
        let yaml_err = crate::metadata::YamlError::ParseError {
            message: diag.message.to_string(),
            line: line as u64,
            column: column as u64,
            byte_offset: Some(diag.byte_start),
        };
        return crate::linter::metadata_diagnostics::yaml_error_diagnostic(&yaml_err, text)
            .into_iter()
            .collect();
    }
    if !schema_enabled {
        return Vec::new();
    }
    match report.tree.as_ref() {
        Some(tree) => schema_diagnostics_for_tree(tree, text, root_id),
        None => Vec::new(),
    }
}

/// The schema root a Quarto project manifest validates against, keyed by file
/// name: `_quarto.yml` is project config (`project-config`); `_metadata.yml`
/// holds directory-level, document-frontmatter-shaped metadata (`front-matter`).
/// Returns `None` for any other file (including bookdown manifests, which are
/// not Quarto).
pub fn manifest_schema_root(path: &Path) -> Option<&'static str> {
    let roots = &schema().roots;
    match path.file_name().and_then(|n| n.to_str()) {
        Some("_quarto.yml") => Some(roots.project.as_str()),
        Some("_metadata.yml") => Some(roots.frontmatter.as_str()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn manifest_root_maps_known_names() {
        assert_eq!(
            manifest_schema_root(Path::new("/proj/_quarto.yml")),
            Some("project-config")
        );
        assert_eq!(
            manifest_schema_root(Path::new("/proj/sub/_metadata.yml")),
            Some("front-matter")
        );
        assert_eq!(manifest_schema_root(Path::new("/proj/_bookdown.yml")), None);
        assert_eq!(manifest_schema_root(Path::new("/proj/config.yml")), None);
    }

    #[test]
    fn flags_project_config_frontmatter_typo() {
        // `forrmat` is a typo of `format`, reachable via the front-matter that
        // `project-config` includes in its `allOf`.
        let diags = validate_standalone_yaml("forrmat: html\n", "project-config");
        assert_eq!(diags.len(), 1, "got: {diags:?}");
        assert_eq!(diags[0].code, UNKNOWN_KEY);
        assert!(diags[0].message.contains("forrmat"));
        assert!(diags[0].message.contains("format"));
    }

    #[test]
    fn flags_closed_profile_subkey() {
        // `project-profile` is a closed object: `defualt` is not a valid key.
        let diags = validate_standalone_yaml("profile:\n  defualt: web\n", "project-config");
        assert!(
            diags.iter().any(|d| d.code == UNKNOWN_KEY),
            "got: {diags:?}"
        );
    }

    #[test]
    fn accepts_valid_project_config() {
        let diags = validate_standalone_yaml(
            "project:\n  type: website\ntitle: My Site\nformat: html\n",
            "project-config",
        );
        assert!(diags.is_empty(), "unexpected: {diags:?}");
    }

    #[test]
    fn validates_metadata_against_frontmatter() {
        let diags = validate_standalone_yaml("toc: maybe\n", "front-matter");
        assert_eq!(diags.len(), 1, "got: {diags:?}");
        assert_eq!(diags[0].code, TYPE_MISMATCH);
    }

    #[test]
    fn malformed_yaml_yields_no_schema_diagnostics() {
        let diags = validate_standalone_yaml("foo: [unterminated\n", "project-config");
        assert!(diags.is_empty(), "unexpected: {diags:?}");
    }

    #[test]
    fn lint_manifest_reports_parse_error_for_malformed_yaml() {
        let diags = lint_manifest_text("title: [\n", "project-config", true);
        assert_eq!(diags.len(), 1, "got: {diags:?}");
        assert_eq!(diags[0].code, "yaml-parse-error");
    }

    #[test]
    fn lint_manifest_reports_schema_diagnostics_when_enabled() {
        let diags = lint_manifest_text("forrmat: html\n", "project-config", true);
        assert_eq!(diags.len(), 1, "got: {diags:?}");
        assert_eq!(diags[0].code, UNKNOWN_KEY);
    }

    #[test]
    fn lint_manifest_skips_schema_when_disabled_but_keeps_parse_errors() {
        // Rule disabled: a valid-but-wrong manifest is clean...
        let clean = lint_manifest_text("forrmat: html\n", "project-config", false);
        assert!(clean.is_empty(), "schema must be gated: {clean:?}");
        // ...but a parse error still surfaces.
        let broken = lint_manifest_text("title: [\n", "project-config", false);
        assert_eq!(broken.len(), 1, "got: {broken:?}");
        assert_eq!(broken[0].code, "yaml-parse-error");
    }
}

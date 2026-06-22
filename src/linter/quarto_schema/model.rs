//! Normalized, validation-only projection of Quarto's
//! `all-schema-definitions.json`.
//!
//! The raw Quarto artifact (~2.9 MB) is a map of ~650 schema definitions, each
//! laden with editor-only metadata (`completions`, `description`,
//! `documentation`, `tags`, `_internalId`). The distilled form kept here keeps
//! only what the linter's interpreter needs—node type, object
//! property/closure structure, enum values, array item schema, and
//! cross-references by definition id—so a Quarto version bump produces a small,
//! reviewable diff.
//!
//! Produced at vendor time by the `distill_quarto_schema` bin (driven by
//! `scripts/update-quarto-schema.sh`) and embedded at build time via
//! `include_str!` (see [`super`]). The distiller emits exactly these types, so
//! the committed artifact is guaranteed to deserialize.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// A distilled Quarto schema: every definition plus the entry-point ids the
/// linter validates each YAML location against.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuartoSchema {
    /// quarto-cli tag the schema was distilled from (e.g. `"v1.9.38"`),
    /// mirrored in `assets/quarto-schema/.panache-source`.
    pub version: String,
    /// Entry-point definition ids by document location.
    pub roots: Roots,
    /// All schema definitions, keyed by Quarto's definition id. A [`BTreeMap`]
    /// keeps the serialized order stable for reviewable version-bump diffs.
    pub defs: BTreeMap<String, SchemaNode>,
}

/// Entry-point definition ids for each YAML location the rule validates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Roots {
    /// Document frontmatter (`---` block). Quarto def id `front-matter`.
    pub frontmatter: String,
    /// Project config (`_quarto.yml`). Quarto def id `project-config`.
    pub project: String,
    /// Code-cell options (`#| ...`) for R cells (the knitr engine). Quarto def
    /// id `engine-knitr`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cell_knitr: Option<String>,
    /// Code-cell options for non-R cells (the jupyter engine). Quarto def id
    /// `engine-jupyter`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cell_jupyter: Option<String>,
}

/// A single normalized schema node.
///
/// Mirrors the bounded vocabulary Quarto's compiler emits (`string`, `number`,
/// `boolean`, `null`, `enum`, `array`, `object`, `anyOf`, `allOf`, `ref`).
/// Anything we do not model (e.g. Quarto's editor-only `key` nodes) distills to
/// [`SchemaNode::Any`], which never produces a diagnostic.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "lowercase")]
pub enum SchemaNode {
    /// Accepts any value; never diagnoses. Used for unmodeled Quarto nodes and
    /// missing array item schemas.
    Any,
    String,
    Number,
    Boolean,
    Null,
    /// A fixed set of allowed values (kept as raw JSON to preserve non-string
    /// enum members).
    Enum {
        values: Vec<serde_json::Value>,
    },
    Array {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        items: Option<Box<SchemaNode>>,
    },
    Object {
        /// Declared property name → value schema.
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        properties: BTreeMap<String, SchemaNode>,
        /// `true` when unknown keys are rejected (Quarto's `closed: true` or
        /// `additionalProperties: false`).
        #[serde(default, skip_serializing_if = "is_false")]
        closed: bool,
        /// Pattern-constrained properties: keys matching `re` validate against
        /// `schema`. A non-empty list keeps the object open for matching keys.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        pattern: Vec<PatternProp>,
    },
    /// Value validates if it matches *any* branch.
    AnyOf {
        of: Vec<SchemaNode>,
    },
    /// Value validates if it matches *every* branch.
    AllOf {
        of: Vec<SchemaNode>,
    },
    /// Cross-reference to another definition by id (resolved via
    /// [`QuartoSchema::defs`]).
    Ref {
        id: String,
    },
}

/// A `patternProperties` entry: a regex over key names and the schema matching
/// keys must satisfy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternProp {
    /// Regex (verbatim from Quarto) constraining matching key names.
    pub re: String,
    pub schema: Box<SchemaNode>,
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_false(b: &bool) -> bool {
    !*b
}

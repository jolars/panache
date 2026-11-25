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
#[derive(Debug, Clone, Serialize)]
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

impl<'de> Deserialize<'de> for SchemaNode {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error;

        // Deriving Deserialize for the internally tagged enum buffers each
        // subtree as Serde Content, then walks and allocates it again at each
        // nested node. Decode the fields directly so the first schema load
        // constructs each child only once, regardless of field order.
        #[derive(Deserialize)]
        #[serde(rename_all = "lowercase")]
        enum Kind {
            Any,
            String,
            Number,
            Boolean,
            Null,
            Enum,
            Array,
            Object,
            AnyOf,
            AllOf,
            Ref,
        }

        #[derive(Deserialize)]
        struct Fields {
            t: Kind,
            values: Option<Vec<serde_json::Value>>,
            items: Option<Box<SchemaNode>>,
            #[serde(default)]
            properties: BTreeMap<String, SchemaNode>,
            #[serde(default)]
            closed: bool,
            #[serde(default)]
            pattern: Vec<PatternProp>,
            of: Option<Vec<SchemaNode>>,
            id: Option<String>,
        }

        let fields = Fields::deserialize(deserializer)?;
        Ok(match fields.t {
            Kind::Any => Self::Any,
            Kind::String => Self::String,
            Kind::Number => Self::Number,
            Kind::Boolean => Self::Boolean,
            Kind::Null => Self::Null,
            Kind::Enum => Self::Enum {
                values: fields
                    .values
                    .ok_or_else(|| D::Error::missing_field("values"))?,
            },
            Kind::Array => Self::Array {
                items: fields.items,
            },
            Kind::Object => Self::Object {
                properties: fields.properties,
                closed: fields.closed,
                pattern: fields.pattern,
            },
            Kind::AnyOf => Self::AnyOf {
                of: fields.of.ok_or_else(|| D::Error::missing_field("of"))?,
            },
            Kind::AllOf => Self::AllOf {
                of: fields.of.ok_or_else(|| D::Error::missing_field("of"))?,
            },
            Kind::Ref => Self::Ref {
                id: fields.id.ok_or_else(|| D::Error::missing_field("id"))?,
            },
        })
    }
}

/// A `patternProperties` entry: a regex over key names and the schema matching
/// keys must satisfy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternProp {
    /// Regex (verbatim from Quarto) constraining matching key names.
    pub re: String,
    pub schema: Box<SchemaNode>,
}

fn is_false(b: &bool) -> bool {
    !*b
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn schema_nodes_round_trip_independently_of_field_order() {
        // Field order is not part of the vendored JSON contract.
        let nodes = [
            json!({"t": "any"}),
            json!({"t": "string"}),
            json!({"t": "number"}),
            json!({"t": "boolean"}),
            json!({"t": "null"}),
            json!({"values": ["a", 1, true, null], "t": "enum"}),
            json!({"items": {"t": "string"}, "t": "array"}),
            json!({"t": "array"}),
            json!({"t": "object"}),
            json!({"properties": {"a": {"t": "boolean"}}, "closed": true,
                "pattern": [{"re": "^x", "schema": {"t": "number"}}], "t": "object"}),
            json!({"of": [{"t": "string"}, {"t": "null"}], "t": "anyof"}),
            json!({"of": [{"t": "object"}], "t": "allof"}),
            json!({"id": "front-matter", "t": "ref"}),
        ];
        for value in nodes {
            let node: SchemaNode = serde_json::from_str(&value.to_string()).unwrap();
            assert_eq!(serde_json::to_value(node).unwrap(), value);
        }
    }

    #[test]
    fn schema_nodes_reject_missing_required_fields() {
        for input in [
            "{}",
            r#"{"t":"unknown"}"#,
            r#"{"t":"enum"}"#,
            r#"{"t":"anyof"}"#,
            r#"{"t":"allof"}"#,
            r#"{"t":"ref"}"#,
        ] {
            assert!(
                serde_json::from_str::<SchemaNode>(input).is_err(),
                "{input}"
            );
        }
    }
}

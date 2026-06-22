//! Vendor-time distillation of Quarto's `all-schema-definitions.json` into the
//! compact [`QuartoSchema`] the linter embeds.
//!
//! This is dev-only tooling invoked by the `distill_quarto_schema` bin (see
//! `scripts/update-quarto-schema.sh`), not part of any runtime path. It walks
//! the raw schema, dropping editor-only metadata and keeping only the
//! validation-relevant shape.

use serde_json::Value;

use super::model::{PatternProp, QuartoSchema, Roots, SchemaNode};

/// Default entry-point definition ids in Quarto's compiled schema.
///
/// Verified against quarto-cli `v1.9.38`.
pub fn default_roots() -> Roots {
    Roots {
        frontmatter: "front-matter".to_string(),
        project: "project-config".to_string(),
        cell_knitr: Some("engine-knitr".to_string()),
        cell_jupyter: Some("engine-jupyter".to_string()),
    }
}

/// Distill the raw `all-schema-definitions.json` value into a [`QuartoSchema`].
///
/// `version` is the quarto-cli tag the artifact was fetched at; it is recorded
/// in the output and cross-checked against `.panache-source` at load time.
pub fn distill(raw: &Value, version: &str, roots: Roots) -> QuartoSchema {
    let defs = raw
        .as_object()
        .map(|obj| {
            obj.iter()
                .map(|(id, node)| (id.clone(), normalize(node)))
                .collect()
        })
        .unwrap_or_default();

    QuartoSchema {
        version: version.to_string(),
        roots,
        defs,
    }
}

/// Normalize one raw schema node, discarding everything but its validation
/// shape.
fn normalize(v: &Value) -> SchemaNode {
    let Some(obj) = v.as_object() else {
        return SchemaNode::Any;
    };
    let ty = obj.get("type").and_then(Value::as_str).unwrap_or("");

    match ty {
        "string" => SchemaNode::String,
        "number" => SchemaNode::Number,
        "boolean" => SchemaNode::Boolean,
        "null" => SchemaNode::Null,
        "enum" => SchemaNode::Enum {
            values: obj
                .get("enum")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
        },
        "array" => SchemaNode::Array {
            items: obj.get("items").map(|i| Box::new(normalize(i))),
        },
        "ref" => SchemaNode::Ref {
            id: obj
                .get("$ref")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
        },
        "anyOf" => SchemaNode::AnyOf {
            of: normalize_list(obj.get("anyOf")),
        },
        "allOf" => SchemaNode::AllOf {
            of: normalize_list(obj.get("allOf")),
        },
        "object" => SchemaNode::Object {
            properties: obj
                .get("properties")
                .and_then(Value::as_object)
                .map(|props| {
                    props
                        .iter()
                        .map(|(k, v)| (k.clone(), normalize(v)))
                        .collect()
                })
                .unwrap_or_default(),
            closed: is_closed(obj),
            pattern: obj
                .get("patternProperties")
                .and_then(Value::as_object)
                .map(|pats| {
                    pats.iter()
                        .map(|(re, v)| PatternProp {
                            re: re.clone(),
                            schema: Box::new(normalize(v)),
                        })
                        .collect()
                })
                .unwrap_or_default(),
        },
        // Quarto's `key` completion nodes and anything we don't model.
        _ => SchemaNode::Any,
    }
}

fn normalize_list(v: Option<&Value>) -> Vec<SchemaNode> {
    v.and_then(Value::as_array)
        .map(|arr| arr.iter().map(normalize).collect())
        .unwrap_or_default()
}

/// An object rejects unknown keys when Quarto marks it `closed: true` or pins
/// `additionalProperties: false`.
fn is_closed(obj: &serde_json::Map<String, Value>) -> bool {
    obj.get("closed").and_then(Value::as_bool).unwrap_or(false)
        || obj.get("additionalProperties") == Some(&Value::Bool(false))
}

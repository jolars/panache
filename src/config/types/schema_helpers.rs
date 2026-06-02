//! Proxy types and helpers used to generate the JSON Schema for
//! `panache.toml`. Kept separate so the runtime config types stay
//! lean — these types exist purely to describe the user-facing
//! TOML shape to `schemars`.
//!
//! Each entry has a runtime counterpart that uses `toml::Value`
//! (because the actual deserialization fans out across helpers in
//! `super::resolve_*`). The proxy mirrors the documented input
//! shape, not the materialized struct, so the published schema
//! describes what users write.

use std::collections::HashMap;

use schemars::{JsonSchema, Schema, SchemaGenerator};

use panache_formatter::config::FormatterExtensions;
use panache_parser::Extensions;

use super::FormatterDefinition;

#[derive(JsonSchema)]
#[serde(untagged)]
#[allow(dead_code)]
pub enum FormatterEntry {
    /// Single preset or named definition: `r = "air"`.
    Single(String),
    /// Sequential chain: `python = ["isort", "black"]`.
    Multiple(Vec<String>),
    /// Named definition table: `[formatters.air] args = [...]`.
    Definition(FormatterDefinition),
}

/// Hand-written schema for the `[extensions]` block. Each top-level key is
/// either a known extension name (with a boolean value) or a known flavor
/// name (with a nested table whose keys are themselves known extension
/// names). The enums are derived from `Extensions::KNOWN_NAMES` and
/// `FormatterExtensions::KNOWN_NAMES`, so editors with TOML schema support
/// flag typos like `quato-crossrefs = true` as you type.
pub fn extensions_schema(_generator: &mut SchemaGenerator) -> Schema {
    let known_extensions = known_extension_names_json();
    let known_flavors = known_flavor_keys_json();
    let mut all_keys = known_extensions.clone();
    all_keys.extend(known_flavors.iter().cloned());
    all_keys.sort_by(|a, b| a.as_str().unwrap().cmp(b.as_str().unwrap()));
    all_keys.dedup();

    schemars::json_schema!({
        "type": "object",
        "description": "Pandoc extension toggles. Each key is either an \
                        extension name (`fenced-divs = true`) or a flavor \
                        name whose table scopes overrides to one flavor \
                        (`[extensions.pandoc] fenced-divs = false`).",
        "propertyNames": { "enum": all_keys },
        "additionalProperties": {
            "oneOf": [
                { "type": "boolean" },
                {
                    "type": "object",
                    "propertyNames": { "enum": known_extensions },
                    "additionalProperties": { "type": "boolean" }
                }
            ]
        }
    })
}

pub fn formatters_schema(generator: &mut SchemaGenerator) -> Schema {
    <HashMap<String, FormatterEntry> as JsonSchema>::json_schema(generator)
}

/// Union of parser + formatter extension names, sorted and deduplicated, as
/// JSON strings ready for inclusion in a schema enum.
fn known_extension_names_json() -> Vec<serde_json::Value> {
    let mut names: Vec<&'static str> = Extensions::KNOWN_NAMES
        .iter()
        .chain(FormatterExtensions::KNOWN_NAMES.iter())
        .copied()
        .collect();
    names.sort_unstable();
    names.dedup();
    names.into_iter().map(serde_json::Value::from).collect()
}

/// Canonical flavor table keys accepted under `[extensions]`. Kept aligned
/// with `parse_flavor_key` in `src/config.rs`.
fn known_flavor_keys_json() -> Vec<serde_json::Value> {
    [
        "pandoc",
        "quarto",
        "rmarkdown",
        "r-markdown",
        "gfm",
        "commonmark",
        "common-mark",
        "multimarkdown",
        "multi-markdown",
    ]
    .into_iter()
    .map(serde_json::Value::from)
    .collect()
}

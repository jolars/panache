//! Quarto schema validation: a distilled projection of Quarto's machine-readable
//! schema plus an interpreter that validates document YAML against it.
//!
//! Used by the `quarto-schema` lint rule (Quarto flavor only) to flag
//! unknown/misspelled frontmatter keys and type mismatches—the things Quarto's
//! own YAML intelligence reports, which pandoc (schema-less metadata) cannot.
//!
//! - [`model`] — the normalized schema types embedded in the binary.
//! - [`interp`] — the recursive validator over the schema.
//! - [`distill`] — vendor-time normalization of Quarto's raw artifact (dev-only,
//!   driven by `scripts/update-quarto-schema.sh`).

pub mod distill;
pub mod interp;
pub mod model;
pub mod report;
pub mod value;

pub use report::{
    INVALID_ENUM, TYPE_MISMATCH, UNKNOWN_KEY, lint_manifest_text, manifest_schema_root,
    to_diagnostic, validate_standalone_yaml,
};

use std::sync::OnceLock;

use model::QuartoSchema;

/// The distilled Quarto schema, gzip-compressed at build time (see `build.rs`)
/// to keep the binary and wasm bundle small — the pretty JSON is ~2 MB, the
/// compressed blob ~40 KB. The reviewable source stays committed at
/// `assets/quarto-schema/schema.json`, kept in sync with `.panache-source` by
/// `scripts/update-quarto-schema.sh`.
const EMBEDDED_GZ: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/quarto-schema.json.gz"));

/// The embedded schema, decompressed and parsed once on first use.
///
/// The distiller emits exactly [`QuartoSchema`], so neither the gunzip nor the
/// deserialization can fail for a correctly vendored artifact; a panic here
/// means the committed `schema.json` drifted from [`model`] and would be caught
/// by the artifact smoke test.
pub fn schema() -> &'static QuartoSchema {
    use std::io::Read;

    static SCHEMA: OnceLock<QuartoSchema> = OnceLock::new();
    SCHEMA.get_or_init(|| {
        let mut json = String::new();
        flate2::read::GzDecoder::new(EMBEDDED_GZ)
            .read_to_string(&mut json)
            .expect("embedded Quarto schema must gunzip");
        serde_json::from_str(&json).expect("embedded distilled Quarto schema must deserialize")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The vendored schema version must match the pin in `.panache-source`, so a
    /// version bump can't update one without the other.
    const SOURCE: &str = include_str!("../../assets/quarto-schema/.panache-source");

    #[test]
    fn embedded_schema_deserializes_and_roots_resolve() {
        let s = schema();
        assert!(
            s.defs.contains_key(&s.roots.frontmatter),
            "frontmatter root must resolve"
        );
        assert!(
            s.defs.contains_key(&s.roots.project),
            "project root must resolve"
        );
        // A well-known frontmatter key should be reachable from the root.
        assert!(s.defs.len() > 100, "expected the full definition set");
    }

    #[test]
    fn version_matches_panache_source() {
        let tag = SOURCE
            .lines()
            .find_map(|l| l.strip_prefix("tag="))
            .expect("tag= in .panache-source");
        assert_eq!(schema().version, tag, "schema version drifted from pin");
    }
}

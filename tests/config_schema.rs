//! JSON Schema for `panache.toml`.
//!
//! Generates the schema from the host `Config` type and:
//!   * keeps `docs/reference/panache.schema.json` in sync (set
//!     `UPDATE_EXPECTED=1` to regenerate when the schema legitimately
//!     drifts), and
//!   * validates every fixture `panache.toml` against the schema so the
//!     published schema can't silently reject configs the parser accepts.
//!
//! Run with `UPDATE_EXPECTED=1 cargo test config_schema` after intentional
//! schema-shape changes; review the diff before committing.

use std::fs;
use std::path::Path;

use jsonschema::Validator;
use panache::Config;
use serde_json::Value;

const SCHEMA_ID: &str = "https://panache.bz/panache.schema.json";
const SCHEMA_PATH: &str = "panache.schema.json";

fn generate_schema_json() -> Value {
    let schema = schemars::schema_for!(Config);
    let mut json: Value = serde_json::to_value(&schema).expect("schema to JSON");
    if let Value::Object(map) = &mut json {
        map.insert("$id".to_string(), Value::String(SCHEMA_ID.to_string()));
        map.insert(
            "title".to_string(),
            Value::String("Panache configuration".to_string()),
        );
        map.insert(
            "description".to_string(),
            Value::String(
                "Schema for panache.toml. Generated from the host Config types; \
                 do not hand-edit — run `UPDATE_EXPECTED=1 cargo test config_schema` instead."
                    .to_string(),
            ),
        );
    }
    json
}

fn schema_path() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(SCHEMA_PATH)
}

fn write_pretty(json: &Value) -> String {
    let mut out = serde_json::to_string_pretty(json).expect("serialize schema");
    out.push('\n');
    out
}

#[test]
fn schema_is_in_sync_with_config_types() {
    let generated = generate_schema_json();
    let pretty = write_pretty(&generated);
    let path = schema_path();

    if std::env::var_os("UPDATE_EXPECTED").is_some() {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create docs/reference dir");
        }
        fs::write(&path, &pretty).expect("write schema");
        return;
    }

    let on_disk = fs::read_to_string(&path).unwrap_or_else(|err| {
        panic!(
            "missing {}: {err}. Run `UPDATE_EXPECTED=1 cargo test config_schema` to create it.",
            path.display()
        )
    });

    similar_asserts::assert_eq!(
        on_disk,
        pretty,
        "{} is out of date with the host Config types. \
         Run `UPDATE_EXPECTED=1 cargo test config_schema` to regenerate.",
        path.display()
    );
}

fn toml_to_json(toml_str: &str) -> Value {
    let value: toml::Value = toml::from_str(toml_str).expect("parse fixture TOML");
    serde_json::to_value(value).expect("toml → json")
}

fn build_validator() -> Validator {
    let schema = generate_schema_json();
    Validator::new(&schema).expect("compile schema")
}

#[test]
fn schema_accepts_every_fixture_panache_toml() {
    let validator = build_validator();
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("cases");

    let mut failures: Vec<String> = Vec::new();
    let entries = fs::read_dir(&root).expect("read cases dir");
    for entry in entries.flatten() {
        let toml_path = entry.path().join("panache.toml");
        if !toml_path.is_file() {
            continue;
        }
        let body = fs::read_to_string(&toml_path).expect("read fixture");
        // Skip fixtures with TOML datetimes / other JSON-incompatible values
        // — none today, but defensive.
        let Ok(toml_value) = toml::from_str::<toml::Value>(&body) else {
            failures.push(format!("{}: not valid TOML", toml_path.display()));
            continue;
        };
        let json = match serde_json::to_value(toml_value) {
            Ok(v) => v,
            Err(err) => {
                failures.push(format!(
                    "{}: TOML → JSON failed: {err}",
                    toml_path.display()
                ));
                continue;
            }
        };
        let errors: Vec<String> = validator
            .iter_errors(&json)
            .map(|e| format!("    {} at {}", e, e.instance_path()))
            .collect();
        if !errors.is_empty() {
            failures.push(format!("{}:\n{}", toml_path.display(), errors.join("\n")));
        }
    }

    assert!(
        failures.is_empty(),
        "schema rejected fixture configs that the parser accepts:\n{}",
        failures.join("\n\n")
    );
}

#[test]
fn schema_rejects_unknown_top_level_key() {
    // Sanity: even if we don't set `additionalProperties: false` at the top
    // level today, this test pins the *intended* shape — when we do tighten
    // the schema later, this guards the regression.
    let validator = build_validator();
    let toml = r#"
flavor = "pandoc"

[format]
wrap = "definitely-not-a-wrap-mode"
"#;
    let json = toml_to_json(toml);
    let errors: Vec<_> = validator.iter_errors(&json).collect();
    assert!(
        !errors.is_empty(),
        "schema must reject `format.wrap = \"definitely-not-a-wrap-mode\"`"
    );
}

#[test]
fn schema_rejects_bad_flavor_enum() {
    let validator = build_validator();
    let toml = r#"flavor = "not-a-real-flavor""#;
    let json = toml_to_json(toml);
    let errors: Vec<_> = validator.iter_errors(&json).collect();
    assert!(
        !errors.is_empty(),
        "schema must reject `flavor = \"not-a-real-flavor\"`"
    );
}

#[test]
fn schema_rejects_bad_pandoc_compat_enum() {
    let validator = build_validator();
    let toml = r#"pandoc-compat = "9.0""#;
    let json = toml_to_json(toml);
    let errors: Vec<_> = validator.iter_errors(&json).collect();
    assert!(
        !errors.is_empty(),
        "schema must reject `pandoc-compat = \"9.0\"`"
    );
}

#[test]
fn schema_rejects_non_integer_line_width() {
    let validator = build_validator();
    let toml = r#"line-width = "eighty""#;
    let json = toml_to_json(toml);
    let errors: Vec<_> = validator.iter_errors(&json).collect();
    assert!(
        !errors.is_empty(),
        "schema must reject `line-width = \"eighty\"`"
    );
}

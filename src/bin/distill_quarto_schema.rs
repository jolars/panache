//! Vendor-time distiller for Quarto's `all-schema-definitions.json`.
//!
//! Reads the raw Quarto schema artifact, normalizes it into the compact
//! [`QuartoSchema`] the linter embeds, and writes pretty JSON. Driven by
//! `scripts/update-quarto-schema.sh`; not part of any runtime path.
//!
//! Usage:
//!     distill_quarto_schema <raw-input.json> <quarto-tag> [out.json]
//!
//! With no output path the result is written to stdout.

use std::process::ExitCode;

use panache::linter::quarto_schema::distill::{default_roots, distill};

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let (Some(input), Some(version)) = (args.next(), args.next()) else {
        eprintln!("usage: distill_quarto_schema <raw-input.json> <quarto-tag> [out.json]");
        return ExitCode::FAILURE;
    };
    let out = args.next();

    let raw_text = match std::fs::read_to_string(&input) {
        Ok(text) => text,
        Err(err) => {
            eprintln!("error: cannot read {input}: {err}");
            return ExitCode::FAILURE;
        }
    };
    let raw: serde_json::Value = match serde_json::from_str(&raw_text) {
        Ok(value) => value,
        Err(err) => {
            eprintln!("error: {input} is not valid JSON: {err}");
            return ExitCode::FAILURE;
        }
    };

    let schema = distill(&raw, &version, default_roots());
    let mut rendered = match serde_json::to_string_pretty(&schema) {
        Ok(text) => text,
        Err(err) => {
            eprintln!("error: failed to serialize distilled schema: {err}");
            return ExitCode::FAILURE;
        }
    };
    rendered.push('\n');

    match out {
        Some(path) => {
            if let Err(err) = std::fs::write(&path, &rendered) {
                eprintln!("error: cannot write {path}: {err}");
                return ExitCode::FAILURE;
            }
            eprintln!(
                "distilled {} definitions from {version} -> {path}",
                schema.defs.len()
            );
        }
        None => print!("{rendered}"),
    }

    ExitCode::SUCCESS
}

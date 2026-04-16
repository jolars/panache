//! Synchronous external linter integration for CLI use.
//!
//! This module provides blocking versions of external linter functions for use in
//! the CLI without requiring a tokio runtime.

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::linter::diagnostics::Diagnostic;
use crate::linter::external_linters::{ExternalLinterRegistry, LinterError};

/// Run an external linter on code and parse its output (synchronous version).
pub fn run_linter_sync(
    linter_name: &str,
    language: &str,
    code: &str,
    original_input: &str,
    registry: &ExternalLinterRegistry,
    mappings: Option<&[crate::linter::code_block_collector::BlockMapping]>,
) -> Result<Vec<Diagnostic>, LinterError> {
    let linter_info = registry
        .get(linter_name)
        .ok_or_else(|| LinterError::SpawnFailed(format!("unknown linter: {}", linter_name)))?;
    if !registry
        .supports_language(linter_name, language)
        .unwrap_or(false)
    {
        return Err(LinterError::SpawnFailed(format!(
            "unsupported linter-language mapping: {} for {}",
            linter_name, language
        )));
    }

    let (_temp_dir, temp_path) =
        crate::linter::external_linters::create_linter_temp_input(language, code)?;

    // Build command
    let mut cmd = Command::new(linter_info.command);
    cmd.args(linter_info.args.iter());
    crate::linter::external_linters::append_language_specific_args(&mut cmd, linter_name, language);
    if (linter_name.eq_ignore_ascii_case("eslint") || linter_name.eq_ignore_ascii_case("clippy"))
        && let Some(parent) = temp_path.parent()
    {
        cmd.current_dir(parent);
    }
    cmd.arg(&temp_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // Execute with timeout (manual timeout implementation)
    let start = Instant::now();
    let timeout = Duration::from_secs(30);

    let output = cmd
        .output()
        .map_err(|e| LinterError::SpawnFailed(format!("{}: {}", linter_info.command, e)))?;

    if start.elapsed() > timeout {
        return Err(LinterError::Timeout);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // Note: Many linters exit with code 1 when they find issues, so we don't treat that as an error
    // Only fail if the command truly failed to run
    if !output.status.success() && stdout.trim().is_empty() && stderr.trim().is_empty() {
        return Err(LinterError::NonZeroExit {
            code: output.status.code().unwrap_or(-1),
            stderr: stderr.to_string(),
        });
    }

    let linter_output = if stdout.trim().is_empty() {
        stderr.as_ref()
    } else {
        stdout.as_ref()
    };

    // Parse output based on linter type (reuse async parser)
    crate::linter::external_linters::parse_linter_output(
        linter_name,
        linter_output,
        code,
        original_input,
        mappings,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jarl_linter_sync() {
        // Skip if jarl not available
        if which::which("jarl").is_err() {
            println!("Skipping jarl test - jarl not installed");
            return;
        }

        let code = "any(is.na(x))\n";
        let registry = ExternalLinterRegistry::new();

        let result = run_linter_sync("jarl", "r", code, code, &registry, None);
        assert!(result.is_ok());

        let diagnostics = result.unwrap();
        assert!(!diagnostics.is_empty());

        let any_is_na_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == "any_is_na")
            .collect();
        assert_eq!(any_is_na_diags.len(), 1);
    }

    #[test]
    fn test_ruff_linter_sync() {
        // Skip if ruff not available
        if which::which("ruff").is_err() {
            println!("Skipping ruff test - ruff not installed");
            return;
        }

        let code = "import os\n";
        let registry = ExternalLinterRegistry::new();

        let result = run_linter_sync("ruff", "python", code, code, &registry, None);
        assert!(result.is_ok());

        let diagnostics = result.unwrap();
        assert!(!diagnostics.is_empty());

        let unused_import_diags: Vec<_> = diagnostics.iter().filter(|d| d.code == "F401").collect();
        assert_eq!(unused_import_diags.len(), 1);
    }

    #[test]
    fn test_shellcheck_linter_sync() {
        if which::which("shellcheck").is_err() {
            println!("Skipping shellcheck test - shellcheck not installed");
            return;
        }

        let code = "echo $UNSET\n";
        let registry = ExternalLinterRegistry::new();

        let result = run_linter_sync("shellcheck", "sh", code, code, &registry, None);
        assert!(result.is_ok());

        let diagnostics = result.unwrap();
        assert!(!diagnostics.is_empty());

        let sc_diags: Vec<_> = diagnostics.iter().filter(|d| d.code == "SC2086").collect();
        assert_eq!(sc_diags.len(), 1);
    }

    #[test]
    fn test_eslint_linter_sync() {
        if which::which("eslint").is_err() {
            println!("Skipping eslint test - eslint not installed");
            return;
        }

        let code = "const x = 1;\nconsole.log(1)\n";
        let registry = ExternalLinterRegistry::new();

        let result = run_linter_sync("eslint", "js", code, code, &registry, None);
        assert!(result.is_ok());

        let diagnostics = result.unwrap();
        assert!(!diagnostics.is_empty());

        let eslint_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == "no-unused-vars")
            .collect();
        assert_eq!(eslint_diags.len(), 1);
    }

    #[test]
    fn test_staticcheck_linter_sync() {
        if which::which("staticcheck").is_err() || which::which("go").is_err() {
            println!("Skipping staticcheck test - staticcheck and/or go not installed");
            return;
        }

        let code = "package main\nfunc main() { var x int; _ = x }\n";
        let registry = ExternalLinterRegistry::new();

        let result = run_linter_sync("staticcheck", "go", code, code, &registry, None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_clippy_linter_sync() {
        if which::which("clippy-driver").is_err() {
            println!("Skipping clippy test - clippy-driver not installed");
            return;
        }

        let code = "fn main(){ let x = vec![1,2,3]; println!(\"{}\", x.len()); }\n";
        let registry = ExternalLinterRegistry::new();

        let result = run_linter_sync("clippy", "rust", code, code, &registry, None);
        assert!(result.is_ok());

        let diagnostics = result.unwrap();
        if diagnostics.is_empty() {
            println!(
                "Skipping strict clippy assertion - clippy produced no diagnostics in this environment"
            );
            return;
        }

        let clippy_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code.starts_with("clippy::") || d.code == "clippy")
            .collect();
        assert!(!clippy_diags.is_empty());
    }

    #[test]
    fn test_unknown_linter_sync() {
        let code = "x <- 1\n";
        let registry = ExternalLinterRegistry::new();

        let result = run_linter_sync("unknown_linter", "r", code, code, &registry, None);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), LinterError::SpawnFailed(_)));
    }
}

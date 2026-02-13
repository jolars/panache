//! Synchronous external linter integration for CLI use.
//!
//! This module provides blocking versions of external linter functions for use in
//! the CLI without requiring a tokio runtime.

use std::io::Write;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::linter::diagnostics::Diagnostic;
use crate::linter::external_linters::{ExternalLinterRegistry, LinterError};

/// Run an external linter on code and parse its output (synchronous version).
pub fn run_linter_sync(
    linter_name: &str,
    code: &str,
    registry: &ExternalLinterRegistry,
) -> Result<Vec<Diagnostic>, LinterError> {
    let linter_info = registry
        .get(linter_name)
        .ok_or_else(|| LinterError::SpawnFailed(format!("unknown linter: {}", linter_name)))?;

    // Create temp file with code
    let mut temp_file = tempfile::NamedTempFile::new()?;
    temp_file.write_all(code.as_bytes())?;
    temp_file.flush()?;

    let temp_path = temp_file.path();

    // Build command
    let mut cmd = Command::new(linter_info.command);
    cmd.args(linter_info.args.iter())
        .arg(temp_path)
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
    if !output.status.success() && stdout.is_empty() {
        return Err(LinterError::NonZeroExit {
            code: output.status.code().unwrap_or(-1),
            stderr: stderr.to_string(),
        });
    }

    // Parse output based on linter type (reuse async parser)
    crate::linter::external_linters::parse_linter_output(linter_name, &stdout, code)
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

        let code = "x = 1\n";
        let registry = ExternalLinterRegistry::new();

        let result = run_linter_sync("jarl", code, &registry);
        assert!(result.is_ok());

        let diagnostics = result.unwrap();
        assert!(!diagnostics.is_empty());

        // Should find assignment issue
        let assignment_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == "assignment")
            .collect();
        assert_eq!(assignment_diags.len(), 1);
    }

    #[test]
    fn test_unknown_linter_sync() {
        let code = "x <- 1\n";
        let registry = ExternalLinterRegistry::new();

        let result = run_linter_sync("unknown_linter", code, &registry);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), LinterError::SpawnFailed(_)));
    }
}

//! External code formatter integration.
//!
//! This module handles spawning external formatter processes (like `black`, `air`, `rustfmt`)
//! and piping code through them via stdin/stdout.

use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use crate::config::FormatterConfig;

/// Errors that can occur when invoking external formatters.
#[derive(Debug)]
pub enum FormatterError {
    /// Formatter command not found or failed to spawn
    SpawnFailed(String),
    /// Formatter process exited with non-zero status
    NonZeroExit { code: i32, stderr: String },
    /// Formatter timed out
    Timeout,
    /// I/O error during communication with formatter
    IoError(std::io::Error),
}

impl std::fmt::Display for FormatterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SpawnFailed(cmd) => write!(f, "failed to spawn formatter: {}", cmd),
            Self::NonZeroExit { code, stderr } => {
                write!(f, "formatter exited with code {}: {}", code, stderr)
            }
            Self::Timeout => write!(f, "formatter timed out"),
            Self::IoError(e) => write!(f, "formatter I/O error: {}", e),
        }
    }
}

impl std::error::Error for FormatterError {}

impl From<std::io::Error> for FormatterError {
    fn from(e: std::io::Error) -> Self {
        Self::IoError(e)
    }
}

/// Format a code block using an external formatter.
///
/// # Arguments
/// * `code` - The code content to format
/// * `config` - Formatter configuration (command, args, etc.)
/// * `timeout` - Maximum duration to wait for the formatter
///
/// # Returns
/// * `Ok(String)` - Formatted code on success
/// * `Err(FormatterError)` - Error details if formatting failed
pub async fn format_code_async(
    code: &str,
    config: &FormatterConfig,
    timeout: Duration,
) -> Result<String, FormatterError> {
    log::debug!(
        "Invoking formatter: {} {}",
        config.cmd,
        config.args.join(" ")
    );

    // Spawn the formatter process
    let mut child = Command::new(&config.cmd)
        .args(&config.args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| FormatterError::SpawnFailed(format!("{}: {}", config.cmd, e)))?;

    // Write code to stdin
    let mut stdin = child.stdin.take().expect("stdin was piped");
    stdin
        .write_all(code.as_bytes())
        .await
        .map_err(FormatterError::IoError)?;
    drop(stdin); // Close stdin to signal EOF

    // Wait for process with timeout
    let output = tokio::time::timeout(timeout, child.wait_with_output())
        .await
        .map_err(|_| FormatterError::Timeout)?
        .map_err(FormatterError::IoError)?;

    // Check exit status
    if !output.status.success() {
        let code = output.status.code().unwrap_or(-1);
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        log::warn!(
            "Formatter '{}' failed with exit code {}: {}",
            config.cmd,
            code,
            stderr
        );
        return Err(FormatterError::NonZeroExit { code, stderr });
    }

    // Parse output
    let formatted = String::from_utf8(output.stdout).map_err(|e| {
        FormatterError::IoError(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("formatter output not valid UTF-8: {}", e),
        ))
    })?;

    log::debug!(
        "Formatter '{}' succeeded ({} bytes -> {} bytes)",
        config.cmd,
        code.len(),
        formatted.len()
    );

    Ok(formatted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_cat_formatter() {
        // 'cat' is a simple formatter that just echoes input
        let config = FormatterConfig {
            cmd: "cat".to_string(),
            args: vec![],
            enabled: true,
        };

        let code = "hello world\n";
        let result = format_code_async(code, &config, Duration::from_secs(5))
            .await
            .unwrap();

        assert_eq!(result, code);
    }

    #[tokio::test]
    async fn test_uppercase_formatter() {
        // 'tr' can convert to uppercase
        let config = FormatterConfig {
            cmd: "tr".to_string(),
            args: vec!["[:lower:]".to_string(), "[:upper:]".to_string()],
            enabled: true,
        };

        let code = "hello world";
        let result = format_code_async(code, &config, Duration::from_secs(5))
            .await
            .unwrap();

        assert_eq!(result, "HELLO WORLD");
    }

    #[tokio::test]
    async fn test_missing_command() {
        let config = FormatterConfig {
            cmd: "nonexistent_formatter_12345".to_string(),
            args: vec![],
            enabled: true,
        };

        let code = "test";
        let result = format_code_async(code, &config, Duration::from_secs(5)).await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            FormatterError::SpawnFailed(_)
        ));
    }

    #[tokio::test]
    async fn test_nonzero_exit() {
        // 'false' always exits with code 1
        let config = FormatterConfig {
            cmd: "sh".to_string(),
            args: vec!["-c".to_string(), "exit 1".to_string()],
            enabled: true,
        };

        let code = "test";
        let result = format_code_async(code, &config, Duration::from_secs(5)).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            FormatterError::NonZeroExit { code, .. } => assert_eq!(code, 1),
            _ => panic!("expected NonZeroExit error"),
        }
    }

    #[tokio::test]
    async fn test_timeout() {
        // 'sleep 10' should timeout with 1 second limit
        let config = FormatterConfig {
            cmd: "sleep".to_string(),
            args: vec!["10".to_string()],
            enabled: true,
        };

        let code = "test";
        let result = format_code_async(code, &config, Duration::from_millis(100)).await;

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), FormatterError::Timeout));
    }
}

//! External code formatter integration (async version).
//!
//! This module handles spawning external formatter processes (like `black`, `air`, `rustfmt`)
//! and piping code through them via stdin/stdout or temporary files using tokio.

use std::time::Duration;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::time::Instant;

use crate::config::FormatterConfig;
pub use crate::external_formatters_common::FormatterError;
use crate::external_formatters_common::{
    FormatterIoMode, log_formatter_invocation, log_formatter_nonzero_exit,
    log_formatter_spawn_failed, log_formatter_success, log_formatter_timeout, resolve_stdin_args,
    temp_file_extension_for_language,
};

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
    language: &str,
    config: &FormatterConfig,
    timeout: Duration,
) -> Result<String, FormatterError> {
    if config.stdin {
        format_with_stdin(code, language, config, timeout).await
    } else {
        format_with_file(code, language, config, timeout).await
    }
}

/// Format code by piping through stdin/stdout.
async fn format_with_stdin(
    code: &str,
    language: &str,
    config: &FormatterConfig,
    timeout: Duration,
) -> Result<String, FormatterError> {
    let resolved_args = resolve_stdin_args(&config.args, language);
    log_formatter_invocation(
        &config.cmd,
        language,
        FormatterIoMode::Stdin,
        &resolved_args,
    );
    let start = Instant::now();

    // Spawn the formatter process
    let mut child = Command::new(&config.cmd)
        .args(&resolved_args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| {
            log_formatter_spawn_failed(&config.cmd, language, FormatterIoMode::Stdin, &e);
            FormatterError::SpawnFailed(format!("{}: {}", config.cmd, e))
        })?;

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
        .map_err(|_| {
            log_formatter_timeout(&config.cmd, language, FormatterIoMode::Stdin);
            FormatterError::Timeout
        })?
        .map_err(FormatterError::IoError)?;

    // Check exit status
    if !output.status.success() {
        let code = output.status.code().unwrap_or(-1);
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        log_formatter_nonzero_exit(&config.cmd, language, FormatterIoMode::Stdin, code, &stderr);
        return Err(FormatterError::NonZeroExit { code, stderr });
    }

    // Parse output
    let formatted = String::from_utf8(output.stdout).map_err(|e| {
        FormatterError::IoError(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("formatter output not valid UTF-8: {}", e),
        ))
    })?;

    log_formatter_success(
        &config.cmd,
        language,
        FormatterIoMode::Stdin,
        formatted.len(),
        start.elapsed(),
    );

    Ok(formatted)
}

/// Format code using a temporary file.
async fn format_with_file(
    code: &str,
    language: &str,
    config: &FormatterConfig,
    timeout: Duration,
) -> Result<String, FormatterError> {
    let start = Instant::now();

    // Create a temporary file using tempfile crate
    let mut temp_file = tempfile::Builder::new()
        .suffix(&format!(".{}", temp_file_extension_for_language(language)))
        .tempfile()
        .map_err(FormatterError::IoError)?;

    // Write code to temp file (sync write since NamedTempFile is std::fs::File)
    use std::io::Write;
    temp_file
        .write_all(code.as_bytes())
        .map_err(FormatterError::IoError)?;
    temp_file.flush().map_err(FormatterError::IoError)?;

    let temp_path = temp_file.path();

    // Build args with temp file path (replace {} placeholder or append)
    let args: Vec<String> = if config.args.iter().any(|arg| arg.contains("{}")) {
        config
            .args
            .iter()
            .map(|arg| arg.replace("{}", temp_path.to_str().unwrap()))
            .collect()
    } else {
        let mut args = config.args.clone();
        args.push(temp_path.to_str().unwrap().to_string());
        args
    };
    log_formatter_invocation(&config.cmd, language, FormatterIoMode::File, &args);
    log::trace!(
        "External formatter temp path ({}): {}",
        config.cmd,
        temp_path.display()
    );

    // Spawn the formatter process
    let child = Command::new(&config.cmd)
        .args(&args)
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| {
            log_formatter_spawn_failed(&config.cmd, language, FormatterIoMode::File, &e);
            FormatterError::SpawnFailed(format!("{}: {}", config.cmd, e))
        })?;

    // Wait for process with timeout
    let output = tokio::time::timeout(timeout, child.wait_with_output())
        .await
        .map_err(|_| {
            log_formatter_timeout(&config.cmd, language, FormatterIoMode::File);
            FormatterError::Timeout
        })?
        .map_err(FormatterError::IoError)?;

    // Read formatted content from file (async read)
    let formatted = fs::read_to_string(&temp_path)
        .await
        .map_err(FormatterError::IoError)?;

    // Temp file automatically cleaned up when temp_file is dropped

    // Check exit status
    if !output.status.success() {
        let code = output.status.code().unwrap_or(-1);
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        log_formatter_nonzero_exit(&config.cmd, language, FormatterIoMode::File, code, &stderr);
        return Err(FormatterError::NonZeroExit { code, stderr });
    }

    log_formatter_success(
        &config.cmd,
        language,
        FormatterIoMode::File,
        formatted.len(),
        start.elapsed(),
    );

    Ok(formatted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_cat_formatter() {
        // A simple formatter that just echoes input
        #[cfg(not(target_os = "windows"))]
        let config = FormatterConfig {
            cmd: "cat".to_string(),
            args: vec![],
            enabled: true,
            stdin: true,
        };

        #[cfg(target_os = "windows")]
        let config = FormatterConfig {
            cmd: "cmd".to_string(),
            args: vec!["/c".to_string(), "more".to_string()],
            enabled: true,
            stdin: true,
        };

        let code = "hello world\n";
        let result = format_code_async(code, "text", &config, Duration::from_secs(5))
            .await
            .unwrap();

        // Normalize line endings for cross-platform comparison
        let normalized_result = result.replace("\r\n", "\n");
        assert_eq!(normalized_result.trim_end(), code.trim_end());
    }

    #[tokio::test]
    #[cfg(not(target_os = "windows"))]
    async fn test_uppercase_formatter() {
        // 'tr' can convert to uppercase (Unix only - no simple Windows equivalent)
        let config = FormatterConfig {
            cmd: "tr".to_string(),
            args: vec!["[:lower:]".to_string(), "[:upper:]".to_string()],
            enabled: true,
            stdin: true,
        };

        let code = "hello world";
        let result = format_code_async(code, "text", &config, Duration::from_secs(5))
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
            stdin: true,
        };

        let code = "test";
        let result = format_code_async(code, "text", &config, Duration::from_secs(5)).await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            FormatterError::SpawnFailed(_)
        ));
    }

    #[tokio::test]
    async fn test_timeout() {
        // Command that sleeps for 10 seconds - should timeout with 100ms limit
        #[cfg(not(target_os = "windows"))]
        let config = FormatterConfig {
            cmd: "sleep".to_string(),
            args: vec!["10".to_string()],
            enabled: true,
            stdin: true,
        };

        #[cfg(target_os = "windows")]
        let config = FormatterConfig {
            cmd: "powershell".to_string(),
            args: vec![
                "-Command".to_string(),
                "Start-Sleep -Seconds 10".to_string(),
            ],
            enabled: true,
            stdin: true,
        };

        let code = "test";
        let result = format_code_async(code, "text", &config, Duration::from_millis(100)).await;

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), FormatterError::Timeout));
    }

    #[tokio::test]
    async fn test_file_based_formatter() {
        // Reading from file - simulates formatters that don't use stdin
        #[cfg(not(target_os = "windows"))]
        let config = FormatterConfig {
            cmd: "cat".to_string(),
            args: vec![],
            enabled: true,
            stdin: false,
        };

        #[cfg(target_os = "windows")]
        let config = FormatterConfig {
            cmd: "cmd".to_string(),
            args: vec!["/c".to_string(), "type".to_string()],
            enabled: true,
            stdin: false,
        };

        let code = "hello from file\n";
        let result = format_code_async(code, "text", &config, Duration::from_secs(5))
            .await
            .unwrap();

        // Normalize line endings for cross-platform comparison
        let normalized_result = result.replace("\r\n", "\n");
        assert_eq!(normalized_result, code);
    }

    #[tokio::test]
    async fn test_file_formatter_with_placeholder() {
        // Test {} placeholder replacement in args
        #[cfg(not(target_os = "windows"))]
        let config = FormatterConfig {
            cmd: "sh".to_string(),
            args: vec![
                "-c".to_string(),
                "cat \"$1\"".to_string(),
                "sh".to_string(),
                "{}".to_string(),
            ],
            enabled: true,
            stdin: false,
        };

        #[cfg(target_os = "windows")]
        let config = FormatterConfig {
            cmd: "cmd".to_string(),
            args: vec!["/c".to_string(), "type".to_string(), "{}".to_string()],
            enabled: true,
            stdin: false,
        };

        let code = "test with placeholder\n";
        let result = format_code_async(code, "text", &config, Duration::from_secs(5))
            .await
            .unwrap();

        // Normalize line endings for cross-platform comparison
        let normalized_result = result.replace("\r\n", "\n");
        assert_eq!(normalized_result, code);
    }
}

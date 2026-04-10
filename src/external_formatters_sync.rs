//! Synchronous external code formatter integration.
//!
//! This module handles spawning external formatter processes using standard threads
//! instead of async/await. Suitable for CLI and WASM contexts.

use std::io::Write;
use std::process::{Command, Stdio};

use std::thread;
use std::time::Duration;

use crate::config::FormatterConfig;
pub use crate::external_formatters_common::FormatterError;
use crate::external_formatters_common::{
    find_missing_formatter_commands, log_missing_formatter_commands, resolve_stdin_args,
    temp_file_extension_for_language,
};
use crate::formatter::code_blocks::{ExternalCodeBlock, FormattedCodeMap};

/// Format a code block using an external formatter (synchronous).
///
/// # Arguments
/// * `code` - The code content to format
/// * `config` - Formatter configuration (command, args, etc.)
/// * `timeout` - Maximum duration to wait for the formatter
///
/// # Returns
/// * `Ok(String)` - Formatted code on success
/// * `Err(FormatterError)` - Error details if formatting failed
pub fn format_code_sync(
    code: &str,
    language: &str,
    config: &FormatterConfig,
    timeout: Duration,
) -> Result<String, FormatterError> {
    if config.stdin {
        format_with_stdin(code, language, config, timeout)
    } else {
        format_with_file(code, language, config, timeout)
    }
}

/// Format code by piping through stdin/stdout (synchronous).
fn format_with_stdin(
    code: &str,
    language: &str,
    config: &FormatterConfig,
    timeout: Duration,
) -> Result<String, FormatterError> {
    let resolved_args = resolve_stdin_args(&config.args, language);
    log::debug!(
        "Invoking formatter (stdin): {} {}",
        config.cmd,
        resolved_args.join(" ")
    );

    // Build command
    let mut cmd = Command::new(&config.cmd);
    cmd.args(&resolved_args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // Spawn process
    let mut child = cmd.spawn().map_err(|e| {
        log::error!("Failed to spawn formatter '{}': {}", config.cmd, e);
        FormatterError::SpawnFailed(format!("{}: {}", config.cmd, e))
    })?;

    // Write input to stdin
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(code.as_bytes())?;
        drop(stdin); // Close stdin to signal EOF
    }

    // Use std::sync::mpsc for timeout handling
    use std::sync::mpsc;
    use std::time::Instant;

    let (tx, rx) = mpsc::channel();

    // Spawn thread to wait for process
    thread::spawn(move || {
        let output = child.wait_with_output();
        let _ = tx.send(output);
    });

    // Wait with timeout
    let start = Instant::now();
    match rx.recv_timeout(timeout) {
        Ok(Ok(output)) => {
            if output.status.success() {
                let formatted = String::from_utf8_lossy(&output.stdout).to_string();
                log::debug!(
                    "Formatter succeeded: {} bytes output in {:?}",
                    formatted.len(),
                    start.elapsed()
                );
                Ok(formatted)
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                log::warn!(
                    "Formatter exited with code {:?}: {}",
                    output.status.code(),
                    stderr
                );
                Err(FormatterError::NonZeroExit {
                    code: output.status.code().unwrap_or(-1),
                    stderr,
                })
            }
        }
        Ok(Err(e)) => {
            log::error!("Formatter I/O error: {}", e);
            Err(FormatterError::IoError(e))
        }
        Err(_) => {
            log::warn!("Formatter timed out after {:?}", timeout);
            Err(FormatterError::Timeout)
        }
    }
}

/// Format code using a temporary file (synchronous).
fn format_with_file(
    code: &str,
    language: &str,
    config: &FormatterConfig,
    timeout: Duration,
) -> Result<String, FormatterError> {
    use std::fs;

    log::debug!(
        "Invoking formatter (file): {} {}",
        config.cmd,
        config.args.join(" ")
    );

    // Create a temporary file
    let temp_dir = std::env::temp_dir();
    let temp_path = temp_dir.join(format!(
        "panache-{}.{}",
        uuid::Uuid::new_v4(),
        temp_file_extension_for_language(language)
    ));

    // Write code to temp file
    fs::write(&temp_path, code).map_err(FormatterError::IoError)?;

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

    // Spawn the formatter process
    let mut cmd = Command::new(&config.cmd);
    cmd.args(&args).stderr(Stdio::piped());

    let child = cmd.spawn().map_err(|e| {
        log::error!("Failed to spawn formatter '{}': {}", config.cmd, e);
        FormatterError::SpawnFailed(format!("{}: {}", config.cmd, e))
    })?;

    // Use std::sync::mpsc for timeout handling
    use std::sync::mpsc;
    use std::time::Instant;

    let (tx, rx) = mpsc::channel();

    // Spawn thread to wait for process
    thread::spawn(move || {
        let output = child.wait_with_output();
        let _ = tx.send(output);
    });

    // Wait with timeout
    let start = Instant::now();
    let result = match rx.recv_timeout(timeout) {
        Ok(Ok(output)) => {
            // Read formatted content from file
            let formatted = fs::read_to_string(&temp_path).map_err(FormatterError::IoError)?;

            // Check exit status
            if output.status.success() {
                log::debug!(
                    "Formatter succeeded: {} bytes output in {:?}",
                    formatted.len(),
                    start.elapsed()
                );
                Ok(formatted)
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                log::warn!(
                    "Formatter exited with code {:?}: {}",
                    output.status.code(),
                    stderr
                );
                Err(FormatterError::NonZeroExit {
                    code: output.status.code().unwrap_or(-1),
                    stderr,
                })
            }
        }
        Ok(Err(e)) => {
            log::error!("Formatter I/O error: {}", e);
            Err(FormatterError::IoError(e))
        }
        Err(_) => {
            log::warn!("Formatter timed out after {:?}", timeout);
            Err(FormatterError::Timeout)
        }
    };

    // Clean up temp file
    let _ = fs::remove_file(&temp_path);

    result
}

/// Run external formatters in parallel using threads.
///
/// # Arguments
/// * `blocks` - Vector of code blocks to format
/// * `formatters` - Map of language to formatter config
/// * `timeout` - Timeout per formatter invocation
///
/// # Returns
/// HashMap of original code -> formatted code (only successful formats)
pub fn run_formatters_parallel(
    blocks: Vec<ExternalCodeBlock>,
    formatters: &std::collections::HashMap<String, Vec<FormatterConfig>>,
    timeout: Duration,
    max_parallel: usize,
) -> FormattedCodeMap {
    use rayon::prelude::*;

    let missing_formatters = find_missing_formatter_commands(formatters);
    log_missing_formatter_commands(&missing_formatters);

    let max_parallel = max_parallel.max(1);

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(max_parallel)
        .build()
        .expect("failed to build rayon thread pool");

    pool.install(|| {
        blocks
            .into_par_iter()
            .filter_map(|block| {
                let lang = block.language;
                let formatter_configs = formatters.get(&lang)?;
                if formatter_configs.is_empty() {
                    return None;
                }

                let formatter_configs = formatter_configs.clone();
                let mut current_code = block.formatter_input;
                let original = block.original;
                let hashpipe_prefix = block.hashpipe_prefix;

                for (idx, formatter_cfg) in formatter_configs.iter().enumerate() {
                    let formatter_cmd = formatter_cfg.cmd.trim();

                    if formatter_cmd.is_empty() {
                        continue;
                    }

                    if missing_formatters.contains(formatter_cmd) {
                        return None;
                    }

                    log::debug!(
                        "Formatting {} code with {} ({}/{} in chain)",
                        lang,
                        formatter_cfg.cmd,
                        idx + 1,
                        formatter_configs.len()
                    );

                    match format_code_sync(&current_code, &lang, formatter_cfg, timeout) {
                        Ok(formatted) => {
                            current_code = formatted;
                        }
                        Err(e) => {
                            log::warn!(
                                "{} formatter '{}' failed: {}. Using original code.",
                                lang,
                                formatter_cfg.cmd,
                                e
                            );
                            return None;
                        }
                    }
                }

                if current_code == original {
                    return None;
                }

                let output = if let Some(prefix) = hashpipe_prefix {
                    format!("{}{}", prefix, current_code)
                } else {
                    current_code
                };

                Some(((lang, original), output))
            })
            .collect::<FormattedCodeMap>()
    })
}

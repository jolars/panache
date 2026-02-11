//! Synchronous external code formatter integration.
//!
//! This module handles spawning external formatter processes using standard threads
//! instead of async/await. Suitable for CLI and WASM contexts.

use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::config::FormatterConfig;
pub use crate::external_formatters_common::FormatterError;

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
    config: &FormatterConfig,
    timeout: Duration,
) -> Result<String, FormatterError> {
    log::debug!(
        "Invoking formatter (sync): {} {}",
        config.cmd,
        config.args.join(" ")
    );

    // Build command
    let mut cmd = Command::new(&config.cmd);
    cmd.args(&config.args)
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

/// Run external formatters in parallel using threads.
///
/// # Arguments
/// * `blocks` - Vector of (language, code) pairs to format
/// * `formatters` - Map of language to formatter config
/// * `timeout` - Timeout per formatter invocation
///
/// # Returns
/// HashMap of original code -> formatted code (only successful formats)
pub fn run_formatters_parallel(
    blocks: Vec<(String, String)>,
    formatters: &std::collections::HashMap<String, FormatterConfig>,
    timeout: Duration,
) -> std::collections::HashMap<String, String> {
    use std::collections::HashMap;

    let results = Arc::new(Mutex::new(HashMap::new()));

    thread::scope(|s| {
        let mut handles = Vec::new();

        for (lang, code) in blocks {
            if let Some(formatter_cfg) = formatters.get(&lang)
                && formatter_cfg.enabled
                && !formatter_cfg.cmd.is_empty()
            {
                let formatter_cfg = formatter_cfg.clone();
                let code = code.clone();
                let lang = lang.clone();
                let results = Arc::clone(&results);

                let handle = s.spawn(move || {
                    log::info!("Formatting {} code with {}", lang, formatter_cfg.cmd);
                    match format_code_sync(&code, &formatter_cfg, timeout) {
                        Ok(formatted) => {
                            if formatted != code {
                                results.lock().unwrap().insert(code, formatted);
                            }
                        }
                        Err(e) => {
                            log::warn!("Failed to format {} code: {}", lang, e);
                        }
                    }
                });

                handles.push(handle);
            }
        }

        // Wait for all threads to complete
        for handle in handles {
            let _ = handle.join();
        }
    });

    Arc::try_unwrap(results).unwrap().into_inner().unwrap()
}

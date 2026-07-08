//! Synchronous external code formatter integration.
//!
//! This module handles spawning external formatter processes using standard threads
//! instead of async/await. Suitable for CLI and WASM contexts.

use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};

use std::thread;
use std::time::Duration;

use crate::config::FormatterConfig;
pub use crate::external_formatters_common::FormatterError;
use crate::external_formatters_common::{
    FormatterIoMode, find_missing_formatter_commands, log_formatter_invocation,
    log_formatter_nonzero_exit, log_formatter_spawn_failed, log_formatter_success,
    log_formatter_timeout, log_missing_formatter_commands, resolve_file_args,
    resolve_formatter_configs, resolve_stdin_args, temp_file_extension_for_language,
};
use panache_formatter::{ExternalCodeBlock, FormattedCodeMap};

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
    log_formatter_invocation(
        &config.cmd,
        language,
        FormatterIoMode::Stdin,
        &resolved_args,
    );

    // Build command
    let mut cmd = Command::new(&config.cmd);
    cmd.args(&resolved_args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // Spawn process
    let mut child = cmd.spawn().map_err(|e| {
        log_formatter_spawn_failed(&config.cmd, language, FormatterIoMode::Stdin, &e);
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
                log_formatter_success(
                    &config.cmd,
                    language,
                    FormatterIoMode::Stdin,
                    formatted.len(),
                    start.elapsed(),
                );
                Ok(formatted)
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                let code = output.status.code().unwrap_or(-1);
                log_formatter_nonzero_exit(
                    &config.cmd,
                    language,
                    FormatterIoMode::Stdin,
                    code,
                    &stderr,
                );
                Err(FormatterError::NonZeroExit { code, stderr })
            }
        }
        Ok(Err(e)) => {
            log::error!("Formatter I/O error: {}", e);
            Err(FormatterError::IoError(e))
        }
        Err(_) => {
            log_formatter_timeout(&config.cmd, language, FormatterIoMode::Stdin);
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

    // Create a temporary file
    let temp_dir = std::env::temp_dir();
    let temp_path = temp_dir.join(format!(
        "panache-{}.{}",
        uuid::Uuid::new_v4(),
        temp_file_extension_for_language(language)
    ));

    // Write code to temp file
    fs::write(&temp_path, code).map_err(FormatterError::IoError)?;

    let args = resolve_file_args(&config.args, language, temp_path.to_str().unwrap());

    log_formatter_invocation(&config.cmd, language, FormatterIoMode::File, &args);
    log::trace!(
        "External formatter temp path ({}): {}",
        config.cmd,
        temp_path.display()
    );

    // Spawn the formatter process
    let mut cmd = Command::new(&config.cmd);
    cmd.args(&args).stderr(Stdio::piped());

    let child = cmd.spawn().map_err(|e| {
        log_formatter_spawn_failed(&config.cmd, language, FormatterIoMode::File, &e);
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
                log_formatter_success(
                    &config.cmd,
                    language,
                    FormatterIoMode::File,
                    formatted.len(),
                    start.elapsed(),
                );
                Ok(formatted)
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                let code = output.status.code().unwrap_or(-1);
                log_formatter_nonzero_exit(
                    &config.cmd,
                    language,
                    FormatterIoMode::File,
                    code,
                    &stderr,
                );
                Err(FormatterError::NonZeroExit { code, stderr })
            }
        }
        Ok(Err(e)) => {
            log::error!("Formatter I/O error: {}", e);
            Err(FormatterError::IoError(e))
        }
        Err(_) => {
            log_formatter_timeout(&config.cmd, language, FormatterIoMode::File);
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
    formatters: &HashMap<String, Vec<FormatterConfig>>,
    timeout: Duration,
    max_parallel: usize,
) -> FormattedCodeMap {
    use rayon::prelude::*;

    let missing_formatters = find_missing_formatter_commands(formatters);
    log_missing_formatter_commands(&missing_formatters);

    let max_parallel = max_parallel.max(1);

    // Dedup: group blocks by the exact formatter input (language +
    // pre-formatting body). Every block in a group produces the same subprocess
    // output, so the formatter chain runs once per group instead of once per
    // block. Blocks in a group can still differ in `original`/`hashpipe_prefix`,
    // so each group fans back out to one map entry per block.
    let mut groups: HashMap<(String, String), Vec<ExternalCodeBlock>> = HashMap::new();
    for block in blocks {
        groups
            .entry((block.language.clone(), block.formatter_input.clone()))
            .or_default()
            .push(block);
    }
    let groups: Vec<((String, String), Vec<ExternalCodeBlock>)> = groups.into_iter().collect();

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(max_parallel)
        .build()
        .expect("failed to build rayon thread pool");

    pool.install(|| {
        groups
            .into_par_iter()
            .flat_map(|((lang, input), blocks)| {
                let Some(formatted) =
                    run_formatter_chain(&lang, &input, formatters, &missing_formatters, timeout)
                else {
                    return Vec::new();
                };

                blocks
                    .into_iter()
                    .filter_map(|block| {
                        if formatted == block.original {
                            return None;
                        }
                        let output = match block.hashpipe_prefix {
                            Some(prefix) => format!("{}{}", prefix, formatted),
                            None => formatted.clone(),
                        };
                        Some(((lang.clone(), block.original), output))
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<FormattedCodeMap>()
    })
}

/// Run the configured formatter chain for `lang` over `input`, returning the
/// formatted output (post-chain, before any hashpipe prefix is re-applied).
///
/// Returns `None` when no formatter is configured, a required command is
/// missing, or any step in the chain fails — matching the per-block fallback of
/// leaving the block unchanged. Results are memoized through
/// [`FORMATTER_CHAIN_CACHE`]; failures are never cached.
fn run_formatter_chain(
    lang: &str,
    input: &str,
    formatters: &HashMap<String, Vec<FormatterConfig>>,
    missing_formatters: &HashSet<String>,
    timeout: Duration,
) -> Option<String> {
    let formatter_configs = resolve_formatter_configs(formatters, lang)?;
    if formatter_configs.is_empty() {
        return None;
    }

    let chain_fp = chain_fingerprint(formatter_configs);
    if let Some(cached) = chain_cache_get(&chain_fp, lang, input) {
        return Some(cached);
    }

    // Bound concurrent subprocesses to the shared external-tool budget. Held for
    // the whole chain; the chain runs sequentially, so at most one subprocess
    // per permit is live at a time. Acquired only on a cache miss, so cache hits
    // never consume budget.
    let _permit = crate::external_tools_common::acquire_external_tool_permit();

    let mut current_code = input.to_string();
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

        match format_code_sync(&current_code, lang, formatter_cfg, timeout) {
            Ok(formatted) => {
                current_code = formatted;
            }
            Err(e) => {
                log::warn!(
                    "{} formatter '{}' failed: {}. Falling back to original code block unchanged.",
                    lang,
                    formatter_cfg.cmd,
                    e
                );
                return None;
            }
        }
    }

    chain_cache_put(&chain_fp, lang, input, &current_code);
    Some(current_code)
}

/// Process-global memoization of external-formatter chain results, keyed on the
/// resolved chain (cmd + args + flags) plus language and pre-formatting input.
///
/// External formatters are deterministic functions of their input, so this
/// dedups identical blocks within a run *and* — in a long-lived process (the
/// LSP) — reuses results across edits, so the editor save loop no longer
/// re-spawns the formatter for every untouched block. Cold processes (a one-shot
/// `panache format`) only get the in-run dedup, which is also why a short-lived
/// CLI run carries no cross-run staleness risk.
///
/// Caveat: the key does not capture the external tool's *own* config files
/// (`.prettierrc`, `pyproject.toml`, …). Like the file-level CLI cache, edits to
/// those are only observed after the process restarts.
static FORMATTER_CHAIN_CACHE: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();

/// Upper bound on cached chain results. Once full the cache stops accepting new
/// entries (entries are immutable, so no eviction is needed for correctness);
/// the working set of a typical editor session stays well under this.
const FORMATTER_CHAIN_CACHE_CAP: usize = 8192;

/// Fingerprint the formatter chain so a config change (different cmd/args/flags)
/// produces a fresh cache key instead of returning stale output. `\u{1}` and
/// `\u{2}` separate fields/entries to keep the encoding unambiguous.
fn chain_fingerprint(configs: &[FormatterConfig]) -> String {
    let mut fp = String::new();
    for cfg in configs {
        fp.push_str(cfg.cmd.trim());
        fp.push('\u{1}');
        for arg in &cfg.args {
            fp.push_str(arg);
            fp.push('\u{1}');
        }
        fp.push(if cfg.stdin { 'S' } else { 'F' });
        fp.push('\u{2}');
    }
    fp
}

fn chain_cache_key(chain_fp: &str, lang: &str, input: &str) -> String {
    // `\u{0}` cannot appear in the chain fingerprint or language, so this is an
    // unambiguous join of the three components.
    format!("{chain_fp}\u{0}{lang}\u{0}{input}")
}

fn chain_cache_get(chain_fp: &str, lang: &str, input: &str) -> Option<String> {
    let cache = FORMATTER_CHAIN_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let key = chain_cache_key(chain_fp, lang, input);
    cache
        .lock()
        .expect("formatter chain cache mutex poisoned")
        .get(&key)
        .cloned()
}

fn chain_cache_put(chain_fp: &str, lang: &str, input: &str, output: &str) {
    let cache = FORMATTER_CHAIN_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let key = chain_cache_key(chain_fp, lang, input);
    let mut guard = cache.lock().expect("formatter chain cache mutex poisoned");
    if guard.len() >= FORMATTER_CHAIN_CACHE_CAP && !guard.contains_key(&key) {
        return;
    }
    guard.insert(key, output.to_string());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(cmd: &str, args: &[&str], stdin: bool) -> FormatterConfig {
        FormatterConfig {
            cmd: cmd.to_string(),
            args: args.iter().map(|a| a.to_string()).collect(),
            stdin,
        }
    }

    #[test]
    fn chain_fingerprint_distinguishes_cmd_args_and_flags() {
        let base = chain_fingerprint(&[cfg("black", &["-"], true)]);
        // Different command, args, or io-mode must all change the fingerprint so
        // a config change never returns a stale cached result.
        assert_ne!(base, chain_fingerprint(&[cfg("blue", &["-"], true)]));
        assert_ne!(base, chain_fingerprint(&[cfg("black", &["-q", "-"], true)]));
        assert_ne!(base, chain_fingerprint(&[cfg("black", &["-"], false)]));
        // A two-step chain differs from either single step.
        assert_ne!(
            base,
            chain_fingerprint(&[cfg("black", &["-"], true), cfg("isort", &["-"], true)])
        );
        // Identical config reproduces the same fingerprint.
        assert_eq!(base, chain_fingerprint(&[cfg("black", &["-"], true)]));
    }

    #[test]
    fn chain_fingerprint_is_unambiguous_across_field_boundaries() {
        // The arg-vs-flag boundary must not collide: `["ab"]` and `["a", "b"]`
        // are different chains and must fingerprint differently.
        assert_ne!(
            chain_fingerprint(&[cfg("fmt", &["ab"], true)]),
            chain_fingerprint(&[cfg("fmt", &["a", "b"], true)])
        );
    }

    #[test]
    fn cache_round_trips_per_chain_lang_and_input() {
        // Use a fingerprint unique to this test so the process-global cache can't
        // collide with other tests sharing the static.
        let fp = "test-roundtrip\u{2}";
        assert_eq!(chain_cache_get(fp, "py", "x=1"), None);

        chain_cache_put(fp, "py", "x=1", "x = 1");
        assert_eq!(chain_cache_get(fp, "py", "x=1").as_deref(), Some("x = 1"));

        // Distinct input / language / chain are separate entries.
        assert_eq!(chain_cache_get(fp, "py", "y=2"), None);
        assert_eq!(chain_cache_get(fp, "sh", "x=1"), None);
        assert_eq!(chain_cache_get("other\u{2}", "py", "x=1"), None);
    }
}

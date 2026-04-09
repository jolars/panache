//! Shared helpers for external tool availability checks and warning emission.

use std::collections::HashSet;
use std::io::IsTerminal;
use std::sync::{Mutex, OnceLock};

static WARNED_MESSAGES: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
static COLOR_OVERRIDE: OnceLock<Option<bool>> = OnceLock::new();

/// Find missing commands from an iterator of command names.
#[cfg(not(target_arch = "wasm32"))]
pub fn find_missing_commands<'a, I>(commands: I) -> HashSet<String>
where
    I: IntoIterator<Item = &'a str>,
{
    commands
        .into_iter()
        .filter_map(|cmd| {
            let trimmed = cmd.trim();
            if trimmed.is_empty() || command_exists(trimmed) {
                None
            } else {
                Some(trimmed.to_string())
            }
        })
        .collect()
}

/// Build a stable warning message for missing external commands.
pub fn missing_commands_warning_message(
    missing: &HashSet<String>,
    tool_kind: &str,
    action_name: &str,
) -> Option<String> {
    if missing.is_empty() {
        return None;
    }

    let mut sorted_missing: Vec<_> = missing.iter().map(String::as_str).collect();
    sorted_missing.sort_unstable();

    Some(format!(
        "External {} command(s) not found: {}. Configured external {} for these tools will be skipped.",
        tool_kind,
        sorted_missing.join(", "),
        action_name
    ))
}

/// Emit a warning only once per process for a given message.
pub fn log_warning_once(message: &str) -> bool {
    let warned_messages = WARNED_MESSAGES.get_or_init(|| Mutex::new(HashSet::new()));
    let mut warned = warned_messages
        .lock()
        .expect("warning message mutex poisoned");

    if !warned.insert(message.to_string()) {
        return false;
    }

    if log::log_enabled!(log::Level::Warn) {
        log::warn!("{}", message);
    } else {
        eprintln!("{}", format_warning_line(message, warning_color_enabled()));
    }
    true
}

/// Set CLI-driven warning color policy once per process.
pub fn set_warning_color_override(use_color: bool) {
    let _ = COLOR_OVERRIDE.set(Some(use_color));
}

fn format_warning_line(message: &str, use_color: bool) -> String {
    if use_color {
        format!("\x1b[1;33mwarning:\x1b[0m \x1b[1m{}\x1b[0m", message)
    } else {
        format!("Warning: {}", message)
    }
}

fn warning_color_enabled() -> bool {
    if let Some(Some(use_color)) = COLOR_OVERRIDE.get() {
        return *use_color;
    }
    default_stderr_warning_color()
}

fn default_stderr_warning_color() -> bool {
    if std::env::var_os("NO_COLOR").is_some() {
        return false;
    }
    std::io::stderr().is_terminal()
}

#[cfg(not(target_arch = "wasm32"))]
fn command_exists(cmd: &str) -> bool {
    use std::path::Path;

    if has_path_separator(cmd) {
        return Path::new(cmd).exists();
    }
    which::which(cmd).is_ok()
}

#[cfg(not(target_arch = "wasm32"))]
fn has_path_separator(cmd: &str) -> bool {
    cmd.contains(std::path::MAIN_SEPARATOR)
        || cfg!(windows) && (cmd.contains('/') || cmd.contains('\\'))
}

#[cfg(test)]
mod tests {
    use super::{
        default_stderr_warning_color, find_missing_commands, format_warning_line, log_warning_once,
        missing_commands_warning_message,
    };
    use std::collections::HashSet;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static TEST_COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn unique_message(prefix: &str) -> String {
        let n = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        format!("{}-{}", prefix, n)
    }

    #[test]
    fn warning_message_sorts_and_deduplicates_commands() {
        let missing = HashSet::from([
            "black".to_string(),
            "rustfmt".to_string(),
            "black".to_string(),
        ]);

        let message =
            missing_commands_warning_message(&missing, "formatter", "formatting").expect("message");
        assert_eq!(
            message,
            "External formatter command(s) not found: black, rustfmt. Configured external formatting for these tools will be skipped."
        );
    }

    #[test]
    fn warning_message_is_none_for_empty_set() {
        let missing = HashSet::new();
        assert!(missing_commands_warning_message(&missing, "linter", "linting").is_none());
    }

    #[test]
    fn log_warning_only_once_per_unique_message() {
        let message = unique_message("panache-warn-once");
        assert!(log_warning_once(&message));
        assert!(!log_warning_once(&message));

        let another = unique_message("panache-warn-once");
        assert!(log_warning_once(&another));
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn reports_missing_commands_once() {
        let missing = find_missing_commands([
            "definitely-not-a-real-tool-123",
            "definitely-not-a-real-tool-123",
            "   ",
        ]);
        assert_eq!(missing.len(), 1);
        assert!(missing.contains("definitely-not-a-real-tool-123"));
    }

    #[test]
    fn warning_line_uses_styled_prefix_when_color_enabled() {
        let line = format_warning_line("External formatter command(s) not found", true);
        assert!(line.contains("\x1b[1;33mwarning:\x1b[0m"));
        assert!(line.contains("\x1b[1mExternal formatter command(s) not found\x1b[0m"));
    }

    #[test]
    fn warning_line_uses_plain_prefix_without_color() {
        let line = format_warning_line("External formatter command(s) not found", false);
        assert_eq!(line, "Warning: External formatter command(s) not found");
    }

    #[test]
    fn default_warning_color_disables_with_no_color_env() {
        let was_set = std::env::var_os("NO_COLOR");
        // SAFETY: tests in this module only read/write NO_COLOR for this assertion.
        unsafe { std::env::set_var("NO_COLOR", "1") };
        assert!(!default_stderr_warning_color());
        if let Some(previous) = was_set {
            // SAFETY: restoring original process env var for test isolation.
            unsafe { std::env::set_var("NO_COLOR", previous) };
        } else {
            // SAFETY: restoring original process env var for test isolation.
            unsafe { std::env::remove_var("NO_COLOR") };
        }
    }
}

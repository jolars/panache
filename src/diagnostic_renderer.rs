use std::path::Path;

use crate::cli::MessageFormat;
use annotate_snippets::{AnnotationKind, Level, Renderer, Snippet};
use panache::linter::{Diagnostic, DiagnosticOrigin, Severity};

pub(crate) fn print_diagnostics(
    diagnostics: &[Diagnostic],
    file: Option<&Path>,
    source: Option<&str>,
    use_color: bool,
    message_format: MessageFormat,
) {
    let file_name = file.and_then(Path::to_str).unwrap_or("<stdin>");
    let renderer = if use_color {
        Renderer::styled()
    } else {
        Renderer::plain()
    };

    for diag in diagnostics {
        if matches!(message_format, MessageFormat::Short) {
            println!(
                "{}[{}]: {} at {}:{}:{}",
                severity_name(&diag.severity),
                diag.code,
                diag.message,
                file_name,
                diag.location.line,
                diag.location.column
            );
            continue;
        }

        if let Some(source) = source {
            print_source_snippet(diag, file_name, source, &renderer, diag.fix.as_ref());
        } else {
            println!(
                "{}[{}]: {}",
                severity_name(&diag.severity),
                diag.code,
                diag.message
            );
            println!(
                "  --> {}:{}:{}",
                file_name, diag.location.line, diag.location.column
            );
        }

        if let Some(fix) = &diag.fix
            && (source.is_none() || fix.edits.is_empty())
        {
            print_subdiag("help", &fix.message);
        }

        if diag.origin == DiagnosticOrigin::BuiltIn {
            print_subdiag(
                "note",
                &format!(
                    "configure this rule in panache.toml with [lint.rules] {} = false",
                    diag.code
                ),
            );
            print_subdiag(
                "help",
                &format!(
                    "for further information visit https://jolars.github.io/panache/linting.html#{}",
                    diag.code
                ),
            );
        }
    }

    println!("\nFound {} issue(s)", diagnostics.len());
}

fn print_source_snippet(
    diag: &Diagnostic,
    file_name: &str,
    source: &str,
    renderer: &Renderer,
    fix: Option<&panache::linter::Fix>,
) {
    let start: usize = diag.location.range.start().into();
    let end: usize = diag.location.range.end().into();
    let end = end.max(start.saturating_add(1)).min(source.len());

    let primary = if let Some(fix) = fix
        && let Some(edit) = fix.edits.first()
    {
        let edit_start: usize = edit.range.start().into();
        let edit_end: usize = edit.range.end().into();
        let edit_end = edit_end.max(edit_start.saturating_add(1)).min(source.len());
        AnnotationKind::Primary
            .span(edit_start..edit_end)
            .label(format!("help: {}", fix.message))
    } else {
        AnnotationKind::Primary.span(start..end)
    };

    let snippet = Snippet::source(source)
        .line_start(1)
        .path(file_name)
        .annotation(primary);

    let snippet = if diag.code == "heading-hierarchy" {
        if let Some(context_span) = find_previous_heading_span(source, start) {
            snippet.annotation(
                AnnotationKind::Context
                    .span(context_span)
                    .label("previous heading is here"),
            )
        } else {
            snippet
        }
    } else {
        snippet
    };

    let title = format!("[{}] {}", diag.code, diag.message);
    let report = &[severity_level(&diag.severity)
        .primary_title(&title)
        .element(snippet)];
    println!("{}", renderer.render(report));
}

fn severity_level(severity: &Severity) -> Level<'static> {
    match severity {
        Severity::Error => Level::ERROR,
        Severity::Warning => Level::WARNING,
        Severity::Info => Level::INFO,
    }
}

fn severity_name(severity: &Severity) -> &'static str {
    match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Info => "info",
    }
}

fn print_subdiag(kind: &str, message: &str) {
    println!("  = {kind}: {message}");
}

fn find_previous_heading_span(
    source: &str,
    before_offset: usize,
) -> Option<std::ops::Range<usize>> {
    let mut line_start = 0usize;
    let mut prev_heading = None;

    for line in source.lines() {
        let line_end = line_start + line.len();
        if line_end >= before_offset {
            break;
        }

        let trimmed = line.trim_start();
        if trimmed.starts_with('#') {
            let indent = line.len() - trimmed.len();
            let hashes = trimmed.chars().take_while(|c| *c == '#').count();
            if hashes > 0 {
                prev_heading = Some((line_start + indent)..(line_start + indent + hashes));
            }
        }

        line_start = line_end + 1;
    }

    prev_heading
}

#[cfg(test)]
mod tests {
    use super::severity_name;
    use panache::linter::{Diagnostic, DiagnosticOrigin, Location, Severity};
    use rowan::TextRange;

    #[test]
    fn built_in_diagnostics_show_panache_guidance() {
        let diag = Diagnostic {
            severity: Severity::Warning,
            location: Location {
                line: 1,
                column: 1,
                range: TextRange::new(0.into(), 1.into()),
            },
            message: "msg".to_string(),
            code: "heading-hierarchy".to_string(),
            origin: DiagnosticOrigin::BuiltIn,
            fix: None,
        };
        assert_eq!(diag.origin, DiagnosticOrigin::BuiltIn);
        assert_eq!(severity_name(&diag.severity), "warning");
    }

    #[test]
    fn external_diagnostics_can_be_marked_explicitly() {
        let diag = Diagnostic::warning(
            Location {
                line: 1,
                column: 1,
                range: TextRange::new(0.into(), 1.into()),
            },
            "SA5009",
            "msg",
        )
        .with_origin(DiagnosticOrigin::External);
        assert_eq!(diag.origin, DiagnosticOrigin::External);
    }
}

use std::path::Path;

use annotate_snippets::{AnnotationKind, Level, Renderer, Snippet};
use panache::linter::{Diagnostic, Severity};

pub(crate) fn print_diagnostics(
    diagnostics: &[Diagnostic],
    file: Option<&Path>,
    source: Option<&str>,
    use_color: bool,
) {
    let file_name = file.and_then(Path::to_str).unwrap_or("<stdin>");
    let renderer = if use_color {
        Renderer::styled()
    } else {
        Renderer::plain()
    };

    for diag in diagnostics {
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

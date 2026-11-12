//! Generate the rule catalog from metadata and examples run by the real linter.

use std::fmt::Write as _;
use std::fs;
use std::io;

use crate::config::{self, Flavor};
use crate::linter::fix::apply_fixes;
use crate::linter::render::DiagnosticRenderer;
use crate::linter::{Diagnostic, Requirement, Rule, Severity, all_rules, lint_with_metadata};

/// A reference example, including the context needed to reproduce its findings.
pub struct Example {
    pub caption: &'static str,
    pub source: &'static str,
    pub expected_codes: &'static [&'static str],
    pub flavor: Flavor,
    pub config: &'static str,
    pub companions: &'static [(&'static str, &'static str)],
    pub allow_unsafe_fixes: bool,
}

impl Example {
    pub const fn new(source: &'static str, expected_codes: &'static [&'static str]) -> Self {
        Self {
            caption: "",
            source,
            expected_codes,
            flavor: Flavor::Quarto,
            config: "",
            companions: &[],
            allow_unsafe_fixes: false,
        }
    }

    pub const fn caption(mut self, caption: &'static str) -> Self {
        self.caption = caption;
        self
    }

    pub const fn flavor(mut self, flavor: Flavor) -> Self {
        self.flavor = flavor;
        self
    }

    pub const fn config(mut self, config: &'static str) -> Self {
        self.config = config;
        self
    }

    pub const fn companions(mut self, companions: &'static [(&'static str, &'static str)]) -> Self {
        self.companions = companions;
        self
    }

    pub const fn unsafe_fixes(mut self) -> Self {
        self.allow_unsafe_fixes = true;
        self
    }

    fn filename(&self) -> &'static str {
        match self.flavor {
            Flavor::RMarkdown => "example.Rmd",
            Flavor::CommonMark | Flavor::Gfm => "example.md",
            _ => "example.qmd",
        }
    }
}

/// Lint an example without reading personal configuration or external tools.
/// Companion files use a temporary directory so bibliography examples exercise
/// the same file loading as ordinary documents.
pub fn lint_example(rule: &dyn Rule, example: &Example) -> io::Result<Vec<Diagnostic>> {
    lint_example_source(rule, example, example.source)
}

/// Recheck edited example text under the original flavor and companion files.
pub fn lint_example_source(
    rule: &dyn Rule,
    example: &Example,
    source: &str,
) -> io::Result<Vec<Diagnostic>> {
    let directory = tempfile::tempdir()?;
    let root = directory.path();
    for (name, source) in example.companions {
        let path = root.join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, source)?;
    }
    let config_path = root.join("panache.toml");
    fs::write(&config_path, example.config)?;
    let path = root.join(example.filename());
    let (mut config, _) =
        config::load(Some(&config_path), root, Some(&path), Some(example.flavor))?;
    config.lint.select = Some(vec![rule.name().to_string()]);
    config.lint.extend_select.clear();
    config.lint.ignore.clear();
    let tree = crate::parse(source, Some(config.clone()));
    let metadata = match crate::metadata::extract_metadata(&tree, &path) {
        Ok(metadata) => Some(metadata),
        Err(crate::metadata::YamlError::NotFound(_)) => None,
        Err(error) => return Err(io::Error::other(error)),
    };
    let mut diagnostics = lint_with_metadata(&tree, source, &config, metadata.as_ref());

    // Temporary paths are implementation details, and would change every render.
    let prefix = format!(
        "{}{separator}",
        root.display(),
        separator = std::path::MAIN_SEPARATOR
    );
    for diagnostic in &mut diagnostics {
        diagnostic.message = diagnostic.message.replace(&prefix, "");
        for note in &mut diagnostic.notes {
            note.message = note.message.replace(&prefix, "");
        }
        if let Some(fix) = &mut diagnostic.fix {
            fix.message = fix.message.replace(&prefix, "");
        }
    }
    Ok(diagnostics)
}

/// Render one rule with its declared facts, explanations, and current output.
pub fn render_rule(rule: &dyn Rule) -> io::Result<String> {
    let meta = rule.metadata();
    let mut out = String::new();
    writeln!(out, "### `{name}` {{#{name}}}\n", name = meta.name).unwrap();
    writeln!(out, "{}\n", rule.description().trim()).unwrap();
    writeln!(
        out,
        "Default\n:   {}\n",
        if meta.default_on { "On" } else { "Off" }
    )
    .unwrap();
    if !meta.default_on {
        writeln!(
            out,
            "Enable this rule with `[lint] extend-select = [\"{}\"]`.\n",
            meta.name
        )
        .unwrap();
    }
    writeln!(
        out,
        "Auto-fix\n:   {}\n",
        if meta.auto_fix {
            "Available; see the examples for fix safety."
        } else {
            "No"
        }
    )
    .unwrap();
    if let Some(requirement) = requirement_description(meta.requires) {
        writeln!(out, "Requirements\n:   {requirement}\n").unwrap();
    }
    writeln!(out, "Diagnostic codes\n").unwrap();
    writeln!(out, "| Code | Severity |").unwrap();
    writeln!(out, "| --- | --- |").unwrap();
    for code in meta.codes {
        writeln!(
            out,
            "| [`{code}`](#{code}) | {severity} |",
            code = code.code,
            severity = severity_name(code.severity)
        )
        .unwrap();
    }
    for code in meta.codes.iter().filter(|code| code.code != meta.name) {
        writeln!(out, "\n#### `{code}` {{#{code}}}\n", code = code.code).unwrap();
        if let Some(description) = rule.code_description(code.code) {
            writeln!(out, "{}\n", description.trim()).unwrap();
        }
    }
    writeln!(out, "\n#### Examples\n").unwrap();
    for example in rule.examples() {
        if !example.caption.is_empty() {
            writeln!(out, "{}\n", example.caption).unwrap();
        }
        if example.flavor != Flavor::Quarto {
            writeln!(
                out,
                "This example uses the {} flavor.\n",
                flavor_name(example.flavor)
            )
            .unwrap();
        }
        if !example.config.is_empty() {
            writeln!(out, "With this configuration:\n").unwrap();
            fenced(&mut out, "toml", example.config);
        }
        for (name, source) in example.companions {
            writeln!(out, "With `{name}`:\n").unwrap();
            let language = match name.rsplit('.').next() {
                Some("bib") => "bibtex",
                Some("json") => "json",
                _ => "yaml",
            };
            fenced(&mut out, language, source);
        }
        fenced(&mut out, "markdown", example.source);
        let diagnostics = lint_example(rule, example)?;
        let renderer = DiagnosticRenderer::new(example.filename(), Some(example.source), false);
        let rendered: String = diagnostics
            .iter()
            .map(|diagnostic| renderer.render(diagnostic))
            .collect();
        fenced(&mut out, "text", rendered.trim_end());

        let fixed = apply_fixes(example.source, &diagnostics, example.allow_unsafe_fixes);
        if fixed.output != example.source {
            let command = if example.allow_unsafe_fixes {
                "--fix --unsafe-fixes"
            } else {
                "--fix"
            };
            writeln!(
                out,
                "After `panache lint {command}`{}:\n",
                if example.allow_unsafe_fixes {
                    " (review the change before applying it)"
                } else {
                    ""
                }
            )
            .unwrap();
            fenced(&mut out, "markdown", &fixed.output);
        }
    }
    Ok(out)
}

/// Render the catalog included by `docs/reference/linter-rules.qmd`.
pub fn render_catalog() -> io::Result<String> {
    let mut out = String::from(
        "<!-- Generated by `task docs:rules`. Edit descriptions and examples in src/linter/rules/. -->\n\n",
    );
    for rule in all_rules() {
        out.push_str(&render_rule(rule.as_ref())?);
        out.push('\n');
    }
    let mut config = crate::Config {
        flavor: Flavor::Quarto,
        extensions: crate::config::Extensions::for_flavor(Flavor::Quarto),
        ..Default::default()
    };
    config.formatters.clear();
    Ok(crate::format(&out, Some(config), None))
}

fn fenced(out: &mut String, language: &str, source: &str) {
    // Quarto inspects executable cells even inside a longer Markdown fence.
    // Doubling their braces displays the original cell without executing it.
    let escaped;
    let source = if language == "markdown" {
        static CELLS: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
            regex::Regex::new(r"(?m)^([ \t>]*(?:`{3,}|~{3,}))\{([^{}\n]*)\}").unwrap()
        });
        escaped = CELLS.replace_all(source, "$1{{$2}}");
        escaped.as_ref()
    } else {
        source
    };
    let longest_run = source
        .split(|character| character != '`')
        .map(str::len)
        .max()
        .unwrap_or(0);
    let fence = "`".repeat(3.max(longest_run + 1));
    writeln!(out, "{fence}{language}").unwrap();
    out.push_str(source);
    if !source.ends_with('\n') {
        out.push('\n');
    }
    writeln!(out, "{fence}\n").unwrap();
}

fn severity_name(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "Error",
        Severity::Warning => "Warning",
        Severity::Info => "Info",
    }
}

fn flavor_name(flavor: Flavor) -> &'static str {
    match flavor {
        Flavor::Pandoc => "Pandoc",
        Flavor::Quarto => "Quarto",
        Flavor::RMarkdown => "R Markdown",
        Flavor::CommonMark => "CommonMark",
        Flavor::Gfm => "GFM",
        Flavor::MultiMarkdown => "MultiMarkdown",
        Flavor::Mdsvex => "mdsvex",
        Flavor::Myst => "MyST",
        Flavor::Hugo => "Hugo",
        Flavor::Jupyter => "Jupyter",
    }
}

fn requirement_description(requirement: Requirement) -> Option<&'static str> {
    Some(match requirement {
        Requirement::Always => return None,
        Requirement::HeaderAttributes => "`extensions.header-attributes = true`.",
        Requirement::Footnotes => "`extensions.footnotes = true`.",
        Requirement::InlineFootnotes => "`extensions.inline-footnotes = true`.",
        Requirement::Citations => "`extensions.citations = true`.",
        Requirement::FencedCodeAttributes => "`extensions.fenced-code-attributes = true`.",
        Requirement::FencedDivs => "`extensions.fenced-divs = true`.",
        Requirement::Emoji => "`extensions.emoji = true`.",
        Requirement::RawHtml => "`extensions.raw-html = true`.",
        Requirement::TexMath => "At least one `tex-math-*` extension is enabled.",
        Requirement::TexMathDollars => "`extensions.tex-math-dollars = true`.",
        Requirement::ChunkFlavor => "The Quarto or R Markdown flavor.",
        Requirement::Quarto => "The Quarto flavor.",
        Requirement::PandocMetadata => "The Pandoc, Quarto, or R Markdown flavor.",
    })
}

#[cfg(test)]
mod tests {
    use super::fenced;

    #[test]
    fn executable_cells_are_displayed_without_being_run_by_quarto() {
        let mut out = String::new();
        fenced(&mut out, "markdown", "```{r}\n#| echo: [\n```\n");
        assert!(out.starts_with("````markdown\n```{{r}}\n"));
        assert!(out.ends_with("```\n````\n\n"));

        out.clear();
        fenced(&mut out, "text", "```{r}\n");
        assert!(out.contains("```{r}\n"));
    }
}

use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use panache::Config;
use panache::config::{ConfigSource, Extensions, Flavor, FormatterExtensions};
use serde::Serialize;

#[derive(Serialize)]
struct Report<'a> {
    flavor: &'static str,
    configuration: Configuration<'a>,
    extensions: BTreeMap<&'static str, ExtensionState>,
}

#[derive(Serialize)]
struct Configuration<'a> {
    source: &'static str,
    path: Option<&'a Path>,
    chain: &'a [PathBuf],
}

#[derive(Serialize)]
struct ExtensionState {
    effective: bool,
    default: bool,
}

pub fn write_report(
    mut out: impl Write,
    cfg: &Config,
    source: &ConfigSource,
    chain: &[PathBuf],
    json: bool,
) -> io::Result<()> {
    let flavor = match cfg.flavor {
        Flavor::Pandoc => "pandoc",
        Flavor::Quarto => "quarto",
        Flavor::RMarkdown => "rmarkdown",
        Flavor::Gfm => "gfm",
        Flavor::CommonMark => "commonmark",
        Flavor::MultiMarkdown => "multimarkdown",
        Flavor::Mdsvex => "mdsvex",
        Flavor::Myst => "myst",
        Flavor::Hugo => "hugo",
        Flavor::Jupyter => "jupyter",
    };
    let (source_name, origin) = match source {
        ConfigSource::Explicit(_) => ("explicit", "--config"),
        ConfigSource::Discovered(_) => ("discovered", "discovered"),
        ConfigSource::Env(_) => ("env", "PANACHE_CONFIG"),
        ConfigSource::Global(_) => ("global", "user config"),
        ConfigSource::None => ("none", "built-in defaults"),
    };

    let parser_defaults = Extensions::for_flavor(cfg.flavor);
    let formatter_defaults = FormatterExtensions::for_flavor(cfg.flavor);
    let defaults: BTreeMap<_, _> = parser_defaults
        .states()
        .chain(formatter_defaults.states())
        .collect();
    let extensions = cfg
        .extensions
        .states()
        .chain(cfg.formatter_extensions.states())
        .map(|(name, effective)| {
            (
                name,
                ExtensionState {
                    effective,
                    default: defaults[name],
                },
            )
        })
        .collect();
    let report = Report {
        flavor,
        configuration: Configuration {
            source: source_name,
            path: source.path(),
            chain,
        },
        extensions,
    };

    if json {
        serde_json::to_writer_pretty(&mut out, &report).map_err(io::Error::other)?;
        return writeln!(out);
    }

    writeln!(out, "Flavor: {}", report.flavor)?;
    if let Some(path) = report.configuration.path {
        writeln!(out, "Configuration: {} ({origin})", path.display())?;
        for path in report.configuration.chain.iter().skip(1) {
            writeln!(out, "  extends {}", path.display())?;
        }
    } else {
        writeln!(out, "Configuration: {origin}")?;
    }
    let width = report
        .extensions
        .keys()
        .map(|name| name.len())
        .max()
        .unwrap_or(9);

    writeln!(out)?;
    writeln!(out, "{:<width$}  {:<9}  Default", "Extension", "Effective")?;
    for (name, state) in report.extensions {
        writeln!(
            out,
            "{name:<width$}  {:<9}  {}",
            state.effective, state.default
        )?;
    }
    Ok(())
}

use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::PathBuf;

use panache::Config;
use panache::config::{ConfigSource, Extensions, Flavor, FormatterExtensions};

pub fn write_report(
    mut out: impl Write,
    cfg: &Config,
    source: &ConfigSource,
    chain: &[PathBuf],
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
    };
    writeln!(out, "Flavor: {flavor}")?;

    let origin = match source {
        ConfigSource::Explicit(_) => "--config",
        ConfigSource::Discovered(_) => "discovered",
        ConfigSource::Env(_) => "PANACHE_CONFIG",
        ConfigSource::Global(_) => "user config",
        ConfigSource::None => "built-in defaults",
    };
    if let Some(path) = source.path() {
        writeln!(out, "Configuration: {} ({origin})", path.display())?;
        for path in chain.iter().skip(1) {
            writeln!(out, "  extends {}", path.display())?;
        }
    } else {
        writeln!(out, "Configuration: {origin}")?;
    }

    let parser_defaults = Extensions::for_flavor(cfg.flavor);
    let formatter_defaults = FormatterExtensions::for_flavor(cfg.flavor);
    let effective: BTreeMap<_, _> = cfg
        .extensions
        .states()
        .chain(cfg.formatter_extensions.states())
        .collect();
    let defaults: BTreeMap<_, _> = parser_defaults
        .states()
        .chain(formatter_defaults.states())
        .collect();
    let width = effective.keys().map(|name| name.len()).max().unwrap_or(9);

    writeln!(out)?;
    writeln!(out, "{:<width$}  {:<9}  Default", "Extension", "Effective")?;
    for (name, enabled) in effective {
        writeln!(out, "{name:<width$}  {enabled:<9}  {}", defaults[name])?;
    }
    Ok(())
}

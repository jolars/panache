use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    pub line_ending: Option<LineEnding>,
    pub line_width: usize,
    pub math_indent: usize,
    pub wrap: Option<WrapMode>,
    pub blank_lines: BlankLines,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            line_ending: Some(LineEnding::Auto),
            line_width: 80,
            math_indent: 0,
            wrap: Some(WrapMode::Reflow),
            blank_lines: BlankLines::Collapse,
        }
    }
}

#[derive(Default, Clone)]
pub struct ConfigBuilder {
    config: Config,
}

impl ConfigBuilder {
    pub fn math_indent(mut self, indent: usize) -> Self {
        self.config.math_indent = indent;
        self
    }

    pub fn line_width(mut self, width: usize) -> Self {
        self.config.line_width = width;
        self
    }

    pub fn line_ending(mut self, ending: LineEnding) -> Self {
        self.config.line_ending = Some(ending);
        self
    }

    pub fn blank_lines(mut self, mode: BlankLines) -> Self {
        self.config.blank_lines = mode;
        self
    }

    pub fn build(self) -> Config {
        self.config
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum WrapMode {
    Preserve,
    Reflow,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum LineEnding {
    Auto,
    Lf,
    Crlf,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum BlankLines {
    /// Preserve original blank lines (any number)
    Preserve,
    /// Collapse multiple consecutive blank lines to a single blank line
    Collapse,
}

const CANDIDATE_NAMES: &[&str] = &[".quartofmt.toml", "quartofmt.toml"];

fn parse_config_str(s: &str, path: &Path) -> io::Result<Config> {
    toml::from_str::<Config>(s).map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid config {}: {e}", path.display()),
        )
    })
}

fn read_config(path: &Path) -> io::Result<Config> {
    let s = fs::read_to_string(path)?;
    parse_config_str(&s, path)
}

fn find_in_tree(start_dir: &Path) -> Option<PathBuf> {
    for dir in start_dir.ancestors() {
        for name in CANDIDATE_NAMES {
            let p = dir.join(name);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

fn xdg_config_path() -> Option<PathBuf> {
    if let Ok(xdg) = env::var("XDG_CONFIG_HOME") {
        let p = Path::new(&xdg).join("quartofmt").join("config.toml");
        if p.is_file() {
            return Some(p);
        }
    }
    if let Ok(home) = env::var("HOME") {
        let p = Path::new(&home)
            .join(".config")
            .join("quartofmt")
            .join("config.toml");
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

/// Load configuration with precedence:
/// 1) explicit path (error if unreadable/invalid)
/// 2) walk up from start_dir: .quartofmt.toml, quartofmt.toml
/// 3) XDG: $XDG_CONFIG_HOME/quartofmt/config.toml or ~/.config/quartofmt/config.toml
/// 4) default config
pub fn load(explicit: Option<&Path>, start_dir: &Path) -> io::Result<(Config, Option<PathBuf>)> {
    if let Some(path) = explicit {
        let cfg = read_config(path)?;
        return Ok((cfg, Some(path.to_path_buf())));
    }

    if let Some(p) = find_in_tree(start_dir)
        && let Ok(cfg) = read_config(&p)
    {
        return Ok((cfg, Some(p)));
    }

    if let Some(p) = xdg_config_path()
        && let Ok(cfg) = read_config(&p)
    {
        return Ok((cfg, Some(p)));
    }

    Ok((Config::default(), None))
}

#[test]
fn missing_fields_panics_on_unwrap() {
    let toml_str = r#"
        wrap = "reflow"
    "#;
    let cfg = toml::from_str::<Config>(toml_str).unwrap();
    let line_width = cfg.line_width; // This will panic and fail the test
    assert_eq!(line_width, 80);
}

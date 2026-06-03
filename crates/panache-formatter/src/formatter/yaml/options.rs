//! Options surface for the in-tree YAML formatter.
//!
//! Kept dependency-lean and free of host config concerns. The bridge
//! that maps from the host `Config` (line-width, wrap mode) into
//! `YamlFormatOptions` lives at the call site, `yaml_engine.rs`
//! (`yaml_wrap_for_config`). As of Phase 2a that bridge targets this
//! struct rather than `pretty_yaml::config::FormatOptions`.

/// Wrapping policy for plain scalars. Quoted (`"…"` / `'…'`) and
/// block (`>` / `|`) styles are never wrapped — see `STYLE.md` (the
/// "Plain-scalar wrapping" section).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WrapMode {
    /// Wrap plain scalars to fit `line_width` with +2 indent on
    /// continuation lines.
    #[default]
    Always,
    /// Leave plain scalars unwrapped regardless of width.
    Preserve,
}

#[derive(Debug, Clone)]
pub struct YamlFormatOptions {
    pub line_width: usize,
    pub wrap: WrapMode,
}

impl Default for YamlFormatOptions {
    fn default() -> Self {
        Self {
            line_width: 80,
            wrap: WrapMode::Always,
        }
    }
}

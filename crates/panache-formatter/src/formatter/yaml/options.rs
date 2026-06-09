//! Options surface for the in-tree YAML formatter.
//!
//! Kept dependency-lean and free of host config concerns. The bridge
//! that maps from the host `Config` (line-width, wrap mode, language)
//! into `YamlFormatOptions` lives in the formatter crate as
//! [`crate::formatter::yaml::options_from_config`], called from both
//! `yaml_engine.rs` bridges. As of Phase 2a that bridge targets this
//! struct rather than `pretty_yaml::config::FormatOptions`.

/// Wrapping policy for plain and folded (`>`) block scalars. Mirrors
/// the host markdown wrap modes so YAML prose reflows the same way as
/// document body prose. Literal (`|`) and quoted (`"…"` / `'…'`)
/// scalars are never wrapped — see `STYLE.md` (the "Plain-scalar
/// wrapping" and rule 15 "Folded block-scalar wrapping" sections).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WrapMode {
    /// Greedy-fill prose to `line_width`: short lines are joined and
    /// the whole scalar is re-wrapped (folding is loss-free).
    #[default]
    Reflow,
    /// One sentence per line; line length is not bounded.
    Sentence,
    /// Sentence breaks layered on top of the author's existing line
    /// breaks (semantic linefeeds); line length is not bounded.
    Semantic,
    /// Leave the scalar's line breaks exactly as the author wrote them.
    Preserve,
}

#[derive(Debug, Clone)]
pub struct YamlFormatOptions {
    pub line_width: usize,
    pub wrap: WrapMode,
    /// Resolved document language code (e.g. `en`, `de`), used only by
    /// the sentence/semantic wrap modes to pick a sentence-boundary
    /// profile. `None` falls back to English.
    pub lang: Option<String>,
    /// User-configured no-break abbreviations already merged for the
    /// active language (`default` bucket + language bucket). Consulted
    /// only by the sentence/semantic wrap modes.
    pub no_break_abbreviations: Vec<String>,
}

impl Default for YamlFormatOptions {
    fn default() -> Self {
        Self {
            line_width: 80,
            wrap: WrapMode::Reflow,
            lang: None,
            no_break_abbreviations: Vec::new(),
        }
    }
}

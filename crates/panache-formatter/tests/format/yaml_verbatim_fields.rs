//! Verbatim frontmatter fields (STYLE.md rule 16). A few top-level
//! frontmatter keys hold code/directives that a downstream *non-YAML*
//! consumer reads line-by-line, so the folded-scalar reflow (rule 15)
//! must leave them untouched even though folding would be loss-free as
//! YAML.
//!
//! `vignette`: R/knitr vignette magic. `tools::vignetteInfo` greps the
//! raw frontmatter lines for `%\VignetteEngine{…}` etc.; folding two
//! directives onto one line hides the engine and breaks `R CMD check`
//! (issue #366).

use panache_formatter::config::WrapMode;
use panache_formatter::{Config, format};

fn reflow80() -> Config {
    Config {
        wrap: Some(WrapMode::Reflow),
        line_width: 80,
        ..Default::default()
    }
}

const VIGNETTE: &str = "---\n\
title: \"mappings\"\n\
output: rmarkdown::html_vignette\n\
vignette: >\n  \
%\\VignetteIndexEntry{mappings}\n  \
%\\VignetteEngine{knitr::rmarkdown}\n  \
%\\VignetteEncoding{UTF-8}\n\
---\n\n\
Body.\n";

#[test]
fn vignette_directives_keep_their_line_breaks() {
    let out = format(VIGNETTE, Some(reflow80()), None);

    // Each directive must stay on its own physical line — R reads them raw.
    for directive in [
        "  %\\VignetteIndexEntry{mappings}\n",
        "  %\\VignetteEngine{knitr::rmarkdown}\n",
        "  %\\VignetteEncoding{UTF-8}\n",
    ] {
        assert!(
            out.contains(directive),
            "directive folded away: {directive:?}\n{out}"
        );
    }
    // The folded header is preserved as-is.
    assert!(out.contains("\nvignette: >\n"), "header changed:\n{out}");
}

#[test]
fn vignette_is_idempotent() {
    let once = format(VIGNETTE, Some(reflow80()), None);
    let twice = format(&once, Some(reflow80()), None);
    assert_eq!(once, twice, "not idempotent:\n{once}");
}

/// Sanity guard: a non-verbatim folded scalar still reflows (rule 15),
/// so the exemption is scoped to the allowlist rather than disabling
/// folded wrapping wholesale.
#[test]
fn non_verbatim_folded_scalar_still_reflows() {
    let long = "This is a fairly long abstract that runs well beyond eighty \
                characters once the folded lines are joined back together into one.";
    let input = format!("---\nabstract: >\n  {long}\n---\n\nBody.\n");
    let out = format(&input, Some(reflow80()), None);
    for line in out.lines() {
        assert!(
            line.chars().count() <= 80,
            "line over width: {line:?}\n{out}"
        );
    }
}

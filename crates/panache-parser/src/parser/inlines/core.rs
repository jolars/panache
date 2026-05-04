//! Inline emission walk.
//!
//! Consumes the IR plans built by [`super::inline_ir::build_full_plans`]
//! (emphasis pairings, bracket resolutions, standalone Pandoc constructs)
//! and emits the inline CST tokens / nodes in source order. Resolution
//! decisions for emphasis, brackets, and standalone Pandoc constructs
//! are entirely IR-driven for both dialects; the dispatcher's
//! `try_parse_*` recognizers are still called to *parse* a matched byte
//! range into a CST subtree, but "what is this byte range?" is answered
//! exclusively by the IR.

use crate::options::{Dialect, ParserOptions};
use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;

use super::inline_ir::{
    BracketPlan, ConstructDispo, ConstructPlan, DelimChar, EmphasisKind, EmphasisPlan,
};

// Import inline element parsers from sibling modules
use super::bookdown::{
    try_parse_bookdown_definition, try_parse_bookdown_reference, try_parse_bookdown_text_reference,
};
use super::bracketed_spans::{emit_bracketed_span, try_parse_bracketed_span};
use super::citations::{
    emit_bare_citation, emit_bracketed_citation, try_parse_bare_citation,
    try_parse_bracketed_citation,
};
use super::code_spans::{emit_code_span, try_parse_code_span};
use super::emoji::{emit_emoji, try_parse_emoji};
use super::escapes::{EscapeType, emit_escape, try_parse_escape};
use super::inline_executable::{emit_inline_executable, try_parse_inline_executable};
use super::inline_footnotes::{
    emit_footnote_reference, emit_inline_footnote, try_parse_footnote_reference,
    try_parse_inline_footnote,
};
use super::inline_html::{emit_inline_html, try_parse_inline_html};
use super::latex::{parse_latex_command, try_parse_latex_command};
use super::links::{
    LinkScanContext, emit_autolink, emit_bare_uri_link, emit_inline_image, emit_inline_link,
    emit_reference_image, emit_reference_link, emit_unresolved_reference, try_parse_autolink,
    try_parse_bare_uri, try_parse_inline_image, try_parse_inline_link, try_parse_reference_image,
    try_parse_reference_link,
};
use super::mark::{emit_mark, try_parse_mark};
use super::math::{
    emit_display_math, emit_display_math_environment, emit_double_backslash_display_math,
    emit_double_backslash_inline_math, emit_gfm_inline_math, emit_inline_math,
    emit_single_backslash_display_math, emit_single_backslash_inline_math, try_parse_display_math,
    try_parse_double_backslash_display_math, try_parse_double_backslash_inline_math,
    try_parse_gfm_inline_math, try_parse_inline_math, try_parse_math_environment,
    try_parse_single_backslash_display_math, try_parse_single_backslash_inline_math,
};
use super::native_spans::{emit_native_span, try_parse_native_span};
use super::raw_inline::is_raw_inline;
use super::shortcodes::{emit_shortcode, try_parse_shortcode};
use super::strikeout::{emit_strikeout, try_parse_strikeout};
use super::subscript::{emit_subscript, try_parse_subscript};
use super::superscript::{emit_superscript, try_parse_superscript};

/// Parse inline text into the CST builder.
///
/// Top-level entry point for inline parsing. Builds the IR plans
/// (emphasis pairings, bracket resolutions, standalone Pandoc constructs)
/// once via [`super::inline_ir::build_full_plans`], then walks the byte
/// range left-to-right consulting those plans plus the dispatcher's
/// ordered-try chain for non-IR-resolved constructs (autolinks, code
/// spans, escapes, math, etc.). Dialect-specific behavior is selected
/// inside `build_full_plans`.
///
/// # Arguments
/// * `text` - The inline text to parse
/// * `config` - Configuration for extensions and formatting
/// * `builder` - The CST builder to emit nodes to
pub fn parse_inline_text_recursive(
    builder: &mut GreenNodeBuilder,
    text: &str,
    config: &ParserOptions,
) {
    log::trace!(
        "Recursive inline parsing: {:?} ({} bytes)",
        &text[..text.len().min(40)],
        text.len()
    );

    let mask = structural_byte_mask(config);
    if try_emit_plain_text_fast_path_with_mask(builder, text, &mask) {
        log::trace!("Recursive inline parsing complete (plain-text fast path)");
        return;
    }

    let plans = super::inline_ir::build_full_plans(text, 0, text.len(), config);
    parse_inline_range_impl(
        text,
        0,
        text.len(),
        config,
        builder,
        false,
        &plans.emphasis,
        &plans.brackets,
        &plans.constructs,
        false,
        &mask,
    );

    log::trace!("Recursive inline parsing complete");
}

/// Parse inline elements from text content nested inside a link/image/span.
///
/// Used for recursive inline parsing of link text, image alt, span content, etc.
/// Suppresses constructs that would create nested links (CommonMark §6.3 forbids
/// links inside links), notably extended bare-URI autolinks under GFM.
///
/// `suppress_inner_links` should be `true` when the recursion is for a
/// LINK or REFERENCE-LINK's text, where inner link / reference-link
/// brackets must emit as literal text (pandoc-native:
/// `[link [inner](u2)](u1)` → outer `Link` with `Str "[inner](u2)"`).
/// Image alt text and all non-link contexts pass `false`:
/// pandoc-native verifies `![alt with [inner](u)](u2)` keeps the inner
/// `Link`, and bracketed spans / native spans / inline footnotes /
/// emphasis all allow nested links.
pub fn parse_inline_text(
    builder: &mut GreenNodeBuilder,
    text: &str,
    config: &ParserOptions,
    suppress_inner_links: bool,
) {
    log::trace!(
        "Parsing inline text (nested in link): {:?} ({} bytes)",
        &text[..text.len().min(40)],
        text.len()
    );

    let mask = structural_byte_mask(config);
    if try_emit_plain_text_fast_path_with_mask(builder, text, &mask) {
        return;
    }

    let plans = super::inline_ir::build_full_plans(text, 0, text.len(), config);
    parse_inline_range_impl(
        text,
        0,
        text.len(),
        config,
        builder,
        true,
        &plans.emphasis,
        &plans.brackets,
        &plans.constructs,
        suppress_inner_links,
        &mask,
    );
}

/// Plain-text fast path for inline ranges with no structural bytes.
///
/// Returns `true` if the range was emitted as a single `TEXT` token and
/// the caller should skip the IR + dispatcher pipeline. Returns `false`
/// if any structural byte appears (or the range is empty), letting the
/// caller proceed normally. Empty input returns `false` so the caller's
/// existing "no events → no output" path is preserved exactly.
///
/// The structural byte set is computed from `config.dialect` and
/// `config.extensions` so prose containing dialect-irrelevant punctuation
/// (e.g. `-` outside a citation flavor) doesn't unnecessarily disable the
/// fast path. `\n` and `\r` are always structural — multi-line inline
/// content must still split into TEXT + NEWLINE tokens like the slow path.
fn try_emit_plain_text_fast_path_with_mask(
    builder: &mut GreenNodeBuilder,
    text: &str,
    mask: &[bool; 256],
) -> bool {
    if text.is_empty() {
        return false;
    }
    for &b in text.as_bytes() {
        if mask[b as usize] {
            return false;
        }
    }
    builder.token(SyntaxKind::TEXT.into(), text);
    true
}

/// Build a 256-entry byte mask: `mask[b]` is `true` iff byte `b` could
/// trigger any IR-recognised construct or dispatcher branch under the
/// current dialect/extensions. Used by the plain-text fast path to scan
/// inline ranges in a single pass.
fn structural_byte_mask(config: &ParserOptions) -> [bool; 256] {
    let mut mask = [false; 256];
    let exts = &config.extensions;
    let pandoc = config.dialect == Dialect::Pandoc;

    // Always structural: line breaks (CST splits TEXT/NEWLINE), backslash
    // (escape / hard break / backslash-math / latex / bookdown ref),
    // backtick (code span / inline executable), `*`/`_` (emphasis is a
    // core CommonMark construct, not extension-gated), and `[`/`]` if
    // any bracket-shaped construct is reachable.
    mask[b'\n' as usize] = true;
    mask[b'\r' as usize] = true;
    mask[b'\\' as usize] = true;
    mask[b'`' as usize] = true;
    mask[b'*' as usize] = true;
    mask[b'_' as usize] = true;

    // Brackets: the IR/dispatcher only acts on `[`/`]` if some
    // bracket-shaped feature is reachable. `!` is the leading byte of
    // `![alt]` image brackets — the IR's `BracketPlan` keys image
    // openers at the `!` position, so the dispatcher must stop here
    // to consult the plan.
    if exts.inline_links
        || exts.reference_links
        || exts.inline_images
        || exts.bracketed_spans
        || exts.footnotes
        || exts.citations
    {
        mask[b'[' as usize] = true;
        mask[b']' as usize] = true;
    }
    if exts.inline_images || exts.reference_links {
        mask[b'!' as usize] = true;
    }

    // `<` covers autolinks, raw HTML, and Pandoc native spans.
    if exts.autolinks || exts.raw_html || exts.native_spans {
        mask[b'<' as usize] = true;
    }

    // `^` covers Pandoc inline footnotes (`^[...]`), CM inline footnotes
    // (when explicitly enabled), and superscript (`^text^`).
    if exts.inline_footnotes || exts.superscript {
        mask[b'^' as usize] = true;
    }

    // `@` and `-` cover Pandoc citation forms (`@cite`, `-@cite`,
    // `[@cite]`). Under Pandoc dialect, the IR's `ConstructPlan` keys
    // bare citations at the `@` or `-` position, so the dispatcher
    // must stop at either to consult the plan. Including `-` is
    // pessimistic — most prose hyphens won't form `-@` — but missing
    // it would skip past valid suppress-author citations.
    if exts.citations || exts.quarto_crossrefs {
        mask[b'@' as usize] = true;
        if pandoc {
            mask[b'-' as usize] = true;
        }
    }

    // `$` covers dollar-math and GFM math.
    if exts.tex_math_dollars || exts.tex_math_gfm {
        mask[b'$' as usize] = true;
    }

    // `~` covers subscript and strikeout (both `~text~` and `~~text~~`).
    if exts.subscript || exts.strikeout {
        mask[b'~' as usize] = true;
    }

    if exts.mark {
        mask[b'=' as usize] = true;
    }
    if exts.emoji {
        mask[b':' as usize] = true;
    }
    if exts.bookdown_references {
        mask[b'(' as usize] = true;
    }
    // `{{< ... >}}` shortcodes: the dispatcher tries them on any
    // `{` regardless of the `quarto_shortcodes` extension flag, so
    // `{` must always be flagged here.
    mask[b'{' as usize] = true;

    // Bare-URI autolinks (`http://...` without `<>`) have no
    // leading-byte gate in the dispatcher — `try_parse_bare_uri`
    // probes for a URI scheme starting at every byte. Flag all
    // ASCII alphabetic bytes so the bulk-skip stops on every
    // potential scheme starter. This effectively disables the
    // bulk-skip benefit for prose under GFM-style flavors but
    // preserves correctness; ASCII digits / punctuation / non-ASCII
    // bytes still skip cleanly.
    if exts.autolink_bare_uris {
        for b in b'a'..=b'z' {
            mask[b as usize] = true;
        }
        for b in b'A'..=b'Z' {
            mask[b as usize] = true;
        }
    }

    mask
}

fn is_emoji_boundary(text: &str, pos: usize) -> bool {
    if pos > 0 {
        let prev = text.as_bytes()[pos - 1] as char;
        if prev.is_ascii_alphanumeric() || prev == '_' {
            return false;
        }
    }
    true
}

#[inline]
fn advance_char_boundary(text: &str, pos: usize, end: usize) -> usize {
    if pos >= end || pos >= text.len() {
        return pos;
    }
    let ch_len = text[pos..]
        .chars()
        .next()
        .map_or(1, std::primitive::char::len_utf8);
    (pos + ch_len).min(end)
}

#[allow(clippy::too_many_arguments)]
fn parse_inline_range_impl(
    text: &str,
    start: usize,
    end: usize,
    config: &ParserOptions,
    builder: &mut GreenNodeBuilder,
    nested_in_link: bool,
    plan: &EmphasisPlan,
    bracket_plan: &BracketPlan,
    construct_plan: &ConstructPlan,
    suppress_inner_links: bool,
    mask: &[bool; 256],
) {
    log::trace!(
        "parse_inline_range: start={}, end={}, text={:?}",
        start,
        end,
        &text[start..end]
    );
    let mut pos = start;
    let mut text_start = start;
    let bytes = text.as_bytes();

    while pos < end {
        // Bulk-skip plain bytes between structural bytes. Plans
        // (`construct_plan`, `bracket_plan`, emphasis `plan`) only
        // resolve at structural byte positions, so skipping here
        // never elides a real match. `text_start` is preserved
        // across the skip; the next emitted construct flushes the
        // accumulated TEXT span.
        if !mask[bytes[pos] as usize] {
            let mut next = pos + 1;
            while next < end && !mask[bytes[next] as usize] {
                next += 1;
            }
            pos = next;
            if pos >= end {
                break;
            }
        }
        // IR-driven dispatch: if the IR identified a Pandoc standalone
        // construct starting here, emit it directly. Bypasses the
        // dispatcher's ordered-try chain for inline footnotes, native
        // spans, footnote references, citations, and bracketed spans
        // under `Dialect::Pandoc`. The IR scan gates these on
        // `!is_commonmark` and the relevant extension flag, so this
        // branch is empty under CommonMark dialect (where the legacy
        // dispatcher branches still run when the extension is enabled).
        if let Some(dispo) = construct_plan.lookup(pos) {
            match *dispo {
                ConstructDispo::InlineFootnote { end: dispo_end } => {
                    if dispo_end <= end
                        && let Some((len, content)) = try_parse_inline_footnote(&text[pos..])
                        && pos + len == dispo_end
                    {
                        if pos > text_start {
                            builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
                        }
                        log::trace!("IR: matched inline footnote at pos {}", pos);
                        emit_inline_footnote(builder, content, config);
                        pos += len;
                        text_start = pos;
                        continue;
                    }
                }
                ConstructDispo::NativeSpan { end: dispo_end } => {
                    if dispo_end <= end
                        && let Some((len, content, attributes)) =
                            try_parse_native_span(&text[pos..])
                        && pos + len == dispo_end
                    {
                        if pos > text_start {
                            builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
                        }
                        log::trace!("IR: matched native span at pos {}", pos);
                        emit_native_span(builder, content, &attributes, config);
                        pos += len;
                        text_start = pos;
                        continue;
                    }
                }
                ConstructDispo::FootnoteReference { end: dispo_end } => {
                    if dispo_end <= end
                        && let Some((len, id)) = try_parse_footnote_reference(&text[pos..])
                        && pos + len == dispo_end
                    {
                        if pos > text_start {
                            builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
                        }
                        log::trace!("IR: matched footnote reference at pos {}", pos);
                        emit_footnote_reference(builder, &id);
                        pos += len;
                        text_start = pos;
                        continue;
                    }
                }
                ConstructDispo::BracketedCitation { end: dispo_end } => {
                    if dispo_end <= end
                        && let Some((len, content)) = try_parse_bracketed_citation(&text[pos..])
                        && pos + len == dispo_end
                    {
                        if pos > text_start {
                            builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
                        }
                        log::trace!("IR: matched bracketed citation at pos {}", pos);
                        emit_bracketed_citation(builder, content);
                        pos += len;
                        text_start = pos;
                        continue;
                    }
                }
                ConstructDispo::BareCitation { end: dispo_end } => {
                    if dispo_end <= end
                        && let Some((len, key, has_suppress)) =
                            try_parse_bare_citation(&text[pos..])
                        && pos + len == dispo_end
                    {
                        let is_crossref = config.extensions.quarto_crossrefs
                            && super::citations::is_quarto_crossref_key(key);
                        if is_crossref || config.extensions.citations {
                            if pos > text_start {
                                builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
                            }
                            if is_crossref {
                                log::trace!("IR: matched Quarto crossref at pos {}: {}", pos, key);
                                super::citations::emit_crossref(builder, key, has_suppress);
                            } else {
                                log::trace!("IR: matched bare citation at pos {}: {}", pos, key);
                                emit_bare_citation(builder, key, has_suppress);
                            }
                            pos += len;
                            text_start = pos;
                            continue;
                        }
                    }
                }
                ConstructDispo::BracketedSpan { end: dispo_end } => {
                    if dispo_end <= end
                        && let Some((len, content, attrs)) = try_parse_bracketed_span(&text[pos..])
                        && pos + len == dispo_end
                    {
                        if pos > text_start {
                            builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
                        }
                        log::trace!("IR: matched bracketed span at pos {}", pos);
                        emit_bracketed_span(builder, &content, &attrs, config);
                        pos += len;
                        text_start = pos;
                        continue;
                    }
                }
            }
        }

        // IR-driven bracket dispatch: if the IR's `process_brackets`
        // resolved a bracket pair starting at this position, emit it
        // directly via the appropriate helper. The
        // dispatcher's `try_parse_*` recognizers compute the actual
        // byte length and extract content / attributes; the IR's
        // `suffix_end` is used to constrain the dispatcher's match
        // shape so the two pipelines agree on which link variant
        // resolved (e.g. `[foo][bar]` with `bar` undefined and `foo`
        // defined: IR resolves `[foo]` as shortcut, but the
        // dispatcher's `try_parse_reference_link` would otherwise
        // greedily return the full-ref shape). Suppression of inner
        // LINK / REFERENCE LINK during LINK-text recursion is applied
        // here (pandoc-native: outer-wins for nested links).
        //
        // Pandoc-extended `{.attrs}` after a link can extend the
        // dispatcher's match length past the IR's `suffix_end`. The
        // dispatcher's len is therefore constrained to
        // `[suffix_end, end]` rather than required to equal
        // `suffix_end` exactly.
        // IR-driven dispatch: Pandoc unresolved bracket-shape pattern.
        // Before emitting the `UNRESOLVED_REFERENCE` wrapper, give the
        // dispatcher's lenient inline-link / inline-image parsers a
        // chance to override. The IR's `try_inline_suffix` is stricter
        // than pandoc-markdown for some destination shapes (URLs with
        // spaces, titles with embedded quotes, shortcode-style braces);
        // the dispatcher accepts those and produces a real LINK / IMAGE
        // node — pandoc-native agrees. Without this override, valid
        // pandoc links would degrade to `UNRESOLVED_REFERENCE` here.
        if let Some(super::inline_ir::BracketDispo::UnresolvedReference {
            is_image,
            text_start: ref_text_start,
            text_end: ref_text_end,
            end: ref_end,
        }) = bracket_plan.lookup(pos)
        {
            let is_image = *is_image;
            let dispo_suffix_end = *ref_end;
            let suppress = suppress_inner_links && !is_image;
            if !suppress {
                let ctx = LinkScanContext::from_options(config);
                let is_commonmark = config.dialect == Dialect::CommonMark;
                if is_image {
                    if config.extensions.inline_images
                        && let Some((len, alt_text, dest, attributes)) =
                            try_parse_inline_image(&text[pos..], ctx)
                        && pos + len >= dispo_suffix_end
                        && pos + len <= end
                    {
                        if pos > text_start {
                            builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
                        }
                        log::trace!(
                            "IR: dispatcher overrode UnresolvedReference with inline image at pos {}",
                            pos
                        );
                        emit_inline_image(
                            builder,
                            &text[pos..pos + len],
                            alt_text,
                            dest,
                            attributes,
                            config,
                        );
                        pos += len;
                        text_start = pos;
                        continue;
                    }
                } else if config.extensions.inline_links
                    && let Some((len, link_text, dest, attributes)) =
                        try_parse_inline_link(&text[pos..], is_commonmark, ctx)
                    && pos + len >= dispo_suffix_end
                    && pos + len <= end
                {
                    if pos > text_start {
                        builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
                    }
                    log::trace!(
                        "IR: dispatcher overrode UnresolvedReference with inline link at pos {}",
                        pos
                    );
                    emit_inline_link(
                        builder,
                        &text[pos..pos + len],
                        link_text,
                        dest,
                        attributes,
                        config,
                    );
                    pos += len;
                    text_start = pos;
                    continue;
                }
            }

            // Dispatcher didn't override; emit the wrapper.
            let inner_text = &text[*ref_text_start..*ref_text_end];
            let suffix_start = *ref_text_end + 1;
            let label_suffix = if suffix_start < *ref_end {
                Some(&text[suffix_start..*ref_end])
            } else {
                None
            };
            if pos > text_start {
                builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
            }
            log::trace!(
                "IR: unresolved Pandoc reference shape at pos {}..{}",
                pos,
                ref_end
            );
            emit_unresolved_reference(builder, is_image, inner_text, label_suffix, config);
            pos = *ref_end;
            text_start = pos;
            continue;
        }

        if let Some(super::inline_ir::BracketDispo::Open {
            is_image,
            suffix_end,
            ..
        }) = bracket_plan.lookup(pos)
        {
            let is_image = *is_image;
            let dispo_suffix_end = *suffix_end;
            let suppress = suppress_inner_links && !is_image;
            if !suppress {
                let ctx = LinkScanContext::from_options(config);
                let allow_shortcut = config.extensions.shortcut_reference_links;
                let is_commonmark = config.dialect == Dialect::CommonMark;
                if is_image {
                    if config.extensions.inline_images
                        && let Some((len, alt_text, dest, attributes)) =
                            try_parse_inline_image(&text[pos..], ctx)
                        && pos + len >= dispo_suffix_end
                        && pos + len <= end
                    {
                        if pos > text_start {
                            builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
                        }
                        log::trace!("IR: matched inline image at pos {}", pos);
                        emit_inline_image(
                            builder,
                            &text[pos..pos + len],
                            alt_text,
                            dest,
                            attributes,
                            config,
                        );
                        pos += len;
                        text_start = pos;
                        continue;
                    }
                    if config.extensions.reference_links
                        && let Some((len, alt_text, reference, is_shortcut)) =
                            try_parse_reference_image(&text[pos..], allow_shortcut)
                        && pos + len == dispo_suffix_end
                        && pos + len <= end
                    {
                        if pos > text_start {
                            builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
                        }
                        log::trace!("IR: matched reference image at pos {}", pos);
                        emit_reference_image(builder, alt_text, &reference, is_shortcut, config);
                        pos += len;
                        text_start = pos;
                        continue;
                    }
                } else {
                    if config.extensions.inline_links
                        && let Some((len, link_text, dest, attributes)) =
                            try_parse_inline_link(&text[pos..], is_commonmark, ctx)
                        && pos + len >= dispo_suffix_end
                        && pos + len <= end
                    {
                        if pos > text_start {
                            builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
                        }
                        log::trace!("IR: matched inline link at pos {}", pos);
                        emit_inline_link(
                            builder,
                            &text[pos..pos + len],
                            link_text,
                            dest,
                            attributes,
                            config,
                        );
                        pos += len;
                        text_start = pos;
                        continue;
                    }
                    if config.extensions.reference_links
                        && let Some((len, link_text, reference, is_shortcut)) =
                            try_parse_reference_link(
                                &text[pos..],
                                allow_shortcut,
                                config.extensions.inline_links,
                                ctx,
                            )
                        && pos + len == dispo_suffix_end
                        && pos + len <= end
                    {
                        if pos > text_start {
                            builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
                        }
                        log::trace!("IR: matched reference link at pos {}", pos);
                        emit_reference_link(builder, link_text, &reference, is_shortcut, config);
                        pos += len;
                        text_start = pos;
                        continue;
                    }
                }
            }
        }

        let byte = text.as_bytes()[pos];

        // Backslash math (highest priority if enabled)
        if byte == b'\\' {
            // Try double backslash display math first: \\[...\\]
            if config.extensions.tex_math_double_backslash {
                if let Some((len, content)) = try_parse_double_backslash_display_math(&text[pos..])
                {
                    if pos > text_start {
                        builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
                    }
                    log::trace!("Matched double backslash display math at pos {}", pos);
                    emit_double_backslash_display_math(builder, content);
                    pos += len;
                    text_start = pos;
                    continue;
                }

                // Try double backslash inline math: \\(...\\)
                if let Some((len, content)) = try_parse_double_backslash_inline_math(&text[pos..]) {
                    if pos > text_start {
                        builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
                    }
                    log::trace!("Matched double backslash inline math at pos {}", pos);
                    emit_double_backslash_inline_math(builder, content);
                    pos += len;
                    text_start = pos;
                    continue;
                }
            }

            // Try single backslash display math: \[...\]
            if config.extensions.tex_math_single_backslash {
                if let Some((len, content)) = try_parse_single_backslash_display_math(&text[pos..])
                {
                    if pos > text_start {
                        builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
                    }
                    log::trace!("Matched single backslash display math at pos {}", pos);
                    emit_single_backslash_display_math(builder, content);
                    pos += len;
                    text_start = pos;
                    continue;
                }

                // Try single backslash inline math: \(...\)
                if let Some((len, content)) = try_parse_single_backslash_inline_math(&text[pos..]) {
                    if pos > text_start {
                        builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
                    }
                    log::trace!("Matched single backslash inline math at pos {}", pos);
                    emit_single_backslash_inline_math(builder, content);
                    pos += len;
                    text_start = pos;
                    continue;
                }
            }

            // Try math environments \begin{equation}...\end{equation}
            if config.extensions.raw_tex
                && let Some((len, begin_marker, content, end_marker)) =
                    try_parse_math_environment(&text[pos..])
            {
                if pos > text_start {
                    builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
                }
                log::trace!("Matched math environment at pos {}", pos);
                emit_display_math_environment(builder, begin_marker, content, end_marker);
                pos += len;
                text_start = pos;
                continue;
            }

            // Try bookdown reference: \@ref(label)
            if config.extensions.bookdown_references
                && let Some((len, label)) = try_parse_bookdown_reference(&text[pos..])
            {
                if pos > text_start {
                    builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
                }
                log::trace!("Matched bookdown reference at pos {}: {}", pos, label);
                super::citations::emit_bookdown_crossref(builder, label);
                pos += len;
                text_start = pos;
                continue;
            }

            // Try escapes (after bookdown refs and backslash math)
            if let Some((len, ch, escape_type)) = try_parse_escape(&text[pos..]) {
                let escape_enabled = match escape_type {
                    EscapeType::HardLineBreak => config.extensions.escaped_line_breaks,
                    EscapeType::NonbreakingSpace => config.extensions.all_symbols_escapable,
                    EscapeType::Literal => {
                        // BASE_ESCAPABLE matches Pandoc's markdown_strict /
                        // original Markdown set, plus `|` and `~` which the
                        // formatter emits as escapes for pipe-table separators
                        // and strikethrough delimiters. Recognising those here
                        // keeps round-trips idempotent in flavors that don't
                        // enable all_symbols_escapable.
                        //
                        // Under CommonMark dialect, the spec (§2.4) explicitly
                        // allows ANY ASCII punctuation to be backslash-escaped,
                        // independent of the all_symbols_escapable extension
                        // (which also widens to whitespace, a Pandoc-only
                        // construct).
                        const BASE_ESCAPABLE: &str = "\\`*_{}[]()>#+-.!|~";
                        BASE_ESCAPABLE.contains(ch)
                            || config.extensions.all_symbols_escapable
                            || (config.dialect == crate::Dialect::CommonMark
                                && ch.is_ascii_punctuation())
                    }
                };
                if !escape_enabled {
                    // Don't treat as hard line break - skip the escape and continue
                    // The backslash will be included in the next TEXT token
                    pos = advance_char_boundary(text, pos, end);
                    continue;
                }

                // Emit accumulated text
                if pos > text_start {
                    builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
                }

                log::trace!("Matched escape at pos {}: \\{}", pos, ch);
                emit_escape(builder, ch, escape_type);
                pos += len;
                text_start = pos;
                continue;
            }

            // Try LaTeX commands (after escapes, before shortcodes)
            if config.extensions.raw_tex
                && let Some(len) = try_parse_latex_command(&text[pos..])
            {
                if pos > text_start {
                    builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
                }
                log::trace!("Matched LaTeX command at pos {}", pos);
                parse_latex_command(builder, &text[pos..], len);
                pos += len;
                text_start = pos;
                continue;
            }
        }

        // Try Quarto shortcodes: {{< shortcode >}}
        if byte == b'{'
            && pos + 1 < text.len()
            && text.as_bytes()[pos + 1] == b'{'
            && let Some((len, name, attrs)) = try_parse_shortcode(&text[pos..])
        {
            if pos > text_start {
                builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
            }
            log::trace!("Matched shortcode at pos {}: {}", pos, &name);
            emit_shortcode(builder, &name, attrs);
            pos += len;
            text_start = pos;
            continue;
        }

        // Try inline executable code spans (`... `r expr`` and `... `{r} expr``)
        if byte == b'`'
            && let Some(m) = try_parse_inline_executable(
                &text[pos..],
                config.extensions.rmarkdown_inline_code,
                config.extensions.quarto_inline_code,
            )
        {
            if pos > text_start {
                builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
            }
            log::trace!("Matched inline executable code at pos {}", pos);
            emit_inline_executable(builder, &m);
            pos += m.total_len;
            text_start = pos;
            continue;
        }

        // Try code spans
        if byte == b'`' {
            if let Some((len, content, backtick_count, attributes)) =
                try_parse_code_span(&text[pos..])
            {
                // Emit accumulated text
                if pos > text_start {
                    builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
                }

                log::trace!(
                    "Matched code span at pos {}: {} backticks",
                    pos,
                    backtick_count
                );

                // Check for raw inline
                if let Some(ref attrs) = attributes
                    && config.extensions.raw_attribute
                    && let Some(format) = is_raw_inline(attrs)
                {
                    use super::raw_inline::emit_raw_inline;
                    log::trace!("Matched raw inline span at pos {}: format={}", pos, format);
                    emit_raw_inline(builder, content, backtick_count, format);
                } else if !config.extensions.inline_code_attributes && attributes.is_some() {
                    let code_span_len = backtick_count * 2 + content.len();
                    emit_code_span(builder, content, backtick_count, None);
                    pos += code_span_len;
                    text_start = pos;
                    continue;
                } else {
                    emit_code_span(builder, content, backtick_count, attributes);
                }

                pos += len;
                text_start = pos;
                continue;
            }

            // Unmatched backtick run.
            //
            // CommonMark (and GFM) treat the whole run as literal text — the
            // run cannot be re-entered as a shorter opener. Pandoc-markdown
            // instead lets a longer run shadow a shorter one (e.g.
            // `` ```foo`` `` parses as `` ` `` + ``<code>foo</code>``), so
            // for the Pandoc dialect we fall through and advance one byte at
            // a time, allowing the inner run to be tried on a later iteration.
            if config.dialect == Dialect::CommonMark {
                let run_len = text[pos..].bytes().take_while(|&b| b == b'`').count();
                pos += run_len;
                continue;
            }
        }

        // Try textual emoji aliases: :smile:
        if byte == b':'
            && config.extensions.emoji
            && is_emoji_boundary(text, pos)
            && let Some((len, _alias)) = try_parse_emoji(&text[pos..])
        {
            if pos > text_start {
                builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
            }
            log::trace!("Matched emoji at pos {}", pos);
            emit_emoji(builder, &text[pos..pos + len]);
            pos += len;
            text_start = pos;
            continue;
        }

        // Try inline footnotes: ^[note]. Under Pandoc dialect this is
        // consumed via the IR's `ConstructPlan` at the top of the loop;
        // this dispatcher branch only fires for CommonMark dialect with
        // the extension explicitly enabled.
        if byte == b'^'
            && pos + 1 < text.len()
            && text.as_bytes()[pos + 1] == b'['
            && config.dialect == Dialect::CommonMark
            && config.extensions.inline_footnotes
            && let Some((len, content)) = try_parse_inline_footnote(&text[pos..])
        {
            if pos > text_start {
                builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
            }
            log::trace!("Matched inline footnote at pos {}", pos);
            emit_inline_footnote(builder, content, config);
            pos += len;
            text_start = pos;
            continue;
        }

        // Try superscript: ^text^
        if byte == b'^'
            && config.extensions.superscript
            && let Some((len, content)) = try_parse_superscript(&text[pos..])
        {
            if pos > text_start {
                builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
            }
            log::trace!("Matched superscript at pos {}", pos);
            emit_superscript(builder, content, config);
            pos += len;
            text_start = pos;
            continue;
        }

        // Try bookdown definition: (\#label) or (ref:label)
        if byte == b'(' && config.extensions.bookdown_references {
            if let Some((len, label)) = try_parse_bookdown_definition(&text[pos..]) {
                if pos > text_start {
                    builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
                }
                log::trace!("Matched bookdown definition at pos {}: {}", pos, label);
                builder.token(SyntaxKind::TEXT.into(), &text[pos..pos + len]);
                pos += len;
                text_start = pos;
                continue;
            }
            if let Some((len, label)) = try_parse_bookdown_text_reference(&text[pos..]) {
                if pos > text_start {
                    builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
                }
                log::trace!("Matched bookdown text reference at pos {}: {}", pos, label);
                builder.token(SyntaxKind::TEXT.into(), &text[pos..pos + len]);
                pos += len;
                text_start = pos;
                continue;
            }
        }

        // Try strikeout: ~~text~~
        // Must run before subscript so `~~text~~` is matched as a single
        // Strikeout rather than two empty Subscripts. Subscript falls back
        // to consuming `~~` as an empty subscript only when strikeout
        // didn't match (e.g. `~~unclosed`).
        if byte == b'~'
            && config.extensions.strikeout
            && let Some((len, content)) = try_parse_strikeout(&text[pos..])
        {
            if pos > text_start {
                builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
            }
            log::trace!("Matched strikeout at pos {}", pos);
            emit_strikeout(builder, content, config);
            pos += len;
            text_start = pos;
            continue;
        }

        // Try subscript: ~text~ or `~~` as empty subscript when strikeout
        // didn't match (matches pandoc: `~~unclosed` → `Subscript [] + text`).
        if byte == b'~'
            && config.extensions.subscript
            && let Some((len, content)) = try_parse_subscript(&text[pos..])
        {
            if pos > text_start {
                builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
            }
            log::trace!("Matched subscript at pos {}", pos);
            emit_subscript(builder, content, config);
            pos += len;
            text_start = pos;
            continue;
        }

        // Try mark/highlight: ==text==
        if byte == b'='
            && config.extensions.mark
            && let Some((len, content)) = try_parse_mark(&text[pos..])
        {
            if pos > text_start {
                builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
            }
            log::trace!("Matched mark at pos {}", pos);
            emit_mark(builder, content, config);
            pos += len;
            text_start = pos;
            continue;
        }

        // Try GFM inline math: $`...`$
        if byte == b'$'
            && config.extensions.tex_math_gfm
            && let Some((len, content)) = try_parse_gfm_inline_math(&text[pos..])
        {
            if pos > text_start {
                builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
            }
            log::trace!("Matched GFM inline math at pos {}", pos);
            emit_gfm_inline_math(builder, content);
            pos += len;
            text_start = pos;
            continue;
        }

        // Try math ($...$, $$...$$)
        if byte == b'$' && config.extensions.tex_math_dollars {
            // Try display math first ($$...$$)
            if let Some((len, content)) = try_parse_display_math(&text[pos..]) {
                // Emit accumulated text
                if pos > text_start {
                    builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
                }

                let dollar_count = text[pos..].chars().take_while(|&c| c == '$').count();
                log::trace!(
                    "Matched display math at pos {}: {} dollars",
                    pos,
                    dollar_count
                );

                // Check for trailing attributes (Quarto cross-reference support)
                let after_math = &text[pos + len..];
                let attr_len = if config.extensions.quarto_crossrefs {
                    use crate::parser::utils::attributes::try_parse_trailing_attributes;
                    if let Some((_attr_block, _)) = try_parse_trailing_attributes(after_math) {
                        let trimmed_after = after_math.trim_start();
                        if let Some(open_brace_pos) = trimmed_after.find('{') {
                            let ws_before_brace = after_math.len() - trimmed_after.len();
                            let attr_text_len = trimmed_after[open_brace_pos..]
                                .find('}')
                                .map(|close| close + 1)
                                .unwrap_or(0);
                            ws_before_brace + open_brace_pos + attr_text_len
                        } else {
                            0
                        }
                    } else {
                        0
                    }
                } else {
                    0
                };

                let total_len = len + attr_len;
                emit_display_math(builder, content, dollar_count);

                // Emit attributes if present
                if attr_len > 0 {
                    use crate::parser::utils::attributes::{
                        emit_attributes, try_parse_trailing_attributes,
                    };
                    let attr_text = &text[pos + len..pos + total_len];
                    if let Some((attr_block, _text_before)) =
                        try_parse_trailing_attributes(attr_text)
                    {
                        let trimmed_after = attr_text.trim_start();
                        let ws_len = attr_text.len() - trimmed_after.len();
                        if ws_len > 0 {
                            builder.token(SyntaxKind::WHITESPACE.into(), &attr_text[..ws_len]);
                        }
                        emit_attributes(builder, &attr_block);
                    }
                }

                pos += total_len;
                text_start = pos;
                continue;
            }

            // Try inline math ($...$)
            if let Some((len, content)) = try_parse_inline_math(&text[pos..]) {
                // Emit accumulated text
                if pos > text_start {
                    builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
                }

                log::trace!("Matched inline math at pos {}", pos);
                emit_inline_math(builder, content);
                pos += len;
                text_start = pos;
                continue;
            }

            // Neither display nor inline math matched - emit the $ as literal text
            // This ensures each $ gets its own TEXT token for CST compatibility
            if pos > text_start {
                builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
            }
            builder.token(SyntaxKind::TEXT.into(), "$");
            pos = advance_char_boundary(text, pos, end);
            text_start = pos;
            continue;
        }

        // Try autolinks: <url> or <email>
        if byte == b'<'
            && config.extensions.autolinks
            && let Some((len, url)) = try_parse_autolink(
                &text[pos..],
                config.dialect == crate::options::Dialect::CommonMark,
            )
        {
            if pos > text_start {
                builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
            }
            log::trace!("Matched autolink at pos {}", pos);
            emit_autolink(builder, &text[pos..pos + len], url);
            pos += len;
            text_start = pos;
            continue;
        }

        if !nested_in_link
            && config.extensions.autolink_bare_uris
            && let Some((len, url)) = try_parse_bare_uri(&text[pos..])
        {
            if pos > text_start {
                builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
            }
            log::trace!("Matched bare URI at pos {}", pos);
            emit_bare_uri_link(builder, url, config);
            pos += len;
            text_start = pos;
            continue;
        }

        // Try native spans: <span>text</span> (after autolink since both
        // start with <). Under Pandoc dialect this is consumed via the
        // IR's `ConstructPlan` at the top of the loop; this dispatcher
        // branch only fires for CommonMark dialect with the extension
        // explicitly enabled.
        if byte == b'<'
            && config.dialect == Dialect::CommonMark
            && config.extensions.native_spans
            && let Some((len, content, attributes)) = try_parse_native_span(&text[pos..])
        {
            if pos > text_start {
                builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
            }
            log::trace!("Matched native span at pos {}", pos);
            emit_native_span(builder, content, &attributes, config);
            pos += len;
            text_start = pos;
            continue;
        }

        // Try inline raw HTML (CommonMark §6.6 / Pandoc raw_html). Must run
        // after autolinks (more specific) and native spans (Pandoc
        // <span>…</span> wrapper) since all three start with `<`.
        if byte == b'<'
            && config.extensions.raw_html
            && let Some(len) = try_parse_inline_html(&text[pos..])
        {
            if pos > text_start {
                builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
            }
            log::trace!("Matched inline raw HTML at pos {}", pos);
            emit_inline_html(builder, &text[pos..pos + len]);
            pos += len;
            text_start = pos;
            continue;
        }

        // Bracket-starting elements: inline / reference links and
        // images are dispatched via the IR-driven arm at the top of
        // the loop, gated by the IR's `BracketPlan`. Only dialect-CM-
        // specific Pandoc-extension constructs that share the `[...]`
        // shape (footnote refs, bracketed citations) need a CM-gated
        // dispatcher branch — under Pandoc dialect they're consumed
        // via the IR's `ConstructPlan` instead.
        if byte == b'['
            && config.dialect == Dialect::CommonMark
            && config.extensions.footnotes
            && let Some((len, id)) = try_parse_footnote_reference(&text[pos..])
        {
            if pos > text_start {
                builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
            }
            log::trace!("Matched footnote reference at pos {}", pos);
            emit_footnote_reference(builder, &id);
            pos += len;
            text_start = pos;
            continue;
        }
        if byte == b'['
            && config.dialect == Dialect::CommonMark
            && config.extensions.citations
            && let Some((len, content)) = try_parse_bracketed_citation(&text[pos..])
        {
            if pos > text_start {
                builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
            }
            log::trace!("Matched bracketed citation at pos {}", pos);
            emit_bracketed_citation(builder, content);
            pos += len;
            text_start = pos;
            continue;
        }

        // Try bracketed spans: [text]{.class}. Must come after
        // links/citations. Under Pandoc dialect this is consumed via
        // the IR's `ConstructPlan` at the top of the loop; this
        // dispatcher branch only fires for CommonMark dialect with the
        // extension explicitly enabled.
        if config.dialect == Dialect::CommonMark
            && byte == b'['
            && config.extensions.bracketed_spans
            && let Some((len, text_content, attrs)) = try_parse_bracketed_span(&text[pos..])
        {
            if pos > text_start {
                builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
            }
            log::trace!("Matched bracketed span at pos {}", pos);
            emit_bracketed_span(builder, &text_content, &attrs, config);
            pos += len;
            text_start = pos;
            continue;
        }

        // Try bare citation: @cite (must come after bracketed elements).
        // Under Pandoc dialect this is consumed via the IR's
        // `ConstructPlan` at the top of the loop; this dispatcher branch
        // only fires for CommonMark dialect with the extension
        // explicitly enabled.
        if config.dialect == Dialect::CommonMark
            && byte == b'@'
            && (config.extensions.citations || config.extensions.quarto_crossrefs)
            && let Some((len, key, has_suppress)) = try_parse_bare_citation(&text[pos..])
        {
            let is_crossref =
                config.extensions.quarto_crossrefs && super::citations::is_quarto_crossref_key(key);
            if is_crossref || config.extensions.citations {
                if pos > text_start {
                    builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
                }
                if is_crossref {
                    log::trace!("Matched Quarto crossref at pos {}: {}", pos, &key);
                    super::citations::emit_crossref(builder, key, has_suppress);
                } else {
                    log::trace!("Matched bare citation at pos {}: {}", pos, &key);
                    emit_bare_citation(builder, key, has_suppress);
                }
                pos += len;
                text_start = pos;
                continue;
            }
        }

        // Try suppress-author citation: -@cite. Under Pandoc dialect
        // this is consumed via the IR's `ConstructPlan` at the top of
        // the loop; this dispatcher branch only fires for CommonMark
        // dialect with the extension explicitly enabled.
        if config.dialect == Dialect::CommonMark
            && byte == b'-'
            && pos + 1 < text.len()
            && text.as_bytes()[pos + 1] == b'@'
            && (config.extensions.citations || config.extensions.quarto_crossrefs)
            && let Some((len, key, has_suppress)) = try_parse_bare_citation(&text[pos..])
        {
            let is_crossref =
                config.extensions.quarto_crossrefs && super::citations::is_quarto_crossref_key(key);
            if is_crossref || config.extensions.citations {
                if pos > text_start {
                    builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
                }
                if is_crossref {
                    log::trace!("Matched Quarto crossref at pos {}: {}", pos, &key);
                    super::citations::emit_crossref(builder, key, has_suppress);
                } else {
                    log::trace!("Matched suppress-author citation at pos {}: {}", pos, &key);
                    emit_bare_citation(builder, key, has_suppress);
                }
                pos += len;
                text_start = pos;
                continue;
            }
        }

        // Emphasis emission, plan-driven. The IR's emphasis pass has
        // already decided every delimiter byte's disposition (open
        // marker, close marker, or unmatched literal); consult the
        // plan here instead of re-scanning.
        if byte == b'*' || byte == b'_' {
            match plan.lookup(pos) {
                Some(DelimChar::Open {
                    len,
                    partner,
                    partner_len,
                    kind,
                }) => {
                    if pos > text_start {
                        builder.token(SyntaxKind::TEXT.into(), &text[text_start..pos]);
                    }
                    let len = len as usize;
                    let partner_len = partner_len as usize;
                    let (wrapper_kind, marker_kind) = match kind {
                        EmphasisKind::Strong => (SyntaxKind::STRONG, SyntaxKind::STRONG_MARKER),
                        EmphasisKind::Emph => (SyntaxKind::EMPHASIS, SyntaxKind::EMPHASIS_MARKER),
                    };
                    builder.start_node(wrapper_kind.into());
                    builder.token(marker_kind.into(), &text[pos..pos + len]);
                    parse_inline_range_impl(
                        text,
                        pos + len,
                        partner,
                        config,
                        builder,
                        nested_in_link,
                        plan,
                        bracket_plan,
                        construct_plan,
                        suppress_inner_links,
                        mask,
                    );
                    builder.token(marker_kind.into(), &text[partner..partner + partner_len]);
                    builder.finish_node();
                    pos = partner + partner_len;
                    text_start = pos;
                    continue;
                }
                Some(DelimChar::Close) => {
                    // Defensive: a close should be jumped past by its
                    // matching open. If we hit one anyway (e.g. when the
                    // outer caller's range starts mid-pair), let it be
                    // emitted as part of the surrounding text by simply
                    // advancing. text_start stays put so the byte folds
                    // into the next TEXT flush.
                    pos += 1;
                    continue;
                }
                Some(DelimChar::Literal) | None => {
                    // Unmatched delim chars at this position behave as
                    // literal text. Don't emit yet — let them coalesce
                    // with surrounding plain bytes via the existing
                    // text_start flushing so the CST keeps the same TEXT
                    // token granularity Pandoc fixtures expect.
                    let bytes = text.as_bytes();
                    let mut end_pos = pos + 1;
                    while end_pos < end && bytes[end_pos] == byte {
                        match plan.lookup(end_pos) {
                            Some(DelimChar::Literal) | None => end_pos += 1,
                            _ => break,
                        }
                    }
                    pos = end_pos;
                    continue;
                }
            }
        }

        // Check for newlines - may need to emit as hard line break
        if byte == b'\r' && pos + 1 < end && text.as_bytes()[pos + 1] == b'\n' {
            let text_before = &text[text_start..pos];

            // Check for trailing spaces hard line break (always enabled in Pandoc)
            let trailing_spaces = text_before.chars().rev().take_while(|&c| c == ' ').count();
            if trailing_spaces >= 2 {
                // Emit text before the trailing spaces
                let text_content = &text_before[..text_before.len() - trailing_spaces];
                if !text_content.is_empty() {
                    builder.token(SyntaxKind::TEXT.into(), text_content);
                }
                let spaces = " ".repeat(trailing_spaces);
                builder.token(
                    SyntaxKind::HARD_LINE_BREAK.into(),
                    &format!("{}\r\n", spaces),
                );
                pos += 2;
                text_start = pos;
                continue;
            }

            // hard_line_breaks: treat all single newlines as hard line breaks
            if config.extensions.hard_line_breaks {
                if !text_before.is_empty() {
                    builder.token(SyntaxKind::TEXT.into(), text_before);
                }
                builder.token(SyntaxKind::HARD_LINE_BREAK.into(), "\r\n");
                pos += 2;
                text_start = pos;
                continue;
            }

            // Regular newline
            if !text_before.is_empty() {
                builder.token(SyntaxKind::TEXT.into(), text_before);
            }
            builder.token(SyntaxKind::NEWLINE.into(), "\r\n");
            pos += 2;
            text_start = pos;
            continue;
        }

        if byte == b'\n' {
            let text_before = &text[text_start..pos];

            // Check for trailing spaces hard line break (always enabled in Pandoc)
            let trailing_spaces = text_before.chars().rev().take_while(|&c| c == ' ').count();
            if trailing_spaces >= 2 {
                // Emit text before the trailing spaces
                let text_content = &text_before[..text_before.len() - trailing_spaces];
                if !text_content.is_empty() {
                    builder.token(SyntaxKind::TEXT.into(), text_content);
                }
                let spaces = " ".repeat(trailing_spaces);
                builder.token(SyntaxKind::HARD_LINE_BREAK.into(), &format!("{}\n", spaces));
                pos += 1;
                text_start = pos;
                continue;
            }

            // hard_line_breaks: treat all single newlines as hard line breaks
            if config.extensions.hard_line_breaks {
                if !text_before.is_empty() {
                    builder.token(SyntaxKind::TEXT.into(), text_before);
                }
                builder.token(SyntaxKind::HARD_LINE_BREAK.into(), "\n");
                pos += 1;
                text_start = pos;
                continue;
            }

            // Regular newline
            if !text_before.is_empty() {
                builder.token(SyntaxKind::TEXT.into(), text_before);
            }
            builder.token(SyntaxKind::NEWLINE.into(), "\n");
            pos += 1;
            text_start = pos;
            continue;
        }

        // Regular character, keep accumulating
        pos = advance_char_boundary(text, pos, end);
    }

    // Emit any remaining text
    if pos > text_start && text_start < end {
        log::trace!("Emitting remaining TEXT: {:?}", &text[text_start..end]);
        builder.token(SyntaxKind::TEXT.into(), &text[text_start..end]);
    }

    log::trace!("parse_inline_range complete: start={}, end={}", start, end);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::{SyntaxKind, SyntaxNode};
    use rowan::GreenNode;

    #[test]
    fn test_recursive_simple_emphasis() {
        let text = "*test*";
        let config = ParserOptions::default();
        let mut builder = GreenNodeBuilder::new();

        parse_inline_text_recursive(&mut builder, text, &config);

        let green: GreenNode = builder.finish();
        let node = SyntaxNode::new_root(green);

        // Should be lossless
        assert_eq!(node.text().to_string(), text);

        // Should have EMPHASIS node
        let has_emph = node.descendants().any(|n| n.kind() == SyntaxKind::EMPHASIS);
        assert!(has_emph, "Should have EMPHASIS node");
    }

    #[test]
    fn test_recursive_nested() {
        let text = "*foo **bar** baz*";
        let config = ParserOptions::default();
        let mut builder = GreenNodeBuilder::new();

        // Wrap in a PARAGRAPH node (inline content needs a parent)
        builder.start_node(SyntaxKind::PARAGRAPH.into());
        parse_inline_text_recursive(&mut builder, text, &config);
        builder.finish_node();

        let green: GreenNode = builder.finish();
        let node = SyntaxNode::new_root(green);

        // Should be lossless
        assert_eq!(node.text().to_string(), text);

        // Should have both EMPHASIS and STRONG
        let has_emph = node.descendants().any(|n| n.kind() == SyntaxKind::EMPHASIS);
        let has_strong = node.descendants().any(|n| n.kind() == SyntaxKind::STRONG);

        assert!(has_emph, "Should have EMPHASIS node");
        assert!(has_strong, "Should have STRONG node");
    }

    /// Test Pandoc's "three" algorithm: ***foo* bar**
    /// Expected: Strong[Emph[foo], bar]
    #[test]
    fn test_triple_emphasis_star_then_double_star() {
        use crate::options::ParserOptions;
        use crate::syntax::SyntaxNode;
        use rowan::GreenNode;

        let text = "***foo* bar**";
        let config = ParserOptions::default();
        let mut builder = GreenNodeBuilder::new();

        builder.start_node(SyntaxKind::DOCUMENT.into());
        parse_inline_text_recursive(&mut builder, text, &config);
        builder.finish_node();

        let green: GreenNode = builder.finish();
        let node = SyntaxNode::new_root(green);

        // Verify losslessness
        assert_eq!(node.text().to_string(), text);

        // Expected structure: STRONG > EMPH > "foo"
        // The STRONG should contain EMPH, not the other way around
        let structure = format!("{:#?}", node);

        // Should have both STRONG and EMPH
        assert!(structure.contains("STRONG"), "Should have STRONG node");
        assert!(structure.contains("EMPHASIS"), "Should have EMPHASIS node");

        // STRONG should be outer, EMPH should be inner
        // Check that STRONG comes before EMPH in tree traversal
        let mut found_strong = false;
        let mut found_emph_after_strong = false;
        for descendant in node.descendants() {
            if descendant.kind() == SyntaxKind::STRONG {
                found_strong = true;
            }
            if found_strong && descendant.kind() == SyntaxKind::EMPHASIS {
                found_emph_after_strong = true;
                break;
            }
        }

        assert!(
            found_emph_after_strong,
            "EMPH should be inside STRONG, not before it. Current structure:\n{}",
            structure
        );
    }

    /// Test Pandoc's "three" algorithm: ***foo** bar*
    /// Expected: Emph[Strong[foo], bar]
    #[test]
    fn test_triple_emphasis_double_star_then_star() {
        use crate::options::ParserOptions;
        use crate::syntax::SyntaxNode;
        use rowan::GreenNode;

        let text = "***foo** bar*";
        let config = ParserOptions::default();
        let mut builder = GreenNodeBuilder::new();

        builder.start_node(SyntaxKind::DOCUMENT.into());
        parse_inline_text_recursive(&mut builder, text, &config);
        builder.finish_node();

        let green: GreenNode = builder.finish();
        let node = SyntaxNode::new_root(green);

        // Verify losslessness
        assert_eq!(node.text().to_string(), text);

        // Expected structure: EMPH > STRONG > "foo"
        let structure = format!("{:#?}", node);

        // Should have both EMPH and STRONG
        assert!(structure.contains("EMPHASIS"), "Should have EMPHASIS node");
        assert!(structure.contains("STRONG"), "Should have STRONG node");

        // EMPH should be outer, STRONG should be inner
        let mut found_emph = false;
        let mut found_strong_after_emph = false;
        for descendant in node.descendants() {
            if descendant.kind() == SyntaxKind::EMPHASIS {
                found_emph = true;
            }
            if found_emph && descendant.kind() == SyntaxKind::STRONG {
                found_strong_after_emph = true;
                break;
            }
        }

        assert!(
            found_strong_after_emph,
            "STRONG should be inside EMPH. Current structure:\n{}",
            structure
        );
    }

    /// Test that display math with attributes parses correctly
    /// Regression test for equation_attributes_single_line golden test
    #[test]
    fn test_display_math_with_attributes() {
        use crate::options::ParserOptions;
        use crate::syntax::SyntaxNode;
        use rowan::GreenNode;

        let text = "$$ E = mc^2 $$ {#eq-einstein}";
        let mut config = ParserOptions::default();
        config.extensions.quarto_crossrefs = true; // Enable Quarto cross-references

        let mut builder = GreenNodeBuilder::new();
        builder.start_node(SyntaxKind::DOCUMENT.into()); // Need a root node

        // Parse the whole text
        parse_inline_text_recursive(&mut builder, text, &config);

        builder.finish_node(); // Finish ROOT
        let green: GreenNode = builder.finish();
        let node = SyntaxNode::new_root(green);

        // Verify losslessness
        assert_eq!(node.text().to_string(), text);

        // Should have DISPLAY_MATH node
        let has_display_math = node
            .descendants()
            .any(|n| n.kind() == SyntaxKind::DISPLAY_MATH);
        assert!(has_display_math, "Should have DISPLAY_MATH node");

        // Should have ATTRIBUTE node
        let has_attributes = node
            .descendants()
            .any(|n| n.kind() == SyntaxKind::ATTRIBUTE);
        assert!(
            has_attributes,
            "Should have ATTRIBUTE node for {{#eq-einstein}}"
        );

        // Attributes should not be TEXT
        let math_followed_by_text = node.descendants().any(|n| {
            n.kind() == SyntaxKind::DISPLAY_MATH
                && n.next_sibling()
                    .map(|s| {
                        s.kind() == SyntaxKind::TEXT
                            && s.text().to_string().contains("{#eq-einstein}")
                    })
                    .unwrap_or(false)
        });
        assert!(
            !math_followed_by_text,
            "Attributes should not be parsed as TEXT"
        );
    }

    #[test]
    fn test_parse_inline_text_gfm_inline_link_destination_not_autolinked() {
        use crate::options::{Dialect, Extensions, Flavor};

        let config = ParserOptions {
            flavor: Flavor::Gfm,
            dialect: Dialect::for_flavor(Flavor::Gfm),
            extensions: Extensions::for_flavor(Flavor::Gfm),
            ..ParserOptions::default()
        };

        let mut builder = GreenNodeBuilder::new();
        builder.start_node(SyntaxKind::PARAGRAPH.into());
        parse_inline_text_recursive(
            &mut builder,
            "Second Link [link_text](https://link.com)",
            &config,
        );
        builder.finish_node();
        let green = builder.finish();
        let root = SyntaxNode::new_root(green);

        let links: Vec<_> = root
            .descendants()
            .filter(|n| n.kind() == SyntaxKind::LINK)
            .collect();
        assert_eq!(
            links.len(),
            1,
            "Expected exactly one LINK node for inline link, not nested bare URI autolink"
        );

        let link = links[0].clone();
        let mut link_text = None::<String>;
        let mut link_dest = None::<String>;

        for child in link.children() {
            match child.kind() {
                SyntaxKind::LINK_TEXT => link_text = Some(child.text().to_string()),
                SyntaxKind::LINK_DEST => link_dest = Some(child.text().to_string()),
                _ => {}
            }
        }

        assert_eq!(link_text.as_deref(), Some("link_text"));
        assert_eq!(link_dest.as_deref(), Some("https://link.com"));
    }

    #[test]
    fn test_autolink_bare_uri_utf8_boundary_safe() {
        let text = "§";
        let mut config = ParserOptions::default();
        config.extensions.autolink_bare_uris = true;
        let mut builder = GreenNodeBuilder::new();

        builder.start_node(SyntaxKind::DOCUMENT.into());
        parse_inline_text_recursive(&mut builder, text, &config);
        builder.finish_node();

        let green: GreenNode = builder.finish();
        let node = SyntaxNode::new_root(green);
        assert_eq!(node.text().to_string(), text);
    }

    #[test]
    fn test_parse_emphasis_unicode_content_no_panic() {
        let text = "*§*";
        let config = ParserOptions::default();
        let mut builder = GreenNodeBuilder::new();

        builder.start_node(SyntaxKind::PARAGRAPH.into());
        parse_inline_text_recursive(&mut builder, text, &config);
        builder.finish_node();

        let green: GreenNode = builder.finish();
        let node = SyntaxNode::new_root(green);
        let has_emph = node.descendants().any(|n| n.kind() == SyntaxKind::EMPHASIS);
        assert!(has_emph, "Should have EMPHASIS node");
        assert_eq!(node.text().to_string(), text);
    }
}

#[test]
fn test_two_with_nested_one_and_triple_closer() {
    // **bold with *italic***
    // Should parse as: Strong["bold with ", Emph["italic"]]
    // The *** at end is parsed as * (closes Emph) + ** (closes Strong)

    use crate::options::ParserOptions;
    use crate::syntax::SyntaxNode;
    use rowan::GreenNode;

    let text = "**bold with *italic***";
    let config = ParserOptions::default();
    let mut builder = GreenNodeBuilder::new();

    builder.start_node(SyntaxKind::PARAGRAPH.into());
    parse_inline_text_recursive(&mut builder, text, &config);
    builder.finish_node();

    let green: GreenNode = builder.finish();
    let node = SyntaxNode::new_root(green);

    assert_eq!(node.text().to_string(), text, "Should be lossless");

    let strong_nodes: Vec<_> = node
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::STRONG)
        .collect();
    assert_eq!(strong_nodes.len(), 1, "Should have exactly one STRONG node");
    let has_emphasis_in_strong = strong_nodes[0]
        .descendants()
        .any(|n| n.kind() == SyntaxKind::EMPHASIS);
    assert!(
        has_emphasis_in_strong,
        "STRONG should contain EMPHASIS node"
    );
}

#[test]
fn test_emphasis_with_trailing_space_before_closer() {
    // *foo * should parse as emphasis (Pandoc behavior)
    // For asterisks, Pandoc doesn't require right-flanking for closers

    use crate::options::ParserOptions;
    use crate::syntax::SyntaxNode;
    use rowan::GreenNode;

    let text = "*foo *";
    let config = ParserOptions::default();
    let mut builder = GreenNodeBuilder::new();

    builder.start_node(SyntaxKind::PARAGRAPH.into());
    parse_inline_text_recursive(&mut builder, text, &config);
    builder.finish_node();

    let green: GreenNode = builder.finish();
    let node = SyntaxNode::new_root(green);

    let has_emph = node.descendants().any(|n| n.kind() == SyntaxKind::EMPHASIS);
    assert!(has_emph, "Should have EMPHASIS node");
    assert_eq!(node.text().to_string(), text);
}

#[test]
fn test_triple_emphasis_all_strong_nested() {
    // ***foo** bar **baz*** should parse as Emph[Strong[foo], " bar ", Strong[baz]]
    // Pandoc output confirms this

    use crate::options::ParserOptions;
    use crate::syntax::SyntaxNode;
    use rowan::GreenNode;

    let text = "***foo** bar **baz***";
    let config = ParserOptions::default();
    let mut builder = GreenNodeBuilder::new();

    builder.start_node(SyntaxKind::DOCUMENT.into());
    parse_inline_text_recursive(&mut builder, text, &config);
    builder.finish_node();

    let green: GreenNode = builder.finish();
    let node = SyntaxNode::new_root(green);

    // Should have one EMPHASIS node at root
    let emphasis_nodes: Vec<_> = node
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::EMPHASIS)
        .collect();
    assert_eq!(
        emphasis_nodes.len(),
        1,
        "Should have exactly one EMPHASIS node, found: {}",
        emphasis_nodes.len()
    );

    // EMPHASIS should contain two STRONG nodes
    let emphasis_node = emphasis_nodes[0].clone();
    let strong_in_emphasis: Vec<_> = emphasis_node
        .children()
        .filter(|n| n.kind() == SyntaxKind::STRONG)
        .collect();
    assert_eq!(
        strong_in_emphasis.len(),
        2,
        "EMPHASIS should contain two STRONG nodes, found: {}",
        strong_in_emphasis.len()
    );

    // Verify losslessness
    assert_eq!(node.text().to_string(), text);
}

#[test]
fn test_triple_emphasis_all_emph_nested() {
    // ***foo* bar *baz*** should parse as Strong[Emph[foo], " bar ", Emph[baz]]
    // Pandoc output confirms this

    use crate::options::ParserOptions;
    use crate::syntax::SyntaxNode;
    use rowan::GreenNode;

    let text = "***foo* bar *baz***";
    let config = ParserOptions::default();
    let mut builder = GreenNodeBuilder::new();

    builder.start_node(SyntaxKind::DOCUMENT.into());
    parse_inline_text_recursive(&mut builder, text, &config);
    builder.finish_node();

    let green: GreenNode = builder.finish();
    let node = SyntaxNode::new_root(green);

    // Should have one STRONG node at root
    let strong_nodes: Vec<_> = node
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::STRONG)
        .collect();
    assert_eq!(
        strong_nodes.len(),
        1,
        "Should have exactly one STRONG node, found: {}",
        strong_nodes.len()
    );

    // STRONG should contain two EMPHASIS nodes
    let strong_node = strong_nodes[0].clone();
    let emph_in_strong: Vec<_> = strong_node
        .children()
        .filter(|n| n.kind() == SyntaxKind::EMPHASIS)
        .collect();
    assert_eq!(
        emph_in_strong.len(),
        2,
        "STRONG should contain two EMPHASIS nodes, found: {}",
        emph_in_strong.len()
    );

    // Verify losslessness
    assert_eq!(node.text().to_string(), text);
}

// Multiline emphasis tests
#[test]
fn test_parse_emphasis_multiline() {
    // Per Pandoc spec, emphasis CAN contain newlines (soft breaks)
    use crate::options::ParserOptions;
    use crate::syntax::SyntaxNode;
    use rowan::GreenNode;

    let text = "*text on\nline two*";
    let config = ParserOptions::default();
    let mut builder = GreenNodeBuilder::new();

    builder.start_node(SyntaxKind::PARAGRAPH.into());
    parse_inline_text_recursive(&mut builder, text, &config);
    builder.finish_node();

    let green: GreenNode = builder.finish();
    let node = SyntaxNode::new_root(green);

    let has_emph = node.descendants().any(|n| n.kind() == SyntaxKind::EMPHASIS);
    assert!(has_emph, "Should have EMPHASIS node");

    assert_eq!(node.text().to_string(), text);
    assert!(
        node.text().to_string().contains('\n'),
        "Should preserve newline in emphasis content"
    );
}

#[test]
fn test_parse_strong_multiline() {
    // Per Pandoc spec, strong emphasis CAN contain newlines
    use crate::options::ParserOptions;
    use crate::syntax::SyntaxNode;
    use rowan::GreenNode;

    let text = "**strong on\nline two**";
    let config = ParserOptions::default();
    let mut builder = GreenNodeBuilder::new();

    builder.start_node(SyntaxKind::PARAGRAPH.into());
    parse_inline_text_recursive(&mut builder, text, &config);
    builder.finish_node();

    let green: GreenNode = builder.finish();
    let node = SyntaxNode::new_root(green);

    let has_strong = node.descendants().any(|n| n.kind() == SyntaxKind::STRONG);
    assert!(has_strong, "Should have STRONG node");

    assert_eq!(node.text().to_string(), text);
    assert!(
        node.text().to_string().contains('\n'),
        "Should preserve newline in strong content"
    );
}

#[test]
fn test_parse_triple_emphasis_multiline() {
    // Triple emphasis with newlines
    use crate::options::ParserOptions;
    use crate::syntax::SyntaxNode;
    use rowan::GreenNode;

    let text = "***both on\nline two***";
    let config = ParserOptions::default();
    let mut builder = GreenNodeBuilder::new();

    builder.start_node(SyntaxKind::PARAGRAPH.into());
    parse_inline_text_recursive(&mut builder, text, &config);
    builder.finish_node();

    let green: GreenNode = builder.finish();
    let node = SyntaxNode::new_root(green);

    // Should have STRONG node (triple = strong + emph)
    let has_strong = node.descendants().any(|n| n.kind() == SyntaxKind::STRONG);
    assert!(has_strong, "Should have STRONG node");

    assert_eq!(node.text().to_string(), text);
    assert!(
        node.text().to_string().contains('\n'),
        "Should preserve newline in triple emphasis content"
    );
}

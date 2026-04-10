use crate::syntax::SyntaxKind;
use rowan::GreenNodeBuilder;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InlineExecutableVariant {
    RMarkdown,
    Quarto,
}

pub(crate) struct InlineExecutableMatch<'a> {
    pub(crate) total_len: usize,
    pub(crate) backtick_count: usize,
    pub(crate) prefix: &'a str,
    pub(crate) spacing_after_marker: &'a str,
    pub(crate) code: &'a str,
    pub(crate) variant: InlineExecutableVariant,
}

pub(crate) fn try_parse_inline_executable(
    text: &str,
    allow_rmarkdown: bool,
    allow_quarto: bool,
) -> Option<InlineExecutableMatch<'_>> {
    let (code_span_len, prefix, backtick_count, attrs) =
        super::code_spans::try_parse_code_span(text)?;
    if backtick_count != 1 || attrs.is_some() {
        return None;
    }

    let remaining = &text[code_span_len..];
    let line_len = remaining.find('\n').unwrap_or(remaining.len());
    let tail = &remaining[..line_len];
    if !tail.ends_with("``") {
        return None;
    }

    parse_tail(tail, allow_rmarkdown, allow_quarto).map(|(spacing_after_marker, code, variant)| {
        InlineExecutableMatch {
            total_len: code_span_len + line_len,
            backtick_count,
            prefix,
            spacing_after_marker,
            code,
            variant,
        }
    })
}

fn parse_tail(
    tail: &str,
    allow_rmarkdown: bool,
    allow_quarto: bool,
) -> Option<(&str, &str, InlineExecutableVariant)> {
    if allow_rmarkdown && tail.starts_with('r') {
        return parse_marker_and_code(tail, "r", InlineExecutableVariant::RMarkdown);
    }
    if allow_quarto && tail.starts_with("{r}") {
        return parse_marker_and_code(tail, "{r}", InlineExecutableVariant::Quarto);
    }
    None
}

fn parse_marker_and_code<'a>(
    tail: &'a str,
    marker: &'a str,
    variant: InlineExecutableVariant,
) -> Option<(&'a str, &'a str, InlineExecutableVariant)> {
    let suffix = &tail[marker.len()..];
    let spacing_len = suffix
        .chars()
        .take_while(|ch| ch.is_whitespace())
        .map(char::len_utf8)
        .sum::<usize>();
    if spacing_len == 0 {
        return None;
    }
    let spacing_after_marker = &suffix[..spacing_len];
    let mut code = &suffix[spacing_len..];
    if let Some(stripped) = code.strip_suffix("``") {
        code = stripped;
    } else {
        return None;
    }
    if code.trim().is_empty() {
        return None;
    }
    Some((spacing_after_marker, code, variant))
}

pub(crate) fn emit_inline_executable(
    builder: &mut GreenNodeBuilder,
    m: &InlineExecutableMatch<'_>,
) {
    builder.start_node(SyntaxKind::INLINE_EXEC.into());
    builder.token(
        SyntaxKind::INLINE_EXEC_MARKER.into(),
        &"`".repeat(m.backtick_count),
    );
    if !m.prefix.is_empty() {
        builder.token(SyntaxKind::TEXT.into(), m.prefix);
    }
    let lang = match m.variant {
        InlineExecutableVariant::RMarkdown => "r",
        InlineExecutableVariant::Quarto => "{r}",
    };
    builder.token(SyntaxKind::INLINE_EXEC_LANG.into(), lang);
    builder.token(SyntaxKind::WHITESPACE.into(), m.spacing_after_marker);
    builder.token(SyntaxKind::INLINE_EXEC_CONTENT.into(), m.code);
    builder.token(
        SyntaxKind::INLINE_EXEC_MARKER.into(),
        &"`".repeat(m.backtick_count),
    );
    builder.finish_node();
}

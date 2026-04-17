use std::borrow::Cow;

pub(super) fn normalize_smart_punctuation(
    text: &str,
    smart_enabled: bool,
    smart_quotes_enabled: bool,
) -> Cow<'_, str> {
    if !smart_enabled && !smart_quotes_enabled {
        return Cow::Borrowed(text);
    }

    if !text.contains([
        '\u{2018}', '\u{2019}', '\u{201C}', '\u{201D}', '\u{2013}', '\u{2014}', '\u{2026}',
    ]) {
        return Cow::Borrowed(text);
    }

    let mut out = String::with_capacity(text.len() + 8);
    for ch in text.chars() {
        match ch {
            '\u{2018}' | '\u{2019}' if smart_enabled || smart_quotes_enabled => out.push('\''),
            '\u{201C}' | '\u{201D}' if smart_enabled || smart_quotes_enabled => out.push('"'),
            '\u{2013}' if smart_enabled => out.push_str("--"),
            '\u{2014}' if smart_enabled => out.push_str("---"),
            '\u{2026}' if smart_enabled => out.push_str("..."),
            _ => out.push(ch),
        }
    }
    Cow::Owned(out)
}

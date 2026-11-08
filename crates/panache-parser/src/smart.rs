//! Pandoc's `smart` typography rules, shared by the Pandoc AST projector and
//! the formatter, plus their inverse for the formatter's ASCII output.
//!
//! Quote pairing works on a flat run of [`QuoteAtom`]s (one inline container
//! level, like a pandoc inline list) and reports which atoms open and close a
//! quote; callers map the result back onto their own representation.

use std::borrow::Cow;

/// Which quote character a [`QuotePair`] uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuoteKind {
    Single,
    Double,
}

impl QuoteKind {
    fn ascii(self) -> char {
        match self {
            QuoteKind::Single => '\'',
            QuoteKind::Double => '"',
        }
    }
}

/// One element of an inline run, reduced to what quote pairing inspects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuoteAtom<'a> {
    /// A run of non-whitespace text (a pandoc `Str`).
    Str(&'a str),
    /// Whitespace, a soft break, or a hard line break.
    Space,
    /// Emphasis-like markup or a code span: an opening quote may attach to it.
    Markup,
    /// Any other inline (link, math, note, ...).
    Other,
}

/// A matched quote: `open` is the atom starting with the opening quote,
/// `close` the atom ending with the closing one (they may be the same atom).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuotePair {
    pub open: usize,
    pub close: usize,
    pub kind: QuoteKind,
}

fn is_boundary_before(prev: Option<QuoteAtom<'_>>) -> bool {
    match prev {
        None | Some(QuoteAtom::Space) => true,
        Some(QuoteAtom::Str(s)) => s.chars().last().is_some_and(|c| !c.is_alphanumeric()),
        Some(_) => false,
    }
}

fn is_boundary_after(next: Option<QuoteAtom<'_>>) -> bool {
    match next {
        None | Some(QuoteAtom::Space) => true,
        Some(QuoteAtom::Str(s)) => s.chars().next().is_some_and(|c| !c.is_alphanumeric()),
        Some(_) => false,
    }
}

/// Pair opening and closing quotes in `atoms`. Pairs never overlap and are
/// returned in order; quotes nested inside a pair are not paired.
pub fn quote_pairs(atoms: &[QuoteAtom<'_>]) -> Vec<QuotePair> {
    let mut pairs = Vec::new();
    let mut prev_was_quoted = false;
    let mut i = 0;
    while i < atoms.len() {
        if let QuoteAtom::Str(s) = atoms[i] {
            let mut chars = s.chars();
            let kind = match chars.next() {
                Some('"') => Some(QuoteKind::Double),
                Some('\'') => Some(QuoteKind::Single),
                _ => None,
            };
            let second = chars.next();
            let attaches = match second {
                Some(c) => !c.is_whitespace(),
                // Like pandoc, any inline right after the quote lets it open.
                None => matches!(atoms.get(i + 1), Some(QuoteAtom::Markup | QuoteAtom::Other)),
            };
            // A just-closed quote is an inline of its own, never a boundary.
            let prev_is_boundary =
                !prev_was_quoted && is_boundary_before(i.checked_sub(1).map(|j| atoms[j]));
            if let Some(kind) = kind
                && prev_is_boundary
                && attaches
                && let Some(close) = find_matching_close(atoms, i, kind.ascii())
            {
                pairs.push(QuotePair {
                    open: i,
                    close,
                    kind,
                });
                prev_was_quoted = true;
                i = close + 1;
                continue;
            }
        }
        prev_was_quoted = false;
        i += 1;
    }
    pairs
}

fn find_matching_close(atoms: &[QuoteAtom<'_>], open: usize, quote: char) -> Option<usize> {
    if let QuoteAtom::Str(s) = atoms[open]
        && s.chars().count() >= 3
        && s.ends_with(quote)
        && is_boundary_after(atoms.get(open + 1).copied())
    {
        return Some(open);
    }
    for j in open + 1..atoms.len().min(open + 33) {
        if let QuoteAtom::Str(s) = atoms[j]
            && s.ends_with(quote)
            && is_boundary_after(atoms.get(j + 1).copied())
        {
            return Some(j);
        }
    }
    None
}

/// Replace `---` with an em dash, `--` with an en dash, and `...` with an
/// ellipsis.
pub fn dashes_and_ellipsis(s: &str) -> Cow<'_, str> {
    if !s.contains("--") && !s.contains("...") {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while !rest.is_empty() {
        if let Some(r) = rest.strip_prefix("---") {
            out.push('—');
            rest = r;
        } else if let Some(r) = rest.strip_prefix("--") {
            out.push('–');
            rest = r;
        } else if let Some(r) = rest.strip_prefix("...") {
            out.push('…');
            rest = r;
        } else {
            let c = rest.chars().next().unwrap();
            out.push(c);
            rest = &rest[c.len_utf8()..];
        }
    }
    Cow::Owned(out)
}

/// Inverse of the `smart` rules: write `‘’` as `'` and `“”` as `"`, and with
/// `dashes` also `—`, `–`, and `…` as `---`, `--`, and `...`.
pub fn ascii_punctuation(s: &str, dashes: bool) -> Cow<'_, str> {
    if !s.contains(['‘', '’', '“', '”', '–', '—', '…']) {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '‘' | '’' => out.push('\''),
            '“' | '”' => out.push('"'),
            '–' if dashes => out.push_str("--"),
            '—' if dashes => out.push_str("---"),
            '…' if dashes => out.push_str("..."),
            _ => out.push(c),
        }
    }
    Cow::Owned(out)
}

/// Replace a `'` between two alphanumeric characters with an apostrophe.
pub fn intraword_apostrophe(s: &str) -> Cow<'_, str> {
    if !s.contains('\'') {
        return Cow::Borrowed(s);
    }
    let chars: Vec<char> = s.chars().collect();
    chars
        .iter()
        .enumerate()
        .map(|(i, &c)| {
            let between_words = i > 0
                && chars[i - 1].is_alphanumeric()
                && chars.get(i + 1).is_some_and(|n| n.is_alphanumeric());
            if c == '\'' && between_words { '’' } else { c }
        })
        .collect::<String>()
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use QuoteAtom::*;

    #[test]
    fn pairs_quotes_across_atoms() {
        let atoms = [Str("\"hi"), Space, Markup, Str("\""), Space, Str("x")];
        assert_eq!(
            quote_pairs(&atoms),
            vec![QuotePair {
                open: 0,
                close: 3,
                kind: QuoteKind::Double
            }]
        );
    }

    #[test]
    fn single_atom_pair_and_unmatched_quote() {
        assert_eq!(quote_pairs(&[Str("'a'")]).len(), 1);
        assert!(quote_pairs(&[Str("\"a"), Space, Str("b")]).is_empty());
        // Not after a word character.
        assert!(quote_pairs(&[Str("a\"b\"")]).is_empty());
    }

    #[test]
    fn ascii_punctuation_inverts_dashes_and_ellipsis() {
        let unicode = "a — b – c… “d” ‘e’";
        assert_eq!(
            ascii_punctuation(unicode, true),
            "a --- b -- c... \"d\" 'e'"
        );
        assert_eq!(
            dashes_and_ellipsis(&ascii_punctuation(unicode, true)),
            "a — b – c… \"d\" 'e'"
        );
        assert_eq!(ascii_punctuation(unicode, false), "a — b – c… \"d\" 'e'");
    }

    #[test]
    fn dashes_ellipsis_and_apostrophes() {
        assert_eq!(dashes_and_ellipsis("a---b--c...d----"), "a—b–c…d—-");
        assert_eq!(intraword_apostrophe("it's 'x'"), "it’s 'x'");
    }
}

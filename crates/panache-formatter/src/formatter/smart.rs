use std::borrow::Cow;
use std::collections::HashMap;
use std::ops::Range;

use rowan::NodeOrToken;

use panache_parser::smart::{
    QuoteAtom, QuoteKind, ascii_punctuation, dashes_and_ellipsis, intraword_apostrophe, quote_pairs,
};

use crate::config::{Config, SmartPunctuation};
use crate::syntax::{SyntaxKind, SyntaxNode, SyntaxToken};

use super::sentence_wrap::resolve_lang_string;

/// Text of `token` with smart punctuation applied, when the current format
/// run computed any for it (see [`smart_text_map`]).
pub(super) fn smart_token_text<'a>(token: &'a SyntaxToken, config: &Config) -> Cow<'a, str> {
    match config
        .smart_text
        .get(&u32::from(token.text_range().start()))
    {
        Some(text) => Cow::Owned(text.clone()),
        None => Cow::Borrowed(token.text()),
    }
}

/// Source text of `node` with [`smart_token_text`] applied to each token.
pub(super) fn smart_node_text(node: &SyntaxNode, config: &Config) -> String {
    if config.smart_text.is_empty() {
        return node.text().to_string();
    }
    node.descendants_with_tokens()
        .filter_map(NodeOrToken::into_token)
        .map(|token| smart_token_text(&token, config).into_owned())
        .collect()
}

/// `[open double, close double, open single, close single]` for a language,
/// from CLDR's `quotationStart`/`quotationEnd` and
/// `alternateQuotationStart`/`alternateQuotationEnd` locale delimiters.
fn quotes_for_lang(lang: Option<&str>) -> [char; 4] {
    const ENGLISH: [char; 4] = ['“', '”', '‘', '’'];
    let Some(lang) = lang else {
        return ENGLISH;
    };
    match lang {
        "fr-ch" => return ['«', '»', '‹', '›'],
        "pt-pt" => return ['«', '»', '“', '”'],
        _ => {}
    }
    match lang.split(['-', '_']).next().unwrap_or(lang) {
        "de" | "cs" | "sk" => ['„', '“', '‚', '‘'],
        "fr" => ['«', '»', '«', '»'],
        "it" => ['«', '»', '“', '”'],
        "ru" | "uk" => ['«', '»', '„', '“'],
        "pl" => ['„', '”', '«', '»'],
        "sv" | "fi" => ['”', '”', '’', '’'],
        "nb" | "nn" | "no" => ['«', '»', '‘', '’'],
        "nl" => ['‘', '’', '‘', '’'],
        "ja" => ['「', '」', '『', '』'],
        _ => ENGLISH,
    }
}

/// Element kinds whose direct `TEXT` children are prose that pandoc's `smart`
/// extension applies to. Everything else (URLs, attributes, code, raw HTML,
/// shortcodes, table pipes, ...) keeps its text verbatim.
fn is_prose_container(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::PARAGRAPH
            | SyntaxKind::PLAIN
            | SyntaxKind::HEADING_CONTENT
            | SyntaxKind::EMPHASIS
            | SyntaxKind::STRONG
            | SyntaxKind::STRIKEOUT
            | SyntaxKind::MARK
            | SyntaxKind::SUPERSCRIPT
            | SyntaxKind::SUBSCRIPT
            | SyntaxKind::LINK_TEXT
            | SyntaxKind::IMAGE_ALT
            | SyntaxKind::SPAN_CONTENT
            | SyntaxKind::INLINE_FOOTNOTE
            | SyntaxKind::CITATION
    )
}

/// Tokens of a prose container that hold prose (a citation's prefix and
/// suffix are `CITATION_CONTENT`).
fn is_prose_text(kind: SyntaxKind) -> bool {
    matches!(kind, SyntaxKind::TEXT | SyntaxKind::CITATION_CONTENT)
}

/// Markup a quote may attach to, like pandoc's `Emph`/`Strong`/`Code`.
fn is_markup_atom(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::EMPHASIS
            | SyntaxKind::STRONG
            | SyntaxKind::STRIKEOUT
            | SyntaxKind::SUPERSCRIPT
            | SyntaxKind::SUBSCRIPT
            | SyntaxKind::INLINE_CODE
    )
}

/// Char indices where a word splits into separate quote-pairing pieces: before
/// a quote that follows punctuation and after one that precedes punctuation.
/// Pandoc pairs quotes inside a word (`("x").`), while the shared pairing rules
/// only look at the start and end of each piece. Adjacent quotes stay together
/// so `"'nested'"` still pairs as a whole.
fn split_points(view: &[char]) -> Vec<usize> {
    let is_quote = |c: char| c == '"' || c == '\'';
    let is_punct = |c: char| !c.is_alphanumeric() && !is_quote(c);
    let mut points = Vec::new();
    for (j, &c) in view.iter().enumerate() {
        if !is_quote(c) {
            continue;
        }
        if j > 0 && is_punct(view[j - 1]) {
            points.push(j);
        }
        if j + 1 < view.len() && is_punct(view[j + 1]) {
            points.push(j + 1);
        }
    }
    points.dedup();
    points
}

/// A container's own delimiters, which are not inlines for pandoc.
fn is_delimiter(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::EMPHASIS_MARKER
            | SyntaxKind::STRONG_MARKER
            | SyntaxKind::STRIKEOUT_MARKER
            | SyntaxKind::MARK_MARKER
            | SyntaxKind::SUPERSCRIPT_MARKER
            | SyntaxKind::SUBSCRIPT_MARKER
            | SyntaxKind::INLINE_FOOTNOTE_START
            | SyntaxKind::INLINE_FOOTNOTE_END
            | SyntaxKind::LINE_PREFIX
            | SyntaxKind::BLOCK_QUOTE_MARKER
    )
}

/// A non-whitespace run inside a prose `TEXT` token.
struct Piece {
    token: usize,
    /// Byte range in the token text.
    range: Range<usize>,
    /// Source text with recognized quotes made ASCII and intraword `'` made
    /// `’`. Has the same number of chars as the source text.
    view: String,
}

/// One element of a container's inline run: a byte range of a piece's view,
/// or a non-text atom.
#[derive(Clone)]
enum Atom {
    Text { piece: usize, range: Range<usize> },
    Other(QuoteAtom<'static>),
}

/// Compute the smart-punctuation text of every prose token in `tree` that
/// changes under `smart-punctuation`, keyed by token start offset. Empty unless
/// `smart` or `smart-quotes` is enabled.
pub(super) fn smart_text_map(tree: &SyntaxNode, config: &Config) -> HashMap<u32, String> {
    let ext = &config.formatter_extensions;
    let mut map = HashMap::new();
    if !(ext.smart || ext.smart_quotes) {
        return map;
    }
    let containers = tree.descendants().filter(|n| is_prose_container(n.kind()));
    match config.smart_punctuation {
        SmartPunctuation::Preserve => {}
        SmartPunctuation::Ascii => {
            let tokens = containers.flat_map(|node| {
                node.children_with_tokens()
                    .filter_map(NodeOrToken::into_token)
                    .filter(|token| is_prose_text(token.kind()))
            });
            for token in tokens {
                let text = ascii_punctuation(token.text(), ext.smart);
                if text != token.text() {
                    map.insert(u32::from(token.text_range().start()), text.into_owned());
                }
            }
        }
        SmartPunctuation::Unicode => {
            let quotes = config.quotes.unwrap_or_else(|| {
                quotes_for_lang(resolve_lang_string(tree, config.lang.as_deref()).as_deref())
            });
            let converter = Converter {
                quotes,
                dashes: ext.smart,
            };
            for node in containers {
                converter.convert_container(&node, &mut map);
            }
        }
    }
    map
}

struct Converter {
    quotes: [char; 4],
    dashes: bool,
}

impl Converter {
    /// The ASCII quote a source char stands for: the active quotes plus the
    /// English ones, so a document can be re-localized. Active quotes win,
    /// since some languages nest English double quotes as their single ones.
    fn ascii_quote(&self, c: char) -> Option<char> {
        let [od, cd, os, cs] = self.quotes;
        match c {
            _ if c == od || c == cd => Some('"'),
            _ if c == os || c == cs => Some('\''),
            '“' | '”' => Some('"'),
            '‘' | '’' => Some('\''),
            _ => None,
        }
    }

    fn convert_container(&self, node: &SyntaxNode, map: &mut HashMap<u32, String>) {
        let mut tokens: Vec<SyntaxToken> = Vec::new();
        let mut pieces: Vec<Piece> = Vec::new();
        let mut atoms: Vec<Atom> = Vec::new();
        let push_space = |atoms: &mut Vec<Atom>| {
            if !matches!(atoms.last(), Some(Atom::Other(QuoteAtom::Space))) {
                atoms.push(Atom::Other(QuoteAtom::Space));
            }
        };
        for child in node.children_with_tokens() {
            if is_delimiter(child.kind()) {
                continue;
            }
            let token = match child {
                NodeOrToken::Node(n) => {
                    atoms.push(Atom::Other(if is_markup_atom(n.kind()) {
                        QuoteAtom::Markup
                    } else {
                        QuoteAtom::Other
                    }));
                    continue;
                }
                NodeOrToken::Token(t) => t,
            };
            match token.kind() {
                kind if is_prose_text(kind) => {}
                SyntaxKind::NEWLINE | SyntaxKind::WHITESPACE | SyntaxKind::HARD_LINE_BREAK => {
                    push_space(&mut atoms);
                    continue;
                }
                _ => {
                    atoms.push(Atom::Other(QuoteAtom::Other));
                    continue;
                }
            }
            let text = token.text();
            let mut word_start = None;
            for (i, c) in text.char_indices().chain([(text.len(), ' ')]) {
                if !c.is_whitespace() {
                    word_start.get_or_insert(i);
                    continue;
                }
                if let Some(s) = word_start.take() {
                    let mapped: String = text[s..i]
                        .chars()
                        .map(|c| self.ascii_quote(c).unwrap_or(c))
                        .collect();
                    let view = intraword_apostrophe(&mapped);
                    let starts: Vec<usize> = text[s..i]
                        .char_indices()
                        .map(|(b, _)| s + b)
                        .chain([i])
                        .collect();
                    let view_chars: Vec<char> = view.chars().collect();
                    let mut from = 0;
                    for to in split_points(&view_chars)
                        .into_iter()
                        .chain([view_chars.len()])
                    {
                        let view: String = view_chars[from..to].iter().collect();
                        atoms.push(Atom::Text {
                            piece: pieces.len(),
                            range: 0..view.len(),
                        });
                        pieces.push(Piece {
                            token: tokens.len(),
                            range: starts[from]..starts[to],
                            view,
                        });
                        from = to;
                    }
                }
                if i < text.len() {
                    push_space(&mut atoms);
                }
            }
            tokens.push(token);
        }
        if pieces.is_empty() {
            return;
        }

        let mut marks: HashMap<(usize, usize), char> = HashMap::new();
        self.pair(&atoms, &pieces, &mut marks);

        let mut new_texts: Vec<String> = tokens.iter().map(|t| t.text().to_string()).collect();
        // Back to front, so earlier byte ranges in a token stay valid.
        for (p, piece) in pieces.iter().enumerate().rev() {
            let source = &tokens[piece.token].text()[piece.range.clone()];
            let converted: String = source
                .chars()
                .zip(piece.view.chars())
                .enumerate()
                .map(|(i, (src, view))| match marks.get(&(p, i)) {
                    Some(&quote) => quote,
                    // Pandoc renders a lone `'` as an apostrophe.
                    None if src == '\'' => '’',
                    // Other unpaired quotes keep their source spelling.
                    None if self.ascii_quote(src).is_some() && view != '’' => src,
                    None => view,
                })
                .collect();
            let converted = if self.dashes {
                dashes_and_ellipsis(&converted)
            } else {
                converted
            };
            new_texts[piece.token].replace_range(piece.range.clone(), &converted);
        }
        for (token, text) in tokens.iter().zip(new_texts) {
            if text != token.text() {
                map.insert(u32::from(token.text_range().start()), text);
            }
        }
    }

    /// Pair quotes among `atoms`, recording the quote char for each paired
    /// position as `(piece, char index)`, then recurse into each pair's
    /// content so nested quotes pair too.
    fn pair(&self, atoms: &[Atom], pieces: &[Piece], marks: &mut HashMap<(usize, usize), char>) {
        let quote_atoms: Vec<QuoteAtom<'_>> = atoms
            .iter()
            .map(|atom| match atom {
                Atom::Text { piece, range } => QuoteAtom::Str(&pieces[*piece].view[range.clone()]),
                Atom::Other(atom) => *atom,
            })
            .collect();
        let [od, cd, os, cs] = self.quotes;
        for pair in quote_pairs(&quote_atoms) {
            let (open_quote, close_quote) = match pair.kind {
                QuoteKind::Double => (od, cd),
                QuoteKind::Single => (os, cs),
            };
            let (
                Atom::Text {
                    piece: op,
                    range: or,
                },
                Atom::Text {
                    piece: cp,
                    range: cr,
                },
            ) = (&atoms[pair.open], &atoms[pair.close])
            else {
                unreachable!("quote pairs start and end in text");
            };
            let char_index = |piece: usize, byte: usize| pieces[piece].view[..byte].chars().count();
            marks.insert((*op, char_index(*op, or.start)), open_quote);
            marks.insert((*cp, char_index(*cp, cr.end - 1)), close_quote);

            // The quotes are ASCII in the view, so +-1 stays on char boundaries.
            let inner: Vec<Atom> = if pair.open == pair.close {
                vec![Atom::Text {
                    piece: *op,
                    range: or.start + 1..or.end - 1,
                }]
            } else {
                std::iter::once(Atom::Text {
                    piece: *op,
                    range: or.start + 1..or.end,
                })
                .chain(atoms[pair.open + 1..pair.close].iter().cloned())
                .chain([Atom::Text {
                    piece: *cp,
                    range: cr.start..cr.end - 1,
                }])
                .collect()
            };
            // Pandoc drops a quote's emptied first/last `Str`.
            let inner: Vec<Atom> = inner
                .into_iter()
                .filter(|atom| !matches!(atom, Atom::Text { range, .. } if range.is_empty()))
                .collect();
            self.pair(&inner, pieces, marks);
        }
    }
}

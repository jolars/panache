//! Math operator *interpretation* — the analog of YAML scalar cooking
//! ([`panache_parser::parser::yaml`]'s `cooking.rs`).
//!
//! The parser emits neutral `MATH_WORD` runs and never tags bin/rel or builds a
//! precedence tree: TeX
//! assigns an atom's class contextually during mlist→hlist (a Bin atom after
//! Bin/Rel/Open/Punct becomes Ord — that *is* unary minus), it is
//! override-able (`\mathbin`) and macro-dependent. So class/precedence is a
//! pure *interpretation* shared between consumers, not a CST shape — it lives
//! here, keyed on operator text and command name, never in `MATH_*` kinds.
//!
//! [`word_atoms`] supplies the shared character-level semantic view used by
//! spacing and line-breaking. The command table supplies the corresponding
//! view for known control words.

use crate::syntax::{AstNode, MathScripted, SyntaxElement, SyntaxKind, SyntaxToken};
use panache_parser::semantic::math::{
    MathClass, coerces_binary_to_ordinary, math_char_info, math_command_info,
};
use rowan::NodeOrToken;

/// TeX atom classes (the subset the formatter's spacing pass needs; see The
/// TeXbook Appendix G).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AtomClass {
    /// Ordinary atom (letters, digits, most commands) — also the result of
    /// coercing a unary `+`/`-`.
    Ord,
    /// Binary operator (`+ - *`, `\cdot`, …) — one space on each side, unless
    /// coerced to [`AtomClass::Ord`] in a unary position.
    Bin,
    /// Relation (`= < >`, `\leq`, …) — one space on each side; never coerced.
    Rel,
    /// Opening delimiter (`(`, `[`, `{`) — makes a following `+`/`-` unary.
    Open,
    /// Closing delimiter (`)`, `]`, `}`).
    Close,
    /// Punctuation (`,`, `;`) — makes a following `+`/`-` unary.
    Punct,
    /// Large operator (`\sum`, `\int`, …) — Ord-like for spacing, but makes a
    /// following `+`/`-` unary.
    Op,
}

/// One semantic slice of a lexically neutral `MATH_WORD` token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WordAtom<'a> {
    pub text: &'a str,
    pub class: AtomClass,
}

impl<'a> WordAtom<'a> {
    pub const fn new(text: &'a str, class: AtomClass) -> Self {
        Self { text, class }
    }
}

/// Semantic atoms derived from one Badness-grain `MATH_WORD` run.
pub struct WordAtoms<'a> {
    rest: &'a str,
}

/// Slice a lexical word into TeX atom candidates without changing the CST.
///
/// Relations (`=<>`) form maximal runs, colon runs ending in `=` join the same relation run,
/// binary signs and fixed delimiters are single-scalar atoms, and all other
/// characters remain maximal ordinary runs.
pub fn word_atoms(word: &str) -> WordAtoms<'_> {
    WordAtoms { rest: word }
}

impl<'a> Iterator for WordAtoms<'a> {
    type Item = WordAtom<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.rest.is_empty() {
            return None;
        }

        let (len, class) = if let Some(len) = definition_relation_prefix_len(self.rest) {
            (len, AtomClass::Rel)
        } else {
            let first = self.rest.chars().next().expect("non-empty word");
            match first {
                ':' => (colon_prefix_len(self.rest), AtomClass::Punct),
                _ => match semantic_char_class(first) {
                    Some(AtomClass::Rel) => (relation_prefix_len(self.rest), AtomClass::Rel),
                    Some(class) => (first.len_utf8(), class),
                    None => (ordinary_prefix_len(self.rest), AtomClass::Ord),
                },
            }
        };
        let (text, rest) = self.rest.split_at(len);
        self.rest = rest;
        Some(WordAtom::new(text, class))
    }
}

fn colon_prefix_len(text: &str) -> usize {
    text.char_indices()
        .find_map(|(offset, ch)| (ch != ':').then_some(offset))
        .unwrap_or(text.len())
}

fn definition_relation_prefix_len(text: &str) -> Option<usize> {
    let colon_end = colon_prefix_len(text);
    let tail = &text[colon_end..];
    tail.starts_with('=')
        .then(|| colon_end + relation_prefix_len(tail))
}

/// Which characters end an ordinary run and which atom class each contributes.
/// `None` means the character is part of an ordinary run.
///
/// This resolves through [`math_char_info`], the same table the parser's
/// semantic atom stream uses, so the legacy renderer and the typed lowering
/// cannot classify a character differently. A `:` is punctuation: it is an atom
/// of its own, and it may start a `:=` definition relation, which
/// [`word_atoms`] recognizes before consulting this.
fn semantic_char_class(c: char) -> Option<AtomClass> {
    atom_class(math_char_info(c).class)
}

fn relation_prefix_len(text: &str) -> usize {
    text.char_indices()
        .find_map(|(offset, ch)| (!matches!(ch, '=' | '<' | '>')).then_some(offset))
        .unwrap_or(text.len())
}

fn ordinary_prefix_len(text: &str) -> usize {
    text.char_indices()
        .skip(1)
        .find_map(|(offset, ch)| semantic_char_class(ch).is_some().then_some(offset))
        .unwrap_or(text.len())
}

/// Whether `text` is one maximal relation run (`=`, `<`, `>` characters only)
/// — the shape a relation head has after the parser's script split severed it
/// from its final scalar (`x<` + scripted `=`).
pub fn is_relation_run(text: &str) -> bool {
    !text.is_empty() && text.chars().all(|c| matches!(c, '=' | '<' | '>'))
}

/// The base atom of a `MATH_SCRIPTED` element, or `None` for anything else.
pub fn scripted_base(element: &SyntaxElement) -> Option<SyntaxElement> {
    element
        .as_node()
        .and_then(|node| MathScripted::cast(node.clone()))
        .and_then(|scripted| scripted.base())
}

/// The control-sequence token naming a command element: a bare control token
/// itself, or the head control word of a `MATH_COMMAND` node.
pub fn command_name_token(element: &SyntaxElement) -> Option<SyntaxToken> {
    match element {
        NodeOrToken::Token(token)
            if matches!(
                token.kind(),
                SyntaxKind::MATH_CONTROL_WORD | SyntaxKind::MATH_CONTROL_SYMBOL
            ) =>
        {
            Some(token.clone())
        }
        NodeOrToken::Node(node) if node.kind() == SyntaxKind::MATH_COMMAND => node
            .children_with_tokens()
            .filter_map(|child| child.into_token())
            .find(|token| token.kind() == SyntaxKind::MATH_CONTROL_WORD),
        _ => None,
    }
}

/// Atom class of a scripted base *as one operand*, shared by inline spacing
/// and line-break candidate scans so the two can never disagree. Token bases
/// resolve through [`word_atoms`] / [`command_class`]; structured bases
/// (groups, environments, `\left…\right`) behave like a closed operand on
/// their left, hence [`AtomClass::Close`] — callers must not treat that as a
/// delimiter-depth change.
pub fn scripted_base_class(base: &SyntaxElement) -> Option<AtomClass> {
    match base.kind() {
        SyntaxKind::MATH_WORD => base
            .as_token()
            .and_then(|token| word_atoms(token.text()).next())
            .map(|atom| atom.class),
        SyntaxKind::MATH_COMMAND | SyntaxKind::MATH_CONTROL_SYMBOL => {
            command_name_token(base).map(|token| {
                let name = token.text().strip_prefix('\\').unwrap_or(token.text());
                command_class(name).unwrap_or(AtomClass::Ord)
            })
        }
        SyntaxKind::MATH_GROUP | SyntaxKind::MATH_ENVIRONMENT | SyntaxKind::MATH_DELIMITED => {
            Some(AtomClass::Close)
        }
        SyntaxKind::MATH_CARET | SyntaxKind::MATH_UNDERSCORE | SyntaxKind::MATH_ALIGN => {
            Some(AtomClass::Open)
        }
        SyntaxKind::MATH_LINE_BREAK => None,
        _ => Some(AtomClass::Ord),
    }
}

/// Class of a command operator, keyed on its name **without** the leading
/// backslash. `None` for any command that spaces like an ordinary atom (Greek
/// letters, `\frac`, `\text`, …), which the caller treats as
/// [`AtomClass::Ord`].
///
/// This resolves through [`math_command_info`], the same table the parser's
/// semantic atom stream uses, so the legacy renderer and the typed lowering
/// cannot classify a command differently.
pub fn command_class(name: &str) -> Option<AtomClass> {
    atom_class(math_command_info(name).class)
}

/// Map a parser atom class onto the formatter's spacing class. `None` is the
/// ordinary-spacing default, which also covers the delimiter-ish classes that
/// carry no spacing of their own.
fn atom_class(class: MathClass) -> Option<AtomClass> {
    match class {
        MathClass::Bin => Some(AtomClass::Bin),
        MathClass::Rel => Some(AtomClass::Rel),
        MathClass::Op => Some(AtomClass::Op),
        MathClass::Open => Some(AtomClass::Open),
        MathClass::Close => Some(AtomClass::Close),
        MathClass::Punct => Some(AtomClass::Punct),
        MathClass::Ord | MathClass::Fence | MathClass::Inner => None,
    }
}

/// Whether `*` immediately following this command is a syntactic modifier,
/// rather than a binary operator.
pub fn takes_star_modifier(name: &str) -> bool {
    matches!(name, "operatorname")
}

/// Whether a command (name **without** the leading backslash) switches its
/// mandatory `{…}` argument into *text mode*, where whitespace is significant
/// and must be preserved verbatim. The curated set is the single-argument
/// text-switching family; math-mode font commands (`\mathrm`, `\mathbf`) are
/// **excluded** because spaces are already insignificant inside them, and
/// multi-argument commands (`\textcolor`) are excluded because their text
/// argument is not the first group.
pub fn is_text_mode_command(name: &str) -> bool {
    matches!(
        name,
        "text"
            | "textrm"
            | "textbf"
            | "textit"
            | "texttt"
            | "textsf"
            | "textsc"
            | "textnormal"
            | "textup"
            | "textsl"
            | "textmd"
            | "mbox"
            | "hbox"
    )
}

/// TeX Bin→Ord coercion (the rule that yields unary minus): a [`AtomClass::Bin`]
/// run becomes [`AtomClass::Ord`] when the preceding atom is absent (list start)
/// or one of Bin/Rel/Open/Punct/Op. [`AtomClass::Rel`] never coerces.
///
/// The rule itself lives in [`coerces_binary_to_ordinary`], shared with the
/// parser's semantic atom stream so the two paths agree on every input.
pub fn coerce(run_class: AtomClass, prev: Option<AtomClass>) -> AtomClass {
    if run_class != AtomClass::Bin {
        return run_class;
    }
    if coerces_binary_to_ordinary(prev.map(math_class)) {
        AtomClass::Ord
    } else {
        AtomClass::Bin
    }
}

/// Map a formatter spacing class back onto the parser's atom class.
fn math_class(class: AtomClass) -> MathClass {
    match class {
        AtomClass::Ord => MathClass::Ord,
        AtomClass::Bin => MathClass::Bin,
        AtomClass::Rel => MathClass::Rel,
        AtomClass::Open => MathClass::Open,
        AtomClass::Close => MathClass::Close,
        AtomClass::Punct => MathClass::Punct,
        AtomClass::Op => MathClass::Op,
    }
}

/// Whether an already-coerced operator class takes one space on each side. Only
/// binary operators and relations do; a coerced (unary) operator is tight.
pub fn is_spaced(class: AtomClass) -> bool {
    matches!(class, AtomClass::Bin | AtomClass::Rel)
}

/// Break priority of an (already-coerced) atom class for semantic
/// line-breaking: higher = break here first. A long display row breaks at its
/// highest-priority depth-0 operators before any lower ones. Relations outrank
/// binary operators — the TeX/amsmath convention is to break a long chain at
/// its relations (`a = b = c`), keeping binary sub-terms together. Everything
/// else is `0` (never a break site: ordinary atoms, delimiters, punctuation,
/// large operators, and — crucially — a coerced *unary* `+`/`-`, which is
/// [`AtomClass::Ord`] by the time it reaches here).
pub fn break_priority(class: AtomClass) -> u8 {
    match class {
        AtomClass::Rel => 2,
        AtomClass::Bin => 1,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_atoms_slice_lexical_runs_without_changing_the_cst() {
        assert_eq!(
            word_atoms("f(-x):=a+b,y<=z").collect::<Vec<_>>(),
            vec![
                WordAtom::new("f", AtomClass::Ord),
                WordAtom::new("(", AtomClass::Open),
                WordAtom::new("-", AtomClass::Bin),
                WordAtom::new("x", AtomClass::Ord),
                WordAtom::new(")", AtomClass::Close),
                WordAtom::new(":=", AtomClass::Rel),
                WordAtom::new("a", AtomClass::Ord),
                WordAtom::new("+", AtomClass::Bin),
                WordAtom::new("b", AtomClass::Ord),
                WordAtom::new(",", AtomClass::Punct),
                WordAtom::new("y", AtomClass::Ord),
                WordAtom::new("<=", AtomClass::Rel),
                WordAtom::new("z", AtomClass::Ord),
            ]
        );
        assert_eq!(
            word_atoms("α+β").collect::<Vec<_>>(),
            vec![
                WordAtom::new("α", AtomClass::Ord),
                WordAtom::new("+", AtomClass::Bin),
                WordAtom::new("β", AtomClass::Ord),
            ]
        );
        assert_eq!(
            word_atoms("a::=b").collect::<Vec<_>>(),
            vec![
                WordAtom::new("a", AtomClass::Ord),
                WordAtom::new("::=", AtomClass::Rel),
                WordAtom::new("b", AtomClass::Ord),
            ]
        );
    }

    #[test]
    fn command_table_lookups() {
        assert_eq!(command_class("leq"), Some(AtomClass::Rel));
        assert_eq!(command_class("cdot"), Some(AtomClass::Bin));
        assert_eq!(command_class("sum"), Some(AtomClass::Op));
        assert_eq!(command_class("left"), None);
        assert_eq!(command_class("right"), None);
        assert_eq!(command_class("alpha"), None);
        assert_eq!(command_class("frac"), None);
        assert_eq!(command_class("text"), None);
    }

    #[test]
    fn text_mode_command_set() {
        assert!(is_text_mode_command("text"));
        assert!(is_text_mode_command("textbf"));
        assert!(is_text_mode_command("mbox"));
        assert!(!is_text_mode_command("mathrm"));
        assert!(!is_text_mode_command("mathbf"));
        assert!(!is_text_mode_command("frac"));
        assert!(!is_text_mode_command("alpha"));
        assert!(!is_text_mode_command("textcolor"));
    }

    #[test]
    fn bin_coerces_to_unary_in_unary_positions() {
        assert_eq!(coerce(AtomClass::Bin, None), AtomClass::Ord);
        assert_eq!(
            coerce(AtomClass::Bin, Some(AtomClass::Open)),
            AtomClass::Ord
        );
        assert_eq!(coerce(AtomClass::Bin, Some(AtomClass::Rel)), AtomClass::Ord);
        assert_eq!(coerce(AtomClass::Bin, Some(AtomClass::Bin)), AtomClass::Ord);
        assert_eq!(
            coerce(AtomClass::Bin, Some(AtomClass::Punct)),
            AtomClass::Ord
        );
        assert_eq!(coerce(AtomClass::Bin, Some(AtomClass::Op)), AtomClass::Ord);
        assert_eq!(coerce(AtomClass::Bin, Some(AtomClass::Ord)), AtomClass::Bin);
        assert_eq!(
            coerce(AtomClass::Bin, Some(AtomClass::Close)),
            AtomClass::Bin
        );
        assert_eq!(coerce(AtomClass::Rel, None), AtomClass::Rel);
        assert_eq!(
            coerce(AtomClass::Rel, Some(AtomClass::Open)),
            AtomClass::Rel
        );
    }

    #[test]
    fn spacing_predicate() {
        assert!(is_spaced(AtomClass::Bin));
        assert!(is_spaced(AtomClass::Rel));
        assert!(!is_spaced(AtomClass::Ord));
        assert!(!is_spaced(AtomClass::Open));
        assert!(!is_spaced(AtomClass::Close));
        assert!(!is_spaced(AtomClass::Punct));
        assert!(!is_spaced(AtomClass::Op));
    }

    #[test]
    fn break_priority_ranks_rel_over_bin_over_rest() {
        assert!(break_priority(AtomClass::Rel) > break_priority(AtomClass::Bin));
        assert!(break_priority(AtomClass::Bin) > break_priority(AtomClass::Ord));
        for class in [
            AtomClass::Ord,
            AtomClass::Open,
            AtomClass::Close,
            AtomClass::Punct,
            AtomClass::Op,
        ] {
            assert_eq!(break_priority(class), 0, "{class:?}");
        }
        assert_eq!(
            break_priority(AtomClass::Rel) > 0,
            is_spaced(AtomClass::Rel)
        );
        assert_eq!(
            break_priority(AtomClass::Bin) > 0,
            is_spaced(AtomClass::Bin)
        );
    }
}

//! Math operator *interpretation* — the analog of YAML scalar cooking
//! ([`panache_parser::parser::yaml`]'s `cooking.rs`).
//!
//! The parser emits a *neutral* `MATH_OPERATOR` token (one per char of
//! `+ - * = < >`) and never tags bin/rel or builds a precedence tree: TeX
//! assigns an atom's class contextually during mlist→hlist (a Bin atom after
//! Bin/Rel/Open/Punct becomes Ord — that *is* unary minus), it is
//! override-able (`\mathbin`) and macro-dependent. So class/precedence is a
//! pure *interpretation* shared between consumers, not a CST shape — it lives
//! here, keyed on operator text and command name, never in `MATH_*` kinds.
//!
//! This module is intentionally `pub` so the LSP can reuse it later (semantic
//! tokens / hover). Today only the formatter's spacing pass
//! ([`super::render`]) consumes it.
//!
//! Scope of *this* slice (Phase 5): classify the char operators and apply
//! precedence-aware spacing to them. The command table below is consumed for
//! the *preceding-atom* class (so a `-` after `\leq` reads as unary), but
//! command operators are not themselves re-spaced yet — that, and a
//! break-priority column for semantic line-breaking, are Phase 5b/6. The table
//! is a plain `match` so extending it stays trivial.

use crate::syntax::SyntaxKind;

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

/// Split a run of consecutive `MATH_OPERATOR` chars into operator *atoms*: a
/// maximal sub-run of relation chars (`= < >`) is one composite relation (so
/// `<=`, `>=`, `==` stay one spaced unit, not `< =`), while each sign char
/// (`+ - *`) is its own atom — because a sign is contextually unary. Thus
/// `=-` splits into `=` (relation) and `-` (a sign that coerces to unary after
/// the relation), giving `x = -y` rather than `x =- y`. (`->` likewise splits
/// into the binary `-` and the relation `>` — TeX-faithful, since `->` is not a
/// real arrow command.)
pub fn split_operator_atoms(run: &str) -> Vec<&str> {
    let bytes = run.as_bytes();
    let mut atoms: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if matches!(bytes[i], b'=' | b'<' | b'>') {
            let start = i;
            while i < bytes.len() && matches!(bytes[i], b'=' | b'<' | b'>') {
                i += 1;
            }
            atoms.push(&run[start..i]);
        } else {
            atoms.push(&run[i..i + 1]); // a single `+ - *` sign char
            i += 1;
        }
    }
    atoms
}

/// Classify a single operator atom (from [`split_operator_atoms`]): any of
/// `= < >` makes it a [`AtomClass::Rel`], otherwise it is a [`AtomClass::Bin`].
pub fn classify_operator(atom: &str) -> AtomClass {
    if atom.bytes().any(|b| matches!(b, b'=' | b'<' | b'>')) {
        AtomClass::Rel
    } else {
        AtomClass::Bin
    }
}

/// Class of a command operator, keyed on its name **without** the leading
/// backslash. `None` for any command not in the curated table — the caller
/// treats those as [`AtomClass::Ord`] (Greek letters, `\frac`, `\text`, …).
///
/// Only standard TeX/KaTeX symbols are listed (the cross-validation corpus is
/// KaTeX-bounded). Extend freely; the `Op` arm is deliberately conservative.
pub fn command_class(name: &str) -> Option<AtomClass> {
    let class = match name {
        // Relations.
        "leq" | "le" | "geq" | "ge" | "neq" | "ne" | "equiv" | "approx" | "sim" | "simeq"
        | "cong" | "propto" | "subset" | "supset" | "subseteq" | "supseteq" | "in" | "ni"
        | "notin" | "to" | "gets" | "mapsto" | "rightarrow" | "leftarrow" | "leftrightarrow"
        | "Rightarrow" | "Leftarrow" | "Leftrightarrow" | "implies" | "iff" | "perp"
        | "parallel" | "mid" | "models" | "vdash" | "dashv" | "prec" | "succ" | "preceq"
        | "succeq" | "ll" | "gg" | "doteq" | "asymp" | "coloneqq" => AtomClass::Rel,
        // Binary operators.
        "cdot" | "times" | "div" | "pm" | "mp" | "ast" | "star" | "circ" | "bullet" | "oplus"
        | "ominus" | "otimes" | "oslash" | "odot" | "cap" | "cup" | "uplus" | "sqcap" | "sqcup"
        | "wedge" | "vee" | "setminus" | "amalg" => AtomClass::Bin,
        // Large operators: Ord-like for spacing, but a following `+`/`-` is unary.
        "sum" | "prod" | "int" | "oint" | "coprod" | "bigcup" | "bigcap" | "bigoplus"
        | "bigotimes" | "bigvee" | "bigwedge" | "lim" => AtomClass::Op,
        // Delimiter commands (defensive; the common `\left(`/`\right)` path is
        // already covered by the `(`/`)` MATH_OPEN/MATH_CLOSE tokens).
        "left" => AtomClass::Open,
        "right" => AtomClass::Close,
        _ => return None,
    };
    Some(class)
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

/// Atom class of a delimiter/punctuation **token kind**. The parser tokenizes
/// the unambiguous delimiters into dedicated kinds (`( [` → `MATH_OPEN`,
/// `) ]` → `MATH_CLOSE`, `, ;` → `MATH_PUNCT`) because their TeX mathcode class
/// is fixed at the character level — a CST fact, not the contextual
/// *interpretation* operator class is. This maps those kinds onto the
/// formatter's [`AtomClass`]: an opening delimiter makes a following `+`/`-`
/// unary (`f(-x)`), a closing one is a binary-inducing operand, and `,`/`;` are
/// punctuation. Returns `None` for any non-delimiter kind (the caller treats
/// those as ordinary or handles them specially).
pub fn delimiter_class(kind: SyntaxKind) -> Option<AtomClass> {
    Some(match kind {
        SyntaxKind::MATH_OPEN => AtomClass::Open,
        SyntaxKind::MATH_CLOSE => AtomClass::Close,
        SyntaxKind::MATH_PUNCT => AtomClass::Punct,
        _ => return None,
    })
}

/// TeX Bin→Ord coercion (the rule that yields unary minus): a [`AtomClass::Bin`]
/// run becomes [`AtomClass::Ord`] when the preceding atom is absent (list start)
/// or one of Bin/Rel/Open/Punct/Op. [`AtomClass::Rel`] never coerces.
pub fn coerce(run_class: AtomClass, prev: Option<AtomClass>) -> AtomClass {
    if run_class != AtomClass::Bin {
        return run_class;
    }
    match prev {
        None
        | Some(
            AtomClass::Bin | AtomClass::Rel | AtomClass::Open | AtomClass::Punct | AtomClass::Op,
        ) => AtomClass::Ord,
        _ => AtomClass::Bin,
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
    fn operator_runs_split_into_atoms() {
        // Single operators are one atom.
        assert_eq!(split_operator_atoms("+"), vec!["+"]);
        assert_eq!(split_operator_atoms("="), vec!["="]);
        // Adjacent relation chars merge into one composite relation.
        assert_eq!(split_operator_atoms("<="), vec!["<="]);
        assert_eq!(split_operator_atoms("=="), vec!["=="]);
        // A sign char is always its own atom (so it can be unary).
        assert_eq!(split_operator_atoms("=-"), vec!["=", "-"]);
        assert_eq!(split_operator_atoms("->"), vec!["-", ">"]);
        assert_eq!(split_operator_atoms("--"), vec!["-", "-"]);
        assert_eq!(split_operator_atoms("=-="), vec!["=", "-", "="]);
    }

    #[test]
    fn operator_atoms_classify_bin_vs_rel() {
        assert_eq!(classify_operator("+"), AtomClass::Bin);
        assert_eq!(classify_operator("-"), AtomClass::Bin);
        assert_eq!(classify_operator("*"), AtomClass::Bin);
        assert_eq!(classify_operator("="), AtomClass::Rel);
        assert_eq!(classify_operator("<"), AtomClass::Rel);
        assert_eq!(classify_operator(">"), AtomClass::Rel);
        assert_eq!(classify_operator("<="), AtomClass::Rel);
        assert_eq!(classify_operator("=="), AtomClass::Rel);
    }

    #[test]
    fn command_table_lookups() {
        assert_eq!(command_class("leq"), Some(AtomClass::Rel));
        assert_eq!(command_class("cdot"), Some(AtomClass::Bin));
        assert_eq!(command_class("sum"), Some(AtomClass::Op));
        assert_eq!(command_class("left"), Some(AtomClass::Open));
        assert_eq!(command_class("right"), Some(AtomClass::Close));
        // Unknown / ordinary commands fall through to None (caller → Ord).
        assert_eq!(command_class("alpha"), None);
        assert_eq!(command_class("frac"), None);
        assert_eq!(command_class("text"), None);
    }

    #[test]
    fn text_mode_command_set() {
        // Text-switching commands → true (interior whitespace is significant).
        assert!(is_text_mode_command("text"));
        assert!(is_text_mode_command("textbf"));
        assert!(is_text_mode_command("mbox"));
        // Math-mode font commands and ordinary commands → false.
        assert!(!is_text_mode_command("mathrm"));
        assert!(!is_text_mode_command("mathbf"));
        assert!(!is_text_mode_command("frac"));
        assert!(!is_text_mode_command("alpha"));
        // Multi-argument `\textcolor` is intentionally excluded.
        assert!(!is_text_mode_command("textcolor"));
    }

    #[test]
    fn delimiter_kind_classification() {
        assert_eq!(
            delimiter_class(SyntaxKind::MATH_OPEN),
            Some(AtomClass::Open)
        );
        assert_eq!(
            delimiter_class(SyntaxKind::MATH_CLOSE),
            Some(AtomClass::Close)
        );
        assert_eq!(
            delimiter_class(SyntaxKind::MATH_PUNCT),
            Some(AtomClass::Punct)
        );
        // Non-delimiter kinds are not the delimiter table's concern.
        assert_eq!(delimiter_class(SyntaxKind::MATH_TEXT), None);
        assert_eq!(delimiter_class(SyntaxKind::MATH_OPERATOR), None);
    }

    #[test]
    fn bin_coerces_to_unary_in_unary_positions() {
        // Unary positions → Ord (tight).
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
        // Binary positions stay Bin.
        assert_eq!(coerce(AtomClass::Bin, Some(AtomClass::Ord)), AtomClass::Bin);
        assert_eq!(
            coerce(AtomClass::Bin, Some(AtomClass::Close)),
            AtomClass::Bin
        );
        // Relations never coerce.
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
        // Relations break first, then binary operators.
        assert!(break_priority(AtomClass::Rel) > break_priority(AtomClass::Bin));
        assert!(break_priority(AtomClass::Bin) > break_priority(AtomClass::Ord));
        // Everything that is not a binary/relation operator is never a break
        // site (priority 0) — including delimiters, punctuation, and large ops.
        for class in [
            AtomClass::Ord,
            AtomClass::Open,
            AtomClass::Close,
            AtomClass::Punct,
            AtomClass::Op,
        ] {
            assert_eq!(break_priority(class), 0, "{class:?}");
        }
        // Only spaced classes are break sites.
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

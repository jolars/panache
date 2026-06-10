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
        | "succeq" | "ll" | "gg" | "doteq" | "asymp" => AtomClass::Rel,
        // Binary operators.
        "cdot" | "times" | "div" | "pm" | "mp" | "ast" | "star" | "circ" | "bullet" | "oplus"
        | "ominus" | "otimes" | "oslash" | "odot" | "cap" | "cup" | "uplus" | "sqcap" | "sqcup"
        | "wedge" | "vee" | "setminus" | "amalg" => AtomClass::Bin,
        // Large operators: Ord-like for spacing, but a following `+`/`-` is unary.
        "sum" | "prod" | "int" | "oint" | "coprod" | "bigcup" | "bigcap" | "bigoplus"
        | "bigotimes" | "bigvee" | "bigwedge" | "lim" => AtomClass::Op,
        // Delimiter commands (defensive; the common `\left(`/`\right)` path is
        // already covered by the `(`/`)` text tail).
        "left" => AtomClass::Open,
        "right" => AtomClass::Close,
        _ => return None,
    };
    Some(class)
}

/// Class of a `MATH_TEXT` run, derived from its **last non-space char**: an
/// opening bracket makes a following `+`/`-` unary (`f(-x)`, since `(` is lumped
/// into the `f(` text run), a closing bracket is a binary-inducing operand, and
/// `,`/`;` are punctuation. Everything else (letters, digits, `.`) is ordinary.
pub fn text_tail_class(text: &str) -> AtomClass {
    match text.trim_end_matches([' ', '\t']).chars().next_back() {
        Some('(' | '[') => AtomClass::Open,
        Some(')' | ']') => AtomClass::Close,
        Some(',' | ';') => AtomClass::Punct,
        _ => AtomClass::Ord,
    }
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
    fn text_tail_classification() {
        assert_eq!(text_tail_class("f("), AtomClass::Open);
        assert_eq!(text_tail_class("x["), AtomClass::Open);
        assert_eq!(text_tail_class("(a)"), AtomClass::Close);
        assert_eq!(text_tail_class("x]"), AtomClass::Close);
        assert_eq!(text_tail_class("a,"), AtomClass::Punct);
        assert_eq!(text_tail_class("a;"), AtomClass::Punct);
        assert_eq!(text_tail_class("ab"), AtomClass::Ord);
        assert_eq!(text_tail_class("2.5"), AtomClass::Ord);
        // Trailing whitespace is ignored when reading the tail char.
        assert_eq!(text_tail_class("f( "), AtomClass::Open);
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
}

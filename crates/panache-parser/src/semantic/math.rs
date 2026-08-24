//! Conservative semantic facts for TeX math content.
//!
//! These facts remain separate from the CST because a visible macro definition
//! may replace any built-in meaning. The built-in table covers the initial
//! commands whose arguments establish math or text domains, while a document
//! overlay suppresses those facts for names redefined in raw TeX.

use std::collections::HashSet;

use rowan::{TextRange, TextSize};

use crate::syntax::{SyntaxElement, SyntaxKind, SyntaxNode, SyntaxToken};

/// The useful TeX math-atom class family.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MathClass {
    #[default]
    Ord,
    Op,
    Bin,
    Rel,
    Open,
    Close,
    Punct,
    Fence,
    Inner,
}

/// Whether an atom is a genuinely pairable delimiter, independently of its
/// TeX spacing class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DelimiterRole {
    Open,
    Close,
    Fence,
}

/// Math-atom metadata without a source location.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MathAtomInfo {
    pub class: MathClass,
    pub delimiter: Option<DelimiterRole>,
}

/// One virtual math atom and the exact source bytes that produced it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MathAtom {
    pub range: TextRange,
    pub class: MathClass,
    pub delimiter: Option<DelimiterRole>,
}

const fn atom_info(class: MathClass, delimiter: Option<DelimiterRole>) -> MathAtomInfo {
    MathAtomInfo { class, delimiter }
}

type MathCommandMap = phf::Map<&'static str, MathAtomInfo>;

include!(concat!(env!("OUT_DIR"), "/math_symbols.rs"));

/// LaTeX and amsmath's named, upright function operators.
pub const NAMED_MATH_OPERATORS: &[&str] = &[
    "arccos", "arcsin", "arctan", "arg", "cos", "cosh", "cot", "coth", "csc", "deg", "det", "dim",
    "exp", "gcd", "hom", "inf", "ker", "lg", "lim", "liminf", "limsup", "ln", "log", "max", "min",
    "Pr", "sec", "sin", "sinh", "sup", "tan", "tanh",
];

/// Classify a control-sequence name without its leading backslash.
///
/// Panache's curated overrides take precedence over the generated unicode-math
/// baseline. Unknown commands conservatively behave as ordinary atoms.
pub fn math_command_info(name: &str) -> MathAtomInfo {
    curated_command_info(name)
        .or_else(|| UNICODE_MATH_COMMANDS.get(name).copied())
        .unwrap_or_default()
}

/// Classify a literal Unicode scalar.
///
/// Panache's curated overrides take precedence over the generated unicode-math
/// baseline. Unknown characters conservatively behave as ordinary atoms.
pub fn math_char_info(character: char) -> MathAtomInfo {
    curated_char_info(character)
        .or_else(|| {
            UNICODE_MATH_CHARS
                .binary_search_by_key(&character, |(candidate, _)| *candidate)
                .ok()
                .map(|index| UNICODE_MATH_CHARS[index].1)
        })
        .unwrap_or_default()
}

/// Virtual atoms for one CST element. A coalesced `MATH_WORD` yields one atom
/// per Unicode scalar; structural nodes remain one source-spanning atom.
pub fn math_atoms(element: &SyntaxElement) -> MathAtoms<'_> {
    match element {
        SyntaxElement::Token(token) if token.kind() == SyntaxKind::MATH_WORD => MathAtoms {
            inner: MathAtomsInner::Word {
                text: token.text(),
                start: token.text_range().start(),
                offset: 0,
            },
        },
        SyntaxElement::Token(token) => MathAtoms {
            inner: MathAtomsInner::One(Some(atom(token.text_range(), token_info(token)))),
        },
        SyntaxElement::Node(node) => MathAtoms {
            inner: MathAtomsInner::One(Some(atom(node.text_range(), node_info(node)))),
        },
    }
}

/// Iterator returned by [`math_atoms`].
pub struct MathAtoms<'a> {
    inner: MathAtomsInner<'a>,
}

enum MathAtomsInner<'a> {
    Word {
        text: &'a str,
        start: TextSize,
        offset: usize,
    },
    One(Option<MathAtom>),
}

impl Iterator for MathAtoms<'_> {
    type Item = MathAtom;

    fn next(&mut self) -> Option<Self::Item> {
        match &mut self.inner {
            MathAtomsInner::One(atom) => atom.take(),
            MathAtomsInner::Word {
                text,
                start,
                offset,
            } => {
                let character = text.get(*offset..)?.chars().next()?;
                let len = character.len_utf8();
                let atom_start = *start + TextSize::from(*offset as u32);
                *offset += len;
                let atom_end = *start + TextSize::from(*offset as u32);
                let value = math_char_info(character);
                Some(MathAtom {
                    range: TextRange::new(atom_start, atom_end),
                    class: value.class,
                    delimiter: value.delimiter,
                })
            }
        }
    }
}

fn atom(range: TextRange, value: MathAtomInfo) -> MathAtom {
    MathAtom {
        range,
        class: value.class,
        delimiter: value.delimiter,
    }
}

fn token_info(token: &SyntaxToken) -> MathAtomInfo {
    match token.kind() {
        SyntaxKind::MATH_CONTROL_WORD | SyntaxKind::MATH_CONTROL_SYMBOL => token
            .text()
            .strip_prefix('\\')
            .map_or_else(MathAtomInfo::default, math_command_info),
        _ => {
            let mut characters = token.text().chars();
            match (characters.next(), characters.next()) {
                (Some(character), None) => math_char_info(character),
                _ => MathAtomInfo::default(),
            }
        }
    }
}

fn node_info(node: &SyntaxNode) -> MathAtomInfo {
    match node.kind() {
        SyntaxKind::MATH_COMMAND => node
            .children_with_tokens()
            .filter_map(SyntaxElement::into_token)
            .find(|token| {
                matches!(
                    token.kind(),
                    SyntaxKind::MATH_CONTROL_WORD | SyntaxKind::MATH_CONTROL_SYMBOL
                )
            })
            .as_ref()
            .map_or_else(MathAtomInfo::default, token_info),
        SyntaxKind::MATH_SCRIPTED => node
            .children_with_tokens()
            .find(|element| {
                !matches!(
                    element.kind(),
                    SyntaxKind::MATH_SPACE
                        | SyntaxKind::MATH_NEWLINE
                        | SyntaxKind::MATH_SUBSCRIPT
                        | SyntaxKind::MATH_SUPERSCRIPT
                        | SyntaxKind::LINE_PREFIX
                        | SyntaxKind::NEWLINE
                )
            })
            .and_then(|base| math_atoms(&base).next())
            .map_or_else(MathAtomInfo::default, |base| {
                atom_info(base.class, base.delimiter)
            }),
        SyntaxKind::MATH_GROUP
        | SyntaxKind::MATH_OPTIONAL
        | SyntaxKind::MATH_DELIMITED
        | SyntaxKind::MATH_ENVIRONMENT => atom_info(MathClass::Inner, None),
        _ => MathAtomInfo::default(),
    }
}

fn curated_char_info(character: char) -> Option<MathAtomInfo> {
    match character {
        '*' | '-' => Some(atom_info(MathClass::Bin, None)),
        '!' => Some(atom_info(MathClass::Close, None)),
        '√' => Some(atom_info(MathClass::Ord, None)),
        '∛' | '∜' | '⟌' => Some(atom_info(MathClass::Open, None)),
        _ => None,
    }
}

fn curated_command_info(name: &str) -> Option<MathAtomInfo> {
    if NAMED_MATH_OPERATORS.contains(&name) {
        return Some(atom_info(MathClass::Op, None));
    }
    let value = match name {
        "operatorname" => atom_info(MathClass::Op, None),
        "mathord" => atom_info(MathClass::Ord, None),
        "mathop" => atom_info(MathClass::Op, None),
        "mathbin" => atom_info(MathClass::Bin, None),
        "mathrel" => atom_info(MathClass::Rel, None),
        "mathopen" => atom_info(MathClass::Open, None),
        "mathclose" => atom_info(MathClass::Close, None),
        "mathpunct" => atom_info(MathClass::Punct, None),
        "mathinner" => atom_info(MathClass::Inner, None),
        "le" | "leq" | "ge" | "geq" | "ne" | "neq" | "equiv" | "approx" | "approxeq" | "sim"
        | "simeq" | "cong" | "propto" | "asymp" | "doteq" | "models" | "vdash" | "dashv"
        | "perp" | "parallel" | "mid" | "in" | "ni" | "notin" | "subset" | "subseteq"
        | "subsetneq" | "supset" | "supseteq" | "supsetneq" | "sqsubseteq" | "sqsupseteq"
        | "prec" | "preceq" | "succ" | "succeq" | "ll" | "gg" | "lll" | "ggg" | "to"
        | "rightarrow" | "longrightarrow" | "Rightarrow" | "Longrightarrow" | "implies"
        | "impliedby" | "iff" | "mapsto" | "longmapsto" | "leftarrow" | "Leftarrow" | "gets"
        | "leftrightarrow" | "Leftrightarrow" | "Longleftrightarrow" | "hookrightarrow"
        | "hookleftarrow" | "triangleq" | "coloneq" | "Coloneq" | "coloneqq" | "Coloneqq"
        | "eqcolon" | "Eqcolon" | "eqqcolon" | "Eqqcolon" | "colonapprox" | "Colonapprox"
        | "colonsim" | "Colonsim" | "lesssim" | "gtrsim" => atom_info(MathClass::Rel, None),
        "pm" | "mp" | "times" | "div" | "cdot" | "ast" | "star" | "circ" | "bullet" | "cup"
        | "cap" | "uplus" | "sqcup" | "sqcap" | "vee" | "wedge" | "lor" | "land" | "oplus"
        | "ominus" | "otimes" | "oslash" | "odot" | "setminus" | "amalg" | "diamond" | "wr"
        | "dagger" | "ddagger" | "bigtriangleup" | "bigtriangledown" | "triangleleft"
        | "triangleright" => atom_info(MathClass::Bin, None),
        "{" | "lvert" | "lVert" => atom_info(MathClass::Open, Some(DelimiterRole::Open)),
        "}" | "rvert" | "rVert" => atom_info(MathClass::Close, Some(DelimiterRole::Close)),
        "|" => atom_info(MathClass::Fence, Some(DelimiterRole::Fence)),
        "sqrt" | "cuberoot" | "fourthroot" | "longdivision" => atom_info(MathClass::Open, None),
        "mathexclam" => atom_info(MathClass::Close, None),
        _ => return None,
    };
    Some(value)
}

/// The delimiter shape of an attached command argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArgKind {
    Brace,
    Bracket,
}

/// The TeX domain a command argument is known to establish.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ArgumentDomain {
    #[default]
    Unknown,
    Math,
    Text,
}

/// One positional argument in a command signature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArgSpec {
    pub required: bool,
    pub kind: ArgKind,
    pub domain: ArgumentDomain,
}

/// The semantic argument signature of a built-in command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandSignature {
    pub arguments: &'static [ArgSpec],
}

/// Document-provided command definitions layered over built-in signatures.
///
/// Panache does not expand replacement bodies. A recognized definition therefore
/// suppresses built-in argument-domain knowledge instead of attempting to infer
/// the replacement command's meaning.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SignatureScope {
    redefined_commands: HashSet<String>,
}

impl SignatureScope {
    /// Collect command definitions from raw TeX nodes in a parsed document.
    pub fn from_root(root: &SyntaxNode) -> Self {
        let mut scope = Self::default();
        for node in root.descendants().filter(|node| {
            matches!(
                node.kind(),
                SyntaxKind::TEX_BLOCK | SyntaxKind::LATEX_COMMAND
            )
        }) {
            collect_definition_targets(&node.text().to_string(), &mut scope.redefined_commands);
        }
        scope
    }

    /// Whether a document definition shadows the built-in command name.
    pub fn is_redefined(&self, name: &str) -> bool {
        self.redefined_commands.contains(name)
    }

    /// Resolve a command against the document overlay, then the built-in table.
    pub fn command_signature(&self, name: &str) -> Option<&'static CommandSignature> {
        if self.is_redefined(name) {
            None
        } else {
            builtin_command_signature(name)
        }
    }
}

const fn argument(required: bool, kind: ArgKind, domain: ArgumentDomain) -> ArgSpec {
    ArgSpec {
        required,
        kind,
        domain,
    }
}

const REQUIRED_MATH: ArgSpec = argument(true, ArgKind::Brace, ArgumentDomain::Math);
const OPTIONAL_MATH: ArgSpec = argument(false, ArgKind::Bracket, ArgumentDomain::Math);
const REQUIRED_TEXT: ArgSpec = argument(true, ArgKind::Brace, ArgumentDomain::Text);

const ONE_MATH: CommandSignature = CommandSignature {
    arguments: &[REQUIRED_MATH],
};
const TWO_MATH: CommandSignature = CommandSignature {
    arguments: &[REQUIRED_MATH, REQUIRED_MATH],
};
const OPTIONAL_AND_REQUIRED_MATH: CommandSignature = CommandSignature {
    arguments: &[OPTIONAL_MATH, REQUIRED_MATH],
};
const ONE_TEXT: CommandSignature = CommandSignature {
    arguments: &[REQUIRED_TEXT],
};

/// Return the curated signature for a built-in command in the initial
/// math-domain slice.
///
/// Names do not include the leading backslash. Unknown commands deliberately
/// return `None`, so consumers preserve their arguments without interpreting
/// the contents.
pub fn builtin_command_signature(name: &str) -> Option<&'static CommandSignature> {
    match name {
        "frac" => Some(&TWO_MATH),
        "sqrt" => Some(&OPTIONAL_AND_REQUIRED_MATH),
        "ensuremath" | "mathrm" | "mathsf" | "mathbf" | "mathit" | "mathtt" | "mathnormal"
        | "mathcal" | "mathbb" | "mathfrak" | "mathscr" | "operatorname" => Some(&ONE_MATH),
        "text" | "mbox" | "intertext" => Some(&ONE_TEXT),
        _ => None,
    }
}

/// Match an attached group to the next positional signature slot.
///
/// Omitted optional slots are skipped. A mismatched group does not consume a
/// pending required slot.
pub fn match_arg_slot(arguments: &[ArgSpec], slot: &mut usize, kind: ArgKind) -> Option<ArgSpec> {
    while *slot < arguments.len() {
        let argument = arguments[*slot];
        if argument.kind == kind {
            *slot += 1;
            return Some(argument);
        }
        if !argument.required {
            *slot += 1;
            continue;
        }
        return None;
    }
    None
}

/// Return the curated positional domain of an attached math argument.
///
/// Unowned, unmatched, over-attached, and unknown-command groups are unknown.
pub fn argument_domain(group: &SyntaxNode) -> ArgumentDomain {
    let root = group.ancestors().last().unwrap_or_else(|| group.clone());
    let scope = SignatureScope::from_root(&root);
    argument_domain_with_scope(group, &scope)
}

/// Return an attached math argument's domain using a precomputed document scope.
pub fn argument_domain_with_scope(group: &SyntaxNode, scope: &SignatureScope) -> ArgumentDomain {
    match group.kind() {
        SyntaxKind::MATH_GROUP | SyntaxKind::MATH_OPTIONAL => {}
        _ => return ArgumentDomain::Unknown,
    }
    let Some(owner) = group.parent() else {
        return ArgumentDomain::Unknown;
    };
    if owner.kind() != SyntaxKind::MATH_COMMAND {
        return ArgumentDomain::Unknown;
    }
    let Some(name) = command_name(&owner) else {
        return ArgumentDomain::Unknown;
    };
    let Some(signature) = scope.command_signature(&name) else {
        return ArgumentDomain::Unknown;
    };

    let mut slot = 0;
    for candidate in owner.children() {
        let candidate_kind = match candidate.kind() {
            SyntaxKind::MATH_GROUP => ArgKind::Brace,
            SyntaxKind::MATH_OPTIONAL => ArgKind::Bracket,
            _ => continue,
        };
        let domain = match_arg_slot(signature.arguments, &mut slot, candidate_kind)
            .map_or(ArgumentDomain::Unknown, |argument| argument.domain);
        if candidate == *group {
            return domain;
        }
    }
    ArgumentDomain::Unknown
}

const DEFINITION_COMMANDS: &[&str] = &[
    "newcommand",
    "renewcommand",
    "providecommand",
    "DeclareRobustCommand",
    "def",
    "edef",
    "gdef",
    "xdef",
    "NewDocumentCommand",
    "RenewDocumentCommand",
    "ProvideDocumentCommand",
    "DeclareDocumentCommand",
];

fn collect_definition_targets(text: &str, targets: &mut HashSet<String>) {
    let bytes = text.as_bytes();
    let mut line_start = 0;
    while line_start < bytes.len() {
        let mut cursor = line_start;
        while matches!(bytes.get(cursor), Some(b' ' | b'\t')) {
            cursor += 1;
        }
        if let Some((head, after_head)) = control_word(text, cursor)
            && DEFINITION_COMMANDS.contains(&head)
            && let Some(target) = definition_target(text, after_head)
        {
            targets.insert(target.to_owned());
        }
        line_start = text[line_start..]
            .find('\n')
            .map_or(bytes.len(), |offset| line_start + offset + 1);
    }
}

fn definition_target(text: &str, mut cursor: usize) -> Option<&str> {
    cursor = skip_tex_trivia(text, cursor);
    if text.as_bytes().get(cursor) == Some(&b'*') {
        cursor = skip_tex_trivia(text, cursor + 1);
    }
    if text.as_bytes().get(cursor) != Some(&b'{') {
        return control_word(text, cursor).map(|(name, _)| name);
    }

    cursor = skip_tex_trivia(text, cursor + 1);
    let (name, after_name) = control_word(text, cursor)?;
    cursor = skip_tex_trivia(text, after_name);
    (text.as_bytes().get(cursor) == Some(&b'}')).then_some(name)
}

fn control_word(text: &str, cursor: usize) -> Option<(&str, usize)> {
    let bytes = text.as_bytes();
    if bytes.get(cursor) != Some(&b'\\') {
        return None;
    }
    let start = cursor + 1;
    let mut end = start;
    while bytes
        .get(end)
        .is_some_and(|byte| byte.is_ascii_alphabetic() || matches!(byte, b'@' | b'_' | b':'))
    {
        end += 1;
    }
    (end > start).then(|| (&text[start..end], end))
}

fn skip_tex_trivia(text: &str, mut cursor: usize) -> usize {
    let bytes = text.as_bytes();
    loop {
        while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
            cursor += 1;
        }
        if bytes.get(cursor) != Some(&b'%') {
            return cursor;
        }
        cursor = text[cursor..]
            .find('\n')
            .map_or(bytes.len(), |offset| cursor + offset + 1);
    }
}

fn command_name(command: &SyntaxNode) -> Option<String> {
    command
        .children_with_tokens()
        .find_map(|element| match element {
            SyntaxElement::Token(token) if token.kind() == SyntaxKind::MATH_CONTROL_WORD => {
                token.text().strip_prefix('\\').map(str::to_owned)
            }
            _ => None,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_matching_skips_only_optional_arguments() {
        let mut slot = 0;
        assert_eq!(
            match_arg_slot(
                OPTIONAL_AND_REQUIRED_MATH.arguments,
                &mut slot,
                ArgKind::Brace,
            ),
            Some(REQUIRED_MATH),
        );
        assert_eq!(slot, 2);

        let mut slot = 0;
        assert_eq!(
            match_arg_slot(TWO_MATH.arguments, &mut slot, ArgKind::Bracket),
            None,
        );
        assert_eq!(slot, 0);
    }
}

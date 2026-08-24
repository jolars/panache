//! Conservative semantic facts for TeX math content.
//!
//! These facts remain separate from the CST because a visible macro definition
//! may replace any built-in meaning. This first slice covers the built-in
//! commands whose arguments establish math or text domains; document-provided
//! definitions will be layered over this table separately.

use crate::syntax::{SyntaxElement, SyntaxKind, SyntaxNode};

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
    let Some(signature) = builtin_command_signature(&name) else {
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

//! Differential oracle for the first native math-signature semantic slice.

use badness_parser::parser::parse as parse_badness;
use badness_parser::semantic::{
    ArgKind as BadnessArgKind, ArgumentDomain as BadnessDomain,
    argument_domain as badness_argument_domain, signature::builtin as badness_builtin,
};
use badness_parser::syntax::SyntaxKind as BadnessKind;
use panache_parser::parser::math::{MathParseOptions, parse_math_content};
use panache_parser::semantic::math::{
    ArgKind, ArgumentDomain, argument_domain, builtin_command_signature,
};
use panache_parser::syntax::{SyntaxKind, SyntaxNode};

fn badness_domains(body: &str) -> Vec<BadnessDomain> {
    let parsed = parse_badness(&format!("${body}$"));
    parsed
        .syntax()
        .descendants()
        .filter(|node| matches!(node.kind(), BadnessKind::GROUP | BadnessKind::OPTIONAL))
        .filter(|node| {
            node.parent()
                .is_some_and(|parent| parent.kind() == BadnessKind::COMMAND)
        })
        .map(|group| badness_argument_domain(&group))
        .collect()
}

fn panache_domains(body: &str) -> Vec<ArgumentDomain> {
    let root = SyntaxNode::new_root(parse_math_content(body, MathParseOptions::default()));
    root.descendants()
        .filter(|node| {
            matches!(
                node.kind(),
                SyntaxKind::MATH_GROUP | SyntaxKind::MATH_OPTIONAL
            )
        })
        .filter(|node| {
            node.parent()
                .is_some_and(|parent| parent.kind() == SyntaxKind::MATH_COMMAND)
        })
        .map(|group| argument_domain(&group))
        .collect()
}

#[test]
fn builtin_math_signature_slice_matches_badness() {
    const COMMANDS: &[&str] = &[
        "frac",
        "sqrt",
        "ensuremath",
        "mathrm",
        "mathsf",
        "mathbf",
        "mathit",
        "mathtt",
        "mathnormal",
        "mathcal",
        "mathbb",
        "mathfrak",
        "mathscr",
        "operatorname",
        "text",
        "mbox",
        "intertext",
    ];

    for name in COMMANDS {
        let panache = builtin_command_signature(name)
            .unwrap_or_else(|| panic!("missing Panache signature for `\\{name}`"));
        let badness = badness_builtin()
            .command(name)
            .unwrap_or_else(|| panic!("missing Badness signature for `\\{name}`"));
        assert_eq!(panache.arguments.len(), badness.args.len(), "\\{name}");
        for (panache, badness) in panache.arguments.iter().zip(badness.args.iter()) {
            assert_eq!(panache.required, badness.required, "\\{name}");
            assert_eq!(
                panache.kind,
                match badness.kind {
                    BadnessArgKind::Brace => ArgKind::Brace,
                    BadnessArgKind::Bracket => ArgKind::Bracket,
                },
                "\\{name}",
            );
            assert_eq!(
                panache.domain,
                match badness.domain {
                    BadnessDomain::Unknown => ArgumentDomain::Unknown,
                    BadnessDomain::Math => ArgumentDomain::Math,
                    BadnessDomain::Text => ArgumentDomain::Text,
                },
                "\\{name}",
            );
        }
    }
}

#[test]
fn attached_argument_domains_match_badness() {
    for body in [
        r"\frac{a}{b}",
        r"\sqrt{x}",
        r"\sqrt[3]{x}",
        r"\text{words}",
        r"\text{words}{extra}",
        r"\text{before \frac{a}{b} after}",
        r"\unknown{x}",
        r"\frac[a]{b}",
    ] {
        let panache = panache_domains(body);
        let badness = badness_domains(body)
            .into_iter()
            .map(|domain| match domain {
                BadnessDomain::Unknown => ArgumentDomain::Unknown,
                BadnessDomain::Math => ArgumentDomain::Math,
                BadnessDomain::Text => ArgumentDomain::Text,
            })
            .collect::<Vec<_>>();
        assert_eq!(panache, badness, "{body}");
    }
}

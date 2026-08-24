//! Differential oracle for the first native math-signature semantic slice.

use badness_parser::parser::parse as parse_badness;
use badness_parser::semantic::{
    ArgKind as BadnessArgKind, ArgumentDomain as BadnessDomain,
    argument_domain as badness_argument_domain, signature::builtin as badness_builtin,
};
use badness_parser::syntax::SyntaxKind as BadnessKind;
use panache_parser::parser::math::{MathParseOptions, parse_math_content};
use panache_parser::semantic::math::{
    ArgKind, ArgumentDomain, SignatureScope, argument_domain, builtin_command_signature,
};
use panache_parser::syntax::{SyntaxKind, SyntaxNode};
use panache_parser::{ParserOptions, parse};

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

fn badness_document_domains(source: &str) -> Vec<BadnessDomain> {
    let parsed = parse_badness(source);
    parsed
        .syntax()
        .descendants()
        .filter(|node| matches!(node.kind(), BadnessKind::GROUP | BadnessKind::OPTIONAL))
        .filter(|node| {
            node.parent()
                .is_some_and(|parent| parent.kind() == BadnessKind::COMMAND)
                && node
                    .ancestors()
                    .any(|ancestor| ancestor.kind() == BadnessKind::MATH)
        })
        .map(|group| badness_argument_domain(&group))
        .collect()
}

fn panache_document_domains(source: &str) -> Vec<ArgumentDomain> {
    let root = parse(source, Some(ParserOptions::default()));
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

#[test]
fn document_redefinitions_shadow_builtin_domains() {
    for source in [
        "\\renewcommand{\\frac}[2]{#1/#2}\n\n$\\frac{a}{b}$\n",
        "\\renewcommand\\frac[2]{#1/#2}\n\n$\\frac{a}{b}$\n",
        "\\def\\frac#1#2{#1/#2}\n\n$\\frac{a}{b}$\n",
        "\\RenewDocumentCommand{\\frac}{mm}{#1/#2}\n\n$\\frac{a}{b}$\n",
        "$\\frac{a}{b}$\n\n\\renewcommand{\\frac}[2]{#1/#2}\n",
    ] {
        let panache = panache_document_domains(source);
        let badness = badness_document_domains(source)
            .into_iter()
            .map(|domain| match domain {
                BadnessDomain::Unknown => ArgumentDomain::Unknown,
                BadnessDomain::Math => ArgumentDomain::Math,
                BadnessDomain::Text => ArgumentDomain::Text,
            })
            .collect::<Vec<_>>();
        assert_eq!(panache, badness, "{source}");
        assert_eq!(panache, vec![ArgumentDomain::Unknown; 2], "{source}");
    }
}

#[test]
fn definition_overlay_ignores_comments_and_malformed_targets() {
    let source = "% \\renewcommand{\\frac}[2]{#1/#2}\n\n$\\frac{a}{b}$\n";
    let root = parse(source, Some(ParserOptions::default()));
    let scope = SignatureScope::from_root(&root);
    assert!(!scope.is_redefined("frac"));
    assert_eq!(
        panache_document_domains(source),
        vec![ArgumentDomain::Math; 2],
    );

    let source = "\\renewcommand{frac}[2]{#1/#2}\n\n$\\frac{a}{b}$\n";
    let root = parse(source, Some(ParserOptions::default()));
    assert!(!SignatureScope::from_root(&root).is_redefined("frac"));
}

#[test]
fn blockquoted_raw_tex_redefinitions_are_visible() {
    let source = "> \\renewcommand{\\frac}[2]{#1/#2}\n>\n> $\\frac{a}{b}$\n";
    assert_eq!(
        panache_document_domains(source),
        vec![ArgumentDomain::Unknown; 2],
    );
}

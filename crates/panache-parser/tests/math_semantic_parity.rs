//! Differential oracle for the first native math-signature semantic slice.

use badness_parser::parser::parse as parse_badness;
use badness_parser::semantic::{
    ArgKind as BadnessArgKind, ArgumentDomain as BadnessDomain,
    DelimiterRole as BadnessDelimiterRole, MathClass as BadnessMathClass,
    argument_domain as badness_argument_domain, math_atoms as badness_math_atoms,
    math_char_info as badness_char_info, math_command_info as badness_command_info,
    signature::builtin as badness_builtin,
};
use badness_parser::syntax::SyntaxKind as BadnessKind;
use panache_parser::parser::math::{MathParseOptions, parse_math_content};
use panache_parser::semantic::math::{
    ArgKind, ArgumentDomain, DelimiterRole, MathClass, SignatureScope, argument_domain,
    builtin_command_signature, math_atoms, math_char_info, math_command_info,
};
use panache_parser::syntax::{SyntaxElement, SyntaxKind, SyntaxNode};
use panache_parser::{ParserOptions, parse};
use serde_json::Value;

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

fn class_from_badness(class: BadnessMathClass) -> MathClass {
    match class {
        BadnessMathClass::Ord => MathClass::Ord,
        BadnessMathClass::Op => MathClass::Op,
        BadnessMathClass::Bin => MathClass::Bin,
        BadnessMathClass::Rel => MathClass::Rel,
        BadnessMathClass::Open => MathClass::Open,
        BadnessMathClass::Close => MathClass::Close,
        BadnessMathClass::Punct => MathClass::Punct,
        BadnessMathClass::Fence => MathClass::Fence,
        BadnessMathClass::Inner => MathClass::Inner,
    }
}

fn delimiter_from_badness(role: BadnessDelimiterRole) -> DelimiterRole {
    match role {
        BadnessDelimiterRole::Open => DelimiterRole::Open,
        BadnessDelimiterRole::Close => DelimiterRole::Close,
        BadnessDelimiterRole::Fence => DelimiterRole::Fence,
    }
}

fn assert_atom_info_matches_badness(
    label: &str,
    panache: panache_parser::semantic::math::MathAtomInfo,
    badness: badness_parser::semantic::MathAtomInfo,
) {
    assert_eq!(panache.class, class_from_badness(badness.class), "{label}",);
    assert_eq!(
        panache.delimiter,
        badness.delimiter.map(delimiter_from_badness),
        "{label}",
    );
}

#[test]
fn generated_unicode_math_baseline_matches_badness_exhaustively() {
    let fixture: Value = serde_json::from_str(include_str!("../data/math_symbols.json"))
        .expect("math_symbols.json must be valid JSON");
    let symbols = fixture["symbols"]
        .as_array()
        .expect("math_symbols.json must contain a symbol array");
    assert_eq!(symbols.len(), 2_448);

    let mut characters = std::collections::BTreeSet::new();
    for symbol in symbols {
        let fields = symbol
            .as_array()
            .expect("each math symbol must be a four-field array");
        let codepoint = fields[0].as_str().expect("code point must be a string");
        let name = fields[1].as_str().expect("command must be a string");
        let character = char::from_u32(
            u32::from_str_radix(codepoint, 16).expect("code point must be hexadecimal"),
        )
        .expect("code point must be a Unicode scalar");

        assert_atom_info_matches_badness(
            &format!("\\{name}"),
            math_command_info(name),
            badness_command_info(name),
        );
        characters.insert(character);
    }

    for character in characters {
        assert_atom_info_matches_badness(
            &character.to_string(),
            math_char_info(character),
            badness_char_info(character),
        );
    }
}

#[test]
fn generated_unicode_math_baseline_covers_representative_shapes() {
    assert_eq!(math_command_info("nleq").class, MathClass::Rel);
    assert_eq!(math_char_info('≤').class, MathClass::Rel);
    assert_eq!(math_char_info(',').class, MathClass::Punct);
    assert_eq!(math_command_info("vert").class, MathClass::Fence);
    assert_eq!(
        math_command_info("vert").delimiter,
        Some(DelimiterRole::Fence),
    );
    assert_eq!(math_char_info('𞻰').class, MathClass::Op);
    assert_eq!(math_char_info('√').class, MathClass::Ord);
    assert_eq!(math_char_info('√').delimiter, None);
    assert_eq!(math_char_info('🦀'), Default::default());
    assert_eq!(math_command_info("not-a-real-command"), Default::default());
}

#[test]
fn curated_math_atom_info_matches_badness() {
    for character in ['*', '-', '!', '√', '∛', '∜', '⟌', '🦀'] {
        let panache = math_char_info(character);
        let badness = badness_char_info(character);
        assert_eq!(
            panache.class,
            class_from_badness(badness.class),
            "{character}"
        );
        assert_eq!(
            panache.delimiter,
            badness.delimiter.map(delimiter_from_badness),
            "{character}",
        );
    }

    for name in [
        "sin",
        "operatorname",
        "mathbin",
        "leq",
        "cdot",
        "lvert",
        "rvert",
        "|",
        "sqrt",
        "not-a-real-command",
    ] {
        let panache = math_command_info(name);
        let badness = badness_command_info(name);
        assert_eq!(panache.class, class_from_badness(badness.class), "\\{name}");
        assert_eq!(
            panache.delimiter,
            badness.delimiter.map(delimiter_from_badness),
            "\\{name}",
        );
    }
}

#[test]
fn math_atoms_match_badness_for_words_and_structural_atoms() {
    let body = r"a🦀-b \leq \}^{1/2} {x}";
    let panache_root = SyntaxNode::new_root(parse_math_content(body, MathParseOptions::default()));
    let badness_root = parse_badness(&format!("${body}$"));

    let panache_elements = panache_root
        .descendants_with_tokens()
        .filter(|element| {
            matches!(
                element.kind(),
                SyntaxKind::MATH_WORD
                    | SyntaxKind::MATH_COMMAND
                    | SyntaxKind::MATH_SCRIPTED
                    | SyntaxKind::MATH_GROUP
            )
        })
        .collect::<Vec<SyntaxElement>>();
    let badness_elements = badness_root
        .syntax()
        .descendants_with_tokens()
        .filter(|element| {
            matches!(
                element.kind(),
                BadnessKind::WORD
                    | BadnessKind::COMMAND
                    | BadnessKind::SCRIPTED
                    | BadnessKind::GROUP
            )
        })
        .collect::<Vec<_>>();

    assert_eq!(panache_elements.len(), badness_elements.len());
    for (panache_element, badness_element) in panache_elements.iter().zip(badness_elements.iter()) {
        let panache_start = panache_element.text_range().start();
        let badness_start = badness_element.text_range().start();
        let panache = math_atoms(panache_element)
            .map(|atom| {
                (
                    atom.range.start() - panache_start,
                    atom.range.end() - panache_start,
                    atom.class,
                    atom.delimiter,
                )
            })
            .collect::<Vec<_>>();
        let badness = badness_math_atoms(badness_element)
            .map(|atom| {
                (
                    atom.range.start() - badness_start,
                    atom.range.end() - badness_start,
                    class_from_badness(atom.class),
                    atom.delimiter.map(delimiter_from_badness),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(panache, badness, "{}", panache_element);
    }
}

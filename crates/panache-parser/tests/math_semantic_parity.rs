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
    ArgKind, ArgSpec, ArgumentDomain, CommandSignature, DelimiterRole, MathBreakPriority,
    MathClass, SignatureScope, argument_domain_with_scope, builtin_command_signature, math_atoms,
    math_char_info, math_command_info, semantic_math_atoms,
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
    let scope = SignatureScope::from_root(&root);
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
        .map(|group| argument_domain_with_scope(&group, &scope))
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
    let scope = SignatureScope::from_root(&root);
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
        .map(|group| argument_domain_with_scope(&group, &scope))
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

#[test]
fn configured_signatures_override_builtins_and_add_commands() {
    let root = parse(
        "$\\frac{ a }{ b }$ and $\\custom[ label ]{ x + y }$\n",
        Some(ParserOptions::default()),
    );
    let scope = SignatureScope::from_root_with_configured(
        &root,
        [
            (
                "frac".to_string(),
                CommandSignature {
                    arguments: vec![ArgSpec {
                        required: true,
                        kind: ArgKind::Brace,
                        domain: ArgumentDomain::Text,
                    }],
                },
            ),
            (
                "custom".to_string(),
                CommandSignature {
                    arguments: vec![
                        ArgSpec {
                            required: false,
                            kind: ArgKind::Bracket,
                            domain: ArgumentDomain::Unknown,
                        },
                        ArgSpec {
                            required: true,
                            kind: ArgKind::Brace,
                            domain: ArgumentDomain::Math,
                        },
                    ],
                },
            ),
        ],
    );
    let domains = root
        .descendants()
        .filter(|node| {
            matches!(
                node.kind(),
                SyntaxKind::MATH_GROUP | SyntaxKind::MATH_OPTIONAL
            ) && node
                .parent()
                .is_some_and(|parent| parent.kind() == SyntaxKind::MATH_COMMAND)
        })
        .map(|group| argument_domain_with_scope(&group, &scope))
        .collect::<Vec<_>>();

    assert_eq!(
        domains,
        vec![
            ArgumentDomain::Text,
            ArgumentDomain::Unknown,
            ArgumentDomain::Unknown,
            ArgumentDomain::Math,
        ]
    );
}

#[test]
fn raw_tex_redefinitions_shadow_configured_signatures() {
    let root = parse(
        "\\newcommand{\\custom}[1]{#1}\n\n$\\custom{ a + b }$\n",
        Some(ParserOptions::default()),
    );
    let scope = SignatureScope::from_root_with_configured(
        &root,
        [(
            "custom".to_string(),
            CommandSignature {
                arguments: vec![ArgSpec {
                    required: true,
                    kind: ArgKind::Brace,
                    domain: ArgumentDomain::Math,
                }],
            },
        )],
    );
    let group = root
        .descendants()
        .find(|node| {
            node.kind() == SyntaxKind::MATH_GROUP
                && node
                    .parent()
                    .is_some_and(|parent| parent.kind() == SyntaxKind::MATH_COMMAND)
        })
        .expect("configured command argument");

    assert_eq!(
        argument_domain_with_scope(&group, &scope),
        ArgumentDomain::Unknown
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

type SemanticProjection<T> = (T, MathClass, Option<DelimiterRole>, MathBreakPriority);
type RangedSemanticProjection = (
    u32,
    u32,
    MathClass,
    Option<DelimiterRole>,
    MathBreakPriority,
);
type RawRangedProjection = (u32, u32, MathClass, Option<DelimiterRole>);

fn semantic_atoms(body: &str) -> Vec<SemanticProjection<String>> {
    let root = SyntaxNode::new_root(parse_math_content(body, MathParseOptions::default()));
    semantic_math_atoms(&root)
        .map(|atom| {
            let start = usize::from(atom.range.start());
            let end = usize::from(atom.range.end());
            (
                body[start..end].to_owned(),
                atom.class,
                atom.delimiter,
                atom.break_priority,
            )
        })
        .collect()
}

fn badness_semantic_atoms(body: &str) -> Vec<RangedSemanticProjection> {
    let parsed = parse_badness(&format!("${body}$"));
    let content = parsed
        .syntax()
        .descendants()
        .find(|node| node.kind() == BadnessKind::MATH)
        .expect("Badness math content");
    let content_start = content.text_range().start();
    let mut raw: Vec<RawRangedProjection> = Vec::new();
    for element in content.children_with_tokens().filter(|element| {
        !matches!(
            element.kind(),
            BadnessKind::WHITESPACE
                | BadnessKind::NEWLINE
                | BadnessKind::COMMENT
                | BadnessKind::AMPERSAND
                | BadnessKind::LINE_BREAK
        )
    }) {
        let merge_relations = element.kind() == BadnessKind::WORD;
        let mut element_atoms: Vec<RawRangedProjection> = Vec::new();
        for atom in badness_math_atoms(&element) {
            let atom = (
                u32::from(atom.range.start() - content_start),
                u32::from(atom.range.end() - content_start),
                class_from_badness(atom.class),
                atom.delimiter.map(delimiter_from_badness),
            );
            if merge_relations
                && atom.2 == MathClass::Rel
                && let Some(previous) = element_atoms.last_mut()
                && previous.2 == MathClass::Rel
                && previous.1 == atom.0
            {
                previous.1 = atom.1;
            } else {
                element_atoms.push(atom);
            }
        }
        raw.extend(element_atoms);
    }

    let mut previous_is_operand = false;
    let mut previous_opener = false;
    raw.into_iter()
        .map(|(start, end, raw_class, delimiter)| {
            let is_binary = raw_class == MathClass::Bin;
            let class = if is_binary && (!previous_is_operand || previous_opener) {
                MathClass::Ord
            } else {
                raw_class
            };
            let priority = match class {
                MathClass::Bin => MathBreakPriority::Binary,
                MathClass::Rel => MathBreakPriority::Relation,
                _ => MathBreakPriority::None,
            };
            previous_is_operand = !matches!(class, MathClass::Bin | MathClass::Rel);
            previous_opener = delimiter == Some(DelimiterRole::Open);
            (start, end, class, delimiter, priority)
        })
        .collect()
}

fn panache_semantic_atoms(body: &str) -> Vec<RangedSemanticProjection> {
    let root = SyntaxNode::new_root(parse_math_content(body, MathParseOptions::default()));
    let content_start = root.text_range().start();
    semantic_math_atoms(&root)
        .map(|atom| {
            (
                u32::from(atom.range.start() - content_start),
                u32::from(atom.range.end() - content_start),
                atom.class,
                atom.delimiter,
                atom.break_priority,
            )
        })
        .collect()
}

#[test]
fn semantic_atom_stream_matches_badness_differentially() {
    for body in [
        "-x",
        "a+-b",
        "x=-y",
        "f(-x)",
        r"a\leq b",
        r"a\cdot b",
        "{a}+b",
        r"\leq_i x",
        r"\langle-x\rangle",
        "a<≤b",
    ] {
        assert_eq!(
            panache_semantic_atoms(body),
            badness_semantic_atoms(body),
            "{body}",
        );
    }
}

/// Badness's sequencer leaves a binary atom binary after punctuation. Panache
/// applies the full TeXbook Bin-to-Ord rule instead, so `a,-b` reads `-b` as a
/// unary sign -- matching `operators::coerce` and the formatter output Panache
/// has always shipped.
#[test]
fn panache_coerces_after_punctuation_where_badness_does_not() {
    use MathBreakPriority::{Binary, None as NoBreak};
    use MathClass::{Bin, Ord, Punct};

    assert_eq!(
        panache_semantic_atoms("a,-b"),
        vec![
            (0, 1, Ord, None, NoBreak),
            (1, 2, Punct, None, NoBreak),
            (2, 3, Ord, None, NoBreak),
            (3, 4, Ord, None, NoBreak),
        ],
    );
    assert_eq!(
        badness_semantic_atoms("a,-b"),
        vec![
            (0, 1, Ord, None, NoBreak),
            (1, 2, Punct, None, NoBreak),
            (2, 3, Bin, None, Binary),
            (3, 4, Ord, None, NoBreak),
        ],
    );
}

#[test]
fn semantic_atom_stream_matches_badness_contextual_roles() {
    use MathBreakPriority::{Binary, None as NoBreak, Relation};
    use MathClass::{Bin, Close, Inner, Open, Ord, Punct, Rel};

    assert_eq!(
        semantic_atoms(r"-a + -b = -c, -d {x} \cdot \langle -y \rangle \leq_i z≤w",),
        vec![
            ("-".into(), Ord, None, NoBreak),
            ("a".into(), Ord, None, NoBreak),
            ("+".into(), Bin, None, Binary),
            ("-".into(), Ord, None, NoBreak),
            ("b".into(), Ord, None, NoBreak),
            ("=".into(), Rel, None, Relation),
            ("-".into(), Ord, None, NoBreak),
            ("c".into(), Ord, None, NoBreak),
            (",".into(), Punct, None, NoBreak),
            ("-".into(), Ord, None, NoBreak),
            ("d".into(), Ord, None, NoBreak),
            ("{x}".into(), Inner, None, NoBreak),
            (r"\cdot".into(), Bin, None, Binary),
            (r"\langle".into(), Open, Some(DelimiterRole::Open), NoBreak,),
            ("-".into(), Ord, None, NoBreak),
            ("y".into(), Ord, None, NoBreak),
            (
                r"\rangle".into(),
                Close,
                Some(DelimiterRole::Close),
                NoBreak,
            ),
            (r"\leq_i".into(), Rel, None, Relation),
            ("z".into(), Ord, None, NoBreak),
            ("≤".into(), Rel, None, Relation),
            ("w".into(), Ord, None, NoBreak),
        ],
    );
}

#[test]
fn semantic_atom_stream_coalesces_compound_relations_and_keeps_utf8_ranges() {
    assert_eq!(
        semantic_atoms("α<≤β"),
        vec![
            ("α".into(), MathClass::Ord, None, MathBreakPriority::None,),
            (
                "<≤".into(),
                MathClass::Rel,
                None,
                MathBreakPriority::Relation,
            ),
            ("β".into(), MathClass::Ord, None, MathBreakPriority::None,),
        ],
    );

    assert_eq!(
        semantic_atoms(r"a\leq=b")
            .into_iter()
            .map(|(text, class, _, priority)| (text, class, priority))
            .collect::<Vec<_>>(),
        vec![
            ("a".into(), MathClass::Ord, MathBreakPriority::None),
            (r"\leq".into(), MathClass::Rel, MathBreakPriority::Relation,),
            ("=".into(), MathClass::Rel, MathBreakPriority::Relation),
            ("b".into(), MathClass::Ord, MathBreakPriority::None),
        ],
    );
}

#[test]
fn semantic_atom_stream_ignores_host_trivia_and_keeps_host_ranges() {
    let source = "> $$\n> α + β\n> $$\n";
    let root = parse(source, Some(ParserOptions::default()));
    let content = root
        .descendants()
        .find(|node| node.kind() == SyntaxKind::MATH_CONTENT)
        .expect("embedded math content");

    let atoms = semantic_math_atoms(&content)
        .map(|atom| {
            let start = usize::from(atom.range.start());
            let end = usize::from(atom.range.end());
            (&source[start..end], atom.class, atom.break_priority)
        })
        .collect::<Vec<_>>();

    assert_eq!(
        atoms,
        vec![
            ("α", MathClass::Ord, MathBreakPriority::None),
            ("+", MathClass::Bin, MathBreakPriority::Binary),
            ("β", MathClass::Ord, MathBreakPriority::None),
        ],
    );
}

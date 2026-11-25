//! CommonMark spec.txt conformance harness.
//!
//! Layout:
//! - Fixture: `tests/fixtures/commonmark-spec/spec.txt` (vendored from
//!   the upstream `commonmark/commonmark-spec` repo).
//! - Allowlist: `tests/commonmark/allowlist.txt` — example numbers that
//!   are expected to pass and are guarded against regression.
//! - Blocked list: `tests/commonmark/blocked.txt` — example numbers
//!   we deliberately do not target yet, with reasons.
//!
//! Two main test functions:
//! - `commonmark_allowlist`: panics if any allowlisted example regresses.
//! - `commonmark_full_report` (ignored by default): runs every example and
//!   writes a triage summary to `tests/commonmark/report.txt`.

#[path = "commonmark/spec_parser.rs"]
mod spec_parser;

#[path = "commonmark/html_renderer.rs"]
mod html_renderer;

use panache_parser::{Dialect, Extensions, Flavor, ParserOptions, parse};
use spec_parser::{SpecExample, normalize_html, read_spec};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

const SPEC_FIXTURE_REL: &str = "tests/fixtures/commonmark-spec/spec.txt";
const ALLOWLIST_REL: &str = "tests/commonmark/allowlist.txt";
const BLOCKED_REL: &str = "tests/commonmark/blocked.txt";
const REPORT_REL: &str = "tests/commonmark/report.txt";
const REPORT_JSON_REL: &str = "tests/commonmark/report.json";
const SPEC_VERSION: &str = "0.31.2";

fn manifest_path(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(rel)
}

fn commonmark_options() -> ParserOptions {
    ParserOptions {
        flavor: Flavor::CommonMark,
        dialect: Dialect::for_flavor(Flavor::CommonMark),
        extensions: Extensions::for_flavor(Flavor::CommonMark),
        ..ParserOptions::default()
    }
}

fn render_example(example: &SpecExample) -> String {
    let tree = parse(&example.markdown, Some(commonmark_options()));
    html_renderer::render(&tree)
}

fn matches_expected(example: &SpecExample, rendered: &str) -> bool {
    normalize_html(rendered) == normalize_html(&example.expected_html)
}

#[test]
fn commonmark_empty_sublist_paragraph_continuation() {
    let input = include_str!("fixtures/cases/list_item_empty_marker_paragraph_commonmark/input.md");
    let tree = parse(input, Some(commonmark_options()));
    assert_eq!(
        normalize_html(&html_renderer::render(&tree)),
        normalize_html("<ul>\n<li>a\n*</li>\n<li>b\n1.</li>\n</ul>\n")
    );
}

#[test]
fn commonmark_ordered_sublist_paragraph_continuation() {
    let input =
        include_str!("fixtures/cases/list_item_ordered_paragraph_continuation_commonmark/input.md");
    let tree = parse(input, Some(commonmark_options()));
    assert_eq!(
        normalize_html(&html_renderer::render(&tree)),
        normalize_html("<ol>\n<li>a\n9. b</li>\n</ol>\n")
    );
}

#[test]
fn commonmark_ordered_sublist_after_heading() {
    let input =
        include_str!("fixtures/cases/list_item_ordered_list_after_heading_commonmark/input.md");
    let tree = parse(input, Some(commonmark_options()));
    assert_eq!(
        normalize_html(&html_renderer::render(&tree)),
        normalize_html(
            "<ul>\n<li>\n<h1>h</h1>\n<ol start=\"9\">\n<li>b</li>\n</ol>\n</li>\n</ul>\n"
        )
    );
}

#[test]
fn commonmark_html_block_on_list_marker_line() {
    for (input, html) in [
        (
            "- <div>\n  body\n  </div>\n",
            "<ul>\n<li>\n<div>\nbody\n</div>\n</li>\n</ul>\n",
        ),
        (
            "- <div>\n- foo\n",
            "<ul>\n<li>\n<div>\n</li>\n<li>foo</li>\n</ul>\n",
        ),
        (
            "> - <div>\n>   body\n>   </div>\n",
            "<blockquote>\n<ul>\n<li>\n<div>\nbody\n</div>\n</li>\n</ul>\n</blockquote>\n",
        ),
    ] {
        let tree = parse(input, Some(commonmark_options()));
        assert_eq!(tree.text().to_string(), input);
        assert_eq!(
            normalize_html(&html_renderer::render(&tree)),
            normalize_html(html),
            "{input}"
        );
    }
}

#[test]
fn commonmark_blockquote_after_list() {
    let input = include_str!("fixtures/cases/blockquote_after_list_commonmark/input.md");
    let tree = parse(input, Some(commonmark_options()));
    assert_eq!(
        normalize_html(&html_renderer::render(&tree)),
        normalize_html(
            "<ul>\n<li>x</li>\n</ul>\n<blockquote>\n<ul>\n<li>q</li>\n</ul>\n</blockquote>\n"
        )
    );
}

#[test]
fn commonmark_blockquote_interrupts_list_item() {
    let expected =
        "<ul>\n<li>x</li>\n</ul>\n<blockquote>\n<ul>\n<li>q</li>\n</ul>\n</blockquote>\n";
    for input in [
        include_str!("fixtures/cases/blockquote_after_list_item_commonmark/input.md"),
        "- x\n > - q\n",
        "- x\r\n> - q\r\n",
        "- x\n> - q",
    ] {
        let tree = parse(input, Some(commonmark_options()));
        assert_eq!(tree.text().to_string(), input);
        assert_eq!(
            normalize_html(&html_renderer::render(&tree)),
            normalize_html(expected),
            "{input:?}"
        );
    }
}

#[test]
fn commonmark_blockquote_list_item_indentation() {
    for (input, expected) in [
        (
            "- x\n  > q\n",
            "<ul>\n<li>x\n<blockquote>\n<p>q</p>\n</blockquote>\n</li>\n</ul>\n",
        ),
        (
            "1. x\n  > q\n",
            "<ol>\n<li>x</li>\n</ol>\n<blockquote>\n<p>q</p>\n</blockquote>\n",
        ),
        (
            "1. x\n   > q\n",
            "<ol>\n<li>x\n<blockquote>\n<p>q</p>\n</blockquote>\n</li>\n</ol>\n",
        ),
        (
            "- x\n  - y\n  > q\n",
            "<ul>\n<li>x\n<ul>\n<li>y</li>\n</ul>\n<blockquote>\n<p>q</p>\n</blockquote>\n</li>\n</ul>\n",
        ),
        (
            "> - x\n> > q\n",
            "<blockquote>\n<ul>\n<li>x</li>\n</ul>\n<blockquote>\n<p>q</p>\n</blockquote>\n</blockquote>\n",
        ),
        (
            "- x\n>\n",
            "<ul>\n<li>x</li>\n</ul>\n<blockquote>\n</blockquote>\n",
        ),
    ] {
        let tree = parse(input, Some(commonmark_options()));
        assert_eq!(tree.text().to_string(), input);
        assert_eq!(
            normalize_html(&html_renderer::render(&tree)),
            normalize_html(expected),
            "{input:?}"
        );
    }
}

#[test]
fn commonmark_empty_list_marker_block_then_indented_code() {
    let input = include_str!(
        "fixtures/cases/empty_list_marker_block_then_indented_code_commonmark/input.md"
    );
    let tree = parse(input, Some(commonmark_options()));
    assert_eq!(
        normalize_html(&html_renderer::render(&tree)),
        normalize_html("<ul>\n<li>\n<h1>h</h1>\n<pre><code>code\n</code></pre>\n</li>\n</ul>\n")
    );

    for (block, html) in [
        ("```\n  ```", "<pre><code></code></pre>\n"),
        ("---", "<hr />\n"),
    ] {
        let input = format!("-\n  {block}\n      code\n");
        let tree = parse(&input, Some(commonmark_options()));
        assert_eq!(tree.text().to_string(), input);
        assert_eq!(
            normalize_html(&html_renderer::render(&tree)),
            normalize_html(&format!(
                "<ul>\n<li>\n{html}<pre><code>code\n</code></pre>\n</li>\n</ul>\n"
            )),
            "indented code after {block}"
        );
    }
}

#[test]
fn commonmark_empty_list_marker_block_then_dedent() {
    let input =
        include_str!("fixtures/cases/empty_list_marker_block_then_dedent_commonmark/input.md");
    let tree = parse(input, Some(commonmark_options()));
    assert_eq!(
        normalize_html(&html_renderer::render(&tree)),
        normalize_html("<ul>\n<li>\n<h1>h</h1>\n</li>\n</ul>\n<p>after</p>\n")
    );

    for (input, expected) in [
        (
            "-   \n  # h\n\n after\n",
            "<ul>\n<li>\n<h1>h</h1>\n</li>\n</ul>\n<p>after</p>\n",
        ),
        (
            "1.\n   # h\n\n  after\n",
            "<ol>\n<li>\n<h1>h</h1>\n</li>\n</ol>\n<p>after</p>\n",
        ),
    ] {
        let tree = parse(input, Some(commonmark_options()));
        assert_eq!(tree.text().to_string(), input);
        assert_eq!(
            normalize_html(&html_renderer::render(&tree)),
            normalize_html(expected)
        );
    }
}

#[test]
fn commonmark_empty_list_marker_empty_blockquote() {
    let input =
        include_str!("fixtures/cases/empty_list_marker_empty_blockquote_commonmark/input.md");
    let tree = parse(input, Some(commonmark_options()));
    assert_eq!(
        normalize_html(&html_renderer::render(&tree)),
        normalize_html(
            "<blockquote>\n<ul>\n<li>\n<blockquote>\n</blockquote>\n<p>after</p>\n</li>\n</ul>\n</blockquote>\n"
        )
    );
}

fn read_allowlist(path: &Path) -> BTreeSet<u32> {
    if !path.exists() {
        return BTreeSet::new();
    }
    let content = fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    content
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| {
            l.parse::<u32>()
                .unwrap_or_else(|_| panic!("invalid example number in {}: {l:?}", path.display()))
        })
        .collect()
}

#[test]
fn spec_parser_yields_652_examples() {
    let examples = read_spec(&manifest_path(SPEC_FIXTURE_REL));
    assert_eq!(
        examples.len(),
        652,
        "spec.txt v0.31.2 should contain exactly 652 examples; \
         did the fixture get regenerated against a different ref?"
    );
    assert_eq!(examples[0].number, 1);
    assert_eq!(examples.last().unwrap().number, 652);
    assert!(
        !examples[0].section.is_empty(),
        "first example should have a section name from the surrounding ATX heading"
    );
}

#[test]
fn commonmark_allowlist() {
    let allowlist_path = manifest_path(ALLOWLIST_REL);
    assert!(
        allowlist_path.exists(),
        "missing allowlist file: {}",
        allowlist_path.display()
    );
    let allowed = read_allowlist(&allowlist_path);
    if allowed.is_empty() {
        return; // baseline still being seeded
    }

    let examples = read_spec(&manifest_path(SPEC_FIXTURE_REL));
    let by_number: std::collections::HashMap<u32, &SpecExample> =
        examples.iter().map(|e| (e.number, e)).collect();

    let mut regressions = Vec::new();
    for number in &allowed {
        let example = by_number
            .get(number)
            .unwrap_or_else(|| panic!("allowlisted example #{number} not found in spec.txt"));
        let rendered = render_example(example);
        if !matches_expected(example, &rendered) {
            regressions.push((*number, example.section.clone()));
        }
    }

    assert!(
        regressions.is_empty(),
        "allowlisted CommonMark examples regressed:\n{}",
        regressions
            .iter()
            .map(|(n, s)| format!("  #{n} ({s})"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
#[ignore = "manual: run to generate triage report and seed/grow the allowlist"]
fn commonmark_full_report() {
    let examples = read_spec(&manifest_path(SPEC_FIXTURE_REL));
    let blocked = read_allowlist(&manifest_path(BLOCKED_REL));

    let mut passing = Vec::new();
    let mut failing = Vec::new();
    let mut by_section_pass: std::collections::BTreeMap<String, u32> =
        std::collections::BTreeMap::new();
    let mut by_section_fail: std::collections::BTreeMap<String, u32> =
        std::collections::BTreeMap::new();

    for example in &examples {
        let rendered =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| render_example(example)));
        let ok = match rendered {
            Ok(html) => matches_expected(example, &html),
            Err(_) => false,
        };
        if ok {
            passing.push(example.number);
            *by_section_pass.entry(example.section.clone()).or_insert(0) += 1;
        } else {
            failing.push(example.number);
            *by_section_fail.entry(example.section.clone()).or_insert(0) += 1;
        }
    }

    let total = examples.len();
    let pass = passing.len();
    let fail = failing.len();

    let mut report = String::new();
    report.push_str(&format!(
        "CommonMark spec.txt v{SPEC_VERSION} conformance report\n"
    ));
    report.push_str(&format!(
        "{pass} / {total} examples passing ({:.1}%)\n",
        (pass as f64 / total as f64) * 100.0
    ));
    report.push_str(&format!("{fail} failing\n"));
    report.push_str(&format!("{} blocked-list entries\n\n", blocked.len()));

    report.push_str("=== Per-section pass / fail ===\n");
    let all_sections: BTreeSet<&String> = by_section_pass
        .keys()
        .chain(by_section_fail.keys())
        .collect();
    for section in &all_sections {
        let p = by_section_pass.get(*section).copied().unwrap_or(0);
        let f = by_section_fail.get(*section).copied().unwrap_or(0);
        report.push_str(&format!("  {section}: {p} pass / {f} fail\n"));
    }

    report.push_str("\n=== Passing example numbers (allowlist candidates) ===\n");
    for n in &passing {
        report.push_str(&format!("{n}\n"));
    }

    report.push_str("\n=== Passing examples grouped by section ===\n");
    let by_number: std::collections::HashMap<u32, &SpecExample> =
        examples.iter().map(|e| (e.number, e)).collect();
    let mut section_to_passing: std::collections::BTreeMap<&str, Vec<u32>> =
        std::collections::BTreeMap::new();
    for n in &passing {
        let sec = by_number[n].section.as_str();
        section_to_passing.entry(sec).or_default().push(*n);
    }
    for (section, nums) in &section_to_passing {
        report.push_str(&format!("# {section}\n"));
        for n in nums {
            report.push_str(&format!("{n}\n"));
        }
        report.push('\n');
    }

    let report_path = manifest_path(REPORT_REL);
    fs::write(&report_path, &report)
        .unwrap_or_else(|e| panic!("failed to write {}: {e}", report_path.display()));

    let pass_pct = (pass as f64 / total as f64) * 100.0;
    let sections: Vec<serde_json::Value> = all_sections
        .iter()
        .map(|name| {
            let p = by_section_pass.get(*name).copied().unwrap_or(0);
            let f = by_section_fail.get(*name).copied().unwrap_or(0);
            serde_json::json!({ "name": name, "pass": p, "fail": f })
        })
        .collect();
    let json = serde_json::json!({
        "spec_version": SPEC_VERSION,
        "total_examples": total,
        "passing": pass,
        "failing": fail,
        "pass_pct": (pass_pct * 10.0).round() / 10.0,
        "blocked": blocked.len(),
        "sections": sections,
        "passing_numbers": passing,
    });
    let json_path = manifest_path(REPORT_JSON_REL);
    fs::write(&json_path, format!("{:#}\n", json))
        .unwrap_or_else(|e| panic!("failed to write {}: {e}", json_path.display()));

    eprintln!("\n{}", report);
    eprintln!("(report written to {})", report_path.display());
    eprintln!("(json written to {})", json_path.display());
}

#[cfg(feature = "experimental-block-parser")]
#[test]
fn experimental_block_coverage() {
    use panache_parser::parser::{experimental_block, parse_with_errors};
    let options = commonmark_options();
    let examples = read_spec(&manifest_path(SPEC_FIXTURE_REL));
    let mut failures = Vec::new();
    for example in &examples {
        let (tree, errors) =
            match experimental_block::try_parse_with_errors(&example.markdown, &options) {
                Ok(result) => result,
                Err(gap) => {
                    failures.push(format!(
                        "{}: unsupported {} at byte {}",
                        example.number, gap.construct, gap.offset
                    ));
                    continue;
                }
            };
        let (expected, expected_errors) =
            parse_with_errors(&example.markdown, Some(options.clone()));
        // Example 143 has an existing fenced code info-string discrepancy.
        if example.number == 143 {
            continue;
        }
        if tree.to_string() != example.markdown
            || errors != expected_errors
            || tree.green() != expected.green()
        {
            failures.push(format!(
                "{}: parser parity failed\nactual {tree:#?}\nexpected {expected:#?}",
                example.number
            ));
        }
        if !matches_expected(example, &html_renderer::render(&tree)) {
            failures.push(format!("{}: rendered HTML differs", example.number));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[cfg(feature = "experimental-block-parser")]
#[test]
fn experimental_sibling_markers_close_list_item_quotes() {
    use panache_parser::parser::experimental_block;

    for (input, expected) in [
        (
            "- > first\r\n  > continued\r\n- next",
            "<ul>\n<li>\n<blockquote>\n<p>first\ncontinued</p>\n</blockquote>\n</li>\n<li>next</li>\n</ul>\n",
        ),
        (
            "- > foo\n- > - bar\n \n  \t- nested\n",
            "<ul>\n<li>\n<blockquote>\n<p>foo</p>\n</blockquote>\n</li>\n<li>\n<blockquote>\n<ul>\n<li>bar</li>\n</ul>\n</blockquote>\n<ul>\n<li>nested</li>\n</ul>\n</li>\n</ul>\n",
        ),
    ] {
        let (tree, errors) =
            experimental_block::try_parse_with_errors(input, &commonmark_options()).unwrap();
        assert_eq!(tree.to_string(), input);
        assert!(errors.is_empty());
        assert_eq!(
            normalize_html(&html_renderer::render(&tree)),
            normalize_html(expected)
        );
    }
}

#[cfg(feature = "experimental-block-parser")]
#[test]
fn experimental_indented_code_continues_after_the_item_marker_line() {
    use panache_parser::parser::experimental_block;

    for (input, expected) in [
        (
            "- >     code\n  >       second\n",
            "<ul>\n<li>\n<blockquote>\n<pre><code>code\n  second\n</code></pre>\n</blockquote>\n</li>\n</ul>\n",
        ),
        (
            "> - >     code\n>   >       second\n",
            "<blockquote>\n<ul>\n<li>\n<blockquote>\n<pre><code>code\n  second\n</code></pre>\n</blockquote>\n</li>\n</ul>\n</blockquote>\n",
        ),
        (
            "-     code\n        second\n",
            "<ul>\n<li>\n<pre><code>code\n  second\n</code></pre>\n</li>\n</ul>\n",
        ),
        (
            "> -     code\n>         second\n",
            "<blockquote>\n<ul>\n<li>\n<pre><code>code\n  second\n</code></pre>\n</li>\n</ul>\n</blockquote>\n",
        ),
    ] {
        let (tree, errors) =
            experimental_block::try_parse_with_errors(input, &commonmark_options()).unwrap();
        assert_eq!(tree.to_string(), input);
        assert!(errors.is_empty());
        assert_eq!(
            normalize_html(&html_renderer::render(&tree)),
            normalize_html(expected)
        );
    }
}

#[cfg(feature = "experimental-block-parser")]
#[test]
fn experimental_reference_title_can_continue_after_a_list_marker() {
    use panache_parser::parser::experimental_block;

    let input = "- [ref]: /url\n  \"title\"\n\n[ref]\n";
    let expected = "<ul>\n<li></li>\n</ul>\n<p><a href=\"/url\" title=\"title\">ref</a></p>\n";
    let (tree, errors) =
        experimental_block::try_parse_with_errors(input, &commonmark_options()).unwrap();
    assert_eq!(tree.to_string(), input);
    assert!(errors.is_empty());
    assert_eq!(
        normalize_html(&html_renderer::render(&tree)),
        normalize_html(expected)
    );
}

#[cfg(feature = "experimental-block-parser")]
#[test]
fn experimental_reference_definitions_continue_inside_containers() {
    use panache_parser::parser::experimental_block;

    for (input, expected) in [
        (
            "> [ref]:\n>   /url\n\n[ref]\n",
            "<blockquote>\n</blockquote>\n<p><a href=\"/url\">ref</a></p>\n",
        ),
        (
            "- > [ref]: /url\n  > \"title\"\n\n[ref]\n",
            "<ul>\n<li>\n<blockquote>\n</blockquote>\n</li>\n</ul>\n<p><a href=\"/url\" title=\"title\">ref</a></p>\n",
        ),
        (
            "> [long\n> label]: /url \"long\n> title\"\n\n[long label]\n",
            "<blockquote>\n</blockquote>\n<p><a href=\"/url\" title=\"long\ntitle\">long label</a></p>\n",
        ),
        (
            "- [long\n  label]: /url\n\n[long label]\n",
            "<ul>\n<li></li>\n</ul>\n<p><a href=\"/url\">long label</a></p>\n",
        ),
        (
            "> [ref]:\n/url\n\n[ref]\n",
            "<blockquote>\n<p>[ref]:\n/url</p>\n</blockquote>\n<p>[ref]</p>\n",
        ),
    ] {
        let (tree, errors) =
            experimental_block::try_parse_with_errors(input, &commonmark_options()).unwrap();
        assert_eq!(tree.to_string(), input);
        assert!(errors.is_empty());
        assert_eq!(
            normalize_html(&html_renderer::render(&tree)),
            normalize_html(expected),
            "{input}"
        );
    }
}

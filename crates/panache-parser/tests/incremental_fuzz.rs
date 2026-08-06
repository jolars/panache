//! Seeded property harness for incremental reparsing.
//!
//! For every hazard snippet below, this harness applies pseudo-random edits
//! drawn from a hazard-biased insert alphabet (strings that can change block
//! structure at a distance) and asserts, per edit:
//!
//! 1. **Losslessness** — the incrementally reparsed tree round-trips to the
//!    edited text.
//! 2. **Structural identity** — its [`fingerprint`] equals a from-scratch
//!    parse of the edited text (in debug builds the in-crate oracle in
//!    `parser/verify.rs` checks the same invariant before we ever see the
//!    result; the comparison here also covers release runs).
//!
//! Chained batches additionally feed each spliced tree back in as the next
//! edit's base, mirroring how the LSP chains trees across keystrokes.
//!
//! The generator is a plain LCG (MMIX constants) with fixed per-test seeds:
//! runs are fully deterministic, and every assertion message carries the
//! snippet name, seed, and edit so a failure is reproducible by copying the
//! reported case into a unit test. Iteration counts scale with the
//! `PANACHE_FUZZ_ITERS` environment variable (a multiplier; the default is
//! sized for `cargo test`).
//!
//! A failure here is an incremental-parser bug: minimize it into an
//! `#[ignore]`d red test (see the roadmap in `TODO.md`, "Incremental
//! Parsing") and fix it by adding a bail-to-full-parse condition — never by
//! relaxing these asserts.

use std::panic::{AssertUnwindSafe, catch_unwind};

use panache_parser::parser::{fingerprint, parse, parse_incremental_suffix};

/// Knuth's MMIX linear congruential generator. Deterministic, dependency-free.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }

    /// A pseudo-random value in `0..n` (`n > 0`), using the high bits.
    fn below(&mut self, n: usize) -> usize {
        ((self.next() >> 33) % n as u64) as usize
    }
}

/// Insert alphabet biased toward strings that can restructure blocks at a
/// distance. Every entry is a hazard: fence/div delimiters, setext
/// underlines, list and quote markers, table pipes, math and HTML
/// delimiters, refdef-shaped lines, hard breaks, and multibyte text.
const INSERTS: &[&str] = &[
    "", // pure deletion
    "\n",
    "\n\n",
    " ",
    "    ",
    "\t",
    "text",
    "e",
    "# ",
    "#",
    "> ",
    ">",
    "- ",
    "* ",
    "+ ",
    "1. ",
    "```",
    "```\n",
    "~~~",
    ":::",
    "::: note\n",
    "---",
    "---\n",
    "===",
    "===\n",
    "|",
    "| a |",
    "$",
    "$$",
    "`",
    "*",
    "_",
    "[",
    "]",
    "(",
    ")",
    "[^1]",
    "[^1]: note\n",
    "[x]: /url\n",
    "\\",
    "\\\n",
    "<div>",
    "</div>",
    "<!--",
    "-->",
    "α",
    "παρά",
];

/// Hand-written hazard snippets. Each comment names the trap the snippet
/// encodes — the way an edit near it can change block structure at a
/// distance.
const HAZARD_SNIPPETS: &[(&str, &str)] = &[
    // A paragraph followed by a line that an inserted `---`/`===` can turn
    // into a setext underline, retroactively changing the paragraph's kind.
    ("setext_candidate", "alpha\nbeta\n\ngamma\ndelta\n"),
    // Lazy continuation: the unprefixed line belongs to the blockquote;
    // edits around the boundary move it in or out.
    ("lazy_blockquote", "> quoted\ncontinuation\n\ntail para\n"),
    // Same trap for list items; also indent-sensitive.
    ("lazy_list", "- item one\ncontinuation\n\n- item two\n"),
    // A closed backtick fence; deleting either delimiter makes the rest of
    // the document code.
    ("fenced_code", "```r\ncode <- 1\n```\n\npara\n"),
    // Tilde fences pair only with tildes; mixing is a paragraph.
    ("tilde_fence", "~~~\nliteral\n~~~\n\npara\n"),
    // Already-unterminated fence: everything after the opener is code, so
    // edits far below the opener still land inside one block.
    ("unterminated_fence", "```\ncode\n\npara after\n"),
    // Pandoc fenced div; `:::` runs open and close with loose matching.
    ("fenced_div", "::: note\nbody\n:::\n\npara\n"),
    // Nested divs: closing runs pair innermost-first, so an edit can
    // re-pair the outer fence.
    (
        "nested_div",
        ":::: outer\n::: inner\nbody\n:::\n::::\n\npara\n",
    ),
    // Loose/tight is a property of the whole list; inserting or deleting a
    // blank line between items flips every item's rendering.
    ("list_tightness", "- one\n\n- two\n- three\n\npara\n"),
    ("ordered_list", "1. first\n2. second\n\npara\n"),
    // The delimiter row decides whether the line above is a header or a
    // paragraph — the same backward dependency as setext.
    ("pipe_table", "| a | b |\n|---|---|\n| 1 | 2 |\n\npara\n"),
    // Reference definitions are document-scoped: adding/removing one
    // changes link resolution in *unedited* regions.
    ("refdef", "[foo]: /url\n\nsee [foo] and [bar] here\n"),
    // Use sites *before* the definitions: an edit near the tail that adds
    // or removes a definition changes resolution in the retained prefix.
    (
        "use_before_refdef",
        "see [x] and [foo] here\n\nmore prose\n\n[foo]: /url\n",
    ),
    // YAML frontmatter exists only when the delimiter sits at offset 0.
    ("frontmatter", "---\ntitle: x\n---\n\nbody para\n"),
    // HTML block (type 6) runs to a blank line.
    ("html_block", "<div>\nhtml body\n</div>\n\npara\n"),
    // HTML comment (type 2) runs to `-->`, across blank lines.
    ("html_comment", "<!-- note\n\nstill comment -->\n\npara\n"),
    // Display math: `$$` pairs like a fence.
    ("display_math", "$$\nx^2 + y\n$$\n\npara\n"),
    // Inline delimiter runs: `$`, backticks, emphasis.
    ("inline_spans", "text $x$ and `code` and *emph* span\n"),
    // Footnote definitions are block-level and referenced at a distance.
    ("footnote", "text[^1] more\n\n[^1]: note body\n"),
    // Escaped line break at line end glues lines inside one paragraph.
    ("hard_break", "line one\\\nline two\n\ntail\n"),
    // Multibyte text: every offset clamp must respect char boundaries.
    ("unicode", "αβγ δε ζη\n\nπαρά two λ\n"),
    // Nested blockquotes: marker runs stack.
    ("nested_blockquote", "> outer\n> > inner\n\npara\n"),
    // `---` is a thematic break, a setext underline, or a list bullet's
    // sibling depending on what surrounds it.
    ("hr_vs_setext", "- a\n\n---\n\n- b\n"),
    // Minimal document: edits at offsets 0 and EOF.
    ("tiny", "a\n"),
    // Resolved and unresolved reference-link shapes together.
    (
        "link_shapes",
        "[text](url) and [ref][foo] end\n\n[foo]: /u\n",
    ),
    // ATX headings: the section-window strategy anchors on these.
    (
        "atx_sections",
        "# One\n\nbody one\n\n## Two\n\nbody two\n\n# Three\n\nbody three\n",
    ),
];

fn iterations(default: usize) -> usize {
    std::env::var("PANACHE_FUZZ_ITERS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .map(|mult| default * mult)
        .unwrap_or(default)
}

fn apply_edit(text: &str, old: (usize, usize), insert: &str) -> String {
    let mut out = String::with_capacity(text.len() - (old.1 - old.0) + insert.len());
    out.push_str(&text[..old.0]);
    out.push_str(insert);
    out.push_str(&text[old.1..]);
    out
}

fn clamp_to_char_boundary(text: &str, mut pos: usize) -> usize {
    while !text.is_char_boundary(pos) {
        pos -= 1;
    }
    pos
}

/// A pseudo-random `(delete_range, insert)` for `text`, char-boundary safe.
fn random_edit(rng: &mut Lcg, text: &str) -> ((usize, usize), &'static str) {
    let start = clamp_to_char_boundary(text, rng.below(text.len() + 1));
    let max_delete = (text.len() - start).min(24);
    let end = clamp_to_char_boundary(text, start + rng.below(max_delete + 1)).max(start);
    let insert = INSERTS[rng.below(INSERTS.len())];
    ((start, end), insert)
}

/// Apply one edit incrementally against `old_tree` and check the invariants.
/// Returns the spliced tree so chains can build on it, or `None` when the
/// case must be skipped because the *full parser* is lossy on the edited
/// text: with a broken oracle the splice cannot be judged. Every skip prints
/// its reproducing input, because a skip is a *full-parser* bug worth
/// minimizing into a red test in `incremental_regressions.rs` (that is where
/// the refdef-in-list-item reorder, the `---`-after-blockquote marker
/// duplication, and the line-block panic came from); when a block-parser fix
/// lands, the skip counter drops.
fn check_edit(
    context: &str,
    before: &str,
    old_tree: &panache_parser::SyntaxNode,
    old_edit: (usize, usize),
    insert: &str,
    skipped_lossy: &mut usize,
) -> Option<panache_parser::SyntaxNode> {
    let updated = apply_edit(before, old_edit, insert);
    let new_edit = (old_edit.0, old_edit.0 + insert.len());

    // A full-parser panic or lossy full parse means the oracle itself is
    // broken for this input: skip the case and count it. The known
    // instances are pinned as red tests in `incremental_regressions.rs`;
    // a growing skip count on unchanged seeds means a new parser bug.
    let full = match catch_unwind(AssertUnwindSafe(|| parse(&updated, None))) {
        Ok(full) => full,
        Err(_) => {
            eprintln!(
                "full parser panicked (known-bug class, skipped): {context}\n  \
                 before: {before:?}\n  edit {old_edit:?} insert {insert:?}"
            );
            *skipped_lossy += 1;
            return None;
        }
    };
    let round_tripped = full.text().to_string();
    if round_tripped != updated {
        eprintln!(
            "full parse is lossy (known-bug class, skipped): {context}\n  \
             input:  {updated:?}\n  output: {round_tripped:?}"
        );
        *skipped_lossy += 1;
        return None;
    }

    // The in-crate debug oracle panics inside the call on divergence; catch
    // it so the failure report carries the reproducing case.
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        parse_incremental_suffix(&updated, None, old_tree, old_edit, new_edit)
    }));
    let inc = match outcome {
        Ok(inc) => inc,
        Err(_) => panic!(
            "in-crate oracle diverged: {context}\n  before: {before:?}\n  \
             edit {old_edit:?} insert {insert:?}\n  after: {updated:?}"
        ),
    };

    assert_eq!(
        inc.tree.text().to_string(),
        updated,
        "losslessness violated ({}): {context}\n  before: {before:?}\n  \
         edit {old_edit:?} insert {insert:?}",
        inc.strategy
    );

    assert_eq!(
        fingerprint(&inc.tree),
        fingerprint(&full),
        "structural divergence ({}): {context}\n  before: {before:?}\n  \
         edit {old_edit:?} insert {insert:?}\n  after: {updated:?}",
        inc.strategy
    );

    Some(inc.tree)
}

fn fuzz_single_edits(name: &str, text: &str, iters: usize, seed: u64) -> usize {
    let mut rng = Lcg(seed);
    let mut skipped = 0;
    let old_tree = parse(text, None);
    for i in 0..iters {
        let (old_edit, insert) = random_edit(&mut rng, text);
        let context = format!("snippet {name}, seed {seed}, single edit #{i}");
        check_edit(&context, text, &old_tree, old_edit, insert, &mut skipped);
    }
    skipped
}

fn fuzz_chained_edits(name: &str, text: &str, batches: usize, seed: u64) -> usize {
    let mut rng = Lcg(seed);
    let mut skipped = 0;
    for batch in 0..batches {
        let mut current = text.to_string();
        let mut tree = parse(&current, None);
        let chain_len = 2 + rng.below(3);
        for step in 0..chain_len {
            let (old_edit, insert) = random_edit(&mut rng, &current);
            let context =
                format!("snippet {name}, seed {seed}, batch #{batch}, chain step #{step}");
            let Some(next) = check_edit(&context, &current, &tree, old_edit, insert, &mut skipped)
            else {
                // The chain's text walked into full-parser-lossy territory;
                // later steps would judge splices against a broken oracle.
                break;
            };
            tree = next;
            current = apply_edit(&current, old_edit, insert);
        }
    }
    skipped
}

#[test]
fn hazard_snippets_single_edits() {
    let iters = iterations(200);
    let mut skipped = 0;
    for (index, (name, text)) in HAZARD_SNIPPETS.iter().enumerate() {
        skipped += fuzz_single_edits(name, text, iters, 0x9E3779B9 ^ (index as u64) << 8);
    }
    eprintln!("single edits: skipped {skipped} cases with a lossy full parse");
}

#[test]
fn hazard_snippets_chained_edits() {
    let batches = iterations(30);
    let mut skipped = 0;
    for (index, (name, text)) in HAZARD_SNIPPETS.iter().enumerate() {
        skipped += fuzz_chained_edits(name, text, batches, 0x51ED2701 ^ (index as u64) << 8);
    }
    eprintln!("chained edits: skipped {skipped} cases with a lossy full parse");
}

/// Real documents from `benches/documents/`: a second corpus tier with
/// random edits at random offsets. Iteration counts are low by default
/// (each check costs multiple full parses of a large document); scale with
/// `PANACHE_FUZZ_ITERS` for the graduation gate.
#[test]
fn real_documents_random_edits() {
    let docs_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../benches/documents");
    let names = ["small.qmd", "medium_quarto.qmd", "tables.qmd", "math.qmd"];
    let iters = iterations(20);
    let mut skipped = 0;
    for (index, name) in names.iter().enumerate() {
        let path = docs_dir.join(name);
        let Ok(text) = std::fs::read_to_string(&path) else {
            eprintln!("skipping absent corpus document {}", path.display());
            continue;
        };
        skipped += fuzz_single_edits(name, &text, iters, 0xC0FFEE ^ (index as u64) << 8);
    }
    eprintln!("real documents: skipped {skipped} cases with a lossy full parse");
}

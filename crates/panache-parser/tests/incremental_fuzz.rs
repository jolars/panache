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
//! 3. **Error identity** — its spliced syntax errors equal that parse's.
//!    Malformed YAML is the only error source there is, so the frontmatter
//!    and hashpipe snippets carry this half of the invariant.
//!
//! Chained batches additionally feed each spliced tree *and its errors* back
//! in as the next edit's base, mirroring how the LSP chains trees across
//! keystrokes.
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
//!
//! The hazard snippets are fuzzed with the window-size cutoff *off*
//! ([`CostGuards::Ignored`]). That cutoff declines any window covering more
//! than 85% of the document, which on snippets tens of bytes long is almost
//! every window: enforcing it here drops the share of edits that reach a splice
//! from 78% to 23%, and the guards this harness exists to test stop being
//! exercised. It is a *cost* guard with no soundness content, and the seams it
//! hides on a 30-byte snippet are the same seams that occur mid-document in a
//! real file, where the cutoff admits them. The real-document corpus below runs
//! with the production setting, so the shipped configuration is fuzzed too.
//!
//! Every driver tallies how many edits actually spliced and asserts a floor on
//! that share, because a harness whose edits all decline still passes every
//! invariant above while exercising nothing.

use std::panic::{AssertUnwindSafe, catch_unwind};

use panache_parser::parser::{CostGuards, SyntaxError, fingerprint, parse_with_errors};
use panache_parser::{Dialect, Extensions, Flavor, ParserOptions};

mod common;
use common::reparse_or_full_with_cost_guards;

/// One parser-option configuration to fuzz under, with its share of the
/// per-snippet budget.
struct Tier {
    name: &'static str,
    flavor: Flavor,
    /// Single edits per snippet.
    singles: usize,
    /// Chained batches per snippet.
    batches: usize,
    /// Single edits per real corpus document (0 = skip that corpus).
    real_docs: usize,
}

impl Tier {
    fn options(&self) -> ParserOptions {
        ParserOptions {
            flavor: self.flavor,
            extensions: Extensions::for_flavor(self.flavor),
            dialect: Dialect::for_flavor(self.flavor),
            ..Default::default()
        }
    }
}

/// The option tiers, chosen for *reach* rather than popularity — each brings
/// a hazard the others cannot express:
///
/// - `Pandoc` is the default and the baseline, and the only tier with
///   `pandoc_title_block`;
/// - `Gfm` is the CommonMark-dialect flavor that still enables
///   `yaml_metadata_block`, so it is the one that can reach the
///   mid-document-YAML refusal (plain `CommonMark` leaves the extension off
///   and cannot);
/// - `Quarto` brings hashpipe `#|` YAML, the only source of syntax errors
///   besides frontmatter;
/// - `MultiMarkdown` brings `mmd_title_block`.
///
/// The budgets **split** the old pandoc-only per-snippet counts rather than
/// multiplying them, so a default `cargo test` costs about what it did
/// before. `PANACHE_FUZZ_ITERS` multiplies every tier together, so the
/// graduation gate exercises all four deeply.
const TIERS: &[Tier] = &[
    Tier {
        name: "pandoc",
        flavor: Flavor::Pandoc,
        singles: 80,
        batches: 12,
        real_docs: 12,
    },
    Tier {
        name: "gfm",
        flavor: Flavor::Gfm,
        singles: 30,
        batches: 5,
        real_docs: 0,
    },
    Tier {
        name: "quarto",
        flavor: Flavor::Quarto,
        singles: 30,
        batches: 5,
        real_docs: 8,
    },
    Tier {
        name: "multimarkdown",
        flavor: Flavor::MultiMarkdown,
        singles: 20,
        batches: 3,
        real_docs: 0,
    },
];

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
    // CRLF terminators, so a mixed-ending document is reachable from every
    // snippet rather than only from the two that start out that way.
    "\r\n",
    "\r\n\r\n",
    // Document-start-only shapes: a window is parsed standalone, so its first
    // line is a document's first line to the block dispatcher.
    "%",
    "% Title\n",
    ":",
    "Key: value\n",
    "---\nk: v\n---\n",
    // Hashpipe with malformed YAML: the only error source besides frontmatter.
    "#| echo: [\n",
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
    // Pandoc `%` title block: recognized only on the document's first line,
    // so a window starting on one manufactures it.
    ("pandoc_title", "% Title\n% Author\n% Date\n\nbody para\n"),
    // Same trap for MultiMarkdown's `Key: Value` title block.
    ("mmd_title", "Title: Doc\nAuthor: Me\n\nbody para\n"),
    // A frontmatter-shaped block in the *body*: pandoc metadata, but a
    // thematic break plus a setext heading under CommonMark-family dialects.
    (
        "mid_document_yaml",
        "intro para\n\n---\nkey: value\n---\n\ntail para\n",
    ),
    // Malformed frontmatter: puts a syntax error in the retained prefix, so
    // the splice must carry it rather than re-derive it.
    ("bad_frontmatter", "---\ntitle: [\n---\n\nbody para\n"),
    // Hashpipe options: a second error site, and one that can sit anywhere in
    // the document rather than only at its start.
    (
        "hashpipe",
        "intro\n\n```{r}\n#| echo: false\n1 + 1\n```\n\ntail\n",
    ),
    // CRLF: every seam and blank-line test in the guard cascade is textual, so
    // a line ending it does not recognize refuses the whole document silently.
    // These two mirror `atx_sections` and `lazy_blockquote` byte for byte apart
    // from the terminator, so a splice rate that collapses here is the line
    // ending and nothing else.
    (
        "crlf_sections",
        "# One\r\n\r\nbody one\r\n\r\n## Two\r\n\r\nbody two\r\n\r\n# Three\r\n\r\nbody three\r\n",
    ),
    (
        "crlf_lazy_blockquote",
        "> quoted\r\ncontinuation\r\n\r\ntail para\r\n",
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

/// What a fuzz run exercised, beside passing.
#[derive(Default)]
struct FuzzStats {
    /// Edits whose *full* parse was lossy or panicked, so the splice could not
    /// be judged against it.
    skipped_lossy: usize,
    /// Edits the guard cascade accepted and spliced.
    spliced: usize,
    /// Edits it declined, which cost a full parse and prove nothing.
    declined: usize,
    /// Corpus documents that were not on disk. Counted rather than only
    /// printed: the corpus is gitignored, so a run on a clean checkout skips
    /// the strictest tier entirely and would otherwise report a full pass.
    skipped_absent: usize,
}

impl FuzzStats {
    fn splice_rate(&self) -> f64 {
        let judged = self.spliced + self.declined;
        if judged == 0 {
            return 0.0;
        }
        self.spliced as f64 / judged as f64
    }

    /// Report the run and fail if too few edits reached the splice at all.
    ///
    /// The floor is deliberately far below the measured rates (78% and 76% on
    /// the snippets, 60% on the real-document corpus): it is there to catch a
    /// guard that turns the harness into full-parse-versus-full-parse, not to
    /// pin the exact rate, which every new snippet moves.
    fn assert_exercised_the_splice(&self, what: &str) {
        eprintln!(
            "{what}: {} spliced, {} declined ({:.1}% spliced), {} skipped with a lossy full \
             parse, {} corpus documents absent",
            self.spliced,
            self.declined,
            self.splice_rate() * 100.0,
            self.skipped_lossy,
            self.skipped_absent
        );
        assert!(
            self.splice_rate() >= 0.25,
            "{what}: only {:.1}% of edits reached the splice; the harness is \
             judging full parses against full parses",
            self.splice_rate() * 100.0
        );
    }
}

/// What a driver holds constant across the edits it generates.
struct Run<'a> {
    options: &'a ParserOptions,
    cost_guards: CostGuards,
    stats: &'a mut FuzzStats,
}

/// A pseudo-random `(delete_range, insert)` for `text`, char-boundary safe.
fn random_edit(rng: &mut Lcg, text: &str) -> ((usize, usize), &'static str) {
    let start = clamp_to_char_boundary(text, rng.below(text.len() + 1));
    let max_delete = (text.len() - start).min(24);
    let end = clamp_to_char_boundary(text, start + rng.below(max_delete + 1)).max(start);
    let insert = INSERTS[rng.below(INSERTS.len())];
    ((start, end), insert)
}

/// The parse a splice builds on: the previous tree and the syntax errors that
/// go with it. Chains carry both forward, because both are spliced.
struct Base {
    tree: panache_parser::SyntaxNode,
    errors: Vec<SyntaxError>,
}

impl Base {
    /// Parse the text a chain of edits starts from, or `None` when the *base*
    /// parse is itself lossy or panics.
    ///
    /// The per-edit precondition checks the full parse of the *edited* text;
    /// this checks the parse the splice builds on. A base whose tree is
    /// shorter than its text hands the reparse offsets that do not resolve
    /// against that tree, which judges nothing and reaches rowan as a
    /// panic.
    fn parse(text: &str, options: &ParserOptions) -> Option<Self> {
        let (tree, errors) = catch_unwind(AssertUnwindSafe(|| {
            parse_with_errors(text, Some(options.clone()))
        }))
        .ok()?;
        (tree.text() == text).then_some(Self { tree, errors })
    }
}

/// Apply one edit incrementally against `base` and check the invariants.
/// Returns the spliced tree and its errors so chains can build on them, or
/// `None` when the case must be skipped because the *full parser* is lossy on
/// the edited text: with a broken oracle the splice cannot be judged. Every
/// skip prints its reproducing input, because a skip is a *full-parser* bug
/// worth minimizing into a red test in `incremental_regressions.rs` (that is
/// where the refdef-in-list-item reorder, the `---`-after-blockquote marker
/// duplication, and the line-block panic came from); when a block-parser fix
/// lands, the skip counter drops.
fn check_edit(
    context: &str,
    before: &str,
    run: &mut Run,
    base: &Base,
    old_edit: (usize, usize),
    insert: &str,
) -> Option<Base> {
    let (old_tree, old_errors) = (&base.tree, &base.errors[..]);
    let updated = apply_edit(before, old_edit, insert);
    let new_edit = (old_edit.0, old_edit.0 + insert.len());

    // A full-parser panic or lossy full parse means the oracle itself is
    // broken for this input: skip the case and count it. The known
    // instances are pinned as red tests in `incremental_regressions.rs`;
    // a growing skip count on unchanged seeds means a new parser bug.
    let (full, full_errors) = match catch_unwind(AssertUnwindSafe(|| {
        parse_with_errors(&updated, Some(run.options.clone()))
    })) {
        Ok(full) => full,
        Err(_) => {
            eprintln!(
                "full parser panicked (known-bug class, skipped): {context}\n  \
                 before: {before:?}\n  edit {old_edit:?} insert {insert:?}"
            );
            run.stats.skipped_lossy += 1;
            return None;
        }
    };
    let round_tripped = full.text().to_string();
    if round_tripped != updated {
        eprintln!(
            "full parse is lossy (known-bug class, skipped): {context}\n  \
             input:  {updated:?}\n  output: {round_tripped:?}"
        );
        run.stats.skipped_lossy += 1;
        return None;
    }

    // The in-crate debug oracle panics inside the call on divergence; catch
    // it so the failure report carries the reproducing case.
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        reparse_or_full_with_cost_guards(
            &updated,
            Some(run.options.clone()),
            old_tree,
            old_errors,
            old_edit,
            new_edit,
            run.cost_guards,
        )
    }));
    let inc = match outcome {
        Ok(inc) => inc,
        Err(_) => panic!(
            "in-crate oracle diverged: {context}\n  before: {before:?}\n  \
             edit {old_edit:?} insert {insert:?}\n  after: {updated:?}"
        ),
    };

    if inc.strategy == "full_reparse" {
        run.stats.declined += 1;
    } else {
        run.stats.spliced += 1;
    }

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

    assert_eq!(
        inc.errors, full_errors,
        "syntax-error divergence ({}): {context}\n  before: {before:?}\n  \
         edit {old_edit:?} insert {insert:?}\n  after: {updated:?}",
        inc.strategy
    );

    Some(Base {
        tree: inc.tree,
        errors: inc.errors,
    })
}

fn fuzz_single_edits(
    tier: &Tier,
    name: &str,
    text: &str,
    iters: usize,
    seed: u64,
    cost_guards: CostGuards,
    stats: &mut FuzzStats,
) {
    let mut rng = Lcg(seed);
    let options = tier.options();
    let Some(base) = Base::parse(text, &options) else {
        eprintln!(
            "base parse is lossy (known-bug class, skipped): snippet {name}, tier {}",
            tier.name
        );
        stats.skipped_lossy += 1;
        return;
    };
    let mut run = Run {
        options: &options,
        cost_guards,
        stats,
    };
    for i in 0..iters {
        let (old_edit, insert) = random_edit(&mut rng, text);
        let context = format!(
            "snippet {name}, tier {}, seed {seed}, single edit #{i}",
            tier.name
        );
        check_edit(&context, text, &mut run, &base, old_edit, insert);
    }
}

fn fuzz_chained_edits(
    tier: &Tier,
    name: &str,
    text: &str,
    batches: usize,
    seed: u64,
    cost_guards: CostGuards,
    stats: &mut FuzzStats,
) {
    let mut rng = Lcg(seed);
    let options = tier.options();
    let mut run = Run {
        options: &options,
        cost_guards,
        stats,
    };
    for batch in 0..batches {
        let mut current = text.to_string();
        let Some(mut base) = Base::parse(&current, run.options) else {
            eprintln!(
                "base parse is lossy (known-bug class, skipped): snippet {name}, tier {}",
                tier.name
            );
            run.stats.skipped_lossy += 1;
            break;
        };
        let chain_len = 2 + rng.below(3);
        for step in 0..chain_len {
            let (old_edit, insert) = random_edit(&mut rng, &current);
            let context = format!(
                "snippet {name}, tier {}, seed {seed}, batch #{batch}, chain step #{step}",
                tier.name
            );
            // The spliced errors feed the next step exactly as the spliced
            // tree does: a chain that reset them to empty would never
            // exercise the prefix-carry path past its first step.
            let Some(next) = check_edit(&context, &current, &mut run, &base, old_edit, insert)
            else {
                // The chain's text walked into full-parser-lossy territory;
                // later steps would judge splices against a broken oracle.
                break;
            };
            base = next;
            current = apply_edit(&current, old_edit, insert);
        }
    }
}

/// Per-tier seed: the tier index must participate, or every tier would
/// replay the identical edit sequence and the extra work would buy nothing.
fn seed(base: u64, snippet_index: usize, tier_index: usize) -> u64 {
    base ^ ((snippet_index as u64) << 8) ^ ((tier_index as u64) << 24)
}

#[test]
fn hazard_snippets_single_edits() {
    let mut stats = FuzzStats::default();
    for (tier_index, tier) in TIERS.iter().enumerate() {
        let iters = iterations(tier.singles);
        for (index, (name, text)) in HAZARD_SNIPPETS.iter().enumerate() {
            fuzz_single_edits(
                tier,
                name,
                text,
                iters,
                seed(0x9E3779B9, index, tier_index),
                CostGuards::Ignored,
                &mut stats,
            );
        }
    }
    stats.assert_exercised_the_splice("single edits");
}

#[test]
fn hazard_snippets_chained_edits() {
    let mut stats = FuzzStats::default();
    for (tier_index, tier) in TIERS.iter().enumerate() {
        let batches = iterations(tier.batches);
        for (index, (name, text)) in HAZARD_SNIPPETS.iter().enumerate() {
            fuzz_chained_edits(
                tier,
                name,
                text,
                batches,
                seed(0x51ED2701, index, tier_index),
                CostGuards::Ignored,
                &mut stats,
            );
        }
    }
    stats.assert_exercised_the_splice("chained edits");
}

/// Real documents from `benches/documents/`: a second corpus tier with
/// random edits at random offsets. Iteration counts are low by default
/// (each check costs multiple full parses of a large document); scale with
/// `PANACHE_FUZZ_ITERS` for the graduation gate.
///
/// The corpus is `.qmd`, so only the tiers that would actually be used on it
/// (`pandoc`, `quarto`) get a budget here; the option holes the other tiers
/// exist for are covered by the hazard snippets, which are cheap.
#[test]
fn real_documents_random_edits() {
    let docs_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../benches/documents");
    // Every name here must be one `benches/documents/download.sh` produces
    // (or a tracked file): an absent document is *skipped*, so a stale name
    // silently shrinks this tier instead of failing.
    let names = ["small.qmd", "configuration.qmd", "tables.qmd", "math.qmd"];
    let mut stats = FuzzStats::default();
    for (tier_index, tier) in TIERS.iter().enumerate() {
        if tier.real_docs == 0 {
            continue;
        }
        let iters = iterations(tier.real_docs);
        for (index, name) in names.iter().enumerate() {
            let path = docs_dir.join(name);
            let Ok(text) = std::fs::read_to_string(&path) else {
                eprintln!("skipping absent corpus document {}", path.display());
                stats.skipped_absent += 1;
                continue;
            };
            fuzz_single_edits(
                tier,
                name,
                &text,
                iters,
                seed(0xC0FFEE, index, tier_index),
                CostGuards::Enforced,
                &mut stats,
            );
        }
    }
    stats.assert_exercised_the_splice("real documents");
}

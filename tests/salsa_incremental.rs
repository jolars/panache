//! The salsa layer of incremental reparsing.
//!
//! `parsed_document` is the one authoritative parse; the reparse side channel
//! only makes it faster. These tests hold that line from both directions: that
//! reuse actually happens when it should (observed through the `Arc` identity
//! of retained top-level blocks, which is also what the downstream per-block
//! memos depend on), and that it does *not* happen when a reuse key moved, when
//! the pair was never admitted, or when the document was closed.
//!
//! Reuse is invisible in the *value* by construction -- a splice equals a full
//! parse byte for byte -- so identity is the only honest observable, and
//! "the value is right anyway" is asserted alongside it every time.

use std::collections::HashSet;
use std::path::PathBuf;

use panache::Config;
use panache::salsa::{FileConfig, FileText, SalsaDb, parse_syntax_errors, parsed_document};
use rowan::{GreenNode, GreenNodeData, NodeOrToken};

/// A document with two top-level headings and enough body to give the reparse
/// a genuine prefix to retain.
const DOC: &str = "\
# One

Alpha paragraph.

# Two

Beta paragraph.

# Three

Gamma paragraph.
";

fn doc_path(name: &str) -> PathBuf {
    PathBuf::from(format!("/virtual/{name}.qmd"))
}

/// Seed `db` with `text` at a virtual path and return its input handles.
fn seed(db: &mut SalsaDb, name: &str, text: &str) -> (FileText, FileConfig) {
    let file = db.update_file_text(doc_path(name), text.to_string());
    let config = FileConfig::new(db, Config::default());
    (file, config)
}

/// The document's top-level block nodes, kept *owned*.
///
/// Ownership matters: the identity comparisons below are pointer comparisons,
/// and an allocator is free to hand a freed address straight back. Holding a
/// refcounted handle to every block a test compares makes that impossible.
struct Blocks(Vec<GreenNode>);

impl Blocks {
    fn addrs(&self) -> HashSet<usize> {
        self.0
            .iter()
            .map(|node| {
                let data: &GreenNodeData = node;
                data as *const GreenNodeData as usize
            })
            .collect()
    }
}

fn blocks_of(db: &SalsaDb, file: FileText, config: FileConfig) -> Blocks {
    Blocks(
        parsed_document(db, file, config)
            .green
            .children()
            .filter_map(|child| match child.to_owned() {
                NodeOrToken::Node(node) => Some(node),
                NodeOrToken::Token(_) => None,
            })
            .collect(),
    )
}

/// The text a snapshot currently sees for `file`.
fn snapshot_text(db: &SalsaDb, file: FileText) -> String {
    file.content_or_empty(db).to_string()
}

fn text_of(db: &SalsaDb, file: FileText, config: FileConfig) -> String {
    panache::SyntaxNode::new_root(parsed_document(db, file, config).green.clone()).to_string()
}

/// Replace the first occurrence of `from` with `to`.
fn edited(text: &str, from: &str, to: &str) -> String {
    text.replacen(from, to, 1)
}

/// A config value distinct from the default, for the reuse-key tests.
fn commonmark_config() -> Config {
    Config {
        flavor: panache::config::Flavor::CommonMark,
        ..Config::default()
    }
}

/// A full parse of `text` under the default config, for value comparison.
fn full_parse(text: &str) -> String {
    panache::parse(text, None).to_string()
}

#[test]
fn an_admitted_document_reuses_retained_blocks_across_an_edit() {
    let mut db = SalsaDb::default();
    let (file, config) = seed(&mut db, "admitted", DOC);
    db.reparse_admit(file, config);

    let before = blocks_of(&db, file, config);
    assert!(
        !before.addrs().is_empty(),
        "the fixture must have block children"
    );

    let new_text = edited(DOC, "Gamma paragraph.", "Gamma paragraph edited.");
    db.update_file_text(doc_path("admitted"), new_text.clone());
    let after = blocks_of(&db, file, config);

    assert!(
        !before.addrs().is_disjoint(&after.addrs()),
        "an edit in the last block must retain earlier blocks by identity",
    );
    assert_eq!(text_of(&db, file, config), new_text);
    assert_eq!(text_of(&db, file, config), full_parse(&new_text));
}

#[test]
fn a_document_that_was_never_admitted_shares_nothing() {
    let mut db = SalsaDb::default();
    let (file, config) = seed(&mut db, "cold", DOC);
    // No `reparse_admit`: this is every CLI parse and every project-graph sweep.

    let before = blocks_of(&db, file, config);
    let new_text = edited(DOC, "Gamma paragraph.", "Gamma paragraph edited.");
    db.update_file_text(doc_path("cold"), new_text.clone());
    let after = blocks_of(&db, file, config);

    assert!(
        before.addrs().is_disjoint(&after.addrs()),
        "without admission every parse must be a full parse",
    );
    assert_eq!(text_of(&db, file, config), full_parse(&new_text));
}

#[test]
fn a_closed_document_stops_reusing() {
    let mut db = SalsaDb::default();
    let (file, config) = seed(&mut db, "closed", DOC);
    db.reparse_admit(file, config);

    let before = blocks_of(&db, file, config);
    db.reparse_retire_file(file);

    let new_text = edited(DOC, "Gamma paragraph.", "Gamma paragraph edited.");
    db.update_file_text(doc_path("closed"), new_text.clone());
    let after = blocks_of(&db, file, config);

    assert!(
        before.addrs().is_disjoint(&after.addrs()),
        "a retired base must not be used"
    );
    assert_eq!(text_of(&db, file, config), full_parse(&new_text));
}

#[test]
fn clearing_the_channel_stops_reusing() {
    let mut db = SalsaDb::default();
    let (file, config) = seed(&mut db, "cleared", DOC);
    db.reparse_admit(file, config);

    let before = blocks_of(&db, file, config);
    db.reparse_clear();

    let new_text = edited(DOC, "Gamma paragraph.", "Gamma paragraph edited.");
    db.update_file_text(doc_path("cleared"), new_text.clone());
    let after = blocks_of(&db, file, config);

    assert!(
        before.addrs().is_disjoint(&after.addrs()),
        "switching incremental parsing off must empty the channel",
    );
}

/// The live bug this whole reuse-key design exists to fix: an edit that changes
/// the refdef set changes how *unedited* text resolves, so the prefix cannot be
/// retained. The parser's textual guard only looks within 512 bytes of the
/// edit; here the definition is far away, so only the host's set comparison can
/// catch it.
#[test]
fn a_refdef_change_refuses_reuse_and_reresolves_distant_references() {
    let padding = "Filler paragraph.\n\n".repeat(60);
    let before_text = format!("A [link][ref] in prose.\n\n{padding}[ref]: https://example.com\n");
    // Rename the definition: the *unedited* `[link][ref]` must stop resolving.
    let after_text = before_text.replacen("[ref]: ", "[other]: ", 1);

    let mut db = SalsaDb::default();
    let (file, config) = seed(&mut db, "refdefs", &before_text);
    db.reparse_admit(file, config);

    let resolved = text_of(&db, file, config);
    assert_eq!(resolved, before_text);
    let before = blocks_of(&db, file, config);

    db.update_file_text(doc_path("refdefs"), after_text.clone());
    let after = blocks_of(&db, file, config);

    assert!(
        before.addrs().is_disjoint(&after.addrs()),
        "a refdef-set change must refuse every retained block",
    );
    // The value check that matters: the first block is re-derived, so the
    // now-dangling reference parses the way a full parse of the new text does.
    let reparsed = panache::SyntaxNode::new_root(parsed_document(&db, file, config).green.clone());
    let fresh = panache::parse(&after_text, None);
    assert_eq!(format!("{reparsed:#?}"), format!("{fresh:#?}"));
}

#[test]
fn a_config_change_refuses_reuse() {
    let mut db = SalsaDb::default();
    let (file, config) = seed(&mut db, "config", DOC);
    db.reparse_admit(file, config);
    let before = blocks_of(&db, file, config);

    // A second handle carrying a *different* config value. The LSP re-points a
    // document this way when `panache.toml` changes.
    let other = FileConfig::new(&db, commonmark_config());
    db.reparse_admit(file, other);

    let new_text = edited(DOC, "Gamma paragraph.", "Gamma paragraph edited.");
    db.update_file_text(doc_path("config"), new_text.clone());
    let after = blocks_of(&db, file, other);

    assert!(
        before.addrs().is_disjoint(&after.addrs()),
        "a base recorded under another config must not be spliced against",
    );
}

#[test]
fn a_sibling_config_parse_does_not_clobber_the_open_documents_base() {
    let mut db = SalsaDb::default();
    let (file, config) = seed(&mut db, "sibling", DOC);
    db.reparse_admit(file, config);
    let before = blocks_of(&db, file, config);

    // A project query parsing the same file under another config: not admitted,
    // so it must leave the open document's base untouched.
    let sibling = FileConfig::new(&db, commonmark_config());
    let _ = parsed_document(&db, file, sibling);

    let new_text = edited(DOC, "Gamma paragraph.", "Gamma paragraph edited.");
    db.update_file_text(doc_path("sibling"), new_text.clone());
    let after = blocks_of(&db, file, config);

    assert!(
        !before.addrs().is_disjoint(&after.addrs()),
        "a sibling-config parse must not evict the open document's base",
    );
    assert_eq!(text_of(&db, file, config), full_parse(&new_text));
}

#[test]
fn setting_the_text_back_to_the_base_returns_the_base_unchanged() {
    let mut db = SalsaDb::default();
    let (file, config) = seed(&mut db, "unchanged", DOC);
    db.reparse_admit(file, config);
    let before = blocks_of(&db, file, config);

    // Same bytes, new `Arc`: salsa sees a write, so the query re-executes and
    // takes the unchanged-text fast path rather than reparsing.
    db.update_file_text(doc_path("unchanged"), DOC.to_string());
    let after = blocks_of(&db, file, config);

    assert_eq!(
        before.addrs(),
        after.addrs(),
        "identical text must hand back the base's own tree",
    );
    assert_eq!(text_of(&db, file, config), DOC);
}

/// Syntax errors are covered by the governing invariant as much as the tree is:
/// a reused parse must carry the same error vector as a full parse, wherever
/// the malformed region sits relative to the edit.
#[test]
fn reused_errors_match_a_full_parse_around_the_window() {
    // Malformed frontmatter YAML: an error at the very top of the document,
    // which every reparse window retains rather than re-derives.
    let base = "---\ntitle: [\n---\n\n# One\n\nAlpha.\n\n# Two\n\nBeta.\n";

    for (label, from, to) in [
        ("after the error", "Beta.", "Beta edited."),
        ("in the middle", "Alpha.", "Alpha edited."),
        (
            "introducing a second",
            "Beta.",
            "Beta.\n\n```{r}\n#| bad: [\n```",
        ),
    ] {
        let mut db = SalsaDb::default();
        let (file, config) = seed(&mut db, "errors", base);
        db.reparse_admit(file, config);
        let _ = parsed_document(&db, file, config);

        let new_text = edited(base, from, to);
        db.update_file_text(doc_path("errors"), new_text.clone());

        let reused = parse_syntax_errors(&db, file, config).to_vec();
        let (_, full) = panache::parser::parse_with_errors(&new_text, None);
        assert_eq!(reused, full, "error vectors diverged {label}");
    }
}

/// A worker demanding the parse while the writer edits must either get a tree
/// that agrees with its own snapshot's text, or be cancelled. It must never see
/// a tree spliced from a base that has moved out from under it -- the failure
/// the store-last rule exists to prevent.
///
/// The reader is bounded rather than stop-flagged: salsa's writer waits for
/// in-flight reads, so an unbounded reader could starve it.
#[test]
fn a_worker_snapshot_reads_a_consistent_tree_while_the_writer_edits() {
    let mut db = SalsaDb::default();
    let (file, config) = seed(&mut db, "concurrent", DOC);
    db.reparse_admit(file, config);
    let _ = parsed_document(&db, file, config);

    let snapshot = db.clone();
    let reader = std::thread::spawn(move || {
        for _ in 0..200 {
            // Cancellation is the designed outcome of racing a write, not a
            // failure; `None` just means this read lost the race.
            let ok = salsa::Cancelled::catch(std::panic::AssertUnwindSafe(|| {
                let parsed = parsed_document(&snapshot, file, config);
                let text = snapshot_text(&snapshot, file);
                let root = panache::SyntaxNode::new_root(parsed.green.clone());
                root.to_string() == text
            }));
            if let Ok(consistent) = ok {
                assert!(consistent, "worker saw a tree that disagreed with its text");
            }
            std::thread::yield_now();
        }
    });

    for index in 0..50 {
        let text = edited(
            DOC,
            "Gamma paragraph.",
            &format!("Gamma paragraph {index}."),
        );
        db.update_file_text(doc_path("concurrent"), text);
    }
    reader.join().expect("reader thread must not panic");

    let final_text = edited(DOC, "Gamma paragraph.", "Gamma paragraph 49.");
    assert_eq!(text_of(&db, file, config), final_text);
}

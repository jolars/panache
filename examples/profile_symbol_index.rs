//! Symbol-usage-index micro-benchmark: parse once, then build the
//! `SymbolUsageIndex` in a loop and report the median per-iteration
//! wall-time. Companion to `examples/profile_lint.rs`; guards the
//! single-walk fold of `symbol_usage_index_from_tree` (`src/salsa.rs`),
//! which collapsed ~12 `tree.descendants()` traversals into one
//! (~50% off this query — see TODO.md "Performance").
//!
//! Usage:
//!   profile_symbol_index <doc> [iters] [flavor]
//!
//! `flavor` is one of `pandoc` (default), `quarto`, `rmarkdown`, `gfm`,
//! `commonmark`. Use a Quarto/RMarkdown flavor to exercise the
//! chunk-label/crossref/example-label paths gated off under plain Pandoc.

use std::env;
use std::fs;

use panache::Config;
use panache::config::{Extensions, Flavor};
use panache::salsa::{SalsaDb, symbol_usage_index_from_tree};

fn flavor_from_arg(name: &str) -> Flavor {
    match name.to_ascii_lowercase().as_str() {
        "quarto" => Flavor::Quarto,
        "rmarkdown" | "rmd" => Flavor::RMarkdown,
        "gfm" => Flavor::Gfm,
        "commonmark" | "cm" => Flavor::CommonMark,
        _ => Flavor::Pandoc,
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let path = args
        .get(1)
        .expect("usage: profile_symbol_index <doc> [iters] [flavor]");
    let iters: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(200);
    let flavor = args
        .get(3)
        .map(|s| flavor_from_arg(s))
        .unwrap_or(Flavor::Pandoc);

    let input = fs::read_to_string(path).expect("read input");
    let config = Config {
        flavor,
        extensions: Extensions::for_flavor(flavor),
        ..Default::default()
    };

    // Parse once: we are measuring index construction, not parsing.
    let tree = panache::parse(&input, Some(config.clone()));
    let db = SalsaDb::default();

    // Warm up so the first sample isn't dominated by lazy-static init.
    let warmup = symbol_usage_index_from_tree(&db, &tree, &config.extensions);
    let warm_entries = warmup.reference_definition_entries().count();

    let mut samples: Vec<u128> = Vec::with_capacity(iters);
    let mut sink: usize = 0;
    for _ in 0..iters {
        let start = std::time::Instant::now();
        let index =
            std::hint::black_box(symbol_usage_index_from_tree(&db, &tree, &config.extensions));
        samples.push(start.elapsed().as_nanos());
        sink = sink.wrapping_add(index.reference_definition_entries().count());
    }

    samples.sort_unstable();
    let median = samples[samples.len() / 2];
    let total: u128 = samples.iter().sum();
    eprintln!(
        "{} iters of {} bytes ({:?} flavor): median {:.3}ms/iter (mean {:.3}ms), {} refdef entries, sink {}",
        iters,
        input.len(),
        flavor,
        median as f64 / 1_000_000.0,
        (total as f64 / iters as f64) / 1_000_000.0,
        warm_entries,
        sink,
    );
}

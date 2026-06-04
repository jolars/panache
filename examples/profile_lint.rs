//! Lint micro-benchmark: parse once, then run the built-in lint runner in a
//! loop and report the median per-iteration wall-time. Companion to
//! `crates/panache-parser/examples/profile_parse.rs`, used to measure the
//! shared-walk lint refactor before/after.
//!
//! Usage:
//!   profile_lint <doc> [iters] [flavor]
//!
//! `flavor` is one of `pandoc` (default), `quarto`, `rmarkdown`, `gfm`,
//! `commonmark`. Use a Quarto/RMarkdown flavor to exercise the
//! chunk/crossref/figure-caption rules that are gated off under plain Pandoc.

use std::env;
use std::fs;

use panache::Config;
use panache::config::{Extensions, Flavor};

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
        .expect("usage: profile_lint <doc> [iters] [flavor]");
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

    // Parse once: we are measuring the lint pass, not parsing.
    let tree = panache::parse(&input, Some(config.clone()));

    // Warm up so the first sample isn't dominated by lazy-static init
    // (entity tables, emoji sets, salsa scaffolding).
    let warmup = panache::linter::lint(&tree, &input, &config);
    let diag_count = warmup.len();

    let mut samples: Vec<u128> = Vec::with_capacity(iters);
    let mut sink: usize = 0;
    for _ in 0..iters {
        let start = std::time::Instant::now();
        let diags = std::hint::black_box(panache::linter::lint(&tree, &input, &config));
        samples.push(start.elapsed().as_nanos());
        sink = sink.wrapping_add(diags.len());
    }

    samples.sort_unstable();
    let median = samples[samples.len() / 2];
    let total: u128 = samples.iter().sum();
    eprintln!(
        "{} iters of {} bytes ({:?} flavor): median {:.3}ms/iter (mean {:.3}ms), {} diagnostics, sink {}",
        iters,
        input.len(),
        flavor,
        median as f64 / 1_000_000.0,
        (total as f64 / iters as f64) / 1_000_000.0,
        diag_count,
        sink,
    );
}

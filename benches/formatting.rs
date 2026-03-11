use panache::{format, parse};
use std::env;
use std::fs;
use std::hint::black_box;
use std::path::Path;
use std::time::{Duration, Instant};

fn bench_parse(input: &str, config: &panache::Config, iterations: usize) -> Duration {
    let start = Instant::now();
    for _ in 0..iterations {
        black_box(parse(black_box(input), Some(config.clone())));
    }
    start.elapsed()
}

fn bench_format(input: &str, config: &panache::Config, iterations: usize) -> Duration {
    let start = Instant::now();
    for _ in 0..iterations {
        black_box(format(black_box(input), Some(config.clone()), None));
    }
    start.elapsed()
}

fn bench_parse_only(input: &str, config: &panache::Config, iterations: usize) -> Duration {
    // Parse once to get the CST, then format repeatedly
    let tree = parse(input, Some(config.clone()));
    let start = Instant::now();
    for _ in 0..iterations {
        black_box(panache::formatter::format_tree(
            black_box(&tree),
            config,
            None,
        ));
    }
    start.elapsed()
}

fn run_benchmark(name: &str, input: &str, iterations: usize) {
    let config = panache::Config::default();
    run_benchmark_with_config(name, input, &config, iterations);
}

fn run_benchmark_with_config(name: &str, input: &str, config: &panache::Config, iterations: usize) {
    println!("\n{}", "=".repeat(60));
    println!("Benchmark: {}", name);
    println!("{}", "=".repeat(60));
    println!(
        "Document size: {} bytes, {} lines",
        input.len(),
        input.lines().count()
    );

    // Warmup
    for _ in 0..10 {
        let _ = format(input, Some(config.clone()), None);
    }

    // Full pipeline (parse + format)
    let full_time = bench_format(input, config, iterations);
    let full_avg = full_time.as_micros() as f64 / iterations as f64;
    println!("\nFull pipeline (parse + format):");
    println!("  Total: {:?} for {} iterations", full_time, iterations);
    println!(
        "  Average: {:.2}µs per iteration ({:.2}ms)",
        full_avg,
        full_avg / 1000.0
    );

    // Parse only
    let parse_time = bench_parse(input, config, iterations);
    let parse_avg = parse_time.as_micros() as f64 / iterations as f64;
    println!("\nParse only:");
    println!("  Total: {:?} for {} iterations", parse_time, iterations);
    println!(
        "  Average: {:.2}µs per iteration ({:.2}ms)",
        parse_avg,
        parse_avg / 1000.0
    );

    // Format only (CST already built)
    let format_time = bench_parse_only(input, config, iterations);
    let format_avg = format_time.as_micros() as f64 / iterations as f64;
    println!("\nFormat only (CST pre-built):");
    println!("  Total: {:?} for {} iterations", format_time, iterations);
    println!(
        "  Average: {:.2}µs per iteration ({:.2}ms)",
        format_avg,
        format_avg / 1000.0
    );

    // Throughput
    let throughput = (input.len() as f64 / 1024.0) / (full_avg / 1_000_000.0);
    println!("\nThroughput: {:.2} KB/s", throughput);
}

fn load_document(name: &str) -> Option<String> {
    let path = Path::new("benches/documents").join(name);
    fs::read_to_string(path).ok()
}

fn main() {
    println!("Panache Benchmarks");
    println!("==================\n");

    if let Ok(doc_name) = env::var("PANACHE_BENCH_DOC") {
        let iterations = env::var("PANACHE_BENCH_ITERATIONS")
            .ok()
            .and_then(|raw| raw.parse::<usize>().ok())
            .unwrap_or(10);
        let doc = load_document(&doc_name).unwrap_or_else(|| {
            panic!(
                "PANACHE_BENCH_DOC '{}' not found under benches/documents/",
                doc_name
            )
        });
        run_benchmark(
            &format!("Selected profile doc ({doc_name})"),
            &doc,
            iterations,
        );
        if env::var("PANACHE_BENCH_COMPARE_BUILT_IN_WRAP")
            .ok()
            .as_deref()
            .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        {
            let built_in = panache::Config {
                built_in_greedy_wrap: false,
                ..panache::Config::default()
            };
            run_benchmark_with_config(
                &format!("Selected profile doc ({doc_name}) [built-in-greedy-wrap=false]"),
                &doc,
                &built_in,
                iterations,
            );
        }
        return;
    }

    // Load documents
    let small = load_document("small.qmd").expect("small.qmd not found - this should be committed");
    let medium = load_document("medium_quarto.qmd");
    let large = load_document("large_authoring.qmd");
    let tables = load_document("tables.qmd");
    let math = load_document("math.qmd");
    let pandoc_manual = load_document("pandoc_manual.md");

    // Run benchmarks
    run_benchmark("Small (synthetic)", &small, 1000);

    if let Some(doc) = medium {
        run_benchmark("Medium (Quarto tutorial)", &doc, 100);
    } else {
        println!("\n⚠️  Skipping medium benchmark - run benches/documents/download.sh");
    }

    if let Some(doc) = tables {
        run_benchmark("Tables (table-heavy)", &doc, 50);
    } else {
        println!("\n⚠️  Skipping tables benchmark - run benches/documents/download.sh");
    }

    if let Some(doc) = math {
        run_benchmark("Math (computation-heavy)", &doc, 50);
    } else {
        println!("\n⚠️  Skipping math benchmark - run benches/documents/download.sh");
    }

    if let Some(doc) = large {
        run_benchmark("Large (comprehensive)", &doc, 20);
    } else {
        println!("\n⚠️  Skipping large benchmark - run benches/documents/download.sh");
    }

    if let Some(doc) = pandoc_manual {
        run_benchmark("Pandoc MANUAL (stress)", &doc, 3);
    } else {
        println!("\n⚠️  Skipping Pandoc MANUAL benchmark - run benches/documents/download.sh");
    }

    println!("\n{}", "=".repeat(60));
    println!("Benchmarks complete!");
    println!("{}", "=".repeat(60));
}

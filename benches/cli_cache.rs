use panache::{Config, format, parse};
use serde::Serialize;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::hint::black_box;
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
struct FixtureDoc {
    path: PathBuf,
    input: String,
}

#[derive(Debug, Clone)]
struct FixtureSet {
    root: PathBuf,
    docs: Vec<FixtureDoc>,
}

#[derive(Debug, Clone)]
struct CachedLintDocumentBench {
    path: String,
    input: String,
    diagnostic_count: usize,
}

#[derive(Debug, Clone)]
struct LintEntryBench {
    file_fingerprint: String,
    config_fingerprint: String,
    tool_fingerprint: String,
    root_file: String,
    documents: Vec<CachedLintDocumentBench>,
}

#[derive(Debug, Clone)]
struct FormatEntryBench {
    file_fingerprint: String,
    config_fingerprint: String,
    tool_fingerprint: String,
    mode: String,
    unchanged: bool,
    output: String,
}

#[derive(Debug, Clone, Default)]
struct BenchCache {
    lint: HashMap<String, LintEntryBench>,
    format: HashMap<String, FormatEntryBench>,
}

#[derive(Debug, Serialize)]
struct BenchStats {
    mean_us: f64,
    median_us: f64,
    p95_us: f64,
}

#[derive(Debug, Serialize)]
struct BenchResult {
    id: String,
    files: usize,
    iterations: usize,
    cold_mean_us: f64,
    warm_mean_us: f64,
    speedup_vs_cold: f64,
    cold: BenchStats,
    warm: BenchStats,
}

#[derive(Debug, Serialize)]
struct BenchReport {
    schema_version: u32,
    results: Vec<BenchResult>,
}

fn stable_hash(value: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

fn file_fingerprint(input: &str) -> String {
    format!("{:x}", stable_hash(input))
}

fn config_fingerprint(cfg: &Config) -> String {
    format!("{:x}", stable_hash(&format!("{cfg:?}")))
}

fn tool_fingerprint() -> String {
    format!("panache@{}", env!("CARGO_PKG_VERSION"))
}

fn build_fixture(file_count: usize) -> FixtureSet {
    let root = env::temp_dir().join(format!(
        "panache-cli-cache-bench-{}",
        stable_hash(&format!(
            "{}:{}",
            std::process::id(),
            Instant::now().elapsed().as_nanos()
        ))
    ));
    fs::create_dir_all(&root).expect("failed to create benchmark temp dir");

    let mut docs = Vec::new();
    for idx in 0..file_count {
        let path = root.join(format!("doc-{idx:03}.qmd"));
        let body = format!(
            "# Document {idx}\n\n## Section\n\nThis benchmark paragraph stays reasonably stable.\n"
        );
        fs::write(&path, &body).expect("failed to write fixture document");
        docs.push(FixtureDoc { path, input: body });
    }

    FixtureSet { root, docs }
}

fn cleanup_fixture(set: &FixtureSet) {
    let _ = fs::remove_dir_all(&set.root);
}

fn run_cold_format_check(set: &FixtureSet, cfg: &Config) {
    for doc in &set.docs {
        let output = format(&doc.input, Some(cfg.clone()), None);
        black_box(output);
    }
}

fn run_warm_format_check(set: &FixtureSet, cfg: &Config, cache: &mut BenchCache) {
    let cfg_fp = config_fingerprint(cfg);
    let tool_fp = tool_fingerprint();

    for doc in &set.docs {
        let key = doc.path.to_string_lossy().to_string();
        let file_fp = file_fingerprint(&doc.input);
        if let Some(entry) = cache.format.get(&key)
            && entry.mode == "check"
            && entry.file_fingerprint == file_fp
            && entry.config_fingerprint == cfg_fp
            && entry.tool_fingerprint == tool_fp
        {
            black_box(entry.output.as_str());
            black_box(entry.unchanged);
            continue;
        }

        let output = format(&doc.input, Some(cfg.clone()), None);
        let unchanged = output == doc.input;
        cache.format.insert(
            key,
            FormatEntryBench {
                file_fingerprint: file_fp,
                config_fingerprint: cfg_fp.clone(),
                tool_fingerprint: tool_fp.clone(),
                mode: "check".to_string(),
                unchanged,
                output,
            },
        );
    }
}

fn run_cold_lint(set: &FixtureSet, cfg: &Config) {
    for doc in &set.docs {
        let tree = parse(&doc.input, Some(cfg.clone()));
        let diags = panache::linter::lint(&tree, &doc.input, cfg);
        black_box(diags);
    }
}

fn run_warm_lint(set: &FixtureSet, cfg: &Config, cache: &mut BenchCache) {
    let cfg_fp = config_fingerprint(cfg);
    let tool_fp = tool_fingerprint();

    for doc in &set.docs {
        let key = doc.path.to_string_lossy().to_string();
        let file_fp = file_fingerprint(&doc.input);
        if let Some(entry) = cache.lint.get(&key)
            && entry.file_fingerprint == file_fp
            && entry.config_fingerprint == cfg_fp
            && entry.tool_fingerprint == tool_fp
            && entry.root_file == key
        {
            black_box(entry.documents.len());
            continue;
        }

        let tree = parse(&doc.input, Some(cfg.clone()));
        let diagnostics = panache::linter::lint(&tree, &doc.input, cfg);
        let cached_doc = CachedLintDocumentBench {
            path: key.clone(),
            input: doc.input.clone(),
            diagnostic_count: diagnostics.len(),
        };
        cache.lint.insert(
            key.clone(),
            LintEntryBench {
                file_fingerprint: file_fp,
                config_fingerprint: cfg_fp.clone(),
                tool_fingerprint: tool_fp.clone(),
                root_file: key,
                documents: vec![cached_doc],
            },
        );
    }
}

fn summarize(samples: &[Duration]) -> BenchStats {
    let mut micros: Vec<f64> = samples.iter().map(|d| d.as_micros() as f64).collect();
    micros.sort_by(f64::total_cmp);
    let len = micros.len();
    let mean = if len == 0 {
        0.0
    } else {
        micros.iter().sum::<f64>() / len as f64
    };
    let median = if len == 0 {
        0.0
    } else if len.is_multiple_of(2) {
        (micros[len / 2 - 1] + micros[len / 2]) / 2.0
    } else {
        micros[len / 2]
    };
    let p95_index = ((len as f64 - 1.0) * 0.95).round() as usize;
    let p95 = micros.get(p95_index).copied().unwrap_or(0.0);
    BenchStats {
        mean_us: mean,
        median_us: median,
        p95_us: p95,
    }
}

fn run_benchmark_case(id: &str, files: usize, iterations: usize, mode: &str) -> BenchResult {
    let fixture = build_fixture(files);
    let cfg = Config::default();
    let mut cold = Vec::with_capacity(iterations);
    let mut warm = Vec::with_capacity(iterations);
    let mut cache = BenchCache::default();

    for _ in 0..iterations.max(1) {
        let start = Instant::now();
        match mode {
            "format_check" => run_cold_format_check(&fixture, &cfg),
            "lint" => run_cold_lint(&fixture, &cfg),
            _ => unreachable!("invalid mode"),
        }
        cold.push(start.elapsed());

        let start = Instant::now();
        match mode {
            "format_check" => run_warm_format_check(&fixture, &cfg, &mut cache),
            "lint" => run_warm_lint(&fixture, &cfg, &mut cache),
            _ => unreachable!("invalid mode"),
        }
        warm.push(start.elapsed());
    }

    // Touch cached lint fields so clippy doesn't consider them dead in this harness.
    for entry in cache.lint.values() {
        for doc in &entry.documents {
            black_box(doc.path.as_str());
            black_box(doc.input.as_str());
            black_box(doc.diagnostic_count);
        }
    }

    let cold_stats = summarize(&cold);
    let warm_stats = summarize(&warm);
    let speedup = if warm_stats.mean_us > 0.0 {
        cold_stats.mean_us / warm_stats.mean_us
    } else {
        0.0
    };

    cleanup_fixture(&fixture);

    BenchResult {
        id: id.to_string(),
        files,
        iterations: iterations.max(1),
        cold_mean_us: cold_stats.mean_us,
        warm_mean_us: warm_stats.mean_us,
        speedup_vs_cold: speedup,
        cold: cold_stats,
        warm: warm_stats,
    }
}

fn maybe_write_json_report(results: Vec<BenchResult>) {
    let Some(path) = env::var("PANACHE_CLI_CACHE_BENCH_OUTPUT_JSON").ok() else {
        return;
    };
    let report = BenchReport {
        schema_version: 1,
        results,
    };
    let json =
        serde_json::to_string_pretty(&report).expect("failed to serialize benchmark JSON report");
    fs::write(&path, json).unwrap_or_else(|e| {
        panic!("failed to write benchmark JSON report to '{path}': {e}");
    });
}

fn main() {
    let iterations = env::var("PANACHE_CLI_CACHE_BENCH_ITERATIONS")
        .ok()
        .and_then(|raw| raw.parse::<usize>().ok())
        .unwrap_or(40);
    let files = env::var("PANACHE_CLI_CACHE_BENCH_FILES")
        .ok()
        .and_then(|raw| raw.parse::<usize>().ok())
        .unwrap_or(50);

    println!("CLI Cache Benchmarks");
    println!("====================");
    println!("files: {files}");
    println!("iterations: {iterations}");

    let format_case = run_benchmark_case(
        "cli_format_check_cold_vs_warm_cache",
        files,
        iterations,
        "format_check",
    );
    println!(
        "format --check cold/warm mean: {:.2} / {:.2} us ({:.2}x)",
        format_case.cold_mean_us, format_case.warm_mean_us, format_case.speedup_vs_cold
    );

    let lint_case = run_benchmark_case("cli_lint_cold_vs_warm_cache", files, iterations, "lint");
    println!(
        "lint cold/warm mean: {:.2} / {:.2} us ({:.2}x)",
        lint_case.cold_mean_us, lint_case.warm_mean_us, lint_case.speedup_vs_cold
    );

    maybe_write_json_report(vec![format_case, lint_case]);
}

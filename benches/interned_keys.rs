use panache::salsa::{SalsaDb, intern_label, intern_path};
use std::collections::{HashMap, HashSet};
use std::hint::black_box;
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn synthetic_labels(total: usize, unique: usize) -> Vec<String> {
    (0..total)
        .map(|i| format!("label-{}", i % unique))
        .collect()
}

fn synthetic_paths(total: usize, unique: usize) -> Vec<PathBuf> {
    (0..total)
        .map(|i| {
            let idx = i % unique;
            PathBuf::from(format!(
                "/workspace/docs/section-{}/page-{}.qmd",
                idx / 10,
                idx
            ))
        })
        .collect()
}

fn bench_owned_labels(labels: &[String], iterations: usize) -> Duration {
    let start = Instant::now();
    for _ in 0..iterations {
        let mut counts: HashMap<String, usize> = HashMap::new();
        for label in labels {
            *counts.entry(label.clone()).or_insert(0) += 1;
        }
        black_box(counts);
    }
    start.elapsed()
}

fn bench_interned_labels(labels: &[String], iterations: usize) -> Duration {
    let db = SalsaDb::default();
    let start = Instant::now();
    for _ in 0..iterations {
        let mut counts = HashMap::new();
        for label in labels {
            let key = intern_label(&db, label);
            *counts.entry(key).or_insert(0usize) += 1;
        }
        black_box(counts);
    }
    start.elapsed()
}

fn bench_owned_paths(paths: &[PathBuf], iterations: usize) -> Duration {
    let start = Instant::now();
    for _ in 0..iterations {
        let mut counts: HashMap<PathBuf, usize> = HashMap::new();
        for path in paths {
            *counts.entry(path.clone()).or_insert(0) += 1;
        }
        black_box(counts);
    }
    start.elapsed()
}

fn bench_interned_paths(paths: &[PathBuf], iterations: usize) -> Duration {
    let db = SalsaDb::default();
    let start = Instant::now();
    for _ in 0..iterations {
        let mut counts = HashMap::new();
        for path in paths {
            let key = intern_path(&db, path);
            *counts.entry(key).or_insert(0usize) += 1;
        }
        black_box(counts);
    }
    start.elapsed()
}

fn main() {
    let total = std::env::var("PANACHE_BENCH_KEYS_TOTAL")
        .ok()
        .and_then(|raw| raw.parse::<usize>().ok())
        .unwrap_or(50_000);
    let unique = std::env::var("PANACHE_BENCH_KEYS_UNIQUE")
        .ok()
        .and_then(|raw| raw.parse::<usize>().ok())
        .unwrap_or(1_000);
    let iterations = std::env::var("PANACHE_BENCH_KEYS_ITERATIONS")
        .ok()
        .and_then(|raw| raw.parse::<usize>().ok())
        .unwrap_or(20);

    let labels = synthetic_labels(total, unique);
    let paths = synthetic_paths(total, unique);

    let label_total_bytes: usize = labels.iter().map(|l| l.len()).sum();
    let label_unique_bytes: usize = labels
        .iter()
        .collect::<HashSet<_>>()
        .iter()
        .map(|s| s.len())
        .sum();
    let path_total_bytes: usize = paths.iter().map(|p| p.to_string_lossy().len()).sum();
    let path_unique_bytes: usize = paths
        .iter()
        .collect::<HashSet<_>>()
        .iter()
        .map(|p| p.to_string_lossy().len())
        .sum();

    let owned_labels = bench_owned_labels(&labels, iterations);
    let interned_labels = bench_interned_labels(&labels, iterations);
    let owned_paths = bench_owned_paths(&paths, iterations);
    let interned_paths = bench_interned_paths(&paths, iterations);

    println!("Interned Key Benchmark");
    println!("======================");
    println!("total keys: {total}");
    println!("unique keys: {unique}");
    println!("iterations: {iterations}");
    println!();
    println!(
        "labels: owned={}us interned={}us repeated_bytes={}",
        owned_labels.as_micros() / iterations as u128,
        interned_labels.as_micros() / iterations as u128,
        label_total_bytes.saturating_sub(label_unique_bytes)
    );
    println!(
        "paths:  owned={}us interned={}us repeated_bytes={}",
        owned_paths.as_micros() / iterations as u128,
        interned_paths.as_micros() / iterations as u128,
        path_total_bytes.saturating_sub(path_unique_bytes)
    );
}

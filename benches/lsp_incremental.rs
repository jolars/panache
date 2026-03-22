use panache::Config;
use panache::parser::parse_incremental_suffix;
use serde::Serialize;
use std::env;
use std::fs;
use std::hint::black_box;
use std::path::Path;
use std::time::{Duration, Instant};

#[derive(Clone, Copy)]
struct BenchPosition {
    line: u32,
    character: u32,
}

#[derive(Clone, Copy)]
struct BenchRange {
    start: BenchPosition,
    end: BenchPosition,
}

#[derive(Clone)]
struct BenchChange {
    range: Option<BenchRange>,
    text: String,
}

struct StrategyRun {
    updated_text: String,
    tree_string: String,
    reparsed_range: OffsetRange,
    used_suffix_path: bool,
    fallback_reason: Option<&'static str>,
}

type OffsetRange = (usize, usize);
type AppliedOffsets = (String, OffsetRange, OffsetRange);

#[derive(Debug, Serialize)]
struct StrategyStats {
    mean_us: f64,
    median_us: f64,
    p95_us: f64,
}

#[derive(Debug, Serialize)]
struct CaseResult {
    id: String,
    document_size_bytes: usize,
    changes: usize,
    iterations: usize,
    incremental_reparsed_bytes: usize,
    incremental_reparsed_ratio: f64,
    incremental_used_suffix_path: bool,
    incremental_fallback_reason: Option<String>,
    incremental_fallback_rate: f64,
    incremental_speedup_vs_full: f64,
    strategy_full_reparse: StrategyStats,
    strategy_suffix_incremental_runtime: StrategyStats,
}

#[derive(Debug, Serialize)]
struct BenchmarkReport {
    schema_version: u32,
    results: Vec<CaseResult>,
}

struct BenchCase {
    id: String,
    input: String,
    changes: Vec<BenchChange>,
    iterations: usize,
}

fn position_to_offset_utf16(text: &str, position: BenchPosition) -> Option<usize> {
    let mut offset = 0;
    let mut current_line = 0;
    let bytes = text.as_bytes();

    for line in text.lines() {
        if current_line == position.line {
            let mut utf16_offset = 0;
            for (byte_idx, ch) in line.char_indices() {
                if utf16_offset >= position.character as usize {
                    return Some(offset + byte_idx);
                }
                utf16_offset += ch.len_utf16();
            }
            return Some(offset + line.len());
        }

        let line_end_offset = offset + line.len();
        let line_ending_len = if line_end_offset + 1 < text.len()
            && bytes[line_end_offset] == b'\r'
            && bytes[line_end_offset + 1] == b'\n'
        {
            2
        } else if line_end_offset < text.len() && bytes[line_end_offset] == b'\n' {
            1
        } else {
            0
        };

        offset += line.len() + line_ending_len;
        current_line += 1;
    }

    if current_line == position.line {
        return Some(offset);
    }

    None
}

fn apply_change_lenient(text: &str, change: &BenchChange) -> String {
    match change.range {
        Some(range) => {
            let start_offset = position_to_offset_utf16(text, range.start).unwrap_or(0);
            let end_offset = position_to_offset_utf16(text, range.end).unwrap_or(text.len());
            let mut result =
                String::with_capacity(text.len() - (end_offset - start_offset) + change.text.len());
            result.push_str(&text[..start_offset]);
            result.push_str(&change.text);
            result.push_str(&text[end_offset..]);
            result
        }
        None => change.text.clone(),
    }
}

fn apply_change_strict_with_offsets(text: &str, change: &BenchChange) -> Option<AppliedOffsets> {
    let range = change.range?;
    let start_offset = position_to_offset_utf16(text, range.start)?;
    let end_offset = position_to_offset_utf16(text, range.end)?;
    if start_offset > end_offset || end_offset > text.len() {
        return None;
    }

    let mut result =
        String::with_capacity(text.len() - (end_offset - start_offset) + change.text.len());
    result.push_str(&text[..start_offset]);
    result.push_str(&change.text);
    result.push_str(&text[end_offset..]);

    let new_start = start_offset;
    let new_end = start_offset + change.text.len();
    Some((result, (start_offset, end_offset), (new_start, new_end)))
}

fn full_reparse_strategy(input: &str, changes: &[BenchChange], config: &Config) -> StrategyRun {
    let mut updated_text = input.to_owned();
    for change in changes {
        updated_text = apply_change_lenient(&updated_text, change);
    }

    let tree = panache::parse(&updated_text, Some(config.clone()));
    let len = updated_text.len();
    StrategyRun {
        updated_text,
        tree_string: tree.to_string(),
        reparsed_range: (0, len),
        used_suffix_path: false,
        fallback_reason: None,
    }
}

fn suffix_incremental_runtime_strategy(
    input: &str,
    old_tree: &panache::SyntaxNode,
    changes: &[BenchChange],
    config: &Config,
) -> StrategyRun {
    if changes.len() != 1 {
        let mut run = full_reparse_strategy(input, changes, config);
        run.fallback_reason = Some("multi_change_uses_full_reparse");
        return run;
    }

    let change = &changes[0];

    if let Some((updated_text, old_edit, new_edit)) =
        apply_change_strict_with_offsets(input, change)
    {
        let incremental = parse_incremental_suffix(
            &updated_text,
            Some(config.clone()),
            old_tree,
            old_edit,
            new_edit,
        );
        let reparsed_range = incremental.reparse_range;
        let updated_tree = incremental.tree;
        let used_suffix_path = reparsed_range.0 > 0 || reparsed_range.1 < updated_text.len();
        return StrategyRun {
            updated_text,
            tree_string: updated_tree.to_string(),
            reparsed_range,
            used_suffix_path,
            fallback_reason: (!used_suffix_path).then_some("incremental_fallback_full_reparse"),
        };
    }

    let mut run = full_reparse_strategy(input, changes, config);
    run.fallback_reason = Some("invalid_change_range_uses_full_reparse");
    run
}

fn run_case(
    id: &str,
    input: &str,
    changes: &[BenchChange],
    iterations: usize,
    config: &Config,
) -> CaseResult {
    let old_tree = panache::parse(input, Some(config.clone()));
    let baseline = full_reparse_strategy(input, changes, config);
    let incremental_once = suffix_incremental_runtime_strategy(input, &old_tree, changes, config);
    assert_eq!(
        baseline.updated_text, incremental_once.updated_text,
        "text mismatch in case {id}"
    );
    assert_eq!(
        baseline.tree_string, incremental_once.tree_string,
        "tree mismatch in case {id}"
    );

    for _ in 0..5 {
        black_box(full_reparse_strategy(input, changes, config));
        black_box(suffix_incremental_runtime_strategy(
            input, &old_tree, changes, config,
        ));
    }

    let mut full_samples = Vec::with_capacity(iterations);
    let mut incremental_samples = Vec::with_capacity(iterations);
    let mut fallback_count = 0usize;

    for _ in 0..iterations {
        let start = Instant::now();
        black_box(full_reparse_strategy(input, changes, config));
        full_samples.push(start.elapsed());

        let start = Instant::now();
        let run = black_box(suffix_incremental_runtime_strategy(
            input, &old_tree, changes, config,
        ));
        if run.fallback_reason.is_some() {
            fallback_count += 1;
        }
        incremental_samples.push(start.elapsed());
    }

    let full_stats = summarize_samples(&full_samples);
    let incremental_stats = summarize_samples(&incremental_samples);
    let reparsed_bytes = incremental_once
        .reparsed_range
        .1
        .saturating_sub(incremental_once.reparsed_range.0);
    let reparsed_ratio = if input.is_empty() {
        0.0
    } else {
        reparsed_bytes as f64 / input.len() as f64
    };
    let fallback_rate = if iterations == 0 {
        0.0
    } else {
        fallback_count as f64 / iterations as f64
    };
    let speedup_vs_full = if incremental_stats.mean_us > 0.0 {
        full_stats.mean_us / incremental_stats.mean_us
    } else {
        0.0
    };

    CaseResult {
        id: id.to_owned(),
        document_size_bytes: input.len(),
        changes: changes.len(),
        iterations,
        incremental_reparsed_bytes: reparsed_bytes,
        incremental_reparsed_ratio: reparsed_ratio,
        incremental_used_suffix_path: incremental_once.used_suffix_path,
        incremental_fallback_reason: incremental_once.fallback_reason.map(str::to_owned),
        incremental_fallback_rate: fallback_rate,
        incremental_speedup_vs_full: speedup_vs_full,
        strategy_full_reparse: full_stats,
        strategy_suffix_incremental_runtime: incremental_stats,
    }
}

fn summarize_samples(samples: &[Duration]) -> StrategyStats {
    let mut micros: Vec<f64> = samples.iter().map(|d| d.as_micros() as f64).collect();
    micros.sort_by(f64::total_cmp);

    let len = micros.len();
    let median = if len == 0 {
        0.0
    } else if len.is_multiple_of(2) {
        (micros[len / 2 - 1] + micros[len / 2]) / 2.0
    } else {
        micros[len / 2]
    };
    let p95_index = ((len as f64 - 1.0) * 0.95).round() as usize;
    let p95 = micros.get(p95_index).copied().unwrap_or(0.0);
    let mean = if len == 0 {
        0.0
    } else {
        micros.iter().sum::<f64>() / len as f64
    };

    StrategyStats {
        mean_us: mean,
        median_us: median,
        p95_us: p95,
    }
}

fn range_change(
    start_line: u32,
    start_char: u32,
    end_line: u32,
    end_char: u32,
    text: &str,
) -> BenchChange {
    BenchChange {
        range: Some(BenchRange {
            start: BenchPosition {
                line: start_line,
                character: start_char,
            },
            end: BenchPosition {
                line: end_line,
                character: end_char,
            },
        }),
        text: text.to_owned(),
    }
}

fn full_change(text: &str) -> BenchChange {
    BenchChange {
        range: None,
        text: text.to_owned(),
    }
}

fn synthetic_document(paragraph_count: usize) -> String {
    let mut out = String::from("# Benchmark Document\n\n");
    for i in 0..paragraph_count {
        out.push_str(&format!(
            "Paragraph {:03} alpha beta gamma delta epsilon zeta eta theta.\n",
            i
        ));
    }
    out
}

fn load_document(name: &str) -> Option<String> {
    let path = Path::new("benches/documents").join(name);
    fs::read_to_string(path).ok()
}

fn add_real_document_cases(cases: &mut Vec<BenchCase>, default_iterations: usize) {
    let real_docs: [(&str, &str, u32, u32, u32, u32, &str, usize); 5] = [
        (
            "pandoc_manual_single_edit",
            "pandoc_manual.md",
            200,
            5,
            200,
            10,
            "manual",
            (default_iterations / 4).max(5),
        ),
        (
            "pandoc_manual_late_edit",
            "pandoc_manual.md",
            7600,
            0,
            7600,
            0,
            "NOTE: ",
            (default_iterations / 4).max(5),
        ),
        (
            "large_authoring_single_edit",
            "large_authoring.qmd",
            60,
            4,
            60,
            10,
            "AUTHORING",
            (default_iterations / 2).max(8),
        ),
        (
            "tables_single_edit",
            "tables.qmd",
            40,
            4,
            40,
            8,
            "TABLES",
            (default_iterations / 2).max(8),
        ),
        (
            "math_single_edit",
            "math.qmd",
            25,
            3,
            25,
            8,
            "MATH",
            (default_iterations / 2).max(8),
        ),
    ];

    for (id, file, sl, sc, el, ec, replacement, iterations) in real_docs {
        if let Some(doc) = load_document(file) {
            cases.push(BenchCase {
                id: id.to_owned(),
                input: doc,
                changes: vec![range_change(sl, sc, el, ec, replacement)],
                iterations,
            });
        }
    }
}

fn main() {
    let config = Config::default();
    let default_iterations = env::var("PANACHE_LSP_BENCH_ITERATIONS")
        .ok()
        .and_then(|raw| raw.parse::<usize>().ok())
        .unwrap_or(80);

    let small = synthetic_document(25);
    let medium = synthetic_document(250);
    let large = synthetic_document(1200);
    let utf16_doc = "# UTF16\nemoji: 😀 rocket: 🚀\nRésumé café\nmath αβγ\nclosing line\n";

    let mut cases: Vec<BenchCase> = vec![
        BenchCase {
            id: "single_change_small".to_owned(),
            input: small.clone(),
            changes: vec![range_change(2, 14, 2, 19, "ALPHA")],
            iterations: default_iterations,
        },
        BenchCase {
            id: "multi_change_small_4".to_owned(),
            input: small.clone(),
            changes: vec![
                range_change(2, 14, 2, 19, "ALPHA"),
                range_change(4, 20, 4, 24, "BETA"),
                range_change(6, 25, 6, 30, "GAMMA"),
                range_change(8, 31, 8, 36, "DELTA"),
            ],
            iterations: default_iterations,
        },
        BenchCase {
            id: "multi_change_medium_4".to_owned(),
            input: medium,
            changes: vec![
                range_change(10, 14, 10, 19, "ALPHA"),
                range_change(30, 20, 30, 24, "BETA"),
                range_change(80, 25, 80, 30, "GAMMA"),
                range_change(140, 31, 140, 36, "DELTA"),
            ],
            iterations: default_iterations / 2,
        },
        BenchCase {
            id: "multi_change_large_8".to_owned(),
            input: large,
            changes: vec![
                range_change(30, 14, 30, 19, "A1"),
                range_change(60, 20, 60, 24, "B2"),
                range_change(120, 25, 120, 30, "C3"),
                range_change(180, 31, 180, 36, "D4"),
                range_change(240, 14, 240, 19, "E5"),
                range_change(300, 20, 300, 24, "F6"),
                range_change(360, 25, 360, 30, "G7"),
                range_change(420, 31, 420, 36, "H8"),
            ],
            iterations: default_iterations / 4,
        },
        BenchCase {
            id: "multi_change_utf16_4".to_owned(),
            input: utf16_doc.to_owned(),
            changes: vec![
                range_change(1, 7, 1, 9, "😎"),
                range_change(1, 18, 1, 20, "🛰️"),
                range_change(2, 1, 2, 2, "e"),
                range_change(3, 5, 3, 7, "xyz"),
            ],
            iterations: default_iterations,
        },
        BenchCase {
            id: "full_replace".to_owned(),
            input: small.clone(),
            changes: vec![full_change("# Replaced\n\nAll new text.\n")],
            iterations: default_iterations,
        },
        BenchCase {
            id: "fallback_invalid_range".to_owned(),
            input: small,
            changes: vec![range_change(999, 0, 999, 5, "oops")],
            iterations: default_iterations,
        },
    ];

    add_real_document_cases(&mut cases, default_iterations);

    println!("LSP Incremental Benchmarks");
    println!("==========================");

    let mut results = Vec::new();
    for case in cases {
        println!("\nCase: {}", case.id);
        println!("  Document size: {} bytes", case.input.len());
        println!("  Changes: {}", case.changes.len());
        println!("  Iterations: {}", case.iterations);

        let id = case.id;
        let result = run_case(
            &id,
            &case.input,
            &case.changes,
            case.iterations.max(1),
            &config,
        );
        println!(
            "  Full reparse mean/median/p95: {:.2} / {:.2} / {:.2} us",
            result.strategy_full_reparse.mean_us,
            result.strategy_full_reparse.median_us,
            result.strategy_full_reparse.p95_us
        );
        println!(
            "  Suffix incremental mean/median/p95: {:.2} / {:.2} / {:.2} us",
            result.strategy_suffix_incremental_runtime.mean_us,
            result.strategy_suffix_incremental_runtime.median_us,
            result.strategy_suffix_incremental_runtime.p95_us
        );
        println!(
            "  Incremental reparsed: {} bytes ({:.2}%)",
            result.incremental_reparsed_bytes,
            result.incremental_reparsed_ratio * 100.0
        );
        println!(
            "  Incremental speedup vs full: {:.2}x",
            result.incremental_speedup_vs_full
        );
        println!(
            "  Incremental suffix path used: {} (fallback rate {:.2}%)",
            result.incremental_used_suffix_path,
            result.incremental_fallback_rate * 100.0
        );
        if let Some(reason) = &result.incremental_fallback_reason {
            println!("  Incremental fallback reason: {}", reason);
        }

        results.push(result);
    }

    if let Ok(path) = env::var("PANACHE_LSP_BENCH_OUTPUT_JSON") {
        let report = BenchmarkReport {
            schema_version: 2,
            results,
        };
        let json = serde_json::to_string_pretty(&report)
            .expect("failed to serialize LSP benchmark JSON report");
        fs::write(&path, json)
            .unwrap_or_else(|e| panic!("failed to write benchmark JSON report to '{path}': {e}"));
        println!("\nWrote JSON report to {}", path);
    }
}

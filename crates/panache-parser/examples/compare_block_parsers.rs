use std::hint::black_box;
use std::time::{Duration, Instant};

use panache_parser::parser::{Edit, experimental_block, parse_with_errors};
use panache_parser::{Flavor, ParserOptions};

fn sample(mut run: impl FnMut()) -> Duration {
    let start = Instant::now();
    run();
    start.elapsed()
}

fn stats(mut samples: Vec<Duration>) -> (f64, f64) {
    samples.sort_unstable();
    let micros = |i: usize| samples[i].as_secs_f64() * 1e6;
    (
        micros(samples.len() / 2),
        micros((samples.len() * 95 / 100).min(samples.len() - 1)),
    )
}

fn main() {
    let mut args = std::env::args().skip(1);
    let flavor = match args.next().as_deref() {
        Some("commonmark") => Flavor::CommonMark,
        Some("quarto") => Flavor::Quarto,
        _ => panic!("usage: compare_block_parsers <commonmark|quarto> <file>..."),
    };
    let options = ParserOptions::for_flavor(flavor);
    for path in args {
        let input = std::fs::read_to_string(&path).expect("read benchmark input");
        let (baseline, errors) = parse_with_errors(&input, Some(options.clone()));
        let (tree, diagnostics) = match experimental_block::try_parse_with_errors(&input, &options)
        {
            Ok(result) => result,
            Err(gap) => {
                println!(
                    "{path}: unsupported {} at byte {}",
                    gap.construct, gap.offset
                );
                continue;
            }
        };
        if baseline.green() != tree.green() || errors != diagnostics || tree.to_string() != input {
            println!("{path}: CST or diagnostics differ");
            continue;
        }
        let mut current = || {
            black_box(parse_with_errors(black_box(&input), Some(options.clone())));
        };
        let mut experimental = || {
            let result = experimental_block::try_parse_with_errors(black_box(&input), &options);
            black_box(result.expect("preflight passed"));
        };
        for _ in 0..16 {
            current();
            experimental();
        }
        let mut baseline_samples = Vec::new();
        let mut prototype_samples = Vec::new();
        for iteration in 0..200 {
            if iteration % 2 == 0 {
                baseline_samples.push(sample(&mut current));
                prototype_samples.push(sample(&mut experimental));
            } else {
                prototype_samples.push(sample(&mut experimental));
                baseline_samples.push(sample(&mut current));
            }
        }
        let (baseline_median, baseline_p95) = stats(baseline_samples);
        let (prototype_median, prototype_p95) = stats(prototype_samples);
        println!(
            "{path} ({} bytes): full parse median / p95 in µs, current {baseline_median:.1} / {baseline_p95:.1}, experimental {prototype_median:.1} / {prototype_p95:.1}",
            input.len()
        );
        println!(
            "  throughput in MB/s: current {:.1}, experimental {:.1}",
            input.len() as f64 / baseline_median,
            input.len() as f64 / prototype_median,
        );
        compare_edits(&input, &options);
    }
}

struct EditStep {
    label: &'static str,
    edit: Edit,
    text: String,
}

fn compare_edits(input: &str, options: &ParserOptions) {
    let Some(offset) = input
        .match_indices("\n\n")
        .nth(1)
        .map(|(offset, _)| offset + 2)
    else {
        return;
    };
    let mut text = input.to_owned();
    let mut steps = Vec::new();
    for (label, range, insert) in [
        ("line start", Some(offset..offset), "Edited prose here "),
        ("typing", Some(offset + 8..offset + 8), "x"),
        ("replacement", None, "# Replacement\n\nNew text.\n"),
    ] {
        let edit = Edit {
            range: range.unwrap_or(0..text.len()),
            insert: insert.into(),
        };
        text = edit.apply(&text);
        steps.push(EditStep {
            label,
            edit,
            text: text.clone(),
        });
    }
    for (index, step) in steps.iter().enumerate() {
        let (expected, expected_errors) = parse_with_errors(&step.text, Some(options.clone()));
        let Ok((prototype, errors)) =
            experimental_block::try_parse_with_errors(&step.text, options)
        else {
            println!("  {} edit: prototype coverage gap", step.label);
            return;
        };
        if prototype.green() != expected.green() || errors != expected_errors {
            println!("  {} edit: parity check failed", step.label);
            return;
        }
        for experimental in [false, true] {
            let replayed = replay(input, &steps[..=index], options, experimental, false);
            assert_eq!(replayed.green, expected.green().to_owned());
            assert_eq!(replayed.errors, expected_errors);
        }
    }

    let mut current = vec![Vec::new(); steps.len()];
    let mut prototype = vec![Vec::new(); steps.len()];
    for iteration in 0..216 {
        for experimental in [iteration % 2 != 0, iteration % 2 == 0] {
            let run = replay(input, &steps, options, experimental, iteration == 0);
            if iteration == 0 {
                let engine = if experimental {
                    "experimental"
                } else {
                    "current"
                };
                for (step, (strategy, bytes, fragment_bytes)) in steps.iter().zip(&run.strategies) {
                    println!(
                        "  {engine} {}: {strategy}, {bytes} bytes replaced, {fragment_bytes} fragment bytes",
                        step.label
                    );
                }
            }
            if iteration >= 16 {
                let samples = if experimental {
                    &mut prototype
                } else {
                    &mut current
                };
                for (step, elapsed) in samples.iter_mut().zip(run.samples) {
                    step.push(elapsed);
                }
            }
            black_box(run.green);
        }
    }
    for ((step, current), prototype) in steps.iter().zip(current).zip(prototype) {
        let (a, a95) = stats(current);
        let (b, b95) = stats(prototype);
        println!(
            "  {} median / p95 in µs: current {a:.1} / {a95:.1}, experimental {b:.1} / {b95:.1}",
            step.label
        );
    }
}

struct Replay {
    green: rowan::GreenNode,
    errors: Vec<panache_parser::parser::SyntaxError>,
    samples: Vec<Duration>,
    strategies: Vec<(&'static str, usize, usize)>,
}

fn replay(
    input: &str,
    steps: &[EditStep],
    options: &ParserOptions,
    experimental: bool,
    measure: bool,
) -> Replay {
    use panache_parser::collect_refdef_labels_with_options;
    let full = |text: &str, options: &ParserOptions| {
        if experimental {
            experimental_block::try_parse_with_errors(text, options).expect("preflight passed")
        } else {
            parse_with_errors(text, Some(options.clone()))
        }
    };
    let reparse = if experimental {
        experimental_block::reparse
    } else {
        panache_parser::parser::reparse
    };
    let mut cached = options.clone();
    cached.refdef_labels = Some(collect_refdef_labels_with_options(input, options));
    let (tree, mut errors) = full(input, &cached);
    let mut green = tree.green().to_owned();
    let mut samples = Vec::with_capacity(steps.len());
    let mut strategies = Vec::with_capacity(steps.len());
    for step in steps {
        let start = Instant::now();
        let (result, fragment_bytes) = if measure {
            experimental_block::measure_fragment_work(
                &green,
                &errors,
                &step.edit,
                &step.text,
                &cached,
                experimental,
            )
        } else {
            (reparse(&green, &errors, &step.edit, &step.text, &cached), 0)
        };
        let (next, next_errors, strategy) = if let Some(result) = result {
            let (start, end) = result.reparse_range;
            (
                result.green,
                result.errors,
                (result.strategy.as_str(), end - start, fragment_bytes),
            )
        } else {
            cached.refdef_labels = Some(collect_refdef_labels_with_options(&step.text, options));
            let (tree, errors) = full(&step.text, &cached);
            (
                tree.green().to_owned(),
                errors,
                ("full", step.text.len(), fragment_bytes),
            )
        };
        samples.push(start.elapsed());
        strategies.push(strategy);
        green = next;
        errors = next_errors;
    }
    Replay {
        green,
        errors,
        samples,
        strategies,
    }
}

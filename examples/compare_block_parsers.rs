use std::hint::black_box;
use std::path::{Path, PathBuf};

use clap::{Parser, ValueEnum};
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

#[derive(Clone, Copy, ValueEnum)]
enum ComparisonFlavor {
    #[value(name = "commonmark")]
    CommonMark,
    Pandoc,
    Quarto,
    #[value(name = "rmarkdown")]
    RMarkdown,
    #[value(name = "multimarkdown")]
    MultiMarkdown,
}

#[derive(Parser)]
struct Args {
    #[arg(long)]
    config: Option<PathBuf>,
    #[arg(long)]
    flavor: Option<ComparisonFlavor>,
    #[arg(required = true)]
    files: Vec<PathBuf>,
}

fn load_options(
    path: &Path,
    config: Option<&Path>,
    flavor: Option<Flavor>,
) -> std::io::Result<ParserOptions> {
    let parent = path.parent().unwrap_or(Path::new("."));
    let (config, _) = panache::config::load(config, parent, Some(path), flavor)?;
    Ok(config.parser_options())
}

fn main() -> std::io::Result<()> {
    let args = Args::parse();
    let flavor = args.flavor.map(|value| match value {
        ComparisonFlavor::CommonMark => Flavor::CommonMark,
        ComparisonFlavor::Pandoc => Flavor::Pandoc,
        ComparisonFlavor::Quarto => Flavor::Quarto,
        ComparisonFlavor::RMarkdown => Flavor::RMarkdown,
        ComparisonFlavor::MultiMarkdown => Flavor::MultiMarkdown,
    });
    for file in &args.files {
        let options = load_options(file, args.config.as_deref(), flavor)?;
        let input = std::fs::read_to_string(file)?;
        let path = file.display();
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
        let Some(steps) = check_edits(&input, &options) else {
            continue;
        };
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
        compare_edits(&input, &options, &steps);
    }
    Ok(())
}

struct EditStep {
    label: &'static str,
    edit: Edit,
    text: String,
}

fn check_edits(input: &str, options: &ParserOptions) -> Option<Vec<EditStep>> {
    let Some(offset) = input
        .split_inclusive('\n')
        .scan(0, |offset, line| {
            *offset += line.len();
            Some((*offset, line))
        })
        .filter(|(_, line)| matches!(*line, "\n" | "\r\n"))
        .nth(1)
        .map(|(offset, _)| offset)
    else {
        return Some(Vec::new());
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
            return None;
        };
        if prototype.green() != expected.green()
            || errors != expected_errors
            || prototype.to_string() != step.text
        {
            println!("  {} edit: parity check failed", step.label);
            return None;
        }
        for experimental in [false, true] {
            let replayed = replay(input, &steps[..=index], options, experimental, false);
            assert_eq!(replayed.green, expected.green().to_owned());
            assert_eq!(replayed.errors, expected_errors);
        }
    }

    Some(steps)
}

fn compare_edits(input: &str, options: &ParserOptions, steps: &[EditStep]) {
    if steps.is_empty() {
        return;
    }
    let mut current = vec![Vec::new(); steps.len()];
    let mut prototype = vec![Vec::new(); steps.len()];
    for iteration in 0..216 {
        for experimental in [iteration % 2 != 0, iteration % 2 == 0] {
            let run = replay(input, steps, options, experimental, iteration == 0);
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

#[cfg(test)]
mod tests {
    use super::*;
    use panache_parser::syntax::SyntaxKind;

    fn parity(input: &str, options: &ParserOptions) -> panache_parser::SyntaxNode {
        let (current, errors) = parse_with_errors(input, Some(options.clone()));
        let (experimental, actual_errors) =
            experimental_block::try_parse_with_errors(input, options).unwrap();
        assert_eq!(experimental.to_string(), input);
        assert_eq!(experimental.green(), current.green(), "{input}");
        assert_eq!(actual_errors, errors);
        experimental
    }

    #[test]
    fn project_extensions_change_block_shapes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("chapter.qmd");
        let explicit = dir.path().join("enabled.toml");
        std::fs::write(
            dir.path().join("panache.toml"),
            r"
[extensions]
pipe-tables = true
[extensions.quarto]
pipe-tables = false
fenced-divs = false
executable-code = false
",
        )
        .unwrap();
        std::fs::write(&explicit, "").unwrap();
        let disabled = load_options(&path, None, None).unwrap();
        let enabled = load_options(&path, Some(&explicit), None).unwrap();
        for (input, kind) in [
            ("a | b\n--|--\nx | y\n", SyntaxKind::PIPE_TABLE),
            ("::: {.note}\nBody.\n:::\n", SyntaxKind::FENCED_DIV),
            (
                "```{r}\n#| label: [\nprint(1)\n```\n",
                SyntaxKind::CODE_BLOCK,
            ),
        ] {
            assert_eq!(
                parity(input, &disabled).first_child().unwrap().kind(),
                SyntaxKind::PARAGRAPH
            );
            assert_eq!(parity(input, &enabled).first_child().unwrap().kind(), kind);
            let (_, errors) = experimental_block::try_parse_with_errors(input, &enabled).unwrap();
            assert_eq!(errors.len(), usize::from(kind == SyntaxKind::CODE_BLOCK));
        }

        let mut input = format!(
            "# Intro\n\nBefore.\n\na | b\n--|--\nx | y\n\n{}",
            "After.\n\n".repeat(60)
        );
        let mut tree = parity(&input, &disabled);
        for (original, replacement) in [("x | y", "*x* | y"), ("*x* | y", "*x* | y\nd | e")] {
            let start = input.find(original).unwrap();
            let edit = Edit {
                range: start..start + original.len(),
                insert: replacement.into(),
            };
            input = edit.apply(&input);
            let result = experimental_block::reparse(
                &tree.green().to_owned(),
                &[],
                &edit,
                &input,
                &disabled,
            )
            .unwrap();
            assert_eq!(
                result.strategy,
                panache_parser::parser::ReparseStrategy::Region
            );
            tree = parity(&input, &disabled);
            assert_eq!(&*result.green, tree.green());
            assert!(result.errors.is_empty());
        }
    }

    #[test]
    fn configured_grid_table_can_fall_through_to_prose() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("chapter.qmd");
        let config = dir.path().join("panache.toml");
        let input = "+-----+-----+\n| a   | b   |\n+=====+=====+\n| x   | y   |\n+-----+-----+\n";
        std::fs::write(&config, "").unwrap();
        let enabled = load_options(&path, None, None).unwrap();
        let gap = experimental_block::try_parse_with_errors(input, &enabled).unwrap_err();
        assert_eq!(gap.construct, "grid table");
        assert_eq!(gap.offset, 0);
        std::fs::write(&config, "[extensions.quarto]\ngrid-tables = false\n").unwrap();
        let disabled = load_options(&path, None, None).unwrap();
        assert_eq!(
            parity(input, &disabled).first_child().unwrap().kind(),
            SyntaxKind::PARAGRAPH
        );
    }

    #[test]
    fn project_flavor_selects_dialect() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("panache.toml"),
            "[flavors]\ncommonmark = ['strict.md']\n",
        )
        .unwrap();
        for (filename, kind) in [
            ("strict.md", SyntaxKind::CODE_BLOCK),
            ("chapter.qmd", SyntaxKind::PARAGRAPH),
        ] {
            let options = load_options(&dir.path().join(filename), None, None).unwrap();
            assert_eq!(
                parity("```rust\nfn main() {}\n", &options)
                    .first_child()
                    .unwrap()
                    .kind(),
                kind
            );
        }
    }

    #[test]
    fn project_compat_controls_new_ordered_sublists() {
        let dir = tempfile::tempdir().unwrap();
        for (target, count) in [("3.9", 2), ("3.10", 1), ("latest", 1)] {
            std::fs::write(
                dir.path().join("panache.toml"),
                format!("[compat]\npandoc = '{target}'\n"),
            )
            .unwrap();
            let options = load_options(&dir.path().join("chapter.qmd"), None, None).unwrap();
            for input in ["- outer\n  2. child\n", "> - outer\n>   2. child\n"] {
                let tree = parity(input, &options);
                assert_eq!(
                    tree.descendants()
                        .filter(|node| node.kind() == SyntaxKind::LIST)
                        .count(),
                    count
                );
            }
        }
    }

    #[test]
    fn edit_workload_uses_the_third_paragraph_with_either_line_ending() {
        let options = ParserOptions::for_flavor(Flavor::CommonMark);
        for newline in ["\n", "\r\n"] {
            let before = format!("First.{newline}{newline}Second.{newline}{newline}");
            let input = format!("{before}Third.{newline}");
            let steps = check_edits(&input, &options).unwrap();
            assert_eq!(steps.len(), 3);
            assert_eq!(steps[0].edit.range, before.len()..before.len());
        }
    }
}

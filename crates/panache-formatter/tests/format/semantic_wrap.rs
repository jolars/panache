use panache_formatter::config::WrapMode;
use panache_formatter::{Config, format};

fn cfg_semantic() -> Config {
    Config {
        wrap: Some(WrapMode::Semantic),
        ..Default::default()
    }
}

/// Format twice and assert the output is stable, then return it.
fn run(input: &str) -> String {
    let out = format(input, Some(cfg_semantic()), None);
    let out2 = format(&out, Some(cfg_semantic()), None);
    assert_eq!(out, out2, "semantic wrapping must be idempotent");
    out
}

#[test]
fn adds_sentence_breaks_and_preserves_existing_breaks() {
    // The author broke after "asks:" (a clause break); semantic mode keeps it
    // and additionally breaks after the sentence ending in "worm."
    let input = "First sentence ends here. A question asks:\nthen it continues.\n";
    let expected = "First sentence ends here.\nA question asks:\nthen it continues.\n";
    assert_eq!(run(input), expected);
}

#[test]
fn soft_break_only_breaks_on_newline_not_space() {
    // A space between sentences-on-one-line still gets a sentence break, but a
    // mid-sentence space never introduces a break on its own.
    let input = "one two three four five.\n";
    assert_eq!(run(input), "one two three four five.\n");
}

#[test]
fn long_sentence_without_breaks_stays_on_one_line() {
    // Width is ignored: a single long sentence with no soft break is untouched.
    let input =
        "This is one long sentence that runs well past eighty columns yet carries no soft break.\n";
    assert_eq!(run(input), input);
}

#[test]
fn trailing_newline_does_not_emit_empty_line() {
    let input = "Only one sentence.\n";
    assert_eq!(run(input), "Only one sentence.\n");
}

#[test]
fn preserves_authored_clause_break_after_comma() {
    let input = "First clause,\nsecond clause. Next sentence. Done.\n";
    let expected = "First clause,\nsecond clause.\nNext sentence.\nDone.\n";
    assert_eq!(run(input), expected);
}

#[test]
fn abbreviations_do_not_trigger_breaks() {
    // `e.g.` mid-sentence must not split; the real sentence end after "parser"
    // does, and the authored break after the comma survives.
    let input = "We use tools, e.g. the parser,\nand more. End.\n";
    let expected = "We use tools, e.g. the parser,\nand more.\nEnd.\n";
    assert_eq!(run(input), expected);
}

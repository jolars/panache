use panache_formatter::config::WrapMode;
use panache_formatter::{Config, format};

fn cfg_preserve() -> Config {
    Config {
        wrap: Some(WrapMode::Preserve),
        ..Default::default()
    }
}

fn cfg_sentence() -> Config {
    Config {
        wrap: Some(WrapMode::Sentence),
        ..Default::default()
    }
}

#[test]
fn paragraph_preserve_keeps_line_breaks() {
    let input = "\
First line with manual
breaks that should
stay the same.
";

    let out = format(input, Some(cfg_preserve()), None);
    // Idempotency
    let out2 = format(&out, Some(cfg_preserve()), None);
    assert_eq!(out, out2);

    // Preserve mode should keep paragraph line breaks exactly
    assert_eq!(out, input);
}

#[test]
fn block_quote_preserve_keeps_line_breaks() {
    let input = "\
> First line with manual
> breaks that should
> stay the same.
";

    let out = format(input, Some(cfg_preserve()), None);
    // Idempotency
    let out2 = format(&out, Some(cfg_preserve()), None);
    assert_eq!(out, out2);

    // Preserve mode should keep quoted line breaks exactly
    assert_eq!(out, input);
}

#[test]
fn list_item_preserve_keeps_line_breaks() {
    let input = "\
1. **Simple model**: Convert each of the `r length(levels(forested_train$county))` counties to binary indicators and drop any predictors with zero-variance. 
 2. **Normalization model**: Begin with the simple model and add a normalization step that applies the ORD transformation to all numeric predictors. 
 3. **Encoding model**:  Build on the normalization model by replacing the county dummy indicators with effect encoding.
 4. **Interaction model**:  extend the encoding by including interaction terms. 
 5. **Spline model**:  Enhance the interaction model further with ten natural spline basis functions for a set of predictors.
";

    let out = format(input, Some(cfg_preserve()), None);
    let out2 = format(&out, Some(cfg_preserve()), None);
    assert_eq!(out, out2);
    let expected = "\
1. **Simple model**: Convert each of the `r length(levels(forested_train$county))` counties to binary indicators and drop any predictors with zero-variance. 
2. **Normalization model**: Begin with the simple model and add a normalization step that applies the ORD transformation to all numeric predictors. 
3. **Encoding model**:  Build on the normalization model by replacing the county dummy indicators with effect encoding.
4. **Interaction model**:  extend the encoding by including interaction terms. 
5. **Spline model**:  Enhance the interaction model further with ten natural spline basis functions for a set of predictors.
";
    assert_eq!(out, expected);
}

#[test]
fn paragraph_sentence_wraps_per_sentence() {
    let input = "First sentence. Second sentence! Third sentence?\n";
    let expected = "First sentence.\nSecond sentence!\nThird sentence?\n";

    let out = format(input, Some(cfg_sentence()), None);
    let out2 = format(&out, Some(cfg_sentence()), None);
    assert_eq!(out, out2);
    assert_eq!(out, expected);
}

#[test]
fn block_quote_sentence_wraps_per_sentence() {
    let input = "\
> First sentence. Second sentence; third sentence.
";
    let expected = "\
> First sentence.
> Second sentence; third sentence.
";

    let out = format(input, Some(cfg_sentence()), None);
    let out2 = format(&out, Some(cfg_sentence()), None);
    assert_eq!(out, out2);
    assert_eq!(out, expected);
}

#[test]
fn list_item_sentence_wraps_per_sentence() {
    let input = "\
- First sentence. Second sentence! Third sentence?
";
    let expected = "\
- First sentence.
  Second sentence!
  Third sentence?
";

    let out = format(input, Some(cfg_sentence()), None);
    let out2 = format(&out, Some(cfg_sentence()), None);
    assert_eq!(out, out2);
    assert_eq!(out, expected);
}

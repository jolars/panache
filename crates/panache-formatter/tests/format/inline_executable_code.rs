use panache_formatter::{ConfigBuilder, format};

#[test]
fn rmarkdown_classic_inline_exec_normalizes_to_braced_form() {
    let mut cfg = ConfigBuilder::default().build();
    cfg.parser_extensions.rmarkdown_inline_code = true;
    cfg.parser_extensions.quarto_inline_code = false;

    let input = "`3 == `r 2 + 1``\n";
    let output = format(input, Some(cfg), None);
    assert_eq!(output, "`3 ==`{r} 2 + 1\\`\\`\n");
}

#[test]
fn quarto_braced_inline_exec_stays_braced_form() {
    let mut cfg = ConfigBuilder::default().build();
    cfg.parser_extensions.rmarkdown_inline_code = false;
    cfg.parser_extensions.quarto_inline_code = true;

    let input = "`3 == `{r} 2 + 1``\n";
    let output = format(input, Some(cfg), None);
    assert_eq!(output, "`3 ==`{r} 2 + 1\\`\\`\n");
}

#[test]
fn inline_exec_formatting_is_idempotent() {
    let mut cfg = ConfigBuilder::default().build();
    cfg.parser_extensions.rmarkdown_inline_code = true;
    cfg.parser_extensions.quarto_inline_code = true;

    let input = "`3 == `r 2 + 1``\n";
    let first = format(input, Some(cfg.clone()), None);
    let second = format(&first, Some(cfg), None);
    assert_eq!(first, second);
}

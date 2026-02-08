use panache::config::WrapMode;
use panache::{Config, format};

async fn cfg_preserve() -> Config {
    Config {
        wrap: Some(WrapMode::Preserve),
        ..Default::default()
    }
}

#[tokio::test]
async fn paragraph_preserve_keeps_line_breaks() {
    let input = "\
First line with manual
breaks that should
stay the same.
";

    let out = format(input, Some(cfg_preserve().await)).await;
    // Idempotency
    let out2 = format(&out, Some(cfg_preserve().await)).await;
    assert_eq!(out, out2);

    // Preserve mode should keep paragraph line breaks exactly
    assert_eq!(out, input);
}

#[tokio::test]
async fn block_quote_preserve_keeps_line_breaks() {
    let input = "\
> First line with manual
> breaks that should
> stay the same.
";

    let out = format(input, Some(cfg_preserve().await)).await;
    // Idempotency
    let out2 = format(&out, Some(cfg_preserve().await)).await;
    assert_eq!(out, out2);

    // Preserve mode should keep quoted line breaks exactly
    assert_eq!(out, input);
}

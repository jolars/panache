use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub fn format_qmd(input: &str, line_width: Option<usize>) -> String {
    let cfg = panache::ConfigBuilder::default()
        .line_width(line_width.unwrap_or(80))
        .build();
    panache::format(input, Some(cfg), None)
}

// Optional: expose tokenizer/AST for debugging
#[wasm_bindgen]
pub fn tokenize_debug(input: &str) -> String {
    // return a simple debug string; or serialize to JSON if you add serde
    let tree = panache::parse(input, None);
    format!("{tree:#?}")
}

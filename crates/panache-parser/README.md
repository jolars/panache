# panache-parser

Lossless CST parser and typed syntax wrappers for Pandoc Markdown, Quarto, and R
Markdown.

## Status

This crate is extracted from the Panache project and is evolving alongside it.
The API is still early and may change between releases.

## Usage

```rust
use panache_parser::parse;

let tree = parse("# Heading\n\nParagraph text.", None);
println!("{:#?}", tree);
```

## Documentation

- API docs: <https://docs.rs/panache-parser>
- Project: <https://github.com/jolars/panache>

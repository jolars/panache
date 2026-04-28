# panache-cli

[Panache](https://panache.bz) is an LSP, formatter, and linter for Pandoc
markdown, Quarto, and RMarkdown documents.

## Install

```sh
npm install -g panache-cli
```

This installs the `panache` command globally. The package detects your platform
at install time and pulls in a prebuilt binary via npm's optional dependencies
--- no Rust toolchain or postinstall download required.

You can also use it without a global install:

```sh
npx panache-cli format document.qmd
```

## Usage

```sh
panache format document.qmd     # format in place
panache format <document.qmd    # read stdin, write stdout
panache lint document.qmd       # lint
panache lint --fix document.qmd # lint and apply auto-fixes
panache lsp                     # start the language server
```

See `panache --help` and the [documentation](https://panache.bz) for the full
feature list and configuration reference.

## Supported platforms

Prebuilt binaries are shipped for:

- Linux x64 (glibc and musl)
- Linux arm64 (glibc and musl)
- macOS x64 (Intel) and arm64 (Apple Silicon)
- Windows x64 and arm64

If your platform isn't covered, install via
[Cargo](https://crates.io/crates/panache),
[PyPI](https://pypi.org/project/panache-cli/), or one of the other methods
listed at <https://panache.bz>.

## License

MIT --- see [LICENSE](./LICENSE).

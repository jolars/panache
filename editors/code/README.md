# Panache

A language server for Pandoc, Quarto, and R Markdown.

## Features

- Starts `panache lsp` automatically
- Document formatting via LSP
- Diagnostics and code actions from Panache
- Works for Markdown, Quarto (`.qmd`), and R Markdown (`.Rmd`, `.rmd`)

## Binary Installation

By default, the extension downloads a platform-specific `panache` binary from
GitHub releases and uses that binary for the language server.

You can also provide your own binary path:

```json
{
  "panache.downloadBinary": false,
  "panache.commandPath": "panache"
}
```

## Settings

- `panache.downloadBinary`: auto-download Panache binary (default: `true`)
- `panache.releaseTag`: release tag to install (default: `"latest"`)
- `panache.githubRepo`: GitHub repo for downloads (default: `"jolars/panache"`)
- `panache.commandPath`: fallback command path
- `panache.serverArgs`: extra args after `panache lsp`
- `panache.serverEnv`: extra environment variables
- `panache.trace.server`: LSP trace level (`off`, `messages`, `verbose`)

## Links

- Main repository: <https://github.com/jolars/panache>
- Documentation: <https://jolars.github.io/panache/>

# Panache

A language server for Pandoc, Quarto, and R Markdown.

## Quick start

1. Install the **Panache** extension.
2. Open a regular Markdown (`.md`, Pandoc-style), Quarto (`.qmd`), or R Markdown
   (`.Rmd`, `.rmd`) file.
3. The extension starts `panache lsp` automatically.

By default, the extension downloads a platform-specific `panache` binary from
GitHub releases on first use.

## Features

- Starts `panache lsp` automatically when you open supported documents.
- Formats documents using Panache's formatter, including Pandoc-style constructs
  such as fenced divs, tables, math, citations, and attributes.
- Surfaces Panache diagnostics and code actions in the editor (including
  auto-fixable lint rules such as heading hierarchy).
- Works for regular Markdown (`.md`, Pandoc-style), Quarto (`.qmd`), and R
  Markdown (`.Rmd`, `.rmd`).

## Binary Installation

By default, the extension downloads a platform-specific `panache` binary from
GitHub releases and uses that binary for the language server.

When `panache.version` is set to `latest`, the extension automatically skips
component-only tags and selects the most recent stable CLI release that contains
a matching platform asset.

You can also provide your own binary path:

```json
{
  "panache.downloadBinary": false,
  "panache.commandPath": "panache"
}
```

## Common setup examples

Use a local binary and disable downloads:

```json
{
  "panache.downloadBinary": false,
  "panache.commandPath": "/usr/local/bin/panache"
}
```

Pin to a specific release from a specific repository:

```json
{
  "panache.version": "2.20.0",
  "panache.githubRepo": "jolars/panache"
}
```

Use `panache.releaseTag` only if you need an exact tag override:

```json
{
  "panache.releaseTag": "v2.20.0"
}
```

## Requirements and troubleshooting

- **NixOS**: auto-download is skipped by default unless explicitly configured.
  Set `panache.commandPath` to your installed binary.
- **Offline / restricted networks / proxies**: set `panache.downloadBinary` to
  `false` and provide `panache.commandPath`.
- If download fails, the extension shows a warning and falls back to
  `panache.commandPath`.
- The extension contributes `quarto` (`.qmd`) and `rmarkdown` (`.Rmd`, `.rmd`)
  language registrations, so it works even without installing a separate Quarto
  extension. If Quarto is also installed, both can coexist.

## Settings

- `panache.downloadBinary`: auto-download Panache binary (default: `true`)
- `panache.version`: version to install (default: `"latest"`)
- `panache.releaseTag`: advanced exact tag override (takes precedence if
  explicitly set)
- `panache.githubRepo`: GitHub repo for downloads (default: `"jolars/panache"`)
- `panache.commandPath`: fallback command path
- `panache.serverArgs`: extra args after `panache lsp`
- `panache.serverEnv`: extra environment variables
- `panache.extraPath`: extra PATH entries prepended for the language server
  process
- `panache.trace.server`: LSP trace level (`off`, `messages`, `verbose`)
- `panache.experimental.incrementalParsing`: enable experimental incremental
  parsing in LSP (default: `false`)

If external tools (for example `air` for R code chunks) work in your terminal
but not inside the editor, set `panache.extraPath` to include their install
directory:

```json
{
  "panache.extraPath": ["C:\\Users\\<you>\\.local\\bin"]
}
```

## Security and trust

When `panache.downloadBinary` is enabled, binaries are downloaded from GitHub
releases configured by `panache.githubRepo` and either `panache.version` or
`panache.releaseTag` (if explicitly set).

## Links

- Main repository: <https://github.com/jolars/panache>
- Documentation: <https://jolars.github.io/panache/>

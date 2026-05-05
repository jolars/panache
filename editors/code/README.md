# Panache

A language server for Markdown, Quarto, and R Markdown.

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

## Commands

- `Panache: Restart Server` --- stops and restarts the Panache language server
  (re-reads settings and re-resolves the binary). Useful if the LSP gets wedged
  or after changing settings such as `panache.version` or
  `panache.executablePath`.

## Binary Installation

By default, the extension uses a `panache` binary that ships inside the
extension itself (one platform-specific VSIX per OS/architecture). No download,
no GitHub round-trip, and the language server starts on first activation even on
restricted or offline networks. Behavior is controlled by
`panache.executableStrategy`:

- `bundled` (default) --- use the binary that ships inside the extension. If
  you're on a platform without a platform-specific build (or you've installed
  the universal VSIX), the extension falls back to downloading a matching binary
  from GitHub releases.
- `environment` --- look for `panache` on the system `PATH`.
- `path` --- use the binary at `panache.executablePath`.

If you set `panache.version` or `panache.releaseTag` explicitly, the bundled
binary is skipped and the requested version is downloaded from GitHub. When
`panache.version` is `latest`, the extension automatically skips component-only
tags and selects the most recent stable CLI release that contains a matching
platform asset.

You can also provide your own path to the binary:

```json
{
  "panache.executableStrategy": "path",
  "panache.executablePath": "/usr/local/bin/panache"
}
```

## Common setup examples

Use a local binary at a fixed path:

```json
{
  "panache.executableStrategy": "path",
  "panache.executablePath": "/usr/local/bin/panache"
}
```

Use whatever `panache` is on your `PATH`:

```json
{
  "panache.executableStrategy": "environment"
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

- **NixOS**: the bundled binary won't run because of the dynamic loader path.
  Set `panache.executableStrategy` to `path` (with `panache.executablePath`) or
  `environment` if `panache` is on your `PATH`. On the legacy
  `panache.downloadBinary` path the auto-download is also skipped on NixOS by
  default.
- **Offline / restricted networks / proxies**: the bundled-binary default works
  without network access. Only the explicit-version download paths
  (`panache.version` / `panache.releaseTag`) require GitHub connectivity.
- If a download fall-through fails, the extension shows a warning and falls back
  to looking up `panache` on the system `PATH`.
- The extension contributes `quarto` (`.qmd`) and `rmarkdown` (`.Rmd`, `.rmd`)
  language registrations, so it works even without installing a separate Quarto
  extension. If Quarto is also installed, both can coexist.

## Settings

Panache registers itself as the default formatter for `[quarto]` and
`[rmarkdown]` files. Plain `[markdown]` is left alone --- opt in with
`"editor.defaultFormatter": "jolars.panache"` in your settings if you want it.

- `panache.executableStrategy`: how to locate the `panache` binary --- `bundled`
  (default), `environment`, or `path`.
- `panache.executablePath`: path to the binary, used only when
  `executableStrategy` is `path`.
- `panache.version`: version to install (default: `"latest"`)
- `panache.releaseTag`: advanced exact tag override (takes precedence if
  explicitly set)
- `panache.githubRepo`: GitHub repo for downloads (default: `"jolars/panache"`)
- `panache.downloadBinary` *(deprecated)*: superseded by
  `panache.executableStrategy`.
- `panache.commandPath` *(deprecated)*: superseded by `panache.executablePath`
  (with `executableStrategy` set to `path`).
- `panache.serverArgs`: extra args after `panache lsp`
- `panache.serverEnv`: extra environment variables
- `panache.extraPath`: extra PATH entries prepended for the language server
  process
- `panache.logLevel`: log level for the language server, mapped to `RUST_LOG`
  (`off`, `error`, `warn`, `info`, `debug`, `trace`; unset by default).
  `panache.serverEnv.RUST_LOG` overrides this if both are set.
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

When `panache.executableStrategy` is `bundled` (the default), the extension
prefers the binary that shipped inside the VSIX. If no bundled binary is
available, or `panache.version` / `panache.releaseTag` is set explicitly, it
downloads from GitHub releases configured by `panache.githubRepo` (default
`jolars/panache`).

# Panache <img src='https://raw.githubusercontent.com/jolars/panache/refs/heads/main/images/logo.png' align="right" width="139" />

[![Build and
Test](https://github.com/jolars/panache/actions/workflows/build-and-test.yml/badge.svg)](https://github.com/jolars/panache/actions/workflows/build-and-test.yml)
[![Crates.io](https://img.shields.io/crates/v/panache.svg)](https://crates.io/crates/panache)
[![Open
VSX](https://img.shields.io/open-vsx/v/jolars/panache)](https://open-vsx.org/extension/jolars/panache)
[![VS
Code](https://vsmarketplacebadges.dev/version-short/jolars.panache.svg)](https://marketplace.visualstudio.com/items?itemName=jolars.panache)
[![PyPI
version](https://badge.fury.io/py/panache-cli.svg?icon=si%3Apython)](https://badge.fury.io/py/panache-cli)
[![npm
version](https://badge.fury.io/js/@panache-cli%2Fpanache.svg?icon=si%3Anpm)](https://badge.fury.io/js/@panache-cli%2Fpanache)
[![codecov](https://codecov.io/gh/jolars/panache/graph/badge.svg?token=uaBVOBfILv)](https://codecov.io/gh/jolars/panache)

A language server, formatter, and linter for Markdown, Quarto, and R Markdown,
built in Rust with a lossless CST parser and support for external formatters and
linters on code blocks.

## Installation

### From crates.io

If you have Rust installed, the easiest way is likely to install from
[crates.io](https://crates.io/crates/panache):

```bash
cargo install panache
```

### Pre-built Binaries

Alternatively, you can install pre-built binary packages from the [releases
page](https://github.com/jolars/panache/releases) for Linux, macOS, and Windows.
For Linux, packages are available for generic distributions (tarballs) as well
as Debian/Ubuntu (`.deb`) and Fedora/RHEL/openSUSE (`.rpm`).

If you prefer a one-liner installer that picks the right release artifact for
your platform, you can use the installer scripts below. These scripts are
fetched directly from this repository and then download the latest matching
Panache CLI release asset for your platform, installing to a user-local
directory by default. If you prefer, download and inspect the script before
running it.

For macOS and Linux:

```sh
curl --proto '=https' --tlsv1.2 -LsSf \
    https://raw.githubusercontent.com/jolars/panache/refs/heads/main/scripts/panache-installer.sh | sh
```

For Windows PowerShell:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -Command "irm https://raw.githubusercontent.com/jolars/panache/refs/heads/main/scripts/panache-installer.ps1 | iex"
```

### Arch Linux

There are also two recipies available for Arch Linux in the AUR:
[panache](https://aur.archlinux.org/packages/panache/) and
[panache-bin](https://aur.archlinux.org/packages/panache-bin/). The first builds
from source for your system, the second uses precompiled binaries attatched to
GH releases. Install either using `yay` or your favorite AUR helper:

```bash
yay -S panache
yay -S panache-bin
```

### NixOS

Panache is available in NixOS via the `panache` package in `nixpkgs`. To add it
to your system configuration, include it in the `environment.systemPackages`:

```nix
{ pkgs, ... }:

{
  environment.systemPackages = [
    pkgs.panache
  ];
}
```

### From PyPI (via uv or pipx)

Install with [uv](https://docs.astral.sh/uv/) or [pipx](https://pipx.pypa.io/):

```bash
# One-shot run, no install:
uvx --from panache-cli panache format path/to/file.qmd

# Persistent install:
uv tool install panache-cli
# or
pipx install panache-cli
```

### From NPM (via npmx)

Install with [npx](https://www.npmjs.com/package/npx) or
[npmx](https://www.npmjs.com/package/npmx):

```bash
# One-shot run, no install:
npx @panache-cli/panache format path/to/file.qmd

# Persistent install:
npm install -g @panache-cli/panache
```

### VS Code Extension

If you are running VS Code or an editor that supports VS Code extensions (like
Positron), you can install the [Panache
extension](https://marketplace.visualstudio.com/items?itemName=jolars.panache)
from the VS Code Marketplace or the [Open VSX
extension](https://open-vsx.org/extension/jolars/panache), which will
automatically also install the `panache` CLI and start the language server when
editing supported files.

### Development Version

To install the latest development version, you can run

```bash
cargo install --git https://github.com/jolars/panache.git panache
```

This presumes you have a working and up-to-date Rust toolchain (stable, 2024
edition) installed. You also need to have `cargo` in your `PATH` if you want to
use the `panache` command directly after installation.

## Usage

Panache provides a single CLI interface for formatting, linting, and running the
LSP server.

### Formatting

To format a file in place, simply run:

```bash
panache format document.qmd
```

You can also format from stdin by piping content into `panache format`:

```bash
cat <file> | panache format
```

`panache format` supports glob patterns and recursive directory formatting:

```bash
panache format **/*.{qmd,md}
```

You can use Panache as a linter via the `--check` flag to check if files are
already formatted without making changes:

```bash
panache format --check document.qmd
```

#### External Code Formatters

Panache supports external formatters for code blocks. For example, you can
configure it to run `air` on R code blocks and `ruff` on Python code blocks:

```toml
[formatters]
r = "air"
python = "ruff"
javascript = "prettier"
typescript = "prettier" # Reuse same formatter
```

You can setup custom formatters or modify built-in presets with additional
arguments:

```toml
[formatters]
python = ["isort", "black"]
javascript = "foobar"

[formatters.isort]
args = ["--profile=black"]

[formatters.myformatters]
cmd = "foobar"
args = ["--print-width=100"]
stdin = true
```

### Linting

Panache also features a linter that can report formatting issues and optionally
auto-fix them. To run the linter, use:

```bash
panache lint document.qmd
```

As with `panache format`, you can use glob patterns and recursive formatting:

```bash
panache lint **/*.{qmd,md}
```

#### External Linters

As with formatting, Panache supports external linters for code blocks. These are
configured in the `[linters]` section of the configuration, but due to the
complexity of linting, including dealing with auto-fixing, external linters
cannot be customized and only support presets and at the moment only support R
via the `jarl` linter:

```toml
# Enable R linting
[linters]
r = "jarl" # R linter with JSON output
```

### Language Server

Panache implements the language server protocol (LSP) to provide editor features
like formatting, diagnostics, code actions, and more. See [the language server
documentation](https://panache.bz/guide/lsp) for guides on how to connect
Panache to your editor and configure LSP features.

The list of LSP features supported by Panache includes, among others:

- Document formatting (full document, incremental and range)
- Diagnostics with quick fixes
- Code actions for refactoring
  - Convert between loose/compact lists
  - Convert between inline/reference footnotes
- Document symbols/outline
- Folding ranges
- Go to definition for references and footnotes
- Quaro and Bookdown project awareness

## Configuration

Panache looks for a configuration in:

1. `.panache.toml` or `panache.toml` in current directory or parent directories
2. `$XDG_CONFIG_HOME/panache/config.toml` (usually
   `~/.config/panache/config.toml`)

### Example

```toml
# Markdown flavor and line width
flavor = "quarto"
line-width = 80
line-ending = "auto"

# Formatting style
[format]
wrap = "reflow"

# External code formatters (opt-in)
[formatters]
python = ["isort", "black"] # Sequential formatting
r = "air"                   # Built-in preset
javascript = "prettier"     # Reusable definitions
typescript = "prettier"
yaml = "yamlfmt"            # Formats both code blocks AND frontmatter

# Customize formatters
[formatters.prettier]
prepend-args = ["--print-width=100"]

# External code linters
[linters]
r = "jarl"      # Enable R linting
python = "ruff"
```

See [examples/panache.toml](./examples/panache.toml) for a complete
configuration reference.

## Integrations

### GitHub Actions

For CI, use the dedicated GitHub Action:

```yaml
- uses: jolars/panache-action@v1
```

See the [Integrations documentation](https://panache.bz/guide/integrations) for
configuration options.

### Pre-commit Hooks

Panache integrates with [pre-commit](https://pre-commit.com/) to automatically
format and lint your files before committing.

**Installation:**

First, install pre-commit if you haven't already:

```bash
pip install pre-commit
# or
brew install pre-commit
```

Then add Panache to your `.pre-commit-config.yaml`:

```yaml
repos:
  - repo: https://github.com/jolars/panache
    rev: v2.16.0 # Use the latest version
    hooks:
      - id: panache-format # Format files
      - id: panache-lint # Lint and auto-fix issues
```

Install the hooks:

```bash
pre-commit install
```

Panache will now automatically run on your staged `.qmd`, `.md`, and `.Rmd`
files before each commit.

See [examples/pre-commit-config.yaml](examples/pre-commit-config.yaml) for more
configuration options.

## Motivation

I wanted a formatter that understands Quarto and Pandoc syntax. I have tried to
use Prettier as well as mdformat, but both fail to handle some of the particular
syntax used in Quarto documents, such as fenced divs and some of the table
syntax.

For a side-by-side overview of how Panache compares to Prettier, Pandoc, rumdl,
mdformat, mado, markdownlint, markdownlint-cli2, and marksman, see the
[comparison page](https://panache.bz/guide/comparison). For benchmarks against
the same set of tools, see the [performance
page](https://panache.bz/guide/performance).

## Design Goals and Scope

- Full LSP implementation with formatting, diagnostics, code actions, and more
- Standalone CLI for both formatting and linting
- Support for Quarto, Pandoc, and R Markdown syntax
- Lossless CST-based parsing
- Idempotent formatting
- Semi-opinionated defaults with configurable style options for common
  formatting decisions
- Support for running external formatters and linters on code blocks, with
  built-in presets for popular languages and tools

## Acknowledgements

The development of Panache has simplified considerably thanks to the extensive
documentation, well-structured code, and testing infrastructure provided by
Pandoc. We also owe significant debt to the rust-analyzer project, on which
Panche is heavily inspired.

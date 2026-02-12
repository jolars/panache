# panache <img src='https://raw.githubusercontent.com/jolars/panache/refs/heads/main/images/logo.png' align="right" width="139" />

[![Build and Test](https://github.com/jolars/panache/actions/workflows/build-and-test.yml/badge.svg)](https://github.com/jolars/panache/actions/workflows/build-and-test.yml)
[![Crates.io](https://img.shields.io/crates/v/panache.svg)](https://crates.io/crates/panache)

A formatter, linter, and LSP for Quarto (`.qmd`), Pandoc, and Markdown files.

## Work in Progress

This project is in early development. Expect bugs, missing features, and breaking changes.

## Installation

### From crates.io (Recommended)

```bash
cargo install panache
```

### Pre-built Binaries

Download pre-built binaries from the [releases page](https://github.com/jolars/panache/releases). Available for:

- Linux (x86_64, ARM64)
- macOS (Intel, Apple Silicon)
- Windows (x86_64)

Each archive includes the binary, man pages, and shell completions.

### Linux Packages

For Debian/Ubuntu systems:

```bash
# Download the .deb from releases
sudo dpkg -i panache_*.deb
```

For Fedora/RHEL/openSUSE systems:

```bash
# Download the .rpm from releases
sudo rpm -i panache-*.rpm
```

Packages include:

- Binary at `/usr/bin/panache`
- Man pages for all subcommands
- Shell completions (bash, fish, zsh)

## Usage

### CLI Formatting

```bash
# Format a file and output to stdout
panache format document.qmd

# Format a file in place
panache format --write document.qmd

# Check if a file is formatted
panache format --check document.qmd

# Format from stdin
cat document.qmd | panache format

# Parse and inspect the AST (for debugging)
panache parse document.qmd
```

### Language Server (LSP)

panache includes a built-in Language Server Protocol implementation for editor integration.

**Start the server:**

```bash
panache lsp
```

**Editor Configuration:**

The LSP communicates over stdin/stdout and provides document formatting capabilities.

<details>
<summary>Neovim (using nvim-lspconfig)</summary>

```lua
-- Add to your LSP config
local lspconfig = require("lspconfig")
local configs = require("lspconfig.configs")

-- Define panache LSP
if not configs.panache then
	configs.panache = {
		default_config = {
			cmd = { "panache", "lsp" },
			filetypes = { "quarto", "markdown", "rmarkdown" },
			root_dir = lspconfig.util.root_pattern(".panache.toml", "panache.toml", ".git"),
			settings = {},
		},
	}
end

-- Enable it
lspconfig.panache.setup({})
```

Format on save:

```lua
vim.api.nvim_create_autocmd("BufWritePre", {
	pattern = { "*.qmd", "*.md", "*.rmd" },
	callback = function()
		vim.lsp.buf.format({ async = false })
	end,
})
```

</details>

<details>
<summary>VS Code</summary>

Install a generic LSP client extension like [vscode-languageserver-node](https://marketplace.visualstudio.com/items?itemName=Microsoft.vscode-languageserver-node), then configure in `settings.json`:

```json
{
  "languageServerExample.server": {
    "command": "panache",
    "args": ["lsp"],
    "filetypes": ["quarto", "markdown", "rmarkdown"]
  },
  "editor.formatOnSave": true
}
```

Or use the [Custom LSP](https://marketplace.visualstudio.com/items?itemName=josa.custom-lsp) extension.

</details>

<details>
<summary>Helix</summary>

Add to `~/.config/helix/languages.toml`:

```toml
[[language]]
name = "markdown"
language-servers = ["panache-lsp"]
auto-format = true

[language-server.panache-lsp]
command = "panache"
args = ["lsp"]
```

</details>

**Configuration:** The LSP automatically discovers `.panache.toml` from your workspace root.

## Configuration

panache looks for a configuration in:

1. `.panache.toml` or `panache.toml` in current directory or parent directories
2. `~/.config/panache/config.toml`

### Example config

```toml
# Markdown flavor and line width
flavor = "quarto"
line-width = 80
line-ending = "auto"
wrap = "reflow"

# External code formatters
# Note: R (air) and Python (ruff) are enabled by default - no config needed!

# Switch to a different preset
[formatters.r]
preset = "styler"

# Or use full custom configuration
[formatters.python]
cmd = "black"
args = ["-", "--line-length=88"]

# Add formatters for other languages
[formatters.rust]
cmd = "rustfmt"
```

See `.panache.toml.example` for a complete configuration reference.

### External Code Formatters

panache includes **zero-config support** for formatting R and Python code blocks:

- **R**: Uses `air format` by default (if installed)
- **Python**: Uses `ruff format` by default (if installed)
- Formatters execute in parallel with panache's markdown formatting
- On error, original code is preserved (no warnings needed)

**No configuration needed!** If `air` or `ruff` are in your PATH, they just work.

**Switch presets or add more formatters:**

```toml
# Use different preset
[formatters.r]
preset = "styler"  # Available: "air" (default), "styler"

# Full custom config (overrides default)
[formatters.python]
cmd = "black"
args = ["-", "--line-length=88"]

# Add formatters for other languages
[formatters.rust]
cmd = "rustfmt"
```

**Additional details:**

- Formatters respect their own config files (`.prettierrc`, `pyproject.toml`, etc.)
- Support both stdin/stdout and file-based formatters
- 30 second timeout per formatter invocation

## Motivation

I wanted a formatter that understands Quarto and Pandoc syntax. I have tried
to use Prettier as well as mdformat, but both fail to handle some of
the particular syntax used in Quarto documents, such as fenced divs and
some of the table syntax.

## Design Goals

- Support Quarto, Pandoc, and Markdown syntax
- Be fast
- Be configurable, but have sane defaults (that most people can
  agree on)
- Format math
- ✅ Hook into external formatters for code blocks (e.g. `styler` for R, `black` for Python)

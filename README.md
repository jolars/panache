# panache <img src='https://raw.githubusercontent.com/jolars/panache/refs/heads/main/images/logo.png' align="right" width="139" />

[![Build and
Test](https://github.com/jolars/panache/actions/workflows/build-and-test.yml/badge.svg)](https://github.com/jolars/panache/actions/workflows/build-and-test.yml)
[![Crates.io](https://img.shields.io/crates/v/panache.svg)](https://crates.io/crates/panache)
[![codecov](https://codecov.io/gh/jolars/panache/graph/badge.svg?token=uaBVOBfILv)](https://codecov.io/gh/jolars/panache)

A formatter, linter, and LSP for Quarto (`.qmd`), Pandoc, and Markdown files.

## Work in Progress

This project is in early development. Expect bugs, missing features, and
breaking changes.

## Installation

### From crates.io (Recommended)

```bash
cargo install panache
```

### Pre-built Binaries

Download pre-built binaries from the [releases
page](https://github.com/jolars/panache/releases). Available for:

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

### Formatting

```bash
# Format a file in place
panache format document.qmd

# Check if a file is formatted
panache format --check document.qmd

# Format from stdin
cat document.qmd | panache format

# Format all .qmd and .md files in directory, recursively
panache format **/*.{qmd,md}
```

### Linting

```bash
# Lint a file
panache lint document.qmd

# Lint entire working directory
panache lint .
```

### Language Server (LSP)

panache includes a built-in Language Server Protocol implementation for editor
integration.

**Start the server:**

```bash
panache lsp
```

**Editor Configuration:**

The LSP communicates over stdin/stdout and provides document formatting
capabilities.

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

**Configuration:** The LSP automatically discovers `.panache.toml` from your
workspace root.

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

# Formatting style
[style]
wrap = "reflow"

# External code formatters (opt-in)
[formatters]
python = ["isort", "black"]  # Sequential formatting
r = "air"                    # Built-in preset
javascript = "prettier"      # Reusable definitions
typescript = "prettier"
yaml = "yamlfmt"             # Formats both code blocks AND frontmatter

# Customize formatters (optional)
[formatters.prettier]
args = ["--print-width=100"]

# External code linters (opt-in)
[linters]
r = "jarl"  # Enable R linting
```

See `.panache.toml.example` for a complete configuration reference.

### External Code Formatters

panache supports external formatters for code blocks—**opt-in and easy to
enable**:

```toml
[formatters]
r = "air"           # Available presets: "air", "styler"
python = "ruff"     # Available presets: "ruff", "black"
javascript = "prettier"
typescript = "prettier"  # Reuse same formatter
```

**Key features:**

- **Opt-in by design** - No surprises, explicit configuration
- **Built-in presets** - Quick setup with sensible defaults
- **Sequential formatting** - Run multiple formatters in order:
  `python = ["isort", "black"]`
- **Reusable definitions** - Define once, use for multiple languages
- **Parallel execution** - Formatters run concurrently across languages
- **Graceful fallback** - Missing tools preserve original code (no errors)
- **Custom config** - Full control with `cmd`, `args`, `stdin` fields

**Custom formatter definitions:**

```toml
[formatters]
python = ["isort", "black"]
javascript = "prettier"

[formatters.prettier]
args = ["--print-width=100"]

[formatters.isort]
cmd = "isort"
args = ["-"]
```

**Additional details:**

- Formatters respect their own config files (`.prettierrc`, `pyproject.toml`,
  etc.)
- Support both stdin/stdout and file-based formatters
- 30 second timeout per formatter

### External Code Linters

panache supports external linters for code blocks—**opt-in via configuration**:

```toml
# Enable R linting
[linters]
r = "jarl"  # R linter with JSON output
```

**Key features:**

- **Opt-in by design** - Only runs if configured
- **Stateful code analysis** - Concatenates all code blocks of same language to
  handle cross-block dependencies
- **LSP integration** - Diagnostics appear inline in your editor
- **CLI support** - `panache lint` shows external linter issues
- **Line-accurate diagnostics** - Reports exact line/column locations

**How it works:**

1. Collects all code blocks of each configured language
2. Concatenates blocks with blank-line preservation (keeps original line
   numbers)
3. Runs external linter on concatenated code
4. Maps diagnostics back to original document positions

**Supported linters:**

- **jarl** - R linter with structured JSON output

**Note:** Auto-fixes from external linters are currently disabled due to byte
offset mapping complexity. Diagnostics work perfectly.

## Motivation

I wanted a formatter that understands Quarto and Pandoc syntax. I have tried to
use Prettier as well as mdformat, but both fail to handle some of the particular
syntax used in Quarto documents, such as fenced divs and some of the table
syntax.

## Design Goals

- Full LSP implementation for editor integration
- Linting as part of LSP but also available as a standalone CLI command
- Support Quarto, Pandoc, and Markdown syntax
- Fast lossless parsing and formatting (no AST changes if already formatted)
- Be configurable, but have sane defaults (that most people can agree on)
- Format math
- Hook into external formatters for code blocks (e.g. `air` for R, `ruff` for
  Python)


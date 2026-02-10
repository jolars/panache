# panache <img src='https://raw.githubusercontent.com/jolars/panache/refs/heads/main/images/logo.png' align="right" width="139" />

[![Build and Test](https://github.com/jolars/panache/actions/workflows/build-and-test.yml/badge.svg)](https://github.com/jolars/panache/actions/workflows/build-and-test.yml)

A CLI formatter for Quarto (`.qmd`), Pandoc, and Markdown files.

## Work in Progress

This project is in **very** early development. Expect bugs, missing features, and breaking changes.

## Installation

```bash
cargo install --path .
```

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
local lspconfig = require('lspconfig')
local configs = require('lspconfig.configs')

-- Define panache LSP
if not configs.panache then
  configs.panache = {
    default_config = {
      cmd = { 'panache', 'lsp' },
      filetypes = { 'quarto', 'markdown', 'rmarkdown' },
      root_dir = lspconfig.util.root_pattern('.panache.toml', 'panache.toml', '.git'),
      settings = {},
    },
  }
end

-- Enable it
lspconfig.panache.setup{}
```

Format on save:
```lua
vim.api.nvim_create_autocmd('BufWritePre', {
  pattern = { '*.qmd', '*.md', '*.rmd' },
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
line_width = 80
line-ending = "auto"
wrap = "reflow"

# External code formatters (new!)
[formatters.r]
cmd = "styler"
args = ["--scope=spaces"]

[formatters.python]
cmd = "black"
args = ["-", "--line-length=88"]

[formatters.rust]
cmd = "rustfmt"
args = []
```

See `.panache.toml.example` for a complete configuration reference.

### External Code Formatters

panache can invoke external formatters for code blocks:

- **Formatters run in true parallel**: External formatters execute simultaneously with panache's markdown formatting for maximum performance
- Each formatter must accept code via stdin and output to stdout
- Formatters respect their own config files (`.prettierrc`, `pyproject.toml`, etc.)
- On error, original code is preserved with a warning logged
- 30-second timeout per formatter invocation

**Performance**: If your document has 3 code blocks and each formatter takes 1 second, all 3 will complete in ~1 second (not 3 seconds sequentially).

**Example**: Format R code with `styler` and Python with `black`:

```toml
[formatters.r]
cmd = "styler"
args = ["--scope=spaces"]

[formatters.python]
cmd = "black"
args = ["-"]
```

**Supported formatters** (any CLI tool that reads stdin/writes stdout):
- R: `styler`, `formatR`
- Python: `black`, `ruff format`, `autopep8`
- Rust: `rustfmt`
- JavaScript/TypeScript: `prettier`, `deno fmt`
- JSON: `jq`
- And any other stdin/stdout formatter!

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


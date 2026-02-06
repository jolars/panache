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

```bash
# Format a file and output to stdout
panache document.qmd

# Format a file in place
panache --write document.qmd

# Check if a file is formatted
panache --check document.qmd

# Format from stdin
panache document.qmd | cat
```

## Configuration

panache looks for a configuration in:

1. `.panache.toml` or `panache.toml` in current directory or parent directories
2. `~/.config/panache/config.toml`

### Example config

```toml
line_width = 80
line-ending = "auto"
```

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

Notably, I don't expect to support formatting the code blocks or yaml
frontmatter. The primary reason for this is that it is now possible
to do this already by language injection through tree sitter, for instance,
which means that a good formatter should already be able to handle this.

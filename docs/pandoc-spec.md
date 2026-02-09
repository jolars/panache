---
title: Pandoc User's Guide
author: John MacFarlane
date: 2025-09-06
---

# Pandoc's Markdown

Pandoc understands an extended and slightly revised version of
John Gruber's [Markdown] syntax.  This document explains the syntax,
noting differences from original Markdown. Except where noted, these
differences can be suppressed by using the `markdown_strict` format instead
of `markdown`. Extensions can be enabled or disabled to specify the
behavior more granularly. They are described in the following. See also
[Extensions] above, for extensions that work also on other formats.

## Table of Contents

This specification is split into the following sections:

- [Philosophy](pandoc-spec/philosophy.md)
- [Paragraphs](pandoc-spec/paragraphs.md)
- [Headings](pandoc-spec/headings.md)
- [Block quotations](pandoc-spec/block-quotations.md)
- [Verbatim (code) blocks](pandoc-spec/verbatim-code-blocks.md)
- [Line blocks](pandoc-spec/line-blocks.md)
- [Lists](pandoc-spec/lists.md)
- [Horizontal rules](pandoc-spec/horizontal-rules.md)
- [Tables](pandoc-spec/tables.md)
- [Metadata blocks](pandoc-spec/metadata-blocks.md)
- [Backslash escapes](pandoc-spec/backslash-escapes.md)
- [Inline formatting](pandoc-spec/inline-formatting.md)
- [Math](pandoc-spec/math.md)
- [Raw HTML](pandoc-spec/raw-html.md)
- [LaTeX macros](pandoc-spec/latex-macros.md)
- [Links](pandoc-spec/links.md)
- [Images](pandoc-spec/images.md)
- [Divs and Spans](pandoc-spec/divs-and-spans.md)
- [Footnotes](pandoc-spec/footnotes.md)
- [Citation syntax](pandoc-spec/citation-syntax.md)
- [Non-default extensions](pandoc-spec/non-default-extensions.md)

## External extensions (Added by other projects)

- [Bookdown extensions](pandoc-spec/bookdown-extensions.Rmd)

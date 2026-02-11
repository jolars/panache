# Math


### Extension: `tex_math_dollars` ###

Anything between two `$` characters will be treated as TeX math.  The
opening `$` must have a non-space character immediately to its right,
while the closing `$` must have a non-space character immediately to its
left, and must not be followed immediately by a digit.  Thus,
`$20,000 and $30,000` won't parse as math.  If for some reason
you need to enclose text in literal `$` characters, backslash-escape
them and they won't be treated as math delimiters.

For display math, use `$$` delimiters.  (In this case, the delimiters
may be separated from the formula by whitespace.  However, there can be
no blank lines between the opening and closing `$$` delimiters.)

TeX math will be printed in all output formats. How it is rendered
depends on the output format:

LaTeX
  ~ It will appear verbatim surrounded by `\(...\)` (for inline
    math) or `\[...\]` (for display math).

Markdown, Emacs Org mode, ConTeXt, ZimWiki
  ~ It will appear verbatim surrounded by `$...$` (for inline
    math) or `$$...$$` (for display math).

XWiki
  ~ It will appear verbatim surrounded by `{{formula}}..{{/formula}}`.

reStructuredText
  ~ It will be rendered using an [interpreted text role `:math:`].

AsciiDoc
  ~ For AsciiDoc output math will appear verbatim surrounded by
    `latexmath:[...]`. For `asciidoc_legacy` the bracketed
    material will also include inline or display math delimiters.

Texinfo
  ~ It will be rendered inside a `@math` command.

roff man, Jira markup
  ~ It will be rendered verbatim without `$`'s.

MediaWiki, DokuWiki
  ~ It will be rendered inside `<math>` tags.

Textile
  ~ It will be rendered inside `<span class="math">` tags.

RTF, OpenDocument
  ~ It will be rendered, if possible, using Unicode characters,
    and will otherwise appear verbatim.

ODT
  ~ It will be rendered, if possible, using MathML.

DocBook
  ~ If the `--mathml` flag is used, it will be rendered using MathML
    in an `inlineequation` or `informalequation` tag.  Otherwise it
    will be rendered, if possible, using Unicode characters.

Docx and PowerPoint
  ~ It will be rendered using OMML math markup.

FictionBook2
  ~ If the `--webtex` option is used, formulas are rendered as images
    using CodeCogs or other compatible web service, downloaded
    and embedded in the e-book. Otherwise, they will appear verbatim.

HTML, Slidy, DZSlides, S5, EPUB
  ~ The way math is rendered in HTML will depend on the
    command-line options selected. Therefore see [Math rendering in HTML]
    above.

[interpreted text role `:math:`]: https://docutils.sourceforge.io/docs/ref/rst/roles.html#math

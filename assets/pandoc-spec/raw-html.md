# Raw HTML


### Extension: `raw_html` ###

Markdown allows you to insert raw HTML (or DocBook) anywhere in a document
(except verbatim contexts, where `<`, `>`, and `&` are interpreted
literally).  (Technically this is not an extension, since standard
Markdown allows it, but it has been made an extension so that it can
be disabled if desired.)

The raw HTML is passed through unchanged in HTML, S5, Slidy, Slideous,
DZSlides, EPUB, Markdown, CommonMark, Emacs Org mode, and Textile
output, and suppressed in other formats.

For a more explicit way of including raw HTML in a Markdown
document, see the [`raw_attribute` extension][Extension: `raw_attribute`].

In the CommonMark format, if `raw_html` is enabled, superscripts,
subscripts, strikeouts and small capitals will be represented as HTML.
Otherwise, plain-text fallbacks will be used. Note that even if
`raw_html` is disabled, tables will be rendered with HTML syntax if
they cannot use pipe syntax.

### Extension: `markdown_in_html_blocks` ###

Original Markdown allows you to include HTML "blocks":  blocks
of HTML between balanced tags that are separated from the surrounding text
with blank lines, and start and end at the left margin.  Within
these blocks, everything is interpreted as HTML, not Markdown;
so (for example), `*` does not signify emphasis.

Pandoc behaves this way when the `markdown_strict` format is used; but
by default, pandoc interprets material between HTML block tags as Markdown.
Thus, for example, pandoc will turn

    <table>
    <tr>
    <td>*one*</td>
    <td>[a link](https://google.com)</td>
    </tr>
    </table>

into

    <table>
    <tr>
    <td><em>one</em></td>
    <td><a href="https://google.com">a link</a></td>
    </tr>
    </table>

whereas `Markdown.pl` will preserve it as is.

There is one exception to this rule:  text between `<script>`,
`<style>`, `<pre>`, and `<textarea>` tags is not interpreted as Markdown.

This departure from original Markdown should make it easier to mix
Markdown with HTML block elements.  For example, one can surround
a block of Markdown text with `<div>` tags without preventing it
from being interpreted as Markdown.

### Extension: `native_divs` ###

Use native pandoc `Div` blocks for content inside `<div>` tags.
For the most part this should give the same output as
`markdown_in_html_blocks`, but it makes it easier to write pandoc
filters to manipulate groups of blocks.

### Extension: `native_spans` ###

Use native pandoc `Span` blocks for content inside `<span>` tags.
For the most part this should give the same output as `raw_html`,
but it makes it easier to write pandoc filters to manipulate groups
of inlines.

### Extension: `raw_tex` ###

In addition to raw HTML, pandoc allows raw LaTeX, TeX, and ConTeXt to be
included in a document. Inline TeX commands will be preserved and passed
unchanged to the LaTeX and ConTeXt writers. Thus, for example, you can use
LaTeX to include BibTeX citations:

    This result was proved in \cite{jones.1967}.

Note that in LaTeX environments, like

    \begin{tabular}{|l|l|}\hline
    Age & Frequency \\ \hline
    18--25  & 15 \\
    26--35  & 33 \\
    36--45  & 22 \\ \hline
    \end{tabular}

the material between the begin and end tags will be interpreted as raw
LaTeX, not as Markdown.

For a more explicit and flexible way of including raw TeX in a
Markdown document, see the [`raw_attribute`
extension][Extension: `raw_attribute`].

Inline LaTeX is ignored in output formats other than Markdown, LaTeX,
Emacs Org mode, and ConTeXt.

### Generic raw attribute ###

### Extension: `raw_attribute` ###

Inline spans and fenced code blocks with a special
kind of attribute will be parsed as raw content with the
designated format.  For example, the following produces a raw
roff `ms` block:

    ```{=ms}
    .MYMACRO
    blah blah
    ```
And the following produces a raw `html` inline element:

    This is `<a>html</a>`{=html}

This can be useful to insert raw xml into `docx` documents, e.g.
a pagebreak:

    ```{=openxml}
    <w:p>
      <w:r>
        <w:br w:type="page"/>
      </w:r>
    </w:p>
    ```

The format name should match the target format name (see
`-t/--to`, above, for a list, or use `pandoc
--list-output-formats`). Use `openxml` for `docx` output,
`opendocument` for `odt` output, `html5` for `epub3` output,
`html4` for `epub2` output, and `latex`, `beamer`,
`ms`, or `html5` for `pdf` output (depending on what you
use for `--pdf-engine`).

This extension presupposes that the relevant kind of
inline code or fenced code block is enabled.  Thus, for
example, to use a raw attribute with a backtick code block,
`backtick_code_blocks` must be enabled.

The raw attribute cannot be combined with regular attributes.

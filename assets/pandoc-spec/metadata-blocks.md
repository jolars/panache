# Metadata blocks


### Extension: `pandoc_title_block` ###

If the file begins with a title block

    % title
    % author(s) (separated by semicolons)
    % date

it will be parsed as bibliographic information, not regular text.  (It
will be used, for example, in the title of standalone LaTeX or HTML
output.)  The block may contain just a title, a date and an author,
or all three elements. If you want to include an author but no
title, or a title and a date but no author, you need a blank line:

```
%
% Author
```

```
% My title
%
% June 15, 2006
```

The title may occupy multiple lines, but continuation lines must
begin with leading space, thus:

```
% My title
  on multiple lines
```

If a document has multiple authors, the authors may be put on
separate lines with leading space, or separated by semicolons, or
both.  So, all of the following are equivalent:

```
% Author One
  Author Two
```

```
% Author One; Author Two
```

```
% Author One;
  Author Two
```

The date must fit on one line.

All three metadata fields may contain standard inline formatting
(italics, links, footnotes, etc.).

Title blocks will always be parsed, but they will affect the output only
when the `--standalone` (`-s`) option is chosen. In HTML output, titles
will appear twice: once in the document head---this is the title that
will appear at the top of the window in a browser---and once at the
beginning of the document body. The title in the document head can have
an optional prefix attached (`--title-prefix` or `-T` option). The title
in the body appears as an H1 element with class "title", so it can be
suppressed or reformatted with CSS. If a title prefix is specified with
`-T` and no title block appears in the document, the title prefix will
be used by itself as the HTML title.

The man page writer extracts a title, man page section number, and
other header and footer information from the title line. The title
is assumed to be the first word on the title line, which may optionally
end with a (single-digit) section number in parentheses. (There should
be no space between the title and the parentheses.)  Anything after
this is assumed to be additional footer and header text. A single pipe
character (`|`) should be used to separate the footer text from the header
text.  Thus,

    % PANDOC(1)

will yield a man page with the title `PANDOC` and section 1.

    % PANDOC(1) Pandoc User Manuals

will also have "Pandoc User Manuals" in the footer.

    % PANDOC(1) Pandoc User Manuals | Version 4.0

will also have "Version 4.0" in the header.

### Extension: `yaml_metadata_block` ###

A [YAML] metadata block is a valid YAML object, delimited by a line of three
hyphens (`---`) at the top and a line of three hyphens (`---`) or three dots
(`...`) at the bottom.  The initial line `---` must not be followed by a blank
line. A YAML metadata block may occur anywhere in the document, but if
it is not at the beginning, it must be preceded by a blank line.

Note that, because of the way pandoc concatenates input files
when several are provided, you may also keep the metadata in a
separate YAML file and pass it to pandoc as an argument, along
with your Markdown files:

    pandoc chap1.md chap2.md chap3.md metadata.yaml -s -o book.html

Just be sure that the YAML file begins with `---` and ends with `---` or
`...`. Alternatively, you can use the `--metadata-file` option. Using
that approach however, you cannot reference content (like footnotes)
from the main Markdown input document.

Metadata will be taken from the fields of the YAML object and added to any
existing document metadata.  Metadata can contain lists and objects (nested
arbitrarily), but all string scalars will be interpreted as Markdown.  Fields
with names ending in an underscore will be ignored by pandoc.  (They may be
given a role by external processors.)  Field names must not be
interpretable as YAML numbers or boolean values (so, for
example, `yes`, `True`, and `15` cannot be used as field names).

A document may contain multiple metadata blocks.  If two
metadata blocks attempt to set the same field, the value from
the second block will be taken.

Each metadata block is handled internally as an independent YAML document.
This means, for example, that any YAML anchors defined in a block cannot be
referenced in another block.

When pandoc is used with `-t markdown` to create a Markdown document,
a YAML metadata block will be produced only if the `-s/--standalone`
option is used.  All of the metadata will appear in a single block
at the beginning of the document.

Note that [YAML] escaping rules must be followed. Thus, for example,
if a title contains a colon, it must be quoted, and if it contains a
backslash escape, then it must be ensured that it is not treated as a
[YAML escape sequence]. The pipe character (`|`) can be used to begin
an indented block that will be interpreted literally, without need for
escaping. This form is necessary when the field contains blank lines
or block-level formatting:

    ---
    title:  'This is the title: it contains a colon'
    author:
    - Author One
    - Author Two
    keywords: [nothing, nothingness]
    abstract: |
      This is the abstract.

      It consists of two paragraphs.
    ...

The literal block after the `|` must be indented relative to the
line containing the `|`.  If it is not, the YAML will be invalid
and pandoc will not interpret it as metadata.  For an overview
of the complex rules governing YAML, see the [Wikipedia entry on
YAML syntax].

Template variables will be set automatically from the metadata.  Thus, for
example, in writing HTML, the variable `abstract` will be set to the HTML
equivalent of the Markdown in the `abstract` field:

    <p>This is the abstract.</p>
    <p>It consists of two paragraphs.</p>

Variables can contain arbitrary YAML structures, but the template must match
this structure.  The `author` variable in the default templates expects a
simple list or string, but can be changed to support more complicated
structures.  The following combination, for example, would add an affiliation
to the author if one is given:

    ---
    title: The document title
    author:
    - name: Author One
      affiliation: University of Somewhere
    - name: Author Two
      affiliation: University of Nowhere
    ...

To use the structured authors in the example above, you would need a custom
template:

    $for(author)$
    $if(author.name)$
    $author.name$$if(author.affiliation)$ ($author.affiliation$)$endif$
    $else$
    $author$
    $endif$
    $endfor$

Raw content to include in the document's header may be specified
using `header-includes`; however, it is important to mark up
this content as raw code for a particular output format, using
the [`raw_attribute` extension](#extension-raw_attribute), or it
will be interpreted as Markdown. For example:

    header-includes:
    - |
      ```{=latex}
      \let\oldsection\section
      \renewcommand{\section}[1]{\clearpage\oldsection{#1}}
      ```

Note:  the `yaml_metadata_block` extension works with
`commonmark` as well as `markdown` (and it is enabled by default
in `gfm` and `commonmark_x`).  However, in these formats the
following restrictions apply:

- The YAML metadata block must occur at the beginning of the
  document (and there can be only one).  If multiple files are
  given as arguments to pandoc, only the first can be a YAML
  metadata block.

- The leaf nodes of the YAML structure are parsed in isolation from
  each other and from the rest of the document.  So, for
  example,  you can't use a reference link in these contexts
  if the link definition is somewhere else in the document.

[YAML escape sequence]: https://yaml.org/spec/1.2/spec.html#id2776092
[Wikipedia entry on YAML syntax]:  https://en.wikipedia.org/wiki/YAML#Syntax

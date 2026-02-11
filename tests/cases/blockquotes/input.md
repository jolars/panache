Markdown uses email conventions for quoting blocks of text.

>This is a block quote. This
>paragraph has two lines.
>
> 1. This is a list inside a block quote.
> 2. Second item.

A "lazy" form, which requires the `>` character only on the first line of each
block, is also allowed:

> This is a block quote. This
paragraph has two lines.

> 1. This is a list inside a block quote.
2. Second item.

Among the block elements that can be contained in a block quote are other block
quotes. That is, block quotes can be nested:

> This is a block quote.
>
> > A block quote within a block quote.

If the `>` character is followed by an optional space, that space will be
considered part of the block quote marker and not part of the indentation of
the contents.  Thus, to put an indented code block in a block quote, you need
five spaces after the `>`:

>     code

### Extension: `blank_before_blockquote`

Original Markdown syntax does not require a blank line before a block quote.
Pandoc does require this (except, of course, at the beginning of the document).
The reason for the requirement is that it is all too easy for a `>` to end up
at the beginning of a line by accident (perhaps through line wrapping). So,
unless the `markdown_strict` format is used, the following does not produce a
nested block quote in pandoc:

> This is a block quote.
>> Not nested, since `blank_before_blockquote` is enabled by default


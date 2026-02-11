# Inline formatting


### Emphasis ###

To *emphasize* some text, surround it with `*`s or `_`, like this:

    This text is _emphasized with underscores_, and this
    is *emphasized with asterisks*.

Double `*` or `_` produces **strong emphasis**:

    This is **strong emphasis** and __with underscores__.

A `*` or `_` character surrounded by spaces, or backslash-escaped,
will not trigger emphasis:

    This is * not emphasized *, and \*neither is this\*.

### Extension: `intraword_underscores` ###

Because `_` is sometimes used inside words and identifiers,
pandoc does not interpret a `_` surrounded by alphanumeric
characters as an emphasis marker.  If you want to emphasize
just part of a word, use `*`:

    feas*ible*, not feas*able*.

### Strikeout ###

### Extension: `strikeout` ###

To strike out a section of text with a horizontal line, begin and end it
with `~~`. Thus, for example,

    This ~~is deleted text.~~


### Superscripts and subscripts ###

### Extension: `superscript`, `subscript` ###

Superscripts may be written by surrounding the superscripted text by `^`
characters; subscripts may be written by surrounding the subscripted
text by `~` characters.  Thus, for example,

    H~2~O is a liquid.  2^10^ is 1024.

The text between `^...^` or `~...~` may not contain spaces or
newlines.  If the superscripted or subscripted text contains
spaces, these spaces must be escaped with backslashes.  (This is
to prevent accidental superscripting and subscripting through
the ordinary use of `~` and `^`, and also bad interactions with
footnotes.) Thus, if you want the letter P with 'a cat' in
subscripts, use `P~a\ cat~`, not `P~a cat~`.

### Verbatim ###

To make a short span of text verbatim, put it inside backticks:

    What is the difference between `>>=` and `>>`?

If the verbatim text includes a backtick, use double backticks:

    Here is a literal backtick `` ` ``.

(The spaces after the opening backticks and before the closing
backticks will be ignored.)

The general rule is that a verbatim span starts with a string
of consecutive backticks (optionally followed by a space)
and ends with a string of the same number of backticks (optionally
preceded by a space).

Note that backslash-escapes (and other Markdown constructs) do not
work in verbatim contexts:

    This is a backslash followed by an asterisk: `\*`.

### Extension: `inline_code_attributes` ###

Attributes can be attached to verbatim text, just as with
[fenced code blocks]:

    `<$>`{.haskell}

### Underline ###

To underline text, use the `underline` class:

    [Underline]{.underline}

Or, without the `bracketed_spans` extension (but with `native_spans`):

    <span class="underline">Underline</span>

This will work in all output formats that support underline.

### Small caps ###

To write small caps, use the `smallcaps` class:

    [Small caps]{.smallcaps}

Or, without the `bracketed_spans` extension:

    <span class="smallcaps">Small caps</span>

For compatibility with other Markdown flavors, CSS is also supported:

    <span style="font-variant:small-caps;">Small caps</span>

This will work in all output formats that support small caps.

### Highlighting ###

To highlight text, use the `mark` class:

    [Mark]{.mark}

Or, without the `bracketed_spans` extension (but with `native_spans`):

    <span class="mark">Mark</span>

This will work in all output formats that support highlighting.


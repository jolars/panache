# Headings


There are two kinds of headings: Setext and ATX.

### Setext-style headings ###

A setext-style heading is a line of text "underlined" with a row of `=` signs
(for a level-one heading) or `-` signs (for a level-two heading):

    A level-one heading
    ===================

    A level-two heading
    -------------------

The heading text can contain inline formatting, such as emphasis (see
[Inline formatting], below).


### ATX-style headings ###

An ATX-style heading consists of one to six `#` signs and a line of
text, optionally followed by any number of `#` signs.  The number of
`#` signs at the beginning of the line is the heading level:

    ## A level-two heading

    ### A level-three heading ###

As with setext-style headings, the heading text can contain formatting:

    # A level-one heading with a [link](/url) and *emphasis*

### Extension: `blank_before_header` ###

Original Markdown syntax does not require a blank line before a heading.
Pandoc does require this (except, of course, at the beginning of the
document). The reason for the requirement is that it is all too easy for a
`#` to end up at the beginning of a line by accident (perhaps through line
wrapping). Consider, for example:

    I like several of their flavors of ice cream:
    #22, for example, and #5.

### Extension: `space_in_atx_header` ###

Many Markdown implementations do not require a space between the
opening `#`s of an ATX heading and the heading text, so that
`#5 bolt` and `#hashtag` count as headings.  With this extension,
pandoc does require the space.

### Heading identifiers ###

See also the [`auto_identifiers`
extension](#extension-auto_identifiers) above.

### Extension: `header_attributes` ###

Headings can be assigned attributes using this syntax at the end
of the line containing the heading text:

    {#identifier .class .class key=value key=value}

Thus, for example, the following headings will all be assigned the identifier
`foo`:

    # My heading {#foo}

    ## My heading ##    {#foo}

    My other heading   {#foo}
    ---------------

(This syntax is compatible with [PHP Markdown Extra].)

Note that although this syntax allows assignment of classes and key/value
attributes, writers generally don't use all of this information.  Identifiers,
classes, and key/value attributes are used in HTML and HTML-based formats such
as EPUB and slidy.  Identifiers are used for labels and link anchors in the
LaTeX, ConTeXt, Textile, Jira markup, and AsciiDoc writers.

Headings with the class `unnumbered` will not be numbered, even if
`--number-sections` is specified.  A single hyphen (`-`) in an attribute
context is equivalent to `.unnumbered`, and preferable in non-English
documents.  So,

    # My heading {-}

is just the same as

    # My heading {.unnumbered}

If the `unlisted` class is present in addition to `unnumbered`,
the heading will not be included in a table of contents.
(Currently this feature is only implemented for certain
formats: those based on LaTeX and HTML, PowerPoint, and RTF.)

### Extension: `implicit_header_references` ###

Pandoc behaves as if reference links have been defined for each heading.
So, to link to a heading

    # Heading identifiers in HTML

you can simply write

    [Heading identifiers in HTML]

or

    [Heading identifiers in HTML][]

or

    [the section on heading identifiers][heading identifiers in
    HTML]

instead of giving the identifier explicitly:

    [Heading identifiers in HTML](#heading-identifiers-in-html)

If there are multiple headings with identical text, the corresponding
reference will link to the first one only, and you will need to use explicit
links to link to the others, as described above.

Like regular reference links, these references are case-insensitive.

Explicit link reference definitions always take priority over
implicit heading references.  So, in the following example, the
link will point to `bar`, not to `#foo`:

    # Foo

    [foo]: bar

    See [foo]

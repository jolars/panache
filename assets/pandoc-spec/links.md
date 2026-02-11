# Links


Markdown allows links to be specified in several ways.

### Automatic links ###

If you enclose a URL or email address in pointy brackets, it
will become a link:

    <https://google.com>
    <sam@green.eggs.ham>

### Inline links ###

An inline link consists of the link text in square brackets,
followed by the URL in parentheses. (Optionally, the URL can
be followed by a link title, in quotes.)

    This is an [inline link](/url), and here's [one with
    a title](https://fsf.org "click here for a good time!").

There can be no space between the bracketed part and the parenthesized part.
The link text can contain formatting (such as emphasis), but the title cannot.

Email addresses in inline links are not autodetected, so they have to be
prefixed with `mailto`:

    [Write me!](mailto:sam@green.eggs.ham)

### Reference links ###

An *explicit* reference link has two parts, the link itself and the link
definition, which may occur elsewhere in the document (either
before or after the link).

The link consists of link text in square brackets, followed by a label in
square brackets. (There cannot be space between the two unless the
`spaced_reference_links` extension is enabled.) The link definition
consists of the bracketed label, followed by a colon and a space, followed by
the URL, and optionally (after a space) a link title either in quotes or in
parentheses.  The label must not be parseable as a citation (assuming
the `citations` extension is enabled):  citations take precedence over
link labels.

Here are some examples:

    [my label 1]: /foo/bar.html  "My title, optional"
    [my label 2]: /foo
    [my label 3]: https://fsf.org (The Free Software Foundation)
    [my label 4]: /bar#special  'A title in single quotes'

The URL may optionally be surrounded by angle brackets:

    [my label 5]: <http://foo.bar.baz>

The title may go on the next line:

    [my label 3]: https://fsf.org
      "The Free Software Foundation"

Note that link labels are not case sensitive.  So, this will work:

    Here is [my link][FOO]

    [Foo]: /bar/baz

In an *implicit* reference link, the second pair of brackets is
empty:

    See [my website][].

    [my website]: http://foo.bar.baz

Note:  In `Markdown.pl` and most other Markdown implementations,
reference link definitions cannot occur in nested constructions
such as list items or block quotes.  Pandoc lifts this
arbitrary-seeming restriction.  So the following is fine in pandoc,
though not in most other implementations:

    > My block [quote].
    >
    > [quote]: /foo

### Extension: `shortcut_reference_links` ###

In a *shortcut* reference link, the second pair of brackets may
be omitted entirely:

    See [my website].

    [my website]: http://foo.bar.baz

### Internal links ###

To link to another section of the same document, use the automatically
generated identifier (see [Heading identifiers]). For example:

    See the [Introduction](#introduction).

or

    See the [Introduction].

    [Introduction]: #introduction

Internal links are currently supported for HTML formats (including
HTML slide shows and EPUB), LaTeX, and ConTeXt.

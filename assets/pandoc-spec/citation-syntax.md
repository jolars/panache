# Citation syntax


### Extension: `citations` ###

To cite a bibliographic item with an identifier foo, use the
syntax `@foo`.  Normal citations should be included in square
brackets, with semicolons separating distinct items:

    Blah blah [@doe99; @smith2000; @smith2004].

How this is rendered depends on the citation style.  In an
author-date style, it might render as

    Blah blah (Doe 1999, Smith 2000, 2004).

In a footnote style, it might render as

    Blah blah.[^1]

    [^1]:  John Doe, "Frogs," *Journal of Amphibians* 44 (1999);
    Susan Smith, "Flies," *Journal of Insects* (2000);
    Susan Smith, "Bees," *Journal of Insects* (2004).

See the [CSL user documentation] for more information about CSL
styles and how they affect rendering.

Unless a citation key starts with a letter, digit, or `_`,
and contains only alphanumerics and single internal punctuation
characters (`:.#$%&-+?<>~/`), it must be surrounded
by curly braces, which are not considered part of the key.
In `@Foo_bar.baz.`, the key is `Foo_bar.baz` because the final
period is not *internal* punctuation, so it is not included in
the key.  In `@{Foo_bar.baz.}`, the key is `Foo_bar.baz.`, including
the final period.
In `@Foo_bar--baz`, the key is `Foo_bar` because the repeated internal
punctuation characters terminate the key.
The curly braces are recommended if you use URLs as
keys: `[@{https://example.com/bib?name=foobar&date=2000}, p.  33]`.

Citation items may optionally include a prefix, a locator, and
a suffix.  In

    Blah blah [see @doe99, pp. 33-35 and *passim*; @smith04, chap. 1].

the first item (`doe99`) has prefix `see `, locator `pp.  33-35`,
and suffix `and *passim*`.  The second item (`smith04`) has
locator `chap. 1` and no prefix or suffix.

Pandoc uses some heuristics to separate the locator from the
rest of the subject.  It is sensitive to the locator terms
defined in the [CSL locale files].  Either abbreviated or
unabbreviated forms are accepted. In the `en-US` locale, locator
terms can be written in either singular or plural forms, as
`book`, `bk.`/`bks.`; `chapter`, `chap.`/`chaps.`; `column`,
`col.`/`cols.`; `figure`, `fig.`/`figs.`; `folio`,
`fol.`/`fols.`; `number`, `no.`/`nos.`; `line`, `l.`/`ll.`;
`note`, `n.`/`nn.`; `opus`, `op.`/`opp.`; `page`, `p.`/`pp.`;
`paragraph`, `para.`/`paras.`; `part`, `pt.`/`pts.`; `section`,
`sec.`/`secs.`; `sub verbo`, `s.v.`/`s.vv.`; `verse`,
`v.`/`vv.`; `volume`, `vol.`/`vols.`; `¶`/`¶¶`; `§`/`§§`. If no
locator term is used, "page" is assumed.

In complex cases, you can force something to be treated as
a locator by enclosing it in curly braces or prevent parsing
the suffix as locator by prepending curly braces:

    [@smith{ii, A, D-Z}, with a suffix]
    [@smith, {pp. iv, vi-xi, (xv)-(xvii)} with suffix here]
    [@smith{}, 99 years later]

A minus sign (`-`) before the `@` will suppress mention of
the author in the citation.  This can be useful when the
author is already mentioned in the text:

    Smith says blah [-@smith04].

You can also write an author-in-text citation, by omitting the
square brackets:

    @smith04 says blah.

    @smith04 [p. 33] says blah.

This will cause the author's name to be rendered, followed by
the bibliographical details.  Use this form when you want to
make the citation the subject of a sentence.

When you are using a note style, it is usually better to let
citeproc create the footnotes from citations rather than writing
an explicit note.  If you do write an explicit note that
contains a citation, note that normal citations will be put in
parentheses, while author-in-text citations will not.  For
this reason, it is sometimes preferable to use the
author-in-text style inside notes when using a note style.

[CSL user documentation]: https://citationstyles.org/authors/
[CSL]: https://docs.citationstyles.org/en/stable/specification.html
[CSL markup specs]: https://citeproc-js.readthedocs.io/en/latest/csl-json/markup.html#html-like-formatting-tags
[Chicago Manual of Style]: https://chicagomanualofstyle.org
[Citation Style Language]: https://citationstyles.org
[Zotero Style Repository]: https://www.zotero.org/styles
[finding and editing styles]: https://citationstyles.org/authors/
[CSL locale files]: https://github.com/citation-style-language/locales

# Citations

Introductory text to ensure wrapping and
keep these sentences long enough to
reflow around citations.

Blah blah
[@doe99; @smith2000; @smith2004] with
extra words to exercise wrapping
behavior.

Blah blah
[see @doe99, pp. 33-35 and *passim*; @smith04, chap. 1]
in a longer sentence for reflow.

Smith says blah [-@smith04] when the
author appears in the prose already.

@smith04 says blah in a sentence that
should reflow nicely with the
author-in-text citation.

@smith04 [p. 33] says blah with a
locator in brackets to match pandoc
behavior.

Citation with suffix and locator
[@item1 pp. 33, 35-37, and nowhere else]
in a longer line.

Citation with suffix only
[@item1 and nowhere else] followed by
more explanatory words.

With some markup
[*see* @item1 p. **32**] in a sentence
that should still reflow.

Citation group with unicode key
[see @item1 chap. 3; also @пункт3 p. 34-35]
for UTF-8 handling.

Braced keys and punctuation
[@{Foo_bar.baz.}; @{https://example.com/bib?name=foobar&date=2000}]
in a long sentence.

Repeated punctuation terminates key
[@Foo_bar--baz] with extra text to
ensure wrapping.

Complex locator braces
[@smith{ii, A, D-Z}, with a suffix] and
additional text to reflow.

Locator braces
[@smith, {pp. iv, vi-xi, (xv)-(xvii)} with suffix here]
and more text for wrapping.

Empty locator [@smith{}, 99 years later]
with additional words to wrap.

Reference link followed by citation:
MapReduce is a paradigm popularized by
[Google] [@mapreduce] as its most vocal
proponent in this longer sentence.

[Google]: http://google.com

Footnote with citations.[^1]

[^1]: @пункт3 [p. 12] and a citation
    without locators [@пункт3] in a
    longer note sentence.

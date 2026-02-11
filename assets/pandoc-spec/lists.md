# Lists


### Bullet lists ###

A bullet list is a list of bulleted list items.  A bulleted list
item begins with a bullet (`*`, `+`, or `-`).  Here is a simple
example:

    * one
    * two
    * three

This will produce a "compact" list. If you want a "loose" list, in which
each item is formatted as a paragraph, put spaces between the items:

    * one

    * two

    * three

The bullets need not be flush with the left margin; they may be
indented one, two, or three spaces. The bullet must be followed
by whitespace.

List items look best if subsequent lines are flush with the first
line (after the bullet):

    * here is my first
      list item.
    * and my second.

But Markdown also allows a "lazy" format:

    * here is my first
    list item.
    * and my second.

### Block content in list items ###

A list item may contain multiple paragraphs and other block-level
content. However, subsequent paragraphs must be preceded by a blank line
and indented to line up with the first non-space content after
the list marker.

      * First paragraph.

        Continued.

      * Second paragraph. With a code block, which must be indented
        eight spaces:

            { code }

Exception: if the list marker is followed by an indented code
block, which must begin 5 spaces after the list marker, then
subsequent paragraphs must begin two columns after the last
character of the list marker:

    *     code

      continuation paragraph

List items may include other lists.  In this case the preceding blank
line is optional.  The nested list must be indented to line up with
the first non-space character after the list marker of the
containing list item.

    * fruits
      + apples
        - macintosh
        - red delicious
      + pears
      + peaches
    * vegetables
      + broccoli
      + chard

As noted above, Markdown allows you to write list items "lazily," instead of
indenting continuation lines. However, if there are multiple paragraphs or
other blocks in a list item, the first line of each must be indented.

    + A lazy, lazy, list
    item.

    + Another one; this looks
    bad but is legal.

        Second paragraph of second
    list item.

### Ordered lists ###

Ordered lists work just like bulleted lists, except that the items
begin with enumerators rather than bullets.

In original Markdown, enumerators are decimal numbers followed
by a period and a space.  The numbers themselves are ignored, so
there is no difference between this list:

    1.  one
    2.  two
    3.  three

and this one:

    5.  one
    7.  two
    1.  three

### Extension: `fancy_lists` ###

Unlike original Markdown, pandoc allows ordered list items to be marked
with uppercase and lowercase letters and roman numerals, in addition to
Arabic numerals. List markers may be enclosed in parentheses or followed by a
single right-parenthesis or period. They must be separated from the
text that follows by at least one space, and, if the list marker is a
capital letter with a period, by at least two spaces.[^2]

[^2]:  The point of this rule is to ensure that normal paragraphs
    starting with people's initials, like

        B. Russell won a Nobel Prize (but not for "On Denoting").

    do not get treated as list items.

    This rule will not prevent

        (C) 2007 Joe Smith

    from being interpreted as a list item.  In this case, a backslash
    escape can be used:

        (C\) 2007 Joe Smith

The `fancy_lists` extension also allows '`#`' to be used as an
ordered list marker in place of a numeral:

    #. one
    #. two

Note:  the '`#`' ordered list marker doesn't work with `commonmark`.

### Extension: `startnum` ###

Pandoc also pays attention to the type of list marker used, and to the
starting number, and both of these are preserved where possible in the
output format. Thus, the following yields a list with numbers followed
by a single parenthesis, starting with 9, and a sublist with lowercase
roman numerals:

     9)  Ninth
    10)  Tenth
    11)  Eleventh
           i. subone
          ii. subtwo
         iii. subthree

Pandoc will start a new list each time a different type of list
marker is used.  So, the following will create three lists:

    (2) Two
    (5) Three
    1.  Four
    *   Five

If default list markers are desired, use `#.`:

    #.  one
    #.  two
    #.  three

### Extension: `task_lists` ###

Pandoc supports task lists, using the syntax of GitHub-Flavored Markdown.

    - [ ] an unchecked task list item
    - [x] checked item

### Definition lists ###

### Extension: `definition_lists` ###

Pandoc supports definition lists, using the syntax of
[PHP Markdown Extra] with some extensions.[^3]

    Term 1

    :   Definition 1

    Term 2 with *inline markup*

    :   Definition 2

            { some code, part of Definition 2 }

        Third paragraph of definition 2.

Each term must fit on one line, which may optionally be followed by
a blank line, and must be followed by one or more definitions.
A definition begins with a colon or tilde, which may be indented one
or two spaces.

A term may have multiple definitions, and each definition may
consist of one or more block elements (paragraph, code block,
list, etc.), each indented four spaces or one tab stop.  The
body of the definition (not including the first line)
should be indented four spaces. However, as with
other Markdown lists, you can "lazily" omit indentation except
at the beginning of a paragraph or other block element:

    Term 1

    :   Definition
    with lazy continuation.

        Second paragraph of the definition.

If you leave space before the definition (as in the example above),
the text of the definition will be treated as a paragraph.  In some
output formats, this will mean greater spacing between term/definition
pairs. For a more compact definition list, omit the space before the
definition:

    Term 1
      ~ Definition 1

    Term 2
      ~ Definition 2a
      ~ Definition 2b

Note that space between items in a definition list is required.

[^3]:  I have been influenced by the suggestions of [David
  Wheeler](https://justatheory.com/2009/02/modest-markdown-proposal/).

### Numbered example lists ###

### Extension: `example_lists` ###

The special list marker `@` can be used for sequentially numbered
examples. The first list item with a `@` marker will be numbered '1',
the next '2', and so on, throughout the document. The numbered examples
need not occur in a single list; each new list using `@` will take up
where the last stopped. So, for example:

    (@)  My first example will be numbered (1).
    (@)  My second example will be numbered (2).

    Explanation of examples.

    (@)  My third example will be numbered (3).

Numbered examples can be labeled and referred to elsewhere in the
document:

    (@good)  This is a good example.

    As (@good) illustrates, ...

The label can be any string of alphanumeric characters, underscores,
or hyphens.

Continuation paragraphs in example lists must always
be indented four spaces, regardless of the length of the
list marker.  That is, example lists always behave as if the
`four_space_rule` extension is set.  This is because example
labels tend to be long, and indenting content to the
first non-space character after the label would be awkward.

You can repeat an earlier numbered example by re-using its label:

    (@foo) Sample sentence.

    Intervening text...

    This theory can explain the case we saw earlier (repeated):

    (@foo) Sample sentence.

This only works reliably, though, if the repeated item is in a list
by itself, because each numbered example list will be numbered
continuously from its starting number.

### Ending a list ###

What if you want to put an indented code block after a list?

    -   item one
    -   item two

        { my code block }

Trouble! Here pandoc (like other Markdown implementations) will treat
`{ my code block }` as the second paragraph of item two, and not as
a code block.

To "cut off" the list after item two, you can insert some non-indented
content, like an HTML comment, which won't produce visible output in
any format:

    -   item one
    -   item two

    <!-- end of list -->

        { my code block }

You can use the same trick if you want two consecutive lists instead
of one big list:

    1.  one
    2.  two
    3.  three

    <!-- -->

    1.  uno
    2.  dos
    3.  tres

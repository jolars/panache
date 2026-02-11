# Footnotes


### Extension: `footnotes` ###

Pandoc's Markdown allows footnotes, using the following syntax:

    Here is a footnote reference,[^1] and another.[^longnote]

    [^1]: Here is the footnote.

    [^longnote]: Here's one with multiple blocks.

        Subsequent paragraphs are indented to show that they
    belong to the previous footnote.

            { some.code }

        The whole paragraph can be indented, or just the first
        line.  In this way, multi-paragraph footnotes work like
        multi-paragraph list items.

    This paragraph won't be part of the note, because it
    isn't indented.

The identifiers in footnote references may not contain spaces, tabs,
newlines, or the characters `^`, `[`, or `]`. These identifiers
are used only to correlate the footnote reference with the note
itself; in the output, footnotes will be numbered sequentially.

The footnotes themselves need not be placed at the end of the
document.  They may appear anywhere except inside other block elements
(lists, block quotes, tables, etc.).  Each footnote should be
separated from surrounding content (including other footnotes)
by blank lines.

### Extension: `inline_notes` ###

Inline footnotes are also allowed (though, unlike regular notes,
they cannot contain multiple paragraphs).  The syntax is as follows:

    Here is an inline note.^[Inline notes are easier to write, since
    you don't have to pick an identifier and move down to type the
    note.]

Inline and regular footnotes may be mixed freely.

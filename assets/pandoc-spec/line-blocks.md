# Line blocks


### Extension: `line_blocks` ###

A line block is a sequence of lines beginning with a vertical bar (`|`)
followed by a space.  The division into lines will be preserved in
the output, as will any leading spaces; otherwise, the lines will
be formatted as Markdown.  This is useful for verse and addresses:

    | The limerick packs laughs anatomical
    | In space that is quite economical.
    |    But the good ones I've seen
    |    So seldom are clean
    | And the clean ones so seldom are comical

    | 200 Main St.
    | Berkeley, CA 94718

The lines can be hard-wrapped if needed, but the continuation
line must begin with a space.

    | The Right Honorable Most Venerable and Righteous Samuel L.
      Constable, Jr.
    | 200 Main St.
    | Berkeley, CA 94718

Inline formatting (such as emphasis) is allowed in the content
(though it can't cross line boundaries). Block-level formatting
(such as block quotes or lists) is not recognized.

This syntax is borrowed from [reStructuredText].

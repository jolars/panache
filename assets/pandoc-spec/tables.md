# Tables


Four kinds of tables may be used. The first three kinds presuppose the use of
a fixed-width font, such as Courier. The fourth kind can be used with
proportionally spaced fonts, as it does not require lining up columns.

### Extension: `table_captions` ###

A caption may optionally be provided with all 4 kinds of tables (as
illustrated in the examples below). A caption is a paragraph beginning
with the string `Table:` (or `table:` or just `:`), which will be stripped
off. It may appear either before or after the table.

### Extension: `simple_tables` ###

Simple tables look like this:

      Right     Left     Center     Default
    -------     ------ ----------   -------
         12     12        12            12
        123     123       123          123
          1     1          1             1

    Table:  Demonstration of simple table syntax.

The header and table rows must each fit on one line.  Column
alignments are determined by the position of the header text relative
to the dashed line below it:[^4]

  - If the dashed line is flush with the header text on the right side
    but extends beyond it on the left, the column is right-aligned.
  - If the dashed line is flush with the header text on the left side
    but extends beyond it on the right, the column is left-aligned.
  - If the dashed line extends beyond the header text on both sides,
    the column is centered.
  - If the dashed line is flush with the header text on both sides,
    the default alignment is used (in most cases, this will be left).

[^4]:  This scheme is due to Michel Fortin, who proposed it on the
       [Markdown discussion list](http://six.pairlist.net/pipermail/markdown-discuss/2005-March/001097.html).

The table must end with a blank line, or a line of dashes followed by
a blank line.

The column header row may be omitted, provided a dashed line is used
to end the table. For example:

    -------     ------ ----------   -------
         12     12        12             12
        123     123       123           123
          1     1          1              1
    -------     ------ ----------   -------

When the header row is omitted, column alignments are determined on the basis
of the first line of the table body. So, in the tables above, the columns
would be right, left, center, and right aligned, respectively.

### Extension: `multiline_tables` ###

Multiline tables allow header and table rows to span multiple lines
of text (but cells that span multiple columns or rows of the table are
not supported).  Here is an example:

    -------------------------------------------------------------
     Centered   Default           Right Left
      Header    Aligned         Aligned Aligned
    ----------- ------- --------------- -------------------------
       First    row                12.0 Example of a row that
                                        spans multiple lines.

      Second    row                 5.0 Here's another one. Note
                                        the blank line between
                                        rows.
    -------------------------------------------------------------

    Table: Here's the caption. It, too, may span
    multiple lines.

These work like simple tables, but with the following differences:

  - They must begin with a row of dashes, before the header text
    (unless the header row is omitted).
  - They must end with a row of dashes, then a blank line.
  - The rows must be separated by blank lines.

In multiline tables, the table parser pays attention to the widths of
the columns, and the writers try to reproduce these relative widths in
the output. So, if you find that one of the columns is too narrow in the
output, try widening it in the Markdown source.

The header may be omitted in multiline tables as well as simple tables:

    ----------- ------- --------------- -------------------------
       First    row                12.0 Example of a row that
                                        spans multiple lines.

      Second    row                 5.0 Here's another one. Note
                                        the blank line between
                                        rows.
    ----------- ------- --------------- -------------------------

    : Here's a multiline table without a header.

It is possible for a multiline table to have just one row, but the row
should be followed by a blank line (and then the row of dashes that ends
the table), or the table may be interpreted as a simple table.

### Extension: `grid_tables` ###

Grid tables look like this:

    : Sample grid table.

    +---------------+---------------+--------------------+
    | Fruit         | Price         | Advantages         |
    +===============+===============+====================+
    | Bananas       | $1.34         | - built-in wrapper |
    |               |               | - bright color     |
    +---------------+---------------+--------------------+
    | Oranges       | $2.10         | - cures scurvy     |
    |               |               | - tasty            |
    +---------------+---------------+--------------------+

The row of `=`s separates the header from the table body,
and can be omitted for a headerless table. The cells of grid
tables may contain arbitrary block elements (multiple
paragraphs, code blocks, lists, etc.).

Cells can span multiple columns or rows:

    +---------------------+----------+
    | Property            | Earth    |
    +=============+=======+==========+
    |             | min   | -89.2 °C |
    | Temperature +-------+----------+
    | 1961-1990   | mean  | 14 °C    |
    |             +-------+----------+
    |             | max   | 56.7 °C  |
    +-------------+-------+----------+

A table header may contain more than one row:

    +---------------------+-----------------------+
    | Location            | Temperature 1961-1990 |
    |                     | in degree Celsius     |
    |                     +-------+-------+-------+
    |                     | min   | mean  | max   |
    +=====================+=======+=======+=======+
    | Antarctica          | -89.2 | N/A   | 19.8  |
    +---------------------+-------+-------+-------+
    | Earth               | -89.2 | 14    | 56.7  |
    +---------------------+-------+-------+-------+

Alignments can be specified as with pipe tables, by putting
colons at the boundaries of the separator line after the
header:

    +---------------+---------------+--------------------+
    | Right         | Left          | Centered           |
    +==============:+:==============+:==================:+
    | Bananas       | $1.34         | built-in wrapper   |
    +---------------+---------------+--------------------+

For headerless tables, the colons go on the top line instead:

    +--------------:+:--------------+:------------------:+
    | Right         | Left          | Centered           |
    +---------------+---------------+--------------------+

A table foot can be defined by enclosing it with separator lines
that use `=` instead of `-`:

     +---------------+---------------+
     | Fruit         | Price         |
     +===============+===============+
     | Bananas       | $1.34         |
     +---------------+---------------+
     | Oranges       | $2.10         |
     +===============+===============+
     | Sum           | $3.44         |
     +===============+===============+

The foot must always be placed at the very bottom of the table.

Grid tables can be created easily using Emacs' table-mode
(`M-x table-insert`).

### Extension: `pipe_tables` ###

Pipe tables look like this:

    | Right | Left | Default | Center |
    |------:|:-----|---------|:------:|
    |   12  |  12  |    12   |    12  |
    |  123  |  123 |   123   |   123  |
    |    1  |    1 |     1   |     1  |

      : Demonstration of pipe table syntax.

The syntax is identical to [PHP Markdown Extra tables].  The beginning and
ending pipe characters are optional, but pipes are required between all
columns.  The colons indicate column alignment as shown.  The header
cannot be omitted.  To simulate a headerless table, include a header
with blank cells.

Since the pipes indicate column boundaries, columns need not be vertically
aligned, as they are in the above example.  So, this is a perfectly
legal (though ugly) pipe table:

    fruit| price
    -----|-----:
    apple|2.05
    pear|1.37
    orange|3.09

The cells of pipe tables cannot contain block elements like paragraphs
and lists, and cannot span multiple lines.  If any line of the
Markdown source is longer than the column width (see `--columns`),
then the table will take up the full text width and the cell
contents will wrap, with the relative cell widths determined by
the number of dashes in the line separating the table header
from the table body. (For example `---|-` would make the first column 3/4
and the second column 1/4 of the full text width.)
On the other hand, if no lines are wider than column width, then
cell contents will not be wrapped, and the cells will be sized
to their contents.

Note:  pandoc also recognizes pipe tables of the following
form, as can be produced by Emacs' orgtbl-mode:

    | One | Two   |
    |-----+-------|
    | my  | table |
    | is  | nice  |

The difference is that `+` is used instead of `|`. Other orgtbl features
are not supported. In particular, to get non-default column alignment,
you'll need to add colons as above.

[PHP Markdown Extra tables]: https://michelf.ca/projects/php-markdown/extra/#table

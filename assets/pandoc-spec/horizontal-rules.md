# Horizontal rules


A line containing a row of three or more `*`, `-`, or `_` characters
(optionally separated by spaces) produces a horizontal rule:

    *  *  *  *

    ---------------

We strongly recommend that horizontal rules be separated from
surrounding text by blank lines.  If a horizontal rule is not
followed by a blank line, pandoc may try to interpret the
lines that follow as a YAML metadata block or a table.

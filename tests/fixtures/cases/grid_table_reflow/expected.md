A single-column cell with a trailing blank line reflows and drops the blank:

+--------------------+
| Lorem ipsum dolor  |
| sit                |
+--------------------+

A body cell wraps to its column width; padding blanks are dropped:

+------------+--------------------+
| Key        | Description        |
+============+====================+
| alpha      | A fairly long      |
|            | description that   |
|            | wraps over lines   |
+------------+--------------------+

Cells with block content are preserved verbatim, not reflowed:

+--------------------+
| - item one         |
| - item two         |
+--------------------+

A hard line break (trailing backslash) is preserved, not joined:

+--------------------+
| First line\        |
| second line        |
+--------------------+

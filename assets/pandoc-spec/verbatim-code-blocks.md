# Verbatim (code) blocks


### Indented code blocks ###

A block of text indented four spaces (or one tab) is treated as verbatim
text: that is, special characters do not trigger special formatting,
and all spaces and line breaks are preserved.  For example,

        if (a > 3) {
          moveShip(5 * gravity, DOWN);
        }

The initial (four space or one tab) indentation is not considered part
of the verbatim text, and is removed in the output.

Note: blank lines in the verbatim text need not begin with four spaces.


### Fenced code blocks ###

### Extension: `fenced_code_blocks` ###

In addition to standard indented code blocks, pandoc supports
*fenced* code blocks.  These begin with a row of three or more
tildes (`~`) and end with a row of tildes that must be at least as long as
the starting row. Everything between these lines is treated as code. No
indentation is necessary:

    ~~~~~~~
    if (a > 3) {
      moveShip(5 * gravity, DOWN);
    }
    ~~~~~~~

Like regular code blocks, fenced code blocks must be separated
from surrounding text by blank lines.

If the code itself contains a row of tildes or backticks, just use a longer
row of tildes or backticks at the start and end:

    ~~~~~~~~~~~~~~~~
    ~~~~~~~~~~
    code including tildes
    ~~~~~~~~~~
    ~~~~~~~~~~~~~~~~

### Extension: `backtick_code_blocks` ###

Same as `fenced_code_blocks`, but uses backticks (`` ` ``) instead of tildes
(`~`).

### Extension: `fenced_code_attributes` ###

Optionally, you may attach attributes to fenced or backtick code block using
this syntax:

    ~~~~ {#mycode .haskell .numberLines startFrom="100"}
    qsort []     = []
    qsort (x:xs) = qsort (filter (< x) xs) ++ [x] ++
                   qsort (filter (>= x) xs)
    ~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

Here `mycode` is an identifier, `haskell` and `numberLines` are
classes, and `startFrom` is an attribute with value `100`. Some
output formats can use this information to do syntax
highlighting. Currently, the only output formats that use this
information are HTML, LaTeX, Docx, Ms, and PowerPoint. If
highlighting is supported for your output format and language,
then the code block above will appear highlighted, with numbered
lines. (To see which languages are supported, type `pandoc
--list-highlight-languages`.) Otherwise, the code block above
will appear as follows:

    <pre id="mycode" class="haskell numberLines" startFrom="100">
      <code>
      ...
      </code>
    </pre>

The `numberLines` (or `number-lines`) class will cause the lines
of the code block to be numbered, starting with `1` or the value
of the `startFrom` attribute.  The `lineAnchors` (or
`line-anchors`) class will cause the lines to be clickable
anchors in HTML output.

A shortcut form can also be used for specifying the language of
the code block:

    ```haskell
    qsort [] = []
    ```

This is equivalent to:

    ``` {.haskell}
    qsort [] = []
    ```

This shortcut form may be combined with attributes:

    ```haskell {.numberLines}
    qsort [] = []
    ```

Which is equivalent to:

    ``` {.haskell .numberLines}
    qsort [] = []
    ```

If the `fenced_code_attributes` extension is disabled, but
input contains class attribute(s) for the code block, the first
class attribute will be printed after the opening fence as a bare
word.

To prevent all highlighting, use the `--syntax-highlighting=none`
option. To set the highlighting style or method, use
`--syntax-highlighting`. For more information on highlighting, see
[Syntax highlighting], below.

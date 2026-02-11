# Backslash escapes


### Extension: `all_symbols_escapable` ###

Except inside a code block or inline code, any punctuation or space
character preceded by a backslash will be treated literally, even if it
would normally indicate formatting.  Thus, for example, if one writes

    *\*hello\**

one will get

    <em>*hello*</em>

instead of

    <strong>hello</strong>

This rule is easier to remember than original Markdown's rule,
which allows only the following characters to be backslash-escaped:

    \`*_{}[]()>#+-.!

(However, if the `markdown_strict` format is used, the original Markdown rule
will be used.)

A backslash-escaped space is parsed as a nonbreaking space. In TeX output,
it will appear as `~`. In HTML and XML output, it will appear as a
literal unicode nonbreaking space character (note that it will thus
actually look "invisible" in the generated HTML source; you can still
use the `--ascii` command-line option to make it appear as an explicit
entity).

A backslash-escaped newline (i.e. a backslash occurring at the end of
a line) is parsed as a hard line break.  It will appear in TeX output as
`\\` and in HTML as `<br />`.  This is a nice alternative to
Markdown's "invisible" way of indicating hard line breaks using
two trailing spaces on a line.

Backslash escapes do not work in verbatim contexts.

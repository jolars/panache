The largest change in this release is likely a new wrap mode, `semantic`, which
is a hybrid between `sentence` and `preserve` modes based on [Semantic Line
Breaks](https://sembr.org/). You configure it by setting
`[format]\nwrap = semantic` in the config. It will break lines at sentence
boundaries, like the `sentence` mode, but also preserve existing break points.
In the future, I expect to tailor some lint rules to the mode according to the
sembr spec, but for now it is just a new wrap mode. Thanks to @BontolBailey for
the suggestion ([#313](https://github.com/jolars/panache/issues/313).

This release also comes with support for a new extension, `four-space-rule`,
which is a standard Pandoc extension (off by default) that enforces a four-space
indent for continuation lines. This helps Panache play nicely with systems based
on [Python-Markdown](https://python-markdown.github.io/), where this rule is
enforced. Thanks to @DamonBayer for the suggestion in
[#308](https://github.com/jolars/panache/issues/308).

Finally, there are a number of smaller bug fixes and improvements to the parser
and formatter, as well as new presets for external formatters (`ormulu`,
`biome`, `csharpier`, `mix`, `rustfmt`, `isort`, `runic`, and `stylua`).

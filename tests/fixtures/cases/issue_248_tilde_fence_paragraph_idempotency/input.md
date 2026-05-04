Some intro paragraph that has multiple lines so that the wrapping behavior can
be observed when content gets long enough to wrap.

{line-numbers=off}
~~~~~~~~
> logs <- read_csv("data/2016-07-19.csv.bz2", n_max = 10)
Rows: 10 Columns: 10

ℹ Use `spec()` to retrieve the full column specification for this data.
ℹ Specify the column types or set `show_col_types = FALSE` to quiet this message.
~~~~~~~~
Note that the warnings indicate that `read_csv` may have had some difficulty identifying the type of each column. This can be solved by using the `col_types` argument.

A second leanpub-style block follows, where the closing fence ends up on a
wrapped line of its own when the formatter wraps the surrounding paragraph.

{line-numbers=off}
~~~~~~~~
> x <- "foo"
> y <- data.frame(a = 1L, b = "a")
~~~~~~~~

After.

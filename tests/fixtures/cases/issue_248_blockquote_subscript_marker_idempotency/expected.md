The key functions for converting R objects into a binary format are `save()`,
`save.image()`, and `serialize()`. Individual R objects can be saved to a file
using the `save()` function.

{line-numbers=off} ~~~~~~~~ > a <- data.frame(x = rnorm(100), y = runif(100)) >
b <- c(3, 4.4, 1 / 3) > > ## Save 'a' and 'b' to a file > save(a, b, file =
"mydata.rda") > > ## Load 'a' and 'b' into your workspace > load("mydata.rda")\
~~~~~~~~

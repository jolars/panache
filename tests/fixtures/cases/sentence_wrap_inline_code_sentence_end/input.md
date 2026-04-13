These examples use [inline R code](reuse.html#inline-code) (`` `r ` ``), which generates the list at documentation time (i.e. when you run `devtools::document()`). This only requires including doclisting in `Suggests`.

Document **S4 classes** by adding a roxygen block before `setClass()`. `@export` a class if you want users to create instances or other developers to extend it (e.g. by creating subclasses).

S7 methods are registered with `method(generic, class) <- fn`. Generally, it's not necessary to document straightforward methods.

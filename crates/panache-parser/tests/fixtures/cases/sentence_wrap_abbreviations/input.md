In most cases, you can just use the `@export` tag, and roxygen2 will automatically figure out which `NAMESPACE` directive (i.e. `export()`, `S3method()`, `exportClasses()`, or `exportMethods()`) you need.

If this happens to you, disambiguate with (e.g.) `@method all.equal data.frame`.

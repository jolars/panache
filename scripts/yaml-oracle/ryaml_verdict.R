#!/usr/bin/env Rscript
# Reads a YAML document on stdin and reports whether R's `yaml` package accepts
# it. Prints "ok" on success or "err:<message>" on a parse error.
#
# R's `yaml` package (libyaml under the hood) is the parser the RMarkdown
# toolchain uses: `rmarkdown::yaml_front_matter` for frontmatter and knitr's
# `partition_chunk` / cell-option handling for `#|` chunk options. It differs
# from pandoc's libyaml in one way that matters here: it REJECTS duplicate
# mapping keys ("Duplicate map key") rather than taking the last value.

suppressWarnings(suppressMessages(library(yaml)))
input <- paste(readLines(file("stdin"), warn = FALSE), collapse = "\n")
res <- tryCatch(
  {
    yaml.load(input)
    "ok"
  },
  error = function(e) paste0("err:", gsub("\\s+", " ", conditionMessage(e)))
)
cat(res)

```bash
cargo install tomat
```

```
if (a > 3) {
  moveShip(5 * gravity, DOWN);
}
```

```haskell {.numberLines #mycode startFrom="100"}
qsort []     = []
qsort (x:xs) = qsort (filter (< x) xs) ++ [x] ++
                qsort (filter (>= x) xs)
```

```haskell
qsort [] = []
```

```haskell
qsort [] = []
```

```haskell {.numberLines}
qsort [] = []
```

```haskell {.numberLines}
qsort [] = []
```

A quarto fenced code block, with blankline:

`{r} a <- 1`

A quarto fenced code block, without blankline: `{r} a <- 1`

A fenced code block should be separated with a blankline after we have formatted
it:

```r
a <- 1
b <- 2
```

A code block in a definition list:

Input

:   ```markdown
    # Heading 1

    # Heading 2
    ```

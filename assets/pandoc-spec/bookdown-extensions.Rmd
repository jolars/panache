## Markdown extensions by bookdown

Although Pandoc's Markdown is much richer than the original Markdown syntax, it still lacks a number of things that we may need for academic writing. For example, it supports math equations, but you cannot number and reference equations in multi-page HTML or EPUB output. We have provided a few Markdown extensions in **bookdown** to fill the gaps.

### Number and reference equations {#equations}

To number and refer to equations\index{equation}\index{cross-reference}, put them in the equation environments and assign labels to them using the syntax `(\#eq:label)`, e.g.,

```latex
\begin{equation} 
  f\left(k\right) = \binom{n}{k} p^k\left(1-p\right)^{n-k}
  (\#eq:binom)
\end{equation} 
```

It renders the equation below:

\begin{equation}
f\left(k\right)=\binom{n}{k}p^k\left(1-p\right)^{n-k} (\#eq:binom)
\end{equation}

You may refer to it using `\@ref(eq:binom)`, e.g., see Equation \@ref(eq:binom).

```{block2, type='rmdcaution'}
Equation labels must start with the prefix `eq:` in **bookdown**. All labels in **bookdown** must only contain alphanumeric characters, `:`, `-`, and/or `/`. Equation references work best for LaTeX/PDF output, and they are not well supported in Word output or e-books. For HTML output, **bookdown** can only number the equations with labels. Please make sure equations without labels are not numbered by either using the `equation*` environment or adding `\nonumber` or `\notag` to your equations. The same rules apply to other math environments, such as `eqnarray`, `gather`, `align`, and so on (e.g., you can use the `align*` environment).
```

We demonstrate a few more math equation environments below. Here is an unnumbered equation using the `equation*` environment:

```latex
\begin{equation*} 
\frac{d}{dx}\left( \int_{a}^{x} f(u)\,du\right)=f(x)
\end{equation*} 
```

\begin{equation*}
\frac{d}{dx}\left( \int_{a}^{x} f(u)\,du\right)=f(x)
\end{equation*}

Below is an `align` environment \@ref(eq:align):

```latex
\begin{align} 
g(X_{n}) &= g(\theta)+g'({\tilde{\theta}})(X_{n}-\theta) \notag \\
\sqrt{n}[g(X_{n})-g(\theta)] &= g'\left({\tilde{\theta}}\right)
  \sqrt{n}[X_{n}-\theta ] (\#eq:align)
\end{align} 
```

\begin{align}
g(X_{n}) &= g(\theta)+g'({\tilde{\theta}})(X_{n}-\theta) \notag \\
\sqrt{n}[g(X_{n})-g(\theta)] &= g'\left({\tilde{\theta}}\right)
  \sqrt{n}[X_{n}-\theta ] (\#eq:align)
\end{align}

You can use the `split` environment inside `equation` so that all lines share the same number \@ref(eq:var-beta). By default, each line in the `align` environment will be assigned an equation number. We suppressed the number of the first line in the previous example using `\notag`. In this example, the whole `split` environment was assigned a single number.

```latex
\begin{equation} 
\begin{split}
\mathrm{Var}(\hat{\beta}) & =\mathrm{Var}((X'X)^{-1}X'y)\\
 & =(X'X)^{-1}X'\mathrm{Var}(y)((X'X)^{-1}X')'\\
 & =(X'X)^{-1}X'\mathrm{Var}(y)X(X'X)^{-1}\\
 & =(X'X)^{-1}X'\sigma^{2}IX(X'X)^{-1}\\
 & =(X'X)^{-1}\sigma^{2}
\end{split}
(\#eq:var-beta)
\end{equation} 
```

\begin{equation}
\begin{split}
\mathrm{Var}(\hat{\beta}) & =\mathrm{Var}((X'X)^{-1}X'y)\\
 & =(X'X)^{-1}X'\mathrm{Var}(y)((X'X)^{-1}X')'\\
 & =(X'X)^{-1}X'\mathrm{Var}(y)X(X'X)^{-1}\\
 & =(X'X)^{-1}X'\sigma^{2}IX(X'X)^{-1}\\
 & =(X'X)^{-1}\sigma^{2}
\end{split}
(\#eq:var-beta)
\end{equation}

### Theorems and proofs {#theorems}

Theorems\index{theorem} and proofs are commonly used in articles and books in mathematics. However, please do not be misled by the names: a "theorem" is just a numbered/labeled environment, and it does not have to be a mathematical theorem (e.g., it can be an example irrelevant to mathematics). Similarly, a "proof" is an unnumbered environment. In this section, we always use the _general_ meanings of a "theorem" and "proof" unless explicitly stated.

In **bookdown**, the types of theorem environments supported are in Table \@ref(tab:theorem-envs). To write a theorem, you can use the syntax below:

````markdown
::: {.theorem}
This is a `theorem` environment that can contain **any**
_Markdown_ syntax.
:::
````

This syntax is based on Pandoc's [fenced `Div` blocks](https://pandoc.org/MANUAL.html#divs-and-spans) and can already be used in any R Markdown document to write [custom blocks.](https://bookdown.org/yihui/rmarkdown-cookbook/custom-blocks.html) **Bookdown** only offers special handling for theorem and proof environments. Since this uses the syntax of Pandoc's Markdown, you can write any valid Markdown text inside the block.

(ref:theorem-envs) Theorem environments in **bookdown**.

```r
knitr::kable(data.frame(
  Environment = names(bookdown:::theorem_abbr),
  `Printed Name` = unname(unlist(bookdown:::label_names_math)),
  `Label Prefix` = unname(bookdown:::theorem_abbr),
  stringsAsFactors = FALSE, check.names = FALSE
), caption = '(ref:theorem-envs)', booktabs = TRUE)
```

To write other theorem environments, replace `::: {.theorem}` with other environment names in Table \@ref(tab:theorem-envs), e.g., `::: {.lemma}`.

A theorem can have a `name` attribute so its name will be printed. For example,

````markdown
::: {.theorem name="Pythagorean theorem"}
For a right triangle, if $c$ denotes the length of the hypotenuse
and $a$ and $b$ denote the lengths of the other two sides, we have
$$a^2 + b^2 = c^2$$
:::
````

If you want to refer to a theorem, you should label it. The label can be provided as an ID to the block of the form `#label`. For example,

````markdown
::: {.theorem #foo}
A labeled theorem here.
:::
````

After you label a theorem, you can refer to it using the syntax `\@ref(prefix:label)`.\index{cross-reference} See the column `Label Prefix` in Table \@ref(tab:theorem-envs) for the value of `prefix` for each environment. For example, we have a labeled and named theorem below, and `\@ref(thm:pyth)` gives us its theorem number \@ref(thm:pyth):

````markdown
::: {.theorem #pyth name="Pythagorean theorem"}
For a right triangle, if $c$ denotes the length of the hypotenuse
and $a$ and $b$ denote the lengths of the other two sides, we have

$$a^2 + b^2 = c^2$$
:::
````

::: {.theorem #pyth name="Pythagorean theorem"}
For a right triangle, if $c$ denotes the length of the hypotenuse
and $a$ and $b$ denote the lengths of the other two sides, we have

$$a^2 + b^2 = c^2$$
:::

The proof environments currently supported are `r knitr::combine_words(names(bookdown:::label_names_math2), before = '\x60')`. The syntax is similar to theorem environments, and proof environments can also be named using the `name` attribute. The only difference is that since they are unnumbered, you cannot reference them, even if you provide an ID to a proof environment.

We have tried to make all these theorem and proof environments work out of the box, no matter if your output is PDF or HTML. If you are a LaTeX or HTML expert, you may want to customize the style of these environments anyway (see Chapter \@ref(customization)). Customization in HTML is easy with CSS, and each environment is enclosed in `<div></div>` with the CSS class being the environment name, e.g., `<div class="lemma"></div>`. For LaTeX output, we have predefined the style to be `definition` for environments `r knitr::combine_words(bookdown:::style_definition, before='\x60')`, and `remark` for environments `r knitr::combine_words(c('proof', bookdown:::style_remark), before='\x60')`. All other environments use the `plain` style. The style definition is done through the `\theoremstyle{}` command of the **amsthm** package. If you do not want the default theorem definitions to be automatically added by **bookdown**, you can set `options(bookdown.theorem.preamble = FALSE)`. This can be useful, for example, to avoid conflicts in single documents (Section \@ref(a-single-document)) using the output format `bookdown::pdf_book` with a `base_format` that has already included **amsmath** definitions.

Theorems are numbered by chapters by default. If there are no chapters in your document, they are numbered by sections instead. If the whole document is unnumbered (the output format option `number_sections = FALSE`), all theorems are numbered sequentially from 1, 2, ..., N. LaTeX supports numbering one theorem environment after another, e.g., let theorems and lemmas share the same counter. This is not supported for HTML/EPUB output in **bookdown**. You can change the numbering scheme in the LaTeX preamble by defining your own theorem environments, e.g.,

```latex
\newtheorem{theorem}{Theorem}
\newtheorem{lemma}[theorem]{Lemma}
```

When **bookdown** detects `\newtheorem{theorem}` in your LaTeX preamble, it will not write out its default theorem definitions, which means you have to define all theorem environments by yourself. For the sake of simplicity and consistency, we do not recommend that you do this. It can be confusing when your Theorem 18 in PDF becomes Theorem 2.4 in HTML.

Below we show more examples^[Some examples are adapted from the Wikipedia page https://en.wikipedia.org/wiki/Characteristic_function_(probability_theory)] of the theorem and proof environments, so you can see the default styles in **bookdown**.

::: {.definition}
The characteristic function of a random variable $X$ is defined by

$$\varphi _{X}(t)=\operatorname {E} \left[e^{itX}\right], \; t\in\mathcal{R}$$
:::


::: {.example}
We derive the characteristic function of $X\sim U(0,1)$ with the probability density function $f(x)=\mathbf{1}_{x \in [0,1]}$.

\begin{equation*}
\begin{split}
\varphi _{X}(t) &= \operatorname {E} \left[e^{itX}\right]\\
 & =\int e^{itx}f(x)dx\\
 & =\int_{0}^{1}e^{itx}dx\\
 & =\int_{0}^{1}\left(\cos(tx)+i\sin(tx)\right)dx\\
 & =\left.\left(\frac{\sin(tx)}{t}-i\frac{\cos(tx)}{t}\right)\right|_{0}^{1}\\
 & =\frac{\sin(t)}{t}-i\left(\frac{\cos(t)-1}{t}\right)\\
 & =\frac{i\sin(t)}{it}+\frac{\cos(t)-1}{it}\\
 & =\frac{e^{it}-1}{it}
\end{split}
\end{equation*}

Note that we used the fact $e^{ix}=\cos(x)+i\sin(x)$ twice.
:::

::: {.lemma #chf-pdf}
For any two random variables $X_1$, $X_2$, they both have the same probability distribution if and only if

$$\varphi _{X_1}(t)=\varphi _{X_2}(t)$$
:::

::: {.theorem #chf-sum}
If $X_1$, ..., $X_n$ are independent random variables, and $a_1$, ..., $a_n$ are some constants, then the characteristic function of the linear combination $S_n=\sum_{i=1}^na_iX_i$ is

$$\varphi _{S_{n}}(t)=\prod_{i=1}^n\varphi _{X_i}(a_{i}t)=\varphi _{X_{1}}(a_{1}t)\cdots \varphi _{X_{n}}(a_{n}t)$$
:::

::: {.proposition}
The distribution of the sum of independent Poisson random variables $X_i \sim \mathrm{Pois}(\lambda_i),\: i=1,2,\cdots,n$ is $\mathrm{Pois}(\sum_{i=1}^n\lambda_i)$.
:::

::: {.proof}
The characteristic function of $X\sim\mathrm{Pois}(\lambda)$ is $\varphi _{X}(t)=e^{\lambda (e^{it}-1)}$. Let $P_n=\sum_{i=1}^nX_i$. We know from Theorem \@ref(thm:chf-sum) that

\begin{equation*}
\begin{split}
\varphi _{P_{n}}(t) & =\prod_{i=1}^n\varphi _{X_i}(t) \\
& =\prod_{i=1}^n e^{\lambda_i (e^{it}-1)} \\
& = e^{\sum_{i=1}^n \lambda_i (e^{it}-1)}
\end{split}
\end{equation*}

This is the characteristic function of a Poisson random variable with the parameter $\lambda=\sum_{i=1}^n \lambda_i$. From Lemma \@ref(lem:chf-pdf), we know the distribution of $P_n$ is $\mathrm{Pois}(\sum_{i=1}^n\lambda_i)$.
:::

::: {.remark}
In some cases, it is very convenient and easy to figure out the distribution of the sum of independent random variables using characteristic functions.
:::

::: {.corollary}
The characteristic function of the sum of two independent random variables $X_1$ and $X_2$ is the product of characteristic functions of $X_1$ and $X_2$, i.e.,

$$\varphi _{X_1+X_2}(t)=\varphi _{X_1}(t) \varphi _{X_2}(t)$$
:::

::: {.exercise name="Characteristic Function of the Sample Mean"}
Let $\bar{X}=\sum_{i=1}^n \frac{1}{n} X_i$ be the sample mean of $n$ independent and identically distributed random variables, each with characteristic function $\varphi _{X}$. Compute the characteristic function of $\bar{X}$. 
:::

::: {.solution}
Applying Theorem \@ref(thm:chf-sum), we have

$$\varphi _{\bar{X}}(t)=\prod_{i=1}^n \varphi _{X_i}\left(\frac{t}{n}\right)=\left[\varphi _{X}\left(\frac{t}{n}\right)\right]^n.$$
:::
  
::: {.hypothesis name="Riemann hypothesis"}
The Riemann Zeta-function is defined as
$$\zeta(s) = \sum_{n=1}^{\infty} \frac{1}{n^s}$$
for complex values of $s$ and which converges when the real part of $s$ is greater than 1. The Riemann hypothesis is that the Riemann zeta function has its zeros only at the negative even integers and complex numbers with real part $1/2$.
:::

#### A note on the old syntax {#theorem-engine}

For older versions of **bookdown** (before v0.21), a `theorem` environment could be written like this:

````markdown
`r ''````{theorem pyth, name="Pythagorean theorem"}
For a right triangle, if $c$ denotes the length of the hypotenuse
and $a$ and $b$ denote the lengths of the other two sides, we have

$$a^2 + b^2 = c^2$$
```
````

This syntax still works, but we do not recommend it since the new syntax allows you to write richer content and has a cleaner implementation.

This conversion between the two syntaxes is straightforward. The above theorem could be rewritten in this way:

````markdown
::: {.theorem #pyth name="Pythagorean theorem"}
For a right triangle, if $c$ denotes the length of the hypotenuse
and $a$ and $b$ denote the lengths of the other two sides, we have

$$a^2 + b^2 = c^2$$
:::
````

You can use the helper function `bookdown::fence_theorems()` to convert a whole file or a piece of text. This is a one-time operation. We have tried to do the conversion from old to new syntax safely, but we might have missed some edge cases. To make sure you do not overwrite the `input` file by accident, you can write the converted source to a new file, e.g.,

```r
bookdown::fence_theorems("01-intro.Rmd", output = "01-intro-new.Rmd")
```

Then double check the content of `01-intro-new.Rmd`. Using `output = NULL` will print the result of conversion in the R console, and is another way to check the conversion. If you are using a control version tool, you can set `output` to be the same as `input`, as it should be safe and easy for you to revert the change if anything goes wrong.

### Special headers

There are a few special types of first-level headers that will be processed differently in **bookdown**. The first type is an unnumbered header that starts with the token `(PART)`. This kind of headers are translated to part titles\index{part}. If you are familiar with LaTeX, this basically means `\part{}`. When your book has a large number of chapters, you may want to organize them into parts, e.g.,

```
# (PART) Part I {-} 

# Chapter One

# Chapter Two

# (PART) Part II {-} 

# Chapter Three
```

A part title should be written right before the first chapter title in this part, both title in the same document. You can use `(PART\*)` (the backslash before `*` is required) instead of `(PART)` if a part title should not be numbered.

The second type is an unnumbered header that starts with `(APPENDIX)`, indicating that all chapters after this header are appendices\index{appendix}, e.g.,

```
# Chapter One 

# Chapter Two

# (APPENDIX) Appendix {-} 

# Appendix A

# Appendix B
```

The numbering style of appendices will be automatically changed in LaTeX/PDF and HTML output (usually in the form A, A.1, A.2, B, B.1, ...). This feature is not available to e-books or Word output.

### Text references

You can assign some text to a label and reference the text using the label elsewhere in your document. This can be particularly useful for long figure/table captions (Section \@ref(figures) and \@ref(tables)), in which case you normally will have to write the whole character string in the chunk header (e.g., `fig.cap = "A long long figure caption."`) or your R code (e.g., `kable(caption = "A long long table caption.")`). It is also useful when these captions contain special HTML or LaTeX characters, e.g., if the figure caption contains an underscore, it works in the HTML output but may not work in LaTeX output because the underscore must be escaped in LaTeX.

The syntax for a text reference is `(ref:label) text`, where `label` is a unique label^[You may consider using the code chunk labels.] throughout the document for `text`. It must be in a separate paragraph with empty lines above and below it. The paragraph must not be wrapped into multiple lines, and should not end with a white space. For example,

```markdown
(ref:foo) Define a text reference **here**. 
```

Then you can use `(ref:foo)` in your figure/table captions. The text can contain anything that Markdown supports, as long as it is one single paragraph. Here is a complete example:

````markdown
A normal paragraph.

(ref:foo) A scatterplot of the data `cars` using **base** R graphics. 

`r ''````{r foo, fig.cap='(ref:foo)'}
plot(cars)  # a scatterplot
```
````

Text references can be used anywhere in the document (not limited to figure captions). It can also be useful if you want to reuse a fragment of text in multiple places.


## Cross-references

We have explained how cross-references\index{cross-reference} work for equations (Section \@ref(equations)), theorems (Section \@ref(theorems)), figures (Section \@ref(figures)), and tables (Section \@ref(tables)). In fact, you can also reference sections using the same syntax `\@ref(label)`, where `label` is the section ID. By default, Pandoc will generate an ID for all section headers, e.g., a section `# Hello World` will have an ID `hello-world`. We recommend you to manually assign an ID to a section header to make sure you do not forget to update the reference label after you change the section header. To assign an ID to a section header, simply add `{#id}` to the end of the section header.  Further attributes of section headers can be set using standard [Pandoc syntax](http://pandoc.org/MANUAL.html#heading-identifiers).

When a referenced label cannot be found, you will see two question marks like \@ref(fig:does-not-exist), as well as a warning message in the R console when rendering the book.

You can also create text-based links using explicit or automatic section IDs or even the actual section header text.

- If you are happy with the section header as the link text, use it inside a single set of square brackets:
    - `[Section header text]`: example "[A single document]" via `[A single document]`

- There are two ways to specify custom link text:
    - `[link text][Section header text]`, e.g., "[non-English books][Internationalization]" via `[non-English books][Internationalization]`
    - `[link text](#ID)`, e.g., "[Table stuff](#tables)" via `[Table stuff](#tables)`

The Pandoc documentation provides more details on [automatic section IDs](http://pandoc.org/MANUAL.html#extension-auto_identifiers) and [implicit header references.](http://pandoc.org/MANUAL.html#extension-implicit_header_references)

Cross-references still work even when we refer to an item that is not on the current page of the PDF or HTML output. For example, see Equation \@ref(eq:binom) and Figure \@ref(fig:knitr-logo).

## Custom blocks

Custom blocks are often used in technical books to create salient boxes of code and/or narrative that call the reader's attention. For example, custom blocks may be used to highlight a note or a warning. These can be included in multiple **bookdown** output formats using Pandoc's syntax for fenced `Div` blocks (https://pandoc.org/MANUAL.html#divs-and-spans). Section 9.6 in the [_R Markdown Cookbook_](https://bookdown.org/yihui/rmarkdown-cookbook/custom-blocks.html) [@rmarkdown2020] for instructions.

The `bs4_book()` HTML output format includes styling for selected custom blocks; see Section \@ref(bs4-book).

## Citations {#citations}

Pandoc offers two methods for managing citations\index{citation} and bibliographic references in a document.

1. The default method is to use a Pandoc helper program called [`pandoc-citeproc`](https://github.com/jgm/pandoc-citeproc), which follows the specifications of the [Citation Style Language (CSL)](https://docs.citationstyles.org/en/v1.0.1/specification.html) and obtains specific formatting instructions from one of the huge number of available [CSL style files.](https://www.zotero.org/styles/)

1. Users may also choose to use either [**natbib**](https://ctan.org/pkg/natbib) (based on `bibtex`) or [**biblatex**](https://ctan.org/pkg/biblatex) as a "citation package". In this case, the bibliographic data files need to be in the `bibtex` or `biblatex` format, and the document output format is limited to PDF. Again, various bibliographic styles are available (please consult the documentation of these packages).

    To use **natbib** or **biblatex** to process references, you can set the `citation_package` option of the R Markdown output format, e.g.,
  
    ```yaml
    output:
      pdf_document:
        citation_package: natbib
      bookdown::pdf_book:
        citation_package: biblatex
    ```

Even if you choose `natbib` or `biblatex` for PDF output, all other output formats will be using `pandoc-citeproc`. If you use matching styles (e.g., `biblio-style: apa` for `biblatex` along with `csl: apa.csl` for `pandoc-citeproc`), output to PDF and to non-PDF formats will be very similar, though not necessarily identical.

For any non-PDF output format, `pandoc-citeproc` is the only available option. If consistency across PDF and non-PDF output
formats is important, use `pandoc-citeproc` throughout.

The bibliographic data can be in several formats. We have only shown examples of BibTeX databases in this section, and please see the ["Citations"](https://pandoc.org/MANUAL.html#citations) section of the Pandoc manual for other possible formats.

A BibTeX database is a plain-text file (with the conventional filename extension `.bib`) that consists of bibliography entries like this:

```bibtex
@Manual{R-base,
  title = {R: A Language and Environment for Statistical
    Computing},
  author = {{R Core Team}},
  organization = {R Foundation for Statistical Computing},
  address = {Vienna, Austria},
  year = {2016},
  url = {https://www.R-project.org/},
}
```

A bibliography entry starts with `@type{`, where `type` may be `article`, `book`, `manual`, and so on.^[The type name is case-insensitive, so it does not matter if it is `manual`, `Manual`, or `MANUAL`.] Then there is a citation key, like `R-base` in the above example. To cite an entry, use `@key` or `[@key]` (the latter puts the citation in braces), e.g., `@R-base` is rendered as @R-base, and `[@R-base]` generates "[@R-base]". A note can be included within the square brackets, e.g., `[a note about, @R-base]` will be rendered as "[a note about, @R-base]". If you are familiar with the **natbib** package in LaTeX, `@key` is basically `\citet{key}`, and `[@key]` is equivalent to `\citep{key}`.

There are a number of fields in a bibliography entry, such as `title`, `author`, and `year`, etc. You may see https://en.wikipedia.org/wiki/BibTeX for possible types of entries and fields in BibTeX.

There is a helper function `write_bib()` in **knitr** to generate BibTeX entries automatically for R packages, e.g.,

```{r write-bib, comment='', warning=FALSE}
# the second argument can be a .bib file
knitr::write_bib(c('knitr', 'stringr'), '', width = 60)
```

Once you have one or multiple `.bib` files, you may use the field `bibliography` in the YAML metadata of your first R Markdown document (which is typically `index.Rmd`), and you can also specify the bibliography style via `biblio-style` (this only applies to PDF output), e.g.,

```yaml
---
bibliography: ["one.bib", "another.bib", "yet-another.bib"]
biblio-style: "apalike"
link-citations: true
---
```

The field `link-citations` can be used to add internal links from the citation text of the author-year style to the bibliography entry in the HTML output.

When the output format is LaTeX, the list of references will be automatically put in a chapter or section at the end of the document. For non-LaTeX output, you can add an empty chapter as the last chapter of your book. For example, if your last chapter is the Rmd file `06-references.Rmd`, its content can be an inline R expression:

```markdown
`r "\x60r if (knitr::is_html_output()) '# References {-}'\x60"`
```

For more detailed instructions and further examples on how to use citations, please see the "Citations" section of the Pandoc manual.

## Index {#latex-index}

Currently the index\index{index} is only supported for LaTeX/PDF output. To print an index after the book, you can use the LaTeX package **makeidx** in the preamble (see Section \@ref(yaml-options)):

```latex
\usepackage{makeidx}
\makeindex
```

Alternatively, you can also use the **imakeidx** package:

```latex
\usepackage{imakeidx}
```

This packages offers additional features for formatting the index. For example:

```latex
\makeindex[intoc=true,columns=3,columnseprule=true,
           options=-s latex/indexstyles.ist]
```

In the above example, `intoc=true` will include an entry for the index into the table of contents, `columns=3` will format the index into three columns, and `columnseprule=true` will display a line between index columns. Finally, `options=-s latex/indexstyles.ist` will use additional formatting options from an index-style file located at `latex/indexstyles.ist`. Many other features are available in the **imakeidx** package. Please refer to its documentation for further details.

### Inserting Entries

An index entry can be created via the `\index{}` command in the book body, e.g.,

```latex
Version Control\index{Version Control} is an
important component of the SDLC.
```

Likewise, to insert a subentry for an item:

```
Git\index{Version Control!Git} is a
popular version control system.
```

The above example will add a "Git" entry underneath "Version Control" in the index.

To create a "see also" entry that appears at the bottom of an item's subentries (with no page number), first add the following beneath the call to `\makeindex` in your preamble file:

```latex
% to create a "see also" that appears at the bottom of the
% subentries and with no page number, do the following:
% \index{Main entry!zzzzz@\igobble|seealso{Other item}}

\newcommand{\ii}[1]{{\it #1}}
\newcommand{\nn}[1]{#1n}

\def\igobble#1{}
```

Then, use the `\index{Main entry!zzzzz@\igobble|seealso{Other item}}` syntax in your book. As an example:

```latex
Backups\index{Version Control!zzzzz@\igobble|seealso{backups}}
should be part of your version control system.
```

### Building the Index

To build the index, insert `\printindex` at the end of your book through the YAML option `includes -> after_body`.

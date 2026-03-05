:::{.callout-note}
This is a simple fenced div with a callout note.
:::

::::: {#special .sidebar}
This div has an ID and a class.

It contains multiple paragraphs.
:::::

::: Warning
This is a warning div using a simple class name.
:::

::: Nested
Outer div content.

::: Inner
Nested div content.
:::

Back to outer div.
:::

::: Warning ::::::
This is a warning.

::: Danger
This is a warning within a warning.
:::
::::::::::::::::::

Here's a fenced div with a list inside.

::: exercise
- A
- B
:::

::: exercise
foo bar
::: 

::: mathdeclare
\DeclareMathOperator{\E}{E{}}
\DeclareMathOperator{\Var}{Var{}}
:::

::: declare

A

:::

B

Pandoc requires that opening fences have attributes, so this is not a fenced
div:

:::
A
:::

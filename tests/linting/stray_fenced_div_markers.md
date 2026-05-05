# Stray fenced div markers

::: warning
Properly opened and closed div.
:::

A paragraph that is fine.

::::

Another paragraph.

:::: callout
A different fence count, properly closed.
::::

Inline `:::` should not trigger because it sits inside a code span.

Mid-line ::: text should not trigger either.

Some prose.

::::::

End.

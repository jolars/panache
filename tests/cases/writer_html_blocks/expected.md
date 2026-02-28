<!-- Source: pandoc/test/writer.markdown:L352-L451 -->

# HTML Blocks

Simple block on one line:

::: {}
foo
:::

And nested without indentation:

::: {}
::::: {}
::::::: {}
foo
:::::::
:::::

::::: {}
bar
:::::
:::

Interpreted markdown in a table:

<table>
<tr>
<td>
This is *emphasized*
</td>
<td>
And this is **strong**
</td>
</tr>
</table>
<script type="text/javascript">document.write('This *should not* be interpreted as markdown');</script>

This should be a code block, though:

```
<div>
foo
</div>
```

As should this:

```
<div>foo</div>
```

This should just be an HTML comment:

<!-- Comment -->

Multiline:

<!--
Blah
Blah
-->
<!--
    This is another comment.
-->

Code block:

```
<!-- Comment -->
```

Hr's:

<hr>
<hr />
<hr />
<hr>
<hr />
<hr />
<hr class="foo" id="bar" />
<hr class="foo" id="bar" />
<hr class="foo" id="bar">

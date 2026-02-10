# Raw Blocks Test

This tests the raw_attribute extension.

## HTML Raw Block

```{=html}
<div class="custom">
  <p>This should stay exactly as-is</p>
</div>
```

## LaTeX Raw Block

```{=latex}
\begin{equation}
  E = mc^2
\end{equation}
```

## OpenXML Raw Block

```{=openxml}
<w:p>
  <w:r>
    <w:br w:type="page"/>
  </w:r>
</w:p>
```

## Groff MS Raw Block

```{=ms}
.MYMACRO
blah blah
.NH 1
A Section
```

Regular code block for comparison:

```python
def hello():
    print("world")
```

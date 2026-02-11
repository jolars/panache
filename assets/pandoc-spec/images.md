# Images


A link immediately preceded by a `!` will be treated as an image.
The link text will be used as the image's alt text:

    ![la lune](lalune.jpg "Voyage to the moon")

    ![movie reel]

    [movie reel]: movie.gif

### Extension: `implicit_figures` ###

An image with nonempty alt text, occurring by itself in a
paragraph, will be rendered as a figure with a caption.  The
image's alt text will be used as the caption.

    ![This is the caption](/url/of/image.png)

How this is rendered depends on the output format. Some output
formats (e.g. RTF) do not yet support figures. In those
formats, you'll just get an image in a paragraph by itself, with
no caption. For LaTeX output, you can specify a [figure's
positioning](https://www.overleaf.com/learn/latex/Positioning_images_and_tables#The_figure_environment)
by adding the `latex-placement` attribute.

    ![This is the caption](/url/of/image.png){latex-placement="ht"}

If you just want a regular inline image, just make sure it is not
the only thing in the paragraph. One way to do this is to insert a
nonbreaking space after the image:

    ![This image won't be a figure](/url/of/image.png)\

Note that in reveal.js slide shows, an image in a paragraph
by itself that has the `r-stretch` class will fill the screen,
and the caption and figure tags will be omitted.

### Extension: `link_attributes` ###

Attributes can be set on links and images:

    An inline ![image](foo.jpg){#id .class width=30 height=20px}
    and a reference ![image][ref] with attributes.

    [ref]: foo.jpg "optional title" {#id .class key=val key2="val 2"}

(This syntax is compatible with [PHP Markdown Extra] when only `#id`
and `.class` are used.)

For HTML and EPUB, all known HTML5 attributes except `width` and
`height` (but including `srcset` and `sizes`) are passed through
as is. Unknown attributes are passed through as custom
attributes, with `data-` prepended. The other writers ignore
attributes that are not specifically supported by their output format.

The `width` and `height` attributes on images are treated specially. When
used without a unit, the unit is assumed to be pixels. However, any of
the following unit identifiers can be used: `px`, `cm`, `mm`, `in`, `inch`
and `%`. There must not be any spaces between the number and the unit.
For example:

```
![](file.jpg){ width=50% }
```

- Dimensions may be converted to a form that is compatible with
  the output format (for example, dimensions given in pixels will
  be converted to inches when converting HTML to LaTeX). Conversion
  between pixels and physical measurements is affected by the
  `--dpi` option (by default, 96 dpi is assumed, unless the image
  itself contains dpi information).
- The `%` unit is generally relative to some available space.
  For example the above example will render to the following.
  - HTML: `<img href="file.jpg" style="width: 50%;" />`
  - LaTeX: `\includegraphics[width=0.5\textwidth,height=\textheight]{file.jpg}`
    (If you're using a custom template, you need to configure `graphicx`
    as in the default template.)
  - ConTeXt: `\externalfigure[file.jpg][width=0.5\textwidth]`
- Some output formats have a notion of a class
  ([ConTeXt](https://wiki.contextgarden.net/Using_Graphics#Multiple_Image_Settings))
  or a unique identifier (LaTeX `\caption`), or both (HTML).
- When no `width` or `height` attributes are specified, the fallback
  is to look at the image resolution and the dpi metadata embedded in
  the image file.

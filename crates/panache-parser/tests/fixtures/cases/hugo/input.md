+++
title = "A title that must stay exactly as written"
[params]
value = 42
+++

This paragraph   needs ordinary formatting.

{{< foo
     attr1="value1"
     attr2="value2"
>}}

{{% note %}}
Keep   this **body**

> and this literal quote.
{{% /note %}}

- {{< foo
    attr="value"
  >}}

> {{< foo
>   attr="value"
> >}}

Text {{< foo
  attr="value"
>}} after   the call.

Normal   text after everything.

[Read more]({{< relref "about.md" >}} "Page title")

[Read more][page]

[page]: {{< relref "about.md" >}} "Page title"

# Heading {{< foo value="a b" >}}

# Heading {{< foo value="a b" >}} {#real .class}

# Heading {{< foo
    value="a b"
>}} after

- # Heading {{< foo
      value="a b"
  >}} after

> # Heading {{< foo
>     value="a b"
> >}} after

[^n]: Intro

    {{< foo >}}

    {{< foo
      value="a b"
    >}}

After the footnote.

- [ ] {{< checkbox >}}

- Intro.

  Before {{< continuation
    arg=`a
  b`
  >}} after.

After the list.

> - Intro.
>
>   Before {{< continuation
>     arg=`a
>   b`
>   >}} after.

After the quote.

Text {{< unpaired >}} \{{< escaped >}}

# Keep this heading

{{< /escaped >}}

| A | B | C |
| --- | --- | --- |
| {{< cell arg="a|b" >}} | Text |

[^inline]: Before {{< note
      arg=`a
    b`
    >}} after.

[^block]: {{< note_block
      arg=42
    >}}

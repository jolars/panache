# Reference Links

Explicit reference: [link text][ref1] in paragraph.

Implicit reference: [GitHub][] is a platform.

Shortcut reference: [Wikipedia] works too.

Multiple refs in one line: [first][1] and [second][2] together.

Unresolved reference: [missing link][nonexistent] stays as-is.

Case insensitive: [CASE][myref] matches lowercase definition.

With emphasis: [*emphasized* text][ref1] and [text with `code`][ref1].

[ref1]: https://example.com "Example Site"
[github]: https://github.com
[Wikipedia]: https://wikipedia.org
[1]: https://first.com
[2]: https://second.com
[myref]: https://matched.com

# Heading identifiers in HTML

The header above can be linked through

- [Heading identifiers in HTML]
- [Heading identifiers in HTML][]
- [the section on heading identifiers][heading identifiers in HTML]

Instead of giving the identifier explicitly:

- [Heading identifiers in HTML](#heading-identifiers-in-html)

Explicit link reference definitions always take priority over implicit heading
references. So, in the following example, the link will point to `bar`, not to
`#foo`:

## Foo

[foo]: bar

See [foo]

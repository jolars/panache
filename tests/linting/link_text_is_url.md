# link-text-is-url fixture

Plain match (fires):

See [https://example.com/](https://example.com/) for details.

Trailing-slash mismatch — destination would change (does not fire):

Visit [https://example.net/](https://example.net) sometime.

Scheme-less relative URL — fails autolink validation (does not fire):

Open [/docs/intro](/docs/intro).

URL with a title (does not fire):

Check [https://example.org/](https://example.org/ "Title").

Link text contains formatting (does not fire):

Read [**https://example.com/**](https://example.com/) carefully.

Reference-style link (does not fire — out of scope):

Try [https://example.com/][site].

[site]: https://example.com/

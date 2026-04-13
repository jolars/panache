<!-- Source: pandoc/test/writer.markdown:L632-L658 -->

## With ampersands

Here's a [link with an ampersand in the URL](http://example.com/?foo=1&bar=2).

Here's a link with an amersand in the link text: [AT&T](http://att.com/ "AT&T").

Here's an [inline link](/script?foo=1&bar=2).

Here's an [inline link in pointy braces](/script?foo=1&bar=2).

## Autolinks

With an ampersand: <http://example.com/?foo=1&bar=2>

- In a list?
- <http://example.com/>
- It should.

An e-mail address: <nobody@nowhere.net>

> Blockquoted: <http://example.com/>

Auto-links should not occur here: `<http://example.com/>`

```
or here: <http://example.com/>
```

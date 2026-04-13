<!-- Source: pandoc/test/writer.markdown:L82-L100 -->

# Indented code blocks + escapes

Code:

```
---- (should be four hyphens)

sub status {
    print "working";
}

These should not be escaped:  $ \\ > [ {
```

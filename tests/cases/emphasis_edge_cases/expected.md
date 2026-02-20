# Emphasis Edge Cases Test Suite

## Rule of 3s

***bar***

\*\*foo\*

\*foo\*\*

\***foo**

**foo**\*

## Nested Emphasis

**foo *bar* baz**

*foo **bar** baz*

***foo* bar**

**foo *bar***

## Overlapping Delimiters

\*foo **bar\* baz**

\*\*foo \*bar\*\* baz\*

## Adjacent Patterns

\*foo\*\*bar\*

*foo* *bar*

**foobar**

*foo*bar\*

## Intraword Emphasis (asterisks)

un*frigging*believable

un**frigging**believable

## Intraword Emphasis (underscores)

feas_ible

un_frig_gable

## Whitespace Flanking

- foo\*

*foo*

- foo \*

\*\* bar\*\*

**bar**

\*\* bar \*\*

## Punctuation Flanking

"*foo*"

"**bar**"

(*italic*)

(**bold**)

## Mixed Delimiters

\*foo **bar\* baz**

\*\*foo \*bar\*\* baz\*

*foo **bar** baz*

**foo *bar* baz**

## Same Character Sequences

***foo*** bar

***foo** bar*

**foo** *bar*\*\*

## Backslash Escapes

\*not emphasis\*

\**not bold\**

*escape \* inside*

**escape \*\* inside**

## Left-Flanking Only

\*a

\*\*a

## Right-Flanking Only

a\*

a\*\*

## Both Flanking

a\*b

a\*\*b

## Underscore Word Boundary

foo_bar_baz

*foo_bar*

*foo_bar_baz*

## Complex Nesting

***foo** bar **baz***

***foo* bar *baz***

**foo *bar **nested** baz* qux**

## Empty Emphasis

\*\*

------------------------------------------------------------------------

- - 

------------------------------------------------------------------------

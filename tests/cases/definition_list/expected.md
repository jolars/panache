# Definition Lists

Simple definition list:

Term 1
:   Definition 1

Term 2
:   Definition 2

Multiple definitions per term:

Term A
:   First definition for A
:   Second definition for A

Term B
:   Definition with tilde marker
:   Another tilde definition

Complex example with inline markup:

API
:   Application Programming Interface

REST
:   Representational State Transfer

GraphQL
:   A query language for APIs developed by *Facebook*

Loose format (blank line before definition):

Term Loose 1

:   Definition 1

Term Loose 2
:   Definition 2

Compact format (no blank before definition):

Apple
:   A fruit

Orange
:   Also a fruit
:   Also a color

Term 1
:   Definition 1

Term 2 with *inline markup*
:   Definition 2 { some code, part of Definition 2 } Third paragraph of
    definition 2, quite long so that it wraps multiple lines and should be
    wrapped and indented properly.

Term 3
:   Definition with lazy continuation. Second paragraph of the definition.

Example violation
:   ``` r
    a <- 1
    ```

A definition list with nested items
:   Here comes a list (or wait, is it?) - A - B

Term
:   Definition 2, with a very long ling, we should wrap this and total line
    width should not exceed 80.

Continuation of the definition requires four spaces or a tab, so this is not a
continuation of the definition, but a new paragraph.

Term
:   The following is also not a continuation.

A paragraph.

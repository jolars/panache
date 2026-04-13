# Ignore Directives Test

This document tests ignore directives for formatting and linting.

<!-- panache-ignore-format-start -->
This    paragraph   has    weird     spacing   that
should  be   preserved   exactly.
<!-- panache-ignore-format-end -->

This paragraph should be formatted normally with proper spacing.

<!-- panache-ignore-lint-start -->
#### Heading level skip should not be linted
<!-- panache-ignore-lint-end -->

<!-- panache-ignore-start -->
Both    formatting   and   linting    ignored   here
##### Another heading level skip
<!-- panache-ignore-end -->

Regular content continues here.

- List item 1
- List item 2
  <!-- panache-ignore-format-start -->
  Nested    content   with    spacing
  <!-- panache-ignore-format-end -->
- List item 3

Done.

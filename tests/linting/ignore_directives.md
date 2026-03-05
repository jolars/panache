# Test Document with Ignore Directives

#### This should trigger a warning (skip from h1 to h4)

Some text.

<!-- panache-ignore-lint-start -->

##### This should NOT trigger a warning even though context is wrong

More content.

<!-- panache-ignore-lint-end -->

## This should NOT trigger a warning (h2 after h5 in ignore region)

Even more content.

<!-- panache-ignore-start -->

#### This heading in ignore-both region

<!-- panache-ignore-end -->

## Another h2 heading

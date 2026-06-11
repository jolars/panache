# Nested task list indentation

Children of a task item align at the list content column (2 spaces), not past
the `[ ]` checkbox, so sublists stay sublists instead of collapsing into code
blocks or lazy text.

- [ ] a
  - [ ] b
    - [ ] c

- [ ] parent task with a description that is long enough that it has to wrap
      onto a second line
  - [ ] child task with its own long description that also needs to wrap to a
        second line here

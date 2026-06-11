# Indented code in list items

An indented code block inside a list item is converted to a fence whose content
keeps its true logical indentation --- the list's content indent is stripped,
not doubled.

- item

  ```
  code1
  code2
  ```

- nested indentation is preserved relative to the block

  ```
  line1
    line2
  ```

# Quarto formatter TODO

## Formatter improvements

- Lists: Current ListItem formatting derives marker/indent from raw text;
  fragile for numbered lists and nested mixes. Use the parsed structure (e.g.,
  capture marker and following space as explicit tokens/nodes) so you can compute
  hanging indents precisely and keep wrapped lines aligned.
- Avoid emitting extra trailing newlines (audit nodes that push a newline
  unconditionally).

## Parser/lexer coverage to add

- HTML blocks and inline HTML beyond comments.
- Thematic breaks (---, \*\*\*, \_\_\_) vs table underlines.
- Nested lists with mixed bullets and numbers; list continuation lines.
- Block quotes with nested lists/code blocks.
- Escapes and entities.

## Testing and quality

- Fuzzing (cargo-fuzz) and corpus from Quarto docs.
- Property tests for tokenization invariants (concatenated token text == input).
- add a few big documents for performance smoke tests.

## Config/CLI/editor integration

- Provide CLI: panache [--check] [--write] [--config PATH] [--stdin|PATHS].
- Neovim: expose a robust CLI with --stdin --stdout for formatprg or provide an LSP/formatter endpoint.

## Architecture polish

- Keep DivInfo node (you defined it) and populate it; same for CodeInfo vs including newline in fence nodes.

## Performance

- Benchmark wrapping and parsing on large files (cargo bench); preallocate buffers based on input size.

## What to fix next (priority)

4. List structure improvements

- Parser: in ListItem, emit explicit children:
  - ListIndent (WHITESPACE before marker)
  - ListMarker (including “1.”/“-”/“+”/“\*”)
  - MarkerSpace (the one space after marker)
  - ItemContent (rest of the line plus continuations)
- Formatter: compute hanging indent from ListIndent + ListMarker + MarkerSpace and wrap ItemContent accordingly; support nested lists and ordered markers.
- Rationale: fixes fragility for complex/nested lists and removes heuristic scanning.

5. Populate DivInfo and stop including newline in fence nodes

- DivFenceOpen: DivMarker + DivInfo + NEWLINE (newline outside the DivInfo).
- CodeFenceOpen already has CodeInfo but includes the newline; move the newline out for consistency.
- Formatter: update to read DivInfo/CodeInfo cleanly.
- Rationale: cleaner CST and easier formatting.

7. Coverage follow-ups (incremental)

- Thematic breaks (and their ambiguity with tables).
- HTML blocks/inline beyond comments.
- List continuation lines and mixed nested lists.
- Block quotes containing lists and code blocks.
- Escapes/entities in lexer.

# Math parser and formatter reference

Read this reference for changes that cross the TeX CST, semantic atom model, or
formatter. User-visible formatting behavior belongs in
`crates/panache-formatter/src/formatter/math/STYLE.md`, not here.

## Host and CST boundary

- Read math content through `syntax::math::math_content_text()`, not
  `MATH_CONTENT.text()`. Block parsing can interleave container prefixes such
  as `LINE_PREFIX` and host `NEWLINE` into continuation lines. The helper
  selects `MATH_*` tokens so formatting cannot leak or accumulate prefixes.
- `MATH_SPACE` and `MATH_NEWLINE` are intentionally distinct from host
  `WHITESPACE` and `NEWLINE`. `MATH_SPACE` is required to distinguish authored
  math whitespace from blockquote-prefix whitespace; `MATH_NEWLINE` retains the
  same boundary consistently.
- Scripts are native CST structure. `MATH_SCRIPTED` owns one base atom and one
  or more `MATH_SUBSCRIPT` or `MATH_SUPERSCRIPT` children. Unbraced text bases
  and arguments split at Unicode-scalar boundaries; comments and blank lines
  stop attachment.
- Host-only equation labels remain between ordered `MATH_CONTENT` segments.
  Delimiters, attributes, labels, and container prefixes must not be absorbed
  into the TeX subtree to simplify formatter code.

## Semantic lowering and layout

- Operator class and precedence are semantic interpretation, not CST shape;
  macros can override them. Share the Panache-owned semantic atom stream across
  consumers.
- Formatter interpretation inherits a scripted base atom's class across its
  script. This is required for scripted relation and assignment breaks.
- Panache retains both sides of the full TeXbook contextual coercion rule: a
  `Bin` becomes `Ord` at list start and after `Bin`, `Rel`, `Open`, `Punct`, or
  `Op`, and also before `Rel`, `Close`, or `Punct`. A binary at list end stays
  unchanged because it may be malformed dangling input. The pinned Badness
  version omits the punctuation and right-context cases; keep the intentional
  differentials pinned by
  `panache_coerces_after_punctuation_where_badness_does_not` and
  `panache_coerces_binary_before_closing_delimiter_where_badness_does_not`.
- An ASCII sign immediately following an unbraced TeX dimension-scanning
  command is an ordinary, non-breakable atom with asymmetric attachment: keep
  the authored command-to-sign gap, but bind the sign to the dimension on its
  right. The pinned Badness formatter instead treats this sign as a binary
  operator; keep the intentional differential pinned by
  `panache_attaches_dimension_signs_where_badness_treats_them_as_binary`.
- Authored `\\` separates layout rows but not the semantic atom stream. Derive
  atoms in source order, then lower rows separately; otherwise, a sign after a
  row break is incorrectly coerced as though it began a new math list.
- Free-display definition relations use two layouts. Automatic wrapping treats
  `:=` and `:=_i` as ordinary relations and aligns a later relation under the
  definition colon. In authored rows, a definition-led chain remains at the
  display indent. Assignment-arrow commands retain their right-hand-side
  anchor.
- Tight environment grids discard cell-relative continuation offsets when any
  non-final cell is multiline. Reset each multiline continuation to the
  environment body indent; preserve atom-relative offsets for ordinary aligned
  grids.
- Inside `\left` and `\right`, each top-level punctuation-delimited segment may
  own one well-formed environment. A following environment glues to punctuation
  on the preceding `\end` line and hangs from its actual source column.
  Adjacent environments in one segment cross the preservation boundary.
- Compose environments from semantic atoms while retaining CST source ranges.
  Ordinary delimiters and punctuation can share one lexical word token, so
  lexical token boundaries alone are insufficient.

## Oracle and preservation cases

- Badness 0.10's formatter keeps postfix signs tight, so `N(t-)` now has
  byte parity even though its semantic atom stream still reports the sign as
  binary. It also inserts an extra space after some wrapped relations with
  authored operand whitespace. Keep this formatter defect pinned by
  `panache_uses_one_space_after_wrapped_relations_where_badness_uses_two`;
  Panache continues to emit one space.
- The pinned Badness formatter splits CST-separated scripted tails from
  non-colon composite relations (`<=_i` becomes `< =_i`, likewise `>=_i` and
  `==_i`). Panache preserves these relations. Keep them outside mandatory byte
  parity until the pinned oracle is corrected. Definition relations such as
  `:=_i` have byte parity.
- Preserve malformed input, unescaped lone dollars, unproven argument domains,
  documented Panache/Badness differences, and shapes beyond the supported
  semantic contract. Every non-typed route in the census needs a specific
  reason; do not add a catch-all unsupported category.
- Badness test projectors may mechanically rename kinds, remove wrapper
  offsets, and discard documented host trivia. Any projector that parses,
  infers attachment, or repairs a tree hides a production CST defect.

## Stabilization baseline

The stable formatter landed in commit `c7f72568` on 2026-08-27. Treat these
numbers as comparison points only under a comparable toolchain and environment.

- On Linux 6.18.45 with Rust 1.97.1, nine alternating 400-iteration release
  runs over `benches/documents/math.qmd` measured medians of 565.86 microseconds
  for parsing, 1,008.93 microseconds for formatter-only work, and 1,664.71
  microseconds for the full pipeline. Set `PANACHE_BENCH_FORMAT_MATH=1` to
  enable math formatting in that benchmark.
- Production-style `wasm-pack --release --target web` output measured 1,710,641
  bytes raw and 620,750 bytes gzip. The reviewed increase primarily came from
  the native typed formatter and exhaustive semantic tables; no Badness code
  ships at runtime.

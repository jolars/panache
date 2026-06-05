# Math formatter cross-validation corpus

Bare TeX **math content** (no `$`/`$$` delimiters --- `format_math` consumes
clean content). One construct family per subdirectory. Files are `*.tex`.

Two harnesses walk this tree:

- `tests/math_corpus_properties.rs` (**Tier 1**, always-on, no external dep):
  idempotency `format(format(x)) == format(x)`, parser losslessness
  `parse(x).text() == x`, and gate-off verbatim `format(x, off) == x`. Covers
  **every** case, including `macro_dependent/`.
- `tests/math_cross_validation.rs` (**Tier 2**, dev-only `pulldown-latex`
  oracle): semantic-equivalence invariance --- the formatter must not change the
  rendered meaning, asserted on normalized MathML
  `render(x) == render(format(x))`.

## Context mapping

The `MathContext` is chosen by subdirectory: `inline/` → `Inline`; everything
else → `Display`. (`EnvironmentBody` is not yet reachable from the real
formatter --- standalone `\begin{env}` blocks still parse as opaque `TEX_BLOCK`;
environment fixtures live as `Display` content here, matching the only path the
formatter exercises today.)

## KaTeX-parseable constraint (Tier 2)

Every fixture **outside** `macro_dependent/` must parse under `pulldown-latex`
with no `\newcommand`/custom-macro dependency, using only standard TeX/KaTeX
symbols. Fixtures that require document-level macros (e.g. `\E`, `\Var`) go in
`macro_dependent/`, which Tier 2 **excludes** --- Tier 1 still covers them
(losslessness/idempotency need no renderer).

The Tier-2 harness applies a **four-way rule** per case:

- oracle rejects the **input** → skip (input outside oracle scope), counted.
- oracle accepts input but rejects `format(input)` → **fail** (formatter broke
  parseability).
- both accepted, normalized MathML differs → **fail** (meaning drift).
- both accepted, equal → pass.

Some well-formed fixtures (e.g. `display/` rows with a top-level `\\` outside an
environment) are legitimately rejected by the oracle and skip. The harness
**fails if the skipped fraction exceeds its threshold**, so silent
oracle-coverage erosion stays visible --- keep the renderable majority
renderable.

## Future (Phase 5)

The static symbol → atom-class table (Tier 3) that validates the operator
interpretation module will live alongside it under
`tests/fixtures/math_symbol_classes/` (not yet present).

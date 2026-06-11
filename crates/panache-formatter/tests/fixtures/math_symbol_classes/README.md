# Math symbol-class fixture (Tier 3)

Pins `formatter::math::operators` --- the math operator *interpretation table*
that maps a TeX symbol to its `AtomClass` (`Ord`/`Bin`/`Rel`/`Open`/`Close`/
`Punct`/`Op`) and drives operator spacing (Phase 5) and the upcoming semantic
line-breaking (Phase 6).

Where the Tier-1 corpus (`math_corpus/`) checks parser/format properties and the
Tier-2 oracle (`math_cross_validation.rs`) checks *render invariance*, this tier
pins the static table itself: a typo'd class or a dropped command passes every
other test today. The harness is `tests/math_symbol_classes.rs`; the
`pulldown-latex` oracle is **dev-only** (see the `TEMPORARY` note in
`Cargo.toml`), never a runtime dependency.

## `symbol-classes.tsv`

Tab-separated, three columns; `#` comment lines and blank lines ignored.

  | column       | meaning                                                                                                                                                            |
  | ------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
  | `token`      | the TeX symbol **as written** — leading `\` ⇒ command row, else char row.                                                                                          |
  | `atom_class` | expected `operators::AtomClass`. On a command row, `Ord` means *not in the table* (`command_class` returns `None`; the formatter defaults such commands to `Ord`). |
  | `oracle`     | expected `pulldown-latex` `Content` class for the probe `a <token> b`, or `skip`.                                                                                  |

`oracle` tokens: `binop`, `relation`, `largeop`, `function`, `open`, `close`,
`punct`, `ordinary`, `skip`.

The fixture is an **independent** enumeration of the table, so it catches drift
in both directions: a changed class (assertion 1 fails), and a deleted command
(its lookup returns `None`, assertion 1 fails). The `oracle` column then grounds
each recorded class in a real LaTeX parser (assertion 2).

## `oracle = skip`

`\left` / `\right` are delimiter framing --- pulldown emits no standalone
`Content` event for them --- and `\frac` is a multi-argument visual, so the
probe `a <token> b` is meaningless. These rows skip the oracle check but still
pin their table class (assertion 1).

## Recorded divergences (do **not** "fix" them)

The oracle is a curated parser, not gospel; two symbols are recorded with
pulldown's view in the `oracle` column while we keep our (deliberate) class:

- **`\lim`** --- we classify `Op`; pulldown emits `Function`. For spacing both
  are Op-like (Ord-like, and coerce a following `+`/`-` to unary), so `Op` is
  right for us. Recorded `oracle = function`.
- **`\asymp`** --- we classify `Rel` (AMS makes it `\mathrel`); pulldown emits a
  binary op. We keep `Rel`. Recorded `oracle = binop` so a future pulldown bump
  that changes this surfaces as a fixture review rather than a silent pass.

## Known limitation

The fixture cannot detect a command *added* to `command_class` but not added
here --- a `match` arm isn't enumerable from the test. When you extend the
table, add the corresponding row(s) to keep coverage honest.

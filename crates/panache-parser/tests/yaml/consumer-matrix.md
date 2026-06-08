# YAML consumer-divergence matrix

Empirical classification of where Panache's YAML-1.2 substrate verdict diverges
from the **real consumers** of document YAML, driven by the oracle audit in
`scripts/yaml-oracle/` (regenerate with `scripts/yaml-oracle/run.sh`).

Consumers (three distinct measured parsers, not interchangeable libyaml wrappers):

- **libyaml** — pandoc's Haskell `yaml`/libyaml, the frontmatter parser. Ground
  truth = `pandoc_direct` (pandoc reading the YAML as a metadata block);
  `psych_libyaml` is a cross-check. The lenient baseline: accepts duplicate keys
  (last value wins).
- **jsyaml** — js-yaml (YAML 1.2), the parser Quarto uses for frontmatter and
  hashpipe `#|` cell options. Rejects duplicate keys and tabs.
- **ryaml** — R's `yaml` package, used by the RMarkdown toolchain
  (`rmarkdown::yaml_front_matter` for frontmatter, knitr for `#|` options).
  libyaml-based, but measured to additionally REJECT duplicate keys and tabs —
  so it is its own profile (diverges from js-yaml on 31 suite cases, from
  pandoc/libyaml on duplicate keys).

Active consumer set per (flavor, location) — see `YamlValidationContext::new`:

| flavor + location          | active consumers     |
| -------------------------- | -------------------- |
| Pandoc, Frontmatter        | `{libyaml}`          |
| Quarto, Frontmatter        | `{libyaml, jsyaml}`  |
| RMarkdown, Frontmatter     | `{libyaml, ryaml}`   |
| Quarto, Hashpipe           | `{jsyaml}`           |
| RMarkdown, Hashpipe        | `{ryaml}`            |
| CommonMark/GFM, Frontmatter| `{}` (lenient)       |
| substrate (suite tests)    | all checks, no Pool-2 |

A doc is rejected under a context iff **any** active consumer rejects it.

Substrate verdict is taken to equal the suite's `yaml12` verdict: Panache has
full suite conformity (every allowlisted case parses iff 1.2-valid), so
`yaml12` is an exact proxy for the substrate accept/reject.

## Headline conclusions

1. **The ADD direction (Pool-2 consumer-only checks) is the high-value, clean
   work.** These are real silent failures today — Panache accepts YAML the
   pipeline rejects, so the user only finds out at render time (the exact bug
   that prompted this).
2. **The SUPPRESS direction (making Panache more lenient) is low-value and
   context-dependent.** Every candidate is an exotic edge case, and the headline
   "pandoc accepts tabs as indentation" is *false as a blanket rule*: pandoc
   accepts tabs in scalar content / flow / after a block-seq dash but **rejects**
   tabs in explicit-key context (Y79Y/006–009). The panache check that fires
   (`PARSE_UNEXPECTED_INDENT`) is overloaded across 12 cases with mixed pandoc
   verdicts (6 accept, 6 reject), so it **cannot** be safely suppressed at check
   granularity. Deferred — see below.

## Pool-2 consumer-only checks to ADD (substrate accepts, a consumer rejects)

### B1. Implicit empty block key — `rejecting_consumers = {libyaml, jsyaml}` — LAND

A block mapping key whose only non-trivia content is the `:` (e.g. `:`,
`: a`⏎`: b`, `- :`, `? : x`). Valid YAML 1.2 (the suite marks these valid) but
rejected by **all three** real consumers, uniformly.

Reinstate the reverted `check_implicit_empty_block_key` (reuse
`PARSE_INVALID_KEY_TOKEN`). It is **block-only** — this is load-bearing: the
flow-context empty-key cases below are *accepted* by libyaml and js-yaml and
must NOT be flagged.

Confirmed reject by all three (single-doc): `NHX8` (`:`), `2JQS` (`: a`⏎`: b`),
`6M2F`, `S3PD`, `M2N8/00` (`- ? : x`), `SM9W/01`, `UKK6/00` (`- :`). Plus the
multidoc `NKF9` sub-doc. These are exactly the 8 allowlisted 1.2-valid cases the
draft "flips" — placing the check in Pool-2 (never runs under substrate) keeps
the suite green.

Must stay accepted (flow context — do NOT flag): `HM87/00` (`[:x]`), `CFD4`
(`[ : empty key ]`), `58MP` (`{x: :x}`), `FRK4` (`{ ? foo :, : bar, }`).

### B2. Duplicate mapping keys — `rejecting_consumers = {jsyaml, ryaml}` — LAND

`a: 1`⏎`a: 2` (and nested). Rejected by **js-yaml** (`duplicated mapping key`)
and **R-yaml** (`Duplicate map key`); pandoc/libyaml and Ruby-Psych **accept**
(last value wins, pandoc may warn but exits 0). Verified by direct probe. So
this is a *partial* (bucket C) divergence:

- (Quarto, Frontmatter) `{libyaml, jsyaml}` → REJECT (jsyaml rejects).
- (RMarkdown, Frontmatter) `{libyaml, ryaml}` → REJECT (ryaml rejects).
- (Quarto, Hashpipe) `{jsyaml}` / (RMarkdown, Hashpipe) `{ryaml}` → REJECT.
- (Pandoc, Frontmatter) `{libyaml}` → ACCEPT.

New `check_duplicate_keys` (block + flow mapping), Pool-2,
`rejecting_consumers = {Jsyaml, RYaml}`. No existing substrate check covers this.

## No-op (substrate already matches consumers)

- **Reserved `@` / backtick** starting a plain scalar (`a: @foo`): rejected by
  1.2 substrate AND all consumers. Already handled.
- The large majority of error-contract cases: substrate rejects, all reject.

## SUPPRESS candidates (substrate rejects, a consumer accepts) — DEFERRED

Recorded for completeness; **not landing now**. Each would make Panache accept
something it currently flags, but all are exotic and several need parser surgery
(splitting an overloaded diagnostic into context-specific sub-checks).

Per-check suppress-safety (a check is safe to blanket-suppress for a consumer
only if *every* case firing it is accepted by that consumer):

| panache code | #cases firing | pandoc accepts all? | jsyaml accepts all? | action |
| --- | --- | --- | --- | --- |
| `LEX_COMMENT_NOT_PRECEDED_BY_SPACE` | 1 (`SU5Z`) | yes | yes | safe-but-trivial; defer (1 case, low confidence) |
| `PARSE_INVALID_PLAIN_SCALAR_IN_FLOW` | 1 (`YJV2` `[-]`) | yes | no | pandoc-only; defer (1 case) |
| `PARSE_UNEXPECTED_INDENT` (tabs etc.) | 12 | **no** (6/12) | no (4/12) | NOT safe; needs tab-context sub-check split — defer |
| all other reject codes | — | no | no | genuine, keep |

### The tab story (the TODO's "tabs as indentation")

Tab cases under `PARSE_UNEXPECTED_INDENT`, with the pandoc/js split:

| case | shape | pandoc | jsyaml |
| --- | --- | --- | --- |
| `DK95/01` | tab in dq-scalar continuation | ok | ok |
| `Y79Y/000` | tab as block-scalar content | ok | ok |
| `Y79Y/003` | tab indent in flow seq | ok | ok |
| `Y79Y/004` | `-<TAB>-` | ok | err |
| `Y79Y/005` | `- <TAB>-` | ok | err |
| `Y79Y/006` | `?<TAB>-` | **err** | err |
| `Y79Y/007` | `? -`⏎`:<TAB>-` | **err** | err |
| `Y79Y/008` | `?<TAB>key:` | **err** | err |
| `Y79Y/009` | `? key:`⏎`:<TAB>key:` | **err** | err |

Conclusion: there is no single "tabs" verdict. Acting on this requires the
parser to emit a tab-context-specific diagnostic (separable from generic indent
errors) so the accepted contexts can be gated per-consumer. Tracked as future
work; the (flavor, location) plumbing landed here is the prerequisite. When it
lands, the rejecting set for tabs is `{jsyaml, ryaml}` (R-yaml rejects all 11
substrate-valid tab cases pandoc accepts), so RMarkdown joins Quarto in
rejecting them.

## pandoc-only frontmatter rejections (metadata shape) — OUT OF SCOPE

11 cases where `pandoc_direct=err` but `psych_libyaml=ok` (e.g. `LX3P`
`[flow]: block`, `SBG9`, top-level sequences/scalars). These are pandoc's
*metadata-must-be-a-mapping* rule, a frontmatter-shape concern distinct from
YAML parse validity — a candidate future lint, not a YAML validator check. See
`scripts/yaml-oracle/oracle-discrepancies.md`.

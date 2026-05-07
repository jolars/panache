# YAML shadow scanner rewrite

## Status (as of 2026-05)

The scanner rewrite has fully landed. v2 `scanner.rs` + `parser_v2.rs`
build the returned tree, structural diagnostics flow through
`validator.rs`, and **the v1 line-based lexer is deleted**. The live
diagnostic path is `parse_yaml_report` → `validate_yaml` →
`parse_v2`. There is no v1 directive-ordering pass.

What landed:

- Steps 1–10: scanner scaffolded, `Mark`/`SimpleKey`/trivia/directives/
  flow indicators/block indicators/quoted/block/plain scalars all
  implemented in `scanner.rs`.
- Step 11: `parser_v2.rs` consumes the scanner and emits the rowan
  green tree.
- Step 12a (initial cutover, commit `9b442587`): `parse_yaml_report`
  builds the returned tree from the v2 scanner+builder; structural
  diagnostics route through `validator::validate_yaml`; the v1
  `emit_*` family, `parse_stream`, `emit_document`,
  `has_explicit_key`, `doc_level_property_present`, the flat `? key`/
  `: value` shortcut, and the `DocumentBody` enum are deleted.
- Step 12b (final cutover, this session): the validator's
  `check_directives` cluster (driven off scanner-emitted `Directive`
  tokens) covers what the v1 directive-ordering pass used to do, and
  `check_invalid_dq_escapes` plus the scanner's own `push_diagnostic`
  calls cover `LEX_INVALID_DOUBLE_QUOTED_ESCAPE` and friends. With
  parity confirmed, `parse_yaml_report` no longer calls the v1 lexer,
  and `lexer.rs` + `model.rs::YamlToken` / `YamlTokenSpan` are
  deleted.

What is still deferred (residual work):

- Tag/anchor/alias dispatch (`!`, `&`, `*`) in `scanner.rs`. These
  characters currently fall through to plain scalar at token start.
  The user-visible consequence: malformed inputs like
  `!foo "bar"\n%TAG ...\n---\n` are parsed as one big plain scalar
  followed by a doc-start (no `Directive` token, no
  `PARSE_DIRECTIVE_AFTER_CONTENT` diagnostic). The
  `parse_yaml_report_detects_directive_after_content` test was
  switched to an EB22-shape input (comment terminates the scalar, so
  the `%`-prefixed line is dispatched fresh and emits a `Directive`
  token) until tag dispatch lands.
- `events.rs` projection helpers
  `collect_doc_scalar_text_with_newlines`,
  `collect_value_scalar_text_with_newlines`, and
  `quoted_val_event_multi_line` still re-stitch multi-line scalars
  in projection because the scanner emits per-segment scalar tokens.
  Unifying these into a single styled `Scalar` token is a follow-up.
- Step 13 (recover unlocked cases) is partially done — the lexer
  removal alone moved triage by +38 cases (`passes_now_count`:
  144 → 182). More can likely be allowlisted by walking the
  `passes_now` bucket against the current `allowlist.txt`.

The plan below remains accurate as a record of the design decisions
and the migration sequence; refer to it when picking up residual
work.

## Context

The YAML shadow parser in `crates/panache-parser/src/parser/yaml/` is built
on a line-based lexer (`lexer.rs`, ~1,167 LOC). Each line is classified
by shape (mapping line, sequence entry, doc marker, comment, block-scalar
header) before any content tokens are emitted, with an indent stack
threaded across lines.

This was a startup simplification. It does not match how YAML 1.2
actually tokenizes — a correct YAML scanner is stateful in ways that
ignore line boundaries: simple-key candidacy, multi-line plain scalars,
multi-line quoted scalars, explicit-key (`?`/`:`) continuations.

The line-based design accumulated workarounds that motivated the
rewrite (line numbers omitted — the parser-side workarounds have since
been deleted; see the Status section above):

- `collect_doc_scalar_text_with_newlines`,
  `collect_value_scalar_text_with_newlines`,
  `quoted_val_event_multi_line` in `events.rs` — re-stitch multi-line
  scalars in projection because they were lexed as separate per-line
  tokens. Still present (deferred deletion).
- `has_explicit_key` in `parser.rs` — string-prefix lookahead to
  classify document body as block map vs scalar. **Deleted at cutover.**
- `doc_level_property_present` peek in `parser.rs` — guarded property
  absorption. **Deleted at cutover.**
- Flat `? key` + `: value` shortcut in `parser.rs` — only handled
  single-line explicit-key entries; nested-collection bodies under
  explicit keys could not be expressed. **Deleted at cutover.**

The trigger to rewrite is multi-line plain scalars: the
"is this line a continuation of a scalar or a new mapping key?"
decision needs the **simple-key table** mechanism (tentatively register
a candidate key, confirm/cancel on a downstream `:`). A line-based
lexer has already committed by the time `:` is seen. Once that
mechanism exists, explicit-key, multi-line quoted, and folded
continuations collapse into the same machinery.

The rewrite is contained: parser-crate scoped, shadow-only (no impact
on the live `yaml_parser` dependency path), and gated by the existing
event-parity harness (no allowlisted case may regress at cutover).

## Goals

- Replace the line-based lexer with a streaming, char-by-char scanner
  modeled on PyYAML `scanner.py` (libyaml's design, in readable Python).
- CST stays lossless (every byte recoverable; trivia preserved).
- Existing event-parity harness is the regression bar — every case
  currently in `crates/panache-parser/tests/yaml/allowlist.txt` must
  still pass at cutover.
- Eliminate the workarounds enumerated above, not work around them.
- Land the work as a sequence of independently-green commits on `main`.
  No long-lived branch.

## Non-goals

- Replacing the live `yaml_parser` dependency. That cutover is downstream
  of this work, after parity coverage grows further.
- Touching the formatter, LSP, or CLI. Parser-crate scoped.
- Pre-loading recovery for malformed input beyond what the line-based
  lexer already provides. Recovery improvements come after the architecture
  is right.

## Resolved design decisions

The collaborative discussion left four open questions; all four are
resolved here so the implementation steps are unambiguous.

### 1. Trivia model: in-queue tokens

Scanner emits `Trivia { kind: Whitespace | Newline | Comment, start, end }`
tokens between meaningful tokens. Parser ignores trivia for structural
decisions (consults `tokens_taken` / non-trivia count for simple-key
indices) and consumes them into the CST as it walks the queue.

**Why:** single source of truth for "what's between tokens" — the parser
never re-scans the input. Matches the existing architecture's instinct
(current lexer already emits Whitespace/Newline/Comment tokens with byte
ranges per `lexer.rs:615` and `model.rs:82–105`). Source-range
reconstruction in the CST builder would risk divergence with the
scanner's notion of trivia.

**Cost:** queue is larger; simple-key bookkeeping must use a
non-trivia token counter. Both are cheap.

### 2. Token enum: new in `scanner.rs`, displaces `model.rs::YamlToken` at cutover

Define a new token enum local to `scanner.rs`. The existing `YamlToken`
in `model.rs:82–105` continues to be used by the live (line-based)
parser path until cutover. At the cutover commit, the new enum
replaces the old one wholesale and `model.rs` is updated.

**Why:** the new token kinds (e.g. styled `Scalar`, simple-key-aware
indicators, structured `Trivia`) have different semantics than the
existing variants. Trying to extend in place creates a hybrid that's
worse than either. Two enums coexisting briefly during transition is
acceptable; conversion at the boundary is unnecessary because nothing
crosses (the new parser path consumes only the new tokens).

### 3. Scalar cooking: raw span only, cooking in projection

Scanner emits `Scalar { style: Plain | SingleQuoted | DoubleQuoted | Literal | Folded, start, end }`
with the source span. Folding, escape-decoding, and indentation
stripping happen in `events.rs` projection helpers when they're needed
for the event stream. CST stores raw bytes.

**Why:** keeps the scanner allocation-light, keeps the CST byte-exact,
and avoids two sources of truth (cooked-in-token vs cooked-in-projection).
The current architecture already cooks in projection; this preserves
that split.

### 4. Diagnostics: side-channel `Vec<YamlDiagnostic>` on scanner state

Scanner accumulates diagnostics into a `Vec<YamlDiagnostic>` field on
its state struct. Parser appends its own diagnostics. Both surface via
`YamlParseReport::diagnostics` (already wired, `model.rs:49–52`).
`diagnostic_codes` (`model.rs:54–79`) stays as the registry; new codes
get added there as needed.

**Why:** matches existing infrastructure exactly. Encoding diagnostics
as queue-side trivia tokens would be novel without obvious upside and
would complicate `tokens_taken` accounting.

## Architecture

### Core types (in new `scanner.rs`)

```rust
struct Mark { index: usize, line: usize, column: usize }

struct SimpleKey {
    token_number: usize,   // global non-trivia token count when registered
    required: bool,        // true in block context where indent makes mandatory
    mark: Mark,
}

enum ScalarStyle { Plain, SingleQuoted, DoubleQuoted, Literal, Folded }

enum Token {
    StreamStart, StreamEnd,
    DocumentStart, DocumentEnd,        // --- / ...
    Directive { /* %YAML, %TAG */ },
    BlockSequenceStart, BlockMappingStart, BlockEnd,
    FlowSequenceStart, FlowSequenceEnd,
    FlowMappingStart,  FlowMappingEnd,
    BlockEntry, FlowEntry, Key, Value, // -, ',', ?, :
    Alias, Anchor, Tag,
    Scalar { style: ScalarStyle, start: Mark, end: Mark },
    Trivia { kind: TriviaKind, start: Mark, end: Mark },
}

enum TriviaKind { Whitespace, Newline, Comment }

struct Scanner<'a> {
    input: &'a str,
    cursor: Mark,
    tokens: VecDeque<Token>,
    tokens_taken: usize,             // non-trivia tokens taken
    indent: i32,
    indent_stack: Vec<i32>,
    simple_keys: Vec<Option<SimpleKey>>,
    flow_level: usize,
    allow_simple_key: bool,
    diagnostics: Vec<YamlDiagnostic>,
}
```

### Scanner main loop

```
fetch_more_tokens():
    scan_to_next_token()              // emits Trivia for whitespace/newlines/comments
    if at end → StreamEnd; done
    stale_simple_keys()               // expire candidates that aged out
    unwind_indent(current_column)     // pop indent levels, emit BlockEnd
    dispatch on peek char:
        % at col 0           → fetch_directive
        --- / ... at col 0   → fetch_document_indicator
        [ ] { }              → fetch_flow_*
        ,                    → fetch_flow_entry
        - then space/EOL     → fetch_block_entry
        ? then space/EOL     → fetch_key
        : then space/EOL     → fetch_value (consult simple_keys)
        * & !                → fetch_alias / anchor / tag
        | >                  → fetch_block_scalar
        ' "                  → fetch_flow_scalar
        otherwise            → fetch_plain_scalar
```

The simple-key mechanism: on entry to `fetch_plain_scalar`,
`fetch_flow_scalar`, `fetch_alias`, etc., register a candidate in
`simple_keys[flow_level]` recording `tokens_taken`. On `fetch_value`
(unprefixed `:`), check `simple_keys[flow_level]`: if a candidate
exists, splice `BlockMappingStart` (or `FlowMappingStart`) before the
candidate token in the queue, emit `Key`, then emit `Value`. If not, emit
`Value` only. Candidates expire on next-line-at-same-or-less-indent,
on a blank line, or on flow boundaries.

### Parser-side coupling (`parser.rs` / `parser_v2.rs`)

The body emitters were *not* refactored in place. Instead, a parallel
`parser_v2.rs` was built that consumes the scanner's token stream and
emits the rowan green tree directly, keyed on `BlockMappingStart` /
`Key` / `Value` / `BlockEntry` / `BlockEnd` / flow indicators. Trivia
tokens are consumed inline into the CST. Explicit-key entries (`Key`
token) route through the same path as implicit keys, with
nested-collection bodies handled recursively.

`parser.rs` shrank to a slim orchestrator that calls v1 lex + v2
scanner + validator and stitches the v2 stream into the
`DOCUMENT > YAML_METADATA_CONTENT > YAML_STREAM` envelope expected by
downstream consumers.

What was **deleted at cutover** (commit `9b442587`):

- `parser.rs::parse_stream`, `emit_document`, `emit_block_map`,
  `emit_block_seq`, `emit_block_seq_item`, `emit_block_map_entry`,
  `emit_block_map_key`, `emit_block_map_value`, `emit_flow_map`,
  `emit_flow_map_entry`, `emit_flow_value_tokens`,
  `emit_flow_sequence`, `emit_scalar_document`, `emit_token_as_yaml`
- `parser.rs::has_explicit_key`, `doc_level_property_present`,
  `document_follows`, `scan_plain_scalar_continuation`,
  `consume_block_scalar`, the `DocumentBody` enum
- The flat `? key` / `: value` shortcut path

What is still **live** (deferred to a follow-up cutover step):

- `events.rs::collect_doc_scalar_text_with_newlines`,
  `collect_value_scalar_text_with_newlines`,
  `quoted_val_event_multi_line` — projection still re-stitches
  multi-line scalars

What was deleted in step 12b (this session):

- `lexer.rs` and the `lex_mapping_tokens` / `lex_mapping_tokens_with_diagnostic`
  / `split_once_unquoted_key_colon` functions
- `model.rs::YamlToken`, `model.rs::YamlTokenSpan`
- The `lex_mapping_tokens` / `YamlToken` / `YamlTokenSpan` re-exports in
  `parser/yaml.rs`
- All `lexer_*` tests in `parser/yaml.rs`'s test module

### CST kinds

The 28 YAML-specific `SyntaxKind` variants enumerated in `syntax/kind.rs`
are sufficient. No new kinds expected. Block-scalar style is encoded in
the scalar token's source text (leading `|`/`>` plus chomping/indent
indicators), matching current convention.

## Migration plan — sequential commits on `main`

Each step compiles, tests pass, and is independently committable. The
old lexer remains the live path until step 8.

1. **Scaffold** — add `crates/panache-parser/src/parser/yaml/scanner.rs`
   with types (Mark, SimpleKey, ScalarStyle, Token, TriviaKind, Scanner)
   and a stub `Scanner::new` + `next_token` returning `StreamStart` /
   `StreamEnd`. Wire into `parser/yaml/mod.rs` as `pub(crate) mod scanner;`
   but no callers. Compiles; no behavior change.

2. **Char source + Mark advancement** — implement input cursor with
   line/column tracking. Unit tests for ASCII, newlines, mixed `\r\n`.

3. **Trivia scanning** — `scan_to_next_token` emits Whitespace, Newline,
   Comment trivia tokens. Unit tests verify byte ranges sum to input
   length when input is pure trivia.

4. **Directives + doc markers** — `%YAML`, `%TAG`, `---`, `...`. Unit
   tests for column-0 detection and end-of-line trailing-content
   diagnostics.

5. **Flow indicators** — `[ ] { } ,` plus `flow_level` bookkeeping and
   per-level simple-keys slot. Unit tests for nested flow contexts.

6. **Block indicators with simple-key table** — `- ? :` with candidate
   registration, expiration, and confirmation. The core mechanism. Unit
   tests cover: implicit key on same line, multi-line plain scalar
   followed by `:` (must NOT confirm), explicit `?` key, key in flow
   context.

7. **Quoted scalars** — single, double, with escape handling reported
   via diagnostic codes (cooking in projection, not here). Unit tests
   for multi-line quoted content.

8. **Block scalars** — literal `|`, folded `>`, with chomping and
   indentation indicators. Unit tests for the canonical forms.

9. **Plain scalars + multi-line continuation** — the case that motivated
   the rewrite. Unit tests for: continuation under indent, `:` inside a
   plain scalar body (must not break the scalar), continuation across
   blank-line boundaries (must terminate).

10. **Comparison harness** — a `#[ignore]`-gated test in `tests/yaml.rs`
    that runs the scanner over every allowlisted fixture's input and
    asserts the token stream is byte-complete (sum of token spans equals
    input length, no overlaps). This catches losslessness regressions
    before we reach the parser. Run manually after each scanner step.

11. **New parser path** — add a `parser_v2.rs` (or feature-gated
    branch in `parser.rs`) that consumes the scanner. Initially exercised
    by an `#[ignore]`d test that round-trips against the live parser on
    the allowlist. Build out the body emitters incrementally; each
    sub-commit may flip a few cases at a time on the v2 path.

12. **Cutover** — switch `parse_yaml_report` (`parser.rs`) to consume
    the scanner-built tree and route structural diagnostics through
    `validator.rs`. Delete the v1 `emit_*` family, `parse_stream`,
    `emit_document`, `has_explicit_key`, `doc_level_property_present`,
    the flat `? key`/`: value` shortcut, and the `DocumentBody` enum
    in `parser.rs`. Run the full allowlist; every case must pass.

    **Step 12 was split into 12a / 12b:**

    - 12a (commit `9b442587`) cut the tree-build path over to v2 but
      kept `lexer.rs` for directive ordering and lex-level
      diagnostics, and kept the `events.rs` re-stitching helpers.
    - 12b (this session) finished the lexer-side cutover: the
      validator's `check_directives` cluster is the live
      directive-ordering check, lex-level diagnostics flow through
      the scanner's `push_diagnostic` and the validator's
      `check_invalid_dq_escapes`, and `lexer.rs` plus
      `model.rs::YamlToken` / `YamlTokenSpan` are deleted.

13. **Recover unlocked cases** — regenerate
    `crates/panache-parser/tests/yaml/triage.json` via
    `cargo test -p panache-parser --test yaml yaml_suite_generate_triage_report -- --ignored`.
    Allowlist any cases newly in `passes_now` with rationale comments,
    one per shared root cause. Step 12b alone moved `passes_now_count`
    from 144 to 182 (+38), so a re-walk of `passes_now` against the
    current `allowlist.txt` is high-leverage follow-up work.

Steps 1–11 land on `main` without changing live behavior. Step 12 is the
single risky commit; by then the comparison harness has burned down the
surprises. Step 13 is pure win.

## Critical files

- `crates/panache-parser/src/parser/yaml/scanner.rs` — the rewrite
  (~2,851 LOC). Streaming char-by-char scanner with simple-key table.
- `crates/panache-parser/src/parser/yaml/parser_v2.rs` — consumes the
  scanner and emits the rowan green tree (~1,134 LOC).
- `crates/panache-parser/src/parser/yaml/validator.rs` — v2-aware
  structural validator. Each `check_*` function is one cluster of
  error contracts; `validate_yaml` composes them.
- `crates/panache-parser/src/parser/yaml/parser.rs` — slim
  orchestrator. Calls validator for structural diagnostics, then
  parser_v2 for tree construction.
- `crates/panache-parser/src/parser/yaml/events.rs` — projection
  helpers; the `*_with_newlines` / `*_multi_line` helpers are still
  live (deferred deletion).
- `crates/panache-parser/src/parser/yaml/model.rs` — `YamlDiagnostic`,
  `diagnostic_codes`, `YamlParseReport`. (`YamlToken` /
  `YamlTokenSpan` deleted in step 12b.)
- `crates/panache-parser/src/parser/yaml/mod.rs` — wiring.
- `crates/panache-parser/tests/yaml.rs` — fixture-driven harness;
  unchanged in role.
- `crates/panache-parser/tests/yaml/allowlist.txt` and `blocked.txt` —
  curated coverage list and the parallel record of cases the validator
  cannot yet catch without scanner-side enhancements.

## Reuse from existing code

- `YamlTokenSpan` byte-range conventions (`model.rs:108–113`) — the new
  `Token` follows the same pattern (Mark with byte index).
- `YamlDiagnostic` and `diagnostic_codes` (`model.rs:54–79`) — unchanged.
- `YamlParseReport` (`model.rs:49–52`) — unchanged.
- The 28 `SyntaxKind` YAML variants in `syntax/kind.rs` — unchanged.
- The four core test functions in `tests/yaml.rs`
  (`yaml_allowlist_cases_snapshot`,
  `yaml_allowlist_cases_cst_snapshot`,
  `yaml_allowlist_losslessness_raw_input`,
  `yaml_allowlist_projected_event_parity`) — unchanged.
- `YamlParseReport`-based public entry surface — unchanged.

## Verification

Per step (1–9): `cargo test -p panache-parser --test yaml` plus the
scanner's own unit tests.

After step 10 (comparison harness): manually run
`cargo test -p panache-parser --test yaml -- --ignored scanner_token_completeness`
(or whatever the harness gets named) over the full allowlist.

After step 11: manually run the v2 round-trip test over the allowlist
and confirm 100% match before promoting.

At step 12 (cutover): full validation gate:

```
cargo test -p panache-parser --test yaml
cargo clippy -p panache-parser --all-targets -- -D warnings
cargo fmt -p panache-parser -- --check
cargo test --workspace
cargo run -- debug format --checks all <a few sample .qmd files>
```

After step 13: regenerate triage and confirm
`fails_needs_feature_count` has dropped (and `passes_now_count` has
risen) with no movement into `error_contract_ok` or
`fails_needs_error_path` for non-error cases.

## Plan placement

Once approved, this plan should move from
`/home/jola/.claude/plans/` to
`/home/jola/projects/panache/.claude/skills/yaml-shadow-expand/scanner-rewrite.md`
so it lives alongside the skill it amends. The skill's `SKILL.md` should
also gain a short pointer to this plan in its "Architecture trajectory"
section, since the plan is the concrete instantiation of the trajectory
described there.

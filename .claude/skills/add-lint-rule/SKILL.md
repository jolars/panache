---
name: add-lint-rule
description: Add a new built-in lint rule to the Panache linter — wire it into
  the registry, gate it on the right extension/flavor, add a regression fixture
  with focused assertions, and document it.
---

Use this skill when asked to add a new built-in lint rule (warning, error, or
info), regardless of whether it ships with an auto-fix.

## Scope boundaries

- Built-in lint rules only. External-linter integrations (black, flake8, etc.)
  live in `src/linter/external_linters*` and are out of scope here.
- Rule logic walks the parser CST/AST. Do **not** add parser- or formatter-side
  workarounds. If the rule needs information the CST does not expose, surface it
  through a typed wrapper in `crates/panache-parser/src/syntax/` rather than
  re-parsing inside the rule.
- LSP and CLI consume diagnostics through the same `LintRunner`. The rule must
  not emit CLI-formatted strings; it produces `Diagnostic` values and the
  shared rendering paths handle presentation.

## Key files

- `src/linter/rules.rs` — `Rule` trait (note the required `metadata()` method),
  the `RuleMeta`/`DiagnosticCode`/`Requirement` types, `RuleRegistry`, and the
  `pub mod` list. Every new rule module is declared here.
- `src/linter/rules/<rule_name>.rs` — one file per rule. Contains the
  `pub struct <Name>Rule` plus its `impl Rule` (including `metadata()`) and unit
  tests.
- `src/linter.rs` — `all_rules()` lists every rule once; `default_registry()` is
  **data-driven**: it filters `all_rules()` by each rule's
  `RuleMeta::{requires, default_on}` and `config.lint`. There is no per-rule
  `if` guard to add. `builtin_rule_metadata()` exposes the metadata for tests.
- `src/linter/diagnostics.rs` — `Diagnostic`, `Severity`, `Location`, `Edit`,
  `Fix`, `DiagnosticNoteKind`. The full builder API for diagnostics.
- `src/syntax.rs` — re-exports `SyntaxKind`, `SyntaxNode`, and typed AST
  wrappers from `panache_parser::syntax`.
- `tests/linting.rs` + `tests/linting/<rule_name>.{md,qmd,Rmd}` — integration
  test fixtures. Pattern: a focused fixture file plus a `#[test]` that filters
  diagnostics by `code` and asserts count, span, and (if present) fix shape.
- `docs/reference/linter-rules.qmd` — the per-rule catalogue. Every rule needs a
  `### \`<rule-name>\` {#<rule-name>}` section. `tests/linter_rules_docs.rs`
  cross-checks this file against `builtin_rule_metadata()` and **fails the build**
  if a rule, code, severity, auto-fix flag, default, or requirement drifts.
- `docs/guide/linting.qmd` — user-facing prose guide; links to the reference and
  lists the default `[lint.rules]` keys. Update the example key list there too.

## Workflow

1. **Pick the rule name (kebab-case)** — this is the diagnostic `code`, the
   config key under `[lint.rules]`, and the slug used in URLs/help text. It
   must be unique and stable: renaming it is a breaking config change. Match
   tone of existing names (`heading-hierarchy`, `duplicate-reference-labels`,
   `adjacent-footnote-refs`).

2. **Decide gating before writing code** — these become fields on the rule's
   `RuleMeta`, the single source of truth for both registration and the docs:
   - Severity: `Warning` is the default; `Error` only for genuinely broken
     output; `Info` is reserved. A rule with several codes can mix severities;
     declare each in `RuleMeta::codes`.
   - `requires`: the `Requirement` variant the rule needs
     (`Always`, `Footnotes`, `Citations`, `Emoji`, `FencedDivs`,
     `FencedCodeAttributes`, `HeaderAttributes`, `TexMath`, or `ChunkFlavor`).
     Add a new variant (and its `is_satisfied`/doc-token mapping in
     `tests/linter_rules_docs.rs`) only if no existing one fits.
   - `default_on`: `true` for rules that run unless disabled; `false` for opt-in
     rules (registered only via `is_rule_explicitly_enabled`, documented with a
     `Default: Off` field).
   - Auto-fix: only ship a `Fix` when the replacement is unambiguous and
     preserves intent. If multiple resolutions are valid (rename vs delete vs
     merge), omit the fix and explain why in the docs. Set `RuleMeta::auto_fix`
     accordingly.

3. **Write a failing test first** (TDD per `AGENTS.md`). Either:
   - a unit test inside the new module under `#[cfg(test)] mod tests`, using
     `crate::parser::parse(input, Some(config.clone()))` and calling
     `Rule::check_tree(&tree, input, &config, metadata)`. `check_tree` is the
     default trait method that builds a one-off `LintIndex` for just this
     rule's declared interests and runs it — tests use it because `Rule::check`
     itself takes a `&LintContext`, which the runner (not tests) constructs.
     **or**
   - an integration fixture under `tests/linting/<rule_name>.{md,qmd,Rmd}` and
     a `#[test]` in `tests/linting.rs` that calls `lint_file(...)` and filters
     by `d.code == "<rule-name>"`.
   Cover the positive case, the negative ("should not flag") case, and any
   edge case the rule explicitly handles.

4. **Implement the rule** in `src/linter/rules/<rule_name>.rs`:
   - Rules do **not** walk the tree themselves. The runner does one shared
     `tree.preorder_with_tokens()` pass and buckets nodes by `SyntaxKind`;
     declare which kinds you want via `node_interests()` and read your bucket
     with `cx.nodes(KIND)` instead of `tree.descendants()`. This keeps a lint
     pass at one traversal no matter how many rules exist.
   - Cast bucket nodes to typed wrappers where available
     (`cx.nodes(SyntaxKind::LINK).iter().cloned().filter_map(Link::cast)`) —
     typed wrappers are preferred wherever they exist. For multi-kind rules,
     list every kind in `node_interests()` and iterate each bucket.
   - To scan `TEXT` tokens (e.g. byte-pattern checks), return `true` from
     `wants_text_tokens()` and iterate `cx.text_tokens()`.
   - Salsa-index-backed rules (those that use
     `symbol_usage_index_from_tree(.., cx.tree, ..)` and never read a bucket)
     leave `node_interests()` at its empty default.
   - Build `Location` with `Location::from_range(range, input)` or
     `Location::from_node(node, input)`.
   - For auto-fixes, prefer **insertions** (zero-width `TextRange::new(p, p)`)
     and **replacements over a precise span** rather than rewriting whole
     nodes. Multi-edit fixes are allowed but must be independent — they are
     applied in source order.
   - Honor the trait shape exactly. Implement `metadata()` (required), declare
     interests, then take a `&LintContext` (which bundles
     `tree`/`input`/`config`/`metadata`/`index`):
     ```rust
     fn metadata(&self) -> RuleMeta {
         RuleMeta {
             name: "<rule-name>",
             default_on: true,
             requires: Requirement::Always,
             auto_fix: false,
             codes: const { &[DiagnosticCode::warning("<rule-name>")] },
         }
     }

     fn node_interests(&self) -> &'static [SyntaxKind] {
         &[SyntaxKind::LINK] // omit (defaults to &[]) for index-backed rules
     }

     fn check(&self, cx: &LintContext) -> Vec<Diagnostic> {
         let input = cx.input; // also cx.tree / cx.config / cx.metadata as needed
         // ... iterate cx.nodes(SyntaxKind::LINK) ...
     }
     ```
     `codes` is `&'static [DiagnosticCode]`; wrap the array in a `const { … }`
     block (the `::warning`/`::error`/`::info` const constructors are not
     rvalue-promotable on their own). Import the new types alongside the trait:
     `use crate::linter::rules::{DiagnosticCode, LintContext, Requirement, Rule, RuleMeta};`.

5. **Wire it up** (no `if`-guard — registration is data-driven from `metadata()`):
   - Add `pub mod <rule_name>;` to `src/linter/rules.rs` (alphabetical, with
     the rest of the `pub mod` list).
   - Add one `Box::new(rules::<rule_name>::<Name>Rule)` entry to `all_rules()`
     in `src/linter.rs`. `default_registry()` filters that list by the rule's
     `RuleMeta::{requires, default_on}` and `config.lint`, so the gating you
     declared in step 2 takes effect automatically — there is nothing else to
     edit. (Opt-out via `[lint.rules]` is handled centrally for every rule.)

6. **Document in `docs/reference/linter-rules.qmd`** (enforced by
   `tests/linter_rules_docs.rs`):
   - New `### \`<rule-name>\` {#<rule-name>}` section under "Rules", placed near
     thematically related rules. Use the existing definition-list shape:
     `Severity`, `Auto-fix`, `Requirements` (if `requires` is not `Always`),
     optional `Default` (say `Off` when `default_on` is `false`),
     `Diagnostic codes`, `Description`, then an `**Example violation:**` block,
     and (if auto-fixable) an `**Auto-fix output:**` block.
   - Every `DiagnosticCode` in the rule's `metadata()` must appear in the
     section, the Severity field must name each severity emitted, and the
     Requirements field must mention the gating token — otherwise the
     consistency test fails. Multi-code rules get a `#### \`<code>\`` subsection
     per code.
   - If you reference the rule in the `docs/guide/linting.qmd` example
     `[lint.rules]` key list, keep that list in sync too (it is illustrative,
     not exhaustive, so this is optional).

7. **Validate** in this order:
   - Targeted: `cargo test --lib <rule_name>`,
     `cargo test --test linting <test_name>`, and
     `cargo test --test linter_rules_docs` (catches docs/metadata drift).
   - CLI smoke check on a copy of the fixture:
     `cargo run --quiet -- lint /tmp/<fixture>.md` and
     `cargo run --quiet -- lint --fix /tmp/<fixture>.md` (verify the file
     contents after `--fix`).
   - Full: `cargo check --workspace`, `cargo test --workspace`,
     `cargo clippy --workspace --all-targets --all-features -- -D warnings`,
     `cargo fmt -- --check`.

## Dos and don'ts

- **Do** keep diagnostic spans tight (point at the offending construct, not the
  whole line/paragraph) — this drives both the CLI caret and LSP underlines.
- **Do** put rule logic in the rule module. Shared cross-rule helpers belong
  in `src/linter/` (e.g. via `crate::salsa::symbol_usage_index_from_tree`),
  not duplicated.
- **Do** respect ignore directives implicitly — `LintRunner::run_with_metadata`
  already filters by ignored ranges, so the rule emits unconditionally.
- **Don't** emit CLI strings, ANSI codes, or `eprintln!` from a rule. Return
  `Diagnostic` values and let the renderer handle output.
- **Don't** rely on lexically scanning `input`. Walk the CST/AST.
- **Don't** add a fix that changes prose semantics. If the user's intent is
  ambiguous, omit the fix.
- **Don't** rename an existing rule code to fix a typo without a migration
  plan — the code is part of the user-facing config surface.

## Report-back format

When done, report:

1. Rule name (code), severity, and whether it ships an auto-fix.
2. The `Requirement` and `default_on` it declares in `RuleMeta`.
3. New files (rule module, fixture) and updated files (`rules.rs`, `linter.rs`
   `all_rules()`, `linting.rs`, `linter-rules.qmd`).
4. Targeted test names (including `linter_rules_docs`) and CLI fix smoke-test
   outcome.
5. Full-suite validation results (`cargo test --workspace`, clippy, fmt).

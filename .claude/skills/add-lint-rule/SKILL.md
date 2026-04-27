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

- `src/linter/rules.rs` — `Rule` trait, `RuleRegistry`, `pub mod` list. Every
  new rule module is declared here.
- `src/linter/rules/<rule_name>.rs` — one file per rule. Contains the
  `pub struct <Name>Rule` plus its `impl Rule` and unit tests.
- `src/linter.rs` — `default_registry()` registers each rule, gated on
  extension/flavor flags and `config.lint.is_rule_enabled(...)`.
- `src/linter/diagnostics.rs` — `Diagnostic`, `Severity`, `Location`, `Edit`,
  `Fix`, `DiagnosticNoteKind`. The full builder API for diagnostics.
- `src/syntax.rs` — re-exports `SyntaxKind`, `SyntaxNode`, and typed AST
  wrappers from `panache_parser::syntax`.
- `tests/linting.rs` + `tests/linting/<rule_name>.{md,qmd,Rmd}` — integration
  test fixtures. Pattern: a focused fixture file plus a `#[test]` that filters
  diagnostics by `code` and asserts count, span, and (if present) fix shape.
- `docs/guide/linting.qmd` — user-facing reference. Every rule needs a section,
  and auto-fix-capable rules also get a bullet under "Auto-Fix Capabilities".

## Workflow

1. **Pick the rule name (kebab-case)** — this is the diagnostic `code`, the
   config key under `[lint.rules]`, and the slug used in URLs/help text. It
   must be unique and stable: renaming it is a breaking config change. Match
   tone of existing names (`heading-hierarchy`, `duplicate-reference-labels`,
   `adjacent-footnote-refs`).

2. **Decide gating before writing code:**
   - Severity: `Warning` is the default; `Error` only for genuinely broken
     output; `Info` is reserved.
   - Extension/flavor gates in `default_registry`: e.g. `ext.footnotes`,
     `ext.citations`, `ext.emoji`, or
     `matches!(config.flavor, Flavor::Quarto | Flavor::RMarkdown)` for
     chunk-related rules. Skip the gate only if the rule is universally
     applicable.
   - Auto-fix: only ship a `Fix` when the replacement is unambiguous and
     preserves intent. If multiple resolutions are valid (rename vs delete vs
     merge), omit the fix and explain why in the docs.

3. **Write a failing test first** (TDD per `AGENTS.md`). Either:
   - a unit test inside the new module under `#[cfg(test)] mod tests`, using
     `crate::parser::parse(input, Some(config.clone()))` and calling
     `Rule::check` directly, **or**
   - an integration fixture under `tests/linting/<rule_name>.{md,qmd,Rmd}` and
     a `#[test]` in `tests/linting.rs` that calls `lint_file(...)` and filters
     by `d.code == "<rule-name>"`.
   Cover the positive case, the negative ("should not flag") case, and any
   edge case the rule explicitly handles.

4. **Implement the rule** in `src/linter/rules/<rule_name>.rs`:
   - Walk via `tree.descendants()` and match on `SyntaxKind` for raw kinds, or
     cast to typed wrappers (`Heading::cast(node)`, `FootnoteReference::cast`,
     etc.) when available — typed wrappers are preferred wherever they exist.
   - Build `Location` with `Location::from_range(range, input)` or
     `Location::from_node(&node, input)`.
   - For auto-fixes, prefer **insertions** (zero-width `TextRange::new(p, p)`)
     and **replacements over a precise span** rather than rewriting whole
     nodes. Multi-edit fixes are allowed but must be independent — they are
     applied in source order.
   - Honor the trait signature exactly:
     ```rust
     fn check(
         &self,
         tree: &SyntaxNode,
         input: &str,
         config: &Config,
         metadata: Option<&crate::metadata::DocumentMetadata>,
     ) -> Vec<Diagnostic>
     ```
     Even unused params should keep their names (`_config`, `_metadata`).

5. **Wire it up:**
   - Add `pub mod <rule_name>;` to `src/linter/rules.rs` (alphabetical, with
     the rest of the `pub mod` list).
   - Register in `src/linter.rs::default_registry` behind the right gate:
     ```rust
     if ext.<flag> && config.lint.is_rule_enabled("<rule-name>") {
         registry.register(Box::new(rules::<rule_name>::<Name>Rule));
     }
     ```
     Even default-enabled rules must call `is_rule_enabled` so users can opt
     out via `[lint.rules]`.

6. **Document in `docs/guide/linting.qmd`:**
   - New `### \`<rule-name>\`` subsection under "Lint Rules", placed near
     thematically related rules. Use the existing definition-list shape:
     `Severity`, `Auto-fix`, `Requirements` (if any), `Description`, then an
     `**Example violation:**` block, and (if auto-fixable) an
     `**Auto-fix output:**` block.
   - If the rule is auto-fix-capable, add a bullet under
     "Auto-Fix Capabilities".

7. **Validate** in this order:
   - Targeted: `cargo test --lib <rule_name>` and
     `cargo test --test linting <test_name>`.
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
2. Extension/flavor gate it lives behind in `default_registry`.
3. New files (rule module, fixture) and updated files (`rules.rs`,
   `linter.rs`, `linting.rs`, `linting.qmd`).
4. Targeted test names and CLI fix smoke-test outcome.
5. Full-suite validation results (`cargo test --workspace`, clippy, fmt).

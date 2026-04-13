# Formatter golden corpus ownership

This corpus is owned by top-level formatter integration tests.

- Primary purpose: formatted output expectations and idempotency.
- Harness: `tests/golden_cases.rs`.
- Scope: formatter behavior and user-visible formatting policy.

This corpus may share case names with parser corpus, but it is managed
independently from `crates/panache-parser/tests/cases/`.

Do not assume automatic syncing between the two corpora.

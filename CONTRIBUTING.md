# Contributing to Panache

Thanks for your interest in contributing to Panache.

## Getting Started

1. Install stable Rust (edition 2024 toolchain support required).
2. Fork and clone the repository.
3. Run the full validation command before and after changes:

```bash
cargo check && cargo test && cargo clippy --all-targets --all-features -- -D warnings && cargo fmt -- --check
```

## Development Workflow

- Prefer test-first changes:
  - For bug fixes, add a failing test first, then fix.
  - For new features, add focused tests with the change.
- Keep edits minimal and targeted to the requested behavior.
- Follow existing module and naming patterns in the area you touch.
- Do not edit `CHANGELOG.md` manually; releases are automated.
- Small, focused PRs are easier to review and merge; avoid large refactors or
  multiple behavior changes in one PR.
- Before spending time on a change, consider opening an issue to discuss the
  approach and ensure alignment with project goals.

## Working in Core Areas

### Parser + Formatter

- Keep parser behavior lossless (preserve all input bytes/markers in CST).
- Keep formatting policy in formatter code, not parser code.
- Prefer existing parser/formatter helpers over introducing parallel logic paths.
- For known CST-shape follow-ups around image reference wrappers, see
  `src/syntax/IMAGE_REFERENCE_CST.md`.
- Use debug checks when validating formatting behavior:
  - `cargo run -- debug format --checks all document.qmd` (idempotency +
    losslessness)
- For formatting behavior changes, validate idempotency:
  - `format(format(x)) == format(x)`

### Linter

- Add or adjust rules as focused, user-visible diagnostics.
- Keep fixes explicit and safe; avoid silent behavior changes.
- Add tests for rule behavior and autofix output.

### LSP

- Preserve protocol-correct document lifecycle behavior
  (`didOpen`/`didChange`/`didClose`).
- Be careful with UTF-16/UTF-8 position/range conversions.
- Prefer typed syntax wrappers for feature implementations when available.
- Follow the async/CST safety pattern in `src/lsp/ASYNC_SAFETY.md` (fetch async
  state first, then traverse CST without awaits).
- Run targeted LSP tests before full-suite revalidation.

## Debugging LSP (VS Code + Neovim)

Start the server manually for debugging:

```bash
panache lsp
```

Useful logging examples:

```bash
RUST_LOG=debug cargo run -- lsp
RUST_LOG=info ./target/release/panache lsp
```

### VS Code

- Install extension: `jolars.panache`.
- Confirm the extension can resolve the `panache` binary (or set
  `panache.commandPath`).
- If needed, set `panache.trace.server` to inspect client/server traffic.

### Neovim

- Confirm your LSP config uses `cmd = { "panache", "lsp" }`.
- Ensure root markers include one of: `.panache.toml`, `panache.toml`, `.git`.
- Verify `panache` is available in your shell `PATH` as seen by Neovim.

## Commits and Releases

This project uses Conventional Commits and semantic-release.

- `feat:` typically results in a **minor** release bump.
- `fix:` typically results in a **patch** release bump.
- `feat!:` / `fix!:` or `BREAKING CHANGE:` triggers a **major** bump.
- `docs:` and `chore:` are usually for maintenance/non-runtime behavior changes.

Examples:

- `feat(parser): support fenced div attributes in nested blocks`
- `fix(lsp): correct utf16 range conversion for diagnostics`
- `docs: clarify formatter idempotency checks`
- `chore(ci): tighten clippy gate in workflow`

## Useful References

- Contributor/engineering guidance: `.github/copilot-instructions.md`
- LSP setup and features: `docs/lsp.qmd`
- Project overview and CLI usage: `README.md`

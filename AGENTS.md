# AGENTS Guide

## Workflow Rules
- Before coding: read `architecture.md` and `TODO.md` to align on current design and priorities.
- During work: keep `TODO.md` and `architecture.md` in sync with any functional or behavioral changes; update them as you complete steps—no skipping.
- After changes: run relevant tests/formatters; note any skipped checks and why.
- Avoid destructive git commands (`reset --hard`, `checkout --`) unless explicitly approved; never revert user changes you didn’t make.

## Coding Practices
- Prefer clarity over cleverness; small functions with explicit types.
- Handle errors with context (`anyhow::Context`) and avoid `.unwrap()`/`.expect()` outside tests/main.
- Log with `tracing` at appropriate levels (`debug` for flow, `warn` for recoverable issues).
- Use `?` for propagation; keep fallible sections narrow.
- Keep async code cancellation-safe; avoid holding locks across `.await`.
- Batch I/O (DB writes, IMAP fetches) and bound batch sizes to control memory.
- Validate external inputs (IMAP responses, config) before trusting them.
- Add minimal, high-value comments for non-obvious logic; avoid restating code.
- Default to ASCII in code/docs; only use non-ASCII when necessary and consistent with the file.

## Rust Patterns to Prefer
- Use `Result<T, anyhow::Error>` at boundaries; convert specific errors at edges.
- Structure modules by responsibility (`sync`, `storage`, `imap`, `sanitize`) and keep data types in `types.rs`.
- Use `Arc` for shared state; avoid `Mutex` on hot paths when RwLock or cloning is cheaper.
- Favor iterators and `collect` over manual loops when it improves readability, but don’t sacrifice clarity.
- Use `Option` and `Result` combinators (`map`, `and_then`, `ok_or_else`) where it stays readable.
- Keep migrations idempotent; ignore harmless duplicate-column errors explicitly.

## Anti-Patterns to Avoid
- Swallowing errors: don’t ignore `Result`; log with context or propagate.
- Long-held locks across awaits; clone data out before async calls.
- Large unbounded allocations (e.g., fetching all UIDs/bodies at once); always chunk.
- Silent behavior changes without updating docs/tests.
- Using `unwrap`/`expect` in library code; prefer graceful handling.
- Copy-pasting code for minor variations; extract helpers instead.
- Hidden global state; keep shared resources explicit and well-scoped.

## Testing & Validation
- Run `cargo test` (or targeted modules) after meaningful changes; note if skipped.
- For parser/protocol changes, add unit tests (see `tests/` for patterns).
- Sanity-check IMAP interactions with debug logs when changing sync/query logic.

## Documentation Discipline
- Keep `architecture.md` updated when altering flows, data models, or components.
- Keep `TODO.md` current with completed/added work; move items between Now/Next/Later as priorities shift.
- Summarize notable risks or limitations in docs when introducing them.

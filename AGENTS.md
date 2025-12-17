# AGENTS Guide (Rust TUI Mail Client)

This repo is built to be maintained by humans + agents without drift. The job is not “write code”, it is “advance the system while keeping docs and behavior aligned”.

## North Star

- Keep behavior legible.
- Keep changes small and reviewable.
- Keep docs (`architecture.md`, `TODO.md`) authoritative and up to date.

## Non-Negotiables: Documentation Discipline

- Before touching code, read `architecture.md` and `TODO.md`. If they disagree with the code, treat that as a bug and resolve it explicitly (either fix code or fix docs, not neither).
- During work, update `TODO.md` as you go. If you complete a step, mark it done immediately. If you discover new work, add it immediately.
- After any behavioral change, update `architecture.md` in the same PR/commit. No “we’ll document later”.
- If you change an interface (types, config, storage schema, IMAP behavior, UI behavior), add a short note in `architecture.md` describing the new contract and any edge cases.

## How Agents Should Operate Here

- Always start by stating: (1) what you are changing, (2) which files you will touch, (3) how you will validate it.
- Make the smallest possible diff that achieves the goal. Prefer 2–4 commits over one sprawling one.
- Do not invent requirements. If something is ambiguous, search the repo for existing decisions and follow them. If still unclear, add a TODO entry describing the ambiguity and choose the least invasive default.
- No silent behavior changes. If the user experience changes (keybindings, sort order, threading, sync policy), call it out in `TODO.md` and `architecture.md`.

## Repo Structure Philosophy (Single Owner per File)

- “One file owns one thing” is the default. If a file grows into a grab-bag, split it by responsibility.
- UI state and domain logic must not be interleaved. Keep rendering/input glue thin.
- Prefer explicit module boundaries:
  - `imap/` for protocol and fetch logic
  - `sync/` for reconciliation, policies, scheduling
  - `storage/` for DB and persistence (schema + queries)
  - `ui/` for TUI state, rendering, keymaps, event loop
  - `types.rs` (or `domain/`) for shared domain types
  - `config/` for config parsing + defaults + validation

If this repo already has a different structure, follow it, but keep the “single owner” rule.

## Change Protocol (Do This Every Time)

- Step 1: Identify the current behavior by reading code and docs, not by guessing.
- Step 2: Add or update TODO items for the exact work you’re about to do (Now/Next/Later).
- Step 3: Implement the change with tight scope.
- Step 4: Validate:
  - `cargo fmt`
  - `cargo clippy` (no new warnings; justify any allow)
  - `cargo test` (or targeted tests if slow)
- Step 5: Update `architecture.md` if behavior, flow, or data changed.
- Step 6: Leave the repo in a coherent state. If something is unfinished, it must be tracked in `TODO.md` and ideally gated behind a feature flag or clearly marked TODO.

## Rust Rules That Actually Matter Here

- Avoid clever lifetimes. Prefer owned data (`String`, `Vec`) at boundaries. Optimize later.
- Keep types explicit at module boundaries. Inside modules, infer freely if it stays readable.
- No `unwrap()` / `expect()` in library code. In `main` it is allowed only for truly impossible states, otherwise still prefer `anyhow::Context`.
- Errors must carry context. Every external boundary should add `Context` that names the operation (IMAP fetch, parse envelope, DB write, render, etc).
- Prefer `thiserror` for internal error enums when you need structure; otherwise `anyhow` at app boundaries is fine.
- If you introduce concurrency, prefer message passing (channels) over shared mutable state. If you must share state, keep the lock scope tiny and never hold a lock across `.await`.

## Async, Cancellation, and Backpressure

- Assume IMAP and network calls can hang, fail, or return partial results. Always time out or be cancellable.
- All long operations must be chunked. Never fetch “all bodies” or “all UIDs” unbounded.
- Always bound queues and batches. Memory growth is a bug.
- Keep cancellation safe: if a task is cancelled mid-sync, storage should remain consistent (idempotent writes, transactional updates where appropriate).
- Do not spawn tasks “just because”. Every background task must have a clear owner, lifecycle, and shutdown path.

## Storage Discipline (DB, Cache, Migrations)

- Migrations must be idempotent and safe to re-run.
- Schema changes require:
  - a migration
  - a short note in `architecture.md`
  - a quick sanity test or at least a targeted integration check (open DB, run migration, run basic query)
- Never couple UI directly to raw DB rows. Map to domain types.

## TUI Event Loop Discipline

- UI must stay responsive. Never block the render/event thread on network or DB.
- Input handling should translate events into intents/actions, then dispatch to domain/sync layers.
- Keep keymap and commands centralized and documented (a `keymap` module or `commands` module).
- Rendering should be a pure-ish function of state: minimize side effects during draw.

## Logging and Debuggability

- Use `tracing` consistently. The rule is: if a bug happens in the field, logs should tell you where it happened and what it was doing.
- `debug` for flow, `info` for major lifecycle events (sync start/end), `warn` for recoverable anomalies, `error` for failures.
- Log counts and identifiers, not full email bodies. Avoid leaking sensitive content by default.

## Testing Expectations

- Parsing/protocol logic: add unit tests.
- Sync reconciliation logic: add scenario tests (even if lightweight).
- Storage: prefer tests that run against a temporary DB.
- UI: test the command layer and reducers/state transitions more than pixel output.

If a change is not easily testable, write down why in `TODO.md` and add at least one validation hook (logs, debug mode, or a small deterministic harness).

## Git Safety

- No destructive commands (`reset --hard`, `checkout --`, history rewrites) unless explicitly asked.
- Do not revert edits you did not make.
- Commit messages should describe behavior change, not vibes.

## “Done” Means

- Code compiles.
- Format and lint clean (or explicitly justified).
- Tests run (or explicitly justified).
- `TODO.md` reflects reality.
- `architecture.md` reflects reality.
- No hidden behavior changes.

If you cannot meet “Done”, you must leave an explicit trail in `TODO.md` with what is incomplete, what is risky, and what the next step is.

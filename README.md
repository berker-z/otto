# Otto

Otto is a Rust IMAP syncer for Gmail. It authorizes with OAuth2, opens one IMAP connection per folder, and keeps a local SQLite cache of messages and sanitized bodies. Each run skips work when MODSEQ/EXISTS match; otherwise it fetches only new UIDs and updates flags/labels, parsing messages in parallel and writing in batches.

## Usage

```bash
# First run (onboarding + sync)
cargo run --release

# Normal sync (reuses existing accounts)
cargo run --release

# TUI overlay (top tabs + mail + agent panel)
cargo run --release -- --tui

# Add another account
cargo run --release -- --add-account

# Skip sync, show cache only
cargo run --release -- --no-sync
```

## How It Works

- `SELECT (CONDSTORE)` to read `HIGHESTMODSEQ` and `UIDVALIDITY`.
- If MODSEQ unchanged and `--force` not set → skip.
- No baseline: `UID SEARCH SINCE <cutoff>` then fetch new UIDs.
- With baseline: `UID SEARCH SINCE <cutoff> MODSEQ <stored+1>`; fetch bodies for new UIDs and flags for existing.
- Update folder state and store sanitized content in SQLite.

## Architecture

See `architecture.md` for module layout, data flow, and schema details.

## TODO

See `TODO.md` for current work, planned features, and limitations.

## Database Location

- Linux/macOS: `~/otto/otto.db`
- Windows: `%USERPROFILE%\otto\otto.db`

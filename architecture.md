# Otto Architecture (Lean)

Otto syncs Gmail over IMAP into a local SQLite cache. Each run authorizes with OAuth2, opens one IMAP connection per folder, and uses CONDSTORE/MODSEQ to skip work when nothing changed; otherwise it fetches only new UIDs and flag updates, parses messages in parallel, and writes them in batches.

On startup the CLI loads config and accounts from SQLite, optionally onboards a new account, and runs the sync engine unless `--no-sync`. When TUI mode is enabled, the interface launches immediately from the cached DB, starts a background sync (unless `--no-sync`), shows a top-bar spinner while syncing, and refreshes its message list from the updated cache once sync finishes.

## Components

- `src/cli.rs`: CLI flags (`--add-account`, `--no-sync`, `--force`).
- `src/app.rs`: Wiring; loads config/DB, onboarding, runs sync, prints preview.
- `src/oauth.rs` + `onboarding.rs`: OAuth2 PKCE flow and account creation.
- `src/imap/mod.rs`: IMAP client setup with XOAUTH2 over Rustls.
- `src/sync/mod.rs`: Sync engine with connection pool, MODSEQ-based incremental sync, fetch/update helpers.
- `src/sanitize/mod.rs`: MIME parsing, HTML→text, attachment detection, hashing; strips tracking params from URLs and unwraps common redirectors before rendering text.
- `src/storage/db.rs` + `ops.rs`: SQLite schema/migrations and CRUD helpers.
- `src/types.rs`: Shared structs (Account, MessageRecord, FolderState, etc.).
- `src/tui.rs`: TUI overlay (top tabs + mail list/detail + agent panel placeholder) driven from the SQLite cache with a spinner indicator while background sync runs.

## Sync Flow (per folder)

1. `SELECT (CONDSTORE)` → read `UIDVALIDITY`, `HIGHESTMODSEQ`, `UIDNEXT`.
2. If stored MODSEQ and `EXISTS` match current and `--force` is not set → skip.
3. If no MODSEQ baseline → `UID SEARCH SINCE <cutoff>` then fetch and store new UIDs.
4. Otherwise `UID SEARCH SINCE <cutoff> MODSEQ <stored+1>`:
   - Fetch bodies for unseen UIDs.
   - Fetch flags + labels for existing UIDs and update DB.
5. If `EXISTS` decreased (or scan is stale), run a periodic `UID SEARCH SINCE <cutoff>` to detect missing UIDs.
6. Update folder state (`highest_uid`, `highestmodseq`, counts, timestamps).
7. After all folders finish, purge missing UIDs from the DB (prevents “move” races).
8. Local dedupe pass removes pre-X-GM-MSGID duplicates by `raw_hash`.

## Data Model (SQLite)

- `accounts`: id, email, provider, cutoff date, poll interval, folder list.
- `folders`: per-folder state (`uidvalidity`, `highest_uid`, `highestmodseq`, counts, timestamps).
- `messages`: metadata keyed by stable message id (`X-GM-MSGID`), per-folder uid, flags/labels, hashes.
- `bodies`: raw RFC822, sanitized text, MIME summary, attachments JSON.

## Current Limitations

- QRESYNC/VANISHED is not implemented; expunges are handled via a periodic UID scan fallback.
- Folder membership is modeled as a single “current folder” per message; true multi-label membership isn’t represented yet.\*\*\*

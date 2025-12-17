# Otto

Otto is a Rust IMAP syncer for Gmail. It authorizes with OAuth2, opens one IMAP connection per folder, and keeps a local SQLite cache of messages and sanitized bodies. Each run skips work when MODSEQ/EXISTS match; otherwise it fetches only new UIDs and updates flags/labels, parsing messages in parallel and writing in batches.

On startup the CLI loads accounts from SQLite (or onboards one), runs the sync unless `--no-sync`, and prints a small inbox preview from the cache. Messages are keyed by stable Gmail ids (`X-GM-MSGID`) so moves avoid duplicates; expunges are handled via a periodic UID scan fallback (QRESYNC/VANISHED is not implemented).

## Features

- Incremental IMAP sync via CONDSTORE/MODSEQ
- Parallel folder syncing and MIME parsing
- Local SQLite cache with sanitized plaintext bodies
- OAuth2 with token refresh
- Batched database writes

## Quick Start

### Prerequisites

- Rust toolchain (1.70+)
- Google OAuth credentials ([get them here](https://console.cloud.google.com/apis/credentials))

### Setup

1. **Clone and build**:

```bash
git clone <repo>
cd Otto
cargo build --release
```

2. **Add Google OAuth credentials**:
   - Download OAuth client secret JSON from Google Cloud Console
   - Place it in the Otto directory as `client_secret_*.json`

3. **First run** (onboarding):

```bash
cargo run --release
```

- Opens browser for Google OAuth consent
- Prompts for folders to sync and cutoff date
- Performs initial sync

4. **Subsequent syncs**:

```bash
cargo run --release
```

- Displays latest 10 emails from cache
- Syncs new messages since last run

## Usage

```bash
# Normal sync (default)
cargo run --release

# Add another account
cargo run --release -- --add-account

# Skip sync, show cache only
cargo run --release -- --no-sync

# With debug logging
RUST_LOG=debug cargo run --release
```

## How It Works

- `SELECT (CONDSTORE)` to read `HIGHESTMODSEQ` and `UIDVALIDITY`.
- If MODSEQ unchanged and `--force` not set → skip.
- No baseline: `UID SEARCH SINCE <cutoff>` then fetch new UIDs.
- With baseline: `UID SEARCH SINCE <cutoff> MODSEQ <stored+1>`; fetch bodies for new UIDs and flags for existing.
- Update folder state and store sanitized content in SQLite.

### Performance

- **No changes**: ~300-600ms (just checks MODSEQ, skips sync)
- **With 50 new messages**: ~2-5 seconds (fetch + parse + store)

## Architecture

See [architecture.md](architecture.md) for detailed documentation including:

- Module breakdown and function descriptions
- Database schema with examples
- Data flow diagrams
- Design decisions and trade-offs

## TODO

See [TODO.md](TODO.md) for:

- Outstanding issues and bugs
- Planned features (IDLE, attachment extraction, TUI)
- Performance improvements
- Future enhancements

## Database Location

- Linux/macOS: `~/otto/otto.db`
- Windows: `%USERPROFILE%\otto\otto.db`

## Tech Stack

- **Language**: Rust (async with Tokio)
- **IMAP**: `async-imap` with TLS (`tokio-rustls`)
- **Database**: SQLite (`sqlx`)
- **Auth**: `oauth2` + OS keyring
- **Parsing**: `mailparse` + `rayon` for parallelism
- **Sanitization**: `html2text` for HTML→text conversion

## Project Structure

```
Otto/
├── src/
│   ├── main.rs           # Entry point
│   ├── app.rs            # Command routing
│   ├── cli.rs            # CLI arguments
│   ├── oauth.rs          # OAuth flow
│   ├── onboarding.rs     # Account setup
│   ├── imap/             # IMAP connection
│   ├── sync/             # Sync engine
│   ├── sanitize/         # MIME parsing
│   └── storage/          # SQLite database
├── architecture.md       # Detailed documentation
├── TODO.md              # Outstanding tasks
└── README.md            # This file
```

## License

MIT

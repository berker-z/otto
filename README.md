# Otto

A high-performance IMAP email client that syncs Gmail to a local SQLite cache with intelligent change detection.

## Features

- **Incremental Sync**: Uses IMAP CONDSTORE/MODSEQ to detect changes efficiently (like Gmail History API)
- **Parallel Processing**: Syncs folders concurrently and parses messages in parallel
- **Smart Caching**: Stores sanitized plaintext bodies locally for instant access
- **OAuth2**: Secure Google authentication with automatic token refresh
- **Batch Operations**: Transaction-based database writes for optimal performance

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

Otto uses IMAP with CONDSTORE extension to track changes:

1. **SELECT folder** - Gets current MODSEQ (modification sequence number)
2. **Compare MODSEQ** - If unchanged since last sync → skip entirely ✅
3. **Incremental fetch** - If changed → only fetch new/modified messages
4. **Parallel parse** - Uses all CPU cores to parse MIME and extract plaintext
5. **Batch write** - Single database transaction for all messages

Otto avoids full UID scans after initial seeding. On Gmail, moves between synced folders are
tracked via `X-GM-MSGID` (stable id) + per-folder MODSEQ changes, so a move updates the existing
row instead of creating duplicates. True expunges that don’t reappear in another synced folder
require QRESYNC/VANISHED (not implemented yet).

If you previously synced before `X-GM-MSGID` was stored, Otto performs a local-only dedupe pass
based on `raw_hash` to remove legacy `account:folder:uid` duplicates.

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

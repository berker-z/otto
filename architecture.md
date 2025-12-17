# Otto Architecture Documentation

## Project Overview

**Otto** is a high-performance IMAP-based email client that syncs Gmail accounts to a local SQLite cache with sanitized plaintext message bodies. It uses IMAP with XOAUTH2 authentication, leveraging CONDSTORE/MODSEQ for efficient incremental syncing.

### Core Features

- **Incremental IMAP Sync**: Uses CONDSTORE/MODSEQ to detect changes efficiently
- **Parallel Processing**: Multi-folder syncing with parallel MIME parsing (Rayon)
- **Local Cache**: SQLite database storing sanitized plaintext bodies
- **OAuth2 Authentication**: Secure Google OAuth with token refresh
- **Smart Change Detection**: Avoids unnecessary work when no changes detected

### Technology Stack

- **Language**: Rust (async/await with Tokio)
- **IMAP**: `async-imap` 0.11 with `tokio-rustls` for TLS
- **Database**: SQLite via `sqlx`
- **Parsing**: `mailparse` for MIME, `html2text` for HTML→text conversion
- **Concurrency**: Tokio for async I/O, Rayon for CPU-bound parallelism
- **Auth**: `oauth2` crate with PKCE flow

---

## Directory Structure

```
Otto/
├── src/
│   ├── main.rs                 # Entry point, CLI parsing
│   ├── lib.rs                  # Library root, module exports
│   ├── app.rs                  # Application lifecycle, command routing
│   ├── cli.rs                  # CLI argument definitions (clap)
│   ├── errors.rs               # Custom error types
│   ├── types.rs                # Core data structures (Account, Message, etc.)
│   ├── oauth.rs                # OAuth2 flow, token management
│   ├── onboarding.rs           # New account setup wizard
│   │
│   ├── config/                 # Configuration management
│   │   └── mod.rs              # Load/save config.toml
│   │
│   ├── imap/                   # IMAP connection layer
│   │   └── mod.rs              # TLS setup, XOAUTH2 authentication
│   │
│   ├── sync/                   # Core sync engine
│   │   └── mod.rs              # Incremental sync logic, CONDSTORE handling
│   │
│   ├── sanitize/               # MIME parsing & sanitization
│   │   └── mod.rs              # Extract plaintext, HTML conversion, hashing
│   │
│   ├── storage/                # Database layer
│   │   └── mod.rs (db.rs)      # SQLite operations, schema migrations
│   │
│   ├── api/                    # Future: REST API
│   └── processing/             # Future: Background processing
│
├── Cargo.toml                  # Dependencies
├── config.toml                 # User configuration
├── client_secret_*.json        # Google OAuth credentials
└── docs/                       # Additional documentation
```

---

## Module Breakdown

### `src/main.rs`

**Purpose**: Application entry point

```rust
#[tokio::main]
async fn main() -> Result<()>
```

- Initializes tracing/logging
- Parses CLI arguments via `clap`
- Delegates to `app::run()`

---

### `src/app.rs`

**Purpose**: Command routing and application lifecycle

**Key Functions**:

- `pub async fn run(cli: Cli) -> Result<()>`
  - Routes commands: `sync`, `onboard`, `list`, etc.
  - Initializes database connection
  - Loads accounts from DB

---

### `src/cli.rs`

**Purpose**: CLI argument definitions using `clap`

**Commands**:

- `sync` - Sync all accounts or specific account
- `onboard` - Add new account
- `list` - List accounts
- `messages` - List messages

---

### `src/types.rs`

**Purpose**: Core data structures

**Types**:

```rust
pub struct Account {
    pub id: String,
    pub email: String,
    pub provider: Provider,
    pub settings: AccountSettings,
    pub created_at: i64,
    pub updated_at: i64,
}

pub struct AccountSettings {
    pub folders: Vec<String>,
    pub cutoff_since: NaiveDate,        // Only sync messages after this date
    pub poll_interval_minutes: u32,
    pub prefetch_recent: u32,
    pub safe_mode: bool,
}

pub struct MessageRecord {
    pub id: String,                     // Gmail message ID (X-GM-MSGID)
    pub account_id: String,
    pub folder: String,
    pub uid: Option<u32>,               // IMAP UID
    pub thread_id: Option<String>,      // Gmail thread ID
    pub internal_date: Option<i64>,     // Server timestamp
    pub subject: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub cc: Option<String>,
    pub bcc: Option<String>,
    pub flags: Vec<String>,             // IMAP flags (Seen, Flagged, etc.)
    pub labels: Vec<String>,            // Gmail labels
    pub has_attachments: bool,
    pub size_bytes: Option<u32>,
    pub raw_hash: Option<String>,       // Hash of raw RFC822 for change detection
    pub created_at: i64,
    pub updated_at: i64,
}

pub struct BodyRecord {
    pub message_id: String,
    pub raw_rfc822: Option<Vec<u8>>,    // Original RFC822 message (optional)
    pub sanitized_text: Option<String>,  // Plaintext extracted at sync time
    pub mime_summary: Option<String>,
    pub attachments_json: Option<String>,
    pub sanitized_at: Option<i64>,
}

pub struct FolderState {
    pub id: i64,
    pub account_id: String,
    pub name: String,
    pub uidvalidity: Option<u32>,       // IMAP UIDVALIDITY
    pub highest_uid: Option<u32>,       // Highest UID we've synced
    pub highestmodseq: Option<u64>,     // CONDSTORE MODSEQ value
    pub last_sync_ts: Option<i64>,
}

pub enum Provider {
    GmailImap,
}
```

---

### `src/oauth.rs`

**Purpose**: OAuth2 authentication flow with Google

**Key Functions**:

- `pub async fn authorize_with_scopes(scopes: &[Scope], token_key: &str) -> AppResult<TokenBundle>`
  - Checks keyring for existing refresh token
  - If found, attempts refresh
  - If not found or refresh fails, initiates OAuth flow
  - Opens browser for user consent
  - Listens on localhost callback
  - Exchanges code for tokens
  - Stores refresh token in OS keyring

- `async fn try_refresh(client: &BasicClient, refresh: String) -> AppResult<Option<TokenBundle>>`
  - Attempts to refresh access token using refresh token

**Data Structures**:

```rust
pub struct TokenBundle {
    pub access_token: String,
    pub expires_at: Option<DateTime<Utc>>,
    pub refresh_token: Option<String>,
}
```

**Token Storage**: Uses OS keyring via `keyring` crate

- Service name: `"otto-google-oauth"`
- Key format: `{account_id}`

---

### `src/imap/mod.rs`

**Purpose**: IMAP connection with TLS and XOAUTH2 authentication

**Key Functions**:

- `pub async fn connect(account: &Account, access_token: &str) -> Result<Session<...>>`
  - Establishes TCP connection to `imap.gmail.com:993`
  - Upgrades to TLS using `tokio-rustls`
  - Loads native root certificates
  - Authenticates with XOAUTH2
  - Returns authenticated IMAP session

**XOAUTH2 Implementation**:

```rust
struct Xoauth2 {
    user: String,
    access_token: String,
}

impl Authenticator for Xoauth2 {
    fn process(&mut self, _challenge: &[u8]) -> String {
        format!("user={}\x01auth=Bearer {}\x01\x01", self.user, self.access_token)
    }
}
```

---

### `src/sync/mod.rs`

**Purpose**: Core sync engine with incremental IMAP sync

**Key Type**:

```rust
pub struct SyncEngine {
    db: Arc<Database>,
}

type ImapSession = async_imap::Session<Compat<tokio_rustls::client::TlsStream<TcpStream>>>;
```

**Key Functions**:

#### `pub async fn sync_all(&self, accounts: &[Account]) -> Result<()>`

- Entry point for syncing all accounts
- Iterates over accounts sequentially
- Calls `sync_account()` for each

#### `async fn sync_account(&self, account: &Account) -> Result<()>`

- Obtains OAuth access token
- Establishes IMAP connection
- Iterates over configured folders
- Calls `sync_folder()` for each

#### `async fn sync_folder(&self, session: &mut ImapSession, account: &Account, folder_name: &str) -> Result<()>`

**Core incremental sync logic**:

1. **SELECT folder** with CONDSTORE support
2. **Load folder state** from DB (UIDVALIDITY, highest_uid, highestmodseq)
3. **Check UIDVALIDITY** - if changed, folder was rebuilt (handle reset)
4. **Build search query**:
   - **CONDSTORE path** (most efficient): `MODSEQ {stored+1} SINCE {cutoff}`
     - If stored MODSEQ == current MODSEQ → **skip sync entirely** (no changes)
   - **UID-based fallback**: `UID {highest_uid+1}:* SINCE {cutoff}`
   - **Full sync**: `SINCE {cutoff}`
5. **UID SEARCH** to get matching messages
6. **Compare local vs remote UIDs**:
   - `new_uids`: UIDs on server but not in DB
   - `existing_uids`: UIDs on both server and DB
   - `deleted_uids`: UIDs in DB but not on server
7. **Fetch new messages**: `fetch_and_store_new_messages()`
8. **Delete removed messages** from DB
9. **Update folder state** with new highest_uid and highestmodseq

#### `async fn fetch_and_store_new_messages(&self, session: &mut ImapSession, account: &Account, folder_name: &str, uids: &[u32]) -> Result<()>`

**Parallel fetch, parse, sanitize pipeline**:

1. **Batch UIDs** (chunks of 50)
2. **UID FETCH**: `UID FLAGS INTERNALDATE RFC822.SIZE BODY.PEEK[] ENVELOPE X-GM-MSGID X-GM-THRID X-GM-LABELS`
3. **Collect raw fetches** (fast - just memory copies)
4. **Parallel parse & sanitize** (CPU-intensive):
   ```rust
   tokio::task::spawn_blocking(move || {
       use rayon::prelude::*;
       raw_fetches
           .into_par_iter()
           .map(|(uid, body, ...)| {
               // Parse MIME
               let parsed = mailparse::parse_mail(&body)?;
               // Sanitize
               let sanitized = sanitize_message(&parsed, &body);
               // Build records
               Ok((message_record, body_record))
           })
           .collect()
   }).await?
   ```
5. **Batch write** to SQLite (single transaction):
   ```rust
   db.batch_upsert_messages_with_bodies(&messages_batch, &bodies_batch).await?;
   ```

**Timing Phases**:

- OAuth: ~200ms
- TLS handshake: ~800ms (per new connection)
- SELECT + MODSEQ: Variable (server-side processing)
- UID SEARCH: ~50-200ms
- UID FETCH: ~200-500ms (depends on batch size)
- Parallel parse: ~100-300ms (depends on message count & size)
- DB write: ~50-150ms (transaction with batching)

**Helper Functions**:

- `fn build_uid_sequence(uids: &[u32]) -> String` - Format UID set (e.g., "1,5,7:10")
- `fn extract_gm_msgid(fetch: &Fetch) -> Option<String>` - Extract Gmail message ID
- `fn extract_gm_thrid(fetch: &Fetch) -> Option<String>` - Extract Gmail thread ID
- `fn extract_gm_labels(fetch: &Fetch) -> Vec<String>` - Extract Gmail labels

---

### `src/sanitize/mod.rs`

**Purpose**: MIME parsing and text extraction (runs at sync time)

**Key Functions**:

#### `pub fn sanitize_message(parsed: &ParsedMail, raw_bytes: &[u8]) -> SanitizedBody`

**Public wrapper for sync module**:

- Calls `sanitize()` internally
- If parsing fails, returns fallback: converts entire raw message to lossy UTF-8 string
- Never panics - always returns a result

#### `fn sanitize(parsed: &ParsedMail, raw_bytes: &[u8]) -> Result<SanitizedBody>`

**Core sanitization logic**:

1. Extract plaintext from MIME structure via `extract_text()`
2. Compute hash of raw message for change detection
3. Detect if message has attachments (boolean only)
4. Returns:
   - `sanitized_text` - Extracted plaintext
   - `raw_hash` - Hash of original RFC822 message
   - `has_attachments` - Boolean flag
   - `mime_summary` - **NOT IMPLEMENTED** (always `None`)
   - `attachments_json` - **NOT IMPLEMENTED** (always `None`)

#### `fn extract_text(parsed: &ParsedMail, raw_bytes: &[u8]) -> String`

**Text extraction decision tree** (executed in order):

**STEP 1**: Check if single-part message (no subparts)

- If `Content-Type: text/plain` → Return decoded body directly
- If `Content-Type: text/html` → Convert HTML to plaintext via `html_to_text()`

**STEP 2**: Multipart message - **prefer `text/plain`**

- Loop through all MIME parts
- If any part is `text/plain` → Return that part's decoded body
- This handles `multipart/alternative` emails (HTML + plain versions)

**STEP 3**: No `text/plain` found - fallback to first subpart

- If first part is `text/html` → Convert to plaintext
- Otherwise → Return first part as lossy UTF-8 string

**STEP 4**: Last resort (shouldn't happen normally)

- Return entire raw message as lossy UTF-8 string

#### `fn html_to_text(html: &[u8]) -> String`

- Uses `html2text` crate to convert HTML → plaintext
- Line width: 80 characters
- Strips HTML tags, converts `<p>` to newlines, etc.
- Example: `<p>Hello <b>World</b></p>` → `"Hello World"`

#### `fn compute_hash(data: &[u8]) -> String`

- Hashes the **entire raw RFC822 message** bytes
- Uses Rust's `DefaultHasher` (fast, non-cryptographic)
- Returns hex string (e.g., `"a3f5b2c8d1e9f4a7"`)
- **Purpose**: Detect if message content changed on server

#### `fn detect_attachments(parsed: &ParsedMail) -> bool`

- Loops through all MIME parts
- Returns `true` if any part has `Content-Disposition: attachment`
- **Does NOT extract metadata** (filename, size, MIME type) - that's a TODO

#### `pub fn build_body_record(message_id: &str, raw: Option<Vec<u8>>, sanitized: SanitizedBody) -> BodyRecord`

- Constructs `BodyRecord` from sanitized result
- **Stores BOTH raw RFC822 and sanitized text** in database
- Sets `sanitized_at` timestamp
- Called from sync engine with `Some(body)` - we DO store the raw message

**Data Structures**:

```rust
pub struct SanitizedBody {
    pub sanitized_text: String,
    pub mime_summary: Option<String>,
    pub attachments_json: Option<String>,
    pub raw_hash: String,
    pub has_attachments: bool,
}
```

---

### `src/storage/db.rs`

**Purpose**: SQLite database operations

**Key Functions**:

#### `pub async fn new_default() -> Result<Self>`

- Creates database at `~/otto/otto.db`
- Runs migrations
- Enables foreign keys

#### `async fn migrate(&self) -> Result<()>`

- Creates tables if not exist
- Adds indexes
- Handles schema evolution (e.g., adding `highestmodseq` column)

#### `pub async fn save_account(&self, account: &Account) -> Result<()>`

- Upserts account record
- Serializes settings to JSON

#### `pub async fn load_accounts(&self) -> Result<Vec<Account>>`

- Loads all accounts from DB
- Deserializes settings

#### `pub async fn list_folders(&self, account_id: &str) -> Result<Vec<FolderState>>`

- Loads folder states for an account

#### `pub async fn upsert_folder_state(&self, account_id: &str, name: &str, uidvalidity: Option<u32>, highest_uid: Option<u32>, highestmodseq: Option<u64>, last_sync_ts: Option<i64>) -> Result<()>`

- Updates folder sync state
- Used after each folder sync

#### `pub async fn batch_upsert_messages_with_bodies(&self, messages: &[MessageRecord], bodies: &[BodyRecord]) -> Result<()>`

**Critical performance optimization**:

- Single transaction for entire batch
- Upserts messages first
- Upserts bodies second
- Dramatically faster than individual inserts

#### `pub async fn load_messages(&self, account_id: &str, limit: usize) -> Result<Vec<(MessageRecord, Option<BodyRecord>)>>`

- Loads messages with bodies
- Ordered by `internal_date DESC`
- Returns pre-sanitized text (no parsing at runtime)

#### `pub async fn load_messages_by_folder(&self, account_id: &str, folder: &str, limit: usize) -> Result<Vec<MessageRecord>>`

- Loads messages for specific folder (metadata only)
- Used during sync to compare UIDs

#### `pub async fn delete_message(&self, message_id: &str) -> Result<()>`

- Deletes message and body (cascade via foreign key)

---

### `src/onboarding.rs`

**Purpose**: Interactive account setup wizard

**Key Functions**:

- `pub async fn run(db: Arc<Database>) -> Result<()>`
  - Prompts for email
  - Runs OAuth flow
  - Prompts for folders and cutoff date
  - Saves account to DB

---

## Database Schema

### Table: `accounts`

Stores account configuration and settings.

```sql
CREATE TABLE accounts (
    id TEXT PRIMARY KEY,                    -- Account identifier (e.g., email)
    email TEXT NOT NULL,                    -- User's email address
    provider TEXT NOT NULL,                 -- "GmailImap"
    cutoff_since TEXT NOT NULL,             -- ISO date (YYYY-MM-DD)
    poll_interval_minutes INTEGER NOT NULL,
    prefetch_recent INTEGER NOT NULL,
    safe_mode INTEGER NOT NULL,             -- Boolean (0 or 1)
    folders TEXT NOT NULL,                  -- JSON array of folder names
    created_at INTEGER NOT NULL,            -- Unix timestamp
    updated_at INTEGER NOT NULL
);
```

**Example Row**:

```json
{
  "id": "user@gmail.com",
  "email": "user@gmail.com",
  "provider": "GmailImap",
  "cutoff_since": "2024-12-01",
  "poll_interval_minutes": 5,
  "prefetch_recent": 100,
  "safe_mode": 0,
  "folders": "[\"INBOX\",\"[Gmail]/Sent Mail\",\"[Gmail]/Trash\"]",
  "created_at": 1733097600,
  "updated_at": 1733097600
}
```

---

### Table: `folders`

Tracks sync state per folder (UIDVALIDITY, highest UID, MODSEQ).

```sql
CREATE TABLE folders (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id TEXT NOT NULL,               -- Foreign key to accounts(id)
    name TEXT NOT NULL,                     -- Folder name (e.g., "INBOX")
    uidvalidity INTEGER,                    -- IMAP UIDVALIDITY
    highest_uid INTEGER,                    -- Highest UID we've synced
    highestmodseq INTEGER,                  -- CONDSTORE MODSEQ value
    last_sync_ts INTEGER,                   -- Unix timestamp of last sync
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE(account_id, name),
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
);
CREATE INDEX idx_folders_account ON folders(account_id);
```

**Example Row**:

```json
{
  "id": 1,
  "account_id": "user@gmail.com",
  "name": "INBOX",
  "uidvalidity": 1234567890,
  "highest_uid": 5432,
  "highestmodseq": 987654321,
  "last_sync_ts": 1733097600,
  "created_at": 1733000000,
  "updated_at": 1733097600
}
```

**Key Fields**:

- `uidvalidity`: If this changes, all UIDs are invalid (folder was rebuilt)
- `highest_uid`: Used for incremental UID-based sync (`UID {highest_uid+1}:*`)
- `highestmodseq`: CONDSTORE value - if unchanged, skip sync entirely

---

### Table: `messages`

Stores message metadata (envelope, flags, labels).

```sql
CREATE TABLE messages (
    id TEXT PRIMARY KEY,                    -- Gmail message ID (X-GM-MSGID)
    account_id TEXT NOT NULL,
    folder TEXT NOT NULL,
    uid INTEGER,                            -- IMAP UID (per-folder)
    thread_id TEXT,                         -- Gmail thread ID (X-GM-THRID)
    internal_date INTEGER,                  -- Server timestamp (Unix)
    subject TEXT,
    from_addr TEXT,
    to_addrs TEXT,
    cc_addrs TEXT,
    bcc_addrs TEXT,
    flags TEXT,                             -- JSON array: ["\\Seen", "\\Flagged"]
    labels TEXT,                            -- JSON array: ["INBOX", "IMPORTANT"]
    has_attachments INTEGER NOT NULL DEFAULT 0,
    size_bytes INTEGER,
    raw_hash TEXT,                          -- Hash for change detection
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
);
CREATE INDEX idx_messages_account_folder ON messages(account_id, folder);
CREATE INDEX idx_messages_internal_date ON messages(account_id, internal_date DESC);
```

**Example Row**:

```json
{
  "id": "user@gmail.com:INBOX:5432",
  "account_id": "user@gmail.com",
  "folder": "INBOX",
  "uid": 5432,
  "thread_id": "18c5b3a9e7f4d2b1",
  "internal_date": 1733097600,
  "subject": "Meeting Tomorrow",
  "from_addr": "alice@example.com",
  "to_addrs": "user@gmail.com",
  "cc_addrs": null,
  "bcc_addrs": null,
  "flags": "[\"\\\\Seen\"]",
  "labels": "[\"INBOX\",\"IMPORTANT\"]",
  "has_attachments": 1,
  "size_bytes": 45678,
  "raw_hash": "a3f5b2c8d1e9f4a7",
  "created_at": 1733097600,
  "updated_at": 1733097600
}
```

---

### Table: `bodies`

Stores sanitized message bodies (plaintext extracted at sync time).

```sql
CREATE TABLE bodies (
    message_id TEXT PRIMARY KEY,            -- Foreign key to messages(id)
    raw_rfc822 BLOB,                        -- Original RFC822 (optional)
    sanitized_text TEXT,                    -- Plaintext (parsed at sync time)
    mime_summary TEXT,                      -- MIME structure summary (future)
    attachments_json TEXT,                  -- Attachment metadata (future)
    sanitized_at INTEGER,                   -- Unix timestamp
    FOREIGN KEY (message_id) REFERENCES messages(id) ON DELETE CASCADE
);
```

**Example Row**:

```json
{
  "message_id": "user@gmail.com:INBOX:5432",
  "raw_rfc822": "<binary blob: full RFC822 message, 5-500 KB typical>",
  "sanitized_text": "Hi,\n\nLet's meet tomorrow at 2pm in the conference room.\n\nBest,\nAlice",
  "mime_summary": null,
  "attachments_json": null,
  "sanitized_at": 1733097600
}
```

**Key Points**:

- `raw_rfc822` - **We DO store the complete original message** (BLOB)
- `sanitized_text` - **Plaintext extracted at sync time** (instant viewing at runtime)
- `mime_summary` - **NOT IMPLEMENTED** (always `null`)
- `attachments_json` - **NOT IMPLEMENTED** (always `null`, only boolean flag in messages table)

**What Sanitization Currently Does**:

- ✅ Extracts plaintext from MIME structure (prefers `text/plain` over `text/html`)
- ✅ Converts HTML to plaintext if no plain version available
- ✅ Computes hash of raw message for change detection
- ✅ Detects attachments (boolean flag only, no metadata extraction)
- ✅ Stores BOTH raw RFC822 and sanitized text in database

**What Sanitization Does NOT Do** (Future TODOs):

- ❌ Extract attachment metadata (filename, size, MIME type)
- ❌ Generate MIME structure summary
- ❌ Strip tracking pixels or sanitize dangerous content
- ❌ Parse inline images or embedded content

---

## Data Flow

### Sync Flow (Incremental)

```
1. User runs: otto sync
2. app.rs routes to SyncEngine::sync_all()
3. For each account:
   a. oauth.rs obtains access token (refresh if possible, ~50ms)
   b. imap/mod.rs establishes TLS connection (~800ms first time)
   c. For each folder (can be parallelized):
      i.   SELECT folder with CONDSTORE
      ii.  Load folder state from DB
      iii. Compare MODSEQ:
           - If stored == current → SKIP SYNC ✅
           - If stored < current → incremental search
      iv.  Build search query (MODSEQ or UID-based)
      v.   UID SEARCH for matching messages
      vi.  Compare local vs remote UIDs
      vii. Fetch new messages in batches of 50:
           - UID FETCH (metadata + bodies)
           - Parallel parse (Rayon): mailparse::parse_mail()
           - Parallel sanitize (Rayon): extract plaintext
           - Batch DB write (single transaction)
      viii. Delete removed messages
      ix.  Update folder state (highest_uid, highestmodseq)
4. Done
```

### OAuth Flow (First Time)

```
1. User runs: otto onboard
2. onboarding.rs prompts for email
3. oauth.rs checks keyring for refresh token
4. If not found:
   a. Generate PKCE challenge
   b. Open browser to Google consent screen
   c. Listen on localhost:PORT
   d. User authorizes
   e. Receive callback with authorization code
   f. Exchange code for tokens
   g. Store refresh token in OS keyring
5. Save account to DB
```

### Message Display Flow (Runtime)

```
1. User requests messages
2. db.rs executes:
   SELECT messages.*, bodies.sanitized_text
   FROM messages
   LEFT JOIN bodies ON messages.id = bodies.message_id
   WHERE account_id = ?
   ORDER BY internal_date DESC
3. Return pre-sanitized text (instant, no parsing)
```

---

## Key Design Decisions

### 1. **MIME Parsing & Sanitization at Sync Time, Not Runtime**

- **Why**: Parsing is CPU-intensive (10-30ms per message, 100-300ms for batch of 50)
- **What we store**: BOTH raw RFC822 bytes AND sanitized plaintext
- **Benefit**: Instant message viewing, fast search, ability to re-parse if needed
- **Trade-off**: Larger DB size (~2x storage, but SQLite compresses well)
- **Sanitization process**: Prefers `text/plain`, converts HTML→text, computes hash

### 2. **CONDSTORE/MODSEQ for Change Detection**

- **Why**: Folder-level change tracking (like Gmail History API)
- **Benefit**: Skip sync entirely when no changes (MODSEQ match)
- **Fallback**: UID-based incremental sync if CONDSTORE not supported

### 3. **Parallel Processing**

- **Network I/O**: Can parallelize folder syncs (each gets own connection)
- **CPU-bound**: Rayon parallelizes MIME parsing across messages
- **Database**: Batch writes in single transaction (50-150ms vs 2-5s for individual)

### 4. **Batch Size: 50 messages**

- **Why**: Balance between memory usage and network efficiency
- **Observations**: 50 messages ≈ 5-15 MB typical, fetches in 200-500ms

### 5. **SQLite for Local Cache**

- **Why**: Embedded, serverless, fast queries
- **Indexes**: `(account_id, folder)` and `(account_id, internal_date DESC)` for common queries
- **Foreign Keys**: CASCADE deletes for data integrity

### 6. **OAuth Token Storage in OS Keyring**

- **Why**: More secure than plaintext files
- **Benefit**: Persistent refresh tokens across runs
- **Library**: `keyring` crate (uses macOS Keychain, Windows Credential Manager, Linux Secret Service)

---

## Performance Characteristics

### Typical Sync Times (No Changes)

- OAuth token refresh: ~50-200ms
- Per-folder SELECT + MODSEQ check: ~100-300ms
- If MODSEQ matches: **SKIP** (0 additional time)
- **Total**: ~300-600ms for 4 folders with no changes

### Typical Sync Times (With Changes)

- OAuth + TLS: ~200-800ms (first connection)
- Per-folder (10 new messages):
  - SELECT + SEARCH: ~150-300ms
  - FETCH batch: ~200-500ms
  - Parallel parse: ~100-200ms
  - DB write: ~50-100ms
  - **Subtotal**: ~500-1100ms per folder
- **Total**: ~2-5 seconds for 4 folders with 40 new messages

### Bottlenecks (Identified)

1. **TLS handshake**: ~800ms (mitigated by connection reuse)
2. **Server-side SELECT processing**: ~100-500ms (Gmail's internal work)
3. **Network round-trips**: ~50-100ms per IMAP command
4. **MIME parsing**: ~10-30ms per message (parallelized with Rayon)

---

## Future Enhancements

### Planned Features

- [ ] **IMAP IDLE**: Push notifications for new messages (eliminate polling)
- [ ] **Connection Pool**: Reuse IMAP connections with NOOP keepalive
- [ ] **TLS Session Resumption**: Reduce handshake overhead
- [ ] **Multi-account TUI**: Browse, search, compose
- [ ] **Full-text search**: SQLite FTS5 on sanitized_text
- [ ] **Attachment extraction**: Parse and store attachment metadata
- [ ] **Flag/label updates**: Periodic refresh of existing messages
- [ ] **Trash/Archive actions**: Bidirectional sync (local → server)

### Optimization Opportunities

- [ ] Batch SELECT multiple folders (if IMAP server supports)
- [ ] Adaptive batch sizing based on message size
- [ ] Incremental DB writes (stream instead of collect)
- [ ] Parallel folder syncs with connection pool
- [ ] Compression for `raw_rfc822` BLOB

---

## Dependencies

**Core**:

- `tokio` - Async runtime
- `sqlx` - SQLite with async
- `async-imap` - IMAP client
- `oauth2` - OAuth2 client
- `mailparse` - MIME parsing
- `rayon` - Data parallelism

**Utilities**:

- `anyhow` / `thiserror` - Error handling
- `tracing` - Structured logging
- `clap` - CLI parsing
- `serde` / `serde_json` - Serialization
- `chrono` - Date/time
- `html2text` - HTML→text conversion
- `keyring` - OS credential storage

**Crypto/Network**:

- `tokio-rustls` - TLS for IMAP
- `rustls-native-certs` - Native root certificates
- `reqwest` - HTTP client (for OAuth)

---

## File Locations

- **Database**: `~/otto/otto.db` (or `$HOME/otto/otto.db`)
- **Config**: `./config.toml` (project root)
- **OAuth Credentials**: `./client_secret_*.json` (project root)
- **Token Storage**: OS Keyring (service: `"otto-google-oauth"`, key: `{account_id}`)

---

## Common Operations

### Add Account

```bash
otto onboard
```

### Sync All Accounts

```bash
otto sync
```

### List Messages

```bash
otto messages --account user@gmail.com --limit 50
```

### Check Database

```bash
sqlite3 ~/otto/otto.db "SELECT COUNT(*) FROM messages;"
```

---

## Troubleshooting

### OAuth Errors

- Delete keyring entry: `keyring delete otto-google-oauth {account_id}`
- Re-run `otto onboard`

### UIDVALIDITY Changed

- Indicates folder was rebuilt on server
- Current behavior: logs warning, continues with new UIDVALIDITY
- Future: Implement full resync or mark existing UIDs as invalid

### TLS Handshake Slow

- Normal on first connection (~800ms)
- Connection pooling (future)

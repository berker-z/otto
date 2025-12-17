# Otto TODO

## Current Status

✅ IMAP sync with CONDSTORE/MODSEQ incremental updates  
✅ OAuth2 authentication with token refresh  
✅ Parallel folder syncing  
✅ Parallel MIME parsing (Rayon)  
✅ SQLite caching with sanitized plaintext bodies  
✅ Batch database writes for performance

---

## Outstanding Issues

### 🐛 High Priority Bugs

- [x] **Fix MODSEQ search parsing** - Gmail returns `* SEARCH ... (MODSEQ <n>)`; patched `imap-proto` and re-enabled `UID SEARCH ... MODSEQ ...`
- [ ] **Track expunges without UID scans** - Requires QRESYNC/VANISHED support (RFC 7162) or Gmail-label-centric sync design

### 🔧 Missing Features

- [ ] **Attachment metadata extraction** - Currently only detects boolean flag
  - Extract filename, size, MIME type, Content-ID
  - Store in `attachments_json` field in bodies table
- [ ] **MIME structure summary** - `mime_summary` field is always NULL
  - Generate tree of MIME parts for debugging/inspection
- [ ] **Gmail-specific metadata** - X-GM-MSGID, X-GM-THRID, X-GM-LABELS extraction
  - Helper functions exist but return hardcoded None/empty
  - Need to parse IMAP FETCH response attributes

---

## Performance Improvements

### 🚀 High Impact

- [ ] **IMAP IDLE** - Push notifications instead of polling (BIGGEST WIN)
  - Eliminates SELECT/MODSEQ overhead on every sync
  - Instant notification of new messages
  - Keep connections alive with NOOP
- [ ] **TLS Session Resumption** - Reduce handshake time (~800ms → ~50ms)
  - Configure rustls to cache TLS sessions
  - Reuse across IMAP connections

- [ ] **Connection Pool** - Reuse IMAP connections instead of creating new ones
  - Current: Opens 4 connections per sync (one per folder)
  - Target: Persistent pool with keepalive

### 🔄 Medium Impact

- [ ] **Parallel account syncing** - Currently sequential, can parallelize
- [ ] **Adaptive batch sizing** - Adjust FETCH batch size based on message size
  - Currently fixed at 50 messages
  - Small messages: larger batches (100+)
  - Large messages: smaller batches (10-20)

- [ ] **Incremental DB writes** - Stream results instead of collecting entire batch
- [ ] **Compression for raw_rfc822** - BLOB storage uses a lot of space
  - Consider gzip compression for archived messages

---

## Feature Additions

### 📬 Sync Enhancements

- [ ] **Flag/label updates** - Currently skipped for performance
  - Implement periodic refresh (every N syncs or time-based)
  - On-demand flag sync command
- [ ] **Full resync on UIDVALIDITY change** - Currently just logs warning
  - Detect folder rebuild
  - Mark old UIDs invalid
  - Fetch all messages fresh

- [ ] **Configurable folder selection** - Currently hardcoded list
  - Let user choose which folders to sync
  - Per-account folder configuration

- [ ] **Sync statistics** - Track and display sync performance
  - Messages synced, bytes transferred, time taken
  - Historical stats in DB

### 🖥️ User Interface

- [ ] **TUI (Terminal UI)** - Interactive browser with ratatui
  - Folder tree view
  - Message list with search
  - Message detail view
  - Compose/reply/forward
- [ ] **Full-text search** - SQLite FTS5 on sanitized_text
- [ ] **--force flag** - Bypass MODSEQ checks and force full sync
- [ ] **--folder flag** - Sync specific folder only

### 🔐 Security & Safety

- [ ] **Sanitize dangerous content** - Currently just extracts text
  - Strip tracking pixels
  - Remove external resources from HTML
  - Scan for malicious attachments
- [ ] **Rate limiting** - Respect Gmail API quotas
- [ ] **Backup/export** - Export cache to mbox or other formats

### 📤 Write Operations (Future)

- [ ] **Mark as read/unread** - Bidirectional flag sync
- [ ] **Archive/trash/spam** - Move messages between folders
- [ ] **Compose and send** - Draft and send emails
- [ ] **Labels management** - Add/remove Gmail labels

---

## Multi-Provider Support

- [ ] **Provider abstraction** - Currently Gmail-only
  - Generic IMAP trait
  - Provider-specific extensions (X-GM-\* for Gmail)
- [ ] **Outlook/Office365 support**
- [ ] **Yahoo Mail support**
- [ ] **Generic IMAP support** (any server)

---

## Code Quality

- [ ] **Better error messages** - Current IMAP errors are opaque
  - Surface actual IMAP error responses
  - Provide actionable suggestions
- [ ] **Unit tests** - Currently minimal test coverage
  - Mock IMAP responses
  - Test incremental sync logic
  - Test sanitization edge cases
- [ ] **Integration tests** - E2E testing with test IMAP server
- [ ] **Documentation** - Function-level docs for public APIs

---

## Infrastructure

- [ ] **Logging levels** - Fine-tune debug/info/warn outputs
- [ ] **Metrics/telemetry** - Optional performance tracking
- [ ] **Config validation** - Better error messages for invalid config.toml
- [ ] **Migration system** - Handle DB schema changes gracefully

---

## Notes

### Known Limitations

- MODSEQ searches with SINCE clause fail on Gmail → using UID-based fallback
- Raw RFC822 bodies stored uncompressed → DB can get large
- No support for S/MIME or PGP → encrypted messages show as raw
- HTML→text conversion loses formatting → acceptable tradeoff

### Performance Targets

- Sync with no changes: <1 second (currently ~300-600ms ✅)
- Sync 50 new messages: <3 seconds (currently ~2-5s ✅)
- IDLE implementation: instant notification (<100ms)
- TLS resumption: <2 seconds for 4-folder sync

### Future Considerations

- Consider SQLite Write-Ahead Logging (WAL) for concurrent access
- Investigate JMAP protocol as Gmail alternative (more efficient than IMAP)
- Mobile companion app (read-only sync from Otto's DB)

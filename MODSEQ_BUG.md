# MODSEQ Search Bug

## The Bug

New/changed emails weren't showing up after sync, even though changes were made in Gmail.

## Root Cause

1. `SEARCH MODSEQ {value}` query was executed to find changed messages
2. Gmail returned valid response: `* SEARCH 53999 (MODSEQ 9387530)`
3. **`async-imap` library failed to parse the response** (doesn't handle `(MODSEQ value)` modifier)
4. Error was caught and swallowed, empty result set returned
5. **Stored MODSEQ was updated anyway** (to latest value from folder SELECT)
6. Next sync: MODSEQ matched → early-exit → changes never fetched

## Why It Was Hard to Catch

Initial assumption was "Gmail's MODSEQ hasn't updated yet" but the real issue was:

- We grabbed the latest MODSEQ from Gmail (via SELECT)
- We tried to fetch changes (MODSEQ search failed silently)
- We stored the new MODSEQ value
- **Result**: MODSEQ values match but data is stale

## The Fix

**Fixed MODSEQ searches and re-enabled incremental syncing.**

- Patched the underlying IMAP parser (`imap-proto`) to accept RFC 4551 SEARCH responses that
  include a trailing `(MODSEQ <value>)` modifier (as Gmail returns).
- Use `SELECT (CONDSTORE)` to get `HIGHESTMODSEQ`.
- If `HIGHESTMODSEQ` changed, run `UID SEARCH SINCE {date} MODSEQ {stored+1}` to get only changed
  UIDs and apply:
  - Fetch full bodies for new UIDs
  - Fetch flags for existing UIDs and update the DB
- On Gmail, messages are keyed by `X-GM-MSGID`, so moves between synced folders update the existing
  DB row (no duplicates) without needing UID-diff scans after seeding.
- True expunges that don’t reappear in another synced folder require QRESYNC/VANISHED (not
  implemented yet).

## RFC 4551 Compliance

Per RFC 4551 section 3.5, `SEARCH MODSEQ` responses SHOULD include the highest MODSEQ:

```
C: a SEARCH MODSEQ 620162338
S: * SEARCH 2 5 6 7 (MODSEQ 917162500)
```

Gmail is RFC-compliant. Upstream `async-imap`/`imap-proto` didn’t accept this response format, so
we patch `imap-proto` locally.

## Future Options

1. Fix `async-imap` to parse MODSEQ search responses
2. Switch to different IMAP library
3. Parse raw IMAP responses ourselves
4. Current workaround is acceptable for now

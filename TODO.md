# Otto TODO (Lean)

## Now

- Add QRESYNC/VANISHED support to avoid periodic UID scans (fallback exists).
- Decide and document folder semantics (single “current folder” vs multi-label membership).
- Add lightweight sync stats (counts + timings) to validate correctness/regressions.

## Next

- Add per-folder CLI options (sync subset, rebuild baseline for one folder).
- Harden OAuth/token storage UX during onboarding (better errors, validation).
- Evolve TUI from read-only viewer to interactive client (read/unread toggles, delete/archive, refresh).

## Later

- IMAP IDLE for push-style updates.
- TLS session resumption/connection pooling tuning for faster startups.
- Optional compression for stored RFC822 blobs.\*\*\*
- Bidirectional read/unread sync (when toggled in Otto, update IMAP flags).

## Done (Recent)

- UIDVALIDITY change now clears folder cache and rebuilds baseline.
- Expunge fallback now runs a periodic UID scan and purges missing UIDs after folder syncs complete.
- MIME summaries and attachment metadata are now populated; label refresh is included in flag updates.
- TUI now launches immediately from cached messages, shows a top-bar spinner during background sync, and refreshes from the DB when sync completes.

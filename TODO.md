# Otto TODO (Lean)

## Now

- Add QRESYNC/VANISHED support to avoid periodic UID scans (fallback exists).
- Decide and document folder semantics (single “current folder” vs multi-label membership).
- Add lightweight sync stats (counts + timings) to validate correctness/regressions.

## Next

- Add per-folder CLI options (sync subset, rebuild baseline for one folder).
- Harden OAuth/token storage UX during onboarding (better errors, validation).

## Later

- IMAP IDLE for push-style updates.
- TLS session resumption/connection pooling tuning for faster startups.
- Optional compression for stored RFC822 blobs.\*\*\*

## Done (Recent)

- UIDVALIDITY change now clears folder cache and rebuilds baseline.
- Expunge fallback now runs a periodic UID scan and purges missing UIDs after folder syncs complete.
- MIME summaries and attachment metadata are now populated; label refresh is included in flag updates.

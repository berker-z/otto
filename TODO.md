# Otto TODO (Lean)

## Now
- Track deletions: add QRESYNC/VANISHED support or a safe fallback to purge expunged UIDs.
- Handle UIDVALIDITY changes by forcing a full folder resync and clearing stale state.
- Finish Gmail metadata parsing (X-GM-LABELS, attachment metadata, MIME summaries) instead of placeholders.

## Next
- Add a `--force`/per-folder option to bypass MODSEQ skip and rebuild baselines.
- Surface sync stats (counts, timings) to help validate performance and regression-test changes.
- Harden OAuth/token storage UX during onboarding (better errors, validation).

## Later
- IMAP IDLE for push-style updates.
- TLS session resumption/connection pooling tuning for faster startups.
- Optional compression for stored RFC822 blobs.***

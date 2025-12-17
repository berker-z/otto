# Otto TODO (Lean)

## Now

- Add QRESYNC/VANISHED support to avoid periodic UID scans (fallback exists).
- Decide and document folder semantics (single “current folder” vs multi-label membership).
- Add lightweight sync stats (counts + timings) to validate correctness/regressions.
- Optional: expose a “copy/open raw link” fallback alongside cleaned URLs if stripping ever breaks a link.

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

- Per-folder sync commits now route all message/body inserts plus location + flag updates and folder_sync_state into one `commit_folder_batch` transaction (network/parse stays outside).
- UIDVALIDITY change now clears folder cache and rebuilds baseline.
- Expunge fallback now runs a periodic UID scan and purges missing UIDs after folder syncs complete.
- MIME summaries and attachment metadata are now populated; label refresh is included in flag updates.
- TUI now launches immediately from cached messages, shows a top-bar spinner during background sync, and refreshes from the DB when sync completes.

## Dependency Refresh (Backlog)

- Upgrade stacks in small batches with tests:
  - Rustls stack: tokio-rustls/rustls-native-certs bump.
  - sqlx 0.8 upgrade (macros/migrations).
  - keyring 3.x upgrade and re-test token storage.
  - mailparse 0.16 + base64 0.22 (MIME/sanitize check).
  - dirs 6 + toml 0.9 (config load check).
  - patch bumps like reqwest 0.12.26 can be taken quickly.
- Command to snapshot current gaps: `nix shell nixpkgs#cargo-outdated -c cargo outdated -R --manifest-path ./Cargo.toml` (same as `--depth 1`).

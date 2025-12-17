use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use futures::{StreamExt, future::join_all};
use oauth2::Scope;
use once_cell::sync::Lazy;
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio_util::compat::Compat;
use tracing::{debug, info, warn};

use crate::imap::ImapClient;
use crate::oauth::authorize_with_scopes;
use crate::sanitize::sanitize_message;
use crate::storage::Database;
use crate::types::{now_ts, Account, BodyRecord, MessageRecord};

type ImapSession = async_imap::Session<Compat<tokio_rustls::client::TlsStream<TcpStream>>>;

// Connection pool: cache IMAP connections to avoid TLS handshake overhead
struct ConnectionPool {
    connections: Mutex<HashMap<String, (ImapSession, Instant)>>,
}

impl ConnectionPool {
    fn new() -> Self {
        Self {
            connections: Mutex::new(HashMap::new()),
        }
    }

    async fn get_or_create(
        &self,
        key: String,
        account: &Account,
        access_token: &str,
    ) -> Result<ImapSession> {
        // Quick check for cached connection
        {
            let mut pool = self.connections.lock().await;
            if let Some((session, created_at)) = pool.remove(&key) {
                // Check if connection is still fresh (< 5 minutes old)
                if created_at.elapsed() < Duration::from_secs(300) {
                    debug!("Reusing cached IMAP connection for {}", key);
                    return Ok(session);
                } else {
                    debug!("Cached connection expired for {}", key);
                }
            }
        } // Release lock here!

        // Create new connection WITHOUT holding the lock (allows parallel creation)
        debug!("Creating new IMAP connection for {}", key);
        ImapClient::connect(account, access_token).await
    }

    async fn return_connection(&self, key: String, session: ImapSession) {
        let mut pool = self.connections.lock().await;
        pool.insert(key, (session, Instant::now()));
    }
}

static CONNECTION_POOL: Lazy<ConnectionPool> = Lazy::new(ConnectionPool::new);

pub struct SyncEngine {
    db: Arc<Database>,
}

impl SyncEngine {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    pub async fn sync_all(&self, accounts: &[Account], force: bool) -> Result<()> {
        for account in accounts {
            info!(account = %account.id, email = %account.email, "Starting IMAP sync");

            if let Err(e) = self.sync_account(account, force).await {
                warn!(account = %account.id, error = %e, "Account sync failed");
            }
        }
        Ok(())
    }

    async fn sync_account(&self, account: &Account, force: bool) -> Result<()> {
        let account_start = Instant::now();

        // Local-only cleanup to remove legacy duplicates created before we extracted X-GM-MSGID.
        // Keeps sync fast while letting existing DBs heal without a wipe.
        match self
            .db
            .dedupe_fallback_messages_by_raw_hash(&account.id, 500)
            .await
        {
            Ok(0) => {}
            Ok(n) => info!(account = %account.id, deleted = n, "Deduped legacy messages"),
            Err(e) => warn!(account = %account.id, error = %e, "Deduping legacy messages failed"),
        }

        // Get OAuth token (shared across all connections)
        let token_start = Instant::now();
        let scopes = vec![Scope::new("https://mail.google.com/".into())];
        let token = authorize_with_scopes(&scopes, &account.id).await?;
        info!(account = %account.id, elapsed_ms = ?token_start.elapsed().as_millis(), "OAuth token obtained");

        // Spawn parallel folder sync tasks (one IMAP connection per folder)
        let parallel_start = Instant::now();
        let sync_tasks: Vec<_> = account.settings.folders.iter()
            .map(|folder_name| {
                let db = Arc::clone(&self.db);
                let account = account.clone();
                let folder_name = folder_name.clone();
                let access_token = token.access_token.clone();
                let force = force;

                tokio::spawn(async move {
                    let folder_start = Instant::now();
                    info!(account = %account.id, folder = %folder_name, "Syncing folder (parallel)");

                    // Get connection from pool (or create new one)
                    let connect_start = Instant::now();
                    let pool_key = format!("{}:{}", account.id, folder_name);
                    let mut session = match CONNECTION_POOL.get_or_create(pool_key.clone(), &account, &access_token).await {
                        Ok(s) => s,
                        Err(e) => {
                            warn!(account = %account.id, folder = %folder_name, error = %e, "IMAP connection failed");
                            return Err(e);
                        }
                    };
                    debug!(account = %account.id, folder = %folder_name, elapsed_ms = ?connect_start.elapsed().as_millis(), "IMAP connection obtained");

                    // Sync the folder
                    let sync_engine = SyncEngine { db };
                    let result = sync_engine.sync_folder(&mut session, &account, &folder_name, force).await;

                    // Return connection to pool (don't logout!)
                    CONNECTION_POOL.return_connection(pool_key, session).await;

                    match result {
                        Ok(_) => {
                            info!(
                                account = %account.id,
                                folder = %folder_name,
                                elapsed_ms = ?folder_start.elapsed().as_millis(),
                                "Folder sync completed"
                            );
                            Ok(())
                        }
                        Err(e) => {
                            warn!(account = %account.id, folder = %folder_name, error = %e, "Folder sync failed");
                            Err(e)
                        }
                    }
                })
            })
            .collect();

        // Wait for all folders to complete (in parallel)
        let results = join_all(sync_tasks).await;

        // Check for errors
        let mut success_count = 0;
        let mut error_count = 0;
        for result in results {
            match result {
                Ok(Ok(())) => success_count += 1,
                Ok(Err(_)) => error_count += 1,
                Err(e) => {
                    warn!(account = %account.id, error = %e, "Folder sync task panicked");
                    error_count += 1;
                }
            }
        }

        info!(
            account = %account.id,
            total_elapsed_ms = ?account_start.elapsed().as_millis(),
            parallel_elapsed_ms = ?parallel_start.elapsed().as_millis(),
            success = success_count,
            errors = error_count,
            "Account sync completed (parallel)"
        );

        Ok(())
    }

    async fn sync_folder(
        &self,
        session: &mut ImapSession,
        account: &Account,
        folder_name: &str,
        force: bool,
    ) -> Result<()> {
        // Prefer SELECT (CONDSTORE) so we get HIGHESTMODSEQ. If the server doesn't support it,
        // fall back to a regular SELECT (UID-based sync will be used).
        let mailbox = match session.select_condstore(folder_name).await {
            Ok(mbox) => mbox,
            Err(e) => {
                warn!(
                    account = %account.id,
                    folder = %folder_name,
                    error = %e,
                    "SELECT (CONDSTORE) failed; falling back to SELECT"
                );
                session
                    .select(folder_name)
                    .await
                    .with_context(|| format!("selecting folder {}", folder_name))?
            }
        };

        let current_uidvalidity = mailbox.uid_validity.unwrap_or(0);
        let current_highestmodseq = mailbox.highest_modseq;
        let current_exists = mailbox.exists;
        let current_highest_uid = mailbox
            .uid_next
            .map(|next| next.saturating_sub(1))
            .filter(|uid| *uid > 0);

        // Log CONDSTORE support
        if let Some(modseq) = current_highestmodseq {
            debug!(
                account = %account.id,
                folder = %folder_name,
                highestmodseq = modseq,
                "CONDSTORE supported - can use efficient change tracking"
            );
        }

        // Load existing folder state from DB
        let folder_state = self
            .db
            .list_folders(&account.id)
            .await?
            .into_iter()
            .find(|f| f.name == folder_name);

        // Check UIDVALIDITY
        if let Some(ref state) = folder_state {
            if let Some(stored_uidvalidity) = state.uidvalidity {
                if stored_uidvalidity != current_uidvalidity {
                    warn!(
                        account = %account.id,
                        folder = %folder_name,
                        old_uidvalidity = stored_uidvalidity,
                        new_uidvalidity = current_uidvalidity,
                        "UIDVALIDITY changed, requiring full resync"
                    );
                    // UIDVALIDITY change means all UIDs are invalid - would need full resync
                    // For now, we'll just update the UIDVALIDITY and continue
                }
            }
        }

        // Build search criteria - use CONDSTORE MODSEQ for change detection
        let cutoff_str = account.settings.cutoff_since.format("%d-%b-%Y").to_string();

        // MODSEQ optimization: Early exit if nothing changed (unless force=true)
        if !force {
            if let Some(ref state) = folder_state {
                if let (Some(stored_modseq), Some(current_modseq)) = (state.highestmodseq, current_highestmodseq) {
                    if stored_modseq > 0 && current_modseq == stored_modseq {
                        // No changes at all - skip sync entirely
                        info!(
                            account = %account.id,
                            folder = %folder_name,
                            modseq = current_modseq,
                            "No changes detected (MODSEQ match) - skipping sync"
                        );
                        return Ok(());
                    }
                }
            }
        }

        let now = now_ts();

        // Incremental path: use MODSEQ search to fetch only changed UIDs.
        let stored_modseq = folder_state.as_ref().and_then(|s| s.highestmodseq).unwrap_or(0);
        let stored_highest_uid = folder_state.as_ref().and_then(|s| s.highest_uid).unwrap_or(0);

        if stored_modseq == 0 || current_highestmodseq.is_none() {
            // We don't have a usable MODSEQ baseline yet (or server didn't report it).
            // Fall back to a one-time full scan to establish state.
            warn!(
                account = %account.id,
                folder = %folder_name,
                stored_modseq = stored_modseq,
                current_highestmodseq = ?current_highestmodseq,
                "No MODSEQ baseline available; falling back to full scan"
            );

            let all_uids_query = format!("SINCE {}", cutoff_str);
            let uid_set = session
                .uid_search(&all_uids_query)
                .await
                .with_context(|| format!("UID SEARCH baseline: {}", all_uids_query))?;
            let remote_uids: HashSet<u32> = uid_set.iter().cloned().collect();

            let local_uid_map = self
                .db
                .load_uid_to_message_id_map_by_folder(&account.id, folder_name)
                .await?;
            let local_uids: HashSet<u32> = local_uid_map.keys().copied().collect();

            let new_uids: Vec<u32> = remote_uids
                .iter()
                .filter(|uid| !local_uids.contains(uid))
                .copied()
                .collect();

            if !new_uids.is_empty() {
                self.fetch_and_store_new_messages(session, account, folder_name, &new_uids)
                    .await?;
            }

            let highest_uid = current_highest_uid
                .or_else(|| remote_uids.iter().max().copied())
                .unwrap_or(stored_highest_uid);

            self.db
                .upsert_folder_state(
                    &account.id,
                    folder_name,
                    Some(current_uidvalidity),
                    Some(highest_uid),
                    current_highestmodseq,
                    Some(current_exists),
                    Some(now),
                    None,
                )
                .await?;

            return Ok(());
        }

        let modseq_query = format!("SINCE {} MODSEQ {}", cutoff_str, stored_modseq + 1);
        debug!(
            account = %account.id,
            folder = %folder_name,
            query = %modseq_query,
            "Incremental UID SEARCH via MODSEQ"
        );

        let search_start = Instant::now();
        let uid_set = session
            .uid_search(&modseq_query)
            .await
            .with_context(|| format!("UID SEARCH MODSEQ: {}", modseq_query))?;

        debug!(
            account = %account.id,
            folder = %folder_name,
            elapsed_ms = ?search_start.elapsed().as_millis(),
            "Incremental SEARCH completed"
        );

        let changed_uids: Vec<u32> = uid_set.iter().cloned().collect();
        if changed_uids.is_empty() {
            let highest_uid = current_highest_uid.unwrap_or(stored_highest_uid);
            self.db
                .upsert_folder_state(
                    &account.id,
                    folder_name,
                    Some(current_uidvalidity),
                    Some(highest_uid),
                    current_highestmodseq,
                    Some(current_exists),
                    Some(now),
                    folder_state.as_ref().and_then(|s| s.last_uid_scan_ts),
                )
                .await?;
            return Ok(());
        }

        let existing_by_uid = self
            .db
            .load_message_ids_by_uids(&account.id, folder_name, &changed_uids)
            .await?;

        let mut new_uids = Vec::new();
        let mut existing_uids = Vec::new();
        for uid in &changed_uids {
            if existing_by_uid.contains_key(uid) {
                existing_uids.push(*uid);
            } else {
                new_uids.push(*uid);
            }
        }

        info!(
            account = %account.id,
            folder = %folder_name,
            changed = changed_uids.len(),
            new = new_uids.len(),
            existing = existing_uids.len(),
            "Incremental UID diff computed"
        );

        if !new_uids.is_empty() {
            self.fetch_and_handle_new_uids(session, account, folder_name, &new_uids)
                .await?;
        }

        if !existing_uids.is_empty() {
            self.fetch_and_update_flags(session, account, folder_name, &existing_uids)
                .await?;
        }

        let highest_uid = current_highest_uid
            .or_else(|| changed_uids.iter().max().copied())
            .unwrap_or(stored_highest_uid);
        self.db
            .upsert_folder_state(
                &account.id,
                folder_name,
                Some(current_uidvalidity),
                Some(highest_uid),
                current_highestmodseq,
                Some(current_exists),
                Some(now),
                folder_state.as_ref().and_then(|s| s.last_uid_scan_ts),
            )
            .await?;

        Ok(())
    }

    async fn fetch_and_store_new_messages(
        &self,
        session: &mut ImapSession,
        account: &Account,
        folder_name: &str,
        uids: &[u32],
    ) -> Result<()> {
        // Limit batch size to avoid memory issues
        const BATCH_SIZE: usize = 50;

        for chunk in uids.chunks(BATCH_SIZE) {
            let batch_start = Instant::now();
            let uid_seq = Self::build_uid_sequence(chunk);

            debug!(
                account = %account.id,
                folder = %folder_name,
                uid_seq = %uid_seq,
                count = chunk.len(),
                "Fetching batch of new messages"
            );

            // Fetch metadata + bodies
            let fetch_query =
                "(UID FLAGS INTERNALDATE RFC822.SIZE BODY.PEEK[] ENVELOPE X-GM-MSGID X-GM-THRID X-GM-LABELS)";

            let fetch_start = Instant::now();
            let mut stream = session
                .uid_fetch(&uid_seq, fetch_query)
                .await
                .context("fetching message metadata and bodies")?;

            debug!(
                account = %account.id,
                folder = %folder_name,
                elapsed_ms = ?fetch_start.elapsed().as_millis(),
                "FETCH command completed, processing stream"
            );

            // Step 1: Collect all raw fetches (fast - just memory copies)
            let mut raw_fetches = Vec::new();
            while let Some(fetch_result) = stream.next().await {
                let fetch = match fetch_result {
                    Ok(f) => f,
                    Err(e) => {
                        warn!(error = %e, "Failed to fetch message");
                        continue;
                    }
                };

                let uid = fetch.uid.unwrap_or(0);
                let body = fetch.body().unwrap_or(&[]).to_vec();
                let flags: Vec<String> = fetch.flags().map(|f| format!("{:?}", f)).collect();
                let size = fetch.size.unwrap_or(0) as u32;
                let internal_date = fetch.internal_date().map(|dt| dt.timestamp());
                let gm_msgid = Self::extract_gm_msgid(&fetch);
                let gm_thrid = Self::extract_gm_thrid(&fetch);
                let labels = Self::extract_gm_labels(&fetch);

                // Extract envelope data as owned values (Envelope doesn't implement Clone)
                let envelope_subject = fetch.envelope()
                    .and_then(|e| e.subject.as_ref())
                    .and_then(|s| std::str::from_utf8(s).ok())
                    .map(|s| s.to_string());

                let envelope_from = fetch.envelope()
                    .and_then(|e| e.from.as_ref())
                    .and_then(|addrs| addrs.first())
                    .and_then(|addr| {
                        addr.mailbox
                            .as_ref()
                            .and_then(|m| std::str::from_utf8(m).ok())
                            .map(|m| m.to_string())
                    });

                raw_fetches.push((uid, body, envelope_subject, envelope_from, flags, size, internal_date, gm_msgid, gm_thrid, labels));
            }

            debug!(
                account = %account.id,
                folder = %folder_name,
                count = raw_fetches.len(),
                fetch_ms = ?fetch_start.elapsed().as_millis(),
                "Fetched raw messages, starting parallel parse"
            );

            // Step 2: Parse and sanitize in parallel (CPU-intensive work)
            let parse_start = Instant::now();
            let account_id = account.id.clone();
            let folder_name_owned = folder_name.to_string();

            let parsed_results: Vec<Result<(MessageRecord, BodyRecord)>> =
                tokio::task::spawn_blocking(move || {
                    use rayon::prelude::*;
                    raw_fetches
                        .into_par_iter()
                        .map(|(uid, body, envelope_subject, envelope_from, flags, size, internal_date, gm_msgid, gm_thrid, labels)| {
                            // Parse MIME (CPU-intensive)
                            let parsed = mailparse::parse_mail(&body)
                                .with_context(|| format!("parsing MIME for UID {}", uid))?;

                            // Sanitize (CPU-intensive)
                            let sanitized = sanitize_message(&parsed, &body);

                            // Use pre-extracted envelope data or fallback to headers
                            let subject = envelope_subject
                                .as_ref()
                                .and_then(|s| decode_mime_header(s))
                                .or_else(|| get_header_value(&parsed, "Subject"));

                            let from = envelope_from
                                .or_else(|| get_header_value(&parsed, "From"));

                            // Build message record
                            let message_id = gm_msgid.unwrap_or_else(|| {
                                format!("{}:{}:{}", account_id, folder_name_owned, uid)
                            });

                            let message = MessageRecord {
                                id: message_id.clone(),
                                account_id: account_id.clone(),
                                folder: folder_name_owned.clone(),
                                uid: Some(uid),
                                thread_id: gm_thrid,
                                internal_date,
                                subject,
                                from,
                                to: get_header_value(&parsed, "To"),
                                cc: get_header_value(&parsed, "Cc"),
                                bcc: get_header_value(&parsed, "Bcc"),
                                flags,
                                labels,
                                has_attachments: sanitized.has_attachments,
                                size_bytes: Some(size),
                                raw_hash: Some(sanitized.raw_hash.clone()),
                                created_at: now_ts(),
                                updated_at: now_ts(),
                            };

                            let body_record = crate::sanitize::build_body_record(
                                &message_id,
                                Some(body),
                                sanitized,
                            );

                            Ok((message, body_record))
                        })
                        .collect()
                })
                .await
                .context("parallel parsing task panicked")?;

            debug!(
                account = %account.id,
                folder = %folder_name,
                parse_ms = ?parse_start.elapsed().as_millis(),
                "Parallel parse completed"
            );

            // Step 3: Unpack results and batch write
            let mut messages_batch = Vec::new();
            let mut bodies_batch = Vec::new();

            for result in parsed_results {
                match result {
                    Ok((msg, body)) => {
                        messages_batch.push(msg);
                        bodies_batch.push(body);
                    }
                    Err(e) => warn!(error = %e, "Failed to parse message"),
                }
            }

            // Batch write all messages and bodies in a single transaction
            if !messages_batch.is_empty() {
                let write_start = Instant::now();

                self.db.batch_upsert_messages_with_bodies(&messages_batch, &bodies_batch).await?;

                // Clean up legacy duplicates (old fallback ids) now that we have stable ids + raw_hash.
                // This is intentionally conservative: it only removes legacy ids (contain ':') when
                // a stable numeric id row exists for the same raw bytes.
                if account.provider == crate::types::Provider::GmailImap {
                    if let Ok(n) = self
                        .db
                        .dedupe_fallback_messages_by_raw_hash(&account.id, 500)
                        .await
                    {
                        if n > 0 {
                            debug!(
                                account = %account.id,
                                folder = %folder_name,
                                deleted = n,
                                "Deduped legacy messages after batch insert"
                            );
                        }
                    }
                }

                info!(
                    account = %account.id,
                    folder = %folder_name,
                    count = messages_batch.len(),
                    fetch_ms = ?fetch_start.elapsed().as_millis(),
                    parse_ms = ?parse_start.elapsed().as_millis(),
                    write_ms = ?write_start.elapsed().as_millis(),
                    total_ms = ?batch_start.elapsed().as_millis(),
                    "Batch processed (parallel parse + transaction write)"
                );
            }
        }

        Ok(())
    }

    async fn fetch_and_handle_new_uids(
        &self,
        session: &mut ImapSession,
        account: &Account,
        folder_name: &str,
        uids: &[u32],
    ) -> Result<()> {
        const BATCH_SIZE: usize = 250;

        let mut need_body: Vec<u32> = Vec::new();
        let mut location_updates: Vec<(
            String,
            String,
            u32,
            Vec<String>,
            Vec<String>,
            Option<String>,
            Option<i64>,
            Option<u32>,
        )> = Vec::new();

        for chunk in uids.chunks(BATCH_SIZE) {
            let uid_seq = Self::build_uid_sequence(chunk);
            let fetch_query = "(UID FLAGS INTERNALDATE RFC822.SIZE ENVELOPE X-GM-MSGID X-GM-THRID X-GM-LABELS)";

            let mut stream = session
                .uid_fetch(&uid_seq, fetch_query)
                .await
                .context("fetching metadata for new UIDs")?;

            let mut batch = Vec::new();
            while let Some(fetch_result) = stream.next().await {
                let fetch = match fetch_result {
                    Ok(f) => f,
                    Err(e) => {
                        warn!(error = %e, "Failed to fetch message metadata");
                        continue;
                    }
                };

                let uid = fetch.uid.unwrap_or(0);
                if uid == 0 {
                    continue;
                }

                let gm_msgid = Self::extract_gm_msgid(&fetch);
                let gm_thrid = Self::extract_gm_thrid(&fetch);
                let labels = Self::extract_gm_labels(&fetch);
                let flags: Vec<String> = fetch.flags().map(|f| format!("{:?}", f)).collect();
                let size = fetch.size;
                let internal_date = fetch.internal_date().map(|dt| dt.timestamp());

                let message_id = gm_msgid.unwrap_or_else(|| {
                    format!("{}:{}:{}", account.id, folder_name, uid)
                });

                batch.push((uid, message_id, flags, labels, gm_thrid, internal_date, size));
            }

            if batch.is_empty() {
                continue;
            }

            let ids: Vec<String> = batch.iter().map(|(_, id, _, _, _, _, _)| id.clone()).collect();
            let existing = self.db.load_existing_message_ids(&account.id, &ids).await?;

            for (uid, message_id, flags, labels, thread_id, internal_date, size) in batch {
                if existing.contains(&message_id) {
                    location_updates.push((
                        message_id,
                        folder_name.to_string(),
                        uid,
                        flags,
                        labels,
                        thread_id,
                        internal_date,
                        size,
                    ));
                } else {
                    need_body.push(uid);
                }
            }
        }

        if !location_updates.is_empty() {
            self.db
                .batch_update_message_location_by_id(&account.id, &location_updates)
                .await?;
            info!(
                account = %account.id,
                folder = %folder_name,
                count = location_updates.len(),
                "Moved/existing messages updated without refetching bodies"
            );
        }

        if !need_body.is_empty() {
            self.fetch_and_store_new_messages(session, account, folder_name, &need_body)
                .await?;
        }

        Ok(())
    }

    #[allow(dead_code)]
    async fn update_existing_messages(
        &self,
        session: &mut ImapSession,
        account: &Account,
        folder_name: &str,
        uids: &[u32],
    ) -> Result<()> {
        // For existing messages, fetch only flags/labels to check for changes
        const BATCH_SIZE: usize = 100;

        for chunk in uids.chunks(BATCH_SIZE) {
            let uid_seq = Self::build_uid_sequence(chunk);

            debug!(
                account = %account.id,
                folder = %folder_name,
                count = chunk.len(),
                "Updating flags/labels for existing messages"
            );

            let fetch_query = "(UID FLAGS X-GM-LABELS)";

            let mut stream = session
                .uid_fetch(&uid_seq, fetch_query)
                .await
                .context("fetching message flags")?;

            while let Some(fetch_result) = stream.next().await {
                let fetch = match fetch_result {
                    Ok(f) => f,
                    Err(e) => {
                        warn!(error = %e, "Failed to fetch message flags");
                        continue;
                    }
                };

                let uid = fetch.uid.unwrap_or(0);
                let _flags: Vec<String> = fetch
                    .flags()
                    .map(|f| format!("{:?}", f))
                    .collect();
                let _labels = Self::extract_gm_labels(&fetch);

                // TODO: Load existing message and compare flags/labels
                // For now, we'll skip updates to keep it simple
                debug!(
                    account = %account.id,
                    folder = %folder_name,
                    uid = uid,
                    "Checked message metadata"
                );
            }
        }

        Ok(())
    }

    fn build_uid_sequence(uids: &[u32]) -> String {
        if uids.is_empty() {
            return "1".to_string();
        }

        // Simple comma-separated list
        // In production, compress to ranges (e.g., "1:5,7,10:15")
        uids.iter()
            .map(|u| u.to_string())
            .collect::<Vec<_>>()
            .join(",")
    }

    fn extract_gm_msgid(fetch: &async_imap::types::Fetch) -> Option<String> {
        fetch.gmail_msgid().map(|v| v.to_string())
    }

    fn extract_gm_thrid(fetch: &async_imap::types::Fetch) -> Option<String> {
        fetch.gmail_thrid().map(|v| v.to_string())
    }

    fn extract_gm_labels(fetch: &async_imap::types::Fetch) -> Vec<String> {
        fetch.gmail_labels()
    }
}

impl SyncEngine {
    async fn fetch_and_update_flags(
        &self,
        session: &mut ImapSession,
        account: &Account,
        folder_name: &str,
        uids: &[u32],
    ) -> Result<()> {
        const BATCH_SIZE: usize = 250;

        let mut updates: Vec<(u32, Vec<String>)> = Vec::new();
        for chunk in uids.chunks(BATCH_SIZE) {
            let uid_seq = Self::build_uid_sequence(chunk);
            let mut stream = session
                .uid_fetch(&uid_seq, "(UID FLAGS)")
                .await
                .context("fetching flags for changed messages")?;

            while let Some(fetch_result) = stream.next().await {
                let fetch = match fetch_result {
                    Ok(f) => f,
                    Err(e) => {
                        warn!(error = %e, "Failed to fetch message flags");
                        continue;
                    }
                };

                let uid = fetch.uid.unwrap_or(0);
                if uid == 0 {
                    continue;
                }

                let flags: Vec<String> = fetch.flags().map(|f| format!("{:?}", f)).collect();
                updates.push((uid, flags));
            }
        }

        if !updates.is_empty() {
            self.db
                .batch_update_message_flags_by_uid(&account.id, folder_name, &updates)
                .await?;
            debug!(
                account = %account.id,
                folder = %folder_name,
                count = updates.len(),
                "Updated flags for changed messages"
            );
        }

        Ok(())
    }
}

fn get_header_value(parsed: &mailparse::ParsedMail, header_name: &str) -> Option<String> {
    parsed
        .headers
        .iter()
        .find(|h| h.get_key().eq_ignore_ascii_case(header_name))
        .map(|h| h.get_value())
}

fn decode_mime_header(header: &str) -> Option<String> {
    // Use mailparse to decode MIME-encoded headers
    // This handles both quoted-printable and base64 encoded words
    let header_str = format!("Subject: {}\r\n\r\n", header);
    if let Ok(parsed) = mailparse::parse_mail(header_str.as_bytes()) {
        parsed.headers.iter()
            .find(|h| h.get_key().eq_ignore_ascii_case("Subject"))
            .map(|h| h.get_value())
    } else {
        Some(header.to_string())
    }
}

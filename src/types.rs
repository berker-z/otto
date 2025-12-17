use chrono::{NaiveDate, Utc};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Provider {
    GmailImap,
}

#[derive(Clone, Debug)]
pub struct Account {
    pub id: String,
    pub email: String,
    pub provider: Provider,
    pub settings: AccountSettings,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Clone, Debug)]
pub struct AccountSettings {
    pub folders: Vec<String>,
    pub cutoff_since: NaiveDate,
    pub poll_interval_minutes: u32,
    pub prefetch_recent: u32,
    pub safe_mode: bool,
}

impl AccountSettings {
    pub fn with_defaults(cutoff_since: NaiveDate) -> Self {
        Self {
            folders: vec![
                "INBOX".to_string(),
                "[Gmail]/Sent Mail".to_string(),
                "[Gmail]/Trash".to_string(),
                "[Gmail]/Spam".to_string(),
            ],
            cutoff_since,
            poll_interval_minutes: 5,
            prefetch_recent: 100,
            safe_mode: false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct FolderState {
    pub id: i64,
    pub account_id: String,
    pub name: String,
    pub uidvalidity: Option<u32>,
    pub highest_uid: Option<u32>,
    pub highestmodseq: Option<u64>,
    pub exists_count: Option<u32>,
    pub last_sync_ts: Option<i64>,
    pub last_uid_scan_ts: Option<i64>,
}

#[derive(Clone, Debug)]
pub struct MessageRecord {
    pub id: String, // provider message id (X-GM-MSGID for Gmail)
    pub account_id: String,
    pub folder: String,
    pub uid: Option<u32>,
    pub thread_id: Option<String>,
    pub internal_date: Option<i64>,
    pub subject: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub cc: Option<String>,
    pub bcc: Option<String>,
    pub flags: Vec<String>,
    pub labels: Vec<String>,
    pub has_attachments: bool,
    pub size_bytes: Option<u32>,
    pub raw_hash: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Clone, Debug)]
pub struct BodyRecord {
    pub message_id: String,
    pub raw_rfc822: Option<Vec<u8>>,
    pub sanitized_text: Option<String>,
    pub mime_summary: Option<String>,
    pub attachments_json: Option<String>,
    pub sanitized_at: Option<i64>,
}

pub fn now_ts() -> i64 {
    Utc::now().timestamp()
}

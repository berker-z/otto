use anyhow::Result;
use chrono::NaiveDate;
use std::env;

/// Application-wide defaults. These can be overridden by env vars but do not
/// require any user-authored config files.
#[derive(Debug, Clone)]
pub struct AppDefaults {
    pub cutoff_since: NaiveDate,
    pub poll_interval_minutes: u32,
    pub prefetch_recent: u32,
    pub safe_mode: bool,
    pub folders: Vec<String>,
}

impl AppDefaults {
    pub fn load() -> Result<Self> {
        let cutoff =
            cutoff_from_env().unwrap_or_else(|| NaiveDate::from_ymd_opt(2025, 12, 1).unwrap());
        let poll_interval_minutes = env::var("OTTO_POLL_INTERVAL_MINUTES")
            .ok()
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(5);
        let prefetch_recent = env::var("OTTO_PREFETCH_RECENT")
            .ok()
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(100);
        let safe_mode = env::var("OTTO_SAFE_MODE")
            .ok()
            .map(|s| s == "1" || s.eq_ignore_ascii_case("true"))
            .unwrap_or(false);

        let folders = vec![
            env::var("OTTO_FOLDER_INBOX").unwrap_or_else(|_| "INBOX".to_string()),
            env::var("OTTO_FOLDER_SENT").unwrap_or_else(|_| "[Gmail]/Sent Mail".to_string()),
            env::var("OTTO_FOLDER_TRASH").unwrap_or_else(|_| "[Gmail]/Trash".to_string()),
            env::var("OTTO_FOLDER_SPAM").unwrap_or_else(|_| "[Gmail]/Spam".to_string()),
        ];

        Ok(Self {
            cutoff_since: cutoff,
            poll_interval_minutes,
            prefetch_recent,
            safe_mode,
            folders,
        })
    }
}

fn cutoff_from_env() -> Option<NaiveDate> {
    let raw = env::var("OTTO_CUTOFF_SINCE").ok()?;
    NaiveDate::parse_from_str(&raw, "%Y-%m-%d").ok()
}

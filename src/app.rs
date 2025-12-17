use crate::cli::Cli;
use crate::config::AppDefaults;
use crate::onboarding;
use crate::storage::Database;
use crate::sync::SyncEngine;
use crate::tui;
use crate::types::Account;
use anyhow::Result;
use chrono::{DateTime, Utc};
use std::sync::{Arc, mpsc};
use tracing::{info, warn};

pub async fn run(cli: Cli) -> Result<()> {
    let defaults = AppDefaults::load()?;
    let db = Arc::new(Database::new_default().await?);
    info!(path = %db.path().display(), "Using SQLite store");

    let mut accounts = db.list_accounts().await?;

    if cli.add_account || accounts.is_empty() {
        let (account, _token) = onboarding::onboard_account(&defaults).await?;
        db.save_account(&account).await?;
        accounts = db.list_accounts().await?;
        info!(account = %account.id, "Account added");
    }

    if accounts.is_empty() {
        warn!("No accounts configured. Run with --add-account to onboard.");
        return Ok(());
    }

    if cli.tui {
        launch_tui(&cli, &accounts, db.clone()).await?;
        return Ok(());
    }

    if !cli.no_sync {
        let engine = SyncEngine::new(db.clone());
        engine.sync_all(&accounts, cli.force).await?;
    } else {
        info!("Skipping sync; using cached data only");
    }

    // Display latest 10 emails
    println!("\n{}", "=".repeat(80));
    println!("📬 Latest 10 Emails");
    println!("{}\n", "=".repeat(80));

    for account in &accounts {
        let messages = db.load_messages(&account.id, 10).await?;

        if messages.is_empty() {
            println!("No messages found for {}\n", account.email);
            continue;
        }

        for (i, (msg, body)) in messages.iter().enumerate() {
            let date = msg
                .internal_date
                .map(|ts| {
                    DateTime::<Utc>::from_timestamp(ts, 0)
                        .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
                        .unwrap_or_else(|| "Unknown".to_string())
                })
                .unwrap_or_else(|| "Unknown".to_string());

            let from = msg.from.as_deref().unwrap_or("Unknown");
            let subject = msg.subject.as_deref().unwrap_or("(No Subject)");

            // Decode MIME-encoded subjects for display
            let subject = decode_mime_words(subject);

            let is_read = msg.flags.iter().any(|f| f.eq("Seen") || f.eq("\\Seen"));
            let status = if is_read { "R" } else { "U" };

            println!("{}. [{}] [{}] {}", i + 1, date, status, subject);
            println!("   From: {}", from);
            println!("   Folder: {}", msg.folder);

            if let Some(body_record) = body
                && let Some(text) = &body_record.sanitized_text
            {
                let preview = text
                    .lines()
                    .filter(|line| !line.trim().is_empty())
                    .take(2)
                    .collect::<Vec<_>>()
                    .join(" ");

                let preview = if preview.chars().count() > 100 {
                    let truncated: String = preview.chars().take(100).collect();
                    format!("{}...", truncated)
                } else {
                    preview
                };

                if !preview.is_empty() {
                    println!("   Preview: {}", preview);
                }
            }

            println!();
        }
    }

    println!("{}", "=".repeat(80));

    Ok(())
}

async fn launch_tui(cli: &Cli, accounts: &[Account], db: Arc<Database>) -> Result<()> {
    if let Some(account) = accounts.first() {
        let messages = db.load_messages(&account.id, 50).await?;
        let mail_items = tui::build_mail_items(&messages);
        let (update_tx, update_rx) = mpsc::channel();

        if !cli.no_sync {
            let start_tx = update_tx.clone();
            let sync_tx = update_tx.clone();
            let db_for_sync = db.clone();
            let accounts_for_sync = accounts.to_vec();
            let account_id = account.id.clone();
            let force = cli.force;

            let _ = start_tx.send(tui::TuiEvent::SyncStarted);

            tokio::spawn(async move {
                let engine = SyncEngine::new(db_for_sync.clone());
                if let Err(e) = engine.sync_all(&accounts_for_sync, force).await {
                    warn!(error = %e, "Background sync failed");
                }
                let _ = sync_tx.send(tui::TuiEvent::SyncFinished);

                match db_for_sync.load_messages(&account_id, 50).await {
                    Ok(messages) => {
                        let items = tui::build_mail_items(&messages);
                        let _ = sync_tx.send(tui::TuiEvent::MailItems(items));
                    }
                    Err(e) => {
                        warn!(account = %account_id, error = %e, "Reloading messages after sync failed");
                    }
                }
            });
        } else {
            info!("Skipping sync; TUI will use cached data only");
        }

        let state = tui::TuiState {
            mail_items,
            updates: Some(update_rx),
        };

        tokio::task::block_in_place(|| tui::run(state))?;
    } else {
        warn!("No accounts available for TUI; falling back to simple list.");
    }

    Ok(())
}

#[allow(unused_assignments)]
fn decode_mime_words(text: &str) -> String {
    // Decode MIME-encoded words like =?UTF-8?Q?...?= or =?UTF-8?B?...?=
    if !text.contains("=?") {
        return text.to_string();
    }

    let mut result = String::new();
    let mut remaining = text;
    let mut last_was_encoded = false;

    while let Some(start) = remaining.find("=?") {
        // Add text before the encoded word
        let before = &remaining[..start];
        if !before.is_empty() {
            // If last was encoded and this is just whitespace, skip it
            if last_was_encoded && before.trim().is_empty() {
                // Skip whitespace between consecutive encoded words
            } else {
                result.push_str(before);
                last_was_encoded = false;
            }
        }

        // Find the end of this encoded word by parsing the structure
        // Format: =?charset?encoding?encoded-text?=
        // We need to skip 2 '?' and find the 3rd one followed by '='
        let search_start = start + 2; // Skip "=?"
        let mut question_count = 0;
        let mut end_pos = None;

        for (i, ch) in remaining[search_start..].char_indices() {
            if ch == '?' {
                question_count += 1;
                if question_count == 2 {
                    // Found the '?' before encoded-text, now look for closing ?=
                    let rest = &remaining[search_start + i + 1..];
                    if let Some(closing) = rest.find("?=") {
                        end_pos = Some(search_start + i + 1 + closing + 2);
                        break;
                    }
                }
            }
        }

        if let Some(end) = end_pos {
            let encoded = &remaining[start..end];

            if let Some(decoded) = decode_mime_word(encoded) {
                result.push_str(&decoded);
                last_was_encoded = true;
            } else {
                // If decode failed, keep the original text
                result.push_str(encoded);
                last_was_encoded = false;
            }

            remaining = &remaining[end..];
        } else {
            // No valid closing found, just add the rest
            result.push_str(&remaining[start..]);
            break;
        }
    }

    result.push_str(remaining);
    result
}

fn decode_mime_word(word: &str) -> Option<String> {
    // Format: =?charset?encoding?encoded-text?=
    if !word.starts_with("=?") || !word.ends_with("?=") {
        return None;
    }

    let inner = &word[2..word.len() - 2];
    let parts: Vec<&str> = inner.splitn(3, '?').collect();

    if parts.len() != 3 {
        return None;
    }

    let encoding = parts[1].to_uppercase();
    let encoded_text = parts[2];

    match encoding.as_str() {
        "Q" => decode_quoted_printable_rfc2047(encoded_text),
        "B" => decode_base64_simple(encoded_text),
        _ => None,
    }
}

fn decode_quoted_printable_rfc2047(text: &str) -> Option<String> {
    let mut result = Vec::new();
    let mut i = 0;
    let bytes = text.as_bytes();

    while i < bytes.len() {
        match bytes[i] {
            b'=' if i + 2 < bytes.len() => {
                // Try to decode hex
                let hex_str = std::str::from_utf8(&bytes[i + 1..i + 3]).ok()?;
                if let Ok(byte) = u8::from_str_radix(hex_str, 16) {
                    result.push(byte);
                    i += 3;
                } else {
                    // Not valid hex, just add the '='
                    result.push(b'=');
                    i += 1;
                }
            }
            b'_' => {
                result.push(b' ');
                i += 1;
            }
            b => {
                result.push(b);
                i += 1;
            }
        }
    }

    String::from_utf8(result).ok()
}

fn decode_base64_simple(text: &str) -> Option<String> {
    use base64::Engine;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(text.as_bytes())
        .ok()?;
    String::from_utf8(decoded).ok()
}

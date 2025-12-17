use crate::config::AppDefaults;
use crate::oauth::{authorize_with_scopes, fetch_user_email, TokenBundle};
use crate::types::{now_ts, Account, AccountSettings, Provider};
use anyhow::Result;
use oauth2::Scope;
use tracing::info;

/// Run OAuth flow, fetch the user's email, and return an Account + token bundle.
pub async fn onboard_account(defaults: &AppDefaults) -> Result<(Account, TokenBundle)> {
    let scopes = vec![
        Scope::new("https://mail.google.com/".into()),
        Scope::new("https://www.googleapis.com/auth/userinfo.email".into()),
    ];
    let token = authorize_with_scopes(&scopes, "default").await?;
    let email = fetch_user_email(&token.access_token).await?;
    let now = now_ts();
    let account = Account {
        id: email.clone(),
        email,
        provider: Provider::GmailImap,
        settings: AccountSettings {
            folders: defaults.folders.clone(),
            cutoff_since: defaults.cutoff_since,
            poll_interval_minutes: defaults.poll_interval_minutes,
            prefetch_recent: defaults.prefetch_recent,
            safe_mode: defaults.safe_mode,
        },
        created_at: now,
        updated_at: now,
    };
    info!(account = %account.id, "Onboarded account via OAuth");
    Ok((account, token))
}

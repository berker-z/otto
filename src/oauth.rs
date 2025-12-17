use crate::errors::{AppError, AppResult};
use chrono::{DateTime, Duration, Utc};
use oauth2::basic::BasicClient;
use oauth2::reqwest::async_http_client;
use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, PkceCodeChallenge,
    PkceCodeVerifier, RedirectUrl, RefreshToken, Scope, TokenResponse, TokenUrl,
};
use serde::Deserialize;
use std::env;
use std::fs;
use std::io::Write;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tracing::{info, warn};

const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const SERVICE_NAME: &str = "otto-google-oauth";

#[derive(Clone, Debug)]
pub struct TokenBundle {
    pub access_token: String,
    pub expires_at: Option<DateTime<Utc>>,
    pub refresh_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UserInfo {
    email: String,
}

pub async fn authorize_with_scopes(scopes: &[Scope], token_key: &str) -> AppResult<TokenBundle> {
    let creds = load_credentials()?;
    let token_store = TokenStore::from_key(token_key);

    if let Some(refresh) = token_store.load()? {
        if let Some(bundle) =
            try_refresh(&build_client(&creds, &pick_redirect_uri()?)?, refresh).await?
        {
            return Ok(bundle);
        }
        warn!(account = %token_key, "Stored refresh token failed; re-authenticating");
        let _ = token_store.delete();
    }

    let base_redirect = pick_redirect_uri()?;
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(|e| AppError::Unexpected(format!("failed to bind loopback port: {e}")))?;
    let local_port = listener
        .local_addr()
        .map(|addr| addr.port())
        .map_err(|e| AppError::Unexpected(format!("failed to read local addr: {e}")))?;

    let redirect = build_redirect_url(&base_redirect, local_port)?;
    let client = build_client(&creds, &redirect)?;

    let (auth_url, verifier, csrf) = build_auth_url(&client, scopes)?;
    info!(account = %token_key, redirect = %redirect, "Opening browser for Google OAuth consent");
    open_in_browser(&auth_url);

    let code = listen_for_code(listener).await?;
    if code.state != *csrf.secret() {
        return Err(AppError::AuthExpired);
    }

    let token_res = client
        .exchange_code(AuthorizationCode::new(code.code))
        .set_pkce_verifier(verifier)
        .request_async(async_http_client)
        .await
        .map_err(|e| AppError::Network(format!("token exchange failed: {e}")))?;

    let refresh = token_res.refresh_token().map(|r| r.secret().to_string());
    if let Some(ref_token) = &refresh {
        token_store.save(ref_token)?;
    }

    Ok(TokenBundle {
        access_token: token_res.access_token().secret().to_string(),
        expires_at: token_res
            .expires_in()
            .map(|d| Utc::now() + Duration::from_std(d).unwrap_or_else(|_| Duration::seconds(0))),
        refresh_token: refresh,
    })
}

pub async fn fetch_user_email(access_token: &str) -> AppResult<String> {
    let client = reqwest::Client::new();
    let res = client
        .get("https://www.googleapis.com/oauth2/v2/userinfo")
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|e| AppError::Network(format!("userinfo request failed: {e}")))?;
    if !res.status().is_success() {
        return Err(AppError::Network(format!(
            "userinfo failed with status {}",
            res.status()
        )));
    }
    let parsed: UserInfo = res
        .json()
        .await
        .map_err(|e| AppError::Unexpected(format!("parse userinfo: {e}")))?;
    Ok(parsed.email)
}

fn load_credentials() -> AppResult<InstalledCreds> {
    let id = env::var("GOOGLE_CLIENT_ID")
        .map_err(|_| AppError::Config("GOOGLE_CLIENT_ID missing".into()))?;
    let secret = env::var("GOOGLE_CLIENT_SECRET")
        .map_err(|_| AppError::Config("GOOGLE_CLIENT_SECRET missing".into()))?;
    Ok(InstalledCreds {
        client_id: id,
        client_secret: secret,
        redirect_uris: vec![
            "http://localhost:8000".into(),
            "http://127.0.0.1:8000".into(),
            "urn:ietf:wg:oauth:2.0:oob".into(),
        ],
    })
}

#[derive(Debug, Clone)]
struct InstalledCreds {
    client_id: String,
    client_secret: String,
    #[allow(dead_code)]
    redirect_uris: Vec<String>,
}

fn pick_redirect_uri() -> AppResult<String> {
    Ok("http://127.0.0.1:8000".to_string())
}

fn build_redirect_url(base: &str, port: u16) -> AppResult<String> {
    let mut url = url::Url::parse(base)
        .map_err(|e| AppError::Config(format!("invalid redirect uri {base}: {e}")))?;
    url.set_port(Some(port))
        .map_err(|_| AppError::Config("failed to set redirect port".into()))?;
    Ok(url.to_string())
}

fn build_client(creds: &InstalledCreds, redirect: &str) -> AppResult<BasicClient> {
    let client = BasicClient::new(
        ClientId::new(creds.client_id.clone()),
        Some(ClientSecret::new(creds.client_secret.clone())),
        AuthUrl::new(AUTH_URL.to_string()).unwrap(),
        Some(TokenUrl::new(TOKEN_URL.to_string()).unwrap()),
    )
    .set_redirect_uri(
        RedirectUrl::new(redirect.to_string())
            .map_err(|e| AppError::Config(format!("invalid redirect uri {redirect}: {e}")))?,
    )
    .set_auth_type(oauth2::AuthType::RequestBody);

    Ok(client)
}

fn build_auth_url(
    client: &BasicClient,
    scopes: &[Scope],
) -> AppResult<(String, PkceCodeVerifier, CsrfToken)> {
    let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
    let mut req = client
        .authorize_url(CsrfToken::new_random)
        .add_extra_param("access_type", "offline")
        .add_extra_param("prompt", "consent")
        .set_pkce_challenge(challenge);
    for scope in scopes {
        req = req.add_scope(scope.clone());
    }
    let (url, csrf) = req.url();
    Ok((url.to_string(), verifier, csrf))
}

async fn try_refresh(client: &BasicClient, token: StoredToken) -> AppResult<Option<TokenBundle>> {
    let refresh = RefreshToken::new(token.refresh_token);
    let res = client
        .exchange_refresh_token(&refresh)
        .request_async(async_http_client)
        .await;
    match res {
        Ok(token_res) => Ok(Some(TokenBundle {
            access_token: token_res.access_token().secret().to_string(),
            expires_at: token_res.expires_in().map(|d| {
                Utc::now() + Duration::from_std(d).unwrap_or_else(|_| Duration::seconds(0))
            }),
            refresh_token: None,
        })),
        Err(err) => {
            warn!("Refresh token invalid or expired: {err}");
            Ok(None)
        }
    }
}

struct CodeResponse {
    code: String,
    state: String,
}

async fn listen_for_code(listener: TcpListener) -> AppResult<CodeResponse> {
    let (mut stream, _) = listener
        .accept()
        .await
        .map_err(|e| AppError::Unexpected(format!("redirect accept failed: {e}")))?;

    let mut buf = [0u8; 4096];
    let n = stream
        .read(&mut buf)
        .await
        .map_err(|e| AppError::Unexpected(format!("reading auth callback failed: {e}")))?;
    let req = String::from_utf8_lossy(&buf[..n]);
    let first_line = req.lines().next().unwrap_or("");
    let path = first_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| AppError::Unexpected("invalid HTTP request".into()))?;
    let full_url = format!("http://localhost{path}");
    let parsed = url::Url::parse(&full_url)
        .map_err(|e| AppError::Unexpected(format!("failed to parse callback url: {e}")))?;

    let code = parsed
        .query_pairs()
        .find(|(k, _)| k == "code")
        .map(|(_, v)| v.to_string())
        .ok_or_else(|| AppError::Unexpected("callback missing code parameter".into()))?;
    let state = parsed
        .query_pairs()
        .find(|(k, _)| k == "state")
        .map(|(_, v)| v.to_string())
        .unwrap_or_default();

    let response =
        "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\nAuth complete. You can close this tab.";
    let _ = stream.write_all(response.as_bytes()).await;
    Ok(CodeResponse { code, state })
}

fn open_in_browser(url: &str) {
    let attempt = if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg(url).status()
    } else if cfg!(target_os = "windows") {
        std::process::Command::new("rundll32.exe")
            .args(["url.dll,FileProtocolHandler", url])
            .status()
    } else {
        std::process::Command::new("xdg-open").arg(url).status()
    };
    if let Err(e) = attempt {
        warn!("Could not auto-open browser: {e}. Open this URL manually:\n{url}");
    } else {
        println!("If your browser did not open, navigate to:\n{url}");
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct StoredToken {
    refresh_token: String,
}

#[derive(Clone)]
struct TokenStore {
    account_id: String,
}

impl TokenStore {
    fn from_key(key: &str) -> Self {
        Self {
            account_id: key.to_string(),
        }
    }

    fn load(&self) -> AppResult<Option<StoredToken>> {
        match self.load_keyring() {
            Ok(Some(tok)) => return Ok(Some(tok)),
            Ok(None) => {}
            Err(e) => warn!("Keyring unavailable: {e}"),
        }

        Ok(None)
    }

    fn save(&self, refresh: &str) -> AppResult<()> {
        let token = StoredToken {
            refresh_token: refresh.to_string(),
        };
        let serialized =
            serde_json::to_string(&token).map_err(|e| AppError::Unexpected(format!("{e}")))?;

        if let Err(e) = self.save_keyring(&serialized) {
            warn!("Keyring save failed ({e}); writing to temp file as fallback");
            self.save_file(&serialized)?;
        }
        Ok(())
    }

    fn delete(&self) -> AppResult<()> {
        if let Ok(entry) = keyring::Entry::new(SERVICE_NAME, &self.account_id) {
            let _ = entry.delete_password();
        }
        Ok(())
    }

    fn load_keyring(&self) -> Result<Option<StoredToken>, String> {
        let entry = keyring::Entry::new(SERVICE_NAME, &self.account_id)
            .map_err(|e| format!("keyring entry error: {e}"))?;
        match entry.get_password() {
            Ok(pwd) => serde_json::from_str(&pwd)
                .map(Some)
                .map_err(|e| format!("keyring token decode: {e}")),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(format!("keyring read: {e}")),
        }
    }

    fn save_keyring(&self, serialized: &str) -> Result<(), String> {
        let entry = keyring::Entry::new(SERVICE_NAME, &self.account_id)
            .map_err(|e| format!("keyring entry error: {e}"))?;
        entry
            .set_password(serialized)
            .map_err(|e| format!("keyring write: {e}"))
    }

    fn save_file(&self, serialized: &str) -> AppResult<()> {
        let tmp = std::env::temp_dir().join(format!("otto_token_{}.json", &self.account_id));

        let mut file = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&tmp)
            .map_err(|e| AppError::Unexpected(format!("opening temp token file: {e}")))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = file.set_permissions(fs::Permissions::from_mode(0o600));
        }

        file.write_all(serialized.as_bytes())
            .map_err(|e| AppError::Unexpected(format!("writing token file: {e}")))?;
        file.sync_all()
            .map_err(|e| AppError::Unexpected(format!("syncing token file: {e}")))?;
        warn!(
            path = %tmp.display(),
            "Token saved to temp file due to keyring issues; move/delete after debugging."
        );
        Ok(())
    }
}

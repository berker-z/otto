//! IMAP connector (XOAUTH2) using async-imap 0.11 with tokio-rustls.
use anyhow::{Context, Result};
use async_imap::{Authenticator, Client, Session};
use rustls_native_certs::load_native_certs;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tokio_rustls::rustls::{ClientConfig, RootCertStore, ServerName};
use tokio_util::compat::TokioAsyncReadCompatExt;

use crate::types::Account;

pub struct ImapClient;

impl ImapClient {
    pub async fn connect(
        account: &Account,
        access_token: &str,
    ) -> Result<Session<tokio_util::compat::Compat<tokio_rustls::client::TlsStream<TcpStream>>>>
    {
        // Create TLS config with native root certificates
        let mut root_store = RootCertStore::empty();
        for cert in load_native_certs().context("failed to load native certs")? {
            root_store
                .add(&tokio_rustls::rustls::Certificate(cert.0))
                .context("failed to add cert to root store")?;
        }

        let config = ClientConfig::builder()
            .with_safe_defaults()
            .with_root_certificates(root_store)
            .with_no_client_auth();

        let connector = TlsConnector::from(Arc::new(config));

        // Connect via TCP
        let tcp = TcpStream::connect(("imap.gmail.com", 993))
            .await
            .context("connecting to imap.gmail.com:993")?;

        // Upgrade to TLS
        let server_name = ServerName::try_from("imap.gmail.com").context("invalid DNS name")?;
        let tls_stream = connector
            .connect(server_name, tcp)
            .await
            .context("starting TLS for IMAP")?;

        // Convert tokio AsyncRead/AsyncWrite to futures AsyncRead/AsyncWrite
        let compat_stream = tls_stream.compat();

        // Create IMAP client
        let mut client = Client::new(compat_stream);

        // Read server greeting
        let _greeting = client
            .read_response()
            .await
            .context("reading IMAP greeting")?
            .ok_or_else(|| anyhow::anyhow!("unexpected end of stream, expected greeting"))?;

        // Authenticate using XOAUTH2
        let xoauth = Xoauth2 {
            user: account.email.clone(),
            access_token: access_token.to_string(),
        };

        let session = client
            .authenticate("XOAUTH2", xoauth)
            .await
            .map_err(|(err, _client)| err)
            .context("XOAUTH2 authenticate")?;

        Ok(session)
    }
}

struct Xoauth2 {
    user: String,
    access_token: String,
}

impl Authenticator for Xoauth2 {
    type Response = String;

    fn process(&mut self, _challenge: &[u8]) -> String {
        format!(
            "user={}\x01auth=Bearer {}\x01\x01",
            self.user, self.access_token
        )
    }
}

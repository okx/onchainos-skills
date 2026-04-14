use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tokio_tungstenite::tungstenite::Message;

use super::manager::DohManager;

pub type DohWsStream =
    tokio_tungstenite::WebSocketStream<tokio_rustls::client::TlsStream<TcpStream>>;
pub type StdWsStream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<TcpStream>>;

pub enum DohWsConnection {
    Direct(StdWsStream),
    Proxy(DohWsStream),
}

impl DohWsConnection {
    pub async fn send(&mut self, msg: Message) -> Result<()> {
        match self {
            DohWsConnection::Direct(s) => s.send(msg).await.context("ws send (direct)"),
            DohWsConnection::Proxy(s) => s.send(msg).await.context("ws send (proxy)"),
        }
    }

    pub async fn next(
        &mut self,
    ) -> Option<Result<Message, tokio_tungstenite::tungstenite::Error>> {
        match self {
            DohWsConnection::Direct(s) => s.next().await,
            DohWsConnection::Proxy(s) => s.next().await,
        }
    }
}

fn root_certs() -> rustls::RootCertStore {
    let mut store = rustls::RootCertStore::empty();
    store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    store
}

pub async fn doh_connect_ws(url: &str, doh: &DohManager) -> Result<DohWsConnection> {
    let override_info = doh.resolve_override();

    if let Some((host, addr)) = override_info {
        // Manual connection via proxy IP
        let tcp = TcpStream::connect(addr)
            .await
            .context("TCP connect to proxy")?;

        let tls_config = rustls::ClientConfig::builder()
            .with_root_certificates(root_certs())
            .with_no_client_auth();

        let server_name = rustls::pki_types::ServerName::try_from(host.as_str())
            .context("invalid server name")?
            .to_owned();

        let tls_stream = TlsConnector::from(Arc::new(tls_config))
            .connect(server_name, tcp)
            .await
            .context("TLS handshake with proxy")?;

        let (ws, _) = tokio_tungstenite::client_async(url, tls_stream)
            .await
            .context("WS handshake over proxy TLS")?;

        Ok(DohWsConnection::Proxy(ws))
    } else {
        // Standard direct connection
        let (ws, _) = tokio_tungstenite::connect_async(url)
            .await
            .context("WS connect (direct)")?;

        Ok(DohWsConnection::Direct(ws))
    }
}

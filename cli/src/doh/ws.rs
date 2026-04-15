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

/// 建立 WebSocket 连接，有代理走代理，没代理走直连。
///
/// 代理路径不能用 connect_async（会 DNS 解析被污染的域名），
/// 需要手动分三步建连：TCP → TLS → WS 握手。
pub async fn doh_connect_ws(url: &str, doh: &DohManager) -> Result<DohWsConnection> {
    let override_info = doh.resolve_override();

    if let Some((host, addr)) = override_info {
        // ── 代理模式：手动建连 ──

        // 第一步：TCP 直连代理 IP（跳过 DNS）
        // addr = 8.212.1.102:443，不经过系统 DNS 解析
        let tcp = TcpStream::connect(addr)
            .await
            .context("TCP connect to proxy")?;

        // 第二步：TLS 握手，SNI 设为代理域名
        // server_name = "web3.ynhf1jp.com"，代理服务器持有 *.ynhf1jp.com 证书
        // 这样 TLS 证书校验能通过
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

        // 第三步：在已建好的 TLS 连接上做 WS 握手
        // url 仍是原始地址（如 wss://wsdex.okx.com/ws/v6/dex），
        // 此处只用来构造 HTTP Upgrade 请求的 Host header 和 path，
        // 不会触发新的 DNS 解析或 TCP 连接——复用上面的 tls_stream
        let (ws, _) = tokio_tungstenite::client_async(url, tls_stream)
            .await
            .context("WS handshake over proxy TLS")?;

        Ok(DohWsConnection::Proxy(ws))
    } else {
        // ── 直连模式：标准 connect_async ──
        let (ws, _) = tokio_tungstenite::connect_async(url)
            .await
            .context("WS connect (direct)")?;

        Ok(DohWsConnection::Direct(ws))
    }
}

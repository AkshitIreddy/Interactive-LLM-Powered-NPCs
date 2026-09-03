//! Bounded WebSocket transport for hosted streaming STT.
//!
//! Production endpoints are exact allow-list entries. Arbitrary URLs, plaintext remote sockets,
//! provider-managed WebSocket headers, and secret-bearing query parameters are rejected before
//! DNS or network I/O.

use std::{fmt, time::Duration};

use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio_tungstenite::{
    connect_async_with_config,
    tungstenite::{
        client::IntoClientRequest,
        http::{HeaderName, HeaderValue},
        protocol::WebSocketConfig,
        Message,
    },
    MaybeTlsStream, WebSocketStream,
};
use url::Url;
use zeroize::{Zeroize, Zeroizing};

use crate::{
    ClientFrame, ConnectRequest, ServerFrame, StreamingTransport, TransportAuth, TransportError,
    TransportFactory,
};

const ASSEMBLYAI_HOST: &str = "streaming.assemblyai.com";
const ASSEMBLYAI_PATH: &str = "/v3/ws";
const MAX_URL_BYTES: usize = 8_192;
const MAX_QUERY_PAIRS: usize = 128;
const MAX_QUERY_VALUE_BYTES: usize = 4_096;
const DEFAULT_MAX_MESSAGE_BYTES: usize = 2 * 1_048_576;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostedWebSocketTransportConfig {
    pub connect_timeout: Duration,
    pub io_timeout: Duration,
    pub close_timeout: Duration,
    pub max_frame_bytes: usize,
    pub max_message_bytes: usize,
    /// Test-only escape hatch: permits `ws://localhost:<port>/v3/ws`.
    pub allow_insecure_loopback: bool,
}

impl Default for HostedWebSocketTransportConfig {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(15),
            io_timeout: Duration::from_secs(30),
            close_timeout: Duration::from_millis(100),
            max_frame_bytes: DEFAULT_MAX_MESSAGE_BYTES,
            max_message_bytes: DEFAULT_MAX_MESSAGE_BYTES,
            allow_insecure_loopback: false,
        }
    }
}

impl HostedWebSocketTransportConfig {
    fn validate(self) -> Result<Self, TransportError> {
        if self.connect_timeout.is_zero()
            || self.connect_timeout > Duration::from_secs(60)
            || self.io_timeout.is_zero()
            || self.io_timeout > Duration::from_secs(180)
            || self.close_timeout.is_zero()
            || self.close_timeout > Duration::from_millis(150)
            || !(1_024..=8 * 1_048_576).contains(&self.max_frame_bytes)
            || !(1_024..=8 * 1_048_576).contains(&self.max_message_bytes)
            || self.max_frame_bytes > self.max_message_bytes
        {
            return Err(TransportError);
        }
        Ok(self)
    }
}

#[derive(Clone)]
pub struct HostedWebSocketTransportFactory {
    config: HostedWebSocketTransportConfig,
}

impl HostedWebSocketTransportFactory {
    pub fn new(config: HostedWebSocketTransportConfig) -> Result<Self, TransportError> {
        Ok(Self {
            config: config.validate()?,
        })
    }
}

impl Default for HostedWebSocketTransportFactory {
    fn default() -> Self {
        Self::new(HostedWebSocketTransportConfig::default())
            .expect("default hosted STT transport configuration is valid")
    }
}

impl fmt::Debug for HostedWebSocketTransportFactory {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HostedWebSocketTransportFactory")
            .field("config", &self.config)
            .finish()
    }
}

#[async_trait]
impl TransportFactory for HostedWebSocketTransportFactory {
    async fn connect(
        &self,
        request: ConnectRequest<'_>,
    ) -> Result<Box<dyn StreamingTransport>, TransportError> {
        // This workspace intentionally disables dependency default features. Install the chosen
        // crypto backend explicitly so a process linking another rustls consumer cannot leave the
        // global provider ambiguous.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let upgrade = build_upgrade_request(request, self.config)?;
        let websocket_config = WebSocketConfig::default()
            .max_frame_size(Some(self.config.max_frame_bytes))
            .max_message_size(Some(self.config.max_message_bytes));
        let connect = connect_async_with_config(upgrade, Some(websocket_config), false);
        let (socket, _) = tokio::time::timeout(self.config.connect_timeout, connect)
            .await
            .map_err(|_| TransportError)?
            .map_err(|_| TransportError)?;
        Ok(Box::new(HostedWebSocketTransport {
            socket: Some(socket),
            io_timeout: self.config.io_timeout,
            close_timeout: self.config.close_timeout,
            closed: false,
        }))
    }
}

fn build_upgrade_request(
    request: ConnectRequest<'_>,
    config: HostedWebSocketTransportConfig,
) -> Result<tokio_tungstenite::tungstenite::http::Request<()>, TransportError> {
    if request.query.len() > MAX_QUERY_PAIRS {
        return Err(TransportError);
    }
    let mut endpoint = Url::parse(request.url).map_err(|_| TransportError)?;
    validate_endpoint(&endpoint, config.allow_insecure_loopback)?;
    if endpoint.query().is_some() || endpoint.fragment().is_some() {
        return Err(TransportError);
    }
    {
        let mut query = endpoint.query_pairs_mut();
        for (key, value) in request.query {
            if !allowed_query_name(&key)
                || value.is_empty()
                || value.len() > MAX_QUERY_VALUE_BYTES
                || value.chars().any(char::is_control)
            {
                return Err(TransportError);
            }
            query.append_pair(&key, &value);
        }
    }
    if endpoint.as_str().len() > MAX_URL_BYTES {
        return Err(TransportError);
    }
    let mut upgrade = endpoint
        .as_str()
        .into_client_request()
        .map_err(|_| TransportError)?;
    let TransportAuth::Header {
        name,
        scheme,
        value,
    } = request.auth;
    if name != "Authorization" || scheme.is_some() || value.expose().is_empty() {
        return Err(TransportError);
    }
    let name = HeaderName::from_static("authorization");
    let mut secret_bytes = Zeroizing::new(value.expose().as_bytes().to_vec());
    let header = HeaderValue::from_bytes(&secret_bytes).map_err(|_| TransportError)?;
    secret_bytes.zeroize();
    upgrade.headers_mut().insert(name, header);
    Ok(upgrade)
}

fn validate_endpoint(endpoint: &Url, allow_loopback: bool) -> Result<(), TransportError> {
    if !endpoint.username().is_empty()
        || endpoint.password().is_some()
        || endpoint.fragment().is_some()
        || endpoint.path() != ASSEMBLYAI_PATH
    {
        return Err(TransportError);
    }
    let host = endpoint.host_str().ok_or(TransportError)?;
    let trusted = match endpoint.scheme() {
        "wss" => host == ASSEMBLYAI_HOST && endpoint.port().is_none(),
        "ws" => allow_loopback && matches!(host, "localhost" | "127.0.0.1" | "::1"),
        _ => false,
    };
    trusted.then_some(()).ok_or(TransportError)
}

fn allowed_query_name(name: &str) -> bool {
    matches!(
        name,
        "sample_rate"
            | "speech_model"
            | "format_turns"
            | "language_detection"
            | "keyterms_prompt"
            | "prompt"
            | "min_turn_silence"
            | "max_turn_silence"
    )
}

struct HostedWebSocketTransport {
    socket: Option<WebSocketStream<MaybeTlsStream<TcpStream>>>,
    io_timeout: Duration,
    close_timeout: Duration,
    closed: bool,
}

#[async_trait]
impl StreamingTransport for HostedWebSocketTransport {
    async fn send(&mut self, frame: ClientFrame) -> Result<(), TransportError> {
        if self.closed {
            return Err(TransportError);
        }
        let message = match frame {
            ClientFrame::Text(value) => Message::Text(value.into()),
            ClientFrame::Binary(value) => Message::Binary(value.into()),
            ClientFrame::Close => Message::Close(None),
        };
        let socket = self.socket.as_mut().ok_or(TransportError)?;
        tokio::time::timeout(self.io_timeout, socket.send(message))
            .await
            .map_err(|_| TransportError)?
            .map_err(|_| TransportError)
    }

    async fn receive(&mut self) -> Result<Option<ServerFrame>, TransportError> {
        if self.closed {
            return Ok(None);
        }
        loop {
            let socket = self.socket.as_mut().ok_or(TransportError)?;
            let message = tokio::time::timeout(self.io_timeout, socket.next())
                .await
                .map_err(|_| TransportError)?;
            match message {
                None => {
                    self.closed = true;
                    return Ok(None);
                }
                Some(Err(_)) => return Err(TransportError),
                Some(Ok(Message::Text(value))) => {
                    return Ok(Some(ServerFrame::Text(value.to_string())))
                }
                Some(Ok(Message::Binary(value))) => {
                    return Ok(Some(ServerFrame::Binary(value.to_vec())))
                }
                Some(Ok(Message::Close(frame))) => {
                    self.closed = true;
                    return Ok(Some(ServerFrame::Closed {
                        code: frame.map(|frame| u16::from(frame.code)),
                    }));
                }
                Some(Ok(Message::Ping(_)))
                | Some(Ok(Message::Pong(_)))
                | Some(Ok(Message::Frame(_))) => continue,
            }
        }
    }

    async fn close(&mut self) -> Result<(), TransportError> {
        if self.closed {
            return Ok(());
        }
        self.closed = true;
        if let Some(mut socket) = self.socket.take() {
            let _ = tokio::time::timeout(self.close_timeout, socket.close(None)).await;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{SecretString, TransportAuth};

    fn request<'secret>(
        url: &'static str,
        secret: &'secret SecretString,
    ) -> ConnectRequest<'secret> {
        ConnectRequest {
            url,
            query: vec![
                ("sample_rate".into(), "16000".into()),
                ("speech_model".into(), "u3-rt-pro".into()),
            ],
            auth: TransportAuth::Header {
                name: "Authorization",
                scheme: None,
                value: secret,
            },
        }
    }

    #[test]
    fn official_destination_is_constructible_and_debug_is_redacted() {
        let secret = SecretString::new("assembly-secret-never-print");
        let connect = request("wss://streaming.assemblyai.com/v3/ws", &secret);
        let debug = format!("{connect:?}");
        assert!(debug.contains("REDACTED"));
        assert!(!debug.contains(secret.expose()));
        let upgrade = build_upgrade_request(connect, HostedWebSocketTransportConfig::default())
            .expect("official endpoint accepted");
        assert_eq!(
            upgrade.uri().to_string(),
            "wss://streaming.assemblyai.com/v3/ws?sample_rate=16000&speech_model=u3-rt-pro"
        );
        assert!(upgrade.headers().contains_key("authorization"));
    }

    #[test]
    fn arbitrary_remote_plaintext_query_and_auth_shapes_are_rejected() {
        let secret = SecretString::new("secret");
        for url in [
            "wss://example.com/v3/ws",
            "ws://streaming.assemblyai.com/v3/ws",
            "wss://streaming.assemblyai.com/v2/realtime/ws",
        ] {
            assert!(build_upgrade_request(
                request(url, &secret),
                HostedWebSocketTransportConfig::default()
            )
            .is_err());
        }
        let bad_query = ConnectRequest {
            url: "wss://streaming.assemblyai.com/v3/ws",
            query: vec![("token".into(), "must-not-enter-url".into())],
            auth: TransportAuth::Header {
                name: "Authorization",
                scheme: None,
                value: &secret,
            },
        };
        assert!(
            build_upgrade_request(bad_query, HostedWebSocketTransportConfig::default()).is_err()
        );
    }
}

use std::{
    collections::{hash_map::RandomState, VecDeque},
    hash::BuildHasher,
    io,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use serde_json::Value;
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio_tungstenite::{
    connect_async_with_config,
    tungstenite::{
        client::IntoClientRequest,
        http::{header::RETRY_AFTER, HeaderName, HeaderValue, StatusCode},
        protocol::WebSocketConfig,
        Error as WebSocketError, Message,
    },
    MaybeTlsStream, WebSocketStream,
};
use url::Url;
use zeroize::{Zeroize, Zeroizing};

use crate::{
    CartesiaCommand, DeepgramCommand, HostedTtsProviderId, InworldCommand, OpenRequest,
    PcmEncoding, TransportError, TtsConnection, UsageEvent, VisemeEvent, WireCommand, WireEvent,
    WordAlignment, MAX_AUDIO_CHUNK_BYTES,
};

const MAX_ENDPOINT_BYTES: usize = 4_096;
const MAX_UPGRADE_URL_BYTES: usize = 8_192;
const MAX_HEADER_COUNT: usize = 32;
const MAX_HEADER_VALUE_BYTES: usize = 8_192;
const MAX_QUERY_PAIRS: usize = 32;
const MAX_COMMAND_BYTES: usize = 128 * 1_024;
const MAX_ALIGNMENT_ITEMS: usize = 16_384;
const MAX_VISEME_ITEMS: usize = 32_768;
const MAX_TIMELINE_MS: u64 = 4 * 60 * 60 * 1_000;
const MAX_ITEM_DURATION_MS: u64 = 60_000;
const DEFAULT_MAX_MESSAGE_BYTES: usize = 2 * 1_048_576;
const MAX_PENDING_WIRE_EVENTS: usize = 16;

type HostedSocket = WebSocketStream<MaybeTlsStream<TcpStream>>;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CartesiaConnectionStats {
    pub fresh_connections: u64,
    pub reused_connections: u64,
    pub stale_evictions: u64,
    pub last_connect_micros: u64,
    pub last_connection_reused: bool,
}

pub(crate) struct CartesiaSocketPool {
    slot: Mutex<Option<PooledCartesiaSocket>>,
    hasher: RandomState,
    fresh_connections: AtomicU64,
    reused_connections: AtomicU64,
    stale_evictions: AtomicU64,
    last_connect_micros: AtomicU64,
    last_connection_reused: AtomicU64,
    next_context_nonce: AtomicU64,
}

struct PooledCartesiaSocket {
    socket: HostedSocket,
    route_hash: u64,
    idle_since: tokio::time::Instant,
    idle_generation: u64,
}

impl Default for CartesiaSocketPool {
    fn default() -> Self {
        Self {
            slot: Mutex::new(None),
            hasher: RandomState::new(),
            fresh_connections: AtomicU64::new(0),
            reused_connections: AtomicU64::new(0),
            stale_evictions: AtomicU64::new(0),
            last_connect_micros: AtomicU64::new(0),
            last_connection_reused: AtomicU64::new(0),
            next_context_nonce: AtomicU64::new(1),
        }
    }
}

impl CartesiaSocketPool {
    pub(crate) fn snapshot(&self) -> CartesiaConnectionStats {
        CartesiaConnectionStats {
            fresh_connections: self.fresh_connections.load(Ordering::Relaxed),
            reused_connections: self.reused_connections.load(Ordering::Relaxed),
            stale_evictions: self.stale_evictions.load(Ordering::Relaxed),
            last_connect_micros: self.last_connect_micros.load(Ordering::Relaxed),
            last_connection_reused: self.last_connection_reused.load(Ordering::Relaxed) != 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostedWebSocketLimits {
    pub connect_timeout: Duration,
    pub io_timeout: Duration,
    pub close_timeout: Duration,
    pub max_frame_bytes: usize,
    pub max_message_bytes: usize,
    pub allow_insecure_loopback: bool,
}

impl Default for HostedWebSocketLimits {
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

impl HostedWebSocketLimits {
    pub(crate) fn validate(self) -> Result<Self, TransportError> {
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
            return Err(TransportError::Protocol);
        }
        Ok(self)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WireFlavor {
    Cartesia,
    DeepgramV1,
    DeepgramV2,
    Inworld,
}

impl WireFlavor {
    fn provider_id(self) -> HostedTtsProviderId {
        match self {
            Self::Cartesia => HostedTtsProviderId::Cartesia,
            Self::DeepgramV1 | Self::DeepgramV2 => HostedTtsProviderId::Deepgram,
            Self::Inworld => HostedTtsProviderId::Inworld,
        }
    }
}

pub(crate) async fn connect(
    request: OpenRequest,
    flavor: WireFlavor,
    limits: HostedWebSocketLimits,
) -> Result<Box<dyn TtsConnection>, TransportError> {
    let limits = limits.validate()?;
    let websocket_request = build_upgrade_request(request, flavor, limits)?;
    let websocket_config = WebSocketConfig::default()
        .max_frame_size(Some(limits.max_frame_bytes))
        .max_message_size(Some(limits.max_message_bytes));
    let operation = connect_async_with_config(websocket_request, Some(websocket_config), false);
    let (socket, _) = tokio::time::timeout(limits.connect_timeout, operation)
        .await
        .map_err(|_| TransportError::Timeout)?
        .map_err(map_websocket_error)?;
    let mut connection = HostedWebSocketConnection {
        socket: Some(socket),
        flavor,
        limits,
        pending: VecDeque::new(),
        terminal_received: false,
        closed: false,
        cartesia_pool: None,
        cartesia_route_hash: None,
        cartesia_context_id: None,
        cartesia_wire_context_id: None,
        cartesia_context_nonce: None,
        cartesia_idle_timeout: None,
        inworld_context_id: None,
    };
    if flavor == WireFlavor::DeepgramV2 {
        connection.await_deepgram_connected().await?;
    }
    Ok(Box::new(connection))
}

pub(crate) async fn connect_cartesia_pooled(
    request: OpenRequest,
    limits: HostedWebSocketLimits,
    idle_timeout: Duration,
    pool: Arc<CartesiaSocketPool>,
) -> Result<Box<dyn TtsConnection>, TransportError> {
    let limits = limits.validate()?;
    if idle_timeout.is_zero() || idle_timeout > Duration::from_secs(300) {
        return Err(TransportError::Protocol);
    }
    let route_hash = cartesia_route_hash(&request, &pool)?;
    let started = std::time::Instant::now();
    let pooled = pool.slot.lock().await.take();
    if let Some(mut pooled) = pooled {
        if pooled.route_hash == route_hash && pooled.idle_since.elapsed() <= idle_timeout {
            pool.reused_connections.fetch_add(1, Ordering::Relaxed);
            pool.last_connection_reused.store(1, Ordering::Relaxed);
            pool.last_connect_micros
                .store(elapsed_micros(started.elapsed()), Ordering::Relaxed);
            let context_nonce = pool.next_context_nonce.fetch_add(1, Ordering::Relaxed);
            return Ok(Box::new(HostedWebSocketConnection {
                socket: Some(pooled.socket),
                flavor: WireFlavor::Cartesia,
                limits,
                pending: VecDeque::new(),
                terminal_received: false,
                closed: false,
                cartesia_pool: Some(pool),
                cartesia_route_hash: Some(route_hash),
                cartesia_context_id: None,
                cartesia_wire_context_id: None,
                cartesia_context_nonce: Some(context_nonce),
                cartesia_idle_timeout: Some(idle_timeout),
                inworld_context_id: None,
            }));
        }
        pool.stale_evictions.fetch_add(1, Ordering::Relaxed);
        let _ignored = tokio::time::timeout(limits.close_timeout, pooled.socket.close(None)).await;
    }

    let websocket_request = build_upgrade_request(request, WireFlavor::Cartesia, limits)?;
    let websocket_config = WebSocketConfig::default()
        .max_frame_size(Some(limits.max_frame_bytes))
        .max_message_size(Some(limits.max_message_bytes));
    let operation = connect_async_with_config(websocket_request, Some(websocket_config), false);
    let (socket, _) = tokio::time::timeout(limits.connect_timeout, operation)
        .await
        .map_err(|_| TransportError::Timeout)?
        .map_err(map_websocket_error)?;
    pool.fresh_connections.fetch_add(1, Ordering::Relaxed);
    pool.last_connection_reused.store(0, Ordering::Relaxed);
    pool.last_connect_micros
        .store(elapsed_micros(started.elapsed()), Ordering::Relaxed);
    let context_nonce = pool.next_context_nonce.fetch_add(1, Ordering::Relaxed);
    Ok(Box::new(HostedWebSocketConnection {
        socket: Some(socket),
        flavor: WireFlavor::Cartesia,
        limits,
        pending: VecDeque::new(),
        terminal_received: false,
        closed: false,
        cartesia_pool: Some(pool),
        cartesia_route_hash: Some(route_hash),
        cartesia_context_id: None,
        cartesia_wire_context_id: None,
        cartesia_context_nonce: Some(context_nonce),
        cartesia_idle_timeout: Some(idle_timeout),
        inworld_context_id: None,
    }))
}

fn cartesia_route_hash(
    request: &OpenRequest,
    pool: &CartesiaSocketPool,
) -> Result<u64, TransportError> {
    let secret = request
        .secret_headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("x-api-key"))
        .map(|(_, value)| value.expose_parts())
        .ok_or(TransportError::Authentication)?;
    if secret.0.is_some() || secret.1.is_empty() {
        return Err(TransportError::Authentication);
    }
    let mut route = Vec::with_capacity(5);
    route.push(request.provider_id.as_str().to_owned());
    route.push(request.endpoint.clone());
    route.extend(
        request
            .public_headers
            .iter()
            .map(|(key, value)| format!("{key}:{value}")),
    );
    route.extend(
        request
            .query
            .iter()
            .map(|(key, value)| format!("{key}:{value}")),
    );
    let public_hash = pool.hasher.hash_one(route);
    let secret_hash = pool.hasher.hash_one(secret.1.as_bytes());
    Ok(pool.hasher.hash_one((public_hash, secret_hash)))
}

fn elapsed_micros(elapsed: Duration) -> u64 {
    elapsed.as_micros().min(u128::from(u64::MAX)) as u64
}

fn build_upgrade_request(
    mut request: OpenRequest,
    flavor: WireFlavor,
    limits: HostedWebSocketLimits,
) -> Result<tokio_tungstenite::tungstenite::http::Request<()>, TransportError> {
    if request.provider_id != flavor.provider_id()
        || request.endpoint.len() > MAX_ENDPOINT_BYTES
        || request.public_headers.len() + request.secret_headers.len() > MAX_HEADER_COUNT
        || request.query.len() > MAX_QUERY_PAIRS
    {
        return Err(TransportError::Protocol);
    }
    let mut endpoint = Url::parse(&request.endpoint).map_err(|_| TransportError::Protocol)?;
    validate_endpoint(&endpoint, flavor, limits.allow_insecure_loopback)?;
    if endpoint.query().is_some() {
        return Err(TransportError::Protocol);
    }
    let cartesia_version = if flavor == WireFlavor::Cartesia {
        request
            .public_headers
            .remove("Cartesia-Version")
            .or_else(|| request.public_headers.remove("cartesia-version"))
            .ok_or(TransportError::Protocol)?
    } else {
        String::new()
    };
    {
        let mut query = endpoint.query_pairs_mut();
        if flavor == WireFlavor::Cartesia {
            if cartesia_version.len() > 32
                || !cartesia_version
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || byte == b'-')
                || cartesia_version != crate::CARTESIA_QUALIFIED_API_VERSION
            {
                return Err(TransportError::Protocol);
            }
            query.append_pair("cartesia_version", &cartesia_version);
        }
        for (key, value) in request.query {
            if key.is_empty()
                || key.len() > 128
                || value.len() > 1_024
                || !allowed_query(flavor, &key)
            {
                return Err(TransportError::Protocol);
            }
            query.append_pair(&key, &value);
        }
    }
    if endpoint.as_str().len() > MAX_UPGRADE_URL_BYTES {
        return Err(TransportError::Protocol);
    }
    let mut upgrade = endpoint
        .as_str()
        .into_client_request()
        .map_err(|_| TransportError::Protocol)?;
    for (name, value) in request.public_headers {
        let name = parse_header_name(&name)?;
        if is_websocket_managed_header(&name)
            || !allowed_public_header(flavor, name.as_str())
            || value.len() > MAX_HEADER_VALUE_BYTES
        {
            return Err(TransportError::Protocol);
        }
        let value =
            HeaderValue::from_bytes(value.as_bytes()).map_err(|_| TransportError::Protocol)?;
        upgrade.headers_mut().insert(name, value);
    }
    for (name, value) in request.secret_headers {
        let name = parse_header_name(&name)?;
        if !allowed_secret_header(flavor, name.as_str()) || upgrade.headers().contains_key(&name) {
            return Err(TransportError::Protocol);
        }
        let (scheme, secret) = value.expose_parts();
        if secret.is_empty() || scheme != required_scheme(flavor) {
            return Err(TransportError::Authentication);
        }
        let mut encoded = Zeroizing::new(Vec::with_capacity(
            secret.len() + scheme.map_or(0, |value| value.len() + 1),
        ));
        if let Some(scheme) = scheme {
            encoded.extend_from_slice(scheme.as_bytes());
            encoded.push(b' ');
        }
        encoded.extend_from_slice(secret.as_bytes());
        let header = parse_header_value(&encoded)?;
        encoded.zeroize();
        upgrade.headers_mut().insert(name, header);
    }
    let required = required_secret_header(flavor);
    if !upgrade.headers().contains_key(required) {
        return Err(TransportError::Authentication);
    }
    Ok(upgrade)
}

fn validate_endpoint(
    endpoint: &Url,
    flavor: WireFlavor,
    allow_insecure_loopback: bool,
) -> Result<(), TransportError> {
    if !endpoint.username().is_empty()
        || endpoint.password().is_some()
        || endpoint.fragment().is_some()
        || endpoint.host_str().is_none()
    {
        return Err(TransportError::Protocol);
    }
    let host = endpoint.host_str().ok_or(TransportError::Protocol)?;
    let official = match flavor {
        WireFlavor::Cartesia => "api.cartesia.ai",
        WireFlavor::DeepgramV1 | WireFlavor::DeepgramV2 => "api.deepgram.com",
        WireFlavor::Inworld => "api.inworld.ai",
    };
    let trusted = match endpoint.scheme() {
        "wss" => host == official && endpoint.port().is_none(),
        "ws" => allow_insecure_loopback && is_loopback_host(host),
        _ => false,
    };
    let path_ok = match flavor {
        WireFlavor::Cartesia => endpoint.path() == "/tts/websocket",
        WireFlavor::DeepgramV1 => endpoint.path() == "/v1/speak",
        WireFlavor::DeepgramV2 => endpoint.path() == "/v2/speak",
        WireFlavor::Inworld => endpoint.path() == "/tts/v1/voice:streamBidirectional",
    };
    if trusted && path_ok {
        Ok(())
    } else {
        Err(TransportError::Protocol)
    }
}

fn is_loopback_host(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "::1" | "[::1]")
}

fn allowed_query(flavor: WireFlavor, name: &str) -> bool {
    match flavor {
        WireFlavor::DeepgramV1 | WireFlavor::DeepgramV2 => matches!(
            name,
            "model" | "encoding" | "sample_rate" | "mip_opt_out" | "speed" | "expressivity"
        ),
        WireFlavor::Cartesia | WireFlavor::Inworld => false,
    }
}

fn allowed_public_header(flavor: WireFlavor, name: &str) -> bool {
    matches!(flavor, WireFlavor::Cartesia) && name.eq_ignore_ascii_case("cartesia-version")
}

fn allowed_secret_header(flavor: WireFlavor, name: &str) -> bool {
    match flavor {
        WireFlavor::Cartesia => name.eq_ignore_ascii_case("x-api-key"),
        WireFlavor::DeepgramV1 | WireFlavor::DeepgramV2 | WireFlavor::Inworld => {
            name.eq_ignore_ascii_case("authorization")
        }
    }
}

fn required_secret_header(flavor: WireFlavor) -> &'static str {
    match flavor {
        WireFlavor::Cartesia => "x-api-key",
        WireFlavor::DeepgramV1 | WireFlavor::DeepgramV2 | WireFlavor::Inworld => "authorization",
    }
}

fn required_scheme(flavor: WireFlavor) -> Option<&'static str> {
    match flavor {
        WireFlavor::Cartesia => None,
        WireFlavor::DeepgramV1 | WireFlavor::DeepgramV2 => Some("Token"),
        WireFlavor::Inworld => Some("Basic"),
    }
}

fn parse_header_name(value: &str) -> Result<HeaderName, TransportError> {
    HeaderName::from_bytes(value.as_bytes()).map_err(|_| TransportError::Protocol)
}

fn parse_header_value(value: &[u8]) -> Result<HeaderValue, TransportError> {
    if value.len() > MAX_HEADER_VALUE_BYTES {
        return Err(TransportError::Protocol);
    }
    HeaderValue::from_bytes(value).map_err(|_| TransportError::Authentication)
}

fn is_websocket_managed_header(name: &HeaderName) -> bool {
    matches!(
        name.as_str().to_ascii_lowercase().as_str(),
        "host"
            | "connection"
            | "upgrade"
            | "sec-websocket-key"
            | "sec-websocket-version"
            | "sec-websocket-protocol"
    )
}

struct HostedWebSocketConnection {
    socket: Option<HostedSocket>,
    flavor: WireFlavor,
    limits: HostedWebSocketLimits,
    pending: VecDeque<WireEvent>,
    terminal_received: bool,
    closed: bool,
    cartesia_pool: Option<Arc<CartesiaSocketPool>>,
    cartesia_route_hash: Option<u64>,
    cartesia_context_id: Option<String>,
    cartesia_wire_context_id: Option<String>,
    cartesia_context_nonce: Option<u64>,
    cartesia_idle_timeout: Option<Duration>,
    inworld_context_id: Option<String>,
}

#[async_trait::async_trait]
impl TtsConnection for HostedWebSocketConnection {
    async fn send(&mut self, mut command: WireCommand) -> Result<(), TransportError> {
        if self.terminal_received || self.closed {
            return Err(TransportError::Closed);
        }
        let awaits_inworld_context = matches!(
            &command,
            WireCommand::Inworld(InworldCommand::CreateContext { .. })
        );
        self.prepare_cartesia_command(&mut command)?;
        validate_inworld_command(&command)?;
        self.prepare_inworld_command(&command)?;
        let encoded = encode_command(command, self.flavor)?;
        let message = Message::Text(
            String::from_utf8(encoded.to_vec())
                .map_err(|_| TransportError::Protocol)?
                .into(),
        );
        let socket = self.socket.as_mut().ok_or(TransportError::Closed)?;
        let result = tokio::time::timeout(self.limits.io_timeout, socket.send(message))
            .await
            .map_err(|_| TransportError::Timeout)?
            .map_err(map_websocket_error);
        drop(encoded);
        result?;
        if awaits_inworld_context {
            self.await_inworld_context_ready().await?;
        }
        Ok(())
    }

    async fn receive(&mut self) -> Option<Result<WireEvent, TransportError>> {
        if let Some(event) = self.pending.pop_front() {
            if event == WireEvent::Complete {
                self.terminal_received = true;
            }
            return Some(Ok(event));
        }
        if self.terminal_received || self.closed {
            return None;
        }
        let receive_deadline = tokio::time::Instant::now() + self.limits.io_timeout;
        loop {
            let socket = match self.socket.as_mut() {
                Some(socket) => socket,
                None => return Some(Err(TransportError::Closed)),
            };
            let next = match tokio::time::timeout_at(receive_deadline, socket.next()).await {
                Err(_) => return Some(Err(TransportError::Timeout)),
                Ok(None) => return Some(Err(TransportError::Closed)),
                Ok(Some(Err(error))) => return Some(Err(map_websocket_error(error))),
                Ok(Some(Ok(message))) => message,
            };
            match next {
                Message::Binary(data)
                    if matches!(self.flavor, WireFlavor::DeepgramV1 | WireFlavor::DeepgramV2) =>
                {
                    if data.is_empty() || data.len() > MAX_AUDIO_CHUNK_BYTES {
                        return Some(Err(TransportError::ProtocolStage(
                            "deepgram_audio_chunk_invalid",
                        )));
                    }
                    return Some(Ok(WireEvent::Audio(data.to_vec())));
                }
                Message::Text(text) => {
                    let context_validation = match self.flavor {
                        WireFlavor::Cartesia => {
                            self.validate_cartesia_response_context(text.as_bytes())
                        }
                        WireFlavor::Inworld => {
                            self.validate_inworld_response_context(text.as_bytes())
                        }
                        WireFlavor::DeepgramV1 | WireFlavor::DeepgramV2 => Ok(()),
                    };
                    if let Err(error) = context_validation {
                        return Some(Err(error));
                    }
                    match decode_response(
                        text.as_bytes(),
                        self.flavor,
                        self.limits.max_message_bytes,
                    ) {
                        Ok(events) => {
                            if let Err(error) = self.extend_pending(events) {
                                return Some(Err(error));
                            }
                        }
                        Err(error) => return Some(Err(error)),
                    }
                }
                Message::Ping(payload) => {
                    let Some(socket) = self.socket.as_mut() else {
                        return Some(Err(TransportError::Closed));
                    };
                    match tokio::time::timeout_at(
                        receive_deadline,
                        socket.send(Message::Pong(payload)),
                    )
                    .await
                    {
                        Err(_) => return Some(Err(TransportError::Timeout)),
                        Ok(Err(error)) => return Some(Err(map_websocket_error(error))),
                        Ok(Ok(())) => {}
                    }
                }
                Message::Pong(_) => {}
                Message::Close(_) => return Some(Err(TransportError::Closed)),
                Message::Binary(_) | Message::Frame(_) => {
                    return Some(Err(TransportError::Protocol))
                }
            }
            if let Some(event) = self.pending.pop_front() {
                if event == WireEvent::Complete {
                    self.terminal_received = true;
                }
                return Some(Ok(event));
            }
        }
    }

    async fn close(&mut self) -> Result<(), TransportError> {
        if self.closed {
            return Ok(());
        }
        self.closed = true;
        let Some(mut socket) = self.socket.take() else {
            return Ok(());
        };
        if self.terminal_received {
            if let (Some(pool), Some(route_hash)) =
                (self.cartesia_pool.as_ref(), self.cartesia_route_hash)
            {
                let mut slot = pool.slot.lock().await;
                let idle_since = tokio::time::Instant::now();
                let idle_generation = self.cartesia_context_nonce.unwrap_or_default();
                let evicted = slot.replace(PooledCartesiaSocket {
                    socket,
                    route_hash,
                    idle_since,
                    idle_generation,
                });
                drop(slot);
                if let Some(mut evicted) = evicted {
                    let _ignored =
                        tokio::time::timeout(self.limits.close_timeout, evicted.socket.close(None))
                            .await;
                }
                if let Some(idle_timeout) = self.cartesia_idle_timeout {
                    schedule_cartesia_idle_expiry(
                        Arc::downgrade(pool),
                        idle_generation,
                        idle_timeout,
                        self.limits.close_timeout,
                    );
                }
                return Ok(());
            }
        }
        match tokio::time::timeout(self.limits.close_timeout, graceful_close(&mut socket)).await {
            Err(_) | Ok(Ok(())) => Ok(()),
            Ok(Err(WebSocketError::ConnectionClosed | WebSocketError::AlreadyClosed)) => Ok(()),
            Ok(Err(error)) => Err(map_websocket_error(error)),
        }
    }
}

fn validate_inworld_command(command: &WireCommand) -> Result<(), TransportError> {
    let WireCommand::Inworld(InworldCommand::CreateContext {
        voice_id,
        model_id,
        output,
        ..
    }) = command
    else {
        return Ok(());
    };
    let qualified_model = matches!(
        model_id.as_str(),
        crate::INWORLD_QUALIFIED_MODEL_ID | crate::INWORLD_QUALIFIED_FLASH_MODEL_ID
    );
    if qualified_model
        && voice_id == crate::INWORLD_QUALIFIED_STOCK_VOICE_ID
        && *output == crate::INWORLD_QUALIFIED_AUDIO_FORMAT
    {
        Ok(())
    } else {
        Err(TransportError::ProtocolStage("inworld_route_not_qualified"))
    }
}

fn schedule_cartesia_idle_expiry(
    pool: std::sync::Weak<CartesiaSocketPool>,
    expected_idle_generation: u64,
    idle_timeout: Duration,
    close_timeout: Duration,
) {
    tokio::spawn(async move {
        tokio::time::sleep(idle_timeout).await;
        let Some(pool) = pool.upgrade() else {
            return;
        };
        let stale = {
            let mut slot = pool.slot.lock().await;
            if slot
                .as_ref()
                .is_some_and(|socket| socket.idle_generation == expected_idle_generation)
            {
                slot.take()
            } else {
                None
            }
        };
        if let Some(mut stale) = stale {
            pool.stale_evictions.fetch_add(1, Ordering::Relaxed);
            let _ignored = tokio::time::timeout(close_timeout, stale.socket.close(None)).await;
        }
    });
}

impl HostedWebSocketConnection {
    fn extend_pending(&mut self, events: VecDeque<WireEvent>) -> Result<(), TransportError> {
        if events.len() > MAX_PENDING_WIRE_EVENTS.saturating_sub(self.pending.len()) {
            return Err(TransportError::ProtocolStage(
                "hosted_pending_event_limit_exceeded",
            ));
        }
        self.pending.extend(events);
        Ok(())
    }

    async fn await_deepgram_connected(&mut self) -> Result<(), TransportError> {
        let ready_deadline = tokio::time::Instant::now() + self.limits.io_timeout;
        loop {
            let socket = self.socket.as_mut().ok_or(TransportError::Closed)?;
            let next = tokio::time::timeout_at(ready_deadline, socket.next())
                .await
                .map_err(|_| TransportError::Timeout)?
                .ok_or(TransportError::Closed)?
                .map_err(map_websocket_error)?;
            match next {
                Message::Text(text) => {
                    let value: Value = serde_json::from_slice(text.as_bytes())
                        .map_err(|_| TransportError::Protocol)?;
                    if value.get("type").and_then(Value::as_str) == Some("Connected") {
                        return Ok(());
                    }
                    let events = decode_deepgram(&value, WireFlavor::DeepgramV2)?;
                    self.extend_pending(events)?;
                }
                Message::Ping(payload) => {
                    let socket = self.socket.as_mut().ok_or(TransportError::Closed)?;
                    tokio::time::timeout_at(ready_deadline, socket.send(Message::Pong(payload)))
                        .await
                        .map_err(|_| TransportError::Timeout)?
                        .map_err(map_websocket_error)?;
                }
                Message::Pong(_) => {}
                Message::Close(_) => return Err(TransportError::Closed),
                Message::Binary(_) | Message::Frame(_) => return Err(TransportError::Protocol),
            }
        }
    }

    fn prepare_cartesia_command(
        &mut self,
        command: &mut WireCommand,
    ) -> Result<(), TransportError> {
        if let WireCommand::Cartesia(CartesiaCommand::Generate {
            model_id,
            voice_id,
            output,
            ..
        }) = command
        {
            if model_id != crate::CARTESIA_QUALIFIED_MODEL_ID
                || voice_id != crate::CARTESIA_QUALIFIED_STOCK_VOICE_ID
                || *output != crate::CARTESIA_QUALIFIED_AUDIO_FORMAT
            {
                return Err(TransportError::ProtocolStage(
                    "cartesia_route_not_qualified",
                ));
            }
        }
        let context = match command {
            WireCommand::Cartesia(CartesiaCommand::Generate { context_id, .. })
            | WireCommand::Cartesia(CartesiaCommand::Cancel { context_id }) => context_id,
            _ => return Ok(()),
        };
        if context.is_empty() || context.len() > 480 {
            return Err(TransportError::ProtocolStage("cartesia_context_invalid"));
        }
        match self.cartesia_context_id.as_deref() {
            None => {
                self.cartesia_context_id = Some(context.clone());
                self.cartesia_wire_context_id = Some(match self.cartesia_context_nonce {
                    Some(nonce) => format!("{context}:synthesis-{nonce}"),
                    None => context.clone(),
                });
            }
            Some(active) if active == context => {}
            Some(_) => return Err(TransportError::ProtocolStage("cartesia_context_mismatch")),
        }
        *context = self
            .cartesia_wire_context_id
            .clone()
            .ok_or(TransportError::ProtocolStage("cartesia_context_invalid"))?;
        Ok(())
    }

    fn validate_cartesia_response_context(&self, bytes: &[u8]) -> Result<(), TransportError> {
        let value: Value = serde_json::from_slice(bytes).map_err(|_| TransportError::Protocol)?;
        let kind = value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let received = value.get("context_id").and_then(Value::as_str);
        if received.is_none() && kind == "error" {
            // Cartesia can reject a connection-level request before assigning a
            // context. Context-bearing errors still have to match the lease.
            return Ok(());
        }
        let received = received.ok_or(TransportError::ProtocolStage("cartesia_context_missing"))?;
        match self.cartesia_wire_context_id.as_deref() {
            Some(active) if active == received => Ok(()),
            _ => Err(TransportError::ProtocolStage("cartesia_context_mismatch")),
        }
    }

    fn prepare_inworld_command(&mut self, command: &WireCommand) -> Result<(), TransportError> {
        let context_id = match command {
            WireCommand::Inworld(InworldCommand::CreateContext { context_id, .. })
            | WireCommand::Inworld(InworldCommand::SendText { context_id, .. })
            | WireCommand::Inworld(InworldCommand::FlushContext { context_id })
            | WireCommand::Inworld(InworldCommand::CloseContext { context_id }) => context_id,
            _ => return Ok(()),
        };
        if context_id.is_empty() || context_id.len() > 480 {
            return Err(TransportError::ProtocolStage("inworld_context_invalid"));
        }
        match (&self.inworld_context_id, command) {
            (None, WireCommand::Inworld(InworldCommand::CreateContext { .. })) => {
                self.inworld_context_id = Some(context_id.clone());
                Ok(())
            }
            (Some(active), _) if active == context_id => Ok(()),
            _ => Err(TransportError::ProtocolStage("inworld_context_mismatch")),
        }
    }

    fn validate_inworld_response_context(&self, bytes: &[u8]) -> Result<(), TransportError> {
        let value: Value = serde_json::from_slice(bytes).map_err(|_| TransportError::Protocol)?;
        let received = value
            .pointer("/result/contextId")
            .and_then(Value::as_str)
            .ok_or(TransportError::ProtocolStage("inworld_context_missing"))?;
        match self.inworld_context_id.as_deref() {
            Some(active) if active == received => Ok(()),
            _ => Err(TransportError::ProtocolStage("inworld_context_mismatch")),
        }
    }

    async fn await_inworld_context_ready(&mut self) -> Result<(), TransportError> {
        let ready_deadline = tokio::time::Instant::now() + self.limits.io_timeout;
        loop {
            let socket = self.socket.as_mut().ok_or(TransportError::Closed)?;
            let next = tokio::time::timeout_at(ready_deadline, socket.next())
                .await
                .map_err(|_| TransportError::Timeout)?
                .ok_or(TransportError::Closed)?
                .map_err(map_websocket_error)?;
            match next {
                Message::Text(text) => {
                    self.validate_inworld_response_context(text.as_bytes())?;
                    let value: Value = serde_json::from_slice(text.as_bytes())
                        .map_err(|_| TransportError::Protocol)?;
                    if value.pointer("/result/contextCreated").is_some() {
                        validate_inworld_status(value.pointer("/result/status"))?;
                        return Ok(());
                    }
                    let events = decode_inworld(&value)?;
                    self.extend_pending(events)?;
                }
                Message::Ping(payload) => {
                    let socket = self.socket.as_mut().ok_or(TransportError::Closed)?;
                    tokio::time::timeout_at(ready_deadline, socket.send(Message::Pong(payload)))
                        .await
                        .map_err(|_| TransportError::Timeout)?
                        .map_err(map_websocket_error)?;
                }
                Message::Pong(_) => {}
                Message::Close(_) => return Err(TransportError::Closed),
                Message::Binary(_) | Message::Frame(_) => return Err(TransportError::Protocol),
            }
        }
    }
}

async fn graceful_close(
    socket: &mut WebSocketStream<MaybeTlsStream<TcpStream>>,
) -> Result<(), WebSocketError> {
    socket.close(None).await?;
    loop {
        match socket.next().await {
            None | Some(Ok(Message::Close(_))) => return Ok(()),
            Some(Err(WebSocketError::ConnectionClosed | WebSocketError::AlreadyClosed)) => {
                return Ok(())
            }
            Some(Err(error)) => return Err(error),
            Some(Ok(_)) => {}
        }
    }
}

#[derive(Serialize)]
struct CartesiaVoice<'a> {
    mode: &'static str,
    id: &'a str,
}

#[derive(Serialize)]
struct CartesiaOutput {
    container: &'static str,
    encoding: &'static str,
    sample_rate: u32,
}

#[derive(Serialize)]
struct CartesiaGenerate<'a> {
    model_id: &'a str,
    transcript: &'a str,
    voice: CartesiaVoice<'a>,
    language: &'a str,
    context_id: &'a str,
    output_format: CartesiaOutput,
    add_timestamps: bool,
    add_phoneme_timestamps: bool,
    r#continue: bool,
}

#[derive(Serialize)]
struct CartesiaCancel<'a> {
    context_id: &'a str,
    cancel: bool,
}

#[derive(Serialize)]
struct DeepgramSpeak<'a> {
    r#type: &'static str,
    text: &'a str,
}

#[derive(Serialize)]
struct DeepgramControl {
    r#type: &'static str,
}

#[derive(Serialize)]
struct InworldEnvelope<'a, T: Serialize> {
    #[serde(flatten)]
    command: T,
    #[serde(rename = "contextId")]
    context_id: &'a str,
}

#[derive(Serialize)]
struct InworldCreate<'a> {
    create: InworldCreateBody<'a>,
}

#[derive(Serialize)]
struct InworldCreateBody<'a> {
    #[serde(rename = "voiceId")]
    voice_id: &'a str,
    #[serde(rename = "modelId")]
    model_id: &'a str,
    #[serde(rename = "audioConfig")]
    audio_config: InworldAudioConfig,
    language: &'a str,
    #[serde(rename = "bufferCharThreshold")]
    buffer_char_threshold: u16,
    #[serde(rename = "autoMode")]
    auto_mode: bool,
    #[serde(rename = "applyTextNormalization")]
    apply_text_normalization: &'static str,
    #[serde(rename = "timestampType")]
    timestamp_type: &'static str,
    #[serde(rename = "timestampTransportStrategy")]
    timestamp_transport_strategy: &'static str,
}

#[derive(Serialize)]
struct InworldAudioConfig {
    #[serde(rename = "audioEncoding")]
    audio_encoding: &'static str,
    #[serde(rename = "sampleRateHertz")]
    sample_rate_hertz: u32,
}

#[derive(Serialize)]
struct InworldSendText<'a> {
    send_text: InworldText<'a>,
}

#[derive(Serialize)]
struct InworldText<'a> {
    text: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    flush_context: Option<InworldEmpty>,
}

#[derive(Serialize)]
struct InworldFlush {
    flush_context: InworldEmpty,
}

#[derive(Serialize)]
struct InworldClose {
    close_context: InworldEmpty,
}

#[derive(Serialize)]
struct InworldEmpty {}

fn encode_command(
    command: WireCommand,
    flavor: WireFlavor,
) -> Result<Zeroizing<Vec<u8>>, TransportError> {
    let bytes = match (flavor, command) {
        (
            WireFlavor::Cartesia,
            WireCommand::Cartesia(CartesiaCommand::Generate {
                context_id,
                model_id,
                voice_id,
                language,
                output,
                transcript,
                continue_generation,
                add_timestamps,
            }),
        ) => {
            let encoding = match output.encoding {
                PcmEncoding::PcmS16Le => "pcm_s16le",
                PcmEncoding::MuLaw => "pcm_mulaw",
                PcmEncoding::ALaw => "pcm_alaw",
            };
            serde_json::to_vec(&CartesiaGenerate {
                model_id: &model_id,
                transcript: transcript.expose(),
                voice: CartesiaVoice {
                    mode: "id",
                    id: &voice_id,
                },
                language: primary_language(&language)?,
                context_id: &context_id,
                output_format: CartesiaOutput {
                    container: "raw",
                    encoding,
                    sample_rate: output.sample_rate_hz,
                },
                add_timestamps,
                add_phoneme_timestamps: add_timestamps,
                r#continue: continue_generation,
            })
        }
        (WireFlavor::Cartesia, WireCommand::Cartesia(CartesiaCommand::Cancel { context_id })) => {
            serde_json::to_vec(&CartesiaCancel {
                context_id: &context_id,
                cancel: true,
            })
        }
        (WireFlavor::DeepgramV1 | WireFlavor::DeepgramV2, WireCommand::Deepgram(command)) => {
            match command {
                DeepgramCommand::Speak { text } => serde_json::to_vec(&DeepgramSpeak {
                    r#type: "Speak",
                    text: text.expose(),
                }),
                DeepgramCommand::Flush => serde_json::to_vec(&DeepgramControl { r#type: "Flush" }),
                DeepgramCommand::Clear if flavor == WireFlavor::DeepgramV2 => {
                    serde_json::to_vec(&DeepgramControl {
                        r#type: "Interrupt",
                    })
                }
                DeepgramCommand::Clear => serde_json::to_vec(&DeepgramControl { r#type: "Clear" }),
                DeepgramCommand::Close => serde_json::to_vec(&DeepgramControl { r#type: "Close" }),
            }
        }
        (WireFlavor::Inworld, WireCommand::Inworld(command)) => encode_inworld(command),
        _ => return Err(TransportError::Protocol),
    }
    .map_err(|_| TransportError::Protocol)?;
    if bytes.len() > MAX_COMMAND_BYTES {
        return Err(TransportError::Protocol);
    }
    Ok(Zeroizing::new(bytes))
}

fn encode_inworld(command: InworldCommand) -> Result<Vec<u8>, serde_json::Error> {
    match command {
        InworldCommand::CreateContext {
            context_id,
            voice_id,
            model_id,
            locale,
            output,
            request_alignment,
        } => {
            let encoding = match output.encoding {
                PcmEncoding::PcmS16Le => "PCM",
                PcmEncoding::MuLaw => "MULAW",
                PcmEncoding::ALaw => "ALAW",
            };
            serde_json::to_vec(&InworldEnvelope {
                command: InworldCreate {
                    create: InworldCreateBody {
                        voice_id: &voice_id,
                        model_id: &model_id,
                        audio_config: InworldAudioConfig {
                            audio_encoding: encoding,
                            sample_rate_hertz: output.sample_rate_hz,
                        },
                        language: &locale,
                        buffer_char_threshold: 100,
                        auto_mode: true,
                        apply_text_normalization: "OFF",
                        timestamp_type: if request_alignment {
                            "WORD"
                        } else {
                            "TIMESTAMP_TYPE_UNSPECIFIED"
                        },
                        timestamp_transport_strategy: if request_alignment {
                            "ASYNC"
                        } else {
                            "TIMESTAMP_TRANSPORT_STRATEGY_UNSPECIFIED"
                        },
                    },
                },
                context_id: &context_id,
            })
        }
        InworldCommand::SendText {
            context_id,
            text,
            flush_context,
        } => serde_json::to_vec(&InworldEnvelope {
            command: InworldSendText {
                send_text: InworldText {
                    text: text.expose(),
                    flush_context: flush_context.then_some(InworldEmpty {}),
                },
            },
            context_id: &context_id,
        }),
        InworldCommand::FlushContext { context_id } => serde_json::to_vec(&InworldEnvelope {
            command: InworldFlush {
                flush_context: InworldEmpty {},
            },
            context_id: &context_id,
        }),
        InworldCommand::CloseContext { context_id } => serde_json::to_vec(&InworldEnvelope {
            command: InworldClose {
                close_context: InworldEmpty {},
            },
            context_id: &context_id,
        }),
    }
}

fn primary_language(locale: &str) -> Result<&str, TransportError> {
    let primary = locale.split(['-', '_']).next().unwrap_or_default();
    if (2..=3).contains(&primary.len()) && primary.bytes().all(|byte| byte.is_ascii_alphabetic()) {
        Ok(primary)
    } else {
        Err(TransportError::ProtocolStage("cartesia_language_invalid"))
    }
}

fn decode_response(
    bytes: &[u8],
    flavor: WireFlavor,
    max_bytes: usize,
) -> Result<VecDeque<WireEvent>, TransportError> {
    if bytes.is_empty() || bytes.len() > max_bytes {
        return Err(TransportError::Protocol);
    }
    let value: Value = serde_json::from_slice(bytes).map_err(|_| TransportError::Protocol)?;
    match flavor {
        WireFlavor::Cartesia => decode_cartesia(&value),
        WireFlavor::DeepgramV1 | WireFlavor::DeepgramV2 => decode_deepgram(&value, flavor),
        WireFlavor::Inworld => decode_inworld(&value),
    }
}

fn decode_cartesia(value: &Value) -> Result<VecDeque<WireEvent>, TransportError> {
    let kind = string_field(value, "type")?;
    let mut events = VecDeque::new();
    match kind {
        "chunk" => events.push_back(WireEvent::Audio(decode_audio(value.get("data"))?)),
        "timestamps" => events.push_back(WireEvent::Alignment(parse_cartesia_words(value)?)),
        "phoneme_timestamps" => {
            events.push_back(WireEvent::Viseme(parse_cartesia_phonemes(value)?))
        }
        // Manual flush acknowledgements delimit transcript submissions but do
        // not complete the context. Keep waiting under the same receive deadline;
        // only the documented `done` frame is terminal.
        "flush_done" => {}
        "done" => events.push_back(WireEvent::Complete),
        "error" => return Err(classify_provider_error(value, "cartesia")),
        _ => return Err(TransportError::ProtocolStage("cartesia_message_unknown")),
    }
    Ok(events)
}

fn parse_cartesia_words(value: &Value) -> Result<Vec<WordAlignment>, TransportError> {
    let alignment = value
        .get("word_timestamps")
        .and_then(Value::as_object)
        .ok_or(TransportError::ProtocolStage(
            "cartesia_alignment_shape_invalid",
        ))?;
    parallel_words(
        alignment.get("words"),
        alignment.get("start"),
        alignment.get("end"),
        "cartesia_alignment_shape_invalid",
    )
}

fn parse_cartesia_phonemes(value: &Value) -> Result<Vec<VisemeEvent>, TransportError> {
    let timing = value
        .get("phoneme_timestamps")
        .and_then(Value::as_object)
        .ok_or(TransportError::ProtocolStage(
            "cartesia_phoneme_shape_invalid",
        ))?;
    parallel_visemes(
        timing.get("phonemes"),
        timing.get("start"),
        timing.get("end"),
        "cartesia_phoneme_shape_invalid",
    )
}

fn decode_deepgram(
    value: &Value,
    flavor: WireFlavor,
) -> Result<VecDeque<WireEvent>, TransportError> {
    let kind = string_field(value, "type")?;
    let mut events = VecDeque::new();
    match kind {
        "Metadata" | "Connected" | "SpeechStarted" | "Cleared" | "ConfigureSuccess" => {}
        "Flushed" if flavor == WireFlavor::DeepgramV1 => events.push_back(WireEvent::FlushComplete),
        // Flux v2 emits `Flushed` before its final audio bookkeeping. The
        // subsequent `SpeechMetadata` is the terminal event for that turn.
        "Flushed" => {}
        "SpeechMetadata" => {
            let processed = value
                .pointer("/metadata/billable_character_count")
                .or_else(|| value.get("billable_character_count"))
                .and_then(Value::as_u64)
                .unwrap_or(0);
            let request_id = value
                .get("speech_id")
                .or_else(|| value.get("request_id"))
                .and_then(Value::as_str)
                .filter(|id| id.len() <= 256)
                .map(str::to_owned);
            events.push_back(WireEvent::Usage(UsageEvent {
                processed_characters: processed,
                provider_request_id: request_id,
            }));
            events.push_back(WireEvent::Complete);
        }
        "SpeechInterrupted" => {
            return Err(TransportError::ProtocolStage("deepgram_speech_interrupted"))
        }
        // Provider warnings carry no PCM or timing. Consume them inside this
        // receive operation so warning spam cannot reset its total deadline.
        "Warning" => {}
        "ConfigureFailure" => {
            return Err(TransportError::ProtocolStage(
                "deepgram_configuration_rejected",
            ))
        }
        "Error" => return Err(classify_provider_error(value, "deepgram")),
        "SessionMetadata" if flavor == WireFlavor::DeepgramV2 => {}
        "Close" => events.push_back(WireEvent::Complete),
        _ => return Err(TransportError::ProtocolStage("deepgram_message_unknown")),
    }
    Ok(events)
}

fn decode_inworld(value: &Value) -> Result<VecDeque<WireEvent>, TransportError> {
    let result = value
        .get("result")
        .and_then(Value::as_object)
        .ok_or(TransportError::ProtocolStage("inworld_envelope_invalid"))?;
    if let Some(status) = result.get("status") {
        let code = status.get("code").and_then(Value::as_i64).unwrap_or(0);
        if code != 0 {
            return Err(classify_inworld_status(code));
        }
    }
    let mut events = VecDeque::new();
    if let Some(chunk) = result.get("audioChunk") {
        validate_inworld_status(chunk.get("status"))?;
        if let Some(audio) = chunk
            .get("audioContent")
            .filter(|value| value.as_str().is_some_and(|encoded| !encoded.is_empty()))
        {
            events.push_back(WireEvent::Audio(decode_audio(Some(audio))?));
        }
        if let Some(timestamp) = chunk.get("timestampInfo") {
            parse_inworld_timestamps(timestamp, &mut events)?;
        }
        if let Some(usage) = chunk.get("usage") {
            events.push_back(WireEvent::Usage(UsageEvent {
                processed_characters: usage
                    .get("processedCharactersCount")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
                provider_request_id: None,
            }));
        }
    } else if let Some(timestamp) = result.get("timestampInfo") {
        parse_inworld_timestamps(timestamp, &mut events)?;
    }
    if result.contains_key("flushCompleted") || result.contains_key("contextClosed") {
        events.push_back(WireEvent::FlushComplete);
    }
    if events.is_empty() && !result.contains_key("contextCreated") {
        return Err(TransportError::ProtocolStage("inworld_message_unknown"));
    }
    Ok(events)
}

fn validate_inworld_status(status: Option<&Value>) -> Result<(), TransportError> {
    let Some(status) = status else {
        return Ok(());
    };
    let code = status.get("code").and_then(Value::as_i64).unwrap_or(0);
    if code == 0 {
        Ok(())
    } else {
        Err(classify_inworld_status(code))
    }
}

fn parse_inworld_timestamps(
    timestamp: &Value,
    events: &mut VecDeque<WireEvent>,
) -> Result<(), TransportError> {
    let word = timestamp
        .get("wordAlignment")
        .and_then(Value::as_object)
        .ok_or(TransportError::ProtocolStage(
            "inworld_alignment_shape_invalid",
        ))?;
    let words = parallel_words(
        word.get("words"),
        word.get("wordStartTimeSeconds"),
        word.get("wordEndTimeSeconds"),
        "inworld_alignment_shape_invalid",
    )?;
    let word_count = words.len();
    events.push_back(WireEvent::Alignment(words));
    if let Some(details) = word.get("phoneticDetails").and_then(Value::as_array) {
        let mut visemes = Vec::new();
        let mut previous_word_index = None;
        let mut previous_start = 0;
        let mut previous_end = 0;
        for detail in details {
            let word_index = detail
                .get("wordIndex")
                .and_then(Value::as_u64)
                .and_then(|index| usize::try_from(index).ok())
                .filter(|index| *index < word_count)
                .ok_or(TransportError::ProtocolStage(
                    "inworld_phoneme_shape_invalid",
                ))?;
            if previous_word_index.is_some_and(|previous| word_index < previous) {
                return Err(TransportError::ProtocolStage(
                    "inworld_phoneme_shape_invalid",
                ));
            }
            previous_word_index = Some(word_index);
            let phones = detail.get("phones").and_then(Value::as_array).ok_or(
                TransportError::ProtocolStage("inworld_phoneme_shape_invalid"),
            )?;
            for phone in phones {
                if visemes.len() >= MAX_VISEME_ITEMS {
                    return Err(TransportError::ProtocolStage(
                        "inworld_phoneme_shape_invalid",
                    ));
                }
                let symbol = phone.get("visemeSymbol").and_then(Value::as_str).ok_or(
                    TransportError::ProtocolStage("inworld_phoneme_shape_invalid"),
                )?;
                let start = seconds_to_ms(
                    phone.get("startTimeSeconds"),
                    "inworld_phoneme_timing_invalid",
                )?;
                let duration = seconds_to_ms(
                    phone.get("durationSeconds"),
                    "inworld_phoneme_timing_invalid",
                )?;
                if symbol.is_empty()
                    || symbol.len() > 64
                    || duration > MAX_ITEM_DURATION_MS
                    || start.saturating_add(duration) > MAX_TIMELINE_MS
                    || (!visemes.is_empty()
                        && (start < previous_start
                            || start.saturating_add(duration) < previous_end))
                {
                    return Err(TransportError::ProtocolStage(
                        "inworld_phoneme_timing_invalid",
                    ));
                }
                previous_start = start;
                previous_end = start.saturating_add(duration);
                visemes.push(VisemeEvent {
                    symbol_kind: crate::TimingSymbolKind::ProviderViseme,
                    symbol: symbol.to_owned(),
                    start_ms: start,
                    duration_ms: duration,
                });
            }
        }
        if !visemes.is_empty() {
            events.push_back(WireEvent::Viseme(visemes));
        }
    }
    Ok(())
}

fn parallel_words(
    words: Option<&Value>,
    starts: Option<&Value>,
    ends: Option<&Value>,
    code: &'static str,
) -> Result<Vec<WordAlignment>, TransportError> {
    let words = words
        .and_then(Value::as_array)
        .ok_or(TransportError::ProtocolStage(code))?;
    let starts = starts
        .and_then(Value::as_array)
        .ok_or(TransportError::ProtocolStage(code))?;
    let ends = ends
        .and_then(Value::as_array)
        .ok_or(TransportError::ProtocolStage(code))?;
    if words.len() != starts.len() || words.len() != ends.len() || words.len() > MAX_ALIGNMENT_ITEMS
    {
        return Err(TransportError::ProtocolStage(code));
    }
    let words = words
        .iter()
        .zip(starts)
        .zip(ends)
        .map(|((word, start), end)| {
            let word = word.as_str().ok_or(TransportError::ProtocolStage(code))?;
            let start_ms = seconds_to_ms(Some(start), code)?;
            let end_ms = seconds_to_ms(Some(end), code)?;
            if word.len() > 512 || end_ms < start_ms || end_ms > MAX_TIMELINE_MS {
                return Err(TransportError::ProtocolStage(code));
            }
            Ok(WordAlignment {
                word: word.to_owned(),
                start_ms,
                end_ms,
                source_text_start: None,
                source_text_length: None,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    if words
        .windows(2)
        .any(|pair| pair[1].start_ms < pair[0].start_ms || pair[1].end_ms < pair[0].end_ms)
    {
        return Err(TransportError::ProtocolStage(code));
    }
    Ok(words)
}

fn parallel_visemes(
    symbols: Option<&Value>,
    starts: Option<&Value>,
    ends: Option<&Value>,
    code: &'static str,
) -> Result<Vec<VisemeEvent>, TransportError> {
    let symbols = symbols
        .and_then(Value::as_array)
        .ok_or(TransportError::ProtocolStage(code))?;
    let starts = starts
        .and_then(Value::as_array)
        .ok_or(TransportError::ProtocolStage(code))?;
    let ends = ends
        .and_then(Value::as_array)
        .ok_or(TransportError::ProtocolStage(code))?;
    if symbols.len() != starts.len()
        || symbols.len() != ends.len()
        || symbols.len() > MAX_VISEME_ITEMS
    {
        return Err(TransportError::ProtocolStage(code));
    }
    let visemes = symbols
        .iter()
        .zip(starts)
        .zip(ends)
        .map(|((symbol, start), end)| {
            let symbol = symbol.as_str().ok_or(TransportError::ProtocolStage(code))?;
            let start_ms = seconds_to_ms(Some(start), code)?;
            let end_ms = seconds_to_ms(Some(end), code)?;
            if symbol.is_empty()
                || symbol.len() > 64
                || end_ms < start_ms
                || end_ms - start_ms > MAX_ITEM_DURATION_MS
                || end_ms > MAX_TIMELINE_MS
            {
                return Err(TransportError::ProtocolStage(code));
            }
            Ok(VisemeEvent {
                symbol_kind: crate::TimingSymbolKind::Phoneme,
                symbol: symbol.to_owned(),
                start_ms,
                duration_ms: end_ms - start_ms,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    if visemes.windows(2).any(|pair| {
        pair[1].start_ms < pair[0].start_ms
            || pair[1].start_ms.saturating_add(pair[1].duration_ms)
                < pair[0].start_ms.saturating_add(pair[0].duration_ms)
    }) {
        return Err(TransportError::ProtocolStage(code));
    }
    Ok(visemes)
}

fn seconds_to_ms(value: Option<&Value>, code: &'static str) -> Result<u64, TransportError> {
    let seconds = value
        .and_then(Value::as_f64)
        .ok_or(TransportError::ProtocolStage(code))?;
    if !seconds.is_finite() || seconds < 0.0 || seconds > MAX_TIMELINE_MS as f64 / 1_000.0 {
        return Err(TransportError::ProtocolStage(code));
    }
    Ok((seconds * 1_000.0).round() as u64)
}

fn decode_audio(value: Option<&Value>) -> Result<Vec<u8>, TransportError> {
    let encoded = value
        .and_then(Value::as_str)
        .ok_or(TransportError::Protocol)?;
    if encoded.is_empty() || encoded.len() > MAX_AUDIO_CHUNK_BYTES.saturating_mul(2) {
        return Err(TransportError::Protocol);
    }
    let audio = BASE64
        .decode(encoded)
        .map_err(|_| TransportError::Protocol)?;
    if audio.is_empty() || audio.len() > MAX_AUDIO_CHUNK_BYTES {
        return Err(TransportError::Protocol);
    }
    Ok(audio)
}

fn string_field<'a>(value: &'a Value, field: &str) -> Result<&'a str, TransportError> {
    value
        .get(field)
        .and_then(Value::as_str)
        .ok_or(TransportError::Protocol)
}

fn classify_provider_error(value: &Value, provider: &'static str) -> TransportError {
    let status = value
        .get("status_code")
        .or_else(|| value.get("status"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let code = value
        .get("error_code")
        .or_else(|| value.get("code"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    if status == 401 || status == 403 || code.contains("auth") || code.contains("key") {
        TransportError::Authentication
    } else if status == 402 || code.contains("quota") || code.contains("credit") {
        TransportError::QuotaExceeded
    } else if status == 429 || code.contains("rate") {
        TransportError::RateLimited { retry_after: None }
    } else if code.contains("voice") {
        TransportError::ProtocolStage(match provider {
            "cartesia" => "cartesia_voice_unavailable",
            _ => "deepgram_voice_unavailable",
        })
    } else if code.contains("model") {
        TransportError::ProtocolStage(match provider {
            "cartesia" => "cartesia_model_unavailable",
            _ => "deepgram_model_unavailable",
        })
    } else {
        TransportError::ProtocolStage(match provider {
            "cartesia" => "cartesia_provider_error",
            _ => "deepgram_provider_error",
        })
    }
}

fn classify_inworld_status(code: i64) -> TransportError {
    match code {
        7 | 16 => TransportError::Authentication,
        8 => TransportError::QuotaExceeded,
        4 => TransportError::Timeout,
        14 => TransportError::Unavailable,
        _ => TransportError::ProtocolStage("inworld_provider_error"),
    }
}

fn map_websocket_error(error: WebSocketError) -> TransportError {
    match error {
        WebSocketError::Http(response) => match response.status() {
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => TransportError::Authentication,
            StatusCode::PAYMENT_REQUIRED => TransportError::QuotaExceeded,
            StatusCode::TOO_MANY_REQUESTS => TransportError::RateLimited {
                retry_after: response
                    .headers()
                    .get(RETRY_AFTER)
                    .and_then(|value| value.to_str().ok())
                    .and_then(parse_retry_after),
            },
            StatusCode::REQUEST_TIMEOUT | StatusCode::GATEWAY_TIMEOUT => TransportError::Timeout,
            status if status.is_server_error() => TransportError::Unavailable,
            _ => TransportError::Protocol,
        },
        WebSocketError::Io(error) => map_io_error(&error),
        WebSocketError::ConnectionClosed | WebSocketError::AlreadyClosed => TransportError::Closed,
        WebSocketError::Tls(_) => TransportError::Unavailable,
        WebSocketError::Capacity(_) | WebSocketError::Protocol(_) | WebSocketError::Utf8(_) => {
            TransportError::Protocol
        }
        _ => TransportError::Unavailable,
    }
}

fn parse_retry_after(value: &str) -> Option<Duration> {
    value
        .trim()
        .parse::<u64>()
        .ok()
        .filter(|seconds| *seconds <= 86_400)
        .map(Duration::from_secs)
}

fn map_io_error(error: &io::Error) -> TransportError {
    match error.kind() {
        io::ErrorKind::TimedOut => TransportError::Timeout,
        io::ErrorKind::ConnectionAborted
        | io::ErrorKind::ConnectionReset
        | io::ErrorKind::BrokenPipe
        | io::ErrorKind::UnexpectedEof => TransportError::Closed,
        _ => TransportError::Unavailable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inworld_alignment_preserves_word_and_viseme_timing() {
        let events = decode_response(br#"{"result":{"audioChunk":{"audioContent":"AAEC","usage":{"processedCharactersCount":5},"timestampInfo":{"wordAlignment":{"words":["Hello"],"wordStartTimeSeconds":[0.1],"wordEndTimeSeconds":[0.6],"phoneticDetails":[{"wordIndex":0,"phones":[{"phoneSymbol":"h","startTimeSeconds":0.1,"durationSeconds":0.12,"visemeSymbol":"cdgknstxyz"},{"phoneSymbol":"e","startTimeSeconds":0.22,"durationSeconds":0.1,"visemeSymbol":"aei"}]}]}}},"status":{"code":0}}}"#, WireFlavor::Inworld, 4096).expect("valid Inworld timing fixture");
        assert_eq!(events[0], WireEvent::Audio(vec![0, 1, 2]));
        assert_eq!(
            events[1],
            WireEvent::Alignment(vec![WordAlignment {
                word: "Hello".into(),
                start_ms: 100,
                end_ms: 600,
                source_text_start: None,
                source_text_length: None
            }])
        );
        assert_eq!(
            events[2],
            WireEvent::Viseme(vec![
                VisemeEvent {
                    symbol_kind: crate::TimingSymbolKind::ProviderViseme,
                    symbol: "cdgknstxyz".into(),
                    start_ms: 100,
                    duration_ms: 120
                },
                VisemeEvent {
                    symbol_kind: crate::TimingSymbolKind::ProviderViseme,
                    symbol: "aei".into(),
                    start_ms: 220,
                    duration_ms: 100
                },
            ])
        );
    }

    #[test]
    fn provider_error_bodies_are_classified_without_retaining_text() {
        let fixture = br#"{"type":"error","status_code":402,"message":"secret account detail","error_code":"quota_exceeded"}"#;
        let error = decode_response(fixture, WireFlavor::Cartesia, 4096)
            .expect_err("provider error fixture must fail");
        assert_eq!(error, TransportError::QuotaExceeded);
        assert!(!format!("{error:?}").contains("secret account detail"));
    }

    #[test]
    fn provider_boundary_acknowledgements_are_not_successful_completion() {
        assert!(decode_response(
            br#"{"type":"flush_done","done":false,"flush_done":true,"flush_id":1}"#,
            WireFlavor::Cartesia,
            4096,
        )
        .expect("valid Cartesia flush acknowledgement")
        .is_empty());
        assert_eq!(
            decode_response(
                br#"{"type":"SpeechInterrupted"}"#,
                WireFlavor::DeepgramV2,
                4096,
            ),
            Err(TransportError::ProtocolStage("deepgram_speech_interrupted"))
        );
        assert_eq!(
            decode_response(
                br#"{"type":"ConfigureFailure"}"#,
                WireFlavor::DeepgramV2,
                4096,
            ),
            Err(TransportError::ProtocolStage(
                "deepgram_configuration_rejected"
            ))
        );
    }

    #[test]
    fn parallel_timing_rejects_mismatched_or_unbounded_arrays() {
        let mismatched =
            br#"{"type":"timestamps","word_timestamps":{"words":["a"],"start":[],"end":[0.1]}}"#;
        assert_eq!(
            decode_response(mismatched, WireFlavor::Cartesia, 4096),
            Err(TransportError::ProtocolStage(
                "cartesia_alignment_shape_invalid"
            ))
        );
        let huge = format!(
            r#"{{"type":"phoneme_timestamps","phoneme_timestamps":{{"phonemes":["a"],"start":[0],"end":[{}]}}}}"#,
            MAX_TIMELINE_MS / 1_000 + 1
        );
        assert!(decode_response(huge.as_bytes(), WireFlavor::Cartesia, 4096).is_err());

        let non_monotonic_words = br#"{"type":"timestamps","word_timestamps":{"words":["a","b"],"start":[0.2,0.1],"end":[0.3,0.2]}}"#;
        assert_eq!(
            decode_response(non_monotonic_words, WireFlavor::Cartesia, 4096),
            Err(TransportError::ProtocolStage(
                "cartesia_alignment_shape_invalid"
            ))
        );
        let non_monotonic_phonemes = br#"{"type":"phoneme_timestamps","phoneme_timestamps":{"phonemes":["a","b"],"start":[0.2,0.1],"end":[0.3,0.2]}}"#;
        assert_eq!(
            decode_response(non_monotonic_phonemes, WireFlavor::Cartesia, 4096),
            Err(TransportError::ProtocolStage(
                "cartesia_phoneme_shape_invalid"
            ))
        );

        let invalid_inworld_word_index = br#"{"result":{"audioChunk":{"timestampInfo":{"wordAlignment":{"words":["a"],"wordStartTimeSeconds":[0.0],"wordEndTimeSeconds":[0.1],"phoneticDetails":[{"wordIndex":1,"phones":[{"visemeSymbol":"a","startTimeSeconds":0.0,"durationSeconds":0.1}]}]}}}}}"#;
        assert_eq!(
            decode_response(invalid_inworld_word_index, WireFlavor::Inworld, 4096),
            Err(TransportError::ProtocolStage(
                "inworld_phoneme_shape_invalid"
            ))
        );
    }

    #[test]
    fn command_debug_and_errors_never_expose_dialogue() {
        let command = WireCommand::Deepgram(DeepgramCommand::Speak {
            text: crate::SensitiveString::new("do not log me"),
        });
        assert!(!format!("{command:?}").contains("do not log me"));
        let bytes =
            encode_command(command, WireFlavor::DeepgramV2).expect("valid Deepgram command");
        assert!(std::str::from_utf8(&bytes)
            .expect("command JSON is UTF-8")
            .contains("do not log me"));
    }
}

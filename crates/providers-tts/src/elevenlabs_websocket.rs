//! Concrete ElevenLabs bidirectional WebSocket transport.
//!
//! The provider-neutral adapter owns session semantics. This module is only the
//! bounded wire boundary for ElevenLabs' `stream-input` protocol: it validates
//! the destination, constructs the authenticated upgrade request, serializes
//! commands, and normalizes provider frames into [`WireEvent`] values.
//!
//! This implements ElevenLabs' single-voice Text-to-Speech WebSocket. It does
//! not implement the HTTP whole-utterance endpoint, multi-context sockets,
//! voice/model discovery, Eleven v3 Text-to-Dialogue, or retries. Higher layers
//! own route selection and may only retry before audio has been delivered.

use std::{collections::VecDeque, fmt, io, time::Duration};

use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::net::TcpStream;
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
    ElevenLabsCommand, HostedTtsProviderId, OpenRequest, TransportError, TtsConnection,
    TtsTransport, WireCommand, WireEvent, WordAlignment, MAX_AUDIO_CHUNK_BYTES,
};

const MAX_ENDPOINT_BYTES: usize = 4_096;
const MAX_UPGRADE_URL_BYTES: usize = 8_192;
const MAX_HEADER_COUNT: usize = 32;
const MAX_HEADER_VALUE_BYTES: usize = 8_192;
const MAX_QUERY_PAIRS: usize = 32;
const MAX_ALIGNMENT_ITEMS: usize = 16_384;
const MAX_ALIGNMENT_TIMESTAMP_MS: u64 = 4 * 60 * 60 * 1_000;
const MAX_CHARACTER_DURATION_MS: u64 = 60_000;
const MAX_COMMAND_BYTES: usize = 128 * 1_024;
const DEFAULT_MAX_MESSAGE_BYTES: usize = 2 * 1_048_576;
const OFFICIAL_HOST: &str = "api.elevenlabs.io";

/// Resource and trust boundaries for [`ElevenLabsWebSocketTransport`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ElevenLabsWebSocketTransportConfig {
    /// Maximum time allowed for DNS, TCP, TLS and the WebSocket upgrade.
    pub connect_timeout: Duration,
    /// Maximum time allowed for one send or receive operation.
    pub io_timeout: Duration,
    /// Graceful close budget before the socket is authoritatively dropped.
    /// This is a separate barge-in boundary and may never exceed 150 ms.
    pub close_timeout: Duration,
    /// Maximum decoded WebSocket frame size accepted from the provider.
    pub max_frame_bytes: usize,
    /// Maximum reassembled WebSocket message size accepted from the provider.
    pub max_message_bytes: usize,
    /// Allows plain `ws://` only for an explicit localhost fixture server.
    pub allow_insecure_loopback: bool,
}

impl Default for ElevenLabsWebSocketTransportConfig {
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

impl ElevenLabsWebSocketTransportConfig {
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
            return Err(TransportError::Protocol);
        }
        Ok(self)
    }
}

/// A cloneable transport factory. It stores no credentials or dialogue.
#[derive(Clone)]
pub struct ElevenLabsWebSocketTransport {
    config: ElevenLabsWebSocketTransportConfig,
}

impl ElevenLabsWebSocketTransport {
    pub fn new(config: ElevenLabsWebSocketTransportConfig) -> Result<Self, TransportError> {
        Ok(Self {
            config: config.validate()?,
        })
    }
}

impl Default for ElevenLabsWebSocketTransport {
    fn default() -> Self {
        Self::new(ElevenLabsWebSocketTransportConfig::default())
            .expect("default ElevenLabs transport configuration is valid")
    }
}

impl fmt::Debug for ElevenLabsWebSocketTransport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ElevenLabsWebSocketTransport")
            .field("config", &self.config)
            .finish()
    }
}

#[async_trait]
impl TtsTransport for ElevenLabsWebSocketTransport {
    async fn connect(
        &self,
        request: OpenRequest,
    ) -> Result<Box<dyn TtsConnection>, TransportError> {
        let websocket_request = build_upgrade_request(request, self.config)?;
        let websocket_config = WebSocketConfig::default()
            .max_frame_size(Some(self.config.max_frame_bytes))
            .max_message_size(Some(self.config.max_message_bytes));
        let connect = connect_async_with_config(websocket_request, Some(websocket_config), false);
        let (socket, _response) = tokio::time::timeout(self.config.connect_timeout, connect)
            .await
            .map_err(|_| TransportError::Timeout)?
            .map_err(map_websocket_error)?;
        Ok(Box::new(ElevenLabsWebSocketConnection {
            socket: Some(socket),
            io_timeout: self.config.io_timeout,
            close_timeout: self.config.close_timeout,
            max_message_bytes: self.config.max_message_bytes,
            pending: VecDeque::new(),
            terminal_received: false,
            closed: false,
        }))
    }
}

fn build_upgrade_request(
    request: OpenRequest,
    config: ElevenLabsWebSocketTransportConfig,
) -> Result<tokio_tungstenite::tungstenite::http::Request<()>, TransportError> {
    if request.provider_id != HostedTtsProviderId::ElevenLabs
        || request.endpoint.len() > MAX_ENDPOINT_BYTES
        || request.public_headers.len() + request.secret_headers.len() > MAX_HEADER_COUNT
        || request.query.len() > MAX_QUERY_PAIRS
    {
        return Err(TransportError::Protocol);
    }

    let mut endpoint = Url::parse(&request.endpoint).map_err(|_| TransportError::Protocol)?;
    validate_endpoint(&endpoint, config.allow_insecure_loopback)?;
    if endpoint.query().is_some() {
        return Err(TransportError::Protocol);
    }
    {
        let mut query = endpoint.query_pairs_mut();
        for (key, value) in request.query {
            if key.is_empty()
                || key.len() > 128
                || value.len() > 1_024
                || !is_allowed_query_name(&key)
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
        if is_websocket_managed_header(&name) || name.as_str().eq_ignore_ascii_case("xi-api-key") {
            return Err(TransportError::Protocol);
        }
        if value.len() > MAX_HEADER_VALUE_BYTES {
            return Err(TransportError::Protocol);
        }
        let value =
            HeaderValue::from_bytes(value.as_bytes()).map_err(|_| TransportError::Protocol)?;
        upgrade.headers_mut().insert(name, value);
    }
    for (name, value) in request.secret_headers {
        let name = parse_header_name(&name)?;
        if !name.as_str().eq_ignore_ascii_case("xi-api-key")
            || upgrade.headers().contains_key(&name)
        {
            return Err(TransportError::Protocol);
        }
        let (scheme, secret) = value.expose_parts();
        if scheme.is_some() || secret.is_empty() {
            return Err(TransportError::Authentication);
        }
        let mut encoded = Zeroizing::new(Vec::with_capacity(
            secret.len() + scheme.map_or(0, |scheme| scheme.len() + 1),
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
    if !upgrade.headers().contains_key("xi-api-key") {
        return Err(TransportError::Authentication);
    }
    Ok(upgrade)
}

fn validate_endpoint(endpoint: &Url, allow_insecure_loopback: bool) -> Result<(), TransportError> {
    if endpoint.username() != ""
        || endpoint.password().is_some()
        || endpoint.fragment().is_some()
        || endpoint.host_str().is_none()
    {
        return Err(TransportError::Protocol);
    }
    let host = endpoint.host_str().ok_or(TransportError::Protocol)?;
    let trusted_transport = match endpoint.scheme() {
        "wss" => host == OFFICIAL_HOST && endpoint.port().is_none(),
        "ws" => allow_insecure_loopback && is_loopback_host(host),
        _ => false,
    };
    if !trusted_transport || !is_stream_input_path(endpoint) {
        return Err(TransportError::Protocol);
    }
    Ok(())
}

fn is_loopback_host(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "::1" | "[::1]")
}

fn is_stream_input_path(endpoint: &Url) -> bool {
    let Some(segments) = endpoint.path_segments() else {
        return false;
    };
    let segments = segments.collect::<Vec<_>>();
    segments.len() == 4
        && segments[0] == "v1"
        && segments[1] == "text-to-speech"
        && !segments[2].is_empty()
        && segments[2].len() <= 256
        && segments[3] == "stream-input"
}

fn is_allowed_query_name(name: &str) -> bool {
    matches!(
        name,
        "model_id"
            | "language_code"
            | "output_format"
            | "sync_alignment"
            | "enable_logging"
            | "inactivity_timeout"
    )
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

struct ElevenLabsWebSocketConnection {
    socket: Option<WebSocketStream<MaybeTlsStream<TcpStream>>>,
    io_timeout: Duration,
    close_timeout: Duration,
    max_message_bytes: usize,
    pending: VecDeque<WireEvent>,
    terminal_received: bool,
    closed: bool,
}

#[async_trait]
impl TtsConnection for ElevenLabsWebSocketConnection {
    async fn send(&mut self, command: WireCommand) -> Result<(), TransportError> {
        if self.terminal_received || self.closed {
            return Err(TransportError::Closed);
        }
        let encoded = encode_command(command)?;
        let message = Message::Text(
            String::from_utf8(encoded.to_vec())
                .map_err(|_| TransportError::Protocol)?
                .into(),
        );
        let socket = self.socket.as_mut().ok_or(TransportError::Closed)?;
        let result = tokio::time::timeout(self.io_timeout, socket.send(message))
            .await
            .map_err(|_| TransportError::Timeout)?
            .map_err(map_websocket_error);
        drop(encoded);
        result
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
        loop {
            let socket = match self.socket.as_mut() {
                Some(socket) => socket,
                None => return Some(Err(TransportError::Closed)),
            };
            let next = match tokio::time::timeout(self.io_timeout, socket.next()).await {
                Err(_) => return Some(Err(TransportError::Timeout)),
                Ok(None) => return Some(Err(TransportError::Closed)),
                Ok(Some(Err(error))) => return Some(Err(map_websocket_error(error))),
                Ok(Some(Ok(message))) => message,
            };
            match next {
                Message::Text(text) => {
                    match decode_response(text.as_bytes(), self.max_message_bytes) {
                        Ok(events) => self.pending.extend(events),
                        Err(error) => return Some(Err(error)),
                    }
                }
                Message::Ping(payload) => {
                    let Some(socket) = self.socket.as_mut() else {
                        return Some(Err(TransportError::Closed));
                    };
                    let pong = socket.send(Message::Pong(payload));
                    match tokio::time::timeout(self.io_timeout, pong).await {
                        Err(_) => return Some(Err(TransportError::Timeout)),
                        Ok(Err(error)) => return Some(Err(map_websocket_error(error))),
                        Ok(Ok(())) => {}
                    }
                }
                Message::Pong(_) => {}
                Message::Close(_) => return Some(Err(TransportError::Closed)),
                Message::Binary(_) | Message::Frame(_) => {
                    return Some(Err(TransportError::Protocol));
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
        let close = tokio::time::timeout(self.close_timeout, graceful_close(&mut socket)).await;
        // `socket` is dropped on every branch. A stalled graceful close therefore
        // becomes a hard transport abort within the cancellation budget.
        match close {
            Err(_) | Ok(Ok(())) => Ok(()),
            Ok(Err(WebSocketError::ConnectionClosed | WebSocketError::AlreadyClosed)) => Ok(()),
            Ok(Err(error)) => Err(map_websocket_error(error)),
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
struct InitializeMessage {
    text: &'static str,
    voice_settings: VoiceSettings,
}

#[derive(Serialize)]
struct VoiceSettings {
    stability: f32,
    similarity_boost: f32,
    speed: f32,
}

#[derive(Serialize)]
struct TextMessage<'text> {
    text: &'text str,
    try_trigger_generation: bool,
}

#[derive(Serialize)]
struct FinishMessage {
    text: &'static str,
}

fn encode_command(command: WireCommand) -> Result<Zeroizing<Vec<u8>>, TransportError> {
    let bytes = match command {
        WireCommand::ElevenLabs(ElevenLabsCommand::Initialize {
            stability,
            similarity_boost,
            speed,
        }) if stability.is_finite()
            && (0.0..=1.0).contains(&stability)
            && similarity_boost.is_finite()
            && (0.0..=1.0).contains(&similarity_boost)
            && speed.is_finite()
            && (0.7..=1.2).contains(&speed) =>
        {
            serde_json::to_vec(&InitializeMessage {
                text: " ",
                voice_settings: VoiceSettings {
                    stability,
                    similarity_boost,
                    speed,
                },
            })
        }
        WireCommand::ElevenLabs(ElevenLabsCommand::Text {
            text,
            try_trigger_generation,
        }) if !text.expose().contains('\0') => serde_json::to_vec(&TextMessage {
            text: text.expose(),
            try_trigger_generation,
        }),
        WireCommand::ElevenLabs(ElevenLabsCommand::Finish) => {
            serde_json::to_vec(&FinishMessage { text: "" })
        }
        _ => return Err(TransportError::Protocol),
    }
    .map_err(|_| TransportError::Protocol)?;
    if bytes.len() > MAX_COMMAND_BYTES {
        return Err(TransportError::Protocol);
    }
    Ok(Zeroizing::new(bytes))
}

#[derive(Deserialize)]
struct ProviderResponse {
    #[serde(default)]
    audio: Option<String>,
    #[serde(default)]
    alignment: Option<CharacterAlignment>,
    #[serde(default, alias = "normalizedAlignment")]
    normalized_alignment: Option<CharacterAlignment>,
    #[serde(default, alias = "isFinal")]
    is_final: bool,
}

#[derive(Deserialize)]
struct CharacterAlignment {
    chars: Vec<String>,
    #[serde(alias = "charStartTimesMs")]
    char_start_times_ms: Vec<u64>,
    #[serde(alias = "charDurationsMs")]
    char_durations_ms: Vec<u64>,
}

fn decode_response(
    bytes: &[u8],
    max_message_bytes: usize,
) -> Result<VecDeque<WireEvent>, TransportError> {
    if bytes.len() > max_message_bytes {
        return Err(TransportError::Protocol);
    }
    let response: ProviderResponse =
        serde_json::from_slice(bytes).map_err(|_| TransportError::Protocol)?;
    let mut events = VecDeque::new();
    if let Some(audio) = response.audio {
        let audio = Zeroizing::new(audio);
        let max_encoded = MAX_AUDIO_CHUNK_BYTES.saturating_mul(4).div_ceil(3) + 4;
        if audio.len() > max_encoded {
            return Err(TransportError::Protocol);
        }
        let decoded = BASE64
            .decode(audio.as_bytes())
            .map_err(|_| TransportError::Protocol)?;
        if decoded.len() > MAX_AUDIO_CHUNK_BYTES {
            return Err(TransportError::Protocol);
        }
        if !decoded.is_empty() {
            events.push_back(WireEvent::Audio(decoded));
        }
    }
    if let Some(alignment) = response.normalized_alignment.or(response.alignment) {
        let words = word_alignment(alignment)?;
        if !words.is_empty() {
            events.push_back(WireEvent::Alignment(words));
        }
    }
    if response.is_final {
        events.push_back(WireEvent::Complete);
    }
    if events.is_empty() {
        return Err(TransportError::Protocol);
    }
    Ok(events)
}

fn word_alignment(alignment: CharacterAlignment) -> Result<Vec<WordAlignment>, TransportError> {
    let count = alignment.chars.len();
    if count > MAX_ALIGNMENT_ITEMS
        || alignment.char_start_times_ms.len() != count
        || alignment.char_durations_ms.len() != count
        || alignment
            .chars
            .iter()
            .any(|value| value.chars().count() != 1)
    {
        return Err(TransportError::Protocol);
    }

    let mut previous_start = 0;
    let mut previous_end = 0;
    for (index, (&start, &duration)) in alignment
        .char_start_times_ms
        .iter()
        .zip(&alignment.char_durations_ms)
        .enumerate()
    {
        let Some(end) = start.checked_add(duration) else {
            return Err(TransportError::Protocol);
        };
        if start > MAX_ALIGNMENT_TIMESTAMP_MS
            || duration > MAX_CHARACTER_DURATION_MS
            || end > MAX_ALIGNMENT_TIMESTAMP_MS
            || (index > 0 && (start < previous_start || end < previous_end))
        {
            return Err(TransportError::Protocol);
        }
        previous_start = start;
        previous_end = end;
    }

    let mut words = Vec::new();
    let mut current = String::new();
    let mut start_ms = 0;
    let mut end_ms = 0;
    let mut source_start = 0;
    let mut source_length = 0;
    for (index, ((text, start), duration)) in alignment
        .chars
        .into_iter()
        .zip(alignment.char_start_times_ms)
        .zip(alignment.char_durations_ms)
        .enumerate()
    {
        let is_separator = text.chars().all(char::is_whitespace);
        if is_separator {
            push_word(
                &mut words,
                &mut current,
                start_ms,
                end_ms,
                source_start,
                source_length,
            );
            source_length = 0;
            continue;
        }
        if current.is_empty() {
            start_ms = start;
            source_start = index;
        }
        current.push_str(&text);
        end_ms = start.saturating_add(duration);
        source_length += 1;
    }
    push_word(
        &mut words,
        &mut current,
        start_ms,
        end_ms,
        source_start,
        source_length,
    );
    Ok(words)
}

fn push_word(
    words: &mut Vec<WordAlignment>,
    current: &mut String,
    start_ms: u64,
    end_ms: u64,
    source_start: usize,
    source_length: usize,
) {
    if current.is_empty() {
        return;
    }
    words.push(WordAlignment {
        word: std::mem::take(current),
        start_ms,
        end_ms: end_ms.max(start_ms),
        source_text_start: Some(source_start),
        source_text_length: Some(source_length),
    });
}

fn map_websocket_error(error: WebSocketError) -> TransportError {
    match error {
        WebSocketError::Http(response) => map_http_status(response.status(), response.headers()),
        WebSocketError::Io(error) => map_io_error(&error),
        WebSocketError::ConnectionClosed | WebSocketError::AlreadyClosed => TransportError::Closed,
        WebSocketError::Tls(_) => TransportError::Unavailable,
        WebSocketError::Capacity(_)
        | WebSocketError::Protocol(_)
        | WebSocketError::Utf8(_)
        | WebSocketError::Url(_)
        | WebSocketError::HttpFormat(_)
        | WebSocketError::AttackAttempt => TransportError::Protocol,
        _ => TransportError::Unavailable,
    }
}

fn map_http_status(
    status: StatusCode,
    headers: &tokio_tungstenite::tungstenite::http::HeaderMap,
) -> TransportError {
    match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => TransportError::Authentication,
        StatusCode::PAYMENT_REQUIRED => TransportError::QuotaExceeded,
        StatusCode::TOO_MANY_REQUESTS => TransportError::RateLimited {
            retry_after: headers
                .get(RETRY_AFTER)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<u64>().ok())
                .filter(|seconds| *seconds <= 86_400)
                .map(Duration::from_secs),
        },
        StatusCode::REQUEST_TIMEOUT | StatusCode::GATEWAY_TIMEOUT => TransportError::Timeout,
        status if status.is_server_error() => TransportError::Unavailable,
        _ => TransportError::Protocol,
    }
}

fn map_io_error(error: &io::Error) -> TransportError {
    match error.kind() {
        io::ErrorKind::TimedOut => TransportError::Timeout,
        io::ErrorKind::ConnectionRefused
        | io::ErrorKind::ConnectionReset
        | io::ErrorKind::ConnectionAborted
        | io::ErrorKind::NotConnected
        | io::ErrorKind::BrokenPipe
        | io::ErrorKind::UnexpectedEof => TransportError::Unavailable,
        _ => TransportError::Unavailable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_alignment_groups_non_whitespace_characters() {
        let words = word_alignment(CharacterAlignment {
            chars: ["H", "i", " ", "N", "P", "C"]
                .into_iter()
                .map(str::to_owned)
                .collect(),
            char_start_times_ms: vec![0, 10, 20, 30, 40, 50],
            char_durations_ms: vec![10; 6],
        })
        .expect("valid fixture alignment");
        assert_eq!(words.len(), 2);
        assert_eq!(words[0].word, "Hi");
        assert_eq!(words[0].start_ms, 0);
        assert_eq!(words[0].end_ms, 20);
        assert_eq!(words[1].word, "NPC");
        assert_eq!(words[1].source_text_start, Some(3));
    }

    #[test]
    fn word_alignment_rejects_multi_scalar_and_unbounded_timing() {
        let multi_scalar = CharacterAlignment {
            chars: vec!["ab".to_owned()],
            char_start_times_ms: vec![0],
            char_durations_ms: vec![10],
        };
        assert_eq!(word_alignment(multi_scalar), Err(TransportError::Protocol));

        let non_monotonic = CharacterAlignment {
            chars: vec!["a".to_owned(), "b".to_owned()],
            char_start_times_ms: vec![100, 90],
            char_durations_ms: vec![10, 10],
        };
        assert_eq!(word_alignment(non_monotonic), Err(TransportError::Protocol));

        let excessive_duration = CharacterAlignment {
            chars: vec!["a".to_owned()],
            char_start_times_ms: vec![0],
            char_durations_ms: vec![MAX_CHARACTER_DURATION_MS + 1],
        };
        assert_eq!(
            word_alignment(excessive_duration),
            Err(TransportError::Protocol)
        );

        let excessive_timestamp = CharacterAlignment {
            chars: vec!["a".to_owned()],
            char_start_times_ms: vec![MAX_ALIGNMENT_TIMESTAMP_MS + 1],
            char_durations_ms: vec![0],
        };
        assert_eq!(
            word_alignment(excessive_timestamp),
            Err(TransportError::Protocol)
        );
    }

    #[test]
    fn endpoint_rejects_plain_remote_websocket() {
        let endpoint = Url::parse("ws://example.com/v1/stream-input").expect("valid URL");
        assert_eq!(
            validate_endpoint(&endpoint, true),
            Err(TransportError::Protocol)
        );
    }
}

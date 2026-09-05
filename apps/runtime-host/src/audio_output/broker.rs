//! Production PCM submission to the authenticated native media broker.
//!
//! One broker lease is consumed by exactly one `AudioSink::play` call. Tokens
//! are accepted only on the authenticated sidecar request, redacted from Debug,
//! and zeroized as soon as their lease is consumed or revoked.

use std::{
    collections::{BTreeSet, VecDeque},
    fmt,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    time::{Duration, Instant},
};

use async_trait::async_trait;
use futures_util::StreamExt;
use npc_runtime_core::{
    AudioSink, PlaybackReceipt, ProviderErrorKind, RuntimeDependencyError, SpeechStream,
    SpeechStreamItem, SpeechTimingSymbolKind, TurnIdentity,
};
use serde::{de::Error as _, Deserialize, Deserializer, Serialize};
use tokio_util::sync::CancellationToken;
use zeroize::Zeroize;

pub const PLAYBACK_TRANSPORT_SCHEMA_VERSION: u32 = 2;
pub const MAX_PLAYBACK_LEASES_PER_TURN: usize = 16;
pub const MAX_PCM_CHUNK_BYTES: u32 = 64 * 1024;
const MAX_WIRE_FRAME_BYTES: usize = MAX_PCM_CHUNK_BYTES as usize + 4096;
const MAX_IDENTIFIER_BYTES: usize = 128;
const MAX_PRODUCER_ENDPOINT_BYTES: usize = 256;
const MAX_OUTPUT_ENDPOINT_ID_BYTES: usize = 1024;
const TOKEN_BYTES: usize = 32;
const ENDPOINT_PREFIX: &str = r"\\.\pipe\npc-media-playback-";
const BACKPRESSURE_RETRY: Duration = Duration::from_millis(5);
const BACKPRESSURE_TIMEOUT: Duration = Duration::from_secs(5);
const PIPE_CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(20);
const VISUAL_CUE_SCHEMA_VERSION: u32 = 1;
const VISUAL_CUE_PAYLOAD_BYTES: usize = 24;
const MAX_VISUAL_CUES_PER_STREAM: usize = 128;
const DEFAULT_VISUAL_CUE_DURATION: Duration = Duration::from_millis(80);
const VISUAL_CUE_FULL_STRENGTH_Q15: u16 = 32_767;

#[derive(PartialEq, Eq)]
pub struct PlaybackToken([u8; TOKEN_BYTES]);

impl Clone for PlaybackToken {
    fn clone(&self) -> Self {
        Self(self.0)
    }
}

impl fmt::Debug for PlaybackToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("PlaybackToken([REDACTED])")
    }
}

impl Drop for PlaybackToken {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl<'de> Deserialize<'de> for PlaybackToken {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let mut encoded = String::deserialize(deserializer)?;
        if encoded.len() != TOKEN_BYTES * 2
            || !encoded
                .as_bytes()
                .iter()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
        {
            encoded.zeroize();
            return Err(D::Error::custom(
                "playback token must be exactly 64 lowercase hexadecimal characters",
            ));
        }
        let mut token = [0_u8; TOKEN_BYTES];
        for (index, pair) in encoded.as_bytes().chunks_exact(2).enumerate() {
            token[index] = (decode_nibble(pair[0])
                .ok_or_else(|| D::Error::custom("playback token contains invalid hexadecimal"))?
                << 4)
                | decode_nibble(pair[1]).ok_or_else(|| {
                    D::Error::custom("playback token contains invalid hexadecimal")
                })?;
        }
        encoded.zeroize();
        if token.iter().all(|byte| *byte == 0) {
            token.zeroize();
            return Err(D::Error::custom("playback token cannot be all zero"));
        }
        Ok(Self(token))
    }
}

fn decode_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

#[derive(Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BrokerAudioPlaybackLease {
    pub schema_version: u32,
    pub stream_id: String,
    pub producer_endpoint: String,
    pub one_time_token: PlaybackToken,
    pub session_id: String,
    pub turn_id: String,
    pub generation: u64,
    pub sample_rate: u32,
    pub channels: u16,
    pub max_frames: u64,
    pub max_chunk_bytes: u32,
    pub expires_qpc: u64,
    pub output_selection_mode: BrokerAudioOutputSelectionMode,
    pub output_endpoint_id: String,
    pub output_endpoint_generation: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BrokerAudioOutputSelectionMode {
    SystemDefault,
    EndpointId,
}

impl BrokerAudioOutputSelectionMode {
    fn from_wire(value: u8) -> Option<Self> {
        match value {
            1 => Some(Self::SystemDefault),
            2 => Some(Self::EndpointId),
            _ => None,
        }
    }

    #[cfg(test)]
    fn as_wire(self) -> u8 {
        match self {
            Self::SystemDefault => 1,
            Self::EndpointId => 2,
        }
    }
}

impl fmt::Debug for BrokerAudioPlaybackLease {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BrokerAudioPlaybackLease")
            .field("schema_version", &self.schema_version)
            .field("stream_id", &self.stream_id)
            .field("producer_endpoint", &self.producer_endpoint)
            .field("one_time_token", &"[REDACTED]")
            .field("session_id", &self.session_id)
            .field("turn_id", &self.turn_id)
            .field("generation", &self.generation)
            .field("sample_rate", &self.sample_rate)
            .field("channels", &self.channels)
            .field("max_frames", &self.max_frames)
            .field("max_chunk_bytes", &self.max_chunk_bytes)
            .field("expires_qpc", &self.expires_qpc)
            .field("output_selection_mode", &self.output_selection_mode)
            .field("output_endpoint_id", &self.output_endpoint_id)
            .field(
                "output_endpoint_generation",
                &self.output_endpoint_generation,
            )
            .finish()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum BrokerLeaseError {
    #[error("playback lease pool exceeds the per-turn bound")]
    PoolTooLarge,
    #[error("playback lease is malformed")]
    Malformed,
    #[error("playback lease identity or PCM format does not match the admitted turn")]
    IdentityMismatch,
    #[error("playback lease stream IDs and endpoints must be unique")]
    Duplicate,
}

pub fn validate_playback_lease_pool(
    leases: &[BrokerAudioPlaybackLease],
    session_id: &str,
    turn_id: &str,
    generation: u64,
    sample_rate: u32,
    channels: u16,
) -> Result<(), BrokerLeaseError> {
    if leases.len() > MAX_PLAYBACK_LEASES_PER_TURN {
        return Err(BrokerLeaseError::PoolTooLarge);
    }
    let mut streams = BTreeSet::new();
    let mut endpoints = BTreeSet::new();
    for lease in leases {
        if lease.schema_version != PLAYBACK_TRANSPORT_SCHEMA_VERSION
            || !valid_identifier(&lease.stream_id)
            || !lease.producer_endpoint.starts_with(ENDPOINT_PREFIX)
            || lease.producer_endpoint.len() > MAX_PRODUCER_ENDPOINT_BYTES
            || lease.producer_endpoint.contains('\0')
            || lease.max_frames == 0
            || lease.max_frames > u64::from(lease.sample_rate).saturating_mul(10 * 60)
            || lease.max_chunk_bytes != MAX_PCM_CHUNK_BYTES
            || lease.expires_qpc == 0
            || lease.output_endpoint_id.is_empty()
            || lease.output_endpoint_id.len() > MAX_OUTPUT_ENDPOINT_ID_BYTES
            || lease.output_endpoint_id.contains('\0')
            || lease.output_endpoint_generation == 0
        {
            return Err(BrokerLeaseError::Malformed);
        }
        if lease.session_id != session_id
            || lease.turn_id != turn_id
            || lease.generation != generation
            || lease.sample_rate != sample_rate
            || lease.channels != channels
        {
            return Err(BrokerLeaseError::IdentityMismatch);
        }
        if !streams.insert(lease.stream_id.as_str())
            || !endpoints.insert(lease.producer_endpoint.as_str())
        {
            return Err(BrokerLeaseError::Duplicate);
        }
    }
    Ok(())
}

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_IDENTIFIER_BYTES
        && !value.contains("..")
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
}

#[derive(Clone, Debug, PartialEq)]
pub struct BrokerSubmittedPlaybackReceipt {
    pub receipt_id: String,
    pub stream_id: String,
    pub session_id: String,
    pub turn_id: String,
    pub generation: u64,
    pub source_frames: u64,
    pub device_frames: u64,
    pub source_duration: Duration,
    pub source_submission_complete: bool,
    pub endpoint_drain_complete: bool,
    pub cancelled: bool,
    pub output_selection_mode: BrokerAudioOutputSelectionMode,
    pub output_endpoint_id: String,
    pub output_endpoint_generation: u64,
    pub peak: f32,
    pub rms: f32,
}

impl BrokerSubmittedPlaybackReceipt {
    #[must_use]
    pub fn completed(&self) -> bool {
        self.source_frames > 0
            && self.device_frames > 0
            && !self.source_duration.is_zero()
            && self.source_submission_complete
            && self.endpoint_drain_complete
            && !self.cancelled
            && self.peak > 0.0
            && self.rms > 0.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum BrokerTransportError {
    #[error("playback lease is unavailable or expired")]
    LeaseUnavailable,
    #[error("playback producer endpoint could not be connected")]
    Connection,
    #[error("playback producer transport timed out")]
    Timeout,
    #[error("playback producer frame is malformed")]
    Malformed,
    #[error("playback broker rejected producer command with status {0}")]
    Remote(&'static str),
    #[error("playback broker accepted an unexpected source-frame count")]
    FrameAccounting,
    #[error("speech stream PCM format does not match its one-time lease")]
    FormatMismatch,
    #[error("speech stream audio sequence is invalid")]
    Sequence,
    #[error("speech stream ended without an exact end-of-stream PCM marker")]
    MissingEndOfStream,
    #[error("speech stream contained only silence")]
    Silent,
    #[error("speech provider stream failed before broker submission completed")]
    Provider,
    #[error("speech provider exceeded the first-audio deadline; retry or choose another provider")]
    FirstPcmDeadline,
    #[error("playback was cancelled")]
    Cancelled,
}

#[async_trait]
pub trait BrokerPlaybackTransport: Send + Sync {
    async fn play(
        &self,
        lease: BrokerAudioPlaybackLease,
        stream: SpeechStream,
        cancellation: CancellationToken,
    ) -> Result<BrokerSubmittedPlaybackReceipt, BrokerTransportError>;
}

#[derive(Clone, Debug, Default)]
pub struct NativeBrokerPlaybackTransport;

#[async_trait]
impl BrokerPlaybackTransport for NativeBrokerPlaybackTransport {
    async fn play(
        &self,
        lease: BrokerAudioPlaybackLease,
        stream: SpeechStream,
        cancellation: CancellationToken,
    ) -> Result<BrokerSubmittedPlaybackReceipt, BrokerTransportError> {
        native_play(lease, stream, cancellation).await
    }
}

#[derive(Debug)]
struct ActivePlayback {
    identity: TurnIdentity,
    cancellation: CancellationToken,
}

pub struct BrokerAudioSink<T = NativeBrokerPlaybackTransport> {
    transport: T,
    leases: Mutex<VecDeque<BrokerAudioPlaybackLease>>,
    active: Mutex<Option<ActivePlayback>>,
    exhausted: AtomicBool,
}

impl BrokerAudioSink<NativeBrokerPlaybackTransport> {
    pub fn production(leases: Vec<BrokerAudioPlaybackLease>) -> Self {
        Self::new(leases, NativeBrokerPlaybackTransport)
    }
}

impl<T> BrokerAudioSink<T> {
    pub fn new(leases: Vec<BrokerAudioPlaybackLease>, transport: T) -> Self {
        Self {
            transport,
            leases: Mutex::new(leases.into()),
            active: Mutex::new(None),
            exhausted: AtomicBool::new(false),
        }
    }

    pub fn remaining_leases(&self) -> Result<usize, RuntimeDependencyError> {
        self.leases
            .lock()
            .map(|leases| leases.len())
            .map_err(|_| RuntimeDependencyError::Internal("broker lease pool poisoned".into()))
    }

    #[must_use]
    pub fn pool_exhausted(&self) -> bool {
        self.exhausted.load(Ordering::Acquire)
    }

    /// Drops and zeroizes every unused token. Control separately revokes the
    /// corresponding native endpoints on every terminal path.
    pub fn revoke_unused(&self) -> Result<usize, RuntimeDependencyError> {
        let mut leases = self
            .leases
            .lock()
            .map_err(|_| RuntimeDependencyError::Internal("broker lease pool poisoned".into()))?;
        let revoked = leases.len();
        leases.clear();
        Ok(revoked)
    }
}

impl<T: BrokerPlaybackTransport> BrokerAudioSink<T> {
    pub async fn play_submitted(
        &self,
        identity: &TurnIdentity,
        stream: SpeechStream,
        cancellation: CancellationToken,
    ) -> Result<BrokerSubmittedPlaybackReceipt, RuntimeDependencyError> {
        let lease = self
            .leases
            .lock()
            .map_err(|_| RuntimeDependencyError::Internal("broker lease pool poisoned".into()))?
            .pop_front();
        let Some(lease) = lease else {
            self.exhausted.store(true, Ordering::Release);
            return Err(RuntimeDependencyError::Unavailable(
                "broker playback lease pool exhausted; explicit manual retry is required".into(),
            ));
        };
        if lease.session_id != identity.session_id || lease.turn_id != identity.turn_id {
            return Err(RuntimeDependencyError::Invalid(
                "broker playback lease does not match the active turn identity".into(),
            ));
        }
        let operation = cancellation.child_token();
        let _registration = self.reserve(identity, operation.clone())?;
        self.transport
            .play(lease, stream, operation)
            .await
            .map_err(map_transport_error)
    }

    fn reserve(
        &self,
        identity: &TurnIdentity,
        cancellation: CancellationToken,
    ) -> Result<ActiveRegistration<'_>, RuntimeDependencyError> {
        let mut active = self
            .active
            .lock()
            .map_err(|_| RuntimeDependencyError::Internal("broker audio state poisoned".into()))?;
        if active.is_some() {
            return Err(RuntimeDependencyError::Unavailable(
                "broker audio playback is already active".into(),
            ));
        }
        *active = Some(ActivePlayback {
            identity: identity.clone(),
            cancellation,
        });
        Ok(ActiveRegistration {
            active: &self.active,
            identity: identity.clone(),
        })
    }
}

#[async_trait]
impl<T: BrokerPlaybackTransport> AudioSink for BrokerAudioSink<T> {
    async fn play(
        &self,
        identity: &TurnIdentity,
        _sentence_id: u64,
        stream: SpeechStream,
        cancellation: CancellationToken,
    ) -> Result<PlaybackReceipt, RuntimeDependencyError> {
        let receipt = self.play_submitted(identity, stream, cancellation).await?;
        Ok(PlaybackReceipt {
            audible_frames: receipt.source_frames,
            duration: receipt.source_duration,
            completed: receipt.completed(),
        })
    }

    async fn stop(&self, identity: &TurnIdentity) -> Result<(), RuntimeDependencyError> {
        let active = self
            .active
            .lock()
            .map_err(|_| RuntimeDependencyError::Internal("broker audio state poisoned".into()))?;
        if let Some(active) = active
            .as_ref()
            .filter(|active| &active.identity == identity)
        {
            active.cancellation.cancel();
        }
        Ok(())
    }
}

impl<T> Drop for BrokerAudioSink<T> {
    fn drop(&mut self) {
        if let Ok(active) = self.active.get_mut() {
            if let Some(active) = active.take() {
                active.cancellation.cancel();
            }
        }
        if let Ok(leases) = self.leases.get_mut() {
            leases.clear();
        }
    }
}

struct ActiveRegistration<'a> {
    active: &'a Mutex<Option<ActivePlayback>>,
    identity: TurnIdentity,
}

impl Drop for ActiveRegistration<'_> {
    fn drop(&mut self) {
        if let Ok(mut active) = self.active.lock() {
            if active
                .as_ref()
                .is_some_and(|active| active.identity == self.identity)
            {
                *active = None;
            }
        }
    }
}

fn map_transport_error(error: BrokerTransportError) -> RuntimeDependencyError {
    match error {
        BrokerTransportError::Cancelled => RuntimeDependencyError::Cancelled,
        BrokerTransportError::FormatMismatch
        | BrokerTransportError::Sequence
        | BrokerTransportError::MissingEndOfStream
        | BrokerTransportError::Silent
        | BrokerTransportError::Malformed
        | BrokerTransportError::FrameAccounting => {
            RuntimeDependencyError::Invalid(error.to_string())
        }
        BrokerTransportError::LeaseUnavailable
        | BrokerTransportError::Connection
        | BrokerTransportError::Timeout
        | BrokerTransportError::FirstPcmDeadline
        | BrokerTransportError::Remote(_)
        | BrokerTransportError::Provider => RuntimeDependencyError::Unavailable(error.to_string()),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
enum ProducerCommand {
    Begin = 1,
    Chunk = 2,
    Finish = 3,
    Cancel = 4,
    VisualCue = 5,
}

fn canonical_viseme(symbol: &str) -> Option<u8> {
    let symbol = symbol.trim();
    if symbol.is_empty() {
        return None;
    }

    if symbol.bytes().all(|byte| byte.is_ascii_digit()) {
        return azure_viseme(symbol.parse().ok()?);
    }

    let canonical = symbol.to_lowercase().replace(['-', ' '], "_");
    match canonical.as_str() {
        "silence" | "silent" | "rest" | "pause" => return Some(0),
        "bilabial" => return Some(1),
        "labiodental" | "labio_dental" => return Some(2),
        "dental" => return Some(3),
        "alveolar" => return Some(4),
        "postalveolar" | "post_alveolar" => return Some(5),
        "palatal" => return Some(6),
        "velar" => return Some(7),
        "rounded" | "rounded_vowel" => return Some(8),
        "open" | "open_vowel" => return Some(9),
        "spread" | "spread_vowel" => return Some(10),
        _ => {}
    }

    match symbol {
        "θ" | "ð" => return Some(3),
        "ʃ" | "ʒ" | "tʃ" | "dʒ" => return Some(5),
        "j" | "ç" => return Some(6),
        "ŋ" | "x" | "ɣ" => return Some(7),
        "ɹ" | "ɻ" => return Some(6),
        "ʊ" | "ɔ" | "ø" | "œ" | "y" => return Some(8),
        "ɑ" | "ɐ" | "ʌ" | "ə" | "ɜ" | "ɞ" => return Some(9),
        "ɪ" | "ɛ" | "æ" => return Some(10),
        _ => {}
    }

    let phoneme = symbol
        .trim_end_matches(|character: char| character.is_ascii_digit())
        .to_ascii_uppercase();
    match phoneme.as_str() {
        "SIL" | "SP" | "PAU" | "_" => Some(0),
        "PP" | "P" | "B" | "M" => Some(1),
        "FF" | "F" | "V" => Some(2),
        "TH" | "DH" => Some(3),
        "DD" | "SS" | "NN" | "T" | "D" | "S" | "Z" | "N" | "L" => Some(4),
        "CH" | "SH" | "ZH" | "JH" => Some(5),
        "RR" | "R" | "Y" => Some(6),
        "KK" | "K" | "G" | "NG" => Some(7),
        "OH" | "OU" | "W" | "UW" | "UH" | "OW" | "OY" | "AO" | "U" | "O" => Some(8),
        "AA" | "A" | "AH" | "AW" | "AY" | "ER" => Some(9),
        "E" | "IH" | "IY" | "EH" | "EY" | "I" | "AE" => Some(10),
        _ => None,
    }
}

fn canonical_timing_symbol(alignment: &npc_runtime_core::AlignmentEvent) -> Option<u8> {
    let symbol = alignment.viseme.as_deref()?;
    match alignment.symbol_kind.as_ref() {
        None => canonical_viseme(symbol),
        Some(SpeechTimingSymbolKind::Phoneme)
            if alignment.symbol_provider_id.as_deref() == Some("cartesia")
                && alignment.symbol_model_id.as_deref() == Some("sonic-3.6") =>
        {
            canonical_cartesia_phoneme(symbol)
        }
        Some(SpeechTimingSymbolKind::ProviderViseme) => canonical_provider_viseme(symbol),
        Some(SpeechTimingSymbolKind::Phoneme) => None,
    }
}

fn canonical_cartesia_phoneme(symbol: &str) -> Option<u8> {
    let symbol = symbol.trim().to_lowercase();
    Some(match symbol.as_str() {
        "sil" | "sp" | "pau" | "_" => 0,
        "p" | "b" | "m" => 1,
        "f" | "v" => 2,
        "θ" | "ð" => 3,
        "t" | "d" | "s" | "z" | "n" | "l" => 4,
        "ʃ" | "ʒ" | "tʃ" | "dʒ" => 5,
        "j" | "ç" => 6,
        "k" | "g" | "ŋ" | "x" => 7,
        "u" | "o" | "ɔ" | "ʊ" | "w" => 8,
        "a" | "ɑ" | "æ" | "ə" | "ʌ" | "ɛ" | "ɜ" => 9,
        "i" | "ɪ" | "e" | "ei" | "eɪ" => 10,
        _ => return None,
    })
}

fn canonical_provider_viseme(symbol: &str) -> Option<u8> {
    let canonical = symbol.trim().to_lowercase().replace(['-', ' '], "_");
    Some(match canonical.as_str() {
        "silence" | "silent" | "rest" | "pause" => 0,
        "bilabial" | "pp" => 1,
        "labiodental" | "labio_dental" | "ff" => 2,
        "dental" | "th" => 3,
        "alveolar" | "dd" | "ss" | "nn" => 4,
        "postalveolar" | "post_alveolar" | "ch" => 5,
        "palatal" | "rr" => 6,
        "velar" | "kk" => 7,
        "rounded" | "rounded_vowel" | "oh" | "ou" => 8,
        "open" | "open_vowel" | "aa" => 9,
        "spread" | "spread_vowel" | "e" | "ih" => 10,
        _ => return None,
    })
}

fn azure_viseme(id: u8) -> Option<u8> {
    Some(match id {
        0 => 0,
        21 => 1,
        18 => 2,
        17 => 3,
        14 | 15 | 19 => 4,
        16 => 5,
        13 => 6,
        20 => 7,
        3 | 7 | 8 | 10 => 8,
        1 | 2 | 5 | 9 | 11 | 12 => 9,
        4 | 6 => 10,
        _ => return None,
    })
}

fn duration_to_samples(duration: Duration, sample_rate: u32) -> Option<u64> {
    let samples = duration.as_nanos().checked_mul(u128::from(sample_rate))? / 1_000_000_000_u128;
    u64::try_from(samples).ok()
}

fn encode_visual_cue(
    alignment: &npc_runtime_core::AlignmentEvent,
    lease: &BrokerAudioPlaybackLease,
) -> Option<[u8; VISUAL_CUE_PAYLOAD_BYTES]> {
    let viseme = canonical_timing_symbol(alignment)?;
    let start_sample = duration_to_samples(alignment.audio_offset, lease.sample_rate)?;
    if start_sample >= lease.max_frames {
        return None;
    }

    let duration = alignment
        .audio_duration
        .unwrap_or(DEFAULT_VISUAL_CUE_DURATION);
    if duration.is_zero() {
        return None;
    }
    let requested_duration = duration_to_samples(duration, lease.sample_rate)?;
    if requested_duration == 0 {
        return None;
    }
    let duration_samples = requested_duration.min(lease.max_frames.saturating_sub(start_sample));
    if duration_samples == 0 {
        return None;
    }

    let mut payload = [0_u8; VISUAL_CUE_PAYLOAD_BYTES];
    payload[0..4].copy_from_slice(&VISUAL_CUE_SCHEMA_VERSION.to_le_bytes());
    payload[4..12].copy_from_slice(&start_sample.to_le_bytes());
    payload[12..20].copy_from_slice(&duration_samples.to_le_bytes());
    payload[20] = viseme;
    payload[21] = 0;
    payload[22..24].copy_from_slice(&VISUAL_CUE_FULL_STRENGTH_Q15.to_le_bytes());
    Some(payload)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
enum ProducerStatus {
    Ok = 0,
    InvalidFrame = 1,
    AuthenticationFailed = 2,
    ProducerMismatch = 3,
    IdentityMismatch = 4,
    SequenceReplayed = 5,
    DeadlineExpired = 6,
    DeadlineTooFar = 7,
    InvalidState = 8,
    ChunkTooLarge = 9,
    FrameBudgetExceeded = 10,
    Backpressure = 11,
    DeviceUnavailable = 12,
    Cancelled = 13,
    DrainTimeout = 14,
}

impl ProducerStatus {
    fn parse(value: u16) -> Option<Self> {
        Some(match value {
            0 => Self::Ok,
            1 => Self::InvalidFrame,
            2 => Self::AuthenticationFailed,
            3 => Self::ProducerMismatch,
            4 => Self::IdentityMismatch,
            5 => Self::SequenceReplayed,
            6 => Self::DeadlineExpired,
            7 => Self::DeadlineTooFar,
            8 => Self::InvalidState,
            9 => Self::ChunkTooLarge,
            10 => Self::FrameBudgetExceeded,
            11 => Self::Backpressure,
            12 => Self::DeviceUnavailable,
            13 => Self::Cancelled,
            14 => Self::DrainTimeout,
            _ => return None,
        })
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::InvalidFrame => "invalid_frame",
            Self::AuthenticationFailed => "authentication_failed",
            Self::ProducerMismatch => "producer_mismatch",
            Self::IdentityMismatch => "identity_mismatch",
            Self::SequenceReplayed => "sequence_replayed",
            Self::DeadlineExpired => "deadline_expired",
            Self::DeadlineTooFar => "deadline_too_far",
            Self::InvalidState => "invalid_state",
            Self::ChunkTooLarge => "chunk_too_large",
            Self::FrameBudgetExceeded => "frame_budget_exceeded",
            Self::Backpressure => "backpressure",
            Self::DeviceUnavailable => "device_unavailable",
            Self::Cancelled => "cancelled",
            Self::DrainTimeout => "drain_timeout",
        }
    }
}

#[derive(Debug)]
struct ProducerResponse {
    status: ProducerStatus,
    response_to_sequence: u64,
    accepted_source_frames: u64,
    receipt: Option<WireReceipt>,
}

#[derive(Debug)]
struct WireReceipt {
    receipt_id: String,
    stream_id: String,
    session_id: String,
    turn_id: String,
    generation: u64,
    source_frames: u64,
    device_frames: u64,
    source_duration_micros: u64,
    source_submission_complete: bool,
    endpoint_drain_complete: bool,
    cancelled: bool,
    output_selection_mode: BrokerAudioOutputSelectionMode,
    output_endpoint_id: String,
    output_endpoint_generation: u64,
}

#[async_trait]
trait ProducerConnection: Send {
    async fn exchange(&mut self, request: Vec<u8>) -> Result<Vec<u8>, BrokerTransportError>;
    fn deadline_qpc(
        &self,
        expires_qpc: u64,
        require_unexpired_allocation: bool,
    ) -> Result<u64, BrokerTransportError>;
}

async fn run_producer_session<C: ProducerConnection>(
    connection: &mut C,
    lease: &BrokerAudioPlaybackLease,
    mut speech: SpeechStream,
    cancellation: CancellationToken,
) -> Result<BrokerSubmittedPlaybackReceipt, BrokerTransportError> {
    let mut sequence = 1_u64;
    send_expect_ok(connection, lease, ProducerCommand::Begin, sequence, &[]).await?;
    let mut source_frames = 0_u64;
    let mut last_audio_sequence = None;
    let mut saw_end_of_stream = false;
    let mut peak = 0_f32;
    let mut square_sum = 0_f64;
    let mut sample_count = 0_u64;
    let mut visual_cue_count = 0_usize;
    let mut previous_visual_cue_end = 0_u64;

    while let Some(item) = tokio::select! {
        _ = cancellation.cancelled() => {
            sequence = sequence.saturating_add(1);
            let _ = send_command(connection, lease, ProducerCommand::Cancel, sequence, &[]).await;
            return Err(BrokerTransportError::Cancelled);
        }
        item = speech.next() => item,
    } {
        let item = match item {
            Ok(item) => item,
            Err(error) => {
                cancel_best_effort(connection, lease, &mut sequence).await;
                return Err(
                    if error.kind == ProviderErrorKind::Timeout
                        && error.message == "first_pcm_deadline_exceeded"
                    {
                        BrokerTransportError::FirstPcmDeadline
                    } else {
                        BrokerTransportError::Provider
                    },
                );
            }
        };
        let chunk = match item {
            SpeechStreamItem::Alignment(alignment) => {
                let Some(payload) = encode_visual_cue(&alignment, lease) else {
                    continue;
                };
                let start_sample = u64::from_le_bytes(
                    payload[4..12]
                        .try_into()
                        .map_err(|_| BrokerTransportError::Malformed)?,
                );
                let duration_samples = u64::from_le_bytes(
                    payload[12..20]
                        .try_into()
                        .map_err(|_| BrokerTransportError::Malformed)?,
                );
                let cue_end = start_sample
                    .checked_add(duration_samples)
                    .ok_or(BrokerTransportError::Malformed)?;
                // Provider timelines are advisory visual metadata. Keep them
                // bounded and monotonic before they reach the native session;
                // malformed or overlapping cues must never cancel valid audio.
                if visual_cue_count >= MAX_VISUAL_CUES_PER_STREAM
                    || start_sample < previous_visual_cue_end
                {
                    continue;
                }
                sequence = sequence.saturating_add(1);
                if let Err(error) = send_expect_ok(
                    connection,
                    lease,
                    ProducerCommand::VisualCue,
                    sequence,
                    &payload,
                )
                .await
                {
                    cancel_best_effort(connection, lease, &mut sequence).await;
                    return Err(error);
                }
                visual_cue_count += 1;
                previous_visual_cue_end = cue_end;
                continue;
            }
            SpeechStreamItem::Audio(chunk) => chunk,
        };
        if chunk.sequence == 0
            || saw_end_of_stream
            || last_audio_sequence.is_some_and(|last| chunk.sequence <= last)
        {
            cancel_best_effort(connection, lease, &mut sequence).await;
            return Err(BrokerTransportError::Sequence);
        }
        if chunk.sample_rate_hz != lease.sample_rate
            || chunk.channels != lease.channels
            || chunk.pcm_s16le.is_empty()
            || chunk.pcm_s16le.len() % (usize::from(lease.channels) * 2) != 0
        {
            cancel_best_effort(connection, lease, &mut sequence).await;
            return Err(BrokerTransportError::FormatMismatch);
        }
        last_audio_sequence = Some(chunk.sequence);
        saw_end_of_stream = chunk.end_of_stream;
        for bytes in chunk.pcm_s16le.chunks_exact(2) {
            let sample = f32::from(i16::from_le_bytes([bytes[0], bytes[1]])) / 32_768.0;
            peak = peak.max(sample.abs());
            square_sum += f64::from(sample) * f64::from(sample);
            sample_count = sample_count.saturating_add(1);
        }
        let max_chunk =
            usize::try_from(lease.max_chunk_bytes).map_err(|_| BrokerTransportError::Malformed)?;
        let block_align = usize::from(lease.channels) * 2;
        let bounded_chunk = max_chunk - (max_chunk % block_align);
        if bounded_chunk == 0 {
            return Err(BrokerTransportError::Malformed);
        }
        for payload in chunk.pcm_s16le.chunks(bounded_chunk) {
            let started = Instant::now();
            loop {
                if cancellation.is_cancelled() {
                    cancel_best_effort(connection, lease, &mut sequence).await;
                    return Err(BrokerTransportError::Cancelled);
                }
                sequence = sequence.saturating_add(1);
                let response = match send_command(
                    connection,
                    lease,
                    ProducerCommand::Chunk,
                    sequence,
                    payload,
                )
                .await
                {
                    Ok(response) => response,
                    Err(error) => {
                        cancel_best_effort(connection, lease, &mut sequence).await;
                        return Err(error);
                    }
                };
                let frames = u64::try_from(payload.len() / block_align)
                    .map_err(|_| BrokerTransportError::FrameAccounting)?;
                if response.receipt.is_some() {
                    cancel_best_effort(connection, lease, &mut sequence).await;
                    return Err(BrokerTransportError::Malformed);
                }
                match response.status {
                    ProducerStatus::Ok if response.accepted_source_frames == frames => {
                        source_frames = source_frames
                            .checked_add(frames)
                            .ok_or(BrokerTransportError::FrameAccounting)?;
                        break;
                    }
                    ProducerStatus::Backpressure
                        if response.accepted_source_frames == 0
                            && started.elapsed() < BACKPRESSURE_TIMEOUT =>
                    {
                        tokio::select! {
                            _ = cancellation.cancelled() => {},
                            _ = tokio::time::sleep(BACKPRESSURE_RETRY) => {},
                        }
                    }
                    ProducerStatus::Backpressure => {
                        cancel_best_effort(connection, lease, &mut sequence).await;
                        return Err(BrokerTransportError::Timeout);
                    }
                    status => {
                        cancel_best_effort(connection, lease, &mut sequence).await;
                        return Err(BrokerTransportError::Remote(status.as_str()));
                    }
                }
            }
        }
    }

    if !saw_end_of_stream || source_frames == 0 {
        cancel_best_effort(connection, lease, &mut sequence).await;
        return Err(BrokerTransportError::MissingEndOfStream);
    }
    if peak == 0.0 || sample_count == 0 {
        cancel_best_effort(connection, lease, &mut sequence).await;
        return Err(BrokerTransportError::Silent);
    }
    sequence = sequence.saturating_add(1);
    let response =
        match send_command(connection, lease, ProducerCommand::Finish, sequence, &[]).await {
            Ok(response) => response,
            Err(error) => {
                cancel_best_effort(connection, lease, &mut sequence).await;
                return Err(error);
            }
        };
    if response.status != ProducerStatus::Ok {
        return Err(BrokerTransportError::Remote(response.status.as_str()));
    }
    if response.accepted_source_frames != 0 {
        return Err(BrokerTransportError::FrameAccounting);
    }
    let receipt = response.receipt.ok_or(BrokerTransportError::Malformed)?;
    let expected_duration_micros = source_frames
        .saturating_mul(1_000_000)
        .checked_div(u64::from(lease.sample_rate))
        .ok_or(BrokerTransportError::FrameAccounting)?;
    if receipt.stream_id != lease.stream_id
        || receipt.session_id != lease.session_id
        || receipt.turn_id != lease.turn_id
        || receipt.generation != lease.generation
        || receipt.source_frames != source_frames
        || receipt.device_frames != source_frames
        || receipt.source_duration_micros != expected_duration_micros
        || !receipt.source_submission_complete
        || !receipt.endpoint_drain_complete
        || receipt.cancelled
        || receipt.output_selection_mode != lease.output_selection_mode
        || receipt.output_endpoint_id != lease.output_endpoint_id
        || receipt.output_endpoint_generation != lease.output_endpoint_generation
    {
        return Err(BrokerTransportError::FrameAccounting);
    }
    Ok(BrokerSubmittedPlaybackReceipt {
        receipt_id: receipt.receipt_id,
        stream_id: receipt.stream_id,
        session_id: receipt.session_id,
        turn_id: receipt.turn_id,
        generation: receipt.generation,
        source_frames: receipt.source_frames,
        device_frames: receipt.device_frames,
        source_duration: Duration::from_micros(receipt.source_duration_micros),
        source_submission_complete: receipt.source_submission_complete,
        endpoint_drain_complete: receipt.endpoint_drain_complete,
        cancelled: receipt.cancelled,
        output_selection_mode: receipt.output_selection_mode,
        output_endpoint_id: receipt.output_endpoint_id,
        output_endpoint_generation: receipt.output_endpoint_generation,
        peak,
        rms: (square_sum / sample_count as f64).sqrt() as f32,
    })
}

async fn send_expect_ok<C: ProducerConnection>(
    connection: &mut C,
    lease: &BrokerAudioPlaybackLease,
    command: ProducerCommand,
    sequence: u64,
    payload: &[u8],
) -> Result<ProducerResponse, BrokerTransportError> {
    let response = send_command(connection, lease, command, sequence, payload).await?;
    if response.status != ProducerStatus::Ok
        || response.accepted_source_frames != 0
        || response.receipt.is_some()
    {
        return Err(BrokerTransportError::Remote(response.status.as_str()));
    }
    Ok(response)
}

async fn send_command<C: ProducerConnection>(
    connection: &mut C,
    lease: &BrokerAudioPlaybackLease,
    command: ProducerCommand,
    sequence: u64,
    payload: &[u8],
) -> Result<ProducerResponse, BrokerTransportError> {
    let deadline = connection.deadline_qpc(lease.expires_qpc, command == ProducerCommand::Begin)?;
    let frame = encode_request(lease, command, sequence, deadline, payload)?;
    let response = connection.exchange(frame).await?;
    let response = decode_response(&response)?;
    if response.response_to_sequence != sequence {
        return Err(BrokerTransportError::Malformed);
    }
    Ok(response)
}

async fn cancel_best_effort<C: ProducerConnection>(
    connection: &mut C,
    lease: &BrokerAudioPlaybackLease,
    sequence: &mut u64,
) {
    *sequence = sequence.saturating_add(1);
    let _ = send_command(connection, lease, ProducerCommand::Cancel, *sequence, &[]).await;
}

fn encode_request(
    lease: &BrokerAudioPlaybackLease,
    command: ProducerCommand,
    sequence: u64,
    deadline_qpc: u64,
    payload: &[u8],
) -> Result<Vec<u8>, BrokerTransportError> {
    if sequence == 0
        || deadline_qpc == 0
        || payload.len() > MAX_PCM_CHUNK_BYTES as usize
        || match command {
            ProducerCommand::Chunk => false,
            ProducerCommand::VisualCue => payload.len() != VISUAL_CUE_PAYLOAD_BYTES,
            ProducerCommand::Begin | ProducerCommand::Finish | ProducerCommand::Cancel => {
                !payload.is_empty()
            }
        }
    {
        return Err(BrokerTransportError::Malformed);
    }
    let mut body = Vec::with_capacity(128 + payload.len());
    body.extend_from_slice(b"NPCP");
    push_u32(&mut body, PLAYBACK_TRANSPORT_SCHEMA_VERSION);
    push_u16(&mut body, command as u16);
    push_u16(&mut body, 0);
    push_u64(&mut body, sequence);
    push_u64(&mut body, deadline_qpc);
    push_u64(&mut body, lease.generation);
    push_string(&mut body, &lease.stream_id)?;
    push_string(&mut body, &lease.session_id)?;
    push_string(&mut body, &lease.turn_id)?;
    body.extend_from_slice(&lease.one_time_token.0);
    push_u32(
        &mut body,
        u32::try_from(payload.len()).map_err(|_| BrokerTransportError::Malformed)?,
    );
    body.extend_from_slice(payload);
    if body.len() > MAX_WIRE_FRAME_BYTES {
        return Err(BrokerTransportError::Malformed);
    }
    let mut frame = Vec::with_capacity(body.len() + 4);
    push_u32(
        &mut frame,
        u32::try_from(body.len()).map_err(|_| BrokerTransportError::Malformed)?,
    );
    frame.extend_from_slice(&body);
    Ok(frame)
}

fn decode_response(frame: &[u8]) -> Result<ProducerResponse, BrokerTransportError> {
    if frame.len() < 4 {
        return Err(BrokerTransportError::Malformed);
    }
    let mut prefix_position = 0;
    let body_size = usize::try_from(read_u32(frame, &mut prefix_position)?)
        .map_err(|_| BrokerTransportError::Malformed)?;
    if body_size == 0 || body_size > MAX_WIRE_FRAME_BYTES || frame.len() != body_size + 4 {
        return Err(BrokerTransportError::Malformed);
    }
    let body = &frame[4..];
    if body.len() < 28 || &body[..4] != b"NPCR" {
        return Err(BrokerTransportError::Malformed);
    }
    let mut position = 4;
    if read_u32(body, &mut position)? != PLAYBACK_TRANSPORT_SCHEMA_VERSION {
        return Err(BrokerTransportError::Malformed);
    }
    let status = ProducerStatus::parse(read_u16(body, &mut position)?)
        .ok_or(BrokerTransportError::Malformed)?;
    let has_receipt = read_u16(body, &mut position)?;
    if has_receipt > 1 {
        return Err(BrokerTransportError::Malformed);
    }
    let response_to_sequence = read_u64(body, &mut position)?;
    let accepted_source_frames = read_u64(body, &mut position)?;
    let receipt = if has_receipt == 1 {
        Some(read_receipt(body, &mut position)?)
    } else {
        None
    };
    if position != body.len() || response_to_sequence == 0 {
        return Err(BrokerTransportError::Malformed);
    }
    Ok(ProducerResponse {
        status,
        response_to_sequence,
        accepted_source_frames,
        receipt,
    })
}

fn read_receipt(body: &[u8], position: &mut usize) -> Result<WireReceipt, BrokerTransportError> {
    if read_u32(body, position)? != PLAYBACK_TRANSPORT_SCHEMA_VERSION {
        return Err(BrokerTransportError::Malformed);
    }
    let receipt_id = read_string(body, position)?;
    let stream_id = read_string(body, position)?;
    let session_id = read_string(body, position)?;
    let turn_id = read_string(body, position)?;
    let generation = read_u64(body, position)?;
    let source_frames = read_u64(body, position)?;
    let device_frames = read_u64(body, position)?;
    let source_duration_micros = read_u64(body, position)?;
    if body.len().saturating_sub(*position) < 3 {
        return Err(BrokerTransportError::Malformed);
    }
    let source_submission_complete = read_bool(body[*position])?;
    *position += 1;
    let endpoint_drain_complete = read_bool(body[*position])?;
    *position += 1;
    let cancelled = read_bool(body[*position])?;
    *position += 1;
    let output_selection_mode = BrokerAudioOutputSelectionMode::from_wire(
        *body.get(*position).ok_or(BrokerTransportError::Malformed)?,
    )
    .ok_or(BrokerTransportError::Malformed)?;
    *position += 1;
    let output_endpoint_id = read_bounded_utf8(body, position, MAX_OUTPUT_ENDPOINT_ID_BYTES)?;
    let output_endpoint_generation = read_u64(body, position)?;
    if output_endpoint_generation == 0 {
        return Err(BrokerTransportError::Malformed);
    }
    Ok(WireReceipt {
        receipt_id,
        stream_id,
        session_id,
        turn_id,
        generation,
        source_frames,
        device_frames,
        source_duration_micros,
        source_submission_complete,
        endpoint_drain_complete,
        cancelled,
        output_selection_mode,
        output_endpoint_id,
        output_endpoint_generation,
    })
}

fn read_bool(value: u8) -> Result<bool, BrokerTransportError> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(BrokerTransportError::Malformed),
    }
}

fn push_u16(output: &mut Vec<u8>, value: u16) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn push_u32(output: &mut Vec<u8>, value: u32) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn push_u64(output: &mut Vec<u8>, value: u64) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn push_string(output: &mut Vec<u8>, value: &str) -> Result<(), BrokerTransportError> {
    if !valid_identifier(value) {
        return Err(BrokerTransportError::Malformed);
    }
    push_u16(
        output,
        u16::try_from(value.len()).map_err(|_| BrokerTransportError::Malformed)?,
    );
    output.extend_from_slice(value.as_bytes());
    Ok(())
}

#[cfg(test)]
fn push_bounded_utf8(output: &mut Vec<u8>, value: &str) -> Result<(), BrokerTransportError> {
    if value.is_empty() || value.len() > MAX_OUTPUT_ENDPOINT_ID_BYTES || value.contains('\0') {
        return Err(BrokerTransportError::Malformed);
    }
    push_u16(
        output,
        u16::try_from(value.len()).map_err(|_| BrokerTransportError::Malformed)?,
    );
    output.extend_from_slice(value.as_bytes());
    Ok(())
}

fn read_u16(input: &[u8], position: &mut usize) -> Result<u16, BrokerTransportError> {
    let end = position
        .checked_add(2)
        .ok_or(BrokerTransportError::Malformed)?;
    let bytes: [u8; 2] = input
        .get(*position..end)
        .ok_or(BrokerTransportError::Malformed)?
        .try_into()
        .map_err(|_| BrokerTransportError::Malformed)?;
    *position = end;
    Ok(u16::from_le_bytes(bytes))
}

fn read_u32(input: &[u8], position: &mut usize) -> Result<u32, BrokerTransportError> {
    let end = position
        .checked_add(4)
        .ok_or(BrokerTransportError::Malformed)?;
    let bytes: [u8; 4] = input
        .get(*position..end)
        .ok_or(BrokerTransportError::Malformed)?
        .try_into()
        .map_err(|_| BrokerTransportError::Malformed)?;
    *position = end;
    Ok(u32::from_le_bytes(bytes))
}

fn read_u64(input: &[u8], position: &mut usize) -> Result<u64, BrokerTransportError> {
    let end = position
        .checked_add(8)
        .ok_or(BrokerTransportError::Malformed)?;
    let bytes: [u8; 8] = input
        .get(*position..end)
        .ok_or(BrokerTransportError::Malformed)?
        .try_into()
        .map_err(|_| BrokerTransportError::Malformed)?;
    *position = end;
    Ok(u64::from_le_bytes(bytes))
}

fn read_string(input: &[u8], position: &mut usize) -> Result<String, BrokerTransportError> {
    let length = usize::from(read_u16(input, position)?);
    if length == 0 || length > MAX_IDENTIFIER_BYTES {
        return Err(BrokerTransportError::Malformed);
    }
    let end = position
        .checked_add(length)
        .ok_or(BrokerTransportError::Malformed)?;
    let bytes = input
        .get(*position..end)
        .ok_or(BrokerTransportError::Malformed)?;
    let value = std::str::from_utf8(bytes).map_err(|_| BrokerTransportError::Malformed)?;
    if !valid_identifier(value) {
        return Err(BrokerTransportError::Malformed);
    }
    *position = end;
    Ok(value.to_owned())
}

fn read_bounded_utf8(
    input: &[u8],
    position: &mut usize,
    maximum_bytes: usize,
) -> Result<String, BrokerTransportError> {
    let length = usize::from(read_u16(input, position)?);
    if length == 0 || length > maximum_bytes {
        return Err(BrokerTransportError::Malformed);
    }
    let end = position
        .checked_add(length)
        .ok_or(BrokerTransportError::Malformed)?;
    let bytes = input
        .get(*position..end)
        .ok_or(BrokerTransportError::Malformed)?;
    let value = std::str::from_utf8(bytes).map_err(|_| BrokerTransportError::Malformed)?;
    if value.contains('\0') {
        return Err(BrokerTransportError::Malformed);
    }
    *position = end;
    Ok(value.to_owned())
}

#[cfg(windows)]
async fn native_play(
    lease: BrokerAudioPlaybackLease,
    stream: SpeechStream,
    cancellation: CancellationToken,
) -> Result<BrokerSubmittedPlaybackReceipt, BrokerTransportError> {
    let mut connection = WindowsProducerConnection::connect(&lease, &cancellation).await?;
    run_producer_session(&mut connection, &lease, stream, cancellation).await
}

#[cfg(not(windows))]
async fn native_play(
    _lease: BrokerAudioPlaybackLease,
    _stream: SpeechStream,
    _cancellation: CancellationToken,
) -> Result<BrokerSubmittedPlaybackReceipt, BrokerTransportError> {
    Err(BrokerTransportError::Connection)
}

#[cfg(windows)]
struct WindowsProducerConnection {
    pipe: tokio::net::windows::named_pipe::NamedPipeClient,
    qpc_frequency: u64,
}

#[cfg(windows)]
impl WindowsProducerConnection {
    async fn connect(
        lease: &BrokerAudioPlaybackLease,
        cancellation: &CancellationToken,
    ) -> Result<Self, BrokerTransportError> {
        use tokio::net::windows::named_pipe::ClientOptions;

        let started = Instant::now();
        let pipe = loop {
            if cancellation.is_cancelled() {
                return Err(BrokerTransportError::Cancelled);
            }
            match ClientOptions::new().open(&lease.producer_endpoint) {
                Ok(pipe) => break pipe,
                Err(error)
                    if matches!(error.raw_os_error(), Some(2 | 231))
                        && started.elapsed() < PIPE_CONNECT_TIMEOUT =>
                {
                    tokio::select! {
                        _ = cancellation.cancelled() => {
                            return Err(BrokerTransportError::Cancelled);
                        }
                        _ = tokio::time::sleep(Duration::from_millis(10)) => {}
                    }
                }
                Err(_) => return Err(BrokerTransportError::Connection),
            }
        };
        let (_, frequency) = qpc_now_and_frequency()?;
        Ok(Self {
            pipe,
            qpc_frequency: frequency,
        })
    }
}

#[cfg(windows)]
#[async_trait]
impl ProducerConnection for WindowsProducerConnection {
    async fn exchange(&mut self, request: Vec<u8>) -> Result<Vec<u8>, BrokerTransportError> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        tokio::time::timeout(RESPONSE_TIMEOUT, async {
            self.pipe
                .write_all(&request)
                .await
                .map_err(|_| BrokerTransportError::Connection)?;
            self.pipe
                .flush()
                .await
                .map_err(|_| BrokerTransportError::Connection)?;
            let mut prefix = [0_u8; 4];
            self.pipe
                .read_exact(&mut prefix)
                .await
                .map_err(|_| BrokerTransportError::Connection)?;
            let body_size = u32::from_le_bytes(prefix) as usize;
            if body_size == 0 || body_size > MAX_WIRE_FRAME_BYTES {
                return Err(BrokerTransportError::Malformed);
            }
            let mut frame = Vec::with_capacity(body_size + 4);
            frame.extend_from_slice(&prefix);
            frame.resize(body_size + 4, 0);
            self.pipe
                .read_exact(&mut frame[4..])
                .await
                .map_err(|_| BrokerTransportError::Connection)?;
            Ok(frame)
        })
        .await
        .map_err(|_| BrokerTransportError::Timeout)?
    }

    fn deadline_qpc(
        &self,
        expires_qpc: u64,
        require_unexpired_allocation: bool,
    ) -> Result<u64, BrokerTransportError> {
        let (now, _) = qpc_now_and_frequency()?;
        if now == 0 || (require_unexpired_allocation && now > expires_qpc) {
            return Err(BrokerTransportError::LeaseUnavailable);
        }
        Ok(now.saturating_add(self.qpc_frequency.saturating_mul(4)))
    }
}

#[cfg(windows)]
fn qpc_now_and_frequency() -> Result<(u64, u64), BrokerTransportError> {
    let mut now = 0_i64;
    let mut frequency = 0_i64;
    // SAFETY: both Windows APIs write to valid stack-owned outputs.
    let success = unsafe {
        windows_sys::Win32::System::Performance::QueryPerformanceCounter(&mut now) != 0
            && windows_sys::Win32::System::Performance::QueryPerformanceFrequency(&mut frequency)
                != 0
    };
    if !success || now <= 0 || frequency <= 0 {
        return Err(BrokerTransportError::Connection);
    }
    Ok((now as u64, frequency as u64))
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::stream;
    use npc_runtime_core::{AlignmentEvent, AudioChunk, ProviderError, SpeechStreamItem};
    use std::{collections::VecDeque, sync::Arc};

    fn lease(index: usize) -> BrokerAudioPlaybackLease {
        let token = format!("{:064x}", index + 1);
        serde_json::from_value(serde_json::json!({
            "schemaVersion": 2,
            "streamId": format!("pcm-{index:03}"),
            "producerEndpoint": format!(r"\\.\pipe\npc-media-playback-test-{index:03}"),
            "oneTimeToken": token,
            "sessionId": "session-1",
            "turnId": "turn-1",
            "generation": 7,
            "sampleRate": 24_000,
            "channels": 1,
            "maxFrames": 24_000,
            "maxChunkBytes": 65_536,
            "expiresQpc": 10_000,
            "outputSelectionMode": "systemDefault",
            "outputEndpointId": "{0.0.0.00000000}.fixture-output",
            "outputEndpointGeneration": 17
        }))
        .expect("valid broker lease")
    }

    fn speech(samples: &[i16]) -> SpeechStream {
        let pcm = samples
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect::<Vec<_>>();
        Box::pin(stream::iter(vec![Ok::<_, ProviderError>(
            SpeechStreamItem::Audio(AudioChunk {
                sequence: 1,
                pcm_s16le: pcm,
                sample_rate_hz: 24_000,
                channels: 1,
                end_of_stream: true,
            }),
        )]))
    }

    #[derive(Debug)]
    struct ScriptedConnection {
        statuses: VecDeque<ProducerStatus>,
        accepted_frames: u64,
        device_frames_override: Option<u64>,
        lease: BrokerAudioPlaybackLease,
        commands: Vec<(ProducerCommand, u64, Vec<u8>)>,
    }

    #[async_trait]
    impl ProducerConnection for ScriptedConnection {
        async fn exchange(&mut self, request: Vec<u8>) -> Result<Vec<u8>, BrokerTransportError> {
            let command = ProducerStatusTestRequest::decode(&request)?;
            self.commands
                .push((command.command, command.sequence, command.payload.clone()));
            let status = self.statuses.pop_front().unwrap_or(ProducerStatus::Ok);
            let accepted =
                if command.command == ProducerCommand::Chunk && status == ProducerStatus::Ok {
                    u64::try_from(command.payload.len() / 2).expect("bounded test frames")
                } else {
                    0
                };
            self.accepted_frames = self.accepted_frames.saturating_add(accepted);
            let receipt = (command.command == ProducerCommand::Finish).then(|| WireReceipt {
                receipt_id: format!("receipt-{}", self.lease.stream_id),
                stream_id: self.lease.stream_id.clone(),
                session_id: self.lease.session_id.clone(),
                turn_id: self.lease.turn_id.clone(),
                generation: self.lease.generation,
                source_frames: self.accepted_frames,
                device_frames: self.device_frames_override.unwrap_or(self.accepted_frames),
                source_duration_micros: self.accepted_frames * 1_000_000
                    / u64::from(self.lease.sample_rate),
                source_submission_complete: true,
                endpoint_drain_complete: true,
                cancelled: false,
                output_selection_mode: self.lease.output_selection_mode,
                output_endpoint_id: self.lease.output_endpoint_id.clone(),
                output_endpoint_generation: self.lease.output_endpoint_generation,
            });
            encode_test_response(command.sequence, status, accepted, receipt.as_ref())
        }

        fn deadline_qpc(
            &self,
            _expires_qpc: u64,
            _require_unexpired_allocation: bool,
        ) -> Result<u64, BrokerTransportError> {
            Ok(9_000)
        }
    }

    struct ProducerStatusTestRequest {
        command: ProducerCommand,
        sequence: u64,
        payload: Vec<u8>,
    }

    impl ProducerStatusTestRequest {
        fn decode(frame: &[u8]) -> Result<Self, BrokerTransportError> {
            if frame.len() < 24 || &frame[4..8] != b"NPCP" {
                return Err(BrokerTransportError::Malformed);
            }
            let command = match u16::from_le_bytes([frame[12], frame[13]]) {
                1 => ProducerCommand::Begin,
                2 => ProducerCommand::Chunk,
                3 => ProducerCommand::Finish,
                4 => ProducerCommand::Cancel,
                5 => ProducerCommand::VisualCue,
                _ => return Err(BrokerTransportError::Malformed),
            };
            let sequence = u64::from_le_bytes(
                frame[16..24]
                    .try_into()
                    .map_err(|_| BrokerTransportError::Malformed)?,
            );
            let mut position = 40;
            for _ in 0..3 {
                let length = usize::from(u16::from_le_bytes(
                    frame
                        .get(position..position + 2)
                        .ok_or(BrokerTransportError::Malformed)?
                        .try_into()
                        .map_err(|_| BrokerTransportError::Malformed)?,
                ));
                position += 2 + length;
            }
            position += TOKEN_BYTES;
            let payload_length = usize::try_from(u32::from_le_bytes(
                frame
                    .get(position..position + 4)
                    .ok_or(BrokerTransportError::Malformed)?
                    .try_into()
                    .map_err(|_| BrokerTransportError::Malformed)?,
            ))
            .map_err(|_| BrokerTransportError::Malformed)?;
            position += 4;
            let payload = frame
                .get(position..position + payload_length)
                .ok_or(BrokerTransportError::Malformed)?
                .to_vec();
            Ok(Self {
                command,
                sequence,
                payload,
            })
        }
    }

    #[test]
    fn maps_canonical_meta_arpabet_ipa_and_azure_visemes() {
        let fixtures = [
            ("silence", 0),
            ("bilabial", 1),
            ("PP", 1),
            ("FF", 2),
            ("θ", 3),
            ("DD", 4),
            ("SH", 5),
            ("RR", 6),
            ("NG", 7),
            ("ou", 8),
            ("AA1", 9),
            ("ih", 10),
            ("21", 1),
            ("16", 5),
            ("6", 10),
        ];
        for (symbol, expected) in fixtures {
            assert_eq!(canonical_viseme(symbol), Some(expected), "{symbol}");
        }
        assert_eq!(canonical_viseme("not-a-phoneme"), None);
        assert_eq!(canonical_viseme("22"), None);
    }

    #[test]
    fn cartesia_phonemes_require_exact_typed_provider_and_model_provenance() {
        let event = |symbol: &str, provider: &str, model: &str| AlignmentEvent {
            text_offset: 0,
            text_length: 0,
            audio_offset: Duration::ZERO,
            audio_duration: Some(Duration::from_millis(40)),
            viseme: Some(symbol.to_owned()),
            symbol_kind: Some(SpeechTimingSymbolKind::Phoneme),
            symbol_provider_id: Some(provider.to_owned()),
            symbol_model_id: Some(model.to_owned()),
        };
        for (symbol, expected) in [
            ("p", 1),
            ("v", 2),
            ("ð", 3),
            ("s", 4),
            ("tʃ", 5),
            ("j", 6),
            ("ŋ", 7),
            ("ɔ", 8),
            ("ə", 9),
            ("eɪ", 10),
        ] {
            assert_eq!(
                canonical_timing_symbol(&event(symbol, "cartesia", "sonic-3.6")),
                Some(expected),
                "{symbol}"
            );
        }
        assert_eq!(
            canonical_timing_symbol(&event("ɣ", "cartesia", "sonic-3.6")),
            None,
            "an uncovered phone must fall back to the PCM envelope"
        );
        assert_eq!(
            canonical_timing_symbol(&event("p", "cartesia", "another-model")),
            None
        );
        assert_eq!(
            canonical_timing_symbol(&event("p", "another-provider", "sonic-3.6")),
            None
        );
    }

    #[test]
    fn visual_cue_payload_is_exact_little_endian_and_clamped_to_lease() {
        let lease = lease(12);
        let event = AlignmentEvent {
            text_offset: 0,
            text_length: 0,
            audio_offset: Duration::from_millis(950),
            audio_duration: Some(Duration::from_millis(200)),
            viseme: Some("PP".to_owned()),
            symbol_kind: None,
            symbol_provider_id: None,
            symbol_model_id: None,
        };
        let payload = encode_visual_cue(&event, &lease).expect("valid visual cue");

        assert_eq!(payload.len(), 24);
        assert_eq!(
            u32::from_le_bytes(payload[0..4].try_into().expect("schema bytes")),
            1
        );
        assert_eq!(
            u64::from_le_bytes(payload[4..12].try_into().expect("sample offset bytes")),
            22_800
        );
        assert_eq!(
            u64::from_le_bytes(payload[12..20].try_into().expect("sample count bytes")),
            1_200
        );
        assert_eq!(payload[20], 1);
        assert_eq!(payload[21], 0);
        assert_eq!(
            u16::from_le_bytes(payload[22..24].try_into().expect("strength bytes")),
            32_767
        );

        let missing_duration = AlignmentEvent {
            audio_offset: Duration::from_millis(10),
            audio_duration: None,
            viseme: Some("rounded".to_owned()),
            ..event.clone()
        };
        let payload = encode_visual_cue(&missing_duration, &lease).expect("default duration");
        assert_eq!(
            u64::from_le_bytes(
                payload[12..20]
                    .try_into()
                    .expect("default sample count bytes")
            ),
            1_920
        );

        let out_of_range = AlignmentEvent {
            audio_offset: Duration::from_secs(1),
            ..event
        };
        assert_eq!(encode_visual_cue(&out_of_range, &lease), None);
        assert_eq!(
            encode_request(&lease, ProducerCommand::VisualCue, 2, 9_000, &[0; 23]),
            Err(BrokerTransportError::Malformed)
        );
    }

    #[tokio::test]
    async fn producer_submits_visual_cue_without_changing_source_frame_accounting() {
        let lease = lease(13);
        let pcm = [8_192_i16, -8_192, 8_192, -8_192]
            .into_iter()
            .flat_map(i16::to_le_bytes)
            .collect::<Vec<_>>();
        let stream: SpeechStream = Box::pin(stream::iter(vec![
            Ok::<_, ProviderError>(SpeechStreamItem::Alignment(AlignmentEvent {
                text_offset: 0,
                text_length: 0,
                audio_offset: Duration::from_millis(125),
                audio_duration: Some(Duration::from_millis(40)),
                viseme: Some("FF".to_owned()),
                symbol_kind: None,
                symbol_provider_id: None,
                symbol_model_id: None,
            })),
            Ok(SpeechStreamItem::Alignment(AlignmentEvent {
                text_offset: 0,
                text_length: 0,
                audio_offset: Duration::from_millis(150),
                audio_duration: Some(Duration::from_millis(40)),
                viseme: Some("unknown-provider-symbol".to_owned()),
                symbol_kind: None,
                symbol_provider_id: None,
                symbol_model_id: None,
            })),
            Ok(SpeechStreamItem::Alignment(AlignmentEvent {
                text_offset: 0,
                text_length: 0,
                audio_offset: Duration::from_millis(140),
                audio_duration: Some(Duration::from_millis(40)),
                viseme: Some("open_vowel".to_owned()),
                symbol_kind: None,
                symbol_provider_id: None,
                symbol_model_id: None,
            })),
            Ok(SpeechStreamItem::Audio(AudioChunk {
                sequence: 1,
                sample_rate_hz: 24_000,
                channels: 1,
                pcm_s16le: pcm,
                end_of_stream: true,
            })),
        ]));
        let mut connection = ScriptedConnection {
            statuses: VecDeque::new(),
            accepted_frames: 0,
            device_frames_override: None,
            lease: lease.clone(),
            commands: Vec::new(),
        };

        let receipt =
            run_producer_session(&mut connection, &lease, stream, CancellationToken::new())
                .await
                .expect("cue-bearing stream completes");

        assert_eq!(receipt.source_frames, 4);
        let visual_cues = connection
            .commands
            .iter()
            .filter(|(command, _, _)| *command == ProducerCommand::VisualCue)
            .collect::<Vec<_>>();
        assert_eq!(
            visual_cues.len(),
            1,
            "unknown and overlapping visual cues are ignored without affecting audio"
        );
        assert_eq!(visual_cues[0].2.len(), 24);
        assert_eq!(visual_cues[0].2[20], 2);
        assert_eq!(
            connection
                .commands
                .iter()
                .map(|(command, sequence, _)| (*command, *sequence))
                .collect::<Vec<_>>(),
            vec![
                (ProducerCommand::Begin, 1),
                (ProducerCommand::VisualCue, 2),
                (ProducerCommand::Chunk, 3),
                (ProducerCommand::Finish, 4),
            ],
            "a cue already emitted by the TTS bridge must reach native playback before its first PCM chunk",
        );
        assert_eq!(connection.accepted_frames, 4);
    }

    fn encode_test_response(
        sequence: u64,
        status: ProducerStatus,
        accepted: u64,
        receipt: Option<&WireReceipt>,
    ) -> Result<Vec<u8>, BrokerTransportError> {
        let mut body = Vec::new();
        body.extend_from_slice(b"NPCR");
        push_u32(&mut body, PLAYBACK_TRANSPORT_SCHEMA_VERSION);
        push_u16(&mut body, status as u16);
        push_u16(&mut body, u16::from(receipt.is_some()));
        push_u64(&mut body, sequence);
        push_u64(&mut body, accepted);
        if let Some(receipt) = receipt {
            push_u32(&mut body, PLAYBACK_TRANSPORT_SCHEMA_VERSION);
            push_string(&mut body, &receipt.receipt_id)?;
            push_string(&mut body, &receipt.stream_id)?;
            push_string(&mut body, &receipt.session_id)?;
            push_string(&mut body, &receipt.turn_id)?;
            push_u64(&mut body, receipt.generation);
            push_u64(&mut body, receipt.source_frames);
            push_u64(&mut body, receipt.device_frames);
            push_u64(&mut body, receipt.source_duration_micros);
            body.push(u8::from(receipt.source_submission_complete));
            body.push(u8::from(receipt.endpoint_drain_complete));
            body.push(u8::from(receipt.cancelled));
            body.push(receipt.output_selection_mode.as_wire());
            push_bounded_utf8(&mut body, &receipt.output_endpoint_id)?;
            push_u64(&mut body, receipt.output_endpoint_generation);
        }
        let mut frame = Vec::new();
        push_u32(
            &mut frame,
            u32::try_from(body.len()).map_err(|_| BrokerTransportError::Malformed)?,
        );
        frame.extend_from_slice(&body);
        Ok(frame)
    }

    #[test]
    fn token_wire_is_lowercase_hex_only_and_debug_is_redacted() {
        assert!(
            serde_json::from_value::<PlaybackToken>(serde_json::json!("AA".repeat(32))).is_err()
        );
        assert!(
            serde_json::from_value::<PlaybackToken>(serde_json::json!("00".repeat(32))).is_err()
        );
        let token: PlaybackToken =
            serde_json::from_value(serde_json::json!("01".repeat(32))).expect("lowercase token");
        let debug = format!("{token:?}");
        assert_eq!(debug, "PlaybackToken([REDACTED])");
        assert!(!debug.contains("0101"));
    }

    #[test]
    fn lease_pool_rejects_duplicates_and_wrong_turn_identity() {
        let first = lease(1);
        assert!(validate_playback_lease_pool(
            std::slice::from_ref(&first),
            "session-1",
            "turn-1",
            7,
            24_000,
            1
        )
        .is_ok());
        assert_eq!(
            validate_playback_lease_pool(
                &[first.clone(), first],
                "session-1",
                "turn-1",
                7,
                24_000,
                1
            ),
            Err(BrokerLeaseError::Duplicate)
        );
        assert_eq!(
            validate_playback_lease_pool(&[lease(2)], "session-1", "other", 7, 24_000, 1),
            Err(BrokerLeaseError::IdentityMismatch)
        );

        let mut malformed_endpoint = lease(3);
        malformed_endpoint.output_endpoint_generation = 0;
        assert_eq!(
            validate_playback_lease_pool(
                &[malformed_endpoint],
                "session-1",
                "turn-1",
                7,
                24_000,
                1
            ),
            Err(BrokerLeaseError::Malformed)
        );
    }

    #[tokio::test]
    async fn producer_retries_backpressure_with_new_sequence_and_requires_real_drain_receipt() {
        let lease = lease(3);
        let mut connection = ScriptedConnection {
            statuses: [
                ProducerStatus::Ok,
                ProducerStatus::Backpressure,
                ProducerStatus::Ok,
                ProducerStatus::Ok,
            ]
            .into(),
            accepted_frames: 0,
            device_frames_override: None,
            lease: lease.clone(),
            commands: Vec::new(),
        };
        let receipt = run_producer_session(
            &mut connection,
            &lease,
            speech(&[8_192, -8_192, 8_192, -8_192]),
            CancellationToken::new(),
        )
        .await
        .expect("receipt-backed playback");
        assert!(receipt.completed());
        assert_eq!(receipt.source_frames, 4);
        assert!((receipt.peak - 0.25).abs() < f32::EPSILON);
        assert!((receipt.rms - 0.25).abs() < 0.000_1);
        let chunk_sequences = connection
            .commands
            .iter()
            .filter_map(|(command, sequence, _)| {
                (*command == ProducerCommand::Chunk).then_some(*sequence)
            })
            .collect::<Vec<_>>();
        assert_eq!(chunk_sequences.len(), 2);
        assert!(chunk_sequences[1] > chunk_sequences[0]);
    }

    #[tokio::test]
    async fn producer_refuses_silence_and_sends_cancel_instead_of_finish() {
        let lease = lease(4);
        let mut connection = ScriptedConnection {
            statuses: VecDeque::new(),
            accepted_frames: 0,
            device_frames_override: None,
            lease: lease.clone(),
            commands: Vec::new(),
        };
        let result = run_producer_session(
            &mut connection,
            &lease,
            speech(&[0, 0, 0, 0]),
            CancellationToken::new(),
        )
        .await;
        assert_eq!(result, Err(BrokerTransportError::Silent));
        assert!(connection
            .commands
            .iter()
            .any(|(command, _, _)| *command == ProducerCommand::Cancel));
        assert!(!connection
            .commands
            .iter()
            .any(|(command, _, _)| *command == ProducerCommand::Finish));
    }

    #[tokio::test]
    async fn provider_stream_failure_is_not_swallowed_and_cancels_used_lease() {
        let lease = lease(7);
        let mut connection = ScriptedConnection {
            statuses: VecDeque::new(),
            accepted_frames: 0,
            device_frames_override: None,
            lease: lease.clone(),
            commands: Vec::new(),
        };
        let failed_stream: SpeechStream = Box::pin(stream::iter(vec![Err(
            ProviderError::unavailable("mock-tts", "scripted stream failure"),
        )]));
        let result = run_producer_session(
            &mut connection,
            &lease,
            failed_stream,
            CancellationToken::new(),
        )
        .await;
        assert_eq!(result, Err(BrokerTransportError::Provider));
        assert!(connection
            .commands
            .iter()
            .any(|(command, _, _)| *command == ProducerCommand::Cancel));
        assert!(!connection
            .commands
            .iter()
            .any(|(command, _, _)| *command == ProducerCommand::Finish));
    }

    #[tokio::test]
    async fn first_audio_deadline_preserves_actionable_provider_remediation() {
        let lease = lease(8);
        let mut connection = ScriptedConnection {
            statuses: VecDeque::new(),
            accepted_frames: 0,
            device_frames_override: None,
            lease: lease.clone(),
            commands: Vec::new(),
        };
        let failed_stream: SpeechStream = Box::pin(stream::iter(vec![Err(ProviderError {
            provider_id: "cartesia".into(),
            kind: ProviderErrorKind::Timeout,
            message: "first_pcm_deadline_exceeded".into(),
            retryable: true,
            retry_after: None,
        })]));

        let result = run_producer_session(
            &mut connection,
            &lease,
            failed_stream,
            CancellationToken::new(),
        )
        .await;

        let error = result.expect_err("first PCM deadline must fail the producer session");
        assert_eq!(error, BrokerTransportError::FirstPcmDeadline);
        assert!(error.to_string().contains("choose another provider"));
        assert!(connection
            .commands
            .iter()
            .any(|(command, _, _)| *command == ProducerCommand::Cancel));
    }

    #[tokio::test]
    async fn producer_rejects_a_partial_device_submission_receipt() {
        let lease = lease(8);
        let mut connection = ScriptedConnection {
            statuses: VecDeque::new(),
            accepted_frames: 0,
            device_frames_override: Some(2),
            lease: lease.clone(),
            commands: Vec::new(),
        };
        let result = run_producer_session(
            &mut connection,
            &lease,
            speech(&[8_192, -8_192, 8_192, -8_192]),
            CancellationToken::new(),
        )
        .await;
        assert_eq!(result, Err(BrokerTransportError::FrameAccounting));
    }

    #[tokio::test]
    async fn producer_rejects_stale_output_endpoint_receipt() {
        let lease = lease(9);
        let mut receipt_lease = lease.clone();
        receipt_lease.output_endpoint_generation += 1;
        let mut connection = ScriptedConnection {
            statuses: VecDeque::new(),
            accepted_frames: 0,
            device_frames_override: None,
            lease: receipt_lease,
            commands: Vec::new(),
        };
        let result = run_producer_session(
            &mut connection,
            &lease,
            speech(&[8_192, -8_192, 8_192, -8_192]),
            CancellationToken::new(),
        )
        .await;
        assert_eq!(result, Err(BrokerTransportError::FrameAccounting));
    }

    #[test]
    fn response_decoder_rejects_unknown_output_selection_mode() {
        let fixture = lease(10);
        let receipt = WireReceipt {
            receipt_id: "receipt-fixture".into(),
            stream_id: fixture.stream_id.clone(),
            session_id: fixture.session_id.clone(),
            turn_id: fixture.turn_id.clone(),
            generation: fixture.generation,
            source_frames: 4,
            device_frames: 4,
            source_duration_micros: 166,
            source_submission_complete: true,
            endpoint_drain_complete: true,
            cancelled: false,
            output_selection_mode: fixture.output_selection_mode,
            output_endpoint_id: fixture.output_endpoint_id.clone(),
            output_endpoint_generation: fixture.output_endpoint_generation,
        };
        let mut frame = encode_test_response(3, ProducerStatus::Ok, 0, Some(&receipt))
            .expect("encoded fixture receipt");
        let mode_offset = frame.len() - (1 + 2 + fixture.output_endpoint_id.len() + 8);
        frame[mode_offset] = 3;
        assert!(matches!(
            decode_response(&frame),
            Err(BrokerTransportError::Malformed)
        ));
    }

    #[test]
    fn response_decoder_rejects_legacy_v1_without_endpoint_evidence() {
        let fixture = lease(11);
        let receipt = WireReceipt {
            receipt_id: "receipt-fixture".into(),
            stream_id: fixture.stream_id.clone(),
            session_id: fixture.session_id.clone(),
            turn_id: fixture.turn_id.clone(),
            generation: fixture.generation,
            source_frames: 4,
            device_frames: 4,
            source_duration_micros: 166,
            source_submission_complete: true,
            endpoint_drain_complete: true,
            cancelled: false,
            output_selection_mode: fixture.output_selection_mode,
            output_endpoint_id: fixture.output_endpoint_id,
            output_endpoint_generation: fixture.output_endpoint_generation,
        };
        let mut frame = encode_test_response(3, ProducerStatus::Ok, 0, Some(&receipt))
            .expect("encoded v2 fixture receipt");
        frame[8..12].copy_from_slice(&1_u32.to_le_bytes());
        assert!(matches!(
            decode_response(&frame),
            Err(BrokerTransportError::Malformed)
        ));
    }

    #[derive(Debug)]
    struct ReceiptTransport {
        consumed: Arc<Mutex<Vec<String>>>,
    }

    #[async_trait]
    impl BrokerPlaybackTransport for ReceiptTransport {
        async fn play(
            &self,
            lease: BrokerAudioPlaybackLease,
            _stream: SpeechStream,
            cancellation: CancellationToken,
        ) -> Result<BrokerSubmittedPlaybackReceipt, BrokerTransportError> {
            if cancellation.is_cancelled() {
                return Err(BrokerTransportError::Cancelled);
            }
            self.consumed
                .lock()
                .expect("consumed lock")
                .push(lease.stream_id.clone());
            Ok(BrokerSubmittedPlaybackReceipt {
                receipt_id: format!("receipt-{}", lease.stream_id),
                stream_id: lease.stream_id,
                session_id: lease.session_id,
                turn_id: lease.turn_id,
                generation: lease.generation,
                source_frames: 4,
                device_frames: 4,
                source_duration: Duration::from_micros(166),
                source_submission_complete: true,
                endpoint_drain_complete: true,
                cancelled: false,
                output_selection_mode: lease.output_selection_mode,
                output_endpoint_id: lease.output_endpoint_id,
                output_endpoint_generation: lease.output_endpoint_generation,
                peak: 0.25,
                rms: 0.25,
            })
        }
    }

    #[tokio::test]
    async fn sink_consumes_leases_once_in_order_and_exhaustion_is_explicit() {
        let consumed = Arc::new(Mutex::new(Vec::new()));
        let sink = BrokerAudioSink::new(
            vec![lease(5), lease(6)],
            ReceiptTransport {
                consumed: Arc::clone(&consumed),
            },
        );
        let identity = TurnIdentity {
            session_id: "session-1".into(),
            turn_id: "turn-1".into(),
            cancellation_generation: 99,
        };
        for _ in 0..2 {
            let receipt = sink
                .play_submitted(
                    &identity,
                    speech(&[8_192, -8_192, 8_192, -8_192]),
                    CancellationToken::new(),
                )
                .await
                .expect("leased playback");
            assert!(receipt.completed());
        }
        let exhausted = sink
            .play_submitted(
                &identity,
                speech(&[8_192, -8_192]),
                CancellationToken::new(),
            )
            .await;
        assert!(matches!(
            exhausted,
            Err(RuntimeDependencyError::Unavailable(message))
                if message.contains("lease pool exhausted")
        ));
        assert!(sink.pool_exhausted());
        assert_eq!(
            consumed.lock().expect("consumed lock").as_slice(),
            ["pcm-005", "pcm-006"]
        );
    }
}

//! Authenticated consumer for the media broker's one-use PCM input lease.

use std::{fmt, time::Duration};

use async_trait::async_trait;
use npc_providers_stt::AudioFormat;
use serde::{de::Error as _, Deserialize, Deserializer};
use tokio_util::sync::CancellationToken;
use zeroize::Zeroize;

use crate::stt_bridge::{
    NativePcmAck, NativePcmActivation, NativePcmChunk, NativePcmIdentity, NativePcmReceipt,
    PcmInputSource, PcmInputSourceError,
};

const SCHEMA_VERSION: u32 = 2;
const ENDPOINT_PREFIX: &str = r"\\.\pipe\npc-media-input-";
const MAX_CHUNK_BYTES: usize = 64 * 1_024;
const MAX_WIRE_BODY_BYTES: usize = MAX_CHUNK_BYTES + 40;
const PIPE_CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const IO_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(PartialEq, Eq)]
pub struct InputLeaseToken([u8; 32]);

impl Drop for InputLeaseToken {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl fmt::Debug for InputLeaseToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("InputLeaseToken([REDACTED])")
    }
}

impl<'de> Deserialize<'de> for InputLeaseToken {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let mut encoded = String::deserialize(deserializer)?;
        if encoded.len() != 64 || !encoded.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            encoded.zeroize();
            return Err(D::Error::custom("invalid input lease token"));
        }
        let mut token = [0_u8; 32];
        for (index, pair) in encoded.as_bytes().chunks_exact(2).enumerate() {
            token[index] = hex(pair[0])
                .and_then(|high| hex(pair[1]).map(|low| high << 4 | low))
                .ok_or_else(|| D::Error::custom("invalid input lease token"))?;
        }
        encoded.zeroize();
        Ok(Self(token))
    }
}

fn hex(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BrokerInputSelectionMode {
    SystemDefault,
    EndpointId,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BrokerInputActivationSource {
    ExplicitRehearsal,
    PushToTalk,
}

#[derive(Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BrokerAudioInputLease {
    pub schema_version: u32,
    pub stream_id: String,
    pub producer_endpoint: String,
    pub one_time_token: InputLeaseToken,
    pub session_id: String,
    pub turn_id: String,
    pub generation: u64,
    pub duration_ms: u32,
    pub sample_rate: u32,
    pub channels: u16,
    pub max_frames: u64,
    pub max_chunk_bytes: u32,
    pub expires_qpc: u64,
    pub qpc_frequency: u64,
    pub input_selection_mode: BrokerInputSelectionMode,
    pub input_endpoint_id: String,
    pub input_endpoint_generation: u64,
    pub activation_source: BrokerInputActivationSource,
    pub ptt_virtual_key: u32,
    pub ptt_press_transition_sequence: u64,
    pub ptt_pressed_qpc: u64,
}

impl fmt::Debug for BrokerAudioInputLease {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BrokerAudioInputLease")
            .field("schema_version", &self.schema_version)
            .field("stream_id", &self.stream_id)
            .field("producer_endpoint", &self.producer_endpoint)
            .field("one_time_token", &"[REDACTED]")
            .field("session_id", &self.session_id)
            .field("turn_id", &self.turn_id)
            .field("generation", &self.generation)
            .field("duration_ms", &self.duration_ms)
            .field("sample_rate", &self.sample_rate)
            .field("channels", &self.channels)
            .field("max_frames", &self.max_frames)
            .field("max_chunk_bytes", &self.max_chunk_bytes)
            .field("expires_qpc", &self.expires_qpc)
            .field("qpc_frequency", &self.qpc_frequency)
            .field("input_selection_mode", &self.input_selection_mode)
            .field("input_endpoint_id", &self.input_endpoint_id)
            .field("input_endpoint_generation", &self.input_endpoint_generation)
            .field("activation_source", &self.activation_source)
            .field("ptt_virtual_key", &self.ptt_virtual_key)
            .field(
                "ptt_press_transition_sequence",
                &self.ptt_press_transition_sequence,
            )
            .field("ptt_pressed_qpc", &self.ptt_pressed_qpc)
            .finish()
    }
}

impl BrokerAudioInputLease {
    fn validate(&self) -> Result<(), BrokerPcmInputTransportError> {
        if self.schema_version != SCHEMA_VERSION
            || !bounded(&self.stream_id, 128)
            || !self.producer_endpoint.starts_with(ENDPOINT_PREFIX)
            || self.producer_endpoint.len() > 256
            || self.producer_endpoint.contains('\0')
            || !bounded(&self.session_id, 128)
            || !bounded(&self.turn_id, 128)
            || self.generation == 0
            || !(250..=10_000).contains(&self.duration_ms)
            || self.sample_rate != 16_000
            || self.channels != 1
            || self.max_frames == 0
            || self.max_frames > u64::from(self.sample_rate) * u64::from(self.duration_ms) / 1_000
            || self.max_chunk_bytes != MAX_CHUNK_BYTES as u32
            || self.expires_qpc == 0
            || self.qpc_frequency == 0
            || !bounded(&self.input_endpoint_id, 1_024)
            || self.input_endpoint_generation == 0
            || self.activation_source != BrokerInputActivationSource::PushToTalk
            || self.ptt_virtual_key == 0
            || self.ptt_virtual_key > 0xff
            || self.ptt_press_transition_sequence == 0
            || self.ptt_pressed_qpc == 0
        {
            return Err(BrokerPcmInputTransportError::InvalidLease);
        }
        Ok(())
    }

    fn identity(&self) -> NativePcmIdentity {
        NativePcmIdentity {
            session_id: self.session_id.clone(),
            turn_id: self.turn_id.clone(),
            generation: self.generation,
            input_endpoint_id: self.input_endpoint_id.clone(),
            input_endpoint_generation: self.input_endpoint_generation,
        }
    }

    fn activation(&self) -> NativePcmActivation {
        NativePcmActivation {
            ptt_virtual_key: self.ptt_virtual_key,
            ptt_press_transition_sequence: self.ptt_press_transition_sequence,
            ptt_pressed_qpc: self.ptt_pressed_qpc,
        }
    }
}

fn bounded(value: &str, maximum: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum
        && !value.contains('\0')
        && !value.chars().any(char::is_control)
}

pub struct BrokerPcmInputSource {
    identity: NativePcmIdentity,
    activation: NativePcmActivation,
    stream_id: String,
    sample_rate: u32,
    channels: u16,
    max_frames: u64,
    max_chunk_bytes: u32,
    input_selection_mode: BrokerInputSelectionMode,
    next_sequence: u64,
    last_sequence: Option<u64>,
    captured_frames: u64,
    receipt: Option<NativePcmReceipt>,
    #[cfg(windows)]
    pipe: Option<tokio::net::windows::named_pipe::NamedPipeClient>,
}

impl fmt::Debug for BrokerPcmInputSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BrokerPcmInputSource")
            .field("identity", &self.identity)
            .field("activation", &self.activation)
            .field("stream_id", &self.stream_id)
            .field("sample_rate", &self.sample_rate)
            .field("channels", &self.channels)
            .field("max_frames", &self.max_frames)
            .field("next_sequence", &self.next_sequence)
            .field("captured_frames", &self.captured_frames)
            .field("receipt_received", &self.receipt.is_some())
            .finish()
    }
}

impl BrokerPcmInputSource {
    pub async fn connect(
        lease: BrokerAudioInputLease,
        cancellation: CancellationToken,
    ) -> Result<Self, BrokerPcmInputTransportError> {
        lease.validate()?;
        connect_platform(lease, cancellation).await
    }

    async fn read_next_body(&mut self) -> Result<Vec<u8>, BrokerPcmInputTransportError> {
        #[cfg(windows)]
        {
            use tokio::io::AsyncReadExt;
            let pipe = self
                .pipe
                .as_mut()
                .ok_or(BrokerPcmInputTransportError::Closed)?;
            return tokio::time::timeout(IO_TIMEOUT, async {
                let mut prefix = [0_u8; 4];
                pipe.read_exact(&mut prefix)
                    .await
                    .map_err(|_| BrokerPcmInputTransportError::Connection)?;
                let size = u32::from_le_bytes(prefix) as usize;
                if size == 0 || size > MAX_WIRE_BODY_BYTES {
                    return Err(BrokerPcmInputTransportError::Protocol);
                }
                let mut body = vec![0_u8; size];
                pipe.read_exact(&mut body)
                    .await
                    .map_err(|_| BrokerPcmInputTransportError::Connection)?;
                Ok(body)
            })
            .await
            .map_err(|_| BrokerPcmInputTransportError::Timeout)?;
        }
        #[cfg(not(windows))]
        Err(BrokerPcmInputTransportError::UnsupportedPlatform)
    }

    async fn write_ack(
        &mut self,
        sequence: u64,
        action: NativePcmAck,
    ) -> Result<(), BrokerPcmInputTransportError> {
        if sequence == 0 || self.last_sequence != Some(sequence) {
            return Err(BrokerPcmInputTransportError::Protocol);
        }
        let action = match action {
            NativePcmAck::Continue => 0,
            NativePcmAck::Stop => 1,
            NativePcmAck::Cancel => 2,
        };
        let mut body = Vec::with_capacity(17);
        body.extend_from_slice(b"NPIA");
        body.extend_from_slice(&SCHEMA_VERSION.to_le_bytes());
        body.extend_from_slice(&sequence.to_le_bytes());
        body.push(action);
        #[cfg(windows)]
        {
            use tokio::io::AsyncWriteExt;
            let pipe = self
                .pipe
                .as_mut()
                .ok_or(BrokerPcmInputTransportError::Closed)?;
            let size = (body.len() as u32).to_le_bytes();
            tokio::time::timeout(IO_TIMEOUT, async {
                pipe.write_all(&size)
                    .await
                    .map_err(|_| BrokerPcmInputTransportError::Connection)?;
                pipe.write_all(&body)
                    .await
                    .map_err(|_| BrokerPcmInputTransportError::Connection)?;
                pipe.flush()
                    .await
                    .map_err(|_| BrokerPcmInputTransportError::Connection)
            })
            .await
            .map_err(|_| BrokerPcmInputTransportError::Timeout)??;
            Ok(())
        }
        #[cfg(not(windows))]
        Err(BrokerPcmInputTransportError::UnsupportedPlatform)
    }

    fn accept_receipt(&mut self, body: &[u8]) -> Result<(), BrokerPcmInputTransportError> {
        let receipt = decode_receipt(body, self)?;
        self.receipt = Some(receipt);
        Ok(())
    }
}

#[async_trait]
impl PcmInputSource for BrokerPcmInputSource {
    fn identity(&self) -> &NativePcmIdentity {
        &self.identity
    }

    fn activation(&self) -> &NativePcmActivation {
        &self.activation
    }

    fn format(&self) -> AudioFormat {
        AudioFormat::PCM_16KHZ_MONO
    }

    async fn next_chunk(&mut self) -> Result<Option<NativePcmChunk>, PcmInputSourceError> {
        if self.receipt.is_some() {
            return Ok(None);
        }
        let body = self
            .read_next_body()
            .await
            .map_err(|_| PcmInputSourceError)?;
        if body.starts_with(b"NPIR") {
            self.accept_receipt(&body)
                .map_err(|_| PcmInputSourceError)?;
            return Ok(None);
        }
        let chunk = decode_chunk(&body, self).map_err(|_| PcmInputSourceError)?;
        self.last_sequence = Some(chunk.sequence);
        self.next_sequence = self.next_sequence.saturating_add(1);
        self.captured_frames = self
            .captured_frames
            .saturating_add(u64::from(chunk.frame_count));
        Ok(Some(chunk))
    }

    async fn acknowledge(
        &mut self,
        sequence: u64,
        action: NativePcmAck,
    ) -> Result<(), PcmInputSourceError> {
        self.write_ack(sequence, action)
            .await
            .map_err(|_| PcmInputSourceError)
    }

    async fn finish(&mut self) -> Result<NativePcmReceipt, PcmInputSourceError> {
        if self.receipt.is_none() {
            let body = self
                .read_next_body()
                .await
                .map_err(|_| PcmInputSourceError)?;
            self.accept_receipt(&body)
                .map_err(|_| PcmInputSourceError)?;
        }
        self.receipt.clone().ok_or(PcmInputSourceError)
    }

    async fn cancel(&mut self) -> Result<(), PcmInputSourceError> {
        if let Some(sequence) = self.last_sequence {
            let _ = self.write_ack(sequence, NativePcmAck::Cancel).await;
        }
        #[cfg(windows)]
        {
            self.pipe.take();
        }
        Ok(())
    }
}

fn decode_hello(
    body: &[u8],
    lease: &BrokerAudioInputLease,
) -> Result<(), BrokerPcmInputTransportError> {
    let mut cursor = Cursor::new(body, b"NPIH")?;
    if cursor.u32()? != SCHEMA_VERSION
        || cursor.string(128)? != lease.stream_id
        || cursor.string(128)? != lease.session_id
        || cursor.string(128)? != lease.turn_id
        || cursor.u64()? != lease.generation
        || cursor.u32()? != lease.sample_rate
        || cursor.u16()? != lease.channels
        || cursor.u64()? != lease.max_frames
        || cursor.u32()? != lease.max_chunk_bytes
        || cursor.u64()? != lease.qpc_frequency
        || cursor.u8()? != 2
        || cursor.u32()? != lease.ptt_virtual_key
        || cursor.u64()? != lease.ptt_press_transition_sequence
        || cursor.u64()? != lease.ptt_pressed_qpc
        || !cursor.done()
    {
        return Err(BrokerPcmInputTransportError::Protocol);
    }
    Ok(())
}

fn decode_chunk(
    body: &[u8],
    source: &BrokerPcmInputSource,
) -> Result<NativePcmChunk, BrokerPcmInputTransportError> {
    let mut cursor = Cursor::new(body, b"NPIC")?;
    let version = cursor.u32()?;
    let sequence = cursor.u64()?;
    let first_frame_qpc = cursor.u64()?;
    let first_frame_index = cursor.u64()?;
    let frame_count = cursor.u32()?;
    let pcm_bytes = cursor.u32()? as usize;
    let pcm = cursor.remaining();
    if version != SCHEMA_VERSION
        || sequence != source.next_sequence
        || first_frame_qpc == 0
        || first_frame_index != source.captured_frames
        || frame_count == 0
        || pcm_bytes != pcm.len()
        || pcm_bytes > source.max_chunk_bytes as usize
        || pcm_bytes != frame_count as usize * source.channels as usize * 2
        || source
            .captured_frames
            .saturating_add(u64::from(frame_count))
            > source.max_frames
    {
        return Err(BrokerPcmInputTransportError::Protocol);
    }
    Ok(NativePcmChunk {
        identity: source.identity.clone(),
        sequence,
        first_frame_qpc,
        first_frame_index,
        frame_count,
        pcm_s16le: pcm.to_vec(),
    })
}

fn decode_receipt(
    body: &[u8],
    source: &BrokerPcmInputSource,
) -> Result<NativePcmReceipt, BrokerPcmInputTransportError> {
    let mut cursor = Cursor::new(body, b"NPIR")?;
    let version = cursor.u32()?;
    let _receipt_id = cursor.string(128)?;
    let stream_id = cursor.string(128)?;
    let session_id = cursor.string(128)?;
    let turn_id = cursor.string(128)?;
    let generation = cursor.u64()?;
    let mode = cursor.u8()?;
    let endpoint_id = cursor.string(1_024)?;
    let endpoint_generation = cursor.u64()?;
    let rate = cursor.u32()?;
    let channels = cursor.u16()?;
    let frames = cursor.u64()?;
    let _duration_micros = cursor.u64()?;
    let peak_milli_dbfs = cursor.u32()? as i32;
    let rms_milli_dbfs = cursor.u32()? as i32;
    let clipped_samples = cursor.u64()?;
    let silent_frames = cursor.u64()?;
    let source_complete = cursor.flag()?;
    let _silence_detected = cursor.flag()?;
    let _clipping_detected = cursor.flag()?;
    let cancelled = cursor.flag()?;
    let device_lost = cursor.flag()?;
    let activation_source = cursor.u8()?;
    let ptt_virtual_key = cursor.u32()?;
    let ptt_press_transition_sequence = cursor.u64()?;
    let ptt_pressed_qpc = cursor.u64()?;
    let ptt_release_transition_sequence = cursor.u64()?;
    let ptt_released_qpc = cursor.u64()?;
    let expected_mode = match source.input_selection_mode {
        BrokerInputSelectionMode::SystemDefault => 1,
        BrokerInputSelectionMode::EndpointId => 2,
    };
    if version != SCHEMA_VERSION
        || stream_id != source.stream_id
        || session_id != source.identity.session_id
        || turn_id != source.identity.turn_id
        || generation != source.identity.generation
        || mode != expected_mode
        || endpoint_id != source.identity.input_endpoint_id
        || endpoint_generation != source.identity.input_endpoint_generation
        || rate != source.sample_rate
        || channels != source.channels
        || frames != source.captured_frames
        || frames > source.max_frames
        || !(-120_000..=0).contains(&peak_milli_dbfs)
        || !(-120_000..=0).contains(&rms_milli_dbfs)
        || clipped_samples > frames.saturating_mul(u64::from(channels))
        || silent_frames > frames
        || activation_source != 2
        || ptt_virtual_key != source.activation.ptt_virtual_key
        || ptt_press_transition_sequence != source.activation.ptt_press_transition_sequence
        || ptt_pressed_qpc != source.activation.ptt_pressed_qpc
        || (ptt_release_transition_sequence == 0) != (ptt_released_qpc == 0)
        || (ptt_release_transition_sequence != 0
            && (ptt_release_transition_sequence <= ptt_press_transition_sequence
                || ptt_released_qpc < ptt_pressed_qpc))
        || (source_complete && ptt_release_transition_sequence == 0)
        || !cursor.done()
        || (source_complete && (cancelled || device_lost))
    {
        return Err(BrokerPcmInputTransportError::Protocol);
    }
    Ok(NativePcmReceipt {
        identity: source.identity.clone(),
        activation: source.activation.clone(),
        captured_frames: frames,
        source_capture_complete: source_complete,
        cancelled,
        device_lost,
        ptt_release_transition_sequence,
        ptt_released_qpc,
    })
}

struct Cursor<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8], magic: &[u8; 4]) -> Result<Self, BrokerPcmInputTransportError> {
        if !bytes.starts_with(magic) {
            return Err(BrokerPcmInputTransportError::Protocol);
        }
        Ok(Self { bytes, position: 4 })
    }
    fn take(&mut self, count: usize) -> Result<&'a [u8], BrokerPcmInputTransportError> {
        let end = self
            .position
            .checked_add(count)
            .ok_or(BrokerPcmInputTransportError::Protocol)?;
        let value = self
            .bytes
            .get(self.position..end)
            .ok_or(BrokerPcmInputTransportError::Protocol)?;
        self.position = end;
        Ok(value)
    }
    fn u8(&mut self) -> Result<u8, BrokerPcmInputTransportError> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, BrokerPcmInputTransportError> {
        Ok(u16::from_le_bytes(
            self.take(2)?
                .try_into()
                .expect("take returned the requested two-byte slice"),
        ))
    }
    fn u32(&mut self) -> Result<u32, BrokerPcmInputTransportError> {
        Ok(u32::from_le_bytes(
            self.take(4)?
                .try_into()
                .expect("take returned the requested four-byte slice"),
        ))
    }
    fn u64(&mut self) -> Result<u64, BrokerPcmInputTransportError> {
        Ok(u64::from_le_bytes(
            self.take(8)?
                .try_into()
                .expect("take returned the requested eight-byte slice"),
        ))
    }
    fn string(&mut self, maximum: usize) -> Result<String, BrokerPcmInputTransportError> {
        let length = self.u16()? as usize;
        if length == 0 || length > maximum {
            return Err(BrokerPcmInputTransportError::Protocol);
        }
        let value = std::str::from_utf8(self.take(length)?)
            .map_err(|_| BrokerPcmInputTransportError::Protocol)?;
        if value.contains('\0') {
            return Err(BrokerPcmInputTransportError::Protocol);
        }
        Ok(value.to_owned())
    }
    fn flag(&mut self) -> Result<bool, BrokerPcmInputTransportError> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(BrokerPcmInputTransportError::Protocol),
        }
    }
    fn remaining(&mut self) -> &'a [u8] {
        let remaining = &self.bytes[self.position..];
        self.position = self.bytes.len();
        remaining
    }
    fn done(&self) -> bool {
        self.position == self.bytes.len()
    }
}

#[cfg(windows)]
async fn connect_platform(
    mut lease: BrokerAudioInputLease,
    cancellation: CancellationToken,
) -> Result<BrokerPcmInputSource, BrokerPcmInputTransportError> {
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::windows::named_pipe::ClientOptions,
    };
    let (now, frequency) = qpc_now_and_frequency()?;
    if frequency != lease.qpc_frequency || now > lease.expires_qpc {
        return Err(BrokerPcmInputTransportError::InvalidLease);
    }
    let started = std::time::Instant::now();
    let mut pipe = loop {
        if cancellation.is_cancelled() {
            return Err(BrokerPcmInputTransportError::Cancelled);
        }
        match ClientOptions::new().open(&lease.producer_endpoint) {
            Ok(pipe) => break pipe,
            Err(error)
                if matches!(error.raw_os_error(), Some(2 | 231))
                    && started.elapsed() < PIPE_CONNECT_TIMEOUT =>
            {
                tokio::select! {
                    _ = cancellation.cancelled() => return Err(BrokerPcmInputTransportError::Cancelled),
                    _ = tokio::time::sleep(Duration::from_millis(10)) => {}
                }
            }
            Err(_) => return Err(BrokerPcmInputTransportError::Connection),
        }
    };
    tokio::time::timeout(IO_TIMEOUT, async {
        pipe.write_all(&lease.one_time_token.0)
            .await
            .map_err(|_| BrokerPcmInputTransportError::Connection)?;
        pipe.flush()
            .await
            .map_err(|_| BrokerPcmInputTransportError::Connection)?;
        let mut prefix = [0_u8; 4];
        pipe.read_exact(&mut prefix)
            .await
            .map_err(|_| BrokerPcmInputTransportError::Connection)?;
        let size = u32::from_le_bytes(prefix) as usize;
        if !(48..=1_024).contains(&size) {
            return Err(BrokerPcmInputTransportError::Protocol);
        }
        let mut hello = vec![0_u8; size];
        pipe.read_exact(&mut hello)
            .await
            .map_err(|_| BrokerPcmInputTransportError::Connection)?;
        decode_hello(&hello, &lease)
    })
    .await
    .map_err(|_| BrokerPcmInputTransportError::Timeout)??;
    let source = BrokerPcmInputSource {
        identity: lease.identity(),
        activation: lease.activation(),
        stream_id: std::mem::take(&mut lease.stream_id),
        sample_rate: lease.sample_rate,
        channels: lease.channels,
        max_frames: lease.max_frames,
        max_chunk_bytes: lease.max_chunk_bytes,
        input_selection_mode: lease.input_selection_mode,
        next_sequence: 1,
        last_sequence: None,
        captured_frames: 0,
        receipt: None,
        pipe: Some(pipe),
    };
    Ok(source)
}

#[cfg(not(windows))]
async fn connect_platform(
    _lease: BrokerAudioInputLease,
    _cancellation: CancellationToken,
) -> Result<BrokerPcmInputSource, BrokerPcmInputTransportError> {
    Err(BrokerPcmInputTransportError::UnsupportedPlatform)
}

#[cfg(windows)]
fn qpc_now_and_frequency() -> Result<(u64, u64), BrokerPcmInputTransportError> {
    let mut now = 0_i64;
    let mut frequency = 0_i64;
    // SAFETY: both Windows APIs write to valid stack-owned outputs.
    let success = unsafe {
        windows_sys::Win32::System::Performance::QueryPerformanceCounter(&mut now) != 0
            && windows_sys::Win32::System::Performance::QueryPerformanceFrequency(&mut frequency)
                != 0
    };
    if !success || now <= 0 || frequency <= 0 {
        return Err(BrokerPcmInputTransportError::Connection);
    }
    Ok((now as u64, frequency as u64))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum BrokerPcmInputTransportError {
    #[error("native PCM input lease is invalid")]
    InvalidLease,
    #[error("native PCM input connection failed")]
    Connection,
    #[error("native PCM input transport timed out")]
    Timeout,
    #[error("native PCM input protocol failed validation")]
    Protocol,
    #[error("native PCM input transport is closed")]
    Closed,
    #[error("native PCM input was cancelled")]
    Cancelled,
    #[error("native PCM input is unavailable on this platform")]
    UnsupportedPlatform,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn lease() -> BrokerAudioInputLease {
        serde_json::from_value(serde_json::json!({
            "schemaVersion": 2,
            "streamId": "mic-fixture",
            "producerEndpoint": r"\\.\pipe\npc-media-input-fixture",
            "oneTimeToken": "0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20",
            "sessionId": "session-1", "turnId": "turn-1", "generation": 4,
            "durationMs": 500, "sampleRate": 16000, "channels": 1,
            "maxFrames": 8000, "maxChunkBytes": 65536,
            "expiresQpc": 99, "qpcFrequency": 10000000,
            "inputSelectionMode": "systemDefault",
            "inputEndpointId": "endpoint-fixture", "inputEndpointGeneration": 3,
            "activationSource": "pushToTalk", "pttVirtualKey": 88,
            "pttPressTransitionSequence": 8, "pttPressedQpc": 10
        }))
        .unwrap()
    }

    fn append_string(body: &mut Vec<u8>, value: &str) {
        body.extend_from_slice(&(value.len() as u16).to_le_bytes());
        body.extend_from_slice(value.as_bytes());
    }

    #[test]
    fn lease_and_token_debug_are_redacted_and_shape_is_fixed() {
        let value = lease();
        value.validate().unwrap();
        let debug = format!("{value:?}");
        assert!(debug.contains("[REDACTED]"));
        assert!(!debug.contains("010203040506"));
        assert_eq!(value.identity().generation, 4);
        assert_eq!(value.activation().ptt_press_transition_sequence, 8);

        let mut old_schema = serde_json::to_value(serde_json::json!({
            "schemaVersion": 1,
            "streamId": "mic-fixture",
            "producerEndpoint": r"\\.\pipe\npc-media-input-fixture",
            "oneTimeToken": "0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20",
            "sessionId": "session-1", "turnId": "turn-1", "generation": 4,
            "durationMs": 500, "sampleRate": 16000, "channels": 1,
            "maxFrames": 8000, "maxChunkBytes": 65536,
            "expiresQpc": 99, "qpcFrequency": 10000000,
            "inputSelectionMode": "systemDefault",
            "inputEndpointId": "endpoint-fixture", "inputEndpointGeneration": 3,
            "activationSource": "pushToTalk", "pttVirtualKey": 88,
            "pttPressTransitionSequence": 8, "pttPressedQpc": 10
        }))
        .unwrap();
        old_schema["schemaVersion"] = serde_json::json!(1);
        let old: BrokerAudioInputLease = serde_json::from_value(old_schema).unwrap();
        assert_eq!(
            old.validate(),
            Err(BrokerPcmInputTransportError::InvalidLease)
        );

        let mut rehearsal = serde_json::to_value(serde_json::json!({
            "schemaVersion": 2,
            "streamId": "mic-fixture",
            "producerEndpoint": r"\\.\pipe\npc-media-input-fixture",
            "oneTimeToken": "0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20",
            "sessionId": "session-1", "turnId": "turn-1", "generation": 4,
            "durationMs": 500, "sampleRate": 16000, "channels": 1,
            "maxFrames": 8000, "maxChunkBytes": 65536,
            "expiresQpc": 99, "qpcFrequency": 10000000,
            "inputSelectionMode": "systemDefault",
            "inputEndpointId": "endpoint-fixture", "inputEndpointGeneration": 3,
            "activationSource": "explicitRehearsal", "pttVirtualKey": 0,
            "pttPressTransitionSequence": 0, "pttPressedQpc": 0
        }))
        .unwrap();
        rehearsal["activationSource"] = serde_json::json!("explicitRehearsal");
        let rehearsal: BrokerAudioInputLease = serde_json::from_value(rehearsal).unwrap();
        assert_eq!(
            rehearsal.validate(),
            Err(BrokerPcmInputTransportError::InvalidLease)
        );
    }

    #[test]
    fn exact_hello_and_chunk_wire_decode() {
        let value = lease();
        let mut hello = b"NPIH".to_vec();
        hello.extend_from_slice(&2_u32.to_le_bytes());
        append_string(&mut hello, "mic-fixture");
        append_string(&mut hello, "session-1");
        append_string(&mut hello, "turn-1");
        hello.extend_from_slice(&4_u64.to_le_bytes());
        hello.extend_from_slice(&16_000_u32.to_le_bytes());
        hello.extend_from_slice(&1_u16.to_le_bytes());
        hello.extend_from_slice(&8_000_u64.to_le_bytes());
        hello.extend_from_slice(&65_536_u32.to_le_bytes());
        hello.extend_from_slice(&10_000_000_u64.to_le_bytes());
        hello.push(2);
        hello.extend_from_slice(&88_u32.to_le_bytes());
        hello.extend_from_slice(&8_u64.to_le_bytes());
        hello.extend_from_slice(&10_u64.to_le_bytes());
        decode_hello(&hello, &value).unwrap();

        let source = BrokerPcmInputSource {
            identity: value.identity(),
            activation: value.activation(),
            stream_id: value.stream_id.clone(),
            sample_rate: 16_000,
            channels: 1,
            max_frames: 8_000,
            max_chunk_bytes: 65_536,
            input_selection_mode: BrokerInputSelectionMode::SystemDefault,
            next_sequence: 1,
            last_sequence: None,
            captured_frames: 0,
            receipt: None,
            #[cfg(windows)]
            pipe: None,
        };
        let mut chunk = b"NPIC".to_vec();
        chunk.extend_from_slice(&2_u32.to_le_bytes());
        chunk.extend_from_slice(&1_u64.to_le_bytes());
        chunk.extend_from_slice(&50_u64.to_le_bytes());
        chunk.extend_from_slice(&0_u64.to_le_bytes());
        chunk.extend_from_slice(&2_u32.to_le_bytes());
        chunk.extend_from_slice(&4_u32.to_le_bytes());
        chunk.extend_from_slice(&[1, 0, 2, 0]);
        let decoded = decode_chunk(&chunk, &source).unwrap();
        assert_eq!(decoded.sequence, 1);
        assert_eq!(decoded.frame_count, 2);
        assert!(!format!("{decoded:?}").contains("[1, 0, 2, 0]"));
    }

    #[test]
    fn exact_push_to_talk_receipt_requires_a_newer_broker_release_transition() {
        let value = lease();
        let source = BrokerPcmInputSource {
            identity: value.identity(),
            activation: value.activation(),
            stream_id: value.stream_id.clone(),
            sample_rate: 16_000,
            channels: 1,
            max_frames: 8_000,
            max_chunk_bytes: 65_536,
            input_selection_mode: BrokerInputSelectionMode::SystemDefault,
            next_sequence: 2,
            last_sequence: Some(1),
            captured_frames: 2,
            receipt: None,
            #[cfg(windows)]
            pipe: None,
        };
        let mut receipt = b"NPIR".to_vec();
        receipt.extend_from_slice(&2_u32.to_le_bytes());
        append_string(&mut receipt, "mic-receipt-fixture");
        append_string(&mut receipt, "mic-fixture");
        append_string(&mut receipt, "session-1");
        append_string(&mut receipt, "turn-1");
        receipt.extend_from_slice(&4_u64.to_le_bytes());
        receipt.push(1);
        append_string(&mut receipt, "endpoint-fixture");
        receipt.extend_from_slice(&3_u64.to_le_bytes());
        receipt.extend_from_slice(&16_000_u32.to_le_bytes());
        receipt.extend_from_slice(&1_u16.to_le_bytes());
        receipt.extend_from_slice(&2_u64.to_le_bytes());
        receipt.extend_from_slice(&125_u64.to_le_bytes());
        receipt.extend_from_slice(&(-1_000_i32 as u32).to_le_bytes());
        receipt.extend_from_slice(&(-3_000_i32 as u32).to_le_bytes());
        receipt.extend_from_slice(&0_u64.to_le_bytes());
        receipt.extend_from_slice(&0_u64.to_le_bytes());
        receipt.extend_from_slice(&[1, 0, 0, 0, 0]);
        receipt.push(2);
        receipt.extend_from_slice(&88_u32.to_le_bytes());
        receipt.extend_from_slice(&8_u64.to_le_bytes());
        receipt.extend_from_slice(&10_u64.to_le_bytes());
        receipt.extend_from_slice(&9_u64.to_le_bytes());
        receipt.extend_from_slice(&20_u64.to_le_bytes());

        let decoded = decode_receipt(&receipt, &source).unwrap();
        assert_eq!(decoded.ptt_release_transition_sequence, 9);
        assert_eq!(decoded.ptt_released_qpc, 20);

        let release_sequence_offset = receipt.len() - 16;
        receipt[release_sequence_offset..release_sequence_offset + 8]
            .copy_from_slice(&8_u64.to_le_bytes());
        assert_eq!(
            decode_receipt(&receipt, &source),
            Err(BrokerPcmInputTransportError::Protocol)
        );
    }
}

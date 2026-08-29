use crate::{IpcErrorV1, RequestId, MAX_AUDIO_CHUNK_BYTES, MAX_TEXT_BYTES};
use prost::{Enumeration, Message, Oneof};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, PartialEq, Eq, Message, Serialize, Deserialize)]
pub struct EmptyV1 {}

#[derive(Clone, PartialEq, Eq, Message, Serialize, Deserialize)]
pub struct ProviderEventMetadataV1 {
    #[prost(string, tag = "1")]
    pub provider_id: String,
    #[prost(string, tag = "2")]
    pub model_id: String,
    #[prost(message, optional, tag = "3")]
    pub request_id: Option<RequestId>,
    #[prost(uint64, tag = "4")]
    pub provider_sequence: u64,
    #[prost(uint64, tag = "5")]
    pub provider_elapsed_ms: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Enumeration, Serialize, Deserialize)]
#[repr(i32)]
pub enum AudioEncoding {
    Unspecified = 0,
    PcmS16Le = 1,
    PcmF32Le = 2,
    Opus = 3,
    Mp3 = 4,
    Wav = 5,
}

#[derive(Clone, PartialEq, Eq, Message, Serialize, Deserialize)]
pub struct AudioFormatV1 {
    #[prost(enumeration = "AudioEncoding", tag = "1")]
    pub encoding: i32,
    #[prost(uint32, tag = "2")]
    pub sample_rate_hz: u32,
    #[prost(uint32, tag = "3")]
    pub channels: u32,
    #[prost(uint32, tag = "4")]
    pub bits_per_sample: u32,
}

impl AudioFormatV1 {
    pub fn validate(&self) -> Result<(), EventValidationError> {
        let encoding = AudioEncoding::try_from(self.encoding)
            .map_err(|_| EventValidationError::UnknownEnum("audio encoding"))?;
        if encoding == AudioEncoding::Unspecified {
            return Err(EventValidationError::UnknownEnum("audio encoding"));
        }
        if !(8_000..=192_000).contains(&self.sample_rate_hz) {
            return Err(EventValidationError::InvalidAudioFormat(
                "sample rate must be between 8 kHz and 192 kHz",
            ));
        }
        if !(1..=8).contains(&self.channels) {
            return Err(EventValidationError::InvalidAudioFormat(
                "channel count must be between 1 and 8",
            ));
        }
        match encoding {
            AudioEncoding::PcmS16Le if self.bits_per_sample != 16 => Err(
                EventValidationError::InvalidAudioFormat("PCM S16 must use 16 bits"),
            ),
            AudioEncoding::PcmF32Le if self.bits_per_sample != 32 => Err(
                EventValidationError::InvalidAudioFormat("PCM F32 must use 32 bits"),
            ),
            AudioEncoding::PcmS16Le | AudioEncoding::PcmF32Le => Ok(()),
            _ if self.bits_per_sample != 0 => Err(EventValidationError::InvalidAudioFormat(
                "compressed/container formats must report zero bits per sample",
            )),
            _ => Ok(()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Enumeration, Serialize, Deserialize)]
#[repr(i32)]
pub enum EndpointReason {
    Unspecified = 0,
    PushToTalkReleased = 1,
    VoiceActivitySilence = 2,
    ProviderEndpoint = 3,
    MaximumDuration = 4,
    ManualStop = 5,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct TranscriptSegmentV1 {
    #[prost(string, tag = "1")]
    pub text: String,
    #[prost(string, tag = "2")]
    pub language: String,
    #[prost(float, optional, tag = "3")]
    pub confidence: Option<f32>,
    #[prost(float, optional, tag = "4")]
    pub stability: Option<f32>,
    #[prost(uint64, tag = "5")]
    pub start_ms: u64,
    #[prost(uint64, tag = "6")]
    pub end_ms: u64,
    #[prost(string, tag = "7")]
    pub provider_segment_id: String,
}

impl TranscriptSegmentV1 {
    pub fn validate(&self, allow_empty: bool) -> Result<(), EventValidationError> {
        validate_text(&self.text, allow_empty)?;
        validate_unit_interval(self.confidence, "confidence")?;
        validate_unit_interval(self.stability, "stability")?;
        if self.end_ms < self.start_ms {
            return Err(EventValidationError::InvalidTimeRange);
        }
        if self.language.len() > 64 || self.provider_segment_id.len() > 256 {
            return Err(EventValidationError::MetadataTooLarge);
        }
        Ok(())
    }
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct SpeechActivityV1 {
    #[prost(bool, tag = "1")]
    pub active: bool,
    #[prost(float, tag = "2")]
    pub probability: f32,
    #[prost(float, tag = "3")]
    pub rms_dbfs: f32,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct EndpointDetectedV1 {
    #[prost(enumeration = "EndpointReason", tag = "1")]
    pub reason: i32,
    #[prost(uint64, tag = "2")]
    pub captured_audio_ms: u64,
}

#[derive(Clone, PartialEq, Eq, Message, Serialize, Deserialize)]
pub struct CancelledV1 {
    #[prost(string, tag = "1")]
    pub reason: String,
    #[prost(uint64, tag = "2")]
    pub observed_generation: u64,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct SttEventV1 {
    #[prost(message, optional, tag = "1")]
    pub metadata: Option<ProviderEventMetadataV1>,
    #[prost(oneof = "stt_event_v1::Kind", tags = "10, 11, 12, 13, 14, 15, 16")]
    pub kind: Option<stt_event_v1::Kind>,
}

pub mod stt_event_v1 {
    use super::*;

    #[derive(Clone, PartialEq, Oneof, Serialize, Deserialize)]
    pub enum Kind {
        #[prost(message, tag = "10")]
        Started(AudioFormatV1),
        #[prost(message, tag = "11")]
        Partial(TranscriptSegmentV1),
        #[prost(message, tag = "12")]
        Final(TranscriptSegmentV1),
        #[prost(message, tag = "13")]
        SpeechActivity(SpeechActivityV1),
        #[prost(message, tag = "14")]
        Endpoint(EndpointDetectedV1),
        #[prost(message, tag = "15")]
        Failed(IpcErrorV1),
        #[prost(message, tag = "16")]
        Cancelled(CancelledV1),
    }
}

impl SttEventV1 {
    pub fn validate(&self) -> Result<(), EventValidationError> {
        validate_metadata(self.metadata.as_ref())?;
        match self
            .kind
            .as_ref()
            .ok_or(EventValidationError::MissingKind)?
        {
            stt_event_v1::Kind::Started(format) => format.validate(),
            stt_event_v1::Kind::Partial(segment) => segment.validate(true),
            stt_event_v1::Kind::Final(segment) => segment.validate(false),
            stt_event_v1::Kind::SpeechActivity(activity) => {
                validate_unit_interval(Some(activity.probability), "speech probability")?;
                if !activity.rms_dbfs.is_finite() || activity.rms_dbfs > 0.0 {
                    return Err(EventValidationError::NonFiniteOrOutOfRange("rms_dbfs"));
                }
                Ok(())
            }
            stt_event_v1::Kind::Endpoint(endpoint) => {
                let reason = EndpointReason::try_from(endpoint.reason)
                    .map_err(|_| EventValidationError::UnknownEnum("endpoint reason"))?;
                if reason == EndpointReason::Unspecified {
                    Err(EventValidationError::UnknownEnum("endpoint reason"))
                } else {
                    Ok(())
                }
            }
            stt_event_v1::Kind::Failed(error) => validate_error(error),
            stt_event_v1::Kind::Cancelled(cancelled) => validate_cancelled(cancelled),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Enumeration, Serialize, Deserialize)]
#[repr(i32)]
pub enum LlmFinishReason {
    Unspecified = 0,
    Stop = 1,
    Length = 2,
    ContentFilter = 3,
    ToolBoundary = 4,
    Provider = 5,
}

#[derive(Clone, PartialEq, Eq, Message, Serialize, Deserialize)]
pub struct TextDeltaV1 {
    #[prost(string, tag = "1")]
    pub text: String,
    #[prost(uint64, tag = "2")]
    pub text_offset_bytes: u64,
}

#[derive(Clone, PartialEq, Eq, Message, Serialize, Deserialize)]
pub struct SentenceReadyV1 {
    #[prost(string, tag = "1")]
    pub sentence_id: String,
    #[prost(string, tag = "2")]
    pub text: String,
    #[prost(uint64, tag = "3")]
    pub text_start_bytes: u64,
    #[prost(uint64, tag = "4")]
    pub text_end_bytes: u64,
}

#[derive(Clone, PartialEq, Eq, Message, Serialize, Deserialize)]
pub struct TokenUsageV1 {
    #[prost(uint64, tag = "1")]
    pub input_tokens: u64,
    #[prost(uint64, tag = "2")]
    pub output_tokens: u64,
    #[prost(uint64, tag = "3")]
    pub cached_input_tokens: u64,
}

#[derive(Clone, PartialEq, Eq, Message, Serialize, Deserialize)]
pub struct LlmCompletedV1 {
    #[prost(enumeration = "LlmFinishReason", tag = "1")]
    pub finish_reason: i32,
    #[prost(message, optional, tag = "2")]
    pub usage: Option<TokenUsageV1>,
    #[prost(string, tag = "3")]
    pub provider_finish_reason: String,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct LlmEventV1 {
    #[prost(message, optional, tag = "1")]
    pub metadata: Option<ProviderEventMetadataV1>,
    #[prost(oneof = "llm_event_v1::Kind", tags = "10, 11, 12, 13, 14, 15")]
    pub kind: Option<llm_event_v1::Kind>,
}

pub mod llm_event_v1 {
    use super::*;

    #[derive(Clone, PartialEq, Oneof, Serialize, Deserialize)]
    pub enum Kind {
        #[prost(message, tag = "10")]
        Started(EmptyV1),
        #[prost(message, tag = "11")]
        TextDelta(TextDeltaV1),
        #[prost(message, tag = "12")]
        SentenceReady(SentenceReadyV1),
        #[prost(message, tag = "13")]
        Completed(LlmCompletedV1),
        #[prost(message, tag = "14")]
        Failed(IpcErrorV1),
        #[prost(message, tag = "15")]
        Cancelled(CancelledV1),
    }
}

impl LlmEventV1 {
    pub fn validate(&self) -> Result<(), EventValidationError> {
        validate_metadata(self.metadata.as_ref())?;
        match self
            .kind
            .as_ref()
            .ok_or(EventValidationError::MissingKind)?
        {
            llm_event_v1::Kind::Started(_) => Ok(()),
            llm_event_v1::Kind::TextDelta(delta) => validate_text(&delta.text, false),
            llm_event_v1::Kind::SentenceReady(sentence) => {
                validate_text(&sentence.text, false)?;
                if sentence.sentence_id.is_empty() || sentence.sentence_id.len() > 256 {
                    return Err(EventValidationError::MetadataTooLarge);
                }
                if sentence.text_end_bytes <= sentence.text_start_bytes
                    || sentence.text_end_bytes - sentence.text_start_bytes
                        != sentence.text.len() as u64
                {
                    return Err(EventValidationError::InvalidTextRange);
                }
                Ok(())
            }
            llm_event_v1::Kind::Completed(completed) => {
                let reason = LlmFinishReason::try_from(completed.finish_reason)
                    .map_err(|_| EventValidationError::UnknownEnum("LLM finish reason"))?;
                if reason == LlmFinishReason::Unspecified {
                    return Err(EventValidationError::UnknownEnum("LLM finish reason"));
                }
                if completed.provider_finish_reason.len() > 256 {
                    return Err(EventValidationError::MetadataTooLarge);
                }
                Ok(())
            }
            llm_event_v1::Kind::Failed(error) => validate_error(error),
            llm_event_v1::Kind::Cancelled(cancelled) => validate_cancelled(cancelled),
        }
    }
}

#[derive(Clone, PartialEq, Eq, Message, Serialize, Deserialize)]
pub struct AudioChunkV1 {
    #[prost(uint64, tag = "1")]
    pub chunk_sequence: u64,
    #[prost(uint64, tag = "2")]
    pub start_sample: u64,
    #[prost(message, optional, tag = "3")]
    pub format: Option<AudioFormatV1>,
    #[prost(bytes = "vec", tag = "4")]
    pub data: Vec<u8>,
    #[prost(bool, tag = "5")]
    pub end_of_sentence: bool,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct VisemeV1 {
    #[prost(string, tag = "1")]
    pub id: String,
    #[prost(uint64, tag = "2")]
    pub start_ms: u64,
    #[prost(uint64, tag = "3")]
    pub duration_ms: u64,
    #[prost(float, tag = "4")]
    pub weight: f32,
}

#[derive(Clone, PartialEq, Eq, Message, Serialize, Deserialize)]
pub struct WordAlignmentV1 {
    #[prost(string, tag = "1")]
    pub text: String,
    #[prost(uint64, tag = "2")]
    pub start_ms: u64,
    #[prost(uint64, tag = "3")]
    pub duration_ms: u64,
    #[prost(uint64, tag = "4")]
    pub text_start_bytes: u64,
    #[prost(uint64, tag = "5")]
    pub text_end_bytes: u64,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct AlignmentBatchV1 {
    #[prost(message, repeated, tag = "1")]
    pub words: Vec<WordAlignmentV1>,
    #[prost(message, repeated, tag = "2")]
    pub visemes: Vec<VisemeV1>,
}

#[derive(Clone, PartialEq, Eq, Message, Serialize, Deserialize)]
pub struct TtsCompletedV1 {
    #[prost(uint64, tag = "1")]
    pub total_samples: u64,
    #[prost(uint64, tag = "2")]
    pub total_duration_ms: u64,
}

#[derive(Clone, PartialEq, Message, Serialize, Deserialize)]
pub struct TtsEventV1 {
    #[prost(message, optional, tag = "1")]
    pub metadata: Option<ProviderEventMetadataV1>,
    #[prost(string, tag = "2")]
    pub sentence_id: String,
    #[prost(oneof = "tts_event_v1::Kind", tags = "10, 11, 12, 13, 14, 15")]
    pub kind: Option<tts_event_v1::Kind>,
}

pub mod tts_event_v1 {
    use super::*;

    #[derive(Clone, PartialEq, Oneof, Serialize, Deserialize)]
    pub enum Kind {
        #[prost(message, tag = "10")]
        Started(AudioFormatV1),
        #[prost(message, tag = "11")]
        Audio(AudioChunkV1),
        #[prost(message, tag = "12")]
        Alignment(AlignmentBatchV1),
        #[prost(message, tag = "13")]
        Completed(TtsCompletedV1),
        #[prost(message, tag = "14")]
        Failed(IpcErrorV1),
        #[prost(message, tag = "15")]
        Cancelled(CancelledV1),
    }
}

impl TtsEventV1 {
    pub fn validate(&self) -> Result<(), EventValidationError> {
        validate_metadata(self.metadata.as_ref())?;
        if self.sentence_id.is_empty() || self.sentence_id.len() > 256 {
            return Err(EventValidationError::MetadataTooLarge);
        }
        match self
            .kind
            .as_ref()
            .ok_or(EventValidationError::MissingKind)?
        {
            tts_event_v1::Kind::Started(format) => format.validate(),
            tts_event_v1::Kind::Audio(chunk) => {
                if chunk.chunk_sequence == 0 {
                    return Err(EventValidationError::ZeroSequence);
                }
                chunk
                    .format
                    .as_ref()
                    .ok_or(EventValidationError::MissingAudioFormat)?
                    .validate()?;
                if chunk.data.is_empty() || chunk.data.len() > MAX_AUDIO_CHUNK_BYTES {
                    return Err(EventValidationError::AudioChunkSize(chunk.data.len()));
                }
                Ok(())
            }
            tts_event_v1::Kind::Alignment(batch) => {
                if batch.words.len() > 2_048 || batch.visemes.len() > 8_192 {
                    return Err(EventValidationError::MetadataTooLarge);
                }
                for word in &batch.words {
                    validate_text(&word.text, false)?;
                    if word.text_end_bytes <= word.text_start_bytes {
                        return Err(EventValidationError::InvalidTextRange);
                    }
                }
                for viseme in &batch.visemes {
                    if viseme.id.is_empty() || viseme.id.len() > 64 {
                        return Err(EventValidationError::MetadataTooLarge);
                    }
                    validate_unit_interval(Some(viseme.weight), "viseme weight")?;
                }
                Ok(())
            }
            tts_event_v1::Kind::Completed(_) => Ok(()),
            tts_event_v1::Kind::Failed(error) => validate_error(error),
            tts_event_v1::Kind::Cancelled(cancelled) => validate_cancelled(cancelled),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Error)]
pub enum EventValidationError {
    #[error("event kind is missing")]
    MissingKind,
    #[error("provider metadata is missing")]
    MissingMetadata,
    #[error("request id is missing or malformed")]
    InvalidRequestId,
    #[error("event sequence must be non-zero")]
    ZeroSequence,
    #[error("text is empty")]
    EmptyText,
    #[error("text is too large: {0} bytes")]
    TextTooLarge(usize),
    #[error("metadata exceeds its protocol bound")]
    MetadataTooLarge,
    #[error("unknown or unspecified {0}")]
    UnknownEnum(&'static str),
    #[error("{0} is non-finite or outside its permitted range")]
    NonFiniteOrOutOfRange(&'static str),
    #[error("end time precedes start time")]
    InvalidTimeRange,
    #[error("text byte range is invalid")]
    InvalidTextRange,
    #[error("invalid audio format: {0}")]
    InvalidAudioFormat(&'static str),
    #[error("audio format is missing")]
    MissingAudioFormat,
    #[error("audio chunk has invalid size {0}")]
    AudioChunkSize(usize),
    #[error("error message is malformed")]
    MalformedError,
    #[error("cancellation message is malformed")]
    MalformedCancellation,
}

fn validate_metadata(
    metadata: Option<&ProviderEventMetadataV1>,
) -> Result<(), EventValidationError> {
    let metadata = metadata.ok_or(EventValidationError::MissingMetadata)?;
    if metadata.provider_id.is_empty()
        || metadata.provider_id.len() > 128
        || metadata.model_id.len() > 512
    {
        return Err(EventValidationError::MetadataTooLarge);
    }
    metadata
        .request_id
        .as_ref()
        .ok_or(EventValidationError::InvalidRequestId)?
        .validate()
        .map_err(|_| EventValidationError::InvalidRequestId)?;
    if metadata.provider_sequence == 0 {
        return Err(EventValidationError::ZeroSequence);
    }
    Ok(())
}

fn validate_unit_interval(
    value: Option<f32>,
    name: &'static str,
) -> Result<(), EventValidationError> {
    if let Some(value) = value {
        if !value.is_finite() || !(0.0..=1.0).contains(&value) {
            return Err(EventValidationError::NonFiniteOrOutOfRange(name));
        }
    }
    Ok(())
}

fn validate_text(text: &str, allow_empty: bool) -> Result<(), EventValidationError> {
    if !allow_empty && text.trim().is_empty() {
        return Err(EventValidationError::EmptyText);
    }
    if text.len() > MAX_TEXT_BYTES {
        return Err(EventValidationError::TextTooLarge(text.len()));
    }
    Ok(())
}

fn validate_error(error: &IpcErrorV1) -> Result<(), EventValidationError> {
    error
        .validate()
        .map_err(|_| EventValidationError::MalformedError)
}

fn validate_cancelled(cancelled: &CancelledV1) -> Result<(), EventValidationError> {
    if cancelled.reason.len() > 1_024 {
        Err(EventValidationError::MalformedCancellation)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metadata() -> ProviderEventMetadataV1 {
        ProviderEventMetadataV1 {
            provider_id: "fixture".into(),
            model_id: "fixture-v1".into(),
            request_id: Some(RequestId::new()),
            provider_sequence: 1,
            provider_elapsed_ms: 12,
        }
    }

    #[test]
    fn final_transcripts_cannot_be_empty() {
        let event = SttEventV1 {
            metadata: Some(metadata()),
            kind: Some(stt_event_v1::Kind::Final(TranscriptSegmentV1 {
                text: "  ".into(),
                language: "en-US".into(),
                confidence: Some(0.9),
                stability: None,
                start_ms: 0,
                end_ms: 10,
                provider_segment_id: "1".into(),
            })),
        };
        assert_eq!(event.validate(), Err(EventValidationError::EmptyText));
    }

    #[test]
    fn pcm_format_must_match_bit_depth() {
        let format = AudioFormatV1 {
            encoding: AudioEncoding::PcmS16Le as i32,
            sample_rate_hz: 24_000,
            channels: 1,
            bits_per_sample: 32,
        };
        assert!(matches!(
            format.validate(),
            Err(EventValidationError::InvalidAudioFormat(_))
        ));
    }

    #[test]
    fn sentence_ranges_are_byte_accurate_for_utf8() {
        let text = "Hello, V!";
        let event = LlmEventV1 {
            metadata: Some(metadata()),
            kind: Some(llm_event_v1::Kind::SentenceReady(SentenceReadyV1 {
                sentence_id: "s1".into(),
                text: text.into(),
                text_start_bytes: 0,
                text_end_bytes: text.len() as u64,
            })),
        };
        assert_eq!(event.validate(), Ok(()));
    }

    #[test]
    fn audio_chunks_are_bounded() {
        let event = TtsEventV1 {
            metadata: Some(metadata()),
            sentence_id: "s1".into(),
            kind: Some(tts_event_v1::Kind::Audio(AudioChunkV1 {
                chunk_sequence: 1,
                start_sample: 0,
                format: Some(AudioFormatV1 {
                    encoding: AudioEncoding::PcmS16Le as i32,
                    sample_rate_hz: 24_000,
                    channels: 1,
                    bits_per_sample: 16,
                }),
                data: vec![0; MAX_AUDIO_CHUNK_BYTES + 1],
                end_of_sentence: false,
            })),
        };
        assert_eq!(
            event.validate(),
            Err(EventValidationError::AudioChunkSize(
                MAX_AUDIO_CHUNK_BYTES + 1
            ))
        );
    }
}

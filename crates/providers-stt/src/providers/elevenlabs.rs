use base64::Engine;
use serde_json::{json, Value};

use super::{
    json_frame, safe_warning, text_frame, ProviderSessionProtocol, WireEvent, WireTranscript,
};
use crate::{
    AudioEncoding, AudioFormat, ClientFrame, ConnectRequest, FlushReason, ProviderLifecycle,
    RecognitionConfig, RecognizerCapabilities, RetentionControl, RetentionPolicy, SecretString,
    ServerFrame, SttError, SttErrorKind, TranscriptStatus, TransportAuth, TurnEndReason,
};

const PROVIDER_ID: &str = "elevenlabs-scribe";
const AUDIO: &[AudioFormat] = &[
    AudioFormat {
        encoding: AudioEncoding::PcmS16Le,
        sample_rate_hz: 8_000,
        channels: 1,
    },
    AudioFormat::PCM_16KHZ_MONO,
    AudioFormat {
        encoding: AudioEncoding::PcmS16Le,
        sample_rate_hz: 22_050,
        channels: 1,
    },
    AudioFormat {
        encoding: AudioEncoding::PcmS16Le,
        sample_rate_hz: 24_000,
        channels: 1,
    },
    AudioFormat {
        encoding: AudioEncoding::PcmS16Le,
        sample_rate_hz: 44_100,
        channels: 1,
    },
    AudioFormat {
        encoding: AudioEncoding::PcmS16Le,
        sample_rate_hz: 48_000,
        channels: 1,
    },
    AudioFormat {
        encoding: AudioEncoding::G711Ulaw,
        sample_rate_hz: 8_000,
        channels: 1,
    },
];
const LANGUAGES: &[&str] = &["*"];
static CAPABILITIES: RecognizerCapabilities = RecognizerCapabilities {
    provider_id: PROVIDER_ID,
    display_name: "ElevenLabs Scribe Realtime",
    default_model: "scribe_v2_realtime",
    capability_revision: "2026-08-28",
    languages: LANGUAGES,
    accepted_audio: AUDIO,
    partial_revisions: true,
    provider_turn_detection: true,
    manual_flush: true,
    dynamic_keyword_hints: false,
    context_hints: false,
    resume_events: false,
    retention_control: RetentionControl::RequestLevel,
    sends_audio_to_provider: true,
    sends_context_to_provider: true,
    privacy_policy_url: "https://elevenlabs.io/privacy-policy",
    lifecycle: ProviderLifecycle::Stable,
    availability_note: None,
};

#[derive(Clone, Copy, Debug, Default)]
pub struct ElevenLabsScribe;

impl super::ProviderProtocol for ElevenLabsScribe {
    fn capabilities(&self) -> &'static RecognizerCapabilities {
        &CAPABILITIES
    }

    fn create_session(
        &self,
        config: &RecognitionConfig,
    ) -> Result<Box<dyn ProviderSessionProtocol>, SttError> {
        if !AUDIO.contains(&config.audio) {
            return Err(SttError::invalid_request(
                "unsupported ElevenLabs audio format",
            ));
        }
        if config.keyword_hints.len() > 50
            || config.keyword_hints.iter().any(|term| term.len() > 20)
        {
            return Err(SttError::invalid_request(
                "ElevenLabs realtime accepts at most 50 keyterms of 20 characters",
            ));
        }
        if config.context_hint.is_some() {
            return Err(SttError::invalid_request(
                "ElevenLabs Scribe Realtime does not accept a free-form context hint",
            ));
        }
        Ok(Box::new(ElevenLabsSession {
            segment: 0,
            require_zero_retention: config.retention == RetentionPolicy::RequireRequestLevelOptOut,
            configured_language: config.languages.first().cloned(),
            interim_results: config.interim_results,
        }))
    }
}

struct ElevenLabsSession {
    segment: u64,
    require_zero_retention: bool,
    configured_language: Option<String>,
    interim_results: bool,
}

impl ElevenLabsSession {
    fn turn_id(&self) -> String {
        format!("segment-{}", self.segment)
    }
}

impl ProviderSessionProtocol for ElevenLabsSession {
    fn provider_id(&self) -> &'static str {
        PROVIDER_ID
    }

    fn connect_request<'secret>(
        &self,
        config: &RecognitionConfig,
        credential: &'secret SecretString,
    ) -> Result<ConnectRequest<'secret>, SttError> {
        let audio_format = match config.audio.encoding {
            AudioEncoding::PcmS16Le => format!("pcm_{}", config.audio.sample_rate_hz),
            AudioEncoding::G711Ulaw if config.audio.sample_rate_hz == 8_000 => "ulaw_8000".into(),
            _ => {
                return Err(SttError::invalid_request(
                    "unsupported ElevenLabs audio format",
                ))
            }
        };
        let mut query = vec![
            ("model_id".into(), config.model.clone()),
            ("audio_format".into(), audio_format),
            ("include_language_detection".into(), "true".into()),
        ];
        if matches!(config.endpointing, crate::EndpointingMode::ProviderVad) {
            query.push(("commit_strategy".into(), "vad".into()));
        }
        if let Some(language) = config.languages.first() {
            query.push(("language_code".into(), language.clone()));
        }
        for keyterm in &config.keyword_hints {
            query.push(("keyterms".into(), keyterm.clone()));
        }
        if self.require_zero_retention {
            query.push(("enable_logging".into(), "false".into()));
        }
        Ok(ConnectRequest {
            url: "wss://api.elevenlabs.io/v1/speech-to-text/realtime",
            query,
            auth: TransportAuth::Header {
                name: "xi-api-key",
                scheme: None,
                value: credential,
            },
        })
    }

    fn start_frames(&mut self, _config: &RecognitionConfig) -> Result<Vec<ClientFrame>, SttError> {
        Ok(Vec::new())
    }

    fn audio_frame(&mut self, audio: &[u8]) -> Result<ClientFrame, SttError> {
        text_frame(json!({
            "message_type": "input_audio_chunk",
            "audio_base_64": base64::engine::general_purpose::STANDARD.encode(audio),
        }))
    }

    fn flush_frame(&mut self, _reason: FlushReason) -> Result<ClientFrame, SttError> {
        text_frame(json!({
            "message_type": "input_audio_chunk",
            "audio_base_64": "",
            "commit": true,
        }))
    }

    fn cancel_frame(&mut self) -> Option<ClientFrame> {
        Some(ClientFrame::Close)
    }

    fn close_frame(&mut self) -> Option<ClientFrame> {
        Some(ClientFrame::Close)
    }

    fn parse_frame(&mut self, frame: ServerFrame) -> Result<Vec<WireEvent>, SttError> {
        let value = json_frame(PROVIDER_ID, frame)?;
        let event_type = value
            .get("message_type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        match event_type {
            "session_started" => Ok(vec![WireEvent::SessionStarted {
                provider_session_id: value
                    .get("session_id")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            }]),
            "partial_transcript" if !self.interim_results => Ok(Vec::new()),
            "partial_transcript" => Ok(self.transcript(&value, TranscriptStatus::Partial, false)),
            "committed_transcript" => {
                let events = self.transcript(&value, TranscriptStatus::Final, true);
                self.segment = self.segment.saturating_add(1);
                Ok(events)
            }
            // Timestamp/language companion events are intentionally ignored: emitting a second
            // correction after the committed turn would violate the normalized final boundary.
            "committed_transcript_with_timestamps" | "committed_transcript_entities" => {
                Ok(Vec::new())
            }
            "warning" => {
                if self.require_zero_retention {
                    return Err(SttError::new(
                        PROVIDER_ID,
                        SttErrorKind::PrivacyPolicy,
                        "provider could not apply requested zero-retention mode",
                        false,
                        Some("zero_retention_not_applied"),
                    ));
                }
                Ok(vec![safe_warning(
                    "provider_warning",
                    "ElevenLabs reported a non-fatal recognition warning",
                )])
            }
            "auth_error"
            | "quota_exceeded"
            | "rate_limited"
            | "queue_overflow"
            | "resource_exhausted"
            | "session_time_limit_exceeded"
            | "input_error"
            | "invalid_request"
            | "transcriber_error"
            | "error"
            | "commit_throttled"
            | "unaccepted_terms"
            | "chunk_size_exceeded"
            | "insufficient_audio_activity" => Err(elevenlabs_error(event_type)),
            _ => Err(SttError::protocol(PROVIDER_ID, Some("unknown_event"))),
        }
    }
}

impl ElevenLabsSession {
    fn transcript(&self, value: &Value, status: TranscriptStatus, ended: bool) -> Vec<WireEvent> {
        let turn_id = self.turn_id();
        let text = value
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let mut events = vec![WireEvent::Transcript(WireTranscript {
            turn_id: turn_id.clone(),
            text,
            status,
            language: value
                .get("language_code")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .or_else(|| self.configured_language.clone()),
            confidence: None,
            audio_start_ms: None,
            audio_end_ms: None,
        })];
        if ended {
            events.push(WireEvent::TurnEnded {
                turn_id,
                reason: TurnEndReason::ProviderEndpoint,
            });
        }
        events
    }
}

fn elevenlabs_error(code: &str) -> SttError {
    let (kind, message, retryable) = match code {
        "auth_error" | "unaccepted_terms" => (
            SttErrorKind::Authentication,
            "provider authentication or terms validation failed",
            false,
        ),
        "quota_exceeded" => (
            SttErrorKind::QuotaExceeded,
            "provider quota was exhausted",
            false,
        ),
        "rate_limited" | "commit_throttled" => (
            SttErrorKind::RateLimited,
            "provider rate limit was reached",
            true,
        ),
        "input_error" | "invalid_request" | "chunk_size_exceeded" => (
            SttErrorKind::InvalidRequest,
            "provider rejected the recognition request",
            false,
        ),
        _ => (
            SttErrorKind::Unavailable,
            "provider recognition service failed",
            true,
        ),
    };
    SttError::new(PROVIDER_ID, kind, message, retryable, Some(code))
}

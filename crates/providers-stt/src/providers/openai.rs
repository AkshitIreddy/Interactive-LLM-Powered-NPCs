use std::collections::HashMap;

use base64::Engine;
use serde_json::{json, Value};

use super::{
    json_frame, safe_warning, text_frame, ProviderSessionProtocol, WireEvent, WireTranscript,
};
use crate::{
    AudioEncoding, AudioFormat, ClientFrame, ConnectRequest, EndpointingMode, FlushReason,
    ProviderLifecycle, RecognitionConfig, RecognizerCapabilities, RetentionControl,
    RetentionPolicy, SecretString, ServerFrame, SttError, SttErrorKind, TranscriptStatus,
    TransportAuth, TurnEndReason,
};

const PROVIDER_ID: &str = "openai-realtime-transcription";
const AUDIO: &[AudioFormat] = &[
    AudioFormat {
        encoding: AudioEncoding::PcmS16Le,
        sample_rate_hz: 24_000,
        channels: 1,
    },
    AudioFormat {
        encoding: AudioEncoding::G711Ulaw,
        sample_rate_hz: 8_000,
        channels: 1,
    },
    AudioFormat {
        encoding: AudioEncoding::G711Alaw,
        sample_rate_hz: 8_000,
        channels: 1,
    },
];
const LANGUAGES: &[&str] = &["*"];
static CAPABILITIES: RecognizerCapabilities = RecognizerCapabilities {
    provider_id: PROVIDER_ID,
    display_name: "OpenAI Realtime Transcription",
    default_model: "gpt-4o-mini-transcribe",
    capability_revision: "2026-08-28",
    languages: LANGUAGES,
    accepted_audio: AUDIO,
    partial_revisions: true,
    provider_turn_detection: true,
    manual_flush: true,
    dynamic_keyword_hints: false,
    context_hints: true,
    resume_events: false,
    retention_control: RetentionControl::AccountLevel,
    sends_audio_to_provider: true,
    sends_context_to_provider: true,
    privacy_policy_url: "https://openai.com/policies/privacy-policy/",
    lifecycle: ProviderLifecycle::Stable,
    availability_note: None,
};

#[derive(Clone, Copy, Debug, Default)]
pub struct OpenAiRealtime;

impl super::ProviderProtocol for OpenAiRealtime {
    fn capabilities(&self) -> &'static RecognizerCapabilities {
        &CAPABILITIES
    }

    fn create_session(
        &self,
        config: &RecognitionConfig,
    ) -> Result<Box<dyn ProviderSessionProtocol>, SttError> {
        if !AUDIO.contains(&config.audio) {
            return Err(SttError::invalid_request(
                "unsupported OpenAI realtime audio format",
            ));
        }
        if config.retention == RetentionPolicy::RequireRequestLevelOptOut {
            return Err(SttError::new(
                PROVIDER_ID,
                SttErrorKind::PrivacyPolicy,
                "OpenAI zero retention is controlled by organization policy, not a request flag",
                false,
                None,
            ));
        }
        Ok(Box::new(OpenAiSession {
            accumulated: HashMap::new(),
            interim_results: config.interim_results,
        }))
    }
}

struct OpenAiSession {
    accumulated: HashMap<String, String>,
    interim_results: bool,
}

impl ProviderSessionProtocol for OpenAiSession {
    fn provider_id(&self) -> &'static str {
        PROVIDER_ID
    }

    fn connect_request<'secret>(
        &self,
        _config: &RecognitionConfig,
        credential: &'secret SecretString,
    ) -> Result<ConnectRequest<'secret>, SttError> {
        Ok(ConnectRequest {
            url: "wss://api.openai.com/v1/realtime",
            query: vec![("intent".into(), "transcription".into())],
            auth: TransportAuth::Header {
                name: "Authorization",
                scheme: Some("Bearer"),
                value: credential,
            },
        })
    }

    fn start_frames(&mut self, config: &RecognitionConfig) -> Result<Vec<ClientFrame>, SttError> {
        let format = match config.audio.encoding {
            AudioEncoding::PcmS16Le => "pcm16",
            AudioEncoding::G711Ulaw => "g711_ulaw",
            AudioEncoding::G711Alaw => "g711_alaw",
        };
        let prompt = match (&config.context_hint, config.keyword_hints.is_empty()) {
            (Some(context), false) => Some(format!(
                "{context}\nExpected terms: {}",
                config.keyword_hints.join(", ")
            )),
            (Some(context), true) => Some(context.clone()),
            (None, false) => Some(config.keyword_hints.join(", ")),
            (None, true) => None,
        };
        let turn_detection = match config.endpointing {
            EndpointingMode::Manual => Value::Null,
            EndpointingMode::ProviderVad => json!({ "type": "server_vad" }),
        };
        let frame = json!({
            "type": "transcription_session.update",
            "session": {
                "input_audio_format": format,
                "input_audio_transcription": {
                    "model": config.model,
                    "language": config.languages.first(),
                    "prompt": prompt,
                },
                "turn_detection": turn_detection,
            }
        });
        Ok(vec![text_frame(frame)?])
    }

    fn audio_frame(&mut self, audio: &[u8]) -> Result<ClientFrame, SttError> {
        text_frame(json!({
            "type": "input_audio_buffer.append",
            "audio": base64::engine::general_purpose::STANDARD.encode(audio),
        }))
    }

    fn flush_frame(&mut self, _reason: FlushReason) -> Result<ClientFrame, SttError> {
        text_frame(json!({ "type": "input_audio_buffer.commit" }))
    }

    fn cancel_frame(&mut self) -> Option<ClientFrame> {
        text_frame(json!({ "type": "input_audio_buffer.clear" })).ok()
    }

    fn close_frame(&mut self) -> Option<ClientFrame> {
        Some(ClientFrame::Close)
    }

    fn parse_frame(&mut self, frame: ServerFrame) -> Result<Vec<WireEvent>, SttError> {
        let value = json_frame(PROVIDER_ID, frame)?;
        let event_type = value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        match event_type {
            "transcription_session.created" | "session.created" => {
                Ok(vec![WireEvent::SessionStarted {
                    provider_session_id: value
                        .get("session")
                        .and_then(|session| session.get("id"))
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                }])
            }
            "transcription_session.updated" | "session.updated" => Ok(Vec::new()),
            "input_audio_buffer.speech_started" => value
                .get("item_id")
                .and_then(Value::as_str)
                .map(|turn_id| {
                    vec![WireEvent::TurnStarted {
                        turn_id: turn_id.to_owned(),
                    }]
                })
                .ok_or_else(|| SttError::protocol(PROVIDER_ID, Some("missing_item_id"))),
            "input_audio_buffer.speech_stopped"
            | "input_audio_buffer.committed"
            | "input_audio_buffer.cleared" => Ok(Vec::new()),
            "conversation.item.input_audio_transcription.delta" => self.parse_delta(&value),
            "conversation.item.input_audio_transcription.completed" => self.parse_completed(&value),
            "conversation.item.input_audio_transcription.failed" => Err(SttError::new(
                PROVIDER_ID,
                SttErrorKind::Unavailable,
                "provider transcription failed",
                true,
                value
                    .get("error")
                    .and_then(|error| error.get("code"))
                    .and_then(Value::as_str),
            )),
            "error" => Err(openai_error(&value)),
            "warning" => Ok(vec![safe_warning(
                value
                    .get("code")
                    .and_then(Value::as_str)
                    .unwrap_or("warning"),
                "OpenAI reported a non-fatal recognition warning",
            )]),
            _ => Err(SttError::protocol(PROVIDER_ID, Some("unknown_event"))),
        }
    }
}

impl OpenAiSession {
    fn parse_delta(&mut self, value: &Value) -> Result<Vec<WireEvent>, SttError> {
        let turn_id = item_id(value)?;
        let delta = value
            .get("delta")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let complete = self.accumulated.entry(turn_id.clone()).or_default();
        complete.push_str(delta);
        if !self.interim_results || delta.is_empty() {
            return Ok(Vec::new());
        }
        Ok(vec![WireEvent::Transcript(WireTranscript {
            turn_id,
            text: complete.clone(),
            status: TranscriptStatus::Partial,
            language: None,
            confidence: None,
            audio_start_ms: None,
            audio_end_ms: None,
        })])
    }

    fn parse_completed(&mut self, value: &Value) -> Result<Vec<WireEvent>, SttError> {
        let turn_id = item_id(value)?;
        let transcript = value
            .get("transcript")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| self.accumulated.remove(&turn_id))
            .unwrap_or_default();
        self.accumulated.remove(&turn_id);
        Ok(vec![
            WireEvent::Transcript(WireTranscript {
                turn_id: turn_id.clone(),
                text: transcript,
                status: TranscriptStatus::Final,
                language: value
                    .get("language")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                confidence: None,
                audio_start_ms: None,
                audio_end_ms: None,
            }),
            WireEvent::TurnEnded {
                turn_id,
                reason: TurnEndReason::ProviderEndpoint,
            },
        ])
    }
}

fn item_id(value: &Value) -> Result<String, SttError> {
    value
        .get("item_id")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| SttError::protocol(PROVIDER_ID, Some("missing_item_id")))
}

fn openai_error(value: &Value) -> SttError {
    let error = value.get("error").unwrap_or(value);
    let code = error
        .get("code")
        .or_else(|| error.get("type"))
        .and_then(Value::as_str)
        .unwrap_or("error");
    let lower = code.to_ascii_lowercase();
    let (kind, message, retryable) = if lower.contains("auth") || lower.contains("api_key") {
        (
            SttErrorKind::Authentication,
            "provider authentication failed",
            false,
        )
    } else if lower.contains("rate") {
        (
            SttErrorKind::RateLimited,
            "provider rate limit was reached",
            true,
        )
    } else if lower.contains("quota") {
        (
            SttErrorKind::QuotaExceeded,
            "provider quota was exhausted",
            false,
        )
    } else if lower.contains("invalid") {
        (
            SttErrorKind::InvalidRequest,
            "provider rejected the recognition request",
            false,
        )
    } else {
        (
            SttErrorKind::Unavailable,
            "provider recognition service failed",
            true,
        )
    };
    SttError::new(PROVIDER_ID, kind, message, retryable, Some(code))
}

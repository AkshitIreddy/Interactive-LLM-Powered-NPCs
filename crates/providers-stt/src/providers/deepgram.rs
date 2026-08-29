use serde_json::{json, Value};

use super::{
    json_frame, safe_warning, seconds_to_ms, text_frame, ProviderSessionProtocol, WireEvent,
    WireTranscript,
};
use crate::{
    AudioEncoding, AudioFormat, ClientFrame, ConnectRequest, FlushReason, ProviderLifecycle,
    RecognitionConfig, RecognizerCapabilities, RetentionControl, RetentionPolicy, SecretString,
    ServerFrame, SttError, SttErrorKind, TranscriptStatus, TransportAuth, TurnEndReason,
};

const PROVIDER_ID: &str = "deepgram-flux";
const AUDIO: &[AudioFormat] = &[
    AudioFormat::PCM_16KHZ_MONO,
    AudioFormat {
        encoding: AudioEncoding::PcmS16Le,
        sample_rate_hz: 24_000,
        channels: 1,
    },
    AudioFormat {
        encoding: AudioEncoding::PcmS16Le,
        sample_rate_hz: 48_000,
        channels: 1,
    },
];
const LANGUAGES: &[&str] = &["en", "multi"];
static CAPABILITIES: RecognizerCapabilities = RecognizerCapabilities {
    provider_id: PROVIDER_ID,
    display_name: "Deepgram Flux",
    default_model: "flux-general-en",
    capability_revision: "2026-08-28",
    languages: LANGUAGES,
    accepted_audio: AUDIO,
    partial_revisions: true,
    provider_turn_detection: true,
    manual_flush: false,
    dynamic_keyword_hints: true,
    context_hints: false,
    resume_events: true,
    retention_control: RetentionControl::RequestLevel,
    sends_audio_to_provider: true,
    sends_context_to_provider: true,
    privacy_policy_url: "https://deepgram.com/privacy",
    lifecycle: ProviderLifecycle::Stable,
    availability_note: None,
};

#[derive(Clone, Copy, Debug, Default)]
pub struct DeepgramFlux;

impl super::ProviderProtocol for DeepgramFlux {
    fn capabilities(&self) -> &'static RecognizerCapabilities {
        &CAPABILITIES
    }

    fn create_session(
        &self,
        config: &RecognitionConfig,
    ) -> Result<Box<dyn ProviderSessionProtocol>, SttError> {
        if config.audio.encoding != AudioEncoding::PcmS16Le {
            return Err(SttError::invalid_request(
                "Deepgram Flux requires linear PCM audio",
            ));
        }
        if config.context_hint.is_some() {
            return Err(SttError::invalid_request(
                "Deepgram Flux supports keyterms but not free-form context hints",
            ));
        }
        Ok(Box::new(DeepgramSession {
            interim_results: config.interim_results,
        }))
    }
}

struct DeepgramSession {
    interim_results: bool,
}

impl ProviderSessionProtocol for DeepgramSession {
    fn provider_id(&self) -> &'static str {
        PROVIDER_ID
    }

    fn connect_request<'secret>(
        &self,
        config: &RecognitionConfig,
        credential: &'secret SecretString,
    ) -> Result<ConnectRequest<'secret>, SttError> {
        let mut query = vec![
            ("model".into(), config.model.clone()),
            ("encoding".into(), "linear16".into()),
            (
                "sample_rate".into(),
                config.audio.sample_rate_hz.to_string(),
            ),
        ];
        if matches!(config.retention, RetentionPolicy::RequireRequestLevelOptOut) {
            query.push(("mip_opt_out".into(), "true".into()));
        }
        if config.model.contains("multi") {
            for language in &config.languages {
                query.push(("language_hint".into(), language.clone()));
            }
        } else if config
            .languages
            .iter()
            .any(|language| !language.starts_with("en"))
        {
            return Err(SttError::invalid_request(
                "flux-general-en only accepts English language hints",
            ));
        }
        for keyterm in &config.keyword_hints {
            query.push(("keyterm".into(), keyterm.clone()));
        }
        Ok(ConnectRequest {
            url: "wss://api.deepgram.com/v2/listen",
            query,
            auth: TransportAuth::Header {
                name: "Authorization",
                scheme: Some("Token"),
                value: credential,
            },
        })
    }

    fn start_frames(&mut self, _config: &RecognitionConfig) -> Result<Vec<ClientFrame>, SttError> {
        Ok(Vec::new())
    }

    fn audio_frame(&mut self, audio: &[u8]) -> Result<ClientFrame, SttError> {
        Ok(ClientFrame::Binary(audio.to_vec()))
    }

    fn flush_frame(&mut self, _reason: FlushReason) -> Result<ClientFrame, SttError> {
        // Flux exposes CloseStream rather than a persistent force-endpoint event.
        text_frame(json!({ "type": "CloseStream" }))
    }

    fn cancel_frame(&mut self) -> Option<ClientFrame> {
        Some(ClientFrame::Close)
    }

    fn close_frame(&mut self) -> Option<ClientFrame> {
        text_frame(json!({ "type": "CloseStream" })).ok()
    }

    fn parse_frame(&mut self, frame: ServerFrame) -> Result<Vec<WireEvent>, SttError> {
        let value = json_frame(PROVIDER_ID, frame)?;
        let event_type = value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        match event_type {
            "Connected" => Ok(vec![WireEvent::SessionStarted {
                provider_session_id: string(&value, "request_id"),
            }]),
            "TurnInfo" => self.parse_turn(value),
            "ConfigureSuccess" => Ok(Vec::new()),
            "ConfigureFailure" => Err(SttError::new(
                PROVIDER_ID,
                SttErrorKind::InvalidRequest,
                "provider rejected dynamic recognition configuration",
                false,
                Some("configure_failure"),
            )),
            "Error" => Err(provider_error(&value)),
            "Warning" => Ok(vec![safe_warning(
                value
                    .get("code")
                    .and_then(Value::as_str)
                    .unwrap_or("warning"),
                "Deepgram reported a non-fatal recognition warning",
            )]),
            _ => Err(SttError::protocol(PROVIDER_ID, Some("unknown_event"))),
        }
    }
}

impl DeepgramSession {
    fn parse_turn(&self, value: Value) -> Result<Vec<WireEvent>, SttError> {
        let event = value
            .get("event")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let turn_id = value
            .get("turn_index")
            .and_then(Value::as_u64)
            .map(|index| format!("turn-{index}"))
            .ok_or_else(|| SttError::protocol(PROVIDER_ID, Some("missing_turn")))?;
        let transcript = value
            .get("transcript")
            .and_then(Value::as_str)
            .unwrap_or_default();

        match event {
            "StartOfTurn" => Ok(vec![WireEvent::TurnStarted { turn_id }]),
            "TurnResumed" => Ok(vec![WireEvent::TurnResumed { turn_id }]),
            "Update" if !self.interim_results || transcript.is_empty() => Ok(Vec::new()),
            "Update" => Ok(vec![WireEvent::Transcript(wire_transcript(
                &value,
                turn_id,
                transcript,
                TranscriptStatus::Partial,
            ))]),
            "EagerEndOfTurn" => Ok(vec![WireEvent::Transcript(wire_transcript(
                &value,
                turn_id,
                transcript,
                TranscriptStatus::EagerFinal,
            ))]),
            "EndOfTurn" => Ok(vec![
                WireEvent::Transcript(wire_transcript(
                    &value,
                    turn_id.clone(),
                    transcript,
                    TranscriptStatus::Final,
                )),
                WireEvent::TurnEnded {
                    turn_id,
                    reason: TurnEndReason::ProviderEndpoint,
                },
            ]),
            _ => Err(SttError::protocol(PROVIDER_ID, Some("unknown_turn_event"))),
        }
    }
}

fn wire_transcript(
    value: &Value,
    turn_id: String,
    transcript: &str,
    status: TranscriptStatus,
) -> WireTranscript {
    WireTranscript {
        turn_id,
        text: transcript.to_owned(),
        status,
        language: value
            .get("languages")
            .and_then(Value::as_array)
            .and_then(|languages| languages.first())
            .and_then(Value::as_str)
            .map(str::to_owned),
        confidence: value
            .get("end_of_turn_confidence")
            .and_then(Value::as_f64)
            .map(|value| value as f32),
        audio_start_ms: seconds_to_ms(value.get("audio_window_start").and_then(Value::as_f64)),
        audio_end_ms: seconds_to_ms(value.get("audio_window_end").and_then(Value::as_f64)),
    }
}

fn provider_error(value: &Value) -> SttError {
    let code = value.get("code").and_then(Value::as_str).unwrap_or("error");
    let upper = code.to_ascii_uppercase();
    let (kind, message, retryable) = if upper.contains("AUTH") {
        (
            SttErrorKind::Authentication,
            "provider authentication failed",
            false,
        )
    } else if upper.contains("RATE") {
        (
            SttErrorKind::RateLimited,
            "provider rate limit was reached",
            true,
        )
    } else if upper.contains("INVALID") || upper.contains("BAD_REQUEST") {
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

fn string(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_owned)
}

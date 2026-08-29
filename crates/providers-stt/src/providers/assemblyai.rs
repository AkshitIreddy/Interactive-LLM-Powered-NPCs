use serde_json::{json, Value};

use super::{
    json_frame, safe_warning, text_frame, ProviderSessionProtocol, WireEvent, WireTranscript,
};
use crate::{
    AudioFormat, ClientFrame, ConnectRequest, FlushReason, ProviderLifecycle, RecognitionConfig,
    RecognizerCapabilities, RetentionControl, RetentionPolicy, SecretString, ServerFrame, SttError,
    SttErrorKind, TranscriptStatus, TransportAuth, TurnEndReason,
};

const PROVIDER_ID: &str = "assemblyai";
const AUDIO: &[AudioFormat] = &[AudioFormat::PCM_16KHZ_MONO];
const LANGUAGES: &[&str] = &["en", "es", "de", "fr", "pt", "it", "multi"];
static CAPABILITIES: RecognizerCapabilities = RecognizerCapabilities {
    provider_id: PROVIDER_ID,
    display_name: "AssemblyAI Universal Streaming",
    default_model: "universal-3-pro",
    capability_revision: "2026-08-28",
    languages: LANGUAGES,
    accepted_audio: AUDIO,
    partial_revisions: true,
    provider_turn_detection: true,
    manual_flush: true,
    dynamic_keyword_hints: true,
    context_hints: true,
    resume_events: false,
    retention_control: RetentionControl::AccountLevel,
    sends_audio_to_provider: true,
    sends_context_to_provider: true,
    privacy_policy_url: "https://www.assemblyai.com/legal/privacy-policy",
    lifecycle: ProviderLifecycle::Stable,
    availability_note: None,
};

#[derive(Clone, Copy, Debug, Default)]
pub struct AssemblyAi;

impl super::ProviderProtocol for AssemblyAi {
    fn capabilities(&self) -> &'static RecognizerCapabilities {
        &CAPABILITIES
    }

    fn create_session(
        &self,
        config: &RecognitionConfig,
    ) -> Result<Box<dyn ProviderSessionProtocol>, SttError> {
        if config.audio != AudioFormat::PCM_16KHZ_MONO {
            return Err(SttError::invalid_request(
                "AssemblyAI adapter currently requires 16 kHz mono PCM",
            ));
        }
        if config.retention == RetentionPolicy::RequireRequestLevelOptOut {
            return Err(SttError::new(
                PROVIDER_ID,
                SttErrorKind::PrivacyPolicy,
                "AssemblyAI retention is controlled by account policy, not a request flag",
                false,
                None,
            ));
        }
        Ok(Box::new(AssemblySession {
            interim_results: config.interim_results,
        }))
    }
}

struct AssemblySession {
    interim_results: bool,
}

impl ProviderSessionProtocol for AssemblySession {
    fn provider_id(&self) -> &'static str {
        PROVIDER_ID
    }

    fn connect_request<'secret>(
        &self,
        config: &RecognitionConfig,
        credential: &'secret SecretString,
    ) -> Result<ConnectRequest<'secret>, SttError> {
        let mut query = vec![
            (
                "sample_rate".into(),
                config.audio.sample_rate_hz.to_string(),
            ),
            ("speech_model".into(), config.model.clone()),
            ("format_turns".into(), "true".into()),
            ("language_detection".into(), "true".into()),
        ];
        for keyterm in &config.keyword_hints {
            query.push(("keyterms_prompt".into(), keyterm.clone()));
        }
        if let Some(context) = &config.context_hint {
            query.push(("prompt".into(), context.clone()));
        }
        Ok(ConnectRequest {
            url: "wss://streaming.assemblyai.com/v3/ws",
            query,
            auth: TransportAuth::Header {
                name: "Authorization",
                scheme: None,
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
        text_frame(json!({ "type": "ForceEndpoint" }))
    }

    fn cancel_frame(&mut self) -> Option<ClientFrame> {
        text_frame(json!({ "type": "Terminate" })).ok()
    }

    fn close_frame(&mut self) -> Option<ClientFrame> {
        text_frame(json!({ "type": "Terminate" })).ok()
    }

    fn parse_frame(&mut self, frame: ServerFrame) -> Result<Vec<WireEvent>, SttError> {
        let value = json_frame(PROVIDER_ID, frame)?;
        match value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default()
        {
            "Begin" => Ok(vec![WireEvent::SessionStarted {
                provider_session_id: value.get("id").and_then(Value::as_str).map(str::to_owned),
            }]),
            "Turn" => self.parse_turn(value),
            "Termination" => Ok(Vec::new()),
            "Warning" => Ok(vec![safe_warning(
                value
                    .get("code")
                    .and_then(Value::as_str)
                    .unwrap_or("warning"),
                "AssemblyAI reported a non-fatal recognition warning",
            )]),
            "Error" => Err(assembly_error(&value)),
            _ => Err(SttError::protocol(PROVIDER_ID, Some("unknown_event"))),
        }
    }
}

impl AssemblySession {
    fn parse_turn(&self, value: Value) -> Result<Vec<WireEvent>, SttError> {
        let order = value
            .get("turn_order")
            .and_then(Value::as_u64)
            .ok_or_else(|| SttError::protocol(PROVIDER_ID, Some("missing_turn")))?;
        let turn_id = format!("turn-{order}");
        let transcript = value
            .get("transcript")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if transcript.is_empty() {
            return Ok(Vec::new());
        }
        let end_of_turn = value
            .get("end_of_turn")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let eager = !end_of_turn
            && value
                .get("utterance")
                .and_then(Value::as_str)
                .is_some_and(|utterance| !utterance.is_empty());
        if !self.interim_results && !end_of_turn && !eager {
            return Ok(Vec::new());
        }
        let status = if end_of_turn {
            TranscriptStatus::Final
        } else if eager {
            TranscriptStatus::EagerFinal
        } else {
            TranscriptStatus::Partial
        };
        let transcript_event = WireEvent::Transcript(WireTranscript {
            turn_id: turn_id.clone(),
            text: transcript.to_owned(),
            status,
            language: value
                .get("language_code")
                .and_then(Value::as_str)
                .map(str::to_owned),
            confidence: value
                .get("end_of_turn_confidence")
                .and_then(Value::as_f64)
                .map(|value| value as f32),
            audio_start_ms: None,
            audio_end_ms: None,
        });
        if end_of_turn {
            Ok(vec![
                transcript_event,
                WireEvent::TurnEnded {
                    turn_id,
                    reason: TurnEndReason::ProviderEndpoint,
                },
            ])
        } else {
            Ok(vec![transcript_event])
        }
    }
}

fn assembly_error(value: &Value) -> SttError {
    let code = value
        .get("code")
        .or_else(|| value.get("error"))
        .and_then(Value::as_str)
        .unwrap_or("error");
    let lower = code.to_ascii_lowercase();
    let (kind, message, retryable) = if lower.contains("auth") {
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
    } else {
        (
            SttErrorKind::Unavailable,
            "provider recognition service failed",
            true,
        )
    };
    SttError::new(PROVIDER_ID, kind, message, retryable, Some(code))
}

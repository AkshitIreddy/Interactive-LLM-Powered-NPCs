use std::{collections::BTreeMap, sync::Arc, time::Duration};

use async_trait::async_trait;

use crate::{
    AudioFormat, CartesiaCommand, CredentialResolveError, DeepgramCommand, ElevenLabsCommand,
    HostedTtsProviderId, InworldCommand, OpenRequest, PcmChunk, ProviderCapabilities,
    ProviderCredentialResolver, PushOutcome, SemanticClauseBuffer, SensitiveHeaderValue,
    SensitiveString, SessionIdentity, SessionState, StreamingTtsProvider, StreamingTtsSession,
    TransportError, TtsConnection, TtsError, TtsErrorKind, TtsEvent, TtsSessionRequest,
    TtsTransport, VoiceBindings, WireCommand, WireEvent, MAX_AUDIO_CHUNK_BYTES,
    MAX_PUSH_TEXT_CHARS,
};

const CANCEL_SIGNAL_TIMEOUT: Duration = Duration::from_millis(50);
const MAX_EVENT_TIMELINE_MS: u64 = 4 * 60 * 60 * 1_000;
const MAX_EVENT_DURATION_MS: u64 = 60_000;

#[derive(Clone, Debug)]
pub struct CartesiaConfig {
    pub endpoint: String,
    pub api_version: String,
}

impl Default for CartesiaConfig {
    fn default() -> Self {
        Self {
            endpoint: "wss://api.cartesia.ai/tts/websocket".into(),
            api_version: "2026-03-01".into(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ElevenLabsConfig {
    pub endpoint_root: String,
    /// `false` requests account-gated Zero Retention Mode. Ordinary routes
    /// should leave this `true` unless the selected account supports ZRM.
    pub enable_logging: bool,
    pub inactivity_timeout_seconds: u16,
}

impl Default for ElevenLabsConfig {
    fn default() -> Self {
        Self {
            endpoint_root: "wss://api.elevenlabs.io/v1/text-to-speech".into(),
            enable_logging: true,
            inactivity_timeout_seconds: 60,
        }
    }
}

#[derive(Clone, Debug)]
pub struct InworldConfig {
    pub endpoint: String,
}

impl Default for InworldConfig {
    fn default() -> Self {
        Self {
            endpoint: "wss://api.inworld.ai/tts/v1/voice:streamBidirectional".into(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct DeepgramConfig {
    pub endpoint: String,
    /// Requests model-improvement opt out. Availability and pricing remain an
    /// account-level concern surfaced by the control plane.
    pub model_improvement_opt_out: bool,
}

impl Default for DeepgramConfig {
    fn default() -> Self {
        Self {
            endpoint: "wss://api.deepgram.com/v1/speak".into(),
            model_improvement_opt_out: true,
        }
    }
}

#[derive(Clone)]
struct HostedAdapter {
    flavor: ProviderFlavor,
    transport: Arc<dyn TtsTransport>,
    credential_resolver: Arc<dyn ProviderCredentialResolver>,
    bindings: VoiceBindings,
}

#[derive(Clone)]
enum ProviderFlavor {
    Cartesia(CartesiaConfig),
    ElevenLabs(ElevenLabsConfig),
    Inworld(InworldConfig),
    Deepgram(DeepgramConfig),
}

impl ProviderFlavor {
    fn id(&self) -> HostedTtsProviderId {
        match self {
            Self::Cartesia(_) => HostedTtsProviderId::Cartesia,
            Self::ElevenLabs(_) => HostedTtsProviderId::ElevenLabs,
            Self::Inworld(_) => HostedTtsProviderId::Inworld,
            Self::Deepgram(_) => HostedTtsProviderId::Deepgram,
        }
    }

    fn capabilities(&self) -> ProviderCapabilities {
        match self {
            Self::Cartesia(_) => ProviderCapabilities {
                streaming_input: true,
                streaming_pcm: true,
                alignment: true,
                visemes_or_phonemes: true,
                cancellation: true,
                usage: false,
            },
            Self::ElevenLabs(_) => ProviderCapabilities {
                streaming_input: true,
                streaming_pcm: true,
                alignment: true,
                visemes_or_phonemes: false,
                cancellation: true,
                usage: false,
            },
            Self::Inworld(_) => ProviderCapabilities {
                streaming_input: true,
                streaming_pcm: true,
                alignment: true,
                visemes_or_phonemes: true,
                cancellation: true,
                usage: true,
            },
            Self::Deepgram(_) => ProviderCapabilities {
                streaming_input: true,
                streaming_pcm: true,
                alignment: false,
                visemes_or_phonemes: false,
                cancellation: true,
                usage: false,
            },
        }
    }

    fn validate_session_request(&self, request: &TtsSessionRequest) -> Result<(), &'static str> {
        if !matches!(self, Self::ElevenLabs(_)) {
            return Ok(());
        }
        let supported = request.output.channels == 1
            && match request.output.encoding {
                crate::PcmEncoding::PcmS16Le => {
                    matches!(
                        request.output.sample_rate_hz,
                        16_000 | 22_050 | 24_000 | 44_100
                    )
                }
                crate::PcmEncoding::MuLaw | crate::PcmEncoding::ALaw => {
                    request.output.sample_rate_hz == 8_000
                }
            };
        if supported {
            Ok(())
        } else {
            Err("unsupported_output_format")
        }
    }

    fn build_open_request(
        &self,
        credential: SensitiveString,
        binding: &crate::VoiceBinding,
        request: &TtsSessionRequest,
    ) -> OpenRequest {
        let mut public_headers = BTreeMap::new();
        let mut secret_headers = BTreeMap::new();
        let mut query = BTreeMap::new();
        let endpoint = match self {
            Self::Cartesia(config) => {
                secret_headers.insert("X-API-Key".into(), SensitiveHeaderValue::raw(credential));
                public_headers.insert("Cartesia-Version".into(), config.api_version.clone());
                config.endpoint.clone()
            }
            Self::ElevenLabs(config) => {
                secret_headers.insert("xi-api-key".into(), SensitiveHeaderValue::raw(credential));
                query.insert("model_id".into(), binding.model_id.clone());
                // ElevenLabs accepts an optional ISO 639-1 language code, not
                // a full BCP 47 locale such as `en-US`. Preserve the richer
                // locale in the provider-neutral request while sending only a
                // valid two-letter primary subtag on this wire boundary.
                if let Some(language_code) = elevenlabs_language_code(&request.locale) {
                    query.insert("language_code".into(), language_code);
                }
                query.insert("output_format".into(), elevenlabs_format(request.output));
                query.insert(
                    "sync_alignment".into(),
                    request.request_alignment.to_string(),
                );
                query.insert("enable_logging".into(), config.enable_logging.to_string());
                query.insert(
                    "inactivity_timeout".into(),
                    config.inactivity_timeout_seconds.clamp(1, 180).to_string(),
                );
                format!(
                    "{}/{}/stream-input",
                    config.endpoint_root.trim_end_matches('/'),
                    binding.voice_id
                )
            }
            Self::Inworld(config) => {
                secret_headers.insert(
                    "Authorization".into(),
                    SensitiveHeaderValue::with_scheme("Basic", credential),
                );
                config.endpoint.clone()
            }
            Self::Deepgram(config) => {
                secret_headers.insert(
                    "Authorization".into(),
                    SensitiveHeaderValue::with_scheme("Token", credential),
                );
                query.insert("model".into(), binding.model_id.clone());
                query.insert("encoding".into(), deepgram_encoding(request.output));
                query.insert(
                    "sample_rate".into(),
                    request.output.sample_rate_hz.to_string(),
                );
                query.insert(
                    "mip_opt_out".into(),
                    config.model_improvement_opt_out.to_string(),
                );
                config.endpoint.clone()
            }
        };
        OpenRequest {
            provider_id: self.id(),
            endpoint,
            public_headers,
            secret_headers,
            query,
        }
    }

    fn initialization_command(
        &self,
        binding: &crate::VoiceBinding,
        request: &TtsSessionRequest,
    ) -> Option<WireCommand> {
        match self {
            Self::Cartesia(_) | Self::Deepgram(_) => None,
            Self::ElevenLabs(_) => Some(WireCommand::ElevenLabs(ElevenLabsCommand::Initialize {
                stability: option_f32(binding, "stability", 0.5, 0.0, 1.0),
                similarity_boost: option_f32(binding, "similarity_boost", 0.75, 0.0, 1.0),
                speed: option_f32(binding, "speed", 1.0, 0.7, 1.2),
            })),
            Self::Inworld(_) => Some(WireCommand::Inworld(InworldCommand::CreateContext {
                context_id: context_id(&request.identity),
                voice_id: binding.voice_id.clone(),
                model_id: binding.model_id.clone(),
                locale: request.locale.clone(),
                output: request.output,
                request_alignment: request.request_alignment || request.request_visemes,
            })),
        }
    }

    fn text_command(
        &self,
        binding: &crate::VoiceBinding,
        request: &TtsSessionRequest,
        mut text: String,
        more_text_expected: bool,
    ) -> WireCommand {
        // Context-based providers concatenate chunks verbatim. Preserve a word
        // boundary even though the semantic buffer trims emitted clauses.
        if more_text_expected && !text.is_empty() && !text.ends_with(char::is_whitespace) {
            text.push(' ');
        }
        match self {
            Self::Cartesia(_) => WireCommand::Cartesia(CartesiaCommand::Generate {
                context_id: context_id(&request.identity),
                model_id: binding.model_id.clone(),
                voice_id: binding.voice_id.clone(),
                language: request.locale.clone(),
                output: request.output,
                transcript: SensitiveString::new(text),
                continue_generation: more_text_expected,
                add_timestamps: request.request_alignment || request.request_visemes,
            }),
            Self::ElevenLabs(_) => WireCommand::ElevenLabs(ElevenLabsCommand::Text {
                text: SensitiveString::new(text),
                try_trigger_generation: true,
            }),
            Self::Inworld(_) => WireCommand::Inworld(InworldCommand::SendText {
                context_id: context_id(&request.identity),
                text: SensitiveString::new(text),
                flush_context: !more_text_expected,
            }),
            Self::Deepgram(_) => WireCommand::Deepgram(DeepgramCommand::Speak {
                text: SensitiveString::new(text),
            }),
        }
    }

    fn finish_command(&self, identity: &SessionIdentity) -> Option<WireCommand> {
        match self {
            // Cartesia finishes by sending a continuation with `continue: false`.
            Self::Cartesia(_) => None,
            Self::ElevenLabs(_) => Some(WireCommand::ElevenLabs(ElevenLabsCommand::Finish)),
            // When semantic chunking already emitted every text clause, the
            // documented standalone flush closes the still-open context.
            Self::Inworld(_) => Some(WireCommand::Inworld(InworldCommand::FlushContext {
                context_id: context_id(identity),
            })),
            Self::Deepgram(_) => Some(WireCommand::Deepgram(DeepgramCommand::Flush)),
        }
    }

    fn cancel_command(&self, identity: &SessionIdentity) -> Option<WireCommand> {
        match self {
            Self::Cartesia(_) => Some(WireCommand::Cartesia(CartesiaCommand::Cancel {
                context_id: context_id(identity),
            })),
            // ElevenLabs has no documented per-generation cancellation message.
            // Closing the dedicated voice socket prevents finalization.
            Self::ElevenLabs(_) | Self::Inworld(_) => None,
            Self::Deepgram(_) => Some(WireCommand::Deepgram(DeepgramCommand::Clear)),
        }
    }
}

fn option_f32(
    binding: &crate::VoiceBinding,
    key: &str,
    default: f32,
    minimum: f32,
    maximum: f32,
) -> f32 {
    binding
        .provider_options
        .get(key)
        .and_then(|value| value.parse::<f32>().ok())
        .filter(|value| value.is_finite() && (minimum..=maximum).contains(value))
        .unwrap_or(default)
}

fn context_id(identity: &SessionIdentity) -> String {
    format!(
        "{}:{}:{}",
        identity.session_id, identity.turn_id, identity.cancellation_generation
    )
}

fn elevenlabs_format(output: AudioFormat) -> String {
    match output.encoding {
        crate::PcmEncoding::PcmS16Le => format!("pcm_{}", output.sample_rate_hz),
        crate::PcmEncoding::MuLaw => format!("ulaw_{}", output.sample_rate_hz),
        crate::PcmEncoding::ALaw => format!("alaw_{}", output.sample_rate_hz),
    }
}

fn elevenlabs_language_code(locale: &str) -> Option<String> {
    let primary = locale.split(['-', '_']).next()?.trim();
    (primary.len() == 2 && primary.bytes().all(|byte| byte.is_ascii_alphabetic()))
        .then(|| primary.to_ascii_lowercase())
}

fn deepgram_encoding(output: AudioFormat) -> String {
    match output.encoding {
        crate::PcmEncoding::PcmS16Le => "linear16",
        crate::PcmEncoding::MuLaw => "mulaw",
        crate::PcmEncoding::ALaw => "alaw",
    }
    .into()
}

macro_rules! provider_wrapper {
    ($name:ident, $id:expr, $config:ty, $variant:ident) => {
        #[derive(Clone)]
        pub struct $name {
            inner: HostedAdapter,
        }

        impl $name {
            #[must_use]
            pub fn new(
                transport: Arc<dyn TtsTransport>,
                credential_resolver: Arc<dyn ProviderCredentialResolver>,
                bindings: VoiceBindings,
                config: $config,
            ) -> Self {
                Self {
                    inner: HostedAdapter {
                        flavor: ProviderFlavor::$variant(config),
                        transport,
                        credential_resolver,
                        bindings,
                    },
                }
            }
        }

        #[async_trait]
        impl StreamingTtsProvider for $name {
            fn id(&self) -> HostedTtsProviderId {
                $id
            }

            fn capabilities(&self) -> ProviderCapabilities {
                self.inner.flavor.capabilities()
            }

            async fn start_session(
                &self,
                request: TtsSessionRequest,
            ) -> Result<Box<dyn StreamingTtsSession>, TtsError> {
                self.inner.start_session(request).await
            }
        }
    };
}

provider_wrapper!(
    CartesiaProvider,
    HostedTtsProviderId::Cartesia,
    CartesiaConfig,
    Cartesia
);
provider_wrapper!(
    ElevenLabsProvider,
    HostedTtsProviderId::ElevenLabs,
    ElevenLabsConfig,
    ElevenLabs
);
provider_wrapper!(
    InworldProvider,
    HostedTtsProviderId::Inworld,
    InworldConfig,
    Inworld
);
provider_wrapper!(
    DeepgramProvider,
    HostedTtsProviderId::Deepgram,
    DeepgramConfig,
    Deepgram
);

impl HostedAdapter {
    async fn start_session(
        &self,
        request: TtsSessionRequest,
    ) -> Result<Box<dyn StreamingTtsSession>, TtsError> {
        let provider_id = self.flavor.id();
        request
            .identity
            .validate()
            .and_then(|_| request.output.validate().map(|_| ()))
            .and_then(|_| request.clause_policy.validate())
            .map_err(|code| invalid(provider_id, code))?;
        self.flavor
            .validate_session_request(&request)
            .map_err(|code| invalid(provider_id, code))?;
        if request.locale.trim().is_empty() || request.locale.len() > 35 {
            return Err(invalid(provider_id, "invalid_locale"));
        }
        let binding = self
            .bindings
            .resolve(&request.voice_intent_id, provider_id)
            .cloned()
            .ok_or_else(|| invalid(provider_id, "voice_binding_missing"))?;
        let credential = self
            .credential_resolver
            .resolve(provider_id)
            .await
            .map_err(|error| map_credential(provider_id, error))?;
        if credential.is_empty() {
            return Err(TtsError::new(
                provider_id.as_str(),
                TtsErrorKind::Authentication,
                "credential_missing",
                false,
            ));
        }
        let open = self
            .flavor
            .build_open_request(credential, &binding, &request);
        let mut connection = self
            .transport
            .connect(open)
            .await
            .map_err(|error| map_transport(provider_id, error))?;
        if let Some(command) = self.flavor.initialization_command(&binding, &request) {
            connection
                .send(command)
                .await
                .map_err(|error| map_transport(provider_id, error))?;
        }

        let clause_buffer = SemanticClauseBuffer::new(request.clause_policy.clone());
        Ok(Box::new(HostedSession {
            flavor: self.flavor.clone(),
            binding,
            request,
            connection,
            clause_buffer,
            state: SessionState::AcceptingText,
            utterance_started: false,
            sequence: 0,
            pending_terminal_event: None,
        }))
    }
}

struct HostedSession {
    flavor: ProviderFlavor,
    binding: crate::VoiceBinding,
    request: TtsSessionRequest,
    connection: Box<dyn TtsConnection>,
    clause_buffer: SemanticClauseBuffer,
    state: SessionState,
    utterance_started: bool,
    sequence: u64,
    pending_terminal_event: Option<TtsEvent>,
}

impl HostedSession {
    async fn send_clause(
        &mut self,
        clause: String,
        more_text_expected: bool,
    ) -> Result<(), TtsError> {
        let command =
            self.flavor
                .text_command(&self.binding, &self.request, clause, more_text_expected);
        self.connection.send(command).await.map_err(|error| {
            self.state = SessionState::Faulted;
            map_transport(self.flavor.id(), error)
        })?;
        self.utterance_started = true;
        Ok(())
    }
}

#[async_trait]
impl StreamingTtsSession for HostedSession {
    fn provider_id(&self) -> HostedTtsProviderId {
        self.flavor.id()
    }

    fn identity(&self) -> &SessionIdentity {
        &self.request.identity
    }

    fn state(&self) -> SessionState {
        self.state
    }

    fn has_started_utterance(&self) -> bool {
        self.utterance_started
    }

    async fn push_text(&mut self, text: &str) -> Result<PushOutcome, TtsError> {
        if self.state != SessionState::AcceptingText {
            return Err(invalid(self.provider_id(), "session_not_accepting_text"));
        }
        let accepted_chars = text.chars().count();
        if accepted_chars > MAX_PUSH_TEXT_CHARS || text.contains('\0') {
            return Err(invalid(self.provider_id(), "invalid_text_chunk"));
        }
        let clauses = self.clause_buffer.push(text);
        let submitted = clauses.len();
        for clause in clauses {
            self.send_clause(clause, true).await?;
        }
        Ok(PushOutcome {
            accepted_chars,
            clauses_submitted: submitted,
            buffered_chars: self.clause_buffer.pending_chars(),
        })
    }

    async fn finish(&mut self) -> Result<(), TtsError> {
        if self.state != SessionState::AcceptingText {
            return Err(invalid(self.provider_id(), "session_cannot_finish"));
        }
        let clauses = self.clause_buffer.finish();
        if clauses.is_empty() && !self.utterance_started {
            return Err(invalid(self.provider_id(), "empty_utterance"));
        }
        let final_clause_owns_the_boundary = matches!(
            self.flavor,
            ProviderFlavor::Cartesia(_) | ProviderFlavor::Inworld(_)
        );
        if final_clause_owns_the_boundary {
            if clauses.is_empty() {
                if matches!(self.flavor, ProviderFlavor::Cartesia(_)) {
                    self.send_clause(String::new(), false).await?;
                } else if let Some(command) = self.flavor.finish_command(&self.request.identity) {
                    self.connection.send(command).await.map_err(|error| {
                        self.state = SessionState::Faulted;
                        map_transport(self.provider_id(), error)
                    })?;
                }
            } else {
                let clause_count = clauses.len();
                for (index, clause) in clauses.into_iter().enumerate() {
                    self.send_clause(clause, index + 1 < clause_count).await?;
                }
            }
        } else {
            for clause in clauses {
                self.send_clause(clause, true).await?;
            }
            if let Some(command) = self.flavor.finish_command(&self.request.identity) {
                self.connection.send(command).await.map_err(|error| {
                    self.state = SessionState::Faulted;
                    map_transport(self.provider_id(), error)
                })?;
            }
        }
        self.state = SessionState::Finishing;
        Ok(())
    }

    async fn next_event(&mut self) -> Option<Result<TtsEvent, TtsError>> {
        if let Some(event) = self.pending_terminal_event.take() {
            return Some(Ok(event));
        }
        if self.state.is_terminal() {
            return None;
        }
        loop {
            let Some(event) = self.connection.receive().await else {
                self.state = SessionState::Faulted;
                return Some(Err(TtsError::new(
                    self.provider_id().as_str(),
                    TtsErrorKind::Protocol,
                    "stream_closed_before_completion",
                    true,
                )));
            };
            match event {
                Err(error) => {
                    self.state = SessionState::Faulted;
                    return Some(Err(map_transport(self.provider_id(), error)));
                }
                Ok(WireEvent::Audio(data)) => {
                    if data.len() > MAX_AUDIO_CHUNK_BYTES {
                        self.state = SessionState::Faulted;
                        return Some(Err(TtsError::new(
                            self.provider_id().as_str(),
                            TtsErrorKind::Protocol,
                            "audio_chunk_too_large",
                            false,
                        )));
                    }
                    let sequence = self.sequence;
                    self.sequence = self.sequence.saturating_add(1);
                    return Some(Ok(TtsEvent::Audio(PcmChunk {
                        sequence,
                        format: self.request.output,
                        data,
                    })));
                }
                Ok(WireEvent::Alignment(alignment)) => {
                    if alignment.len() > 16_384
                        || alignment.iter().enumerate().any(|(index, item)| {
                            item.word.len() > 512
                                || item.end_ms < item.start_ms
                                || item.end_ms > MAX_EVENT_TIMELINE_MS
                                || item
                                    .source_text_length
                                    .is_some_and(|length| length > 16_384)
                                || (index > 0
                                    && (item.start_ms < alignment[index - 1].start_ms
                                        || item.end_ms < alignment[index - 1].end_ms))
                        })
                    {
                        self.state = SessionState::Faulted;
                        return Some(Err(TtsError::new(
                            self.provider_id().as_str(),
                            TtsErrorKind::Protocol,
                            "invalid_alignment",
                            false,
                        )));
                    }
                    return Some(Ok(TtsEvent::Alignment(alignment)));
                }
                Ok(WireEvent::Viseme(visemes)) => {
                    if visemes.len() > 32_768
                        || visemes.iter().enumerate().any(|(index, item)| {
                            let end = item.start_ms.saturating_add(item.duration_ms);
                            item.symbol.is_empty()
                                || item.symbol.len() > 64
                                || item.duration_ms > MAX_EVENT_DURATION_MS
                                || end > MAX_EVENT_TIMELINE_MS
                                || (index > 0
                                    && (item.start_ms < visemes[index - 1].start_ms
                                        || end
                                            < visemes[index - 1]
                                                .start_ms
                                                .saturating_add(visemes[index - 1].duration_ms)))
                        })
                    {
                        self.state = SessionState::Faulted;
                        return Some(Err(TtsError::new(
                            self.provider_id().as_str(),
                            TtsErrorKind::Protocol,
                            "invalid_visemes",
                            false,
                        )));
                    }
                    return Some(Ok(TtsEvent::Viseme(visemes)));
                }
                Ok(WireEvent::Usage(usage)) => {
                    if usage
                        .provider_request_id
                        .as_ref()
                        .is_some_and(|request_id| request_id.len() > 256)
                    {
                        self.state = SessionState::Faulted;
                        return Some(Err(TtsError::new(
                            self.provider_id().as_str(),
                            TtsErrorKind::Protocol,
                            "invalid_usage",
                            false,
                        )));
                    }
                    return Some(Ok(TtsEvent::Usage(usage)));
                }
                Ok(WireEvent::FlushComplete | WireEvent::Complete) => {
                    self.state = SessionState::Completed;
                    let _ignored = self.connection.close().await;
                    return Some(Ok(TtsEvent::Completed));
                }
                Ok(WireEvent::Warning { .. }) => continue,
            }
        }
    }

    async fn cancel(&mut self) -> Result<(), TtsError> {
        if self.state == SessionState::Cancelled {
            return Ok(());
        }
        if self.state.is_terminal() {
            return Err(invalid(self.provider_id(), "session_already_terminal"));
        }
        if let Some(command) = self.flavor.cancel_command(&self.request.identity) {
            // Cancellation remains authoritative even if the best-effort provider
            // command stalls or fails; the bounded send attempt may not postpone
            // the hard transport close by a full provider I/O timeout.
            let _ignored =
                tokio::time::timeout(CANCEL_SIGNAL_TIMEOUT, self.connection.send(command)).await;
        }
        let close_result = self.connection.close().await;
        self.state = SessionState::Cancelled;
        self.pending_terminal_event = Some(TtsEvent::Interrupted { reason: "barge_in" });
        close_result.map_err(|error| map_transport(self.provider_id(), error))
    }
}

fn invalid(provider_id: HostedTtsProviderId, code: &'static str) -> TtsError {
    TtsError::new(
        provider_id.as_str(),
        TtsErrorKind::InvalidRequest,
        code,
        false,
    )
}

fn map_transport(provider_id: HostedTtsProviderId, error: TransportError) -> TtsError {
    match error {
        TransportError::Unavailable | TransportError::Closed => TtsError::new(
            provider_id.as_str(),
            TtsErrorKind::Unavailable,
            "transport_unavailable",
            true,
        ),
        TransportError::Timeout => TtsError::new(
            provider_id.as_str(),
            TtsErrorKind::Timeout,
            "transport_timeout",
            true,
        ),
        TransportError::Authentication => TtsError::new(
            provider_id.as_str(),
            TtsErrorKind::Authentication,
            "authentication_failed",
            false,
        ),
        TransportError::QuotaExceeded => TtsError::new(
            provider_id.as_str(),
            TtsErrorKind::QuotaExceeded,
            "quota_exceeded",
            false,
        ),
        TransportError::RateLimited { retry_after } => {
            let mut error = TtsError::new(
                provider_id.as_str(),
                TtsErrorKind::RateLimited,
                "rate_limited",
                true,
            );
            error.retry_after = retry_after;
            error
        }
        TransportError::Protocol => TtsError::new(
            provider_id.as_str(),
            TtsErrorKind::Protocol,
            "transport_protocol_failure",
            false,
        ),
        TransportError::ProtocolStage(code) => {
            TtsError::new(provider_id.as_str(), TtsErrorKind::Protocol, code, false)
        }
    }
}

fn map_credential(provider_id: HostedTtsProviderId, error: CredentialResolveError) -> TtsError {
    match error {
        CredentialResolveError::Missing => TtsError::new(
            provider_id.as_str(),
            TtsErrorKind::Authentication,
            "credential_missing",
            false,
        ),
        CredentialResolveError::Unavailable => TtsError::new(
            provider_id.as_str(),
            TtsErrorKind::Unavailable,
            "credential_vault_unavailable",
            true,
        ),
    }
}

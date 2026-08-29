use std::{
    collections::{BTreeMap, VecDeque},
    fmt,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;

use crate::{
    AudioFormat, HostedTtsProviderId, SensitiveString, TransportError, UsageEvent, VisemeEvent,
    WordAlignment,
};

pub struct OpenRequest {
    pub provider_id: HostedTtsProviderId,
    pub endpoint: String,
    pub public_headers: BTreeMap<String, String>,
    pub secret_headers: BTreeMap<String, SensitiveHeaderValue>,
    pub query: BTreeMap<String, String>,
}

impl fmt::Debug for OpenRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OpenRequest")
            .field("provider_id", &self.provider_id)
            .field("endpoint", &self.endpoint)
            .field(
                "public_header_names",
                &self.public_headers.keys().collect::<Vec<_>>(),
            )
            .field(
                "secret_header_names",
                &self.secret_headers.keys().collect::<Vec<_>>(),
            )
            .field("query", &self.query)
            .finish()
    }
}

/// An authentication header represented without ever building a prefixed plain
/// `String`. A real transport should encode `scheme`, one ASCII space, and
/// `value` into a `Zeroizing<Vec<u8>>`, then drop it immediately after the
/// handshake has been constructed.
pub struct SensitiveHeaderValue {
    scheme: Option<&'static str>,
    value: SensitiveString,
}

impl SensitiveHeaderValue {
    #[must_use]
    pub fn raw(value: SensitiveString) -> Self {
        Self {
            scheme: None,
            value,
        }
    }

    #[must_use]
    pub fn with_scheme(scheme: &'static str, value: SensitiveString) -> Self {
        Self {
            scheme: Some(scheme),
            value,
        }
    }

    /// Explicitly exposes the two borrowed pieces for immediate wire encoding.
    #[must_use]
    pub fn expose_parts(&self) -> (Option<&'static str>, &str) {
        (self.scheme, self.value.expose())
    }
}

impl fmt::Debug for SensitiveHeaderValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[REDACTED]")
    }
}

#[derive(Debug, PartialEq)]
pub enum WireCommand {
    Cartesia(CartesiaCommand),
    ElevenLabs(ElevenLabsCommand),
    Inworld(InworldCommand),
    Deepgram(DeepgramCommand),
}

#[derive(PartialEq)]
pub enum CartesiaCommand {
    Generate {
        context_id: String,
        model_id: String,
        voice_id: String,
        language: String,
        output: AudioFormat,
        transcript: SensitiveString,
        continue_generation: bool,
        add_timestamps: bool,
    },
    Cancel {
        context_id: String,
    },
}

impl fmt::Debug for CartesiaCommand {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Generate {
                context_id,
                model_id,
                voice_id,
                language,
                output,
                transcript,
                continue_generation,
                add_timestamps,
            } => formatter
                .debug_struct("Generate")
                .field("context_id", context_id)
                .field("model_id", model_id)
                .field("voice_id", voice_id)
                .field("language", language)
                .field("output", output)
                .field("transcript", transcript)
                .field("continue_generation", continue_generation)
                .field("add_timestamps", add_timestamps)
                .finish(),
            Self::Cancel { context_id } => formatter
                .debug_struct("Cancel")
                .field("context_id", context_id)
                .finish(),
        }
    }
}

#[derive(PartialEq)]
pub enum ElevenLabsCommand {
    Initialize {
        stability: f32,
        similarity_boost: f32,
        speed: f32,
    },
    Text {
        text: SensitiveString,
        try_trigger_generation: bool,
    },
    Finish,
}

impl fmt::Debug for ElevenLabsCommand {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Initialize {
                stability,
                similarity_boost,
                speed,
            } => formatter
                .debug_struct("Initialize")
                .field("stability", stability)
                .field("similarity_boost", similarity_boost)
                .field("speed", speed)
                .finish(),
            Self::Text {
                text,
                try_trigger_generation,
            } => formatter
                .debug_struct("Text")
                .field("text", text)
                .field("try_trigger_generation", try_trigger_generation)
                .finish(),
            Self::Finish => formatter.write_str("Finish"),
        }
    }
}

#[derive(PartialEq)]
pub enum InworldCommand {
    CreateContext {
        context_id: String,
        voice_id: String,
        model_id: String,
        locale: String,
        output: AudioFormat,
        request_alignment: bool,
    },
    SendText {
        context_id: String,
        text: SensitiveString,
    },
    FlushContext {
        context_id: String,
    },
    CloseContext {
        context_id: String,
    },
}

impl fmt::Debug for InworldCommand {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CreateContext {
                context_id,
                voice_id,
                model_id,
                locale,
                output,
                request_alignment,
            } => formatter
                .debug_struct("CreateContext")
                .field("context_id", context_id)
                .field("voice_id", voice_id)
                .field("model_id", model_id)
                .field("locale", locale)
                .field("output", output)
                .field("request_alignment", request_alignment)
                .finish(),
            Self::SendText { context_id, text } => formatter
                .debug_struct("SendText")
                .field("context_id", context_id)
                .field("text", text)
                .finish(),
            Self::FlushContext { context_id } => formatter
                .debug_struct("FlushContext")
                .field("context_id", context_id)
                .finish(),
            Self::CloseContext { context_id } => formatter
                .debug_struct("CloseContext")
                .field("context_id", context_id)
                .finish(),
        }
    }
}

#[derive(PartialEq)]
pub enum DeepgramCommand {
    Speak { text: SensitiveString },
    Flush,
    Clear,
    Close,
}

impl fmt::Debug for DeepgramCommand {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Speak { text } => formatter.debug_struct("Speak").field("text", text).finish(),
            Self::Flush => formatter.write_str("Flush"),
            Self::Clear => formatter.write_str("Clear"),
            Self::Close => formatter.write_str("Close"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WireEvent {
    Audio(Vec<u8>),
    Alignment(Vec<WordAlignment>),
    Viseme(Vec<VisemeEvent>),
    Usage(UsageEvent),
    FlushComplete,
    Complete,
    Warning { code: &'static str },
}

#[async_trait]
pub trait TtsConnection: Send {
    async fn send(&mut self, command: WireCommand) -> Result<(), TransportError>;
    async fn receive(&mut self) -> Option<Result<WireEvent, TransportError>>;
    async fn close(&mut self) -> Result<(), TransportError>;
}

#[async_trait]
pub trait TtsTransport: Send + Sync {
    async fn connect(&self, request: OpenRequest)
        -> Result<Box<dyn TtsConnection>, TransportError>;
}

/// Scriptable transport for unit, replay and fixture tests. It records no
/// credential values, URLs contain no credentials, and command `Debug` output
/// redacts dialogue text.
#[derive(Clone, Default)]
pub struct MockTransport {
    shared: Arc<Mutex<MockShared>>,
}

#[derive(Default)]
struct MockShared {
    script: MockScript,
    opens: Vec<RecordedOpen>,
    commands: Vec<RecordedCommand>,
    close_count: usize,
}

#[derive(Clone, Debug, Default)]
pub struct MockScript {
    pub incoming: VecDeque<Result<WireEvent, TransportError>>,
    pub connect_error: Option<TransportError>,
    /// Zero-based send call that fails.
    pub fail_send_at: Option<usize>,
    pub close_error: Option<TransportError>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordedOpen {
    pub provider_id: HostedTtsProviderId,
    pub endpoint: String,
    pub header_names: Vec<String>,
    pub query: BTreeMap<String, String>,
}

/// Content-free command metadata retained by [`MockTransport`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecordedCommand {
    CartesiaGenerate {
        model_id: String,
        voice_id: String,
        transcript_chars: usize,
        transcript_ends_with_space: bool,
        continue_generation: bool,
        add_timestamps: bool,
    },
    CartesiaCancel,
    ElevenLabsInitialize,
    ElevenLabsText {
        text_chars: usize,
        text_ends_with_space: bool,
        try_trigger_generation: bool,
    },
    ElevenLabsFinish,
    InworldCreateContext,
    InworldSendText {
        text_chars: usize,
        text_ends_with_space: bool,
    },
    InworldFlushContext,
    InworldCloseContext,
    DeepgramSpeak {
        text_chars: usize,
        text_ends_with_space: bool,
    },
    DeepgramFlush,
    DeepgramClear,
    DeepgramClose,
}

impl WireCommand {
    fn recording(&self) -> RecordedCommand {
        match self {
            Self::Cartesia(CartesiaCommand::Generate {
                model_id,
                voice_id,
                transcript,
                continue_generation,
                add_timestamps,
                ..
            }) => RecordedCommand::CartesiaGenerate {
                model_id: model_id.clone(),
                voice_id: voice_id.clone(),
                transcript_chars: transcript.expose().chars().count(),
                transcript_ends_with_space: transcript.expose().ends_with(' '),
                continue_generation: *continue_generation,
                add_timestamps: *add_timestamps,
            },
            Self::Cartesia(CartesiaCommand::Cancel { .. }) => RecordedCommand::CartesiaCancel,
            Self::ElevenLabs(ElevenLabsCommand::Initialize { .. }) => {
                RecordedCommand::ElevenLabsInitialize
            }
            Self::ElevenLabs(ElevenLabsCommand::Text {
                text,
                try_trigger_generation,
            }) => RecordedCommand::ElevenLabsText {
                text_chars: text.expose().chars().count(),
                text_ends_with_space: text.expose().ends_with(' '),
                try_trigger_generation: *try_trigger_generation,
            },
            Self::ElevenLabs(ElevenLabsCommand::Finish) => RecordedCommand::ElevenLabsFinish,
            Self::Inworld(InworldCommand::CreateContext { .. }) => {
                RecordedCommand::InworldCreateContext
            }
            Self::Inworld(InworldCommand::SendText { text, .. }) => {
                RecordedCommand::InworldSendText {
                    text_chars: text.expose().chars().count(),
                    text_ends_with_space: text.expose().ends_with(' '),
                }
            }
            Self::Inworld(InworldCommand::FlushContext { .. }) => {
                RecordedCommand::InworldFlushContext
            }
            Self::Inworld(InworldCommand::CloseContext { .. }) => {
                RecordedCommand::InworldCloseContext
            }
            Self::Deepgram(DeepgramCommand::Speak { text }) => RecordedCommand::DeepgramSpeak {
                text_chars: text.expose().chars().count(),
                text_ends_with_space: text.expose().ends_with(' '),
            },
            Self::Deepgram(DeepgramCommand::Flush) => RecordedCommand::DeepgramFlush,
            Self::Deepgram(DeepgramCommand::Clear) => RecordedCommand::DeepgramClear,
            Self::Deepgram(DeepgramCommand::Close) => RecordedCommand::DeepgramClose,
        }
    }
}

impl MockTransport {
    #[must_use]
    pub fn scripted(script: MockScript) -> Self {
        Self {
            shared: Arc::new(Mutex::new(MockShared {
                script,
                ..MockShared::default()
            })),
        }
    }

    #[must_use]
    pub fn commands(&self) -> Vec<RecordedCommand> {
        self.shared
            .lock()
            .expect("mock mutex poisoned")
            .commands
            .clone()
    }

    #[must_use]
    pub fn opens(&self) -> Vec<RecordedOpen> {
        self.shared
            .lock()
            .expect("mock mutex poisoned")
            .opens
            .clone()
    }

    #[must_use]
    pub fn close_count(&self) -> usize {
        self.shared.lock().expect("mock mutex poisoned").close_count
    }
}

#[async_trait]
impl TtsTransport for MockTransport {
    async fn connect(
        &self,
        request: OpenRequest,
    ) -> Result<Box<dyn TtsConnection>, TransportError> {
        let mut shared = self.shared.lock().expect("mock mutex poisoned");
        if let Some(error) = shared.script.connect_error.clone() {
            return Err(error);
        }
        shared.opens.push(RecordedOpen {
            provider_id: request.provider_id,
            endpoint: request.endpoint,
            header_names: request
                .public_headers
                .into_keys()
                .chain(request.secret_headers.into_keys())
                .collect(),
            query: request.query,
        });
        Ok(Box::new(MockConnection {
            shared: Arc::clone(&self.shared),
            sends: 0,
        }))
    }
}

struct MockConnection {
    shared: Arc<Mutex<MockShared>>,
    sends: usize,
}

#[async_trait]
impl TtsConnection for MockConnection {
    async fn send(&mut self, command: WireCommand) -> Result<(), TransportError> {
        let mut shared = self.shared.lock().expect("mock mutex poisoned");
        if shared.script.fail_send_at == Some(self.sends) {
            self.sends += 1;
            return Err(TransportError::Unavailable);
        }
        self.sends += 1;
        shared.commands.push(command.recording());
        Ok(())
    }

    async fn receive(&mut self) -> Option<Result<WireEvent, TransportError>> {
        self.shared
            .lock()
            .expect("mock mutex poisoned")
            .script
            .incoming
            .pop_front()
    }

    async fn close(&mut self) -> Result<(), TransportError> {
        let mut shared = self.shared.lock().expect("mock mutex poisoned");
        shared.close_count += 1;
        shared.script.close_error.clone().map_or(Ok(()), Err)
    }
}

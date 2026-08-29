use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::Instant;

use tokio::time::{timeout, timeout_at};
use tokio_util::sync::CancellationToken;

use crate::providers::{ProviderProtocol, ProviderSessionProtocol, WireEvent, WireTranscript};
use crate::{
    ClientFrame, FlushReason, RecognitionConfig, RecognitionEvent, RecognizerCapabilities,
    SecretString, StreamingTransport, SttError, TranscriptEvent, TransportFactory,
};

/// Reusable recognizer configured with a provider protocol and an injected WebSocket factory.
pub struct HostedRecognizer<P> {
    protocol: P,
    transport_factory: Arc<dyn TransportFactory>,
}

impl<P> HostedRecognizer<P>
where
    P: ProviderProtocol,
{
    pub fn new(protocol: P, transport_factory: Arc<dyn TransportFactory>) -> Self {
        Self {
            protocol,
            transport_factory,
        }
    }

    pub fn capabilities(&self) -> &'static RecognizerCapabilities {
        self.protocol.capabilities()
    }

    pub async fn start_session(
        &self,
        config: RecognitionConfig,
        credential: SecretString,
        cancellation: CancellationToken,
    ) -> Result<HostedSession, SttError> {
        config.validate()?;
        let mut protocol = self.protocol.create_session(&config)?;
        let provider_id = protocol.provider_id();
        let request = protocol.connect_request(&config, &credential)?;
        let mut transport = timeout(
            config.timeouts.connect,
            self.transport_factory.connect(request),
        )
        .await
        .map_err(|_| SttError::timeout(provider_id, "provider connection timed out"))?
        .map_err(|_| SttError::transport(provider_id))?;

        for frame in protocol.start_frames(&config)? {
            timeout(config.timeouts.send, transport.send(frame))
                .await
                .map_err(|_| SttError::timeout(provider_id, "provider configuration timed out"))?
                .map_err(|_| SttError::transport(provider_id))?;
        }

        Ok(HostedSession {
            transport,
            protocol,
            config,
            cancellation,
            state: SessionState::Active,
            cancellation_event_pending: false,
            pending: VecDeque::new(),
            revisions: HashMap::new(),
            started_turns: HashSet::new(),
            ended_turns: HashSet::new(),
            flush_deadline: None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SessionState {
    Active,
    Cancelled,
    Closed,
}

/// One persistent provider WebSocket. Audio and transcript data never pass through a logger.
pub struct HostedSession {
    transport: Box<dyn StreamingTransport>,
    protocol: Box<dyn ProviderSessionProtocol>,
    config: RecognitionConfig,
    cancellation: CancellationToken,
    state: SessionState,
    cancellation_event_pending: bool,
    pending: VecDeque<RecognitionEvent>,
    revisions: HashMap<String, u64>,
    started_turns: HashSet<String>,
    ended_turns: HashSet<String>,
    flush_deadline: Option<Instant>,
}

impl HostedSession {
    pub fn provider_id(&self) -> &'static str {
        self.protocol.provider_id()
    }

    pub async fn send_audio(&mut self, pcm: &[u8]) -> Result<(), SttError> {
        self.ensure_active().await?;
        if pcm.is_empty() {
            return Ok(());
        }
        if pcm.len() > 1024 * 1024 {
            return Err(SttError::invalid_request("audio chunk exceeds one MiB"));
        }
        let frame = self.protocol.audio_frame(pcm)?;
        self.send_with_timeout(frame, self.config.timeouts.send, "audio send timed out")
            .await
    }

    /// Forces the current provider turn to a stable final boundary.
    pub async fn flush(&mut self, reason: FlushReason) -> Result<(), SttError> {
        self.ensure_active().await?;
        let frame = self.protocol.flush_frame(reason)?;
        self.send_with_timeout(frame, self.config.timeouts.send, "flush send timed out")
            .await?;
        self.flush_deadline = Some(Instant::now() + self.config.timeouts.flush);
        Ok(())
    }

    /// Cancels immediately, closes the socket, and guarantees no subsequent data events.
    pub async fn cancel(&mut self) -> Result<(), SttError> {
        if self.state != SessionState::Active {
            return Ok(());
        }
        self.state = SessionState::Cancelled;
        self.pending.clear();
        self.cancellation_event_pending = true;

        if let Some(frame) = self.protocol.cancel_frame() {
            let _ = timeout(self.config.timeouts.send, self.transport.send(frame)).await;
        }
        let _ = self.transport.close().await;
        Ok(())
    }

    pub async fn close(&mut self) -> Result<(), SttError> {
        if self.state == SessionState::Active {
            if let Some(frame) = self.protocol.close_frame() {
                self.send_with_timeout(frame, self.config.timeouts.send, "close send timed out")
                    .await?;
            }
            self.transport
                .close()
                .await
                .map_err(|_| SttError::transport(self.provider_id()))?;
            self.state = SessionState::Closed;
        }
        Ok(())
    }

    /// Returns one normalized event. After `Cancelled`, it permanently returns `None`.
    pub async fn next_event(&mut self) -> Result<Option<RecognitionEvent>, SttError> {
        if self.cancellation_event_pending {
            self.cancellation_event_pending = false;
            return Ok(Some(RecognitionEvent::Cancelled));
        }
        if self.state != SessionState::Active {
            return Ok(None);
        }
        if self.cancellation.is_cancelled() {
            self.cancel().await?;
            self.cancellation_event_pending = false;
            return Ok(Some(RecognitionEvent::Cancelled));
        }
        if let Some(event) = self.pending.pop_front() {
            return Ok(Some(event));
        }

        loop {
            let provider_id = self.provider_id();
            let receive = async {
                match self.flush_deadline {
                    Some(deadline) => timeout_at(deadline.into(), self.transport.receive())
                        .await
                        .map_err(|_| SttError::timeout(provider_id, "provider flush timed out"))?,
                    None => timeout(self.config.timeouts.receive_idle, self.transport.receive())
                        .await
                        .map_err(|_| {
                            SttError::timeout(provider_id, "provider receive timed out")
                        })?,
                }
                .map_err(|_| SttError::transport(provider_id))
            };

            let frame = tokio::select! {
                biased;
                _ = self.cancellation.cancelled() => {
                    self.cancel().await?;
                    self.cancellation_event_pending = false;
                    return Ok(Some(RecognitionEvent::Cancelled));
                }
                frame = receive => frame?,
            };

            let Some(frame) = frame else {
                self.state = SessionState::Closed;
                return Ok(None);
            };
            if matches!(frame, crate::ServerFrame::Closed { .. }) {
                self.state = SessionState::Closed;
                return Ok(None);
            }

            let wire_events = self.protocol.parse_frame(frame)?;
            for event in wire_events {
                self.normalize(event)?;
            }
            if let Some(event) = self.pending.pop_front() {
                return Ok(Some(event));
            }
        }
    }

    async fn ensure_active(&mut self) -> Result<(), SttError> {
        if self.cancellation.is_cancelled() && self.state == SessionState::Active {
            self.cancel().await?;
        }
        if self.state == SessionState::Active {
            Ok(())
        } else {
            Err(SttError::cancelled(self.provider_id()))
        }
    }

    async fn send_with_timeout(
        &mut self,
        frame: ClientFrame,
        duration: std::time::Duration,
        timeout_message: &'static str,
    ) -> Result<(), SttError> {
        let provider_id = self.provider_id();
        timeout(duration, self.transport.send(frame))
            .await
            .map_err(|_| SttError::timeout(provider_id, timeout_message))?
            .map_err(|_| SttError::transport(provider_id))
    }

    fn normalize(&mut self, event: WireEvent) -> Result<(), SttError> {
        match event {
            WireEvent::SessionStarted {
                provider_session_id,
            } => self.pending.push_back(RecognitionEvent::SessionStarted {
                provider_session_id,
            }),
            WireEvent::TurnStarted { turn_id } => self.ensure_turn_started(turn_id),
            WireEvent::Transcript(transcript) => self.normalize_transcript(transcript),
            WireEvent::TurnResumed { turn_id } => {
                self.ensure_turn_started(turn_id.clone());
                let revision = self.next_revision(&turn_id);
                self.pending
                    .push_back(RecognitionEvent::TurnResumed { turn_id, revision });
            }
            WireEvent::TurnEnded { turn_id, reason } => {
                self.ensure_turn_started(turn_id.clone());
                if self.ended_turns.insert(turn_id.clone()) {
                    let revision = *self.revisions.get(&turn_id).unwrap_or(&0);
                    let reason = if self.flush_deadline.is_some() {
                        crate::TurnEndReason::ManualFlush
                    } else {
                        reason
                    };
                    self.pending.push_back(RecognitionEvent::TurnEnded {
                        turn_id,
                        revision,
                        reason,
                    });
                    self.flush_deadline = None;
                }
            }
            WireEvent::Warning { code, message } => self
                .pending
                .push_back(RecognitionEvent::Warning { code, message }),
        }
        Ok(())
    }

    fn normalize_transcript(&mut self, transcript: WireTranscript) {
        self.ensure_turn_started(transcript.turn_id.clone());
        let revision = self.next_revision(&transcript.turn_id);
        self.pending
            .push_back(RecognitionEvent::Transcript(TranscriptEvent {
                turn_id: transcript.turn_id,
                revision,
                text: transcript.text,
                status: transcript.status,
                language: transcript.language,
                confidence: transcript.confidence,
                audio_start_ms: transcript.audio_start_ms,
                audio_end_ms: transcript.audio_end_ms,
            }));
    }

    fn ensure_turn_started(&mut self, turn_id: String) {
        if self.started_turns.insert(turn_id.clone()) {
            self.pending
                .push_back(RecognitionEvent::TurnStarted { turn_id });
        }
    }

    fn next_revision(&mut self, turn_id: &str) -> u64 {
        let revision = self.revisions.entry(turn_id.to_owned()).or_default();
        *revision = revision.saturating_add(1);
        *revision
    }
}

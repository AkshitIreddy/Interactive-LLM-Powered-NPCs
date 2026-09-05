//! Provider-bound timing instrumentation for product benchmark receipts.
//!
//! Every timestamp is captured at the producer boundary it describes. The
//! terminal simulation result never fabricates first-token or first-PCM times.

use std::sync::{Arc, Mutex};
#[cfg(not(windows))]
use std::time::Instant;

use async_trait::async_trait;
use futures_util::StreamExt;
use npc_providers_llm::{
    HostedLanguageModel, LlmEvent, LlmEventStream, LlmRequest, ModelInfo, ProviderCapabilities,
    ProviderError as HostedLlmError,
};
use npc_providers_tts::{
    HostedTtsProviderId, ProviderCapabilities as TtsCapabilities, PushOutcome, SessionIdentity,
    SessionState, StreamingTtsProvider, StreamingTtsSession, TtsError, TtsEvent, TtsSessionRequest,
};
use npc_runtime_core::{CharacterIdentity, IdentityResolver, RuntimeDependencyError, TurnRequest};
use tokio_util::sync::CancellationToken;

use crate::turn_contract::{
    RuntimeClockDomainV1, RuntimeClockStampV1, RuntimeProviderRouteBindingV1,
    RuntimeTurnCancellationReceiptV1, RuntimeTurnTimingReceiptV1,
};

#[derive(Clone)]
pub(crate) struct RuntimeTurnTimingLedger {
    inner: Arc<Mutex<TimingState>>,
    clock: RuntimeClock,
    binding: TimingBinding,
}

#[derive(Clone)]
struct TimingBinding {
    session_id: String,
    turn_id: String,
    source_loadout_id: String,
    route_snapshot_generation: u64,
    route_snapshot_sha256: String,
    llm: RuntimeProviderRouteBindingV1,
    tts: RuntimeProviderRouteBindingV1,
}

#[derive(Default)]
struct TimingState {
    input_finalized: Option<RuntimeClockStampV1>,
    identity_started: Option<RuntimeClockStampV1>,
    identity_completed: Option<RuntimeClockStampV1>,
    llm_requested: Option<RuntimeClockStampV1>,
    llm_first_token: Option<RuntimeClockStampV1>,
    llm_provider_terminal: Option<RuntimeClockStampV1>,
    structured_response_validated: Option<RuntimeClockStampV1>,
    output_tokens: Option<u32>,
    tts_requested: Option<RuntimeClockStampV1>,
    tts_first_decoded_pcm: Option<RuntimeClockStampV1>,
    tts_final_decoded_pcm: Option<RuntimeClockStampV1>,
    cancel_requested: Option<RuntimeClockStampV1>,
    cancel_terminal: Option<RuntimeClockStampV1>,
}

#[derive(Clone)]
struct RuntimeClock {
    domain: RuntimeClockDomainV1,
    frequency_hz: u64,
}

impl RuntimeTurnTimingLedger {
    pub(crate) fn new(
        session_id: String,
        turn_id: String,
        source_loadout_id: String,
        route_snapshot_generation: u64,
        route_snapshot_sha256: String,
        llm: RuntimeProviderRouteBindingV1,
        tts: RuntimeProviderRouteBindingV1,
    ) -> Option<Self> {
        Some(Self {
            inner: Arc::new(Mutex::new(TimingState::default())),
            clock: RuntimeClock::current()?,
            binding: TimingBinding {
                session_id,
                turn_id,
                source_loadout_id,
                route_snapshot_generation,
                route_snapshot_sha256,
                llm,
                tts,
            },
        })
    }

    pub(crate) fn mark_input_finalized(&self) {
        self.set_once(|state| &mut state.input_finalized);
    }

    fn mark_identity_started(&self) {
        self.set_once(|state| &mut state.identity_started);
    }

    fn mark_identity_completed(&self) {
        self.set_once(|state| &mut state.identity_completed);
    }

    pub(crate) fn mark_llm_requested(&self) {
        self.set_once(|state| &mut state.llm_requested);
    }

    pub(crate) fn observe_llm_event(&self, event: &LlmEvent) {
        match event {
            LlmEvent::TextDelta { text, .. } if !text.is_empty() => {
                self.set_once(|state| &mut state.llm_first_token);
            }
            LlmEvent::Usage { usage } => {
                if let Ok(tokens) = u32::try_from(usage.output_tokens) {
                    if tokens > 0 {
                        if let Ok(mut state) = self.inner.lock() {
                            state.output_tokens = Some(tokens);
                        }
                    }
                }
            }
            LlmEvent::Finished { .. } => {
                self.set_once(|state| &mut state.llm_provider_terminal);
            }
            _ => {}
        }
    }

    pub(crate) fn mark_structured_response_validated(&self) {
        self.set_once(|state| &mut state.structured_response_validated);
    }

    fn mark_tts_requested(&self) {
        self.set_once(|state| &mut state.tts_requested);
    }

    fn observe_tts_event(&self, event: &TtsEvent) {
        if matches!(event, TtsEvent::Audio(chunk) if !chunk.data.is_empty()) {
            let Some(stamp) = self.clock.stamp() else {
                return;
            };
            if let Ok(mut state) = self.inner.lock() {
                state
                    .tts_first_decoded_pcm
                    .get_or_insert_with(|| stamp.clone());
                state.tts_final_decoded_pcm = Some(stamp);
            }
        }
    }

    pub(crate) fn mark_cancel_requested(&self) {
        self.set_once(|state| &mut state.cancel_requested);
    }

    pub(crate) fn mark_cancel_terminal(&self) {
        self.set_once(|state| &mut state.cancel_terminal);
    }

    pub(crate) fn mark_cancel_terminal_if_requested(&self) {
        let requested = self
            .inner
            .lock()
            .is_ok_and(|state| state.cancel_requested.is_some());
        if requested {
            self.mark_cancel_terminal();
        }
    }

    pub(crate) fn finalize(
        &self,
        live_provider_receipts: bool,
    ) -> Option<RuntimeTurnTimingReceiptV1> {
        let state = self.inner.lock().ok()?;
        let cancellation = match (&state.cancel_requested, &state.cancel_terminal) {
            (None, None) => None,
            (Some(requested), Some(terminal)) => Some(RuntimeTurnCancellationReceiptV1 {
                requested: requested.clone(),
                terminal: terminal.clone(),
            }),
            _ => return None,
        };
        let receipt = RuntimeTurnTimingReceiptV1 {
            schema_version: 1,
            receipt_id: uuid::Uuid::new_v4().to_string(),
            session_id: self.binding.session_id.clone(),
            turn_id: self.binding.turn_id.clone(),
            source_loadout_id: self.binding.source_loadout_id.clone(),
            route_snapshot_generation: self.binding.route_snapshot_generation,
            route_snapshot_sha256: self.binding.route_snapshot_sha256.clone(),
            llm: self.binding.llm.clone(),
            tts: self.binding.tts.clone(),
            input_finalized: state.input_finalized.clone()?,
            identity_started: state.identity_started.clone()?,
            identity_completed: state.identity_completed.clone()?,
            llm_requested: state.llm_requested.clone()?,
            llm_first_token: state.llm_first_token.clone()?,
            llm_provider_terminal: state.llm_provider_terminal.clone()?,
            structured_response_validated: state.structured_response_validated.clone()?,
            output_tokens: state.output_tokens?,
            tts_requested: state.tts_requested.clone()?,
            tts_first_decoded_pcm: state.tts_first_decoded_pcm.clone()?,
            tts_final_decoded_pcm: state.tts_final_decoded_pcm.clone()?,
            cancellation,
            live_provider_receipts,
        };
        receipt.validate().ok()?;
        Some(receipt)
    }

    fn set_once(&self, field: impl FnOnce(&mut TimingState) -> &mut Option<RuntimeClockStampV1>) {
        let Some(stamp) = self.clock.stamp() else {
            return;
        };
        if let Ok(mut state) = self.inner.lock() {
            field(&mut state).get_or_insert(stamp);
        }
    }
}

impl RuntimeClock {
    fn current() -> Option<Self> {
        let (_, frequency_hz, domain) = clock_now()?;
        Some(Self {
            domain,
            frequency_hz,
        })
    }

    fn stamp(&self) -> Option<RuntimeClockStampV1> {
        let (ticks, frequency_hz, domain) = clock_now()?;
        if frequency_hz != self.frequency_hz || domain != self.domain {
            return None;
        }
        Some(RuntimeClockStampV1 {
            clock_domain: domain,
            qpc_frequency_hz: frequency_hz,
            qpc_ticks: ticks,
        })
    }
}

#[cfg(windows)]
fn clock_now() -> Option<(u64, u64, RuntimeClockDomainV1)> {
    let mut ticks = 0_i64;
    let mut frequency = 0_i64;
    // SAFETY: both APIs write only to the provided i64 values.
    let valid = unsafe {
        windows_sys::Win32::System::Performance::QueryPerformanceCounter(&mut ticks) != 0
            && windows_sys::Win32::System::Performance::QueryPerformanceFrequency(&mut frequency)
                != 0
    };
    (valid && ticks > 0 && frequency > 0).then_some((
        ticks as u64,
        frequency as u64,
        RuntimeClockDomainV1::WindowsQpc,
    ))
}

#[cfg(not(windows))]
fn clock_now() -> Option<(u64, u64, RuntimeClockDomainV1)> {
    use std::sync::OnceLock;
    static START: OnceLock<Instant> = OnceLock::new();
    let ticks = START
        .get_or_init(Instant::now)
        .elapsed()
        .as_nanos()
        .try_into()
        .ok()?;
    Some((
        ticks.max(1),
        1_000_000_000,
        RuntimeClockDomainV1::PortableMonotonic,
    ))
}

pub(crate) struct ObservedHostedLanguageModel {
    inner: Arc<dyn HostedLanguageModel>,
    timing: RuntimeTurnTimingLedger,
}

impl ObservedHostedLanguageModel {
    pub(crate) fn new(
        inner: Arc<dyn HostedLanguageModel>,
        timing: RuntimeTurnTimingLedger,
    ) -> Self {
        Self { inner, timing }
    }
}

#[async_trait]
impl HostedLanguageModel for ObservedHostedLanguageModel {
    fn capabilities(&self) -> &ProviderCapabilities {
        self.inner.capabilities()
    }

    async fn stream(
        &self,
        request: LlmRequest,
        cancellation: CancellationToken,
    ) -> Result<LlmEventStream, HostedLlmError> {
        self.timing.mark_llm_requested();
        let mut stream = self.inner.stream(request, cancellation).await?;
        let timing = self.timing.clone();
        Ok(Box::pin(async_stream::stream! {
            while let Some(event) = stream.next().await {
                timing.observe_llm_event(&event);
                yield event;
            }
        }))
    }

    async fn list_models(
        &self,
        cancellation: CancellationToken,
    ) -> Result<Vec<ModelInfo>, HostedLlmError> {
        self.inner.list_models(cancellation).await
    }
}

pub(crate) struct ObservedStreamingTtsProvider {
    inner: Arc<dyn StreamingTtsProvider>,
    timing: RuntimeTurnTimingLedger,
}

impl ObservedStreamingTtsProvider {
    pub(crate) fn new(
        inner: Arc<dyn StreamingTtsProvider>,
        timing: RuntimeTurnTimingLedger,
    ) -> Self {
        Self { inner, timing }
    }
}

#[async_trait]
impl StreamingTtsProvider for ObservedStreamingTtsProvider {
    fn id(&self) -> HostedTtsProviderId {
        self.inner.id()
    }

    fn capabilities(&self) -> TtsCapabilities {
        self.inner.capabilities()
    }

    async fn start_session(
        &self,
        request: TtsSessionRequest,
    ) -> Result<Box<dyn StreamingTtsSession>, TtsError> {
        // Session creation includes DNS/TLS/WebSocket setup for transports that
        // do not have a reusable connection yet. Stamp before construction so
        // first-audio latency cannot silently exclude connection overhead.
        self.timing.mark_tts_requested();
        let session = self.inner.start_session(request).await?;
        Ok(Box::new(ObservedStreamingTtsSession {
            inner: session,
            timing: self.timing.clone(),
        }))
    }
}

struct ObservedStreamingTtsSession {
    inner: Box<dyn StreamingTtsSession>,
    timing: RuntimeTurnTimingLedger,
}

#[async_trait]
impl StreamingTtsSession for ObservedStreamingTtsSession {
    fn provider_id(&self) -> HostedTtsProviderId {
        self.inner.provider_id()
    }

    fn identity(&self) -> &SessionIdentity {
        self.inner.identity()
    }

    fn state(&self) -> SessionState {
        self.inner.state()
    }

    fn has_started_utterance(&self) -> bool {
        self.inner.has_started_utterance()
    }

    async fn push_text(&mut self, text: &str) -> Result<PushOutcome, TtsError> {
        self.inner.push_text(text).await
    }

    async fn finish(&mut self) -> Result<(), TtsError> {
        self.inner.finish().await
    }

    async fn next_event(&mut self) -> Option<Result<TtsEvent, TtsError>> {
        let event = self.inner.next_event().await;
        if let Some(Ok(event)) = &event {
            self.timing.observe_tts_event(event);
        }
        event
    }

    async fn cancel(&mut self) -> Result<(), TtsError> {
        self.inner.cancel().await
    }
}

pub(crate) struct ObservedIdentityResolver {
    inner: Arc<dyn IdentityResolver>,
    timing: RuntimeTurnTimingLedger,
}

impl ObservedIdentityResolver {
    pub(crate) fn new(inner: Arc<dyn IdentityResolver>, timing: RuntimeTurnTimingLedger) -> Self {
        Self { inner, timing }
    }
}

#[async_trait]
impl IdentityResolver for ObservedIdentityResolver {
    async fn resolve(
        &self,
        request: &TurnRequest,
        cancellation: CancellationToken,
    ) -> Result<CharacterIdentity, RuntimeDependencyError> {
        self.timing.mark_identity_started();
        let result = self.inner.resolve(request, cancellation).await;
        if result.is_ok() {
            self.timing.mark_identity_completed();
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::turn_contract::{
        RuntimeTurnLatencyStatusV1, RESPONSE_LATENCY_CEILING_MILLIS, RESPONSE_LATENCY_TARGET_MILLIS,
    };
    use npc_providers_llm::{FinishReason, TokenUsage};
    use npc_providers_tts::{AudioFormat, PcmChunk};

    fn stamp(ticks: u64) -> RuntimeClockStampV1 {
        RuntimeClockStampV1 {
            clock_domain: RuntimeClockDomainV1::WindowsQpc,
            qpc_frequency_hz: 10_000_000,
            qpc_ticks: ticks,
        }
    }

    fn complete_receipt() -> RuntimeTurnTimingReceiptV1 {
        RuntimeTurnTimingReceiptV1 {
            schema_version: 1,
            receipt_id: "10000000-0000-4000-8000-000000000001".into(),
            session_id: "timing-session".into(),
            turn_id: "timing-turn".into(),
            source_loadout_id: "timing-loadout".into(),
            route_snapshot_generation: 7,
            route_snapshot_sha256: "a".repeat(64),
            llm: RuntimeProviderRouteBindingV1 {
                provider_id: "openai".into(),
                model_id: "gpt-5-mini".into(),
                voice_id: None,
                egress: "conversation_text".into(),
            },
            tts: RuntimeProviderRouteBindingV1 {
                provider_id: "elevenlabs".into(),
                model_id: "eleven_flash_v2_5".into(),
                voice_id: Some("stock-voice".into()),
                egress: "conversation_audio".into(),
            },
            input_finalized: stamp(1),
            identity_started: stamp(2),
            identity_completed: stamp(3),
            llm_requested: stamp(4),
            llm_first_token: stamp(5),
            llm_provider_terminal: stamp(6),
            structured_response_validated: stamp(7),
            output_tokens: 8,
            tts_requested: stamp(8),
            tts_first_decoded_pcm: stamp(9),
            tts_final_decoded_pcm: stamp(10),
            cancellation: None,
            live_provider_receipts: true,
        }
    }

    #[test]
    fn strict_receipt_rejects_missing_unordered_and_mixed_clock_marks() {
        let mut receipt = complete_receipt();
        assert!(receipt.validate().is_ok());

        receipt.output_tokens = 0;
        assert!(receipt.validate().is_err());
        receipt = complete_receipt();
        receipt.llm_first_token.qpc_ticks = receipt.llm_requested.qpc_ticks - 1;
        assert!(receipt.validate().is_err());
        receipt = complete_receipt();
        receipt.tts_first_decoded_pcm.clock_domain = RuntimeClockDomainV1::PortableMonotonic;
        assert!(receipt.validate().is_err());
        receipt = complete_receipt();
        receipt.tts_final_decoded_pcm.qpc_frequency_hz = 1_000_000_000;
        assert!(receipt.validate().is_err());
    }

    #[test]
    fn fixtures_and_incomplete_cancellation_cannot_become_product_receipts() {
        let mut receipt = complete_receipt();
        receipt.live_provider_receipts = false;
        assert!(receipt.validate().is_err());

        receipt = complete_receipt();
        receipt.cancellation = Some(RuntimeTurnCancellationReceiptV1 {
            requested: stamp(8),
            terminal: stamp(7),
        });
        assert!(receipt.validate().is_err());
    }

    fn ledger() -> RuntimeTurnTimingLedger {
        RuntimeTurnTimingLedger::new(
            "timing-session".into(),
            "timing-turn".into(),
            "timing-loadout".into(),
            7,
            "a".repeat(64),
            RuntimeProviderRouteBindingV1 {
                provider_id: "openai".into(),
                model_id: "gpt-5-mini".into(),
                voice_id: None,
                egress: "provider_cloud:transcript.game_context".into(),
            },
            RuntimeProviderRouteBindingV1 {
                provider_id: "elevenlabs".into(),
                model_id: "eleven_flash_v2_5".into(),
                voice_id: Some("stock-voice".into()),
                egress: "provider_cloud:response_text".into(),
            },
        )
        .expect("monotonic clock is available")
    }

    #[test]
    fn streaming_tts_can_start_before_the_llm_terminal_without_losing_producer_order() {
        let ledger = ledger();
        ledger.mark_input_finalized();
        ledger.mark_identity_started();
        ledger.mark_identity_completed();
        ledger.mark_llm_requested();
        ledger.observe_llm_event(&LlmEvent::TextDelta {
            text: "First complete sentence.".into(),
            content_index: 0,
        });
        ledger.mark_tts_requested();
        ledger.observe_tts_event(&TtsEvent::Audio(PcmChunk {
            sequence: 0,
            format: AudioFormat::default(),
            data: vec![0, 0, 1, 0],
        }));
        ledger.observe_llm_event(&LlmEvent::Usage {
            usage: TokenUsage {
                input_tokens: 10,
                output_tokens: 4,
                cached_input_tokens: 0,
                total_tokens: 14,
            },
        });
        ledger.observe_llm_event(&LlmEvent::Finished {
            reason: FinishReason::EndTurn,
            provider_reason: "stop".into(),
        });
        ledger.mark_structured_response_validated();

        let receipt = ledger.finalize(true).expect("complete live receipt");
        assert!(receipt.validate().is_ok());
        assert!(receipt.tts_requested.qpc_ticks <= receipt.llm_provider_terminal.qpc_ticks);
        assert_eq!(receipt.output_tokens, 4);
        let wire = serde_json::to_value(&receipt).expect("sidecar serialization");
        assert_eq!(wire["routeSnapshotGeneration"], 7);
        assert_eq!(wire["tts"]["voiceId"], "stock-voice");
        assert_eq!(
            wire["llm"]["egress"],
            "provider_cloud:transcript.game_context"
        );
    }

    #[test]
    fn cancellation_marks_are_atomic_and_terminal_ordered() {
        let ledger = ledger();
        ledger.mark_cancel_terminal_if_requested();
        assert!(ledger
            .inner
            .lock()
            .expect("timing mutex")
            .cancel_terminal
            .is_none());
        ledger.mark_cancel_requested();
        ledger.mark_cancel_terminal_if_requested();
        let state = ledger.inner.lock().expect("timing mutex");
        let requested = state.cancel_requested.as_ref().expect("request stamp");
        let terminal = state.cancel_terminal.as_ref().expect("terminal stamp");
        assert_eq!(requested.clock_domain, terminal.clock_domain);
        assert_eq!(requested.qpc_frequency_hz, terminal.qpc_frequency_hz);
        assert!(terminal.qpc_ticks >= requested.qpc_ticks);
    }

    #[test]
    fn response_latency_budget_uses_exact_clock_deltas_and_boundary_bands() {
        let mut receipt = complete_receipt();
        let frequency = receipt.input_finalized.qpc_frequency_hz;
        receipt.input_finalized.qpc_ticks = frequency;
        receipt.identity_started.qpc_ticks = frequency;
        receipt.identity_completed.qpc_ticks = frequency + 1_000_000;
        receipt.llm_requested.qpc_ticks = frequency + 1_000_000;
        receipt.llm_first_token.qpc_ticks = frequency + 11_000_000;
        receipt.tts_requested.qpc_ticks = frequency + 41_000_000;
        receipt.tts_first_decoded_pcm.qpc_ticks = frequency + 50_000_000;
        receipt.llm_provider_terminal.qpc_ticks = frequency + 42_000_000;
        receipt.structured_response_validated.qpc_ticks = frequency + 43_000_000;
        receipt.tts_final_decoded_pcm.qpc_ticks = frequency + 51_000_000;

        let target = receipt.latency_assessment().expect("target assessment");
        assert_eq!(target.response_to_first_audio_millis, 5_000);
        assert_eq!(target.llm_time_to_first_token_millis, 1_000);
        assert_eq!(target.first_token_to_tts_request_millis, 3_000);
        assert_eq!(target.tts_time_to_first_audio_millis, 900);
        assert_eq!(target.status, RuntimeTurnLatencyStatusV1::MeetsTarget);
        let mut impossible_breakdown = target.clone();
        impossible_breakdown.tts_time_to_first_audio_millis = 2_000;
        assert_eq!(
            impossible_breakdown.validate(),
            Err("invalid_runtime_turn_latency_assessment")
        );

        receipt.tts_first_decoded_pcm.qpc_ticks = frequency + 80_000_000;
        receipt.tts_final_decoded_pcm.qpc_ticks = frequency + 81_000_000;
        let ceiling = receipt.latency_assessment().expect("ceiling assessment");
        assert_eq!(ceiling.response_to_first_audio_millis, 8_000);
        assert_eq!(ceiling.status, RuntimeTurnLatencyStatusV1::WithinCeiling);

        receipt.tts_first_decoded_pcm.qpc_ticks = frequency + 80_000_001;
        receipt.tts_final_decoded_pcm.qpc_ticks = frequency + 81_000_000;
        let exceeded = receipt.latency_assessment().expect("exceeded assessment");
        assert_eq!(exceeded.response_to_first_audio_millis, 8_001);
        assert_eq!(exceeded.status, RuntimeTurnLatencyStatusV1::ExceedsCeiling);
    }

    #[test]
    fn latency_assessment_is_redacted_and_rejects_tampered_budget_claims() {
        let receipt = complete_receipt();
        let mut assessment = receipt.latency_assessment().expect("assessment");
        let wire = serde_json::to_value(&assessment).expect("serialize assessment");
        assert_eq!(wire["scope"], "finalizedTranscriptToFirstDecodedPcm");
        assert_eq!(wire["targetMillis"], RESPONSE_LATENCY_TARGET_MILLIS);
        assert_eq!(wire["ceilingMillis"], RESPONSE_LATENCY_CEILING_MILLIS);
        assert!(wire.get("providerId").is_none());
        assert!(wire.get("transcript").is_none());

        assessment.status = RuntimeTurnLatencyStatusV1::ExceedsCeiling;
        assert_eq!(
            assessment.validate(),
            Err("invalid_runtime_turn_latency_assessment")
        );
    }
}

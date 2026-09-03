//! Explicit, ignored ElevenLabs stock-voice qualification.
//!
//! The key file path and evidence paths arrive through environment variables.
//! Credential bytes are zeroized and never enter arguments, output, evidence,
//! provider URLs, or test failure text.

use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use npc_providers_tts::{
    AudioFormat, CredentialResolveError, ElevenLabsConfig, ElevenLabsProvider,
    ElevenLabsWebSocketTransport, HostedTtsProviderId, ProviderCredentialResolver,
    SemanticClausePolicy, SensitiveString, SessionIdentity, StreamingTtsProvider, TtsEvent,
    TtsSessionRequest, VoiceBinding, VoiceBindings,
};
use serde_json::json;
use zeroize::Zeroizing;

const KEYS_PATH_ENV: &str = "HOSTED_TEST_KEYS_FILE";
const EVIDENCE_PATH_ENV: &str = "ELEVENLABS_LIVE_METRICS_PATH";
const AUDIO_PATH_ENV: &str = "ELEVENLABS_LIVE_AUDIO_PATH";
const PROVIDER_ID: HostedTtsProviderId = HostedTtsProviderId::ElevenLabs;
const VOICE_INTENT: &str = "qualification.stock.aria";
const STOCK_VOICE_ID: &str = "EXAVITQu4vr4xnSDxMaL";
const MODEL_ID: &str = "eleven_flash_v2_5";
const FIXTURE_TEXT: &str = "The north beacon is ready.";

#[derive(Clone)]
struct FileCredential {
    path: PathBuf,
}

#[async_trait]
impl ProviderCredentialResolver for FileCredential {
    async fn resolve(
        &self,
        provider_id: HostedTtsProviderId,
    ) -> Result<SensitiveString, CredentialResolveError> {
        if provider_id != PROVIDER_ID {
            return Err(CredentialResolveError::Missing);
        }
        let bytes = fs::read(&self.path).map_err(|_| CredentialResolveError::Unavailable)?;
        let bytes = Zeroizing::new(bytes);
        let text = std::str::from_utf8(bytes.as_slice())
            .map_err(|_| CredentialResolveError::Unavailable)?;
        for raw in text.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with(['#', ';']) || line.starts_with("//") {
                continue;
            }
            let delimiter = match (line.find('='), line.find(':')) {
                (Some(left), Some(right)) => Some(left.min(right)),
                (Some(position), None) | (None, Some(position)) => Some(position),
                (None, None) => None,
            };
            let Some(delimiter) = delimiter else {
                continue;
            };
            let label = line[..delimiter]
                .trim()
                .trim_matches(['"', '\'', '`'])
                .to_ascii_lowercase()
                .replace([' ', '-'], "_");
            if label != "elevenlabs" {
                continue;
            }
            let value = line[delimiter + 1..].trim().trim_matches(['"', '\'']);
            if value.len() < 8 || value.as_bytes().iter().any(u8::is_ascii_control) {
                return Err(CredentialResolveError::Unavailable);
            }
            return Ok(SensitiveString::new(value.to_owned()));
        }
        Err(CredentialResolveError::Missing)
    }
}

#[derive(Clone, Copy)]
struct MissingCredential;

#[async_trait]
impl ProviderCredentialResolver for MissingCredential {
    async fn resolve(
        &self,
        _provider_id: HostedTtsProviderId,
    ) -> Result<SensitiveString, CredentialResolveError> {
        Err(CredentialResolveError::Missing)
    }
}

fn bindings() -> VoiceBindings {
    VoiceBindings::new([VoiceBinding {
        intent_id: VOICE_INTENT.into(),
        provider_id: PROVIDER_ID,
        voice_id: STOCK_VOICE_ID.into(),
        model_id: MODEL_ID.into(),
        provider_options: BTreeMap::from([
            ("stability".into(), "0.5".into()),
            ("similarity_boost".into(), "0.75".into()),
            ("speed".into(), "1.0".into()),
        ]),
    }])
    .expect("fixed stock voice binding is valid")
}

fn session_request(turn: &str) -> TtsSessionRequest {
    TtsSessionRequest {
        identity: SessionIdentity {
            session_id: "hosted-provider-qualification".into(),
            turn_id: turn.into(),
            cancellation_generation: 1,
        },
        locale: "en-US".into(),
        voice_intent_id: VOICE_INTENT.into(),
        output: AudioFormat::default(),
        request_alignment: true,
        request_visemes: false,
        clause_policy: SemanticClausePolicy::default(),
    }
}

fn write_wav(path: &Path, pcm: &[u8], format: AudioFormat) -> Result<(), &'static str> {
    if pcm.is_empty() || !pcm.len().is_multiple_of(2) || format.channels != 1 {
        return Err("PCM fixture shape is invalid");
    }
    let data_bytes = u32::try_from(pcm.len()).map_err(|_| "PCM fixture is oversized")?;
    let byte_rate = format
        .sample_rate_hz
        .checked_mul(u32::from(format.channels))
        .and_then(|value| value.checked_mul(2))
        .ok_or("WAV byte rate overflow")?;
    let riff_size = data_bytes.checked_add(36).ok_or("WAV length overflow")?;
    let mut wav = Vec::with_capacity(pcm.len() + 44);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&riff_size.to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16_u32.to_le_bytes());
    wav.extend_from_slice(&1_u16.to_le_bytes());
    wav.extend_from_slice(&format.channels.to_le_bytes());
    wav.extend_from_slice(&format.sample_rate_hz.to_le_bytes());
    wav.extend_from_slice(&byte_rate.to_le_bytes());
    wav.extend_from_slice(&(format.channels * 2).to_le_bytes());
    wav.extend_from_slice(&16_u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_bytes.to_le_bytes());
    wav.extend_from_slice(pcm);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|_| "audio directory unavailable")?;
    }
    fs::write(path, wav).map_err(|_| "audio fixture could not be written")
}

fn audio_metrics(pcm: &[u8], format: AudioFormat) -> Result<serde_json::Value, &'static str> {
    if pcm.is_empty() || !pcm.len().is_multiple_of(2) {
        return Err("audio is empty or misaligned");
    }
    let sample_values = pcm
        .chunks_exact(2)
        .map(|pair| i32::from(i16::from_le_bytes([pair[0], pair[1]])))
        .collect::<Vec<_>>();
    let mut square_sum = 0_f64;
    let mut peak = 0_i32;
    let mut clipped = 0_u64;
    for value in &sample_values {
        let value = *value;
        let magnitude = value.unsigned_abs() as i32;
        peak = peak.max(magnitude);
        clipped = clipped.saturating_add(u64::from(magnitude >= 32_767));
        square_sum += f64::from(value * value);
    }
    let samples = sample_values.len() as u64;
    let rms = (square_sum / samples.max(1) as f64).sqrt() / 32_768.0;
    if rms < 0.0001 || peak == 0 {
        return Err("provider audio is silent");
    }
    if clipped > 0 {
        return Err("provider audio is clipped");
    }
    let max_adjacent_delta = sample_values
        .windows(2)
        .map(|pair| (pair[1] - pair[0]).unsigned_abs())
        .max()
        .unwrap_or(0) as f64
        / 32_768.0;
    let onset_samples = (format.sample_rate_hz / 1_000).max(1) as usize;
    let onset_peak = sample_values
        .iter()
        .take(onset_samples)
        .map(|value| value.unsigned_abs())
        .max()
        .unwrap_or(0) as f64;
    let onset_ratio_1ms = onset_peak / f64::from(peak.max(1));
    let tail_samples = (format.sample_rate_hz / 50).max(1) as usize;
    let tail = sample_values
        .iter()
        .rev()
        .take(tail_samples)
        .copied()
        .collect::<Vec<_>>();
    let tail_rms_20ms = (tail
        .iter()
        .map(|value| f64::from(*value * *value))
        .sum::<f64>()
        / tail.len().max(1) as f64)
        .sqrt()
        / 32_768.0;
    Ok(json!({
        "encoding": "pcm-s16le",
        "sampleRateHz": format.sample_rate_hz,
        "channels": format.channels,
        "audioBytes": pcm.len(),
        "durationMs": samples.saturating_mul(1000) / u64::from(format.sample_rate_hz),
        "rms": rms,
        "peak": f64::from(peak) / 32768.0,
        "clippedSamples": clipped,
        "maxAdjacentDelta": max_adjacent_delta,
        "onsetRatio1ms": onset_ratio_1ms,
        "tailRms20ms": tail_rms_20ms,
        "firstSample": f64::from(sample_values.first().copied().unwrap_or(0)) / 32768.0,
        "lastSample": f64::from(sample_values.last().copied().unwrap_or(0)) / 32768.0,
        "nonSilent": true
    }))
}

#[tokio::test]
#[ignore = "explicit low-cost ElevenLabs stock-voice qualification; requires HOSTED_TEST_KEYS_FILE, ELEVENLABS_LIVE_METRICS_PATH and ELEVENLABS_LIVE_AUDIO_PATH"]
async fn live_elevenlabs_stock_voice_streams_pcm_and_cancels() {
    let keys_path = PathBuf::from(env::var_os(KEYS_PATH_ENV).expect("keys path is required"));
    let evidence_path =
        PathBuf::from(env::var_os(EVIDENCE_PATH_ENV).expect("evidence path is required"));
    let audio_path = PathBuf::from(env::var_os(AUDIO_PATH_ENV).expect("audio path is required"));
    assert!(keys_path.is_file());

    let resolver: Arc<dyn ProviderCredentialResolver> =
        Arc::new(FileCredential { path: keys_path });
    let provider = ElevenLabsProvider::new(
        Arc::new(ElevenLabsWebSocketTransport::default()),
        resolver,
        bindings(),
        ElevenLabsConfig::default(),
    );
    let started = Instant::now();
    let mut session = provider
        .start_session(session_request("elevenlabs-complete"))
        .await
        .expect("authenticated stock route must connect");
    let connected_ms = started.elapsed().as_millis() as u64;
    let push = session
        .push_text(FIXTURE_TEXT)
        .await
        .expect("synthetic fixture text must be accepted");
    session.finish().await.expect("stock route must finish");

    let mut first_audio_ms = None;
    let mut audio_chunks = 0_u64;
    let mut alignment_events = 0_u64;
    let mut alignment_words = 0_u64;
    let mut pcm = Vec::new();
    let mut completed = false;
    while let Some(event) = session.next_event().await {
        match event.expect("provider event must normalize") {
            TtsEvent::Audio(chunk) => {
                first_audio_ms.get_or_insert(started.elapsed().as_millis() as u64);
                assert_eq!(chunk.format, AudioFormat::default());
                audio_chunks = audio_chunks.saturating_add(1);
                if pcm.len().saturating_add(chunk.data.len()) > 8 * 1_048_576 {
                    panic!("provider audio exceeded the qualification bound");
                }
                pcm.extend_from_slice(&chunk.data);
            }
            TtsEvent::Alignment(words) => {
                alignment_events = alignment_events.saturating_add(1);
                alignment_words = alignment_words.saturating_add(words.len() as u64);
            }
            TtsEvent::Completed => {
                completed = true;
                break;
            }
            TtsEvent::Viseme(_) | TtsEvent::Usage(_) | TtsEvent::Interrupted { .. } => {}
        }
    }
    assert!(completed);
    let total_ms = started.elapsed().as_millis() as u64;
    let audio = audio_metrics(&pcm, AudioFormat::default()).expect("audio must be measurable");
    write_wav(&audio_path, &pcm, AudioFormat::default()).expect("write project-owned fixture");

    let cancel_connect_started = Instant::now();
    let mut cancellation = provider
        .start_session(session_request("elevenlabs-cancel"))
        .await
        .expect("cancellation route must connect");
    let cancel_connect_ms = cancel_connect_started.elapsed().as_millis() as u64;
    let cancel_close_started = Instant::now();
    cancellation
        .cancel()
        .await
        .expect("socket-close cancellation must succeed");
    let cancel_close_ms = cancel_close_started.elapsed().as_millis() as u64;
    assert!(matches!(
        cancellation.next_event().await,
        Some(Ok(TtsEvent::Interrupted { reason: "barge_in" }))
    ));

    let missing_provider = ElevenLabsProvider::new(
        Arc::new(ElevenLabsWebSocketTransport::default()),
        Arc::new(MissingCredential),
        bindings(),
        ElevenLabsConfig::default(),
    );
    let error = missing_provider
        .start_session(session_request("elevenlabs-missing-secret"))
        .await
        .err()
        .expect("missing credential must fail before network egress");
    assert_eq!(error.code, "credential_missing");
    assert!(!format!("{error:?} {error}").contains("xi-api-key"));

    let report = json!({
        "schemaVersion": 1,
        "checkedAtUnixMs": SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
        "scope": "bounded-live-production-rust-websocket-adapter-not-app-end-to-end",
        "containsCredentialValues": false,
        "containsProviderResponseText": false,
        "provider": "elevenlabs",
        "adapter": "npc-providers-tts::ElevenLabsProvider+ElevenLabsWebSocketTransport",
        "model": MODEL_ID,
        "stockVoice": STOCK_VOICE_ID,
        "voiceCloningUsed": false,
        "stream": {
            "ok": completed,
            "acceptedCharacters": push.accepted_chars,
            "connectMs": connected_ms,
            "firstAudioMs": first_audio_ms,
            "totalMs": total_ms,
            "audioChunks": audio_chunks,
            "alignmentEvents": alignment_events,
            "alignmentWords": alignment_words
        },
        "audio": audio,
        "cancel": {
            "ok": true,
            "transport": "dedicated_websocket_close",
            "connectMs": cancel_connect_ms,
            "closeMs": cancel_close_ms
        },
        "missingCredentialError": {
            "ok": true,
            "kind": "authentication",
            "code": "credential_missing",
            "networkDispatched": false
        }
    });
    let bytes = serde_json::to_vec_pretty(&report).expect("serialize safe evidence");
    if let Some(parent) = evidence_path.parent() {
        fs::create_dir_all(parent).expect("create evidence directory");
    }
    fs::write(&evidence_path, [bytes.as_slice(), b"\n"].concat()).expect("write safe evidence");
}

//! Explicit, ignored AssemblyAI Universal-3 Pro streaming qualification.
//!
//! Credential values, provider transcript text, and PCM bytes never enter process arguments,
//! output, or evidence. The sole input is a project-owned synthetic audio fixture.

use std::{
    env, fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use npc_providers_stt::{
    AssemblyAi, AudioFormat, FlushReason, HostedRecognizer, HostedWebSocketTransportFactory,
    RecognitionConfig, RecognitionEvent, RetentionPolicy, SecretString, TranscriptStatus,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;
use zeroize::Zeroizing;

const KEYS_PATH_ENV: &str = "HOSTED_TEST_KEYS_FILE";
const AUDIO_PATH_ENV: &str = "ASSEMBLYAI_LIVE_AUDIO_PATH";
const EVIDENCE_PATH_ENV: &str = "ASSEMBLYAI_LIVE_METRICS_PATH";
const MODEL_ID: &str = "u3-rt-pro";
const EXPECTED_TEXT: &str = "the north beacon is ready";

struct KeyMaterial(Zeroizing<Vec<u8>>);

impl KeyMaterial {
    fn read(path: &Path, label: &str) -> Result<Self, &'static str> {
        let bytes = Zeroizing::new(fs::read(path).map_err(|_| "credential file unavailable")?);
        let text =
            std::str::from_utf8(bytes.as_slice()).map_err(|_| "credential file is not UTF-8")?;
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
            let Some(delimiter) = delimiter else { continue };
            let normalized = line[..delimiter]
                .trim()
                .trim_matches(['"', '\'', '`'])
                .to_ascii_lowercase()
                .replace([' ', '-'], "_");
            if normalized != label {
                continue;
            }
            let value = line[delimiter + 1..]
                .trim()
                .trim_matches(['"', '\''])
                .as_bytes()
                .to_vec();
            if value.len() < 8 || value.iter().any(u8::is_ascii_control) {
                return Err("credential value is invalid");
            }
            return Ok(Self(Zeroizing::new(value)));
        }
        Err("credential label unavailable")
    }

    fn to_secret(&self) -> SecretString {
        SecretString::new(
            String::from_utf8(self.0.as_slice().to_vec()).expect("validated UTF-8 credential"),
        )
    }
}

fn resample_pcm_s16le_to_16k(input: &[i16], sample_rate: u32) -> Result<Vec<u8>, &'static str> {
    if input.is_empty() || !(8_000..=48_000).contains(&sample_rate) {
        return Err("resample input is invalid");
    }
    if sample_rate == 16_000 {
        return Ok(input
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect());
    }
    let output_samples = input
        .len()
        .saturating_mul(16_000)
        .checked_div(sample_rate as usize)
        .ok_or("resample rate invalid")?;
    if output_samples == 0 {
        return Err("resample output is empty");
    }
    let mut output = Vec::with_capacity(output_samples.saturating_mul(2));
    for index in 0..output_samples {
        let numerator = index.saturating_mul(sample_rate as usize);
        let left = numerator / 16_000;
        let fraction = numerator % 16_000;
        let a = i64::from(*input.get(left).ok_or("resample index invalid")?);
        let b = i64::from(*input.get(left + 1).unwrap_or(&input[left]));
        let sample = ((a * (16_000 - fraction) as i64 + b * fraction as i64) / 16_000)
            .clamp(i64::from(i16::MIN), i64::from(i16::MAX)) as i16;
        output.extend_from_slice(&sample.to_le_bytes());
    }
    Ok(output)
}

fn wav_pcm_16k_mono(path: &Path) -> Result<Vec<u8>, &'static str> {
    let wav = fs::read(path).map_err(|_| "audio fixture unavailable")?;
    if wav.len() < 44 || &wav[0..4] != b"RIFF" || &wav[8..12] != b"WAVE" {
        return Err("audio fixture is not a WAV file");
    }
    let channels = u16::from_le_bytes([wav[22], wav[23]]);
    let sample_rate = u32::from_le_bytes([wav[24], wav[25], wav[26], wav[27]]);
    let bits = u16::from_le_bytes([wav[34], wav[35]]);
    if channels != 1 || bits != 16 || !(8_000..=48_000).contains(&sample_rate) {
        return Err("audio fixture format is unsupported");
    }
    let data_position = wav
        .windows(4)
        .position(|window| window == b"data")
        .ok_or("audio fixture has no data chunk")?;
    let size_position = data_position + 4;
    if size_position + 4 > wav.len() {
        return Err("audio fixture data header is truncated");
    }
    let data_size = u32::from_le_bytes([
        wav[size_position],
        wav[size_position + 1],
        wav[size_position + 2],
        wav[size_position + 3],
    ]) as usize;
    let data_start = size_position + 4;
    let data_end = data_start
        .checked_add(data_size)
        .ok_or("audio size overflow")?;
    if data_end > wav.len() || data_size == 0 || !data_size.is_multiple_of(2) {
        return Err("audio fixture data is invalid");
    }
    let input = wav[data_start..data_end]
        .chunks_exact(2)
        .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
        .collect::<Vec<_>>();
    resample_pcm_s16le_to_16k(&input, sample_rate)
}

#[test]
fn generalized_resampler_handles_full_scale_up_and_down_sampling() {
    let input = [i16::MAX, i16::MIN, i16::MAX, i16::MIN, i16::MAX, i16::MIN];
    let upsampled = resample_pcm_s16le_to_16k(&input, 8_000).expect("8 kHz upsampling");
    let downsampled = resample_pcm_s16le_to_16k(&input, 48_000).expect("48 kHz downsampling");
    assert_eq!(upsampled.len(), input.len() * 4);
    assert_eq!(downsampled.len(), input.len() * 2 / 3);
    let upsampled_samples = upsampled
        .chunks_exact(2)
        .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
        .collect::<Vec<_>>();
    let downsampled_samples = downsampled
        .chunks_exact(2)
        .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
        .collect::<Vec<_>>();
    assert_eq!(&upsampled_samples[..3], &[i16::MAX, 0, i16::MIN]);
    assert_eq!(downsampled_samples, vec![i16::MAX, i16::MIN]);
}

fn normalized(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric() || character.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn config() -> RecognitionConfig {
    RecognitionConfig {
        model: MODEL_ID.into(),
        audio: AudioFormat::PCM_16KHZ_MONO,
        languages: Vec::new(),
        endpointing: npc_providers_stt::EndpointingMode::Manual,
        interim_results: true,
        keyword_hints: Vec::new(),
        context_hint: None,
        retention: RetentionPolicy::ProviderDefaultAllowed,
        timeouts: npc_providers_stt::SessionTimeouts {
            connect: Duration::from_secs(20),
            send: Duration::from_secs(5),
            receive_idle: Duration::from_secs(20),
            flush: Duration::from_secs(15),
        },
    }
}

#[tokio::test]
#[ignore = "explicit low-cost AssemblyAI streaming qualification"]
async fn live_assemblyai_v3_streams_final_cancels_and_redacts() {
    let keys_path = PathBuf::from(env::var_os(KEYS_PATH_ENV).expect("keys path required"));
    let audio_path = PathBuf::from(env::var_os(AUDIO_PATH_ENV).expect("audio path required"));
    let evidence_path =
        PathBuf::from(env::var_os(EVIDENCE_PATH_ENV).expect("evidence path required"));
    let key = KeyMaterial::read(&keys_path, "assemblyai").expect("AssemblyAI key required");
    let pcm = wav_pcm_16k_mono(&audio_path).expect("project audio fixture required");
    assert!(pcm.len() <= 8 * 1_048_576);

    let recognizer = HostedRecognizer::new(
        AssemblyAi,
        Arc::new(HostedWebSocketTransportFactory::default()),
    );
    let started = Instant::now();
    let mut session = recognizer
        .start_session(config(), key.to_secret(), CancellationToken::new())
        .await
        .expect("selected AssemblyAI v3 route must connect");
    let connected_ms = started.elapsed().as_millis() as u64;
    assert!(matches!(
        session.next_event().await.expect("Begin must normalize"),
        Some(RecognitionEvent::SessionStarted { .. })
    ));

    let bytes_per_100ms = 16_000 * 2 / 10;
    let pacing_origin = tokio::time::Instant::now();
    let mut sent_chunks = 0_u64;
    for chunk in pcm.chunks(bytes_per_100ms) {
        session.send_audio(chunk).await.expect("PCM chunk accepted");
        sent_chunks = sent_chunks.saturating_add(1);
        tokio::time::sleep_until(
            pacing_origin + Duration::from_millis(sent_chunks.saturating_mul(100)),
        )
        .await;
    }
    let flush_started = Instant::now();
    session
        .flush(FlushReason::PushToTalkReleased)
        .await
        .expect("PTT release must force an endpoint");

    let mut partials = 0_u64;
    let mut finals = 0_u64;
    let mut ended = false;
    let mut final_text = Zeroizing::new(String::new());
    while let Some(event) = session.next_event().await.expect("event must normalize") {
        match event {
            RecognitionEvent::Transcript(transcript) => match transcript.status {
                TranscriptStatus::Partial | TranscriptStatus::EagerFinal => {
                    partials = partials.saturating_add(1)
                }
                TranscriptStatus::Final => {
                    finals = finals.saturating_add(1);
                    final_text.clear();
                    final_text.push_str(&transcript.text);
                }
            },
            RecognitionEvent::TurnEnded { .. } => {
                ended = true;
                break;
            }
            RecognitionEvent::Warning { .. }
            | RecognitionEvent::TurnStarted { .. }
            | RecognitionEvent::TurnResumed { .. }
            | RecognitionEvent::SessionStarted { .. }
            | RecognitionEvent::Cancelled => {}
        }
    }
    assert!(ended && finals > 0 && !final_text.is_empty());
    let final_latency_ms = flush_started.elapsed().as_millis() as u64;
    let exact_fixture_match = normalized(&final_text) == EXPECTED_TEXT;
    let transcript_sha256 = format!("{:x}", Sha256::digest(final_text.as_bytes()));
    session.close().await.expect("stream must terminate");

    let cancel_started = Instant::now();
    let mut cancellation = recognizer
        .start_session(config(), key.to_secret(), CancellationToken::new())
        .await
        .expect("cancellation socket must connect");
    let cancel_connect_ms = cancel_started.elapsed().as_millis() as u64;
    let cancel_close_started = Instant::now();
    cancellation.cancel().await.expect("socket must cancel");
    let cancel_close_ms = cancel_close_started.elapsed().as_millis() as u64;
    assert!(matches!(
        cancellation.next_event().await.expect("cancel normalizes"),
        Some(RecognitionEvent::Cancelled)
    ));
    assert!(cancellation
        .next_event()
        .await
        .expect("cancel is terminal")
        .is_none());

    let report = json!({
        "schemaVersion": 1,
        "checkedAtUnixMs": SystemTime::now().duration_since(UNIX_EPOCH)
            .unwrap_or_default().as_millis() as u64,
        "scope": "bounded-live-production-rust-websocket-adapter-project-owned-synthetic-audio",
        "containsCredentialValues": false,
        "containsProviderTranscriptText": false,
        "containsPcmBytes": false,
        "provider": "assemblyai",
        "adapter": "npc-providers-stt::HostedRecognizer<AssemblyAi>+HostedWebSocketTransportFactory",
        "route": {
            "endpointClass": "official_pinned_wss",
            "api": "streaming_v3",
            "model": MODEL_ID,
            "egress": "microphone_audio_and_optional_non_secret_context",
            "audioSource": "project_owned_synthetic_fixture",
            "format": "pcm_s16le_16000_mono",
            "realtimePaced": true
        },
        "stream": {
            "ok": true,
            "connectMs": connected_ms,
            "audioChunks": sent_chunks,
            "audioBytes": pcm.len(),
            "partialEvents": partials,
            "finalEvents": finals,
            "pttFinalMs": final_latency_ms,
            "exactFixtureMatch": exact_fixture_match,
            "transcriptBytes": final_text.len(),
            "transcriptSha256": transcript_sha256
        },
        "cancel": {
            "ok": true,
            "transport": "terminate_then_authoritative_websocket_close",
            "connectMs": cancel_connect_ms,
            "closeMs": cancel_close_ms,
            "postCancelEvents": 0
        },
        "remoteObjects": {
            "created": false,
            "remaining": false,
            "streamTerminated": true
        }
    });
    if let Some(parent) = evidence_path.parent() {
        fs::create_dir_all(parent).expect("evidence directory available");
    }
    fs::write(
        evidence_path,
        serde_json::to_vec_pretty(&report).expect("serialize redacted evidence"),
    )
    .expect("write redacted evidence");
}

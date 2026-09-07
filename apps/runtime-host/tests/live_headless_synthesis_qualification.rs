//! Explicit, ignored qualification of the exact hosted reply and stock-voice
//! adapters without opening or writing to an operating-system audio endpoint.
//!
//! This is deliberately not a playback test. The WAV and JSON receipt prove
//! provider synthesis, PCM shape, and the runtime TTS bridge only. They cannot
//! satisfy the product's native-broker playback receipt requirement.

#![cfg(windows)]

use std::{
    collections::BTreeMap,
    env,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use futures_util::StreamExt;
use interactive_npcs_credential_vault::{CredentialVault, WindowsCredentialVault};
use npc_providers_tts::{
    AudioFormat, HostedTtsProviderId, NvidiaNimMagpie, ReqwestNvidiaNimHttpTransport,
    SemanticClausePolicy, StreamingTtsProvider, TonicNvidiaNimGrpcTransport, VoiceBinding,
    VoiceBindings, NVIDIA_MAGPIE_MODEL_ID,
};
use npc_runtime_core::{
    CharacterIdentity, DataClass, GenerationRequest, MemoryContext, ProviderDescriptor,
    ProviderLocation, ProviderModality, SpeechRequest, SpeechStreamItem, TtsProvider, TurnIdentity,
};
use npc_runtime_host::{
    llm_bridge::selected_hosted_llm,
    tts_bridge::{RuntimeTtsBridge, RuntimeTtsBridgeConfig, VaultTtsCredentialResolver},
    RouteExecution, SelectedProviderRoute,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;
use zeroize::Zeroizing;

const OUTPUT_DIRECTORY_ENV: &str = "HEADLESS_ROUTE_QUALIFICATION_DIR";
const COHERE_MODEL: &str = "command-a-plus-05-2026";
const NVIDIA_VOICE: &str = "Magpie-Multilingual.EN-US.Aria";
const MAX_TEXT_BYTES: usize = 16 * 1024;
const EVENT_TIMEOUT: Duration = Duration::from_secs(90);

fn turn_identity() -> TurnIdentity {
    TurnIdentity {
        session_id: "headless-route-qualification".into(),
        turn_id: "headless-route-turn-1".into(),
        cancellation_generation: 1,
    }
}

fn generation_request() -> GenerationRequest {
    GenerationRequest {
        identity: turn_identity(),
        transcript: "Reply with exactly READY and nothing else.".into(),
        character: CharacterIdentity {
            character_id: Some("mara-venn".into()),
            display_name: "Mara Venn".into(),
            confidence: 1.0,
            evidence: vec!["explicit_selection".into()],
            explicit_selection: true,
        },
        memory: MemoryContext::default(),
        locale: "en-US".into(),
        metadata: BTreeMap::from([
            ("route_snapshot_generation".into(), "1".into()),
            ("route_snapshot_loadout".into(), "api-first-starter".into()),
            ("npc_response_format".into(), "legacy_plain_text".into()),
        ]),
    }
}

fn system_prompt() -> String {
    "Speak as Mara Venn, the harbor coordinator. Follow the requested exact short response. Never propose executable actions.".into()
}

fn hosted_speech_descriptor() -> ProviderDescriptor {
    ProviderDescriptor {
        id: HostedTtsProviderId::NvidiaNimMagpie.as_str().into(),
        display_name: "NVIDIA NIM Magpie".into(),
        modality: ProviderModality::Speech,
        location: ProviderLocation::Cloud {
            service: HostedTtsProviderId::NvidiaNimMagpie.as_str().into(),
        },
        may_retain_data: true,
        transmitted_data: vec![DataClass::Transcript],
        capabilities: BTreeMap::new(),
    }
}

fn validate_output_directory(path: &Path) -> PathBuf {
    fs::create_dir_all(path).expect("create bounded private qualification directory");
    let metadata = fs::symlink_metadata(path).expect("inspect qualification directory");
    assert!(metadata.is_dir() && !metadata.file_type().is_symlink());
    path.canonicalize()
        .expect("canonical private qualification directory")
}

fn write_new(path: &Path, bytes: &[u8]) {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .expect("qualification artifact must not overwrite an earlier run");
    file.write_all(bytes).expect("write qualification artifact");
    file.sync_all().expect("flush qualification artifact");
}

fn wav_bytes(sample_rate_hz: u32, channels: u16, pcm: &[u8]) -> Vec<u8> {
    assert!(sample_rate_hz > 0 && channels > 0 && pcm.len().is_multiple_of(2));
    let data_len = u32::try_from(pcm.len()).expect("bounded PCM length");
    let byte_rate = sample_rate_hz
        .checked_mul(u32::from(channels))
        .and_then(|value| value.checked_mul(2))
        .expect("bounded byte rate");
    let block_align = channels.checked_mul(2).expect("bounded block alignment");
    let mut bytes = Vec::with_capacity(44 + pcm.len());
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36_u32 + data_len).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&channels.to_le_bytes());
    bytes.extend_from_slice(&sample_rate_hz.to_le_bytes());
    bytes.extend_from_slice(&byte_rate.to_le_bytes());
    bytes.extend_from_slice(&block_align.to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_len.to_le_bytes());
    bytes.extend_from_slice(pcm);
    bytes
}

#[tokio::test]
#[ignore = "explicit private live Cohere-to-NVIDIA headless synthesis qualification"]
async fn exact_selected_reply_and_tts_adapters_write_a_non_playback_receipt() {
    let output_root = validate_output_directory(
        &env::var_os(OUTPUT_DIRECTORY_ENV)
            .map(PathBuf::from)
            .expect("HEADLESS_ROUTE_QUALIFICATION_DIR is required"),
    );
    let vault: Arc<dyn CredentialVault> = Arc::new(
        WindowsCredentialVault::new("interactive-npcs/v2").expect("open fixed Windows vault"),
    );

    let llm_route = SelectedProviderRoute {
        provider_id: "cohere".into(),
        model_id: COHERE_MODEL.into(),
        voice_id: None,
        execution: RouteExecution::Cloud,
        egress: "conversation_text_and_derived_game_context".into(),
        credential_reference: Some("providers/cohere".into()),
    };
    let llm = selected_hosted_llm(&llm_route, Arc::clone(&vault), system_prompt())
        .expect("construct exact selected LLM adapter");
    let llm_started = Instant::now();
    let mut response_stream = llm
        .stream_response(generation_request(), CancellationToken::new())
        .await
        .expect("start selected LLM stream");
    let mut first_llm_delta_ms = None;
    let mut llm_delta_count = 0_u64;
    let mut response = Zeroizing::new(String::new());
    while let Some(item) = tokio::time::timeout(EVENT_TIMEOUT, response_stream.next())
        .await
        .expect("selected LLM event timeout")
    {
        let delta = item.expect("selected LLM stream event");
        first_llm_delta_ms.get_or_insert_with(|| llm_started.elapsed().as_millis() as u64);
        llm_delta_count = llm_delta_count.saturating_add(1);
        assert!(response.len().saturating_add(delta.text.len()) <= MAX_TEXT_BYTES);
        response.push_str(&delta.text);
    }
    assert!(!response.trim().is_empty() && llm_delta_count > 0);
    let llm_total_ms = llm_started.elapsed().as_millis() as u64;
    let response_sha256 = format!("{:x}", Sha256::digest(response.as_bytes()));

    let intent_id = "selected.stock.voice";
    let bindings = VoiceBindings::new([VoiceBinding {
        intent_id: intent_id.into(),
        provider_id: HostedTtsProviderId::NvidiaNimMagpie,
        voice_id: NVIDIA_VOICE.into(),
        model_id: NVIDIA_MAGPIE_MODEL_ID.into(),
        provider_options: BTreeMap::new(),
    }])
    .expect("exact stock voice binding");
    let grpc = Arc::new(
        TonicNvidiaNimGrpcTransport::connect()
            .await
            .expect("connect exact NVIDIA streaming endpoint"),
    );
    let http =
        Arc::new(ReqwestNvidiaNimHttpTransport::new().expect("NVIDIA voice discovery client"));
    let mut upstream = NvidiaNimMagpie::new(
        grpc,
        http,
        Arc::new(VaultTtsCredentialResolver::new(Arc::clone(&vault))),
        bindings,
    );
    upstream
        .set_request_deadline(Duration::from_secs(60))
        .expect("bounded hosted synthesis deadline");
    assert!(upstream
        .discover_stock_voices()
        .await
        .expect("discover exact stock voice")
        .iter()
        .any(|voice| voice.id == NVIDIA_VOICE));
    let output = AudioFormat {
        sample_rate_hz: 44_100,
        ..AudioFormat::default()
    };
    let bridge = RuntimeTtsBridge::new(
        Arc::new(upstream) as Arc<dyn StreamingTtsProvider>,
        RuntimeTtsBridgeConfig {
            descriptor: hosted_speech_descriptor(),
            voice_intent_id: intent_id.into(),
            model_id: NVIDIA_MAGPIE_MODEL_ID.into(),
            output,
            request_alignment: true,
            request_visemes: false,
            clause_policy: SemanticClausePolicy::default(),
            first_pcm_timeout: Duration::from_secs(5),
        },
    )
    .expect("construct production runtime TTS bridge");
    let identity = turn_identity();
    let mut tts = bridge
        .start_session(&identity, "en-US", CancellationToken::new())
        .await
        .expect("start runtime TTS bridge session");
    let tts_started = Instant::now();
    let mut speech = tts
        .synthesize(
            SpeechRequest {
                identity,
                sentence_id: 1,
                text: response.to_string(),
                locale: "en-US".into(),
                voice_hint: None,
            },
            CancellationToken::new(),
        )
        .await
        .expect("start exact selected stock voice synthesis");
    let mut first_pcm_ms = None;
    let mut pcm = Vec::new();
    let mut prior_sequence = None;
    let mut eos = false;
    let mut alignment_events = 0_u64;
    while let Some(item) = tokio::time::timeout(EVENT_TIMEOUT, speech.next())
        .await
        .expect("selected TTS event timeout")
    {
        match item.expect("selected TTS stream event") {
            SpeechStreamItem::Audio(chunk) => {
                assert_eq!(chunk.sample_rate_hz, output.sample_rate_hz);
                assert_eq!(chunk.channels, output.channels);
                assert!(!chunk.pcm_s16le.is_empty() && chunk.pcm_s16le.len().is_multiple_of(2));
                assert!(prior_sequence.is_none_or(|prior| chunk.sequence > prior));
                prior_sequence = Some(chunk.sequence);
                first_pcm_ms.get_or_insert_with(|| tts_started.elapsed().as_millis() as u64);
                assert!(pcm.len().saturating_add(chunk.pcm_s16le.len()) <= 16 * 1024 * 1024);
                pcm.extend_from_slice(&chunk.pcm_s16le);
                eos |= chunk.end_of_stream;
            }
            SpeechStreamItem::Alignment(_) => {
                alignment_events = alignment_events.saturating_add(1);
            }
        }
    }
    assert!(eos && !pcm.is_empty());
    tts.close().await.expect("close runtime TTS bridge session");
    let tts_total_ms = tts_started.elapsed().as_millis() as u64;

    let mut peak = 0_f32;
    let mut squared_sum = 0_f64;
    for bytes in pcm.chunks_exact(2) {
        let sample = f32::from(i16::from_le_bytes([bytes[0], bytes[1]])) / 32_768.0;
        peak = peak.max(sample.abs());
        squared_sum += f64::from(sample * sample);
    }
    let sample_count = pcm.len() / 2;
    let rms = (squared_sum / sample_count as f64).sqrt() as f32;
    assert!(peak > 0.0 && rms > 0.0);
    let wav = wav_bytes(output.sample_rate_hz, output.channels, &pcm);
    let wav_sha256 = format!("{:x}", Sha256::digest(&wav));
    write_new(&output_root.join("selected-route-headless.wav"), &wav);

    let completed_at_epoch_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time after epoch")
        .as_millis() as u64;
    let receipt = json!({
        "schemaVersion": 1,
        "receiptType": "headlessSynthesisQualification",
        "scope": "selected-provider-adapters-through-runtime-tts-bridge",
        "completedAtEpochMs": completed_at_epoch_ms,
        "llm": {
            "providerId": "cohere",
            "modelId": COHERE_MODEL,
            "credentialReference": "providers/cohere",
            "deltaCount": llm_delta_count,
            "firstDeltaMs": first_llm_delta_ms,
            "totalMs": llm_total_ms,
            "responseBytes": response.len(),
            "responseSha256": response_sha256
        },
        "tts": {
            "providerId": HostedTtsProviderId::NvidiaNimMagpie.as_str(),
            "modelId": NVIDIA_MAGPIE_MODEL_ID,
            "voiceId": NVIDIA_VOICE,
            "credentialReference": "providers/nvidia-nim",
            "firstPcmMs": first_pcm_ms,
            "totalMs": tts_total_ms,
            "sampleRateHz": output.sample_rate_hz,
            "channels": output.channels,
            "pcmBytes": pcm.len(),
            "sampleCount": sample_count,
            "alignmentEvents": alignment_events,
            "endOfStream": eos,
            "peak": peak,
            "rms": rms,
            "wavSha256": wav_sha256
        },
        "audioDeliveryClaimed": false,
        "physicalAudibilityClaimed": false,
        "nativeBrokerReceipt": false,
        "productionTurnDeliverySatisfied": false,
        "reason": "A private WAV proves synthesis and runtime bridge formatting; it is not an operating-system playback endpoint or a native broker drain receipt."
    });
    write_new(
        &output_root.join("selected-route-headless-receipt.json"),
        &serde_json::to_vec_pretty(&receipt).expect("serialize sanitized receipt"),
    );
}

#![cfg(windows)]

use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use futures_util::StreamExt;
use npc_local_tts_native::{KokoroLocalConfig, KokoroLocalProvider, OUTPUT_SAMPLE_RATE_HZ};
use npc_runtime_core::{
    ProviderErrorKind, SpeechRequest, SpeechStreamItem, TtsProvider, TurnIdentity,
};
use tokio_util::sync::CancellationToken;
use windows_sys::Win32::{
    Foundation::CloseHandle,
    System::{
        ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS},
        Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ},
    },
};

fn process_rss(pid: u32) -> u64 {
    // SAFETY: the handle is checked, used only for a bounded read, and closed.
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, 0, pid);
        assert!(!process.is_null());
        let mut counters: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
        counters.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
        assert_ne!(GetProcessMemoryInfo(process, &mut counters, counters.cb), 0);
        let _ = CloseHandle(process);
        counters.WorkingSetSize as u64
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the verified pinned Kokoro pack"]
#[allow(clippy::print_stderr)] // The ignored qualification test emits its measured evidence.
async fn native_worker_emits_real_pcm_through_runtime_provider() {
    let pack_root = PathBuf::from(
        std::env::var_os("NPC_KOKORO_PACK_ROOT").expect("NPC_KOKORO_PACK_ROOT is required"),
    );
    let worker = PathBuf::from(env!("CARGO_BIN_EXE_npc-local-tts-native-worker"));
    let provider = KokoroLocalProvider::new(KokoroLocalConfig {
        worker_executable: worker,
        pack_root,
        voice_id: "af_heart".into(),
        locale: "en-US".into(),
        cpu_threads: 2,
        load_timeout: Duration::from_secs(120),
        response_timeout: Duration::from_secs(120),
    })
    .expect("valid pinned configuration");
    let identity = TurnIdentity {
        session_id: "native-kokoro-qualification".into(),
        turn_id: "turn-1".into(),
        cancellation_generation: 1,
    };
    let started = Instant::now();
    let mut session = provider
        .start_session(&identity, "en-US", CancellationToken::new())
        .await
        .expect("worker loads");
    let loaded = started.elapsed();
    let synthesis_started = Instant::now();
    let mut stream = session
        .synthesize(
            SpeechRequest {
                identity,
                sentence_id: 1,
                text: "The village watch reports clear roads beyond the eastern gate, but the bridge keeper still advises caution after sunset.".into(),
                locale: "en-US".into(),
                voice_hint: Some("af_heart".into()),
            },
            CancellationToken::new(),
        )
        .await
        .expect("synthesis starts");
    let mut pcm = Vec::new();
    let mut first_pcm = None;
    let mut terminal = false;
    let worker_pid = provider.worker_process_id().await.expect("worker PID");
    let mut observed_rss_bytes = process_rss(worker_pid);
    while let Some(item) = stream.next().await {
        match item.expect("valid worker event") {
            SpeechStreamItem::Audio(chunk) => {
                assert_eq!(chunk.sample_rate_hz, OUTPUT_SAMPLE_RATE_HZ);
                assert_eq!(chunk.channels, 1);
                if !chunk.pcm_s16le.is_empty() {
                    first_pcm.get_or_insert_with(|| synthesis_started.elapsed());
                    pcm.extend_from_slice(&chunk.pcm_s16le);
                    observed_rss_bytes = observed_rss_bytes.max(process_rss(worker_pid));
                }
                terminal |= chunk.end_of_stream;
            }
            SpeechStreamItem::Alignment(_) => panic!("Kokoro does not claim alignment"),
        }
    }
    let total = synthesis_started.elapsed();
    assert!(terminal);
    assert!(pcm.len() > 960);
    assert_eq!(pcm.len() % 2, 0);
    let samples = pcm
        .chunks_exact(2)
        .map(|bytes| i16::from_le_bytes([bytes[0], bytes[1]]))
        .collect::<Vec<_>>();
    let peak = samples
        .iter()
        .map(|sample| sample.unsigned_abs())
        .max()
        .unwrap_or(0);
    let square_sum: f64 = samples
        .iter()
        .map(|sample| {
            let normalized = f64::from(*sample) / f64::from(i16::MAX);
            normalized * normalized
        })
        .sum();
    let rms = (square_sum / samples.len() as f64).sqrt();
    let dc = samples.iter().map(|sample| f64::from(*sample)).sum::<f64>()
        / samples.len() as f64
        / f64::from(i16::MAX);
    assert!(peak > 32);
    assert!(rms > 0.001);
    assert!(dc.abs() < 0.002);
    assert!(first_pcm.expect("first PCM exists") <= total);
    eprintln!(
        "load_ms={:.2} first_pcm_ms={:.2} total_ms={:.2} audio_ms={:.2} rtf={:.4} frames={} peak={} rms={:.6} dc={:.7} observed_worker_rss_bytes={}",
        loaded.as_secs_f64() * 1000.0,
        first_pcm.expect("first PCM exists").as_secs_f64() * 1000.0,
        total.as_secs_f64() * 1000.0,
        samples.len() as f64 * 1000.0 / OUTPUT_SAMPLE_RATE_HZ as f64,
        total.as_secs_f64() / (samples.len() as f64 / OUTPUT_SAMPLE_RATE_HZ as f64),
        samples.len(),
        peak,
        rms,
        dc,
        observed_rss_bytes,
    );
    session.close().await.expect("worker closes");
    let warm_started = Instant::now();
    let mut warm_session = provider
        .start_session(
            &TurnIdentity {
                session_id: "native-kokoro-qualification".into(),
                turn_id: "turn-2".into(),
                cancellation_generation: 1,
            },
            "en-US",
            CancellationToken::new(),
        )
        .await
        .expect("warm worker is reused");
    assert!(warm_started.elapsed() < Duration::from_millis(100));
    let cancel = CancellationToken::new();
    let mut cancelled_stream = warm_session
        .synthesize(
            SpeechRequest {
                identity: TurnIdentity {
                    session_id: "native-kokoro-qualification".into(),
                    turn_id: "turn-2".into(),
                    cancellation_generation: 1,
                },
                sentence_id: 2,
                text: "Before the caravan leaves, the quartermaster checks every crate, records the seals, and asks the scouts to confirm the northern road is clear.".into(),
                locale: "en-US".into(),
                voice_hint: Some("af_heart".into()),
            },
            cancel.clone(),
        )
        .await
        .expect("cancellation probe starts");
    let cancel_started = Instant::now();
    cancel.cancel();
    let mut cancellation_seen = false;
    while let Some(item) = cancelled_stream.next().await {
        if let Err(error) = item {
            cancellation_seen = error.kind == ProviderErrorKind::Cancelled;
            break;
        }
    }
    assert!(cancellation_seen);
    assert!(cancel_started.elapsed() < Duration::from_secs(15));
    eprintln!(
        "cancel_drain_ms={:.2}",
        cancel_started.elapsed().as_secs_f64() * 1000.0
    );
    warm_session.close().await.expect("warm session closes");
    provider.shutdown().await.expect("provider shuts down");
}

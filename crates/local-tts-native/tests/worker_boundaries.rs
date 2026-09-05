#![cfg(windows)]

use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use futures_util::StreamExt;
use npc_local_tts_native::{
    KokoroLocalConfig, KokoroLocalProvider, PACK_ID, PACK_REVISION, PROVIDER_ID,
};
use npc_runtime_core::{
    ProviderErrorKind, SpeechRequest, SpeechStreamItem, TtsProvider, TtsSession, TurnIdentity,
};
use tokio_util::sync::CancellationToken;

static NEXT_TEST_ROOT: AtomicU64 = AtomicU64::new(1);

struct Harness {
    provider: KokoroLocalProvider,
    root: PathBuf,
    identity: TurnIdentity,
}

impl Harness {
    fn new(response_timeout: Duration) -> Self {
        let unique = NEXT_TEST_ROOT.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir()
            .join(format!(
                "npc-local-tts-boundary-{}-{unique}",
                std::process::id()
            ))
            .join(PACK_ID)
            .join(PACK_REVISION);
        fs::create_dir_all(&root).expect("create fake pack identity");
        let provider = KokoroLocalProvider::new(KokoroLocalConfig {
            worker_executable: PathBuf::from(env!("CARGO_BIN_EXE_npc-local-tts-fake-worker")),
            pack_root: root.clone(),
            voice_id: "af_heart".into(),
            locale: "en-US".into(),
            cpu_threads: 2,
            load_timeout: Duration::from_secs(2),
            response_timeout,
        })
        .expect("valid fake worker configuration");
        Self {
            provider,
            root,
            identity: TurnIdentity {
                session_id: format!("boundary-{unique}"),
                turn_id: "turn-1".into(),
                cancellation_generation: 1,
            },
        }
    }

    async fn session(&self) -> Box<dyn TtsSession> {
        self.provider
            .start_session(&self.identity, "en-US", CancellationToken::new())
            .await
            .expect("fake worker session")
    }

    fn request(&self, text: &str, sentence_id: u64) -> SpeechRequest {
        SpeechRequest {
            identity: self.identity.clone(),
            sentence_id,
            text: text.into(),
            locale: "en-US".into(),
            voice_hint: Some("af_heart".into()),
        }
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        let test_root = self
            .root
            .parent()
            .and_then(|path| path.parent())
            .expect("test root")
            .to_path_buf();
        let _ = fs::remove_dir_all(test_root);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn final_audio_is_nonempty_and_sequences_begin_at_one() {
    let harness = Harness::new(Duration::from_secs(1));
    let mut session = harness.session().await;
    let mut stream = session
        .synthesize(harness.request("healthy", 1), CancellationToken::new())
        .await
        .expect("synthesis starts");
    let mut chunks = Vec::new();
    while let Some(item) = stream.next().await {
        let SpeechStreamItem::Audio(chunk) = item.expect("valid item") else {
            panic!("unexpected alignment")
        };
        chunks.push(chunk);
    }
    assert!(chunks.len() >= 2);
    for (index, chunk) in chunks.iter().enumerate() {
        assert_eq!(chunk.sequence, index as u64 + 1);
        assert!(!chunk.pcm_s16le.is_empty());
        assert_eq!(chunk.pcm_s16le.len() % 2, 0);
        assert_eq!(chunk.end_of_stream, index + 1 == chunks.len());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancellation_while_waiting_for_gate_returns_without_starting_work() {
    let harness = Harness::new(Duration::from_secs(1));
    let mut first = harness.session().await;
    let first_cancel = CancellationToken::new();
    let mut first_stream = first
        .synthesize(harness.request("cancel_wait", 1), first_cancel.clone())
        .await
        .expect("first synthesis starts");

    let mut second = harness.session().await;
    let second_request = harness.request("healthy", 2);
    let second_cancel = CancellationToken::new();
    let second_cancel_for_task = second_cancel.clone();
    let waiting = tokio::spawn(async move {
        second
            .synthesize(second_request, second_cancel_for_task)
            .await
    });
    tokio::task::yield_now().await;
    second_cancel.cancel();
    let result = tokio::time::timeout(Duration::from_millis(250), waiting)
        .await
        .expect("gate wait is cancellation-aware")
        .expect("join succeeds");
    let error = match result {
        Ok(_) => panic!("cancelled gate wait unexpectedly started synthesis"),
        Err(error) => error,
    };
    assert_eq!(error.kind, ProviderErrorKind::Cancelled);

    first_cancel.cancel();
    let error = first_stream
        .next()
        .await
        .expect("terminal item")
        .expect_err("first synthesis cancelled");
    assert_eq!(error.kind, ProviderErrorKind::Cancelled);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancellation_discards_pcm_that_arrives_after_token_fires() {
    let harness = Harness::new(Duration::from_secs(1));
    let mut session = harness.session().await;
    let cancel = CancellationToken::new();
    let mut stream = session
        .synthesize(
            harness.request("late_audio_after_cancel", 1),
            cancel.clone(),
        )
        .await
        .expect("synthesis starts");
    cancel.cancel();
    let mut audio_items = 0;
    let mut cancelled = false;
    while let Some(item) = stream.next().await {
        match item {
            Ok(SpeechStreamItem::Audio(_)) => audio_items += 1,
            Ok(SpeechStreamItem::Alignment(_)) => panic!("unexpected alignment"),
            Err(error) => cancelled |= error.kind == ProviderErrorKind::Cancelled,
        }
    }
    assert_eq!(audio_items, 0);
    assert!(cancelled);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dropping_stream_cancels_worker_and_releases_gate() {
    let harness = Harness::new(Duration::from_secs(2));
    let mut first = harness.session().await;
    let stream = first
        .synthesize(harness.request("cancel_wait", 1), CancellationToken::new())
        .await
        .expect("first synthesis starts");
    drop(stream);

    let mut second = harness.session().await;
    let recovered = tokio::time::timeout(
        Duration::from_millis(500),
        second.synthesize(harness.request("healthy", 2), CancellationToken::new()),
    )
    .await
    .expect("stream drop releases gate before response timeout")
    .expect("next synthesis starts");
    drop(recovered);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn malformed_frame_retires_child_and_next_synthesis_restarts() {
    let harness = Harness::new(Duration::from_secs(1));
    let mut session = harness.session().await;
    let first_pid = harness
        .provider
        .worker_process_id()
        .await
        .expect("first PID");
    let mut malformed = session
        .synthesize(
            harness.request("malformed_once", 1),
            CancellationToken::new(),
        )
        .await
        .expect("request written");
    let error = malformed
        .next()
        .await
        .expect("error item")
        .expect_err("malformed frame rejected");
    assert_eq!(error.kind, ProviderErrorKind::Protocol);
    drop(malformed);

    let mut recovered = session
        .synthesize(
            harness.request("malformed_once", 2),
            CancellationToken::new(),
        )
        .await
        .expect("replacement child starts");
    let replacement_pid = harness
        .provider
        .worker_process_id()
        .await
        .expect("replacement PID");
    assert_ne!(first_pid, replacement_pid);
    assert!(recovered.next().await.expect("audio").is_ok());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hung_child_is_bounded_released_and_restarted() {
    let harness = Harness::new(Duration::from_millis(150));
    let mut session = harness.session().await;
    let first_pid = harness
        .provider
        .worker_process_id()
        .await
        .expect("first PID");
    let mut hung = session
        .synthesize(harness.request("hang_once", 1), CancellationToken::new())
        .await
        .expect("request written");
    let error = tokio::time::timeout(Duration::from_millis(500), hung.next())
        .await
        .expect("read has a deadline")
        .expect("timeout error item")
        .expect_err("hung worker rejected");
    assert_eq!(error.provider_id, PROVIDER_ID);
    assert_eq!(error.kind, ProviderErrorKind::Unavailable);
    drop(hung);

    let mut recovered = session
        .synthesize(harness.request("hang_once", 2), CancellationToken::new())
        .await
        .expect("replacement child starts");
    let replacement_pid = harness
        .provider
        .worker_process_id()
        .await
        .expect("replacement PID");
    assert_ne!(first_pid, replacement_pid);
    assert!(recovered.next().await.expect("audio").is_ok());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancellation_of_hung_child_is_bounded_and_reports_cancelled() {
    let harness = Harness::new(Duration::from_millis(150));
    let mut session = harness.session().await;
    let cancel = CancellationToken::new();
    let mut hung = session
        .synthesize(harness.request("hang_once", 1), cancel.clone())
        .await
        .expect("request written");
    cancel.cancel();
    let error = tokio::time::timeout(Duration::from_millis(500), hung.next())
        .await
        .expect("cancel drain has a deadline")
        .expect("cancel error item")
        .expect_err("hung cancelled worker rejected");
    assert_eq!(error.kind, ProviderErrorKind::Cancelled);
}

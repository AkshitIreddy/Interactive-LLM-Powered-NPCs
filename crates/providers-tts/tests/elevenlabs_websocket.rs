use std::{collections::BTreeMap, sync::Arc, time::Duration};

use futures_util::{SinkExt, StreamExt};
use npc_providers_tts::{
    ElevenLabsCommand, ElevenLabsWebSocketTransport, ElevenLabsWebSocketTransportConfig,
    HostedTtsProviderId, OpenRequest, SensitiveHeaderValue, SensitiveString, TransportError,
    TtsTransport, WireCommand, WireEvent,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::oneshot,
};
use tokio_tungstenite::{
    accept_hdr_async,
    tungstenite::{handshake::server::Request, Message},
};

const FIXTURE_SECRET: &str = "fixture-secret-never-log";

fn fixture_transport(io_timeout: Duration) -> ElevenLabsWebSocketTransport {
    ElevenLabsWebSocketTransport::new(ElevenLabsWebSocketTransportConfig {
        connect_timeout: Duration::from_secs(2),
        io_timeout,
        close_timeout: Duration::from_millis(50),
        max_frame_bytes: 64 * 1_024,
        max_message_bytes: 128 * 1_024,
        allow_insecure_loopback: true,
    })
    .expect("valid fixture configuration")
}

fn fixture_request(endpoint: String) -> OpenRequest {
    OpenRequest {
        provider_id: HostedTtsProviderId::ElevenLabs,
        endpoint,
        public_headers: BTreeMap::new(),
        secret_headers: BTreeMap::from([(
            "xi-api-key".to_owned(),
            SensitiveHeaderValue::raw(SensitiveString::new(FIXTURE_SECRET)),
        )]),
        query: BTreeMap::from([
            ("language_code".to_owned(), "en".to_owned()),
            ("model_id".to_owned(), "eleven_flash_v2_5".to_owned()),
            ("output_format".to_owned(), "pcm_24000".to_owned()),
            ("sync_alignment".to_owned(), "true".to_owned()),
        ]),
    }
}

#[tokio::test]
// The tungstenite fixture callback fixes its rejection type to an HTTP response;
// this test only returns the success branch but Clippy still measures that API type.
#[allow(clippy::result_large_err)]
async fn fixture_round_trip_preserves_wire_contract_and_event_order() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind fixture server");
    let address = listener.local_addr().expect("fixture address");
    let (observed_tx, observed_rx) = oneshot::channel();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept fixture client");
        let mut observed_tx = Some(observed_tx);
        let mut socket = accept_hdr_async(stream, |request: &Request, response| {
            let query = request.uri().query().unwrap_or_default().to_owned();
            let api_key = request
                .headers()
                .get("xi-api-key")
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_owned();
            observed_tx
                .take()
                .expect("handshake callback called once")
                .send((request.uri().path().to_owned(), query, api_key))
                .expect("test receives handshake");
            Ok(response)
        })
        .await
        .expect("upgrade fixture websocket");

        let initialize = socket
            .next()
            .await
            .expect("initialize frame")
            .expect("valid initialize frame")
            .into_text()
            .expect("initialize is text");
        let initialize: serde_json::Value =
            serde_json::from_str(&initialize).expect("initialize JSON");
        assert_eq!(initialize["text"], " ");
        assert_eq!(initialize["voice_settings"]["stability"], 0.4);
        assert_eq!(initialize["voice_settings"]["similarity_boost"], 0.8);
        assert_eq!(initialize["voice_settings"]["speed"], 1.1);

        let text = socket
            .next()
            .await
            .expect("text frame")
            .expect("valid text frame")
            .into_text()
            .expect("text command is text");
        let text: serde_json::Value = serde_json::from_str(&text).expect("text JSON");
        assert_eq!(text["text"], "The eastern lock is open. ");
        assert_eq!(text["try_trigger_generation"], true);

        let finish = socket
            .next()
            .await
            .expect("finish frame")
            .expect("valid finish frame")
            .into_text()
            .expect("finish command is text");
        let finish: serde_json::Value = serde_json::from_str(&finish).expect("finish JSON");
        assert_eq!(finish, serde_json::json!({"text": ""}));

        socket
            .send(Message::Text(
                serde_json::json!({
                    "audio": "AAECAwQ=",
                    "alignment": {
                        "chars": ["H", "i", " ", "M", "a", "r", "a"],
                        "char_start_times_ms": [0, 10, 20, 30, 40, 50, 60],
                        "char_durations_ms": [10, 10, 10, 10, 10, 10, 10]
                    },
                    "is_final": false
                })
                .to_string()
                .into(),
            ))
            .await
            .expect("send audio fixture");
        socket
            .send(Message::Text(r#"{"is_final":true}"#.into()))
            .await
            .expect("send completion fixture");
        let _ignored = socket.close(None).await;
    });

    let transport: Arc<dyn TtsTransport> = Arc::new(fixture_transport(Duration::from_secs(2)));
    let endpoint = format!("ws://{address}/v1/text-to-speech/voice-1/stream-input");
    let mut connection = transport
        .connect(fixture_request(endpoint))
        .await
        .expect("connect to local fixture");
    connection
        .send(WireCommand::ElevenLabs(ElevenLabsCommand::Initialize {
            stability: 0.4,
            similarity_boost: 0.8,
            speed: 1.1,
        }))
        .await
        .expect("send initialization");
    connection
        .send(WireCommand::ElevenLabs(ElevenLabsCommand::Text {
            text: SensitiveString::new("The eastern lock is open. "),
            try_trigger_generation: true,
        }))
        .await
        .expect("send text");
    connection
        .send(WireCommand::ElevenLabs(ElevenLabsCommand::Finish))
        .await
        .expect("send finish");

    let (path, query, api_key) = observed_rx.await.expect("observe fixture handshake");
    assert_eq!(path, "/v1/text-to-speech/voice-1/stream-input");
    assert!(query.contains("language_code=en"));
    assert!(query.contains("model_id=eleven_flash_v2_5"));
    assert!(query.contains("output_format=pcm_24000"));
    assert!(query.contains("sync_alignment=true"));
    assert_eq!(api_key, FIXTURE_SECRET);

    assert_eq!(
        connection.receive().await,
        Some(Ok(WireEvent::Audio(vec![0, 1, 2, 3, 4])))
    );
    let alignment = connection
        .receive()
        .await
        .expect("alignment event")
        .expect("valid alignment event");
    let WireEvent::Alignment(words) = alignment else {
        panic!("expected alignment event");
    };
    assert_eq!(words.len(), 2);
    assert_eq!(words[0].word, "Hi");
    assert_eq!(words[1].word, "Mara");
    assert_eq!(connection.receive().await, Some(Ok(WireEvent::Complete)));
    connection.close().await.expect("close fixture connection");
    server.await.expect("fixture server completes");
}

#[tokio::test]
async fn receive_deadline_is_bounded_and_content_free() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind fixture server");
    let address = listener.local_addr().expect("fixture address");
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept fixture client");
        let _socket = tokio_tungstenite::accept_async(stream)
            .await
            .expect("upgrade fixture websocket");
        tokio::time::sleep(Duration::from_millis(250)).await;
    });

    let transport = fixture_transport(Duration::from_millis(50));
    let endpoint = format!("ws://{address}/v1/text-to-speech/voice-1/stream-input");
    let mut connection = transport
        .connect(fixture_request(endpoint))
        .await
        .expect("connect to local fixture");
    assert_eq!(
        connection.receive().await,
        Some(Err(TransportError::Timeout))
    );
    assert!(!format!("{:?}", TransportError::Timeout).contains(FIXTURE_SECRET));
    let _ignored = connection.close().await;
    server.await.expect("fixture server completes");
}

#[tokio::test]
async fn close_aborts_an_unresponsive_peer_within_the_barge_in_budget() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind fixture server");
    let address = listener.local_addr().expect("fixture address");
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept fixture client");
        let _unresponsive_socket = tokio_tungstenite::accept_async(stream)
            .await
            .expect("upgrade fixture websocket");
        // Intentionally do not poll the peer after the upgrade: it neither reads
        // the close frame nor sends a close acknowledgement.
        tokio::time::sleep(Duration::from_millis(250)).await;
    });

    let transport = fixture_transport(Duration::from_secs(1));
    let endpoint = format!("ws://{address}/v1/text-to-speech/voice-1/stream-input");
    let mut connection = transport
        .connect(fixture_request(endpoint))
        .await
        .expect("connect to local fixture");
    let started = tokio::time::Instant::now();
    connection
        .close()
        .await
        .expect("close or hard-abort succeeds");
    let elapsed = started.elapsed();
    assert!(elapsed >= Duration::from_millis(30));
    assert!(elapsed <= Duration::from_millis(150));
    assert_eq!(
        connection
            .send(WireCommand::ElevenLabs(ElevenLabsCommand::Finish))
            .await,
        Err(TransportError::Closed)
    );
    assert_eq!(connection.receive().await, None);
    server.await.expect("fixture server completes");
}

#[tokio::test]
async fn oversized_provider_message_is_a_bounded_protocol_failure() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind fixture server");
    let address = listener.local_addr().expect("fixture address");
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept fixture client");
        let mut socket = tokio_tungstenite::accept_async(stream)
            .await
            .expect("upgrade fixture websocket");
        socket
            .send(Message::Text("x".repeat(129 * 1_024).into()))
            .await
            .expect("send oversized fixture frame");
    });

    let transport = fixture_transport(Duration::from_secs(1));
    let endpoint = format!("ws://{address}/v1/text-to-speech/voice-1/stream-input");
    let mut connection = transport
        .connect(fixture_request(endpoint))
        .await
        .expect("connect to local fixture");
    assert_eq!(
        connection.receive().await,
        Some(Err(TransportError::Protocol))
    );
    server.await.expect("fixture server completes");
}

#[tokio::test]
async fn handshake_statuses_map_without_retaining_provider_bodies() {
    let rate_limited = rejected_handshake(
        "429 Too Many Requests",
        Some("Retry-After: 7\r\n"),
        r#"{"detail":"fixture-secret-never-log"}"#,
    )
    .await;
    assert_eq!(
        rate_limited,
        TransportError::RateLimited {
            retry_after: Some(Duration::from_secs(7))
        }
    );
    assert!(!format!("{rate_limited:?}").contains("fixture-secret"));

    let authentication = rejected_handshake(
        "401 Unauthorized",
        None,
        r#"{"detail":"another-secret-provider-body"}"#,
    )
    .await;
    assert_eq!(authentication, TransportError::Authentication);
    assert!(!format!("{authentication:?}").contains("another-secret"));

    let quota = rejected_handshake(
        "402 Payment Required",
        None,
        r#"{"detail":{"code":"insufficient_credits"}}"#,
    )
    .await;
    assert_eq!(quota, TransportError::QuotaExceeded);
}

async fn rejected_handshake(
    status: &'static str,
    additional_headers: Option<&'static str>,
    body: &'static str,
) -> TransportError {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind fixture server");
    let address = listener.local_addr().expect("fixture address");
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("accept fixture client");
        let mut request = vec![0_u8; 16 * 1_024];
        let read = stream
            .read(&mut request)
            .await
            .expect("read upgrade request");
        assert!(read > 0);
        let response = format!(
            "HTTP/1.1 {status}\r\n{}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            additional_headers.unwrap_or_default(),
            body.len()
        );
        stream
            .write_all(response.as_bytes())
            .await
            .expect("write rejected handshake");
    });

    let transport = fixture_transport(Duration::from_secs(1));
    let endpoint = format!("ws://{address}/v1/text-to-speech/voice-1/stream-input");
    let error = transport
        .connect(fixture_request(endpoint))
        .await
        .err()
        .expect("fixture handshake must fail");
    server.await.expect("fixture server completes");
    error
}

#[tokio::test]
async fn transport_rejects_remote_plaintext_and_query_credentials_before_network_io() {
    let transport = fixture_transport(Duration::from_secs(1));
    let mut remote =
        fixture_request("ws://example.com/v1/text-to-speech/voice-1/stream-input".to_owned());
    assert_eq!(
        transport.connect(remote).await.err(),
        Some(TransportError::Protocol)
    );

    remote = fixture_request(
        "wss://malicious.invalid/v1/text-to-speech/voice-1/stream-input".to_owned(),
    );
    assert_eq!(
        transport.connect(remote).await.err(),
        Some(TransportError::Protocol)
    );

    remote = fixture_request("wss://api.elevenlabs.io/v1/user/stream-input".to_owned());
    assert_eq!(
        transport.connect(remote).await.err(),
        Some(TransportError::Protocol)
    );

    remote = fixture_request("ws://127.0.0.1:9/v1/text-to-speech/voice-1/stream-input".to_owned());
    remote.query.insert(
        "single_use_token".to_owned(),
        "must-not-enter-url".to_owned(),
    );
    assert_eq!(
        transport.connect(remote).await.err(),
        Some(TransportError::Protocol)
    );
}

#[test]
fn configuration_rejects_unbounded_or_zero_deadlines() {
    assert_eq!(
        ElevenLabsWebSocketTransport::new(ElevenLabsWebSocketTransportConfig {
            connect_timeout: Duration::ZERO,
            ..ElevenLabsWebSocketTransportConfig::default()
        })
        .err(),
        Some(TransportError::Protocol)
    );
    assert_eq!(
        ElevenLabsWebSocketTransport::new(ElevenLabsWebSocketTransportConfig {
            max_frame_bytes: 2 * 1_048_576,
            max_message_bytes: 1_048_576,
            ..ElevenLabsWebSocketTransportConfig::default()
        })
        .err(),
        Some(TransportError::Protocol)
    );
    assert_eq!(
        ElevenLabsWebSocketTransport::new(ElevenLabsWebSocketTransportConfig {
            close_timeout: Duration::from_millis(151),
            ..ElevenLabsWebSocketTransportConfig::default()
        })
        .err(),
        Some(TransportError::Protocol)
    );
}

// tungstenite fixes the handshake callback's error type to a large HTTP
// response. The deterministic fixture callbacks only return `Ok`.
#![allow(clippy::result_large_err)]

use std::{collections::BTreeMap, sync::Arc, time::Duration};

use futures_util::{SinkExt, StreamExt};
use npc_providers_tts::*;
use serde_json::{json, Value};
use tokio::{net::TcpListener, sync::Notify};
use tokio_tungstenite::{
    accept_hdr_async,
    tungstenite::{
        handshake::server::{Request, Response},
        Message,
    },
};

struct FixtureCredential;

#[async_trait::async_trait]
impl ProviderCredentialResolver for FixtureCredential {
    async fn resolve(
        &self,
        _provider_id: HostedTtsProviderId,
    ) -> Result<SensitiveString, CredentialResolveError> {
        Ok(SensitiveString::new("fixture-credential-never-log"))
    }
}

fn limits() -> HostedWebSocketLimits {
    HostedWebSocketLimits {
        connect_timeout: Duration::from_secs(2),
        io_timeout: Duration::from_secs(2),
        close_timeout: Duration::from_millis(100),
        max_frame_bytes: 64 * 1_024,
        max_message_bytes: 128 * 1_024,
        allow_insecure_loopback: true,
    }
}

fn open_request(
    provider_id: HostedTtsProviderId,
    endpoint: String,
    secret_header: &str,
    scheme: Option<&'static str>,
) -> OpenRequest {
    let mut secret_headers = BTreeMap::new();
    let credential = SensitiveString::new("fixture-credential-never-log");
    let credential = match scheme {
        Some(scheme) => SensitiveHeaderValue::with_scheme(scheme, credential),
        None => SensitiveHeaderValue::raw(credential),
    };
    secret_headers.insert(secret_header.into(), credential);
    OpenRequest {
        provider_id,
        endpoint,
        public_headers: BTreeMap::new(),
        secret_headers,
        query: BTreeMap::new(),
    }
}

async fn next_text<S>(socket: &mut tokio_tungstenite::WebSocketStream<S>) -> Value
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let message = socket
        .next()
        .await
        .expect("client message")
        .expect("valid client message");
    let Message::Text(text) = message else {
        panic!("expected JSON text frame");
    };
    serde_json::from_slice(text.as_bytes()).expect("valid client JSON")
}

#[tokio::test]
async fn cartesia_finish_releases_before_audio_and_completed_socket_is_reused() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("fixture operation");
    let address = listener.local_addr().expect("fixture operation");
    let release_audio = Arc::new(Notify::new());
    let release_on_server = Arc::clone(&release_audio);

    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("fixture operation");
        let mut socket = accept_hdr_async(stream, |request: &Request, response: Response| {
            assert_eq!(request.uri().path(), "/tts/websocket");
            assert_eq!(request.uri().query(), Some("cartesia_version=2026-03-01"));
            assert_eq!(
                request
                    .headers()
                    .get("x-api-key")
                    .expect("fixture operation"),
                "fixture-credential-never-log"
            );
            Ok(response)
        })
        .await
        .expect("fixture operation");

        let mut first_wire_context = None;
        for sentence_index in 0..2 {
            let command = next_text(&mut socket).await;
            assert_eq!(command["model_id"], "sonic-3.6");
            assert_eq!(command["voice"]["id"], CARTESIA_QUALIFIED_STOCK_VOICE_ID);
            let wire_context = command["context_id"].as_str().expect("fixture operation");
            assert!(wire_context.starts_with("same-session:same-turn:7:synthesis-"));
            if let Some(first) = &first_wire_context {
                assert_ne!(
                    first, wire_context,
                    "each sentence needs a unique provider context"
                );
            } else {
                first_wire_context = Some(wire_context.to_owned());
            }
            assert_eq!(command["output_format"]["container"], "raw");
            assert_eq!(command["output_format"]["encoding"], "pcm_s16le");
            assert_eq!(command["output_format"]["sample_rate"], 24_000);
            assert_eq!(command["continue"], false);
            if sentence_index == 0 {
                release_on_server.notified().await;
            }
            socket
                .send(Message::Text(
                    json!({
                        "type":"flush_done",
                        "done":false,
                        "flush_done":true,
                        "flush_id":1,
                        "context_id":wire_context
                    })
                    .to_string()
                    .into(),
                ))
                .await
                .expect("fixture operation");
            socket
                .send(Message::Text(
                    json!({"type":"chunk","context_id":wire_context,"data":"AAECAw=="})
                        .to_string()
                        .into(),
                ))
                .await
                .expect("fixture operation");
            socket
                .send(Message::Text(
                    json!({"type":"done","context_id":wire_context})
                        .to_string()
                        .into(),
                ))
                .await
                .expect("fixture operation");
        }
    });

    let config = CartesiaWebSocketTransportConfig {
        limits: limits(),
        idle_timeout: Duration::from_secs(2),
    };
    let transport = Arc::new(CartesiaWebSocketTransport::new(config).expect("fixture operation"));
    let provider = CartesiaProvider::new(
        Arc::clone(&transport) as Arc<dyn TtsTransport>,
        Arc::new(FixtureCredential),
        VoiceBindings::new([VoiceBinding {
            intent_id: "qualified-stock".into(),
            provider_id: HostedTtsProviderId::Cartesia,
            voice_id: CARTESIA_QUALIFIED_STOCK_VOICE_ID.into(),
            model_id: CARTESIA_QUALIFIED_MODEL_ID.into(),
            provider_options: BTreeMap::new(),
        }])
        .expect("fixture operation"),
        CartesiaConfig {
            endpoint: format!("ws://{address}/tts/websocket"),
            api_version: CARTESIA_QUALIFIED_API_VERSION.into(),
        },
    );
    let request = || TtsSessionRequest {
        identity: SessionIdentity {
            session_id: "same-session".into(),
            turn_id: "same-turn".into(),
            cancellation_generation: 7,
        },
        locale: "en-US".into(),
        voice_intent_id: "qualified-stock".into(),
        output: CARTESIA_QUALIFIED_AUDIO_FORMAT,
        request_alignment: true,
        request_visemes: true,
        clause_policy: SemanticClausePolicy::default(),
    };

    let mut first = provider
        .start_session(request())
        .await
        .expect("fixture operation");
    first
        .push_text("A bounded fixture sentence")
        .await
        .expect("fixture operation");
    tokio::time::timeout(Duration::from_millis(100), first.finish())
        .await
        .expect("finish must not wait for server audio")
        .expect("fixture operation");
    release_audio.notify_one();
    assert_eq!(
        first
            .next_event()
            .await
            .expect("fixture operation")
            .expect("fixture operation"),
        TtsEvent::Audio(PcmChunk {
            sequence: 0,
            format: CARTESIA_QUALIFIED_AUDIO_FORMAT,
            data: vec![0, 1, 2, 3],
        })
    );
    assert_eq!(
        first
            .next_event()
            .await
            .expect("fixture operation")
            .expect("fixture operation"),
        TtsEvent::Completed
    );

    let mut second = provider
        .start_session(request())
        .await
        .expect("fixture operation");
    second
        .push_text("A second sentence")
        .await
        .expect("fixture operation");
    second.finish().await.expect("fixture operation");
    assert!(matches!(
        second
            .next_event()
            .await
            .expect("fixture operation")
            .expect("fixture operation"),
        TtsEvent::Audio(_)
    ));
    assert_eq!(
        second
            .next_event()
            .await
            .expect("fixture operation")
            .expect("fixture operation"),
        TtsEvent::Completed
    );

    let stats = transport.connection_stats();
    assert_eq!(stats.fresh_connections, 1);
    assert_eq!(stats.reused_connections, 1);
    assert!(stats.last_connection_reused);
    server.await.expect("fixture operation");
}

async fn cartesia_connection(
    transport: &CartesiaWebSocketTransport,
    address: std::net::SocketAddr,
    _context: &str,
) -> Box<dyn TtsConnection> {
    let mut request = open_request(
        HostedTtsProviderId::Cartesia,
        format!("ws://{address}/tts/websocket"),
        "X-API-Key",
        None,
    );
    request
        .public_headers
        .insert("Cartesia-Version".into(), "2026-03-01".into());
    transport.connect(request).await.expect("fixture operation")
}

fn cartesia_generate(context_id: &str) -> WireCommand {
    WireCommand::Cartesia(CartesiaCommand::Generate {
        context_id: context_id.into(),
        model_id: "sonic-3.6".into(),
        voice_id: CARTESIA_QUALIFIED_STOCK_VOICE_ID.into(),
        language: "en-US".into(),
        output: AudioFormat::default(),
        transcript: SensitiveString::new("A bounded fixture sentence."),
        continue_generation: false,
        add_timestamps: true,
    })
}

#[tokio::test]
async fn cartesia_cancelled_connection_is_closed_and_never_reused() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("fixture operation");
    let address = listener.local_addr().expect("fixture operation");
    let server = tokio::spawn(async move {
        for _ in 0..2 {
            let (stream, _) = listener.accept().await.expect("fixture operation");
            let mut socket =
                accept_hdr_async(stream, |_: &Request, response: Response| Ok(response))
                    .await
                    .expect("fixture operation");
            let _command = next_text(&mut socket).await;
            let _ = socket.close(None).await;
        }
    });

    let transport = CartesiaWebSocketTransport::new(CartesiaWebSocketTransportConfig {
        limits: limits(),
        idle_timeout: Duration::from_secs(2),
    })
    .expect("fixture operation");
    let mut first = cartesia_connection(&transport, address, "cancelled").await;
    first
        .send(WireCommand::Cartesia(CartesiaCommand::Cancel {
            context_id: "cancelled".into(),
        }))
        .await
        .expect("fixture operation");
    first.close().await.expect("fixture operation");
    let mut second = cartesia_connection(&transport, address, "new").await;
    second
        .send(WireCommand::Cartesia(CartesiaCommand::Cancel {
            context_id: "new".into(),
        }))
        .await
        .expect("fixture operation");
    second.close().await.expect("fixture operation");
    assert_eq!(transport.connection_stats().fresh_connections, 2);
    assert_eq!(transport.connection_stats().reused_connections, 0);
    server.await.expect("fixture operation");
}

#[tokio::test]
async fn cartesia_expired_idle_socket_is_evicted_before_new_upgrade() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("fixture operation");
    let address = listener.local_addr().expect("fixture operation");
    let server = tokio::spawn(async move {
        for context in ["old-context", "fresh-context"] {
            let (stream, _) = listener.accept().await.expect("fixture operation");
            let mut socket =
                accept_hdr_async(stream, |_: &Request, response: Response| Ok(response))
                    .await
                    .expect("fixture operation");
            let command = next_text(&mut socket).await;
            let wire_context = command["context_id"].as_str().expect("fixture operation");
            assert!(wire_context.starts_with(&format!("{context}:synthesis-")));
            socket
                .send(Message::Text(
                    json!({"type":"done","context_id":wire_context})
                        .to_string()
                        .into(),
                ))
                .await
                .expect("fixture operation");
            if context == "old-context" {
                let close = socket
                    .next()
                    .await
                    .expect("fixture operation")
                    .expect("fixture operation");
                assert!(matches!(close, Message::Close(_)));
            }
        }
    });

    let transport = CartesiaWebSocketTransport::new(CartesiaWebSocketTransportConfig {
        limits: limits(),
        idle_timeout: Duration::from_millis(5),
    })
    .expect("fixture operation");
    let mut old = cartesia_connection(&transport, address, "old-context").await;
    old.send(cartesia_generate("old-context"))
        .await
        .expect("fixture operation");
    assert_eq!(
        old.receive()
            .await
            .expect("fixture operation")
            .expect("fixture operation"),
        WireEvent::Complete
    );
    old.close().await.expect("fixture operation");
    tokio::time::sleep(Duration::from_millis(20)).await;

    let mut fresh = cartesia_connection(&transport, address, "fresh-context").await;
    fresh
        .send(cartesia_generate("fresh-context"))
        .await
        .expect("fixture operation");
    assert_eq!(
        fresh
            .receive()
            .await
            .expect("fixture operation")
            .expect("fixture operation"),
        WireEvent::Complete
    );
    fresh.close().await.expect("fixture operation");
    let stats = transport.connection_stats();
    assert_eq!(stats.fresh_connections, 2);
    assert_eq!(stats.reused_connections, 0);
    assert_eq!(stats.stale_evictions, 1);
    server.await.expect("fixture operation");
}

#[tokio::test]
async fn cartesia_drop_and_provider_error_do_not_return_socket_to_pool() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("fixture operation");
    let address = listener.local_addr().expect("fixture operation");
    let server = tokio::spawn(async move {
        let (dropped_stream, _) = listener.accept().await.expect("fixture operation");
        let mut dropped = accept_hdr_async(dropped_stream, |_: &Request, response: Response| {
            Ok(response)
        })
        .await
        .expect("fixture operation");
        assert!(matches!(
            dropped.next().await,
            None | Some(Err(_)) | Some(Ok(Message::Close(_)))
        ));

        let (error_stream, _) = listener.accept().await.expect("fixture operation");
        let mut errored =
            accept_hdr_async(error_stream, |_: &Request, response: Response| Ok(response))
                .await
                .expect("fixture operation");
        let command = next_text(&mut errored).await;
        assert!(command["context_id"]
            .as_str()
            .expect("fixture operation")
            .starts_with("error-context:synthesis-"));
        errored
            .send(Message::Text(
                json!({"type":"error","error_code":"quota_exceeded","message":"sensitive provider detail"})
                    .to_string()
                    .into(),
            ))
            .await
            .expect("fixture operation");
        assert!(matches!(
            errored
                .next()
                .await
                .expect("fixture operation")
                .expect("fixture operation"),
            Message::Close(_)
        ));

        let (final_stream, _) = listener.accept().await.expect("fixture operation");
        let mut final_socket =
            accept_hdr_async(final_stream, |_: &Request, response: Response| Ok(response))
                .await
                .expect("fixture operation");
        let command = next_text(&mut final_socket).await;
        assert!(command["context_id"]
            .as_str()
            .expect("fixture operation")
            .starts_with("final-context:synthesis-"));
        assert!(matches!(
            final_socket.next().await,
            None | Some(Err(_)) | Some(Ok(Message::Close(_)))
        ));
    });

    let transport = CartesiaWebSocketTransport::new(CartesiaWebSocketTransportConfig {
        limits: limits(),
        idle_timeout: Duration::from_secs(2),
    })
    .expect("fixture operation");
    let dropped = cartesia_connection(&transport, address, "dropped-context").await;
    drop(dropped);

    let mut errored = cartesia_connection(&transport, address, "error-context").await;
    errored
        .send(cartesia_generate("error-context"))
        .await
        .expect("fixture operation");
    assert_eq!(
        errored.receive().await.expect("fixture operation"),
        Err(TransportError::QuotaExceeded)
    );
    errored.close().await.expect("fixture operation");

    let mut final_connection = cartesia_connection(&transport, address, "final-context").await;
    final_connection
        .send(WireCommand::Cartesia(CartesiaCommand::Cancel {
            context_id: "final-context".into(),
        }))
        .await
        .expect("fixture operation");
    let _closed_without_pooling = final_connection.close().await;
    assert_eq!(transport.connection_stats().fresh_connections, 3);
    assert_eq!(transport.connection_stats().reused_connections, 0);
    server.await.expect("fixture operation");
}

#[tokio::test]
async fn cartesia_rejects_unqualified_route_and_cross_context_responses() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("fixture operation");
    let address = listener.local_addr().expect("fixture operation");
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("fixture operation");
        let mut socket = accept_hdr_async(stream, |_: &Request, response: Response| Ok(response))
            .await
            .expect("fixture operation");
        let command = next_text(&mut socket).await;
        assert!(command["context_id"]
            .as_str()
            .expect("fixture operation")
            .starts_with("current:synthesis-"));
        socket
            .send(Message::Text(
                json!({"type":"chunk","context_id":"prior:synthesis-1","data":"AAEC"})
                    .to_string()
                    .into(),
            ))
            .await
            .expect("fixture operation");

        let (error_stream, _) = listener.accept().await.expect("fixture operation");
        let mut error_socket =
            accept_hdr_async(error_stream, |_: &Request, response: Response| Ok(response))
                .await
                .expect("fixture operation");
        let command = next_text(&mut error_socket).await;
        assert!(command["context_id"]
            .as_str()
            .expect("fixture operation")
            .starts_with("current-error:synthesis-"));
        error_socket
            .send(Message::Text(
                json!({
                    "type":"error",
                    "context_id":"prior:synthesis-1",
                    "error_code":"quota_exceeded",
                    "message":"sensitive provider detail"
                })
                .to_string()
                .into(),
            ))
            .await
            .expect("fixture operation");
    });

    let transport = CartesiaWebSocketTransport::new(CartesiaWebSocketTransportConfig {
        limits: limits(),
        idle_timeout: Duration::from_secs(2),
    })
    .expect("fixture operation");
    let mut connection = cartesia_connection(&transport, address, "current").await;
    let mut unqualified = cartesia_generate("current");
    if let WireCommand::Cartesia(CartesiaCommand::Generate { model_id, .. }) = &mut unqualified {
        *model_id = "unqualified-model".into();
    }
    assert_eq!(
        connection.send(unqualified).await,
        Err(TransportError::ProtocolStage(
            "cartesia_route_not_qualified"
        ))
    );

    connection
        .send(cartesia_generate("current"))
        .await
        .expect("fixture operation");
    assert_eq!(
        connection.receive().await.expect("fixture operation"),
        Err(TransportError::ProtocolStage("cartesia_context_mismatch"))
    );
    let _closed = connection.close().await;
    assert_eq!(transport.connection_stats().reused_connections, 0);

    let mut error_connection = cartesia_connection(&transport, address, "current-error").await;
    error_connection
        .send(cartesia_generate("current-error"))
        .await
        .expect("fixture operation");
    assert_eq!(
        error_connection.receive().await.expect("fixture operation"),
        Err(TransportError::ProtocolStage("cartesia_context_mismatch"))
    );
    let _closed = error_connection.close().await;
    assert_eq!(transport.connection_stats().reused_connections, 0);
    server.await.expect("fixture operation");
}

#[tokio::test]
async fn deepgram_flux_waits_for_speech_metadata_after_flushed() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("fixture operation");
    let address = listener.local_addr().expect("fixture operation");
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("fixture operation");
        let mut socket = accept_hdr_async(stream, |request: &Request, response: Response| {
            assert_eq!(request.uri().path(), "/v2/speak");
            assert!(request
                .uri()
                .query()
                .expect("fixture operation")
                .contains("model=flux-miles-en"));
            Ok(response)
        })
        .await
        .expect("fixture operation");
        socket
            .send(Message::Text(
                json!({"type":"Connected"}).to_string().into(),
            ))
            .await
            .expect("fixture operation");
        assert_eq!(next_text(&mut socket).await["type"], "Speak");
        assert_eq!(next_text(&mut socket).await["type"], "Flush");
        socket
            .send(Message::Binary(vec![0, 1, 2, 3].into()))
            .await
            .expect("fixture operation");
        socket
            .send(Message::Text(json!({"type":"Flushed"}).to_string().into()))
            .await
            .expect("fixture operation");
        tokio::time::sleep(Duration::from_millis(20)).await;
        socket.send(Message::Text(json!({"type":"SpeechMetadata","speech_id":"safe-id","metadata":{"billable_character_count":8}}).to_string().into())).await.expect("fixture operation");
    });

    let mut request = open_request(
        HostedTtsProviderId::Deepgram,
        format!("ws://{address}/v2/speak"),
        "Authorization",
        Some("Token"),
    );
    request.query.insert("model".into(), "flux-miles-en".into());
    request.query.insert("encoding".into(), "linear16".into());
    request.query.insert("sample_rate".into(), "24000".into());
    request.query.insert("mip_opt_out".into(), "true".into());
    let transport =
        DeepgramWebSocketTransport::new(DeepgramWebSocketTransportConfig { limits: limits() })
            .expect("fixture operation");
    let mut connection = transport.connect(request).await.expect("fixture operation");
    connection
        .send(WireCommand::Deepgram(DeepgramCommand::Speak {
            text: SensitiveString::new("sentence"),
        }))
        .await
        .expect("fixture operation");
    connection
        .send(WireCommand::Deepgram(DeepgramCommand::Flush))
        .await
        .expect("fixture operation");
    assert!(matches!(
        connection
            .receive()
            .await
            .expect("fixture operation")
            .expect("fixture operation"),
        WireEvent::Audio(_)
    ));
    assert!(matches!(
        connection
            .receive()
            .await
            .expect("fixture operation")
            .expect("fixture operation"),
        WireEvent::Usage(_)
    ));
    assert_eq!(
        connection
            .receive()
            .await
            .expect("fixture operation")
            .expect("fixture operation"),
        WireEvent::Complete
    );
    server.await.expect("fixture operation");
}

fn deepgram_flux_request(address: std::net::SocketAddr) -> OpenRequest {
    let mut request = open_request(
        HostedTtsProviderId::Deepgram,
        format!("ws://{address}/v2/speak"),
        "Authorization",
        Some("Token"),
    );
    request.query.insert("model".into(), "flux-miles-en".into());
    request.query.insert("encoding".into(), "linear16".into());
    request.query.insert("sample_rate".into(), "24000".into());
    request.query.insert("mip_opt_out".into(), "true".into());
    request
}

fn deepgram_aura_request(address: std::net::SocketAddr) -> OpenRequest {
    let mut request = open_request(
        HostedTtsProviderId::Deepgram,
        format!("ws://{address}/v1/speak"),
        "Authorization",
        Some("Token"),
    );
    request
        .query
        .insert("model".into(), DEEPGRAM_QUALIFIED_AURA2_MODEL_ID.into());
    request.query.insert("encoding".into(), "linear16".into());
    request.query.insert("sample_rate".into(), "24000".into());
    request.query.insert("mip_opt_out".into(), "true".into());
    request
}

#[tokio::test]
async fn deepgram_ready_wait_has_one_total_deadline_despite_progress_frames() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("fixture operation");
    let address = listener.local_addr().expect("fixture operation");
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("fixture operation");
        let mut socket = accept_hdr_async(stream, |_: &Request, response: Response| Ok(response))
            .await
            .expect("fixture operation");
        for _ in 0..100 {
            if socket
                .send(Message::Text(json!({"type":"Metadata"}).to_string().into()))
                .await
                .is_err()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    });

    let mut bounded = limits();
    bounded.io_timeout = Duration::from_millis(10);
    let transport =
        DeepgramWebSocketTransport::new(DeepgramWebSocketTransportConfig { limits: bounded })
            .expect("fixture operation");
    assert_eq!(
        tokio::time::timeout(
            Duration::from_millis(250),
            transport.connect(deepgram_flux_request(address)),
        )
        .await
        .expect("fixture has a strict wall-clock budget")
        .err(),
        Some(TransportError::Timeout)
    );
    tokio::time::timeout(Duration::from_millis(250), server)
        .await
        .expect("fixture server has a strict wall-clock budget")
        .expect("fixture operation");
}

#[tokio::test]
async fn deepgram_ready_wait_rejects_an_unbounded_pre_ready_event_queue() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("fixture operation");
    let address = listener.local_addr().expect("fixture operation");
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("fixture operation");
        let mut socket = accept_hdr_async(stream, |_: &Request, response: Response| Ok(response))
            .await
            .expect("fixture operation");
        for index in 0..9 {
            socket
                .send(Message::Text(
                    json!({
                        "type":"SpeechMetadata",
                        "speech_id":format!("pre-ready-{index}"),
                        "metadata":{"billable_character_count":1}
                    })
                    .to_string()
                    .into(),
                ))
                .await
                .expect("fixture operation");
        }
    });

    let transport =
        DeepgramWebSocketTransport::new(DeepgramWebSocketTransportConfig { limits: limits() })
            .expect("fixture operation");
    assert_eq!(
        transport
            .connect(deepgram_flux_request(address))
            .await
            .err(),
        Some(TransportError::ProtocolStage(
            "hosted_pending_event_limit_exceeded"
        ))
    );
    server.await.expect("fixture operation");
}

#[tokio::test]
async fn hosted_receive_has_one_total_deadline_despite_progress_frames() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("fixture operation");
    let address = listener.local_addr().expect("fixture operation");
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("fixture operation");
        let mut socket = accept_hdr_async(stream, |_: &Request, response: Response| Ok(response))
            .await
            .expect("fixture operation");
        for _ in 0..100 {
            if socket
                .send(Message::Text(json!({"type":"Metadata"}).to_string().into()))
                .await
                .is_err()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    });

    let mut bounded = limits();
    bounded.io_timeout = Duration::from_millis(10);
    let transport =
        DeepgramWebSocketTransport::new(DeepgramWebSocketTransportConfig { limits: bounded })
            .expect("fixture operation");
    let mut connection = transport
        .connect(deepgram_aura_request(address))
        .await
        .expect("fixture operation");
    assert_eq!(
        tokio::time::timeout(Duration::from_millis(250), connection.receive())
            .await
            .expect("fixture has a strict wall-clock budget"),
        Some(Err(TransportError::Timeout))
    );
    let _ignored = connection.close().await;
    tokio::time::timeout(Duration::from_millis(250), server)
        .await
        .expect("fixture server has a strict wall-clock budget")
        .expect("fixture operation");
}

#[tokio::test]
async fn inworld_waits_for_context_then_preserves_async_provider_visemes() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("fixture operation");
    let address = listener.local_addr().expect("fixture operation");
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("fixture operation");
        let mut socket = accept_hdr_async(stream, |request: &Request, response: Response| {
            assert_eq!(request.uri().path(), "/tts/v1/voice:streamBidirectional");
            Ok(response)
        })
        .await
        .expect("fixture operation");
        let create = next_text(&mut socket).await;
        let context_id = create["contextId"].as_str().expect("context id").to_owned();
        assert_eq!(create["create"]["modelId"], "inworld-tts-2-flash");
        assert_eq!(create["create"]["audioConfig"]["audioEncoding"], "PCM");
        assert_eq!(create["create"]["timestampType"], "WORD");
        socket
            .send(Message::Text(
                json!({"result":{"contextId":context_id,"contextCreated":{},"status":{"code":0}}})
                    .to_string()
                    .into(),
            ))
            .await
            .expect("fixture operation");
        let send = next_text(&mut socket).await;
        assert!(send["send_text"]["flush_context"].is_object());
        socket.send(Message::Text(json!({"result":{"contextId":context_id,"audioChunk":{"audioContent":"AAEC","timestampInfo":{"wordAlignment":{"words":["Hi"],"wordStartTimeSeconds":[0.0],"wordEndTimeSeconds":[0.2],"phoneticDetails":[{"wordIndex":0,"phones":[{"phoneSymbol":"h","startTimeSeconds":0.0,"durationSeconds":0.1,"visemeSymbol":"cdgknstxyz"}]}]}},"status":{"code":0}}}}).to_string().into())).await.expect("fixture operation");
        socket
            .send(Message::Text(
                json!({"result":{"contextId":context_id,"flushCompleted":{}}})
                    .to_string()
                    .into(),
            ))
            .await
            .expect("fixture operation");
    });

    let request = open_request(
        HostedTtsProviderId::Inworld,
        format!("ws://{address}/tts/v1/voice:streamBidirectional"),
        "Authorization",
        Some("Basic"),
    );
    let transport =
        InworldWebSocketTransport::new(InworldWebSocketTransportConfig { limits: limits() })
            .expect("fixture operation");
    let mut connection = transport.connect(request).await.expect("fixture operation");
    connection
        .send(WireCommand::Inworld(InworldCommand::CreateContext {
            context_id: "inworld-context".into(),
            voice_id: "Dennis".into(),
            model_id: "inworld-tts-2-flash".into(),
            locale: "en-US".into(),
            output: AudioFormat::default(),
            request_alignment: true,
        }))
        .await
        .expect("fixture operation");
    connection
        .send(WireCommand::Inworld(InworldCommand::SendText {
            context_id: "inworld-context".into(),
            text: SensitiveString::new("Hi"),
            flush_context: true,
        }))
        .await
        .expect("fixture operation");
    assert!(matches!(
        connection
            .receive()
            .await
            .expect("fixture operation")
            .expect("fixture operation"),
        WireEvent::Audio(_)
    ));
    assert!(matches!(
        connection
            .receive()
            .await
            .expect("fixture operation")
            .expect("fixture operation"),
        WireEvent::Alignment(_)
    ));
    let visemes = connection
        .receive()
        .await
        .expect("fixture operation")
        .expect("fixture operation");
    let WireEvent::Viseme(visemes) = visemes else {
        panic!("provider viseme event")
    };
    assert_eq!(visemes[0].symbol, "cdgknstxyz");
    assert_eq!(
        connection
            .receive()
            .await
            .expect("fixture operation")
            .expect("fixture operation"),
        WireEvent::FlushComplete
    );
    server.await.expect("fixture operation");
}

#[tokio::test]
async fn inworld_streamed_clauses_flush_only_after_all_text() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("fixture operation");
    let address = listener.local_addr().expect("fixture operation");
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("fixture operation");
        let mut socket = accept_hdr_async(stream, |_: &Request, response: Response| Ok(response))
            .await
            .expect("fixture operation");
        let create = next_text(&mut socket).await;
        let context_id = create["contextId"].as_str().expect("context id").to_owned();
        socket
            .send(Message::Text(
                json!({"result":{"contextId":context_id,"contextCreated":{},"status":{"code":0}}})
                    .to_string()
                    .into(),
            ))
            .await
            .expect("fixture operation");

        for expected in ["The eastern lock is open. ", "The western gate is secure. "] {
            let send = next_text(&mut socket).await;
            assert_eq!(send["contextId"], context_id);
            assert_eq!(send["send_text"]["text"], expected);
            assert!(
                send["send_text"].get("flush_context").is_none(),
                "an intermediate semantic clause must not end the utterance"
            );
        }
        let flush = next_text(&mut socket).await;
        assert_eq!(flush["contextId"], context_id);
        assert!(flush["flush_context"].is_object());
        socket
            .send(Message::Text(
                json!({"result":{"contextId":context_id,"audioChunk":{"audioContent":"AAEC"},"status":{"code":0}}})
                    .to_string()
                    .into(),
            ))
            .await
            .expect("fixture operation");
        socket
            .send(Message::Text(
                json!({"result":{"contextId":context_id,"flushCompleted":{},"status":{"code":0}}})
                    .to_string()
                    .into(),
            ))
            .await
            .expect("fixture operation");
    });

    let provider = InworldProvider::new(
        Arc::new(
            InworldWebSocketTransport::new(InworldWebSocketTransportConfig { limits: limits() })
                .expect("fixture operation"),
        ),
        Arc::new(FixtureCredential),
        VoiceBindings::new([VoiceBinding {
            intent_id: "qualified-stock".into(),
            provider_id: HostedTtsProviderId::Inworld,
            voice_id: INWORLD_QUALIFIED_STOCK_VOICE_ID.into(),
            model_id: INWORLD_QUALIFIED_FLASH_MODEL_ID.into(),
            provider_options: BTreeMap::new(),
        }])
        .expect("fixture operation"),
        InworldConfig {
            endpoint: format!("ws://{address}/tts/v1/voice:streamBidirectional"),
        },
    );
    let mut session = provider
        .start_session(TtsSessionRequest {
            identity: SessionIdentity {
                session_id: "inworld-stream".into(),
                turn_id: "turn-1".into(),
                cancellation_generation: 0,
            },
            locale: "en-US".into(),
            voice_intent_id: "qualified-stock".into(),
            output: INWORLD_QUALIFIED_AUDIO_FORMAT,
            request_alignment: false,
            request_visemes: false,
            clause_policy: SemanticClausePolicy::default(),
        })
        .await
        .expect("fixture operation");
    let pushed = session
        .push_text("The eastern lock is open. The western gate is secure.")
        .await
        .expect("fixture operation");
    assert_eq!(pushed.clauses_submitted, 2);
    session.finish().await.expect("fixture operation");
    assert!(matches!(
        session.next_event().await,
        Some(Ok(TtsEvent::Audio(_)))
    ));
    assert_eq!(session.next_event().await, Some(Ok(TtsEvent::Completed)));
    server.await.expect("fixture operation");
}

#[tokio::test]
async fn inworld_rejects_outbound_and_inbound_cross_context_events() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("fixture operation");
    let address = listener.local_addr().expect("fixture operation");
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("fixture operation");
        let mut socket = accept_hdr_async(stream, |_: &Request, response: Response| Ok(response))
            .await
            .expect("fixture operation");
        let create = next_text(&mut socket).await;
        let context_id = create["contextId"].as_str().expect("context id").to_owned();
        socket
            .send(Message::Text(
                json!({"result":{"contextId":context_id,"contextCreated":{},"status":{"code":0}}})
                    .to_string()
                    .into(),
            ))
            .await
            .expect("fixture operation");
        let _send = next_text(&mut socket).await;
        socket
            .send(Message::Text(
                json!({"result":{"contextId":"prior-context","audioChunk":{"audioContent":"AAEC"},"status":{"code":0}}})
                    .to_string()
                    .into(),
            ))
            .await
            .expect("fixture operation");
    });

    let request = open_request(
        HostedTtsProviderId::Inworld,
        format!("ws://{address}/tts/v1/voice:streamBidirectional"),
        "Authorization",
        Some("Basic"),
    );
    let transport =
        InworldWebSocketTransport::new(InworldWebSocketTransportConfig { limits: limits() })
            .expect("fixture operation");
    let mut connection = transport.connect(request).await.expect("fixture operation");
    connection
        .send(WireCommand::Inworld(InworldCommand::CreateContext {
            context_id: "current-context".into(),
            voice_id: INWORLD_QUALIFIED_STOCK_VOICE_ID.into(),
            model_id: INWORLD_QUALIFIED_FLASH_MODEL_ID.into(),
            locale: "en-US".into(),
            output: INWORLD_QUALIFIED_AUDIO_FORMAT,
            request_alignment: false,
        }))
        .await
        .expect("fixture operation");
    assert_eq!(
        connection
            .send(WireCommand::Inworld(InworldCommand::SendText {
                context_id: "wrong-context".into(),
                text: SensitiveString::new("wrong"),
                flush_context: false,
            }))
            .await,
        Err(TransportError::ProtocolStage("inworld_context_mismatch"))
    );
    connection
        .send(WireCommand::Inworld(InworldCommand::SendText {
            context_id: "current-context".into(),
            text: SensitiveString::new("correct"),
            flush_context: true,
        }))
        .await
        .expect("fixture operation");
    assert_eq!(
        connection.receive().await.expect("fixture operation"),
        Err(TransportError::ProtocolStage("inworld_context_mismatch"))
    );
    let _ignored = connection.close().await;
    server.await.expect("fixture operation");
}

#[tokio::test]
async fn inworld_rejects_a_cross_context_creation_acknowledgement() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("fixture operation");
    let address = listener.local_addr().expect("fixture operation");
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("fixture operation");
        let mut socket = accept_hdr_async(stream, |_: &Request, response: Response| Ok(response))
            .await
            .expect("fixture operation");
        let _create = next_text(&mut socket).await;
        socket
            .send(Message::Text(
                json!({"result":{"contextId":"prior-context","contextCreated":{},"status":{"code":0}}})
                    .to_string()
                    .into(),
            ))
            .await
            .expect("fixture operation");
    });

    let request = open_request(
        HostedTtsProviderId::Inworld,
        format!("ws://{address}/tts/v1/voice:streamBidirectional"),
        "Authorization",
        Some("Basic"),
    );
    let transport =
        InworldWebSocketTransport::new(InworldWebSocketTransportConfig { limits: limits() })
            .expect("fixture operation");
    let mut connection = transport.connect(request).await.expect("fixture operation");
    assert_eq!(
        connection
            .send(WireCommand::Inworld(InworldCommand::CreateContext {
                context_id: "current-context".into(),
                voice_id: INWORLD_QUALIFIED_STOCK_VOICE_ID.into(),
                model_id: INWORLD_QUALIFIED_FLASH_MODEL_ID.into(),
                locale: "en-US".into(),
                output: INWORLD_QUALIFIED_AUDIO_FORMAT,
                request_alignment: false,
            }))
            .await,
        Err(TransportError::ProtocolStage("inworld_context_mismatch"))
    );
    let _ignored = connection.close().await;
    server.await.expect("fixture operation");
}

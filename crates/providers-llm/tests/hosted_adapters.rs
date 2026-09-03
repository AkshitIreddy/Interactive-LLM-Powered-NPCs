use std::{collections::BTreeMap, sync::Arc, time::Duration};

use futures_util::StreamExt;
use npc_providers_llm::{
    AdapterConfig, AnthropicMessages, CapabilitySupport, ChatMessage, ChatRole, CohereChat,
    ErrorKind, GeminiGenerateContent, GroqChatCompletions, HostedLanguageModel, LlmEvent,
    LlmRequest, MemorySecretProvider, ModelCapabilities, NvidiaNimChat, OpenAiResponses,
    RuntimeBridge, RuntimeBridgeConfig, SecretBytes, SecretReference, ToolDefinition,
};
use npc_runtime_core::{
    CharacterIdentity, GenerationRequest, LanguageModelProvider as RuntimeLanguageModelProvider,
    MemoryContext, TurnIdentity,
};
use serde_json::json;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::oneshot,
};
use tokio_util::sync::CancellationToken;
use url::Url;

const CANARY: &str = "fixture-credential-redaction-canary";

struct FixtureServer {
    base: Url,
    request: oneshot::Receiver<Vec<u8>>,
}

async fn chunked_server(
    status: &str,
    content_type: &str,
    chunks: Vec<Vec<u8>>,
    delay_after_each_chunk: Duration,
) -> FixtureServer {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind fixture");
    let address = listener.local_addr().expect("fixture address");
    let (request_tx, request_rx) = oneshot::channel();
    let status = status.to_owned();
    let content_type = content_type.to_owned();
    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("accept fixture request");
        let request = read_request(&mut socket).await;
        let _ = request_tx.send(request);
        let headers = format!(
            "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nTransfer-Encoding: chunked\r\nConnection: close\r\nx-request-id: req_fixture\r\n\r\n"
        );
        socket
            .write_all(headers.as_bytes())
            .await
            .expect("write headers");
        socket.flush().await.expect("flush headers");
        for chunk in chunks {
            let prefix = format!("{:x}\r\n", chunk.len());
            socket
                .write_all(prefix.as_bytes())
                .await
                .expect("write chunk size");
            socket.write_all(&chunk).await.expect("write chunk");
            socket.write_all(b"\r\n").await.expect("write chunk suffix");
            socket.flush().await.expect("flush chunk");
            if !delay_after_each_chunk.is_zero() {
                tokio::time::sleep(delay_after_each_chunk).await;
            }
        }
        socket
            .write_all(b"0\r\n\r\n")
            .await
            .expect("finish response");
    });
    FixtureServer {
        base: Url::parse(&format!("http://{address}/")).expect("fixture URL"),
        request: request_rx,
    }
}

async fn read_request(socket: &mut tokio::net::TcpStream) -> Vec<u8> {
    let mut request = Vec::new();
    let mut buffer = [0_u8; 4_096];
    let mut expected = None;
    loop {
        let read = socket.read(&mut buffer).await.expect("read request");
        if read == 0 {
            break;
        }
        request.extend_from_slice(&buffer[..read]);
        if expected.is_none() {
            if let Some(header_end) = find_bytes(&request, b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&request[..header_end]);
                let content_length = headers
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().ok())
                            .flatten()
                    })
                    .unwrap_or(0);
                expected = Some(header_end + 4 + content_length);
            }
        }
        if expected.is_some_and(|expected| request.len() >= expected) {
            break;
        }
    }
    request
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn split_bytes(value: &str, sizes: &[usize]) -> Vec<Vec<u8>> {
    let bytes = value.as_bytes();
    let mut chunks = Vec::new();
    let mut offset = 0;
    let mut size_index = 0;
    while offset < bytes.len() {
        let size = sizes[size_index % sizes.len()].max(1);
        let end = (offset + size).min(bytes.len());
        chunks.push(bytes[offset..end].to_vec());
        offset = end;
        size_index += 1;
    }
    chunks
}

fn config(base: Url, path: &str) -> AdapterConfig {
    let vault = MemorySecretProvider::default();
    let reference = SecretReference::new("providers/fixture").expect("reference");
    vault
        .insert(
            reference.clone(),
            SecretBytes::new(CANARY.as_bytes().to_vec()).expect("secret"),
        )
        .expect("insert secret");
    let mut base_url = base;
    base_url.set_path(path);
    AdapterConfig {
        base_url,
        credential: reference,
        secrets: Arc::new(vault),
        request_timeout: Duration::from_secs(2),
        allow_insecure_loopback: true,
    }
}

fn request() -> LlmRequest {
    LlmRequest {
        model: "fixture-model".into(),
        messages: vec![ChatMessage {
            role: ChatRole::User,
            content: "Hello".into(),
            name: None,
            tool_call_id: None,
        }],
        maximum_output_tokens: 128,
        temperature: Some(0.4),
        tools: Vec::new(),
        response_json_schema: None,
    }
}

#[tokio::test]
async fn openai_responses_normalizes_split_text_tool_usage_and_finish() {
    let sse = concat!(
        "event: response.created\ndata: {\"type\":\"response.created\",\"response\":{\"id\":\"resp-1\"}}\n\n",
        "event: response.output_item.added\ndata: {\"type\":\"response.output_item.added\",\"output_index\":1,\"item\":{\"type\":\"function_call\",\"call_id\":\"call-1\",\"name\":\"lookup\"}}\n\n",
        "event: response.function_call_arguments.delta\ndata: {\"type\":\"response.function_call_arguments.delta\",\"output_index\":1,\"delta\":\"{\\\"id\\\":\"}\n\n",
        "event: response.function_call_arguments.delta\ndata: {\"type\":\"response.function_call_arguments.delta\",\"output_index\":1,\"delta\":\"7}\"}\n\n",
        "event: response.output_item.done\ndata: {\"type\":\"response.output_item.done\",\"output_index\":1,\"item\":{\"type\":\"function_call\",\"arguments\":\"{\\\"id\\\":7}\"}}\n\n",
        "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"content_index\":0,\"delta\":\"Hello there.\"}\n\n",
        "event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":12,\"output_tokens\":4,\"total_tokens\":16}}}\n\n",
    );
    let fixture = chunked_server(
        "200 OK",
        "text/event-stream",
        split_bytes(sse, &[1, 2, 5, 13]),
        Duration::ZERO,
    )
    .await;
    let adapter = OpenAiResponses::new(config(fixture.base, "/v1")).expect("adapter");
    let mut llm_request = request();
    llm_request.tools.push(ToolDefinition {
        name: "lookup".into(),
        description: "Look up an NPC".into(),
        input_schema: json!({"type":"object","properties":{"id":{"type":"integer"}},"required":["id"]}),
    });
    let events = adapter
        .stream(llm_request, CancellationToken::new())
        .await
        .expect("open stream")
        .collect::<Vec<_>>()
        .await;
    assert!(events
        .iter()
        .any(|event| matches!(event, LlmEvent::TextDelta { text, .. } if text == "Hello there.")));
    assert!(events.iter().any(|event| matches!(event, LlmEvent::ToolCallCompleted { arguments, .. } if arguments == &json!({"id":7}))));
    assert!(events
        .iter()
        .any(|event| matches!(event, LlmEvent::Usage { usage } if usage.total_tokens == 16)));
    assert!(matches!(events.last(), Some(LlmEvent::Finished { .. })));

    let received =
        String::from_utf8(fixture.request.await.expect("captured request")).expect("UTF-8 request");
    assert!(received.starts_with("POST /v1/responses HTTP/1.1"));
    assert!(received
        .to_ascii_lowercase()
        .contains(&format!("authorization: bearer {CANARY}")));
    let body = received.split("\r\n\r\n").nth(1).expect("request body");
    assert_eq!(
        body.matches(CANARY).count(),
        0,
        "secret belongs only in the auth header"
    );
    assert!(body.contains("\"store\":false"));
}

#[tokio::test]
async fn anthropic_messages_normalizes_incremental_text_and_usage() {
    let sse = concat!(
        "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg-1\",\"usage\":{\"input_tokens\":9,\"output_tokens\":1}}}\n\n",
        "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hi\"}}\n\n",
        "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":3}}\n\n",
        "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
    );
    let fixture = chunked_server(
        "200 OK",
        "text/event-stream; charset=utf-8",
        split_bytes(sse, &[3, 7, 11]),
        Duration::ZERO,
    )
    .await;
    let adapter = AnthropicMessages::new(config(fixture.base, "/v1")).expect("adapter");
    let events = adapter
        .stream(request(), CancellationToken::new())
        .await
        .expect("stream")
        .collect::<Vec<_>>()
        .await;
    assert!(events
        .iter()
        .any(|event| matches!(event, LlmEvent::TextDelta { text, .. } if text == "Hi")));
    assert!(matches!(events.last(), Some(LlmEvent::Finished { .. })));
    let received = String::from_utf8(fixture.request.await.expect("request")).expect("UTF-8");
    assert!(received.starts_with("POST /v1/messages HTTP/1.1"));
    assert!(received
        .to_ascii_lowercase()
        .contains("anthropic-version: 2023-06-01"));
    assert!(received
        .to_ascii_lowercase()
        .contains(&format!("x-api-key: {CANARY}")));
}

#[tokio::test]
async fn gemini_generate_content_normalizes_function_and_safety_finish() {
    let sse = concat!(
        "data: {\"candidates\":[{\"index\":0,\"content\":{\"parts\":[{\"text\":\"Wait.\"}]}}]}\n\n",
        "data: {\"candidates\":[{\"index\":0,\"content\":{\"parts\":[{\"functionCall\":{\"id\":\"g-1\",\"name\":\"pause\",\"args\":{\"seconds\":1}}}]},\"finishReason\":\"SAFETY\"}],\"usageMetadata\":{\"promptTokenCount\":5,\"candidatesTokenCount\":2,\"totalTokenCount\":7}}\n\n",
    );
    let fixture = chunked_server(
        "200 OK",
        "text/event-stream",
        split_bytes(sse, &[4, 1, 9]),
        Duration::ZERO,
    )
    .await;
    let adapter = GeminiGenerateContent::new(config(fixture.base, "/v1beta")).expect("adapter");
    let events = adapter
        .stream(request(), CancellationToken::new())
        .await
        .expect("stream")
        .collect::<Vec<_>>()
        .await;
    assert!(events
        .iter()
        .any(|event| matches!(event, LlmEvent::ToolCallCompleted { name, .. } if name == "pause")));
    assert!(matches!(
        events.last(),
        Some(LlmEvent::Finished {
            reason: npc_providers_llm::FinishReason::Safety,
            ..
        })
    ));
    let received = String::from_utf8(fixture.request.await.expect("request")).expect("UTF-8");
    assert!(received
        .starts_with("POST /v1beta/models/fixture-model:streamGenerateContent?alt=sse HTTP/1.1"));
    assert!(received
        .to_ascii_lowercase()
        .contains(&format!("x-goog-api-key: {CANARY}")));
    assert!(received.contains("\"store\":false"));
}

#[tokio::test]
async fn groq_chat_completions_handles_done_and_usage_chunk() {
    let sse = concat!(
        "data: {\"id\":\"chat-1\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Ready\"},\"finish_reason\":null}]}\n\n",
        "data: {\"id\":\"chat-1\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":3,\"completion_tokens\":1,\"total_tokens\":4}}\n\n",
        "data: [DONE]\n\n",
    );
    let fixture = chunked_server(
        "200 OK",
        "text/event-stream",
        split_bytes(sse, &[2, 8, 17]),
        Duration::ZERO,
    )
    .await;
    let adapter = GroqChatCompletions::new(config(fixture.base, "/openai/v1")).expect("adapter");
    let events = adapter
        .stream(request(), CancellationToken::new())
        .await
        .expect("stream")
        .collect::<Vec<_>>()
        .await;
    assert!(events
        .iter()
        .any(|event| matches!(event, LlmEvent::TextDelta { text, .. } if text == "Ready")));
    assert!(matches!(events.last(), Some(LlmEvent::Finished { .. })));
    let received = String::from_utf8(fixture.request.await.expect("request")).expect("UTF-8");
    assert!(received.starts_with("POST /openai/v1/chat/completions HTTP/1.1"));
}

#[tokio::test]
async fn cohere_v2_chat_normalizes_split_text_tool_usage_and_finish() {
    let sse = concat!(
        "event: message-start\ndata: {\"type\":\"message-start\",\"id\":\"cohere-1\",\"delta\":{\"message\":{\"role\":\"assistant\"}}}\n\n",
        "event: content-start\ndata: {\"type\":\"content-start\",\"index\":0,\"delta\":{\"message\":{\"content\":{\"type\":\"thinking\",\"thinking\":\"\"}}}}\n\n",
        "event: content-delta\ndata: {\"type\":\"content-delta\",\"index\":0,\"delta\":{\"message\":{\"content\":{\"thinking\":\"private provider reasoning must not become dialogue\"}}}}\n\n",
        "event: content-end\ndata: {\"type\":\"content-end\",\"index\":0}\n\n",
        "event: content-delta\ndata: {\"type\":\"content-delta\",\"index\":0,\"delta\":{\"message\":{\"content\":{\"text\":\"Checking.\"}}}}\n\n",
        "event: tool-call-start\ndata: {\"type\":\"tool-call-start\",\"index\":1,\"delta\":{\"message\":{\"tool_calls\":{\"id\":\"lookup-1\",\"type\":\"function\",\"function\":{\"name\":\"lookup\",\"arguments\":\"\"}}}}}\n\n",
        "event: tool-call-delta\ndata: {\"type\":\"tool-call-delta\",\"index\":1,\"delta\":{\"message\":{\"tool_calls\":{\"function\":{\"arguments\":\"{\\\"id\\\":7}\"}}}}}\n\n",
        "event: tool-call-end\ndata: {\"type\":\"tool-call-end\",\"index\":1}\n\n",
        "event: message-end\ndata: {\"type\":\"message-end\",\"delta\":{\"finish_reason\":\"TOOL_CALL\",\"usage\":{\"tokens\":{\"input_tokens\":11,\"output_tokens\":5}}}}\n\n",
    );
    let fixture = chunked_server(
        "200 OK",
        "text/event-stream",
        split_bytes(sse, &[1, 3, 8, 21]),
        Duration::ZERO,
    )
    .await;
    let adapter = CohereChat::new(config(fixture.base, "/")).expect("adapter");
    assert!(adapter.capabilities().privacy.application_stateless);
    assert!(!adapter.capabilities().privacy.request_disables_storage);
    assert!(adapter.capabilities().privacy.provider_may_retain_data);
    let mut llm_request = request();
    llm_request.tools.push(ToolDefinition {
        name: "lookup".into(),
        description: "Look up an NPC".into(),
        input_schema: json!({"type":"object","properties":{"id":{"type":"integer"}},"required":["id"]}),
    });
    let events = adapter
        .stream(llm_request, CancellationToken::new())
        .await
        .expect("stream")
        .collect::<Vec<_>>()
        .await;
    assert!(events
        .iter()
        .any(|event| matches!(event, LlmEvent::TextDelta { text, .. } if text == "Checking.")));
    assert!(!events.iter().any(|event| matches!(
        event,
        LlmEvent::TextDelta { text, .. } if text.contains("private provider reasoning")
    )));
    assert!(events.iter().any(|event| matches!(event, LlmEvent::ToolCallCompleted { name, arguments, .. } if name == "lookup" && arguments == &json!({"id":7}))));
    assert!(events
        .iter()
        .any(|event| matches!(event, LlmEvent::Usage { usage } if usage.total_tokens == 16)));
    assert!(matches!(
        events.last(),
        Some(LlmEvent::Finished {
            reason: npc_providers_llm::FinishReason::ToolUse,
            ..
        })
    ));
    let received = String::from_utf8(fixture.request.await.expect("request")).expect("UTF-8");
    assert!(received.starts_with("POST /v2/chat HTTP/1.1"));
    assert!(received
        .to_ascii_lowercase()
        .contains(&format!("authorization: bearer {CANARY}")));
    assert!(received
        .to_ascii_lowercase()
        .contains("x-client-name: interactive-npcs"));
    let body = received.split("\r\n\r\n").nth(1).expect("request body");
    assert!(!body.contains("\"store\""));
    assert!(body.contains("\"tools\""));
}

#[tokio::test]
async fn cohere_model_listing_uses_v1_chat_filter_and_marks_deprecated_models() {
    let fixture = chunked_server(
        "200 OK",
        "application/json",
        vec![br#"{"models":[{"name":"command-current","is_deprecated":false,"endpoints":["chat"]},{"name":"command-old","is_deprecated":true,"endpoints":["chat"]},{"name":"embed-only","is_deprecated":false,"endpoints":["embed"]}]}"#.to_vec()],
        Duration::ZERO,
    )
    .await;
    let adapter = CohereChat::new(config(fixture.base, "/")).expect("adapter");
    let models = adapter
        .list_models(CancellationToken::new())
        .await
        .expect("models");
    assert_eq!(models.len(), 3);
    assert!(models
        .iter()
        .find(|model| model.id == "command-current")
        .is_some_and(|model| model.supports_generation));
    assert!(models
        .iter()
        .filter(|model| model.id != "command-current")
        .all(|model| !model.supports_generation));
    let received = String::from_utf8(fixture.request.await.expect("request")).expect("UTF-8");
    assert!(received.starts_with("GET /v1/models?page_size=1000&endpoint=chat HTTP/1.1"));
}

#[tokio::test]
async fn cohere_rejects_tools_with_json_schema_before_network_egress() {
    let fixture = chunked_server(
        "200 OK",
        "text/event-stream",
        vec![b"event: message-end\ndata: {\"type\":\"message-end\",\"delta\":{\"finish_reason\":\"COMPLETE\"}}\n\n".to_vec()],
        Duration::ZERO,
    )
    .await;
    let adapter = CohereChat::new(config(fixture.base, "/")).expect("adapter");
    let mut invalid = request();
    invalid.tools.push(ToolDefinition {
        name: "lookup".into(),
        description: "Lookup".into(),
        input_schema: json!({"type":"object"}),
    });
    invalid.response_json_schema = Some(json!({"type":"object"}));
    let error = match adapter.stream(invalid, CancellationToken::new()).await {
        Ok(_) => panic!("invalid provider combination must be rejected"),
        Err(error) => error,
    };
    assert_eq!(error.kind, ErrorKind::InvalidRequest);
}

#[tokio::test]
async fn cohere_invalid_token_status_is_sanitized_as_authentication() {
    let fixture = chunked_server(
        "498 Invalid Token",
        "application/json",
        vec![format!(r#"{{"message":"reflected {CANARY}"}}"#).into_bytes()],
        Duration::ZERO,
    )
    .await;
    let adapter = CohereChat::new(config(fixture.base, "/")).expect("adapter");
    let error = match adapter.stream(request(), CancellationToken::new()).await {
        Ok(_) => panic!("invalid token must fail before opening a stream"),
        Err(error) => error,
    };
    assert_eq!(error.kind, ErrorKind::Authentication);
    assert!(!format!("{error:?} {error}").contains(CANARY));
}

fn nvidia_request(model: &str) -> LlmRequest {
    let mut value = request();
    value.model = model.into();
    value
}

fn verified_nvidia_tools() -> BTreeMap<String, ModelCapabilities> {
    BTreeMap::from([(
        "meta/llama-fixture-instruct".into(),
        ModelCapabilities {
            streaming: CapabilitySupport::Supported,
            tool_calls: CapabilitySupport::Supported,
            json_schema: CapabilitySupport::Supported,
            reasoning: CapabilitySupport::Unknown,
        },
    )])
}

#[tokio::test]
async fn nvidia_nim_normalizes_200_sse_and_preserves_exact_model_id() {
    let sse = concat!(
        "data: {\"id\":\"nim-1\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Ready.\"},\"finish_reason\":null}]}\n\n",
        "data: {\"id\":\"nim-1\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call-1\",\"function\":{\"name\":\"lookup\",\"arguments\":\"{\\\"id\\\":\"}}]},\"finish_reason\":null}]}\n\n",
        "data: {\"id\":\"nim-1\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"7}\"}}]},\"finish_reason\":\"tool_calls\"}],\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":5,\"total_tokens\":15}}\n\n",
        "data: [DONE]\n\n",
    );
    let fixture = chunked_server(
        "200 OK",
        "text/event-stream",
        split_bytes(sse, &[1, 2, 7, 19]),
        Duration::ZERO,
    )
    .await;
    let adapter = NvidiaNimChat::with_verified_model_capabilities(
        config(fixture.base, "/"),
        verified_nvidia_tools(),
    )
    .expect("adapter");
    assert_eq!(adapter.capabilities().provider_id, "nvidia-nim");
    assert!(adapter.capabilities().privacy.application_stateless);
    assert!(!adapter.capabilities().privacy.request_disables_storage);
    assert!(adapter.capabilities().privacy.provider_may_retain_data);
    let mut llm_request = nvidia_request("meta/llama-fixture-instruct");
    llm_request.tools.push(ToolDefinition {
        name: "lookup".into(),
        description: "Look up an NPC".into(),
        input_schema: json!({"type":"object","properties":{"id":{"type":"integer"}},"required":["id"]}),
    });
    let events = adapter
        .stream(llm_request, CancellationToken::new())
        .await
        .expect("stream")
        .collect::<Vec<_>>()
        .await;
    assert!(events
        .iter()
        .any(|event| matches!(event, LlmEvent::TextDelta { text, .. } if text == "Ready.")));
    assert!(events.iter().any(|event| matches!(event, LlmEvent::ToolCallCompleted { arguments, .. } if arguments == &json!({"id":7}))));
    assert!(events
        .iter()
        .any(|event| matches!(event, LlmEvent::Usage { usage } if usage.total_tokens == 15)));
    assert!(matches!(
        events.last(),
        Some(LlmEvent::Finished {
            reason: npc_providers_llm::FinishReason::ToolUse,
            ..
        })
    ));
    let received = String::from_utf8(fixture.request.await.expect("request")).expect("UTF-8");
    assert!(received.starts_with("POST /v1/chat/completions HTTP/1.1"));
    assert!(received
        .to_ascii_lowercase()
        .contains(&format!("authorization: bearer {CANARY}")));
    let body = received.split("\r\n\r\n").nth(1).expect("body");
    assert!(body.contains("\"model\":\"meta/llama-fixture-instruct\""));
    assert!(!body.contains("\"store\""));
}

#[tokio::test]
async fn nvidia_nemotron_three_disables_reasoning_for_low_latency_dialogue() {
    let sse = concat!(
        "data: {\"id\":\"nim-fast\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Ready.\"},\"finish_reason\":\"stop\"}]}\n\n",
        "data: [DONE]\n\n",
    );
    let fixture = chunked_server(
        "200 OK",
        "text/event-stream",
        vec![sse.as_bytes().to_vec()],
        Duration::ZERO,
    )
    .await;
    let adapter = NvidiaNimChat::new(config(fixture.base, "/")).expect("adapter");
    let _events = adapter
        .stream(
            nvidia_request("nvidia/nemotron-3.5-lightning-30b-a3b"),
            CancellationToken::new(),
        )
        .await
        .expect("stream")
        .collect::<Vec<_>>()
        .await;
    let received = String::from_utf8(fixture.request.await.expect("request")).expect("UTF-8");
    let body: serde_json::Value =
        serde_json::from_str(received.split("\r\n\r\n").nth(1).expect("body"))
            .expect("JSON request body");
    assert_eq!(body["chat_template_kwargs"]["enable_thinking"], false);
}

#[tokio::test]
async fn nvidia_nim_returns_typed_polling_state_for_202() {
    let fixture = chunked_server(
        "202 Accepted",
        "application/json",
        vec![format!(r#"{{"requestId":"poll-123","message":"{CANARY}"}}"#).into_bytes()],
        Duration::ZERO,
    )
    .await;
    let adapter = NvidiaNimChat::new(config(fixture.base, "/")).expect("adapter");
    let error = match adapter
        .stream(
            nvidia_request("nvidia/async-fixture"),
            CancellationToken::new(),
        )
        .await
    {
        Ok(_) => panic!("202 must not be treated as an open SSE stream"),
        Err(error) => error,
    };
    assert_eq!(error.kind, ErrorKind::PollingRequired);
    assert_eq!(error.http_status, Some(202));
    assert_eq!(error.request_id.as_deref(), Some("poll-123"));
    assert!(!format!("{error:?} {error}").contains(CANARY));
}

#[tokio::test]
async fn nvidia_nim_classifies_401_and_429_without_reflecting_secrets() {
    for (status, expected) in [
        ("401 Unauthorized", ErrorKind::Authentication),
        ("429 Too Many Requests", ErrorKind::RateLimited),
    ] {
        let fixture = chunked_server(
            status,
            "application/json",
            vec![format!(r#"{{"error":"reflected {CANARY}"}}"#).into_bytes()],
            Duration::ZERO,
        )
        .await;
        let adapter = NvidiaNimChat::new(config(fixture.base, "/")).expect("adapter");
        let error = match adapter
            .stream(
                nvidia_request("nvidia/error-fixture"),
                CancellationToken::new(),
            )
            .await
        {
            Ok(_) => panic!("HTTP error must fail before opening a stream"),
            Err(error) => error,
        };
        assert_eq!(error.kind, expected);
        assert!(!format!("{error:?} {error}").contains(CANARY));
    }
}

#[tokio::test]
async fn nvidia_nim_malformed_sse_is_a_protocol_error() {
    let fixture = chunked_server(
        "200 OK",
        "text/event-stream",
        vec![b"data: {malformed\n\n".to_vec()],
        Duration::ZERO,
    )
    .await;
    let adapter = NvidiaNimChat::new(config(fixture.base, "/")).expect("adapter");
    let events = adapter
        .stream(
            nvidia_request("nvidia/malformed-fixture"),
            CancellationToken::new(),
        )
        .await
        .expect("stream")
        .collect::<Vec<_>>()
        .await;
    assert!(
        matches!(events.as_slice(), [LlmEvent::Error { error }] if error.kind == ErrorKind::Protocol)
    );
}

#[tokio::test]
async fn nvidia_nim_cancellation_interrupts_stalled_sse() {
    let first = "data: {\"id\":\"nim-wait\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"First\"},\"finish_reason\":null}]}\n\n";
    let fixture = chunked_server(
        "200 OK",
        "text/event-stream",
        vec![first.as_bytes().to_vec(), b": late\n\n".to_vec()],
        Duration::from_secs(5),
    )
    .await;
    let adapter = NvidiaNimChat::new(config(fixture.base, "/")).expect("adapter");
    let cancellation = CancellationToken::new();
    let mut stream = adapter
        .stream(
            nvidia_request("nvidia/cancel-fixture"),
            cancellation.clone(),
        )
        .await
        .expect("stream");
    assert!(matches!(
        stream.next().await,
        Some(LlmEvent::Started { .. })
    ));
    assert!(matches!(
        stream.next().await,
        Some(LlmEvent::TextDelta { .. })
    ));
    cancellation.cancel();
    let event = tokio::time::timeout(Duration::from_millis(250), stream.next())
        .await
        .expect("prompt cancellation")
        .expect("cancel event");
    assert!(matches!(event, LlmEvent::Error { error } if error.kind == ErrorKind::Cancelled));
}

#[tokio::test]
async fn nvidia_nim_discovery_preserves_ids_and_defaults_features_conservatively() {
    let fixture = chunked_server(
        "200 OK",
        "application/json",
        vec![br#"{"object":"list","data":[{"id":"meta/llama-fixture-instruct","owned_by":"meta"},{"id":"nvidia/nemotron-fixture","owned_by":"nvidia"}]}"#.to_vec()],
        Duration::ZERO,
    )
    .await;
    let adapter = NvidiaNimChat::with_verified_model_capabilities(
        config(fixture.base, "/"),
        verified_nvidia_tools(),
    )
    .expect("adapter");
    let models = adapter
        .list_models(CancellationToken::new())
        .await
        .expect("models");
    assert_eq!(
        models
            .iter()
            .map(|model| model.id.as_str())
            .collect::<Vec<_>>(),
        vec!["meta/llama-fixture-instruct", "nvidia/nemotron-fixture"]
    );
    let verified = models
        .iter()
        .find(|model| model.id == "meta/llama-fixture-instruct")
        .expect("verified model");
    assert_eq!(
        verified.capabilities.tool_calls,
        CapabilitySupport::Supported
    );
    let unknown = models
        .iter()
        .find(|model| model.id == "nvidia/nemotron-fixture")
        .expect("unknown model");
    assert_eq!(unknown.capabilities.streaming, CapabilitySupport::Supported);
    assert_eq!(unknown.capabilities.tool_calls, CapabilitySupport::Unknown);
    assert_eq!(unknown.capabilities.json_schema, CapabilitySupport::Unknown);
    assert_eq!(unknown.capabilities.reasoning, CapabilitySupport::Unknown);
    let received = String::from_utf8(fixture.request.await.expect("request")).expect("UTF-8");
    assert!(received.starts_with("GET /v1/models HTTP/1.1"));
}

#[tokio::test]
async fn nvidia_nim_model_discovery_reuses_the_bounded_cache() {
    let fixture = chunked_server(
        "200 OK",
        "application/json",
        vec![
            br#"{"object":"list","data":[{"id":"nvidia/nemotron-fixture","owned_by":"nvidia"}]}"#
                .to_vec(),
        ],
        Duration::ZERO,
    )
    .await;
    let adapter = NvidiaNimChat::with_verified_model_capabilities_and_cache_ttl(
        config(fixture.base, "/"),
        BTreeMap::new(),
        Duration::from_secs(5),
    )
    .expect("adapter");

    let first = adapter
        .list_models(CancellationToken::new())
        .await
        .expect("first discovery");
    let second = tokio::time::timeout(
        Duration::from_millis(100),
        adapter.list_models(CancellationToken::new()),
    )
    .await
    .expect("cache lookup is immediate")
    .expect("cached models");

    assert_eq!(first, second);
    assert_eq!(second[0].id, "nvidia/nemotron-fixture");
    let received = String::from_utf8(fixture.request.await.expect("request")).expect("UTF-8");
    assert!(received.starts_with("GET /v1/models HTTP/1.1"));
}

#[test]
fn nvidia_nim_rejects_unbounded_model_cache_ttls() {
    let listener_free_base = Url::parse("http://127.0.0.1:9/").expect("fixture URL");
    assert!(
        NvidiaNimChat::with_verified_model_capabilities_and_cache_ttl(
            config(listener_free_base.clone(), "/"),
            BTreeMap::new(),
            Duration::ZERO,
        )
        .is_err()
    );
    assert!(
        NvidiaNimChat::with_verified_model_capabilities_and_cache_ttl(
            config(listener_free_base, "/"),
            BTreeMap::new(),
            Duration::from_secs(24 * 60 * 60 + 1),
        )
        .is_err()
    );
}

#[test]
fn nvidia_nim_rejects_arbitrary_https_hosts_and_unverified_model_features() {
    let vault = MemorySecretProvider::default();
    let reference = SecretReference::new("providers/nvidia").expect("reference");
    vault
        .insert(
            reference.clone(),
            SecretBytes::new(CANARY.as_bytes().to_vec()).expect("secret"),
        )
        .expect("insert");
    let config = AdapterConfig {
        base_url: Url::parse("https://example.com/").expect("URL"),
        credential: reference,
        secrets: Arc::new(vault),
        request_timeout: Duration::from_secs(2),
        allow_insecure_loopback: false,
    };
    assert!(NvidiaNimChat::new(config).is_err());
}

#[tokio::test]
async fn cancellation_interrupts_an_open_stream() {
    let first = "event: response.created\ndata: {\"type\":\"response.created\",\"response\":{\"id\":\"resp-wait\"}}\n\n";
    let fixture = chunked_server(
        "200 OK",
        "text/event-stream",
        vec![first.as_bytes().to_vec(), b": late keepalive\n\n".to_vec()],
        Duration::from_secs(5),
    )
    .await;
    let adapter = OpenAiResponses::new(config(fixture.base, "/v1")).expect("adapter");
    let cancellation = CancellationToken::new();
    let mut stream = adapter
        .stream(request(), cancellation.clone())
        .await
        .expect("stream");
    assert!(matches!(
        stream.next().await,
        Some(LlmEvent::Started { .. })
    ));
    cancellation.cancel();
    let event = tokio::time::timeout(Duration::from_millis(250), stream.next())
        .await
        .expect("cancellation must be prompt")
        .expect("cancel event");
    assert!(matches!(event, LlmEvent::Error { error } if error.kind == ErrorKind::Cancelled));
}

#[tokio::test]
async fn total_deadline_interrupts_a_stalled_stream() {
    let first = "event: response.created\ndata: {\"type\":\"response.created\",\"response\":{\"id\":\"resp-timeout\"}}\n\n";
    let fixture = chunked_server(
        "200 OK",
        "text/event-stream",
        vec![first.as_bytes().to_vec(), b": late keepalive\n\n".to_vec()],
        Duration::from_secs(5),
    )
    .await;
    let mut adapter_config = config(fixture.base, "/v1");
    adapter_config.request_timeout = Duration::from_millis(150);
    let adapter = OpenAiResponses::new(adapter_config).expect("adapter");
    let mut stream = adapter
        .stream(request(), CancellationToken::new())
        .await
        .expect("stream");
    assert!(matches!(
        stream.next().await,
        Some(LlmEvent::Started { .. })
    ));
    let event = tokio::time::timeout(Duration::from_millis(500), stream.next())
        .await
        .expect("deadline must be enforced")
        .expect("timeout event");
    assert!(matches!(event, LlmEvent::Error { error } if error.kind == ErrorKind::Timeout));
}

#[tokio::test]
async fn malformed_stream_json_becomes_sanitized_protocol_error() {
    let fixture = chunked_server(
        "200 OK",
        "text/event-stream",
        vec![b"event: response.output_text.delta\ndata: {not-json\n\n".to_vec()],
        Duration::ZERO,
    )
    .await;
    let adapter = OpenAiResponses::new(config(fixture.base, "/v1")).expect("adapter");
    let events = adapter
        .stream(request(), CancellationToken::new())
        .await
        .expect("stream")
        .collect::<Vec<_>>()
        .await;
    assert!(
        matches!(events.as_slice(), [LlmEvent::Error { error }] if error.kind == ErrorKind::Protocol && !error.message.contains("not-json"))
    );
}

#[tokio::test]
async fn reflected_http_error_and_debug_output_never_leak_secret() {
    let fixture = chunked_server(
        "401 Unauthorized",
        "application/json",
        vec![format!(r#"{{"error":"credential was {CANARY}"}}"#).into_bytes()],
        Duration::ZERO,
    )
    .await;
    let adapter = OpenAiResponses::new(config(fixture.base, "/v1")).expect("adapter");
    let debug = format!("{adapter:?}");
    assert!(!debug.contains(CANARY));
    let error = match adapter.stream(request(), CancellationToken::new()).await {
        Ok(_) => panic!("401 must fail before opening stream"),
        Err(error) => error,
    };
    let rendered = format!("{error:?} {error}");
    assert_eq!(error.kind, ErrorKind::Authentication);
    assert!(!rendered.contains(CANARY));
    assert_eq!(error.request_id.as_deref(), Some("req_fixture"));
}

#[tokio::test]
async fn model_listing_is_sorted_validated_and_deduplicated() {
    let fixture = chunked_server(
        "200 OK",
        "application/json",
        vec![br#"{"data":[{"id":"z-model","owned_by":"fixture"},{"id":"a-model"},{"id":"a-model"},{"id":"../invalid"}]}"#.to_vec()],
        Duration::ZERO,
    )
    .await;
    let adapter = OpenAiResponses::new(config(fixture.base, "/v1")).expect("adapter");
    let models = adapter
        .list_models(CancellationToken::new())
        .await
        .expect("models");
    assert_eq!(
        models
            .iter()
            .map(|model| model.id.as_str())
            .collect::<Vec<_>>(),
        vec!["a-model", "z-model"]
    );
    let received = String::from_utf8(fixture.request.await.expect("request")).expect("UTF-8");
    assert!(received.starts_with("GET /v1/models HTTP/1.1"));
}

#[tokio::test]
async fn gemini_model_listing_filters_non_generation_models() {
    let fixture = chunked_server(
        "200 OK",
        "application/json",
        vec![br#"{"models":[{"name":"models/embed-only","displayName":"Embed","supportedGenerationMethods":["embedContent"]},{"name":"models/gemini-fixture","displayName":"Gemini Fixture","supportedGenerationMethods":["generateContent"]}]}"#.to_vec()],
        Duration::ZERO,
    )
    .await;
    let adapter = GeminiGenerateContent::new(config(fixture.base, "/v1beta")).expect("adapter");
    let models = adapter
        .list_models(CancellationToken::new())
        .await
        .expect("models");
    assert_eq!(models.len(), 2);
    assert_eq!(models[0].id, "embed-only");
    assert!(!models[0].supports_generation);
    assert_eq!(models[1].id, "gemini-fixture");
    assert!(models[1].supports_generation);
}

#[tokio::test]
async fn runtime_bridge_projects_normalized_text_without_losing_provider_policy() {
    let sse = concat!(
        "event: response.created\ndata: {\"type\":\"response.created\",\"response\":{\"id\":\"resp-runtime\"}}\n\n",
        "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"content_index\":0,\"delta\":\"First \"}\n\n",
        "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"content_index\":0,\"delta\":\"reply.\"}\n\n",
        "event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{}}\n\n",
    );
    let fixture = chunked_server(
        "200 OK",
        "text/event-stream",
        split_bytes(sse, &[1, 4, 15]),
        Duration::ZERO,
    )
    .await;
    let adapter = OpenAiResponses::new(config(fixture.base, "/v1")).expect("adapter");
    let bridge = RuntimeBridge::new(
        Arc::new(adapter),
        RuntimeBridgeConfig {
            default_model: "fixture-model".into(),
            maximum_output_tokens: 128,
            temperature: Some(0.2),
            system_prompt: Some("Stay in character.".into()),
        },
    )
    .expect("bridge");
    assert_eq!(bridge.descriptor().id, "openai");
    assert!(!bridge.descriptor().may_retain_data);
    let stream = bridge
        .stream_response(
            GenerationRequest {
                identity: TurnIdentity {
                    session_id: "session".into(),
                    turn_id: "turn".into(),
                    cancellation_generation: 1,
                },
                transcript: "Hello".into(),
                character: CharacterIdentity::default(),
                memory: MemoryContext::default(),
                locale: "en-US".into(),
                metadata: BTreeMap::new(),
            },
            CancellationToken::new(),
        )
        .await
        .expect("runtime stream");
    let deltas = stream.collect::<Vec<_>>().await;
    assert_eq!(deltas.len(), 2);
    assert_eq!(deltas[0].as_ref().expect("first").sequence, 1);
    assert_eq!(deltas[1].as_ref().expect("second").sequence, 2);
    assert_eq!(
        deltas
            .into_iter()
            .map(|delta| delta.expect("text delta").text)
            .collect::<String>(),
        "First reply."
    );
}

#[test]
fn strict_local_validation_rejects_injection_and_oversized_shapes() {
    let mut invalid = request();
    invalid.model = "../escape".into();
    assert_eq!(
        invalid.validate("fixture").expect_err("invalid model").kind,
        ErrorKind::InvalidRequest
    );

    let mut invalid_tool = request();
    invalid_tool.tools.push(ToolDefinition {
        name: "bad tool name".into(),
        description: String::new(),
        input_schema: json!({"type":"object"}),
    });
    assert_eq!(
        invalid_tool
            .validate("fixture")
            .expect_err("invalid tool")
            .kind,
        ErrorKind::InvalidRequest
    );

    let mut invalid_role = request();
    invalid_role.messages[0] = ChatMessage {
        role: ChatRole::Tool,
        content: "{}".into(),
        name: None,
        tool_call_id: None,
    };
    assert_eq!(
        invalid_role
            .validate("fixture")
            .expect_err("incomplete tool result")
            .kind,
        ErrorKind::InvalidRequest
    );
}

#[tokio::test]
async fn capability_flags_make_stateless_and_retention_policy_explicit() {
    let openai_fixture = chunked_server(
        "200 OK",
        "text/event-stream",
        vec![b"event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{}}\n\n".to_vec()],
        Duration::ZERO,
    )
    .await;
    let openai = OpenAiResponses::new(config(openai_fixture.base, "/v1")).expect("OpenAI");
    assert!(openai.capabilities().privacy.application_stateless);
    assert!(openai.capabilities().privacy.request_disables_storage);
    assert!(!openai.capabilities().privacy.provider_may_retain_data);

    let anthropic_fixture = chunked_server(
        "200 OK",
        "text/event-stream",
        vec![b"event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n".to_vec()],
        Duration::ZERO,
    )
    .await;
    let anthropic =
        AnthropicMessages::new(config(anthropic_fixture.base, "/v1")).expect("Anthropic");
    assert!(anthropic.capabilities().privacy.application_stateless);
    assert!(!anthropic.capabilities().privacy.request_disables_storage);
    assert!(anthropic.capabilities().privacy.provider_may_retain_data);
}

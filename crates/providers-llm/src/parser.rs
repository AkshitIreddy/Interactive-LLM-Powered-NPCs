use std::collections::BTreeMap;

use serde_json::Value;

use crate::{sse::SseFrame, FinishReason, LlmEvent, ProviderError, ProviderProtocol, TokenUsage};

#[derive(Default)]
struct ToolAccumulator {
    id: String,
    name: String,
    arguments: String,
}

pub(crate) struct ProtocolParser {
    provider_id: String,
    protocol: ProviderProtocol,
    tools: BTreeMap<usize, ToolAccumulator>,
    pending_finish: Option<(FinishReason, String)>,
    started: bool,
    terminal: bool,
}

impl ProtocolParser {
    pub fn new(provider_id: String, protocol: ProviderProtocol) -> Self {
        Self {
            provider_id,
            protocol,
            tools: BTreeMap::new(),
            pending_finish: None,
            started: false,
            terminal: false,
        }
    }

    pub fn terminal(&self) -> bool {
        self.terminal
    }

    pub fn parse(&mut self, frame: SseFrame) -> Result<Vec<LlmEvent>, ProviderError> {
        if self.terminal {
            return Ok(Vec::new());
        }
        let events = match self.protocol {
            ProviderProtocol::OpenAiResponses => self.parse_openai_responses(frame),
            ProviderProtocol::AnthropicMessages => self.parse_anthropic(frame),
            ProviderProtocol::GeminiGenerateContent => self.parse_gemini(frame),
            ProviderProtocol::OpenAiChatCompletions => self.parse_chat_completions(frame),
            ProviderProtocol::CohereChat => self.parse_cohere(frame),
            ProviderProtocol::NvidiaNimChat => self.parse_chat_completions(frame),
        }?;
        if events.iter().any(LlmEvent::terminal) {
            self.terminal = true;
        }
        Ok(events)
    }

    fn json(&self, data: &str) -> Result<Value, ProviderError> {
        serde_json::from_str(data).map_err(|_| {
            ProviderError::protocol(&self.provider_id, "provider emitted malformed JSON")
        })
    }

    fn parse_openai_responses(&mut self, frame: SseFrame) -> Result<Vec<LlmEvent>, ProviderError> {
        let value = self.json(&frame.data)?;
        let event_type = required_str(&value, "type", &self.provider_id)?;
        let mut events = Vec::new();
        match event_type {
            "response.created" => {
                self.started = true;
                events.push(LlmEvent::Started {
                    request_id: value
                        .pointer("/response/id")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                });
            }
            "response.output_text.delta" => {
                ensure_started(&mut self.started, &mut events);
                let text = required_str(&value, "delta", &self.provider_id)?;
                if !text.is_empty() {
                    events.push(LlmEvent::TextDelta {
                        text: text.to_owned(),
                        content_index: value
                            .get("content_index")
                            .and_then(Value::as_u64)
                            .unwrap_or(0) as usize,
                    });
                }
            }
            "response.output_item.added" => {
                if value.pointer("/item/type").and_then(Value::as_str) == Some("function_call") {
                    let index = required_usize(&value, "output_index", &self.provider_id)?;
                    let id = value
                        .pointer("/item/call_id")
                        .or_else(|| value.pointer("/item/id"))
                        .and_then(Value::as_str)
                        .ok_or_else(|| {
                            ProviderError::protocol(&self.provider_id, "tool call id is missing")
                        })?
                        .to_owned();
                    let name = value
                        .pointer("/item/name")
                        .and_then(Value::as_str)
                        .ok_or_else(|| {
                            ProviderError::protocol(&self.provider_id, "tool call name is missing")
                        })?
                        .to_owned();
                    self.start_tool(index, id, name, &mut events);
                }
            }
            "response.function_call_arguments.delta" => {
                let index = required_usize(&value, "output_index", &self.provider_id)?;
                let delta = required_str(&value, "delta", &self.provider_id)?;
                let tool = self.tools.get_mut(&index).ok_or_else(|| {
                    ProviderError::protocol(
                        &self.provider_id,
                        "tool delta arrived before tool start",
                    )
                })?;
                tool.arguments.push_str(delta);
                events.push(LlmEvent::ToolCallDelta {
                    index,
                    arguments_fragment: delta.to_owned(),
                });
            }
            "response.output_item.done" => {
                if value.pointer("/item/type").and_then(Value::as_str) == Some("function_call") {
                    let index = required_usize(&value, "output_index", &self.provider_id)?;
                    if let Some(arguments) =
                        value.pointer("/item/arguments").and_then(Value::as_str)
                    {
                        if let Some(tool) = self.tools.get_mut(&index) {
                            if tool.arguments.is_empty() {
                                tool.arguments.push_str(arguments);
                            }
                        }
                    }
                    events.push(self.complete_tool(index)?);
                }
            }
            "response.completed" => {
                if let Some(usage) = usage_openai(value.pointer("/response/usage")) {
                    events.push(LlmEvent::Usage { usage });
                }
                events.push(LlmEvent::Finished {
                    reason: FinishReason::EndTurn,
                    provider_reason: "completed".into(),
                });
            }
            "response.incomplete" => {
                if let Some(usage) = usage_openai(value.pointer("/response/usage")) {
                    events.push(LlmEvent::Usage { usage });
                }
                let reason = value
                    .pointer("/response/incomplete_details/reason")
                    .and_then(Value::as_str)
                    .unwrap_or("incomplete");
                events.push(LlmEvent::Finished {
                    reason: map_finish(reason),
                    provider_reason: bounded(reason),
                });
            }
            "response.failed" | "error" => events.push(LlmEvent::Error {
                error: ProviderError::protocol(
                    &self.provider_id,
                    "provider reported a streaming error",
                ),
            }),
            "response.cancelled" => events.push(LlmEvent::Error {
                error: ProviderError::cancelled(&self.provider_id),
            }),
            _ => {
                // OpenAI adds new event variants over time. Unknown well-formed
                // events are forward-compatible and do not affect text/tool state.
            }
        }
        Ok(events)
    }

    fn parse_anthropic(&mut self, frame: SseFrame) -> Result<Vec<LlmEvent>, ProviderError> {
        let value = self.json(&frame.data)?;
        let event_type = required_str(&value, "type", &self.provider_id)?;
        let mut events = Vec::new();
        match event_type {
            "message_start" => {
                self.started = true;
                events.push(LlmEvent::Started {
                    request_id: value
                        .pointer("/message/id")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                });
                if let Some(usage) = usage_anthropic(value.pointer("/message/usage"), None) {
                    events.push(LlmEvent::Usage { usage });
                }
            }
            "content_block_start" => {
                if value.pointer("/content_block/type").and_then(Value::as_str) == Some("tool_use")
                {
                    let index = required_usize(&value, "index", &self.provider_id)?;
                    let id = value
                        .pointer("/content_block/id")
                        .and_then(Value::as_str)
                        .ok_or_else(|| {
                            ProviderError::protocol(&self.provider_id, "tool call id is missing")
                        })?
                        .to_owned();
                    let name = value
                        .pointer("/content_block/name")
                        .and_then(Value::as_str)
                        .ok_or_else(|| {
                            ProviderError::protocol(&self.provider_id, "tool call name is missing")
                        })?
                        .to_owned();
                    self.start_tool(index, id, name, &mut events);
                }
            }
            "content_block_delta" => match value.pointer("/delta/type").and_then(Value::as_str) {
                Some("text_delta") => {
                    ensure_started(&mut self.started, &mut events);
                    let text = value
                        .pointer("/delta/text")
                        .and_then(Value::as_str)
                        .ok_or_else(|| {
                            ProviderError::protocol(&self.provider_id, "text delta is missing")
                        })?;
                    if !text.is_empty() {
                        events.push(LlmEvent::TextDelta {
                            text: text.to_owned(),
                            content_index: required_usize(&value, "index", &self.provider_id)?,
                        });
                    }
                }
                Some("input_json_delta") => {
                    let index = required_usize(&value, "index", &self.provider_id)?;
                    let delta = value
                        .pointer("/delta/partial_json")
                        .and_then(Value::as_str)
                        .ok_or_else(|| {
                            ProviderError::protocol(&self.provider_id, "tool delta is missing")
                        })?;
                    let tool = self.tools.get_mut(&index).ok_or_else(|| {
                        ProviderError::protocol(
                            &self.provider_id,
                            "tool delta arrived before tool start",
                        )
                    })?;
                    tool.arguments.push_str(delta);
                    events.push(LlmEvent::ToolCallDelta {
                        index,
                        arguments_fragment: delta.to_owned(),
                    });
                }
                _ => {}
            },
            "content_block_stop" => {
                let index = required_usize(&value, "index", &self.provider_id)?;
                if self.tools.contains_key(&index) {
                    events.push(self.complete_tool(index)?);
                }
            }
            "message_delta" => {
                let provider_reason = value
                    .pointer("/delta/stop_reason")
                    .and_then(Value::as_str)
                    .unwrap_or("end_turn");
                if let Some(usage) = usage_anthropic(None, value.get("usage")) {
                    events.push(LlmEvent::Usage { usage });
                }
                self.pending_finish = Some((map_finish(provider_reason), bounded(provider_reason)));
            }
            "message_stop" => {
                let (reason, provider_reason) = self
                    .pending_finish
                    .take()
                    .unwrap_or((FinishReason::EndTurn, "end_turn".into()));
                events.push(LlmEvent::Finished {
                    reason,
                    provider_reason,
                });
            }
            "error" => events.push(LlmEvent::Error {
                error: ProviderError::protocol(
                    &self.provider_id,
                    "provider reported a streaming error",
                ),
            }),
            "ping" => {}
            _ => {}
        }
        Ok(events)
    }

    fn parse_gemini(&mut self, frame: SseFrame) -> Result<Vec<LlmEvent>, ProviderError> {
        let value = self.json(&frame.data)?;
        let mut events = Vec::new();
        if value.get("error").is_some() {
            events.push(LlmEvent::Error {
                error: ProviderError::protocol(
                    &self.provider_id,
                    "provider reported a streaming error",
                ),
            });
            return Ok(events);
        }
        ensure_started(&mut self.started, &mut events);
        if let Some(candidates) = value.get("candidates").and_then(Value::as_array) {
            for candidate in candidates {
                let candidate_index =
                    candidate.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                if let Some(parts) = candidate
                    .pointer("/content/parts")
                    .and_then(Value::as_array)
                {
                    for (part_index, part) in parts.iter().enumerate() {
                        let index = candidate_index
                            .saturating_mul(1_000)
                            .saturating_add(part_index);
                        if part.get("thought").and_then(Value::as_bool) == Some(true) {
                            continue;
                        }
                        if let Some(text) = part.get("text").and_then(Value::as_str) {
                            if !text.is_empty() {
                                events.push(LlmEvent::TextDelta {
                                    text: text.to_owned(),
                                    content_index: index,
                                });
                            }
                        }
                        if let Some(call) = part.get("functionCall") {
                            let name = call
                                .get("name")
                                .and_then(Value::as_str)
                                .ok_or_else(|| {
                                    ProviderError::protocol(
                                        &self.provider_id,
                                        "tool call name is missing",
                                    )
                                })?
                                .to_owned();
                            let id = call
                                .get("id")
                                .and_then(Value::as_str)
                                .map(str::to_owned)
                                .unwrap_or_else(|| {
                                    format!("gemini-{candidate_index}-{part_index}")
                                });
                            events.push(LlmEvent::ToolCallStarted {
                                index,
                                id: id.clone(),
                                name: name.clone(),
                            });
                            events.push(LlmEvent::ToolCallCompleted {
                                index,
                                id,
                                name,
                                arguments: call
                                    .get("args")
                                    .cloned()
                                    .unwrap_or(Value::Object(Default::default())),
                            });
                        }
                    }
                }
                if let Some(reason) = candidate.get("finishReason").and_then(Value::as_str) {
                    self.pending_finish = Some((map_finish(reason), bounded(reason)));
                }
            }
        }
        if let Some(usage) = usage_gemini(value.get("usageMetadata")) {
            events.push(LlmEvent::Usage { usage });
        }
        if let Some((reason, provider_reason)) = self.pending_finish.take() {
            events.push(LlmEvent::Finished {
                reason,
                provider_reason,
            });
        }
        Ok(events)
    }

    fn parse_chat_completions(&mut self, frame: SseFrame) -> Result<Vec<LlmEvent>, ProviderError> {
        if frame.data.trim() == "[DONE]" {
            if self.terminal {
                return Ok(Vec::new());
            }
            let mut events = self.complete_all_tools()?;
            let (reason, provider_reason) = self
                .pending_finish
                .take()
                .unwrap_or((FinishReason::EndTurn, "stop".into()));
            events.push(LlmEvent::Finished {
                reason,
                provider_reason,
            });
            return Ok(events);
        }
        let value = self.json(&frame.data)?;
        if value.get("error").is_some() {
            return Ok(vec![LlmEvent::Error {
                error: ProviderError::protocol(
                    &self.provider_id,
                    "provider reported a streaming error",
                ),
            }]);
        }
        let mut events = Vec::new();
        if !self.started {
            self.started = true;
            events.push(LlmEvent::Started {
                request_id: value.get("id").and_then(Value::as_str).map(str::to_owned),
            });
        }
        if let Some(choices) = value.get("choices").and_then(Value::as_array) {
            for choice in choices {
                let content_index =
                    choice.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                if let Some(text) = choice.pointer("/delta/content").and_then(Value::as_str) {
                    if !text.is_empty() {
                        events.push(LlmEvent::TextDelta {
                            text: text.to_owned(),
                            content_index,
                        });
                    }
                }
                if let Some(tool_calls) = choice
                    .pointer("/delta/tool_calls")
                    .and_then(Value::as_array)
                {
                    for call in tool_calls {
                        let index = call.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                        let id = call.get("id").and_then(Value::as_str);
                        let name = call.pointer("/function/name").and_then(Value::as_str);
                        if !self.tools.contains_key(&index) {
                            self.start_tool(
                                index,
                                id.unwrap_or("tool-call").to_owned(),
                                name.unwrap_or("unknown_tool").to_owned(),
                                &mut events,
                            );
                        } else if let Some(tool) = self.tools.get_mut(&index) {
                            if let Some(id) = id {
                                tool.id = id.to_owned();
                            }
                            if let Some(name) = name {
                                tool.name = name.to_owned();
                            }
                        }
                        if let Some(delta) =
                            call.pointer("/function/arguments").and_then(Value::as_str)
                        {
                            if let Some(tool) = self.tools.get_mut(&index) {
                                tool.arguments.push_str(delta);
                            }
                            events.push(LlmEvent::ToolCallDelta {
                                index,
                                arguments_fragment: delta.to_owned(),
                            });
                        }
                    }
                }
                if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
                    self.pending_finish = Some((map_finish(reason), bounded(reason)));
                }
            }
        }
        if let Some(usage) = usage_openai(value.get("usage")) {
            events.push(LlmEvent::Usage { usage });
        }
        Ok(events)
    }

    fn parse_cohere(&mut self, frame: SseFrame) -> Result<Vec<LlmEvent>, ProviderError> {
        let value = self.json(&frame.data)?;
        let event_type = required_str(&value, "type", &self.provider_id)?;
        let mut events = Vec::new();
        match event_type {
            "message-start" => {
                self.started = true;
                events.push(LlmEvent::Started {
                    request_id: value.get("id").and_then(Value::as_str).map(str::to_owned),
                });
            }
            "content-delta" => {
                ensure_started(&mut self.started, &mut events);
                let text = value
                    .pointer("/delta/message/content/text")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        ProviderError::protocol(&self.provider_id, "text delta is missing")
                    })?;
                if !text.is_empty() {
                    events.push(LlmEvent::TextDelta {
                        text: text.to_owned(),
                        content_index: value.get("index").and_then(Value::as_u64).unwrap_or(0)
                            as usize,
                    });
                }
            }
            "tool-call-start" => {
                let index = required_usize(&value, "index", &self.provider_id)?;
                let call = value.pointer("/delta/message/tool_calls").ok_or_else(|| {
                    ProviderError::protocol(&self.provider_id, "tool call is missing")
                })?;
                let id = call
                    .get("id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        ProviderError::protocol(&self.provider_id, "tool call id is missing")
                    })?
                    .to_owned();
                let name = call
                    .pointer("/function/name")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        ProviderError::protocol(&self.provider_id, "tool call name is missing")
                    })?
                    .to_owned();
                self.start_tool(index, id, name, &mut events);
                if let Some(arguments) = call.pointer("/function/arguments").and_then(Value::as_str)
                {
                    if !arguments.is_empty() {
                        let tool = self.tools.get_mut(&index).ok_or_else(|| {
                            ProviderError::protocol(
                                &self.provider_id,
                                "tool state could not be initialized",
                            )
                        })?;
                        tool.arguments.push_str(arguments);
                        events.push(LlmEvent::ToolCallDelta {
                            index,
                            arguments_fragment: arguments.to_owned(),
                        });
                    }
                }
            }
            "tool-call-delta" => {
                let index = required_usize(&value, "index", &self.provider_id)?;
                let delta = value
                    .pointer("/delta/message/tool_calls/function/arguments")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        ProviderError::protocol(&self.provider_id, "tool delta is missing")
                    })?;
                let tool = self.tools.get_mut(&index).ok_or_else(|| {
                    ProviderError::protocol(
                        &self.provider_id,
                        "tool delta arrived before tool start",
                    )
                })?;
                tool.arguments.push_str(delta);
                events.push(LlmEvent::ToolCallDelta {
                    index,
                    arguments_fragment: delta.to_owned(),
                });
            }
            "tool-call-end" => {
                let index = required_usize(&value, "index", &self.provider_id)?;
                events.push(self.complete_tool(index)?);
            }
            "message-end" => {
                if !self.tools.is_empty() {
                    return Err(ProviderError::protocol(
                        &self.provider_id,
                        "provider ended before completing all tool calls",
                    ));
                }
                if let Some(usage) = usage_cohere(value.pointer("/delta/usage")) {
                    events.push(LlmEvent::Usage { usage });
                }
                let reason = value
                    .pointer("/delta/finish_reason")
                    .and_then(Value::as_str)
                    .unwrap_or("COMPLETE");
                if reason == "TIMEOUT" || reason.starts_with("ERROR") {
                    events.push(LlmEvent::Error {
                        error: ProviderError::new(
                            &self.provider_id,
                            if reason == "TIMEOUT" {
                                crate::ErrorKind::Timeout
                            } else {
                                crate::ErrorKind::Unavailable
                            },
                            "provider reported a terminal generation error",
                        ),
                    });
                } else {
                    events.push(LlmEvent::Finished {
                        reason: map_finish(reason),
                        provider_reason: bounded(reason),
                    });
                }
            }
            "error" => events.push(LlmEvent::Error {
                error: ProviderError::protocol(
                    &self.provider_id,
                    "provider reported a streaming error",
                ),
            }),
            _ => {}
        }
        Ok(events)
    }

    fn start_tool(&mut self, index: usize, id: String, name: String, events: &mut Vec<LlmEvent>) {
        self.tools.entry(index).or_insert_with(|| ToolAccumulator {
            id: id.clone(),
            name: name.clone(),
            arguments: String::new(),
        });
        events.push(LlmEvent::ToolCallStarted { index, id, name });
    }

    fn complete_tool(&mut self, index: usize) -> Result<LlmEvent, ProviderError> {
        let tool = self.tools.remove(&index).ok_or_else(|| {
            ProviderError::protocol(
                &self.provider_id,
                "tool completion arrived before tool start",
            )
        })?;
        let arguments = if tool.arguments.trim().is_empty() {
            Value::Object(Default::default())
        } else {
            serde_json::from_str(&tool.arguments).map_err(|_| {
                ProviderError::protocol(&self.provider_id, "tool arguments are malformed JSON")
            })?
        };
        Ok(LlmEvent::ToolCallCompleted {
            index,
            id: tool.id,
            name: tool.name,
            arguments,
        })
    }

    fn complete_all_tools(&mut self) -> Result<Vec<LlmEvent>, ProviderError> {
        let indices = self.tools.keys().copied().collect::<Vec<_>>();
        indices
            .into_iter()
            .map(|index| self.complete_tool(index))
            .collect()
    }
}

fn ensure_started(started: &mut bool, events: &mut Vec<LlmEvent>) {
    if !*started {
        *started = true;
        events.push(LlmEvent::Started { request_id: None });
    }
}

fn required_str<'a>(
    value: &'a Value,
    key: &str,
    provider_id: &str,
) -> Result<&'a str, ProviderError> {
    value.get(key).and_then(Value::as_str).ok_or_else(|| {
        ProviderError::protocol(provider_id, "provider event is missing a required field")
    })
}

fn required_usize(value: &Value, key: &str, provider_id: &str) -> Result<usize, ProviderError> {
    let raw = value.get(key).and_then(Value::as_u64).ok_or_else(|| {
        ProviderError::protocol(provider_id, "provider event is missing a required index")
    })?;
    usize::try_from(raw)
        .map_err(|_| ProviderError::protocol(provider_id, "provider event index is out of range"))
}

fn bounded(value: &str) -> String {
    value.chars().take(128).collect()
}

fn map_finish(reason: &str) -> FinishReason {
    match reason.to_ascii_lowercase().as_str() {
        "stop" | "end_turn" | "complete" | "completed" => FinishReason::EndTurn,
        "length" | "max_tokens" | "max_output_tokens" => FinishReason::MaximumTokens,
        "tool_call" | "tool_calls" | "tool_use" => FinishReason::ToolUse,
        "stop_sequence" => FinishReason::StopSequence,
        "safety" | "content_filter" | "recitation" | "blocklist" | "prohibited_content" => {
            FinishReason::Safety
        }
        "cancelled" => FinishReason::Cancelled,
        other => FinishReason::Other(bounded(other)),
    }
}

fn usage_openai(value: Option<&Value>) -> Option<TokenUsage> {
    let value = value?;
    let input_tokens = value
        .get("input_tokens")
        .or_else(|| value.get("prompt_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let output_tokens = value
        .get("output_tokens")
        .or_else(|| value.get("completion_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let cached_input_tokens = value
        .pointer("/input_tokens_details/cached_tokens")
        .or_else(|| value.pointer("/prompt_tokens_details/cached_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    Some(TokenUsage {
        input_tokens,
        output_tokens,
        cached_input_tokens,
        total_tokens: value
            .get("total_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(input_tokens.saturating_add(output_tokens)),
    })
}

fn usage_anthropic(start: Option<&Value>, delta: Option<&Value>) -> Option<TokenUsage> {
    let value = start.or(delta)?;
    let input_tokens = value
        .get("input_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0)
        + value
            .get("cache_creation_input_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0)
        + value
            .get("cache_read_input_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0);
    let output_tokens = value
        .get("output_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    Some(TokenUsage {
        input_tokens,
        output_tokens,
        cached_input_tokens: value
            .get("cache_read_input_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        total_tokens: input_tokens.saturating_add(output_tokens),
    })
}

fn usage_gemini(value: Option<&Value>) -> Option<TokenUsage> {
    let value = value?;
    let input_tokens = value
        .get("promptTokenCount")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let output_tokens = value
        .get("candidatesTokenCount")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    Some(TokenUsage {
        input_tokens,
        output_tokens,
        cached_input_tokens: value
            .get("cachedContentTokenCount")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        total_tokens: value
            .get("totalTokenCount")
            .and_then(Value::as_u64)
            .unwrap_or(input_tokens.saturating_add(output_tokens)),
    })
}

fn usage_cohere(value: Option<&Value>) -> Option<TokenUsage> {
    let value = value?;
    let tokens = value.get("tokens").or_else(|| value.get("billed_units"))?;
    let input_tokens = numeric_u64(tokens.get("input_tokens"));
    let output_tokens = numeric_u64(tokens.get("output_tokens"));
    Some(TokenUsage {
        input_tokens,
        output_tokens,
        cached_input_tokens: 0,
        total_tokens: input_tokens.saturating_add(output_tokens),
    })
}

fn numeric_u64(value: Option<&Value>) -> u64 {
    value
        .and_then(|value| {
            value.as_u64().or_else(|| {
                let number = value.as_f64()?;
                (number.is_finite() && number >= 0.0 && number.fract() == 0.0)
                    .then_some(number as u64)
            })
        })
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(data: &str) -> SseFrame {
        SseFrame {
            event: None,
            data: data.into(),
        }
    }

    #[test]
    fn malformed_provider_json_is_not_forwarded() {
        let mut parser = ProtocolParser::new("fixture".into(), ProviderProtocol::OpenAiResponses);
        let error = parser.parse(frame("{broken")).expect_err("must reject");
        assert_eq!(error.kind, crate::ErrorKind::Protocol);
        assert!(!error.message.contains("broken"));
    }

    #[test]
    fn anthropic_accumulates_and_validates_tool_arguments() {
        let mut parser =
            ProtocolParser::new("anthropic".into(), ProviderProtocol::AnthropicMessages);
        parser
            .parse(frame(r#"{"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"call-1","name":"lookup"}}"#))
            .expect("start");
        parser
            .parse(frame(r#"{"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"city\":\"Pa"}}"#))
            .expect("first delta");
        parser
            .parse(frame(r#"{"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"ris\"}"}}"#))
            .expect("second delta");
        let events = parser
            .parse(frame(r#"{"type":"content_block_stop","index":1}"#))
            .expect("completion");
        assert!(matches!(
            &events[0],
            LlmEvent::ToolCallCompleted { arguments, .. }
                if arguments == &serde_json::json!({"city":"Paris"})
        ));
    }

    #[test]
    fn malformed_tool_arguments_fail_locally() {
        let mut parser =
            ProtocolParser::new("compat".into(), ProviderProtocol::OpenAiChatCompletions);
        parser.parse(frame(r#"{"id":"x","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"c","function":{"name":"lookup","arguments":"{"}}]},"finish_reason":null}]}"#)).expect("delta is allowed");
        let error = parser
            .parse(frame("[DONE]"))
            .expect_err("final JSON must validate");
        assert_eq!(error.kind, crate::ErrorKind::Protocol);
    }
}

//! Translation layer: Responses API ↔ Chat Completions API.
//!
//! Only used by Synthetic (and any future providers that lack Responses API support).
//! OpenAI, OpenRouter, and LM Studio all support the Responses API natively.

use crate::client_common::ResponseStream;
use crate::default_client::build_reqwest_client;
use crate::error::CodexErr;
use crate::error::Result;
use crate::model_provider_info::ModelProviderInfo;
use codex_api::common::ResponseEvent;
use codex_api::common::ResponsesApiRequest;
use codex_protocol::models::{ContentItem, ResponseItem};
use codex_protocol::protocol::TokenUsage;
use futures::StreamExt;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, info, trace};

/// Convert a `ResponsesApiRequest` into a Chat Completions JSON body.
pub fn build_chat_request(request: &ResponsesApiRequest) -> Value {
    let mut messages = Vec::<Value>::new();

    // System message from instructions.
    if !request.instructions.is_empty() {
        messages.push(json!({
            "role": "system",
            "content": request.instructions
        }));
    }

    // Convert ResponseItems → Chat messages.
    for item in &request.input {
        match item {
            ResponseItem::Message { role, content, .. } => {
                let text = content
                    .iter()
                    .filter_map(|c| match c {
                        ContentItem::InputText { text } | ContentItem::OutputText { text } => {
                            Some(text.as_str())
                        }
                        ContentItem::InputImage { .. } => None,
                    })
                    .collect::<Vec<_>>()
                    .join("");

                let api_role = if role == "developer" { "system" } else { role };
                messages.push(json!({
                    "role": api_role,
                    "content": text
                }));
            }
            ResponseItem::FunctionCall {
                name,
                arguments,
                call_id,
                ..
            } => {
                let tool_call = json!({
                    "id": call_id,
                    "type": "function",
                    "function": {
                        "name": name,
                        "arguments": arguments,
                    }
                });

                let should_append = messages.last().map_or(false, |m| {
                    m.get("role").and_then(Value::as_str) == Some("assistant")
                        && m.get("content").map_or(false, Value::is_null)
                        && m.get("tool_calls").is_some()
                });

                if should_append {
                    if let Some(last) = messages.last_mut() {
                        if let Some(calls) =
                            last.get_mut("tool_calls").and_then(Value::as_array_mut)
                        {
                            calls.push(tool_call);
                        }
                    }
                } else {
                    messages.push(json!({
                        "role": "assistant",
                        "content": null,
                        "tool_calls": [tool_call]
                    }));
                }
            }
            ResponseItem::FunctionCallOutput { call_id, output } => {
                messages.push(json!({
                    "role": "tool",
                    "tool_call_id": call_id,
                    "content": output.text_content().unwrap_or_default(),
                }));
            }
            ResponseItem::LocalShellCall {
                call_id, action, ..
            } => {
                let codex_protocol::models::LocalShellAction::Exec(exec) = action;
                let args = json!({ "command": exec.command });
                let cid = call_id.as_deref().unwrap_or("shell_call");
                let tool_call = json!({
                    "id": cid,
                    "type": "function",
                    "function": {
                        "name": "shell",
                        "arguments": args.to_string(),
                    }
                });
                messages.push(json!({
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [tool_call]
                }));
            }
            _ => {}
        }
    }

    // Convert tools: Responses API format → Chat Completions format.
    let tools: Vec<Value> = request
        .tools
        .iter()
        .filter_map(|t| {
            let name = t.get("name").and_then(Value::as_str)?;
            let params = t.get("parameters").cloned().unwrap_or(json!({}));
            let desc = t.get("description").and_then(Value::as_str).unwrap_or("");
            Some(json!({
                "type": "function",
                "function": {
                    "name": name,
                    "description": desc,
                    "parameters": params,
                }
            }))
        })
        .collect();

    let mut body = json!({
        "model": request.model,
        "messages": messages,
        "stream": true,
    });

    if !tools.is_empty() {
        body["tools"] = json!(tools);
        body["tool_choice"] = json!("auto");
    }

    body
}

/// State for accumulating streaming tool call deltas.
#[derive(Default, Debug)]
struct ToolCallState {
    id: Option<String>,
    name: String,
    arguments: String,
}

/// Execute a Chat Completions streaming request and return a `ResponseStream`.
///
/// Signature matches what `client.rs` calls from the `WireApi::Chat` dispatch arm.
pub async fn stream_chat_completions(
    request: &ResponsesApiRequest,
    provider: &ModelProviderInfo,
    _auth_manager: &Option<Arc<crate::auth::AuthManager>>,
) -> Result<ResponseStream> {
    let base_url = provider
        .base_url
        .as_deref()
        .unwrap_or("http://localhost:1234/v1");
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));

    let body = build_chat_request(request);
    info!("chat completions request to {url} with model={}", request.model);
    trace!("chat completions body: {}", body);

    let client = build_reqwest_client();
    let mut req_builder = client.post(&url).json(&body);

    // Add bearer token from provider API key.
    if let Ok(Some(api_key)) = provider.api_key() {
        req_builder = req_builder.header("Authorization", format!("Bearer {}", api_key));
    }

    let response = req_builder
        .send()
        .await
        .map_err(|e| CodexErr::Fatal(format!("Chat completions request to {url} failed: {e}")))?;

    let status = response.status();
    if !status.is_success() {
        let body_text = response.text().await.unwrap_or_default();
        return Err(CodexErr::Fatal(format!(
            "Chat completions returned {status}: {body_text}"
        )));
    }

    let (tx, rx) = mpsc::channel::<Result<ResponseEvent>>(1600);

    // Spawn a task to read the raw SSE byte stream.
    let byte_stream = response.bytes_stream();
    tokio::spawn(async move {
        process_sse_bytes(byte_stream, tx).await;
    });

    Ok(ResponseStream { rx_event: rx })
}

/// Parse raw SSE bytes from a reqwest response stream.
async fn process_sse_bytes<S, B>(stream: S, tx: mpsc::Sender<Result<ResponseEvent>>)
where
    B: AsRef<[u8]>,
    S: futures::Stream<Item = std::result::Result<B, reqwest::Error>> + Unpin + Send + 'static,
{
    let mut stream = stream;
    let mut buffer = String::new();
    let mut tool_calls: HashMap<usize, ToolCallState> = HashMap::new();
    let mut tool_call_order: Vec<usize> = Vec::new();
    let mut assistant_text = String::new();
    let mut total_input_tokens: i64 = 0;
    let mut total_output_tokens: i64 = 0;

    while let Some(chunk_result) = stream.next().await {
        let chunk = match chunk_result {
            Ok(c) => c,
            Err(e) => {
                let _ = tx
                    .send(Err(CodexErr::Fatal(format!("Stream error: {e}"))))
                    .await;
                return;
            }
        };

        buffer.push_str(&String::from_utf8_lossy(chunk.as_ref()));

        // Process complete SSE lines.
        loop {
            let line_end = match buffer.find('\n') {
                Some(pos) => pos,
                None => break,
            };
            let line = buffer[..line_end].trim_end_matches('\r').to_string();
            buffer = buffer[line_end + 1..].to_string();

            if line.is_empty() {
                continue;
            }

            let data = if let Some(d) = line.strip_prefix("data: ") {
                d.trim()
            } else if let Some(d) = line.strip_prefix("data:") {
                d.trim()
            } else {
                continue;
            };

            if data.is_empty() {
                continue;
            }
            if data == "[DONE]" || data == "DONE" {
                break;
            }

            let value: Value = match serde_json::from_str(data) {
                Ok(v) => v,
                Err(e) => {
                    debug!("Failed to parse chat SSE: {e}, data: {data}");
                    continue;
                }
            };

            // Extract token usage if present.
            if let Some(usage) = value.get("usage") {
                if let Some(pt) = usage.get("prompt_tokens").and_then(Value::as_i64) {
                    total_input_tokens = pt;
                }
                if let Some(ct) = usage.get("completion_tokens").and_then(Value::as_i64) {
                    total_output_tokens = ct;
                }
            }

            let Some(choices) = value.get("choices").and_then(Value::as_array) else {
                continue;
            };

            for choice in choices {
                if let Some(delta) = choice.get("delta") {
                    // Text content delta.
                    if let Some(content) = delta.get("content").and_then(Value::as_str) {
                        if !content.is_empty() {
                            assistant_text.push_str(content);
                            let _ = tx
                                .send(Ok(ResponseEvent::OutputTextDelta(content.to_string())))
                                .await;
                        }
                    }

                    // Tool call deltas.
                    if let Some(tc_arr) = delta.get("tool_calls").and_then(Value::as_array) {
                        for tc in tc_arr {
                            let idx =
                                tc.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                            let state = tool_calls.entry(idx).or_default();
                            if !tool_call_order.contains(&idx) {
                                tool_call_order.push(idx);
                            }
                            if let Some(id) = tc.get("id").and_then(Value::as_str) {
                                state.id = Some(id.to_string());
                            }
                            if let Some(func) = tc.get("function") {
                                if let Some(name) = func.get("name").and_then(Value::as_str) {
                                    if !name.is_empty() {
                                        state.name = name.to_string();
                                    }
                                }
                                if let Some(args) =
                                    func.get("arguments").and_then(Value::as_str)
                                {
                                    state.arguments.push_str(args);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Flush accumulated assistant text.
    if !assistant_text.is_empty() {
        let item = ResponseItem::Message {
            id: None,
            role: "assistant".to_string(),
            content: vec![ContentItem::OutputText {
                text: assistant_text,
            }],
            end_turn: Some(true),
            phase: None,
        };
        let _ = tx.send(Ok(ResponseEvent::OutputItemDone(item))).await;
    }

    // Flush accumulated tool calls.
    for idx in &tool_call_order {
        if let Some(tc) = tool_calls.remove(idx) {
            let call_id = tc.id.unwrap_or_else(|| format!("call_{idx}"));
            let item = ResponseItem::FunctionCall {
                id: None,
                name: tc.name,
                arguments: tc.arguments,
                call_id,
            };
            let _ = tx.send(Ok(ResponseEvent::OutputItemDone(item))).await;
        }
    }

    // Send Completed event.
    let usage = if total_input_tokens > 0 || total_output_tokens > 0 {
        Some(TokenUsage {
            input_tokens: total_input_tokens,
            output_tokens: total_output_tokens,
            cached_input_tokens: 0,
            reasoning_output_tokens: 0,
            total_tokens: total_input_tokens + total_output_tokens,
        })
    } else {
        None
    };

    let _ = tx
        .send(Ok(ResponseEvent::Completed {
            response_id: String::new(),
            token_usage: usage,
            can_append: false,
        }))
        .await;

    info!("chat completions stream finished");
}

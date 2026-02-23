//! Translates OpenCode REST SSE events into CodexMonitor-shaped
//! `AppServerEvent` messages that the React frontend already knows how to
//! consume.
//!
//! Design invariant (INV4 from spec): the frontend thread reducer receives
//! events in the **same shape** as the original CodexMonitor protocol. All
//! OpenCode ↔ CodexMonitor translation happens here in Rust.

use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

/// Per-session state the translator needs to synthesize IDs the frontend
/// expects but the REST SSE protocol does not provide (turn IDs, monotonic
/// item IDs, etc.).
pub(crate) struct SessionTranslationState {
    /// The session ID (maps to CodexMonitor's "threadId").
    pub(crate) session_id: String,
    /// Synthesized turn ID.  Incremented each time a prompt is sent.
    pub(crate) current_turn_id: String,
    /// Counter for synthesizing unique item IDs.
    item_counter: AtomicU64,
    /// Map tool Part `id` → synthesized CodexMonitor `itemId`.
    tool_call_items: HashMap<String, String>,
    /// Stable item ID for the current agent-message stream.
    agent_message_item_id: Option<String>,
    /// Stable item ID for the current reasoning stream.
    reasoning_item_id: Option<String>,
    /// Stable item ID for contiguous user-message chunk streams.
    pub(crate) user_message_item_id: Option<String>,
    /// Buffered text for contiguous user-message chunk streams.
    pub(crate) user_message_text: String,
}

impl SessionTranslationState {
    pub(crate) fn new(session_id: String) -> Self {
        Self {
            session_id,
            current_turn_id: String::new(),
            item_counter: AtomicU64::new(1),
            tool_call_items: HashMap::new(),
            agent_message_item_id: None,
            reasoning_item_id: None,
            user_message_item_id: None,
            user_message_text: String::new(),
        }
    }

    fn next_item_id(&self) -> String {
        let n = self.item_counter.fetch_add(1, Ordering::SeqCst);
        format!("item_{n}")
    }

    /// Start a new turn (called before sending a prompt). Resets per-turn
    /// ephemeral state so the next batch of events gets fresh item IDs.
    pub(crate) fn start_turn(&mut self, turn_id: String) {
        self.current_turn_id = turn_id;
        self.tool_call_items.clear();
        self.agent_message_item_id = None;
        self.reasoning_item_id = None;
        self.user_message_item_id = None;
        self.user_message_text.clear();
    }

    /// Prepare translation state for replaying historical messages.
    pub(crate) fn prepare_replay(&mut self, session_id: String) {
        self.session_id = session_id;
        self.current_turn_id.clear();
        self.tool_call_items.clear();
        self.agent_message_item_id = None;
        self.reasoning_item_id = None;
        self.user_message_item_id = None;
        self.user_message_text.clear();
    }

    fn agent_message_item(&mut self) -> String {
        if let Some(ref id) = self.agent_message_item_id {
            id.clone()
        } else {
            let id = self.next_item_id();
            self.agent_message_item_id = Some(id.clone());
            id
        }
    }

    fn reasoning_item(&mut self) -> String {
        if let Some(ref id) = self.reasoning_item_id {
            id.clone()
        } else {
            let id = self.next_item_id();
            self.reasoning_item_id = Some(id.clone());
            id
        }
    }

    pub(crate) fn user_message_item(&mut self) -> String {
        if let Some(ref id) = self.user_message_item_id {
            id.clone()
        } else {
            let id = self.next_item_id();
            self.user_message_item_id = Some(id.clone());
            id
        }
    }

    pub(crate) fn mark_new_replayed_user_message_boundary(&mut self) {
        self.tool_call_items.clear();
        self.agent_message_item_id = None;
        self.reasoning_item_id = None;
    }
}

// ---------------------------------------------------------------------------
// Public translation entry points
// ---------------------------------------------------------------------------

/// Translate an OpenCode REST SSE event into one or more CodexMonitor-shaped
/// JSON-RPC messages (method + params).
///
/// SSE events have shape: `{ type: "<event_type>", properties: { ... } }`
///
/// Returns an empty Vec when the event should be silently dropped.
pub(crate) fn translate_sse_event(
    sse_event: &Value,
    state: &mut SessionTranslationState,
) -> Vec<Value> {
    let event_type = match sse_event.get("type").and_then(|v| v.as_str()) {
        Some(t) => t,
        None => return vec![],
    };
    let properties = sse_event.get("properties").unwrap_or(&Value::Null);

    match event_type {
        "message.part.updated" => translate_part_updated(properties, state),
        "message.updated" => translate_message_updated(properties, state),
        "session.status" => translate_session_status(properties, state),
        "permission.updated" => translate_sse_permission(properties, state),
        _ => {
            #[cfg(debug_assertions)]
            eprintln!("[event_translator] unknown SSE event type: {event_type}");
            vec![]
        }
    }
}

// ---------------------------------------------------------------------------
// message.part.updated — the main event for streaming content
// ---------------------------------------------------------------------------

fn translate_part_updated(properties: &Value, state: &mut SessionTranslationState) -> Vec<Value> {
    let part = match properties.get("part") {
        Some(p) => p,
        None => return vec![],
    };
    let delta = properties
        .get("delta")
        .and_then(|v| v.as_str())
        .unwrap_or_default();

    let part_type = part.get("type").and_then(|v| v.as_str()).unwrap_or("");

    // Extract session ID from the part if available.
    if let Some(sid) = part.get("sessionID").and_then(|v| v.as_str()) {
        if !sid.is_empty() {
            state.session_id = sid.to_string();
        }
    }
    let thread_id = state.session_id.clone();
    let turn_id = state.current_turn_id.clone();

    match part_type {
        "text" => {
            if delta.is_empty() {
                return vec![];
            }

            // Check message role to distinguish user vs assistant text.
            let message_role = part
                .get("messageID")
                .and_then(|_| properties.get("part"))
                .and_then(|_| {
                    // If we're in replay mode (no turn ID), text parts from
                    // user messages should be emitted as userMessage items.
                    // The REST API doesn't give us explicit role on the Part,
                    // but during replay we infer from turn state.
                    None::<&str>
                });
            let _ = message_role; // Unused for now; user messages handled separately.

            // In active turn mode, text deltas are agent message chunks.
            let item_id = state.agent_message_item();
            vec![json!({
                "method": "item/agentMessage/delta",
                "params": {
                    "threadId": thread_id,
                    "turnId": turn_id,
                    "itemId": item_id,
                    "delta": delta
                }
            })]
        }

        "reasoning" => {
            if delta.is_empty() {
                return vec![];
            }
            let item_id = state.reasoning_item();
            vec![json!({
                "method": "item/reasoning/textDelta",
                "params": {
                    "threadId": thread_id,
                    "turnId": turn_id,
                    "itemId": item_id,
                    "delta": delta
                }
            })]
        }

        "tool" => translate_tool_part(part, state, &thread_id),

        _ => vec![],
    }
}

// ---------------------------------------------------------------------------
// Tool part translation
// ---------------------------------------------------------------------------

fn translate_tool_part(
    part: &Value,
    state: &mut SessionTranslationState,
    thread_id: &str,
) -> Vec<Value> {
    let tool_state = match part.get("state") {
        Some(s) => s,
        None => return vec![],
    };
    let status = tool_state
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("pending");
    let tool_name = part
        .get("tool")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let part_id = part.get("id").and_then(|v| v.as_str()).unwrap_or_default();

    let item_id = if let Some(existing) = state.tool_call_items.get(part_id) {
        existing.clone()
    } else {
        let id = state.next_item_id();
        if !part_id.is_empty() {
            state
                .tool_call_items
                .insert(part_id.to_string(), id.clone());
        }
        id
    };

    let item_type = tool_kind_to_item_type(tool_name);
    let raw_input = tool_state.get("input").cloned();

    let mut events = Vec::new();

    match status {
        "pending" | "running" => {
            let started_item = build_tool_item(
                &item_id,
                item_type,
                tool_name,
                "in_progress",
                raw_input.as_ref(),
                None,
                None,
            );
            events.push(json!({
                "method": "item/started",
                "params": {
                    "threadId": thread_id,
                    "item": started_item
                }
            }));

            if let Some(ref input) = raw_input {
                let delta_text = serde_json::to_string_pretty(input).unwrap_or_default();
                if !delta_text.is_empty() {
                    let method = if item_type == "fileChange" {
                        "item/fileChange/outputDelta"
                    } else {
                        "item/commandExecution/outputDelta"
                    };
                    events.push(json!({
                        "method": method,
                        "params": {
                            "threadId": thread_id,
                            "itemId": item_id,
                            "delta": delta_text
                        }
                    }));
                }
            }
        }
        "completed" => {
            let output_text = tool_state
                .get("output")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let item = build_tool_item(
                &item_id,
                item_type,
                tool_name,
                "completed",
                raw_input.as_ref(),
                Some(output_text),
                None,
            );
            events.push(json!({
                "method": "item/completed",
                "params": {
                    "threadId": thread_id,
                    "item": item
                }
            }));
        }
        "error" => {
            let error_text = tool_state
                .get("output")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let item = build_tool_item(
                &item_id,
                item_type,
                tool_name,
                "failed",
                raw_input.as_ref(),
                Some(error_text),
                None,
            );
            events.push(json!({
                "method": "item/completed",
                "params": {
                    "threadId": thread_id,
                    "item": item
                }
            }));
        }
        _ => {}
    }

    events
}

// ---------------------------------------------------------------------------
// message.updated — token/usage info
// ---------------------------------------------------------------------------

fn translate_message_updated(
    properties: &Value,
    state: &mut SessionTranslationState,
) -> Vec<Value> {
    let info = match properties.get("info") {
        Some(i) => i,
        None => return vec![],
    };
    let thread_id = state.session_id.clone();

    // Extract token usage from message info if available.
    let input_tokens = info
        .get("inputTokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let output_tokens = info
        .get("outputTokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let cached_tokens = info
        .get("cachedInputTokens")
        .or_else(|| info.get("cacheReadInputTokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let reasoning_tokens = info
        .get("reasoningOutputTokens")
        .or_else(|| info.get("reasoningTokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let total = input_tokens + output_tokens;

    if total == 0 {
        return vec![];
    }

    vec![json!({
        "method": "thread/tokenUsage/updated",
        "params": {
            "threadId": thread_id,
            "tokenUsage": {
                "total": {
                    "totalTokens": total,
                    "inputTokens": input_tokens,
                    "cachedInputTokens": cached_tokens,
                    "outputTokens": output_tokens,
                    "reasoningOutputTokens": reasoning_tokens
                },
                "last": {
                    "totalTokens": total,
                    "inputTokens": input_tokens,
                    "cachedInputTokens": cached_tokens,
                    "outputTokens": output_tokens,
                    "reasoningOutputTokens": reasoning_tokens
                },
                "modelContextWindow": 0
            }
        }
    })]
}

// ---------------------------------------------------------------------------
// session.status — idle/active/error transitions
// ---------------------------------------------------------------------------

fn translate_session_status(properties: &Value, state: &mut SessionTranslationState) -> Vec<Value> {
    let session_id = properties
        .get("sessionID")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    if !session_id.is_empty() {
        state.session_id = session_id.to_string();
    }

    let status = properties
        .get("status")
        .and_then(|s| s.get("type"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let thread_id = state.session_id.clone();
    let turn_id = state.current_turn_id.clone();

    match status {
        "idle" => {
            // Idle after active means the turn is done.
            if !turn_id.is_empty() {
                let mut events = Vec::new();
                if let Some(msg_completed) = build_agent_message_completed(state) {
                    events.push(msg_completed);
                }
                events.push(build_turn_completed(&thread_id, &turn_id));
                events
            } else {
                vec![]
            }
        }
        "error" => {
            let error_msg = properties
                .get("status")
                .and_then(|s| s.get("message"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown error");
            vec![json!({
                "method": "error",
                "params": {
                    "threadId": thread_id,
                    "turnId": turn_id,
                    "willRetry": false,
                    "error": {
                        "message": error_msg
                    }
                }
            })]
        }
        _ => vec![],
    }
}

// ---------------------------------------------------------------------------
// permission.updated — permission requests from the agent
// ---------------------------------------------------------------------------

fn translate_sse_permission(properties: &Value, state: &mut SessionTranslationState) -> Vec<Value> {
    let permission_id = properties
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let session_id = properties
        .get("sessionID")
        .and_then(|v| v.as_str())
        .unwrap_or(&state.session_id);
    let perm_type = properties
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("command");
    let pattern = properties.get("pattern");

    // Encode sessionId:permissionId into the event `id` so the frontend
    // can round-trip it back to `respond_to_server_request`.
    let composite_id = format!("{session_id}:{permission_id}");

    let command_label = if let Some(pat) = pattern {
        if let Some(arr) = pat.as_array() {
            arr.iter()
                .filter_map(|v| v.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        } else {
            pat.as_str().unwrap_or(perm_type).to_string()
        }
    } else {
        perm_type.to_string()
    };

    vec![json!({
        "id": composite_id,
        "method": "codex/requestApproval",
        "params": {
            "threadId": session_id,
            "type": perm_type,
            "command": [command_label],
            "rawInput": {}
        }
    })]
}

/// Build the REST body for a permission decision.
///
/// `accept` = true  → `{ response: "allow" }`
/// `accept` = false → `{ response: "deny" }`
pub(crate) fn build_permission_response(accept: bool) -> Value {
    let response = if accept { "allow" } else { "deny" };
    json!({ "response": response })
}

// ---------------------------------------------------------------------------
// Synthetic turn events
// ---------------------------------------------------------------------------

pub(crate) fn build_turn_started(session_id: &str, turn_id: &str) -> Value {
    json!({
        "method": "turn/started",
        "params": {
            "threadId": session_id,
            "turn": {
                "id": turn_id,
                "threadId": session_id
            }
        }
    })
}

pub(crate) fn build_turn_completed(session_id: &str, turn_id: &str) -> Value {
    json!({
        "method": "turn/completed",
        "params": {
            "threadId": session_id,
            "turn": {
                "id": turn_id,
                "threadId": session_id
            }
        }
    })
}

pub(crate) fn build_agent_message_completed(state: &SessionTranslationState) -> Option<Value> {
    let item_id = state.agent_message_item_id.as_ref()?;
    Some(json!({
        "method": "item/completed",
        "params": {
            "threadId": state.session_id,
            "item": {
                "id": item_id,
                "type": "agentMessage",
                "text": ""
            }
        }
    }))
}

// ---------------------------------------------------------------------------
// Tool call helpers
// ---------------------------------------------------------------------------

fn tool_kind_to_item_type(kind: &str) -> &str {
    match kind {
        "edit" | "write" | "create" => "fileChange",
        "bash" | "command" | "terminal" => "commandExecution",
        _ => "commandExecution",
    }
}

fn command_parts_from_raw_input(raw_input: &Value, fallback_title: &str) -> Vec<String> {
    if let Some(command) = raw_input.get("command") {
        if let Some(parts) = command.as_array() {
            let values: Vec<String> = parts
                .iter()
                .filter_map(|value| value.as_str())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
                .collect();
            if !values.is_empty() {
                return values;
            }
        }
        if let Some(text) = command.as_str() {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                return vec![trimmed.to_string()];
            }
        }
    }
    let fallback = fallback_title.trim();
    if fallback.is_empty() {
        Vec::new()
    } else {
        vec![fallback.to_string()]
    }
}

fn cwd_from_raw_input(raw_input: &Value) -> String {
    ["workdir", "cwd", "path"]
        .iter()
        .find_map(|key| raw_input.get(key).and_then(|value| value.as_str()))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_default()
}

fn file_path_from_raw_input(raw_input: &Value) -> Option<String> {
    ["filePath", "path"]
        .iter()
        .find_map(|key| raw_input.get(key).and_then(|value| value.as_str()))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn build_tool_item(
    item_id: &str,
    item_type: &str,
    title: &str,
    status: &str,
    raw_input: Option<&Value>,
    output: Option<&str>,
    changes_from_content: Option<Vec<Value>>,
) -> Value {
    let mut item = json!({
        "id": item_id,
        "type": item_type,
        "status": status
    });

    if item_type == "commandExecution" {
        let command_parts = raw_input
            .map(|input| command_parts_from_raw_input(input, title))
            .unwrap_or_else(|| {
                let trimmed = title.trim();
                if trimmed.is_empty() {
                    Vec::new()
                } else {
                    vec![trimmed.to_string()]
                }
            });
        if !command_parts.is_empty() {
            item["command"] = json!(command_parts);
        }
        if let Some(input) = raw_input {
            let cwd = cwd_from_raw_input(input);
            if !cwd.is_empty() {
                item["cwd"] = json!(cwd);
            }
        }
        if let Some(output_text) = output {
            if !output_text.trim().is_empty() {
                item["aggregatedOutput"] = json!(output_text);
            }
        }
        return item;
    }

    if item_type == "fileChange" {
        let mut changes = changes_from_content.unwrap_or_default();
        if changes.is_empty() {
            if let Some(input) = raw_input {
                if let Some(path) = file_path_from_raw_input(input) {
                    changes.push(json!({ "path": path, "kind": "modify" }));
                }
            }
        }
        if !changes.is_empty() {
            item["changes"] = json!(changes);
        }
        if let Some(output_text) = output {
            if !output_text.trim().is_empty() {
                item["output"] = json!(output_text);
            }
        }
        return item;
    }

    item
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_state() -> SessionTranslationState {
        let mut s = SessionTranslationState::new("ses_test123".into());
        s.start_turn("turn_1".into());
        s
    }

    #[test]
    fn text_part_produces_agent_message_delta() {
        let mut state = make_state();
        let event = json!({
            "type": "message.part.updated",
            "properties": {
                "part": {
                    "type": "text",
                    "id": "part_1",
                    "sessionID": "ses_test123",
                    "text": "Hello world"
                },
                "delta": "Hello world"
            }
        });
        let events = translate_sse_event(&event, &mut state);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["method"], "item/agentMessage/delta");
        assert_eq!(events[0]["params"]["threadId"], "ses_test123");
        assert_eq!(events[0]["params"]["delta"], "Hello world");
        // Same item ID on second call.
        let item_id = events[0]["params"]["itemId"].as_str().unwrap().to_string();
        let events2 = translate_sse_event(&event, &mut state);
        assert_eq!(events2[0]["params"]["itemId"].as_str().unwrap(), item_id);
    }

    #[test]
    fn reasoning_part_produces_reasoning_delta() {
        let mut state = make_state();
        let event = json!({
            "type": "message.part.updated",
            "properties": {
                "part": {
                    "type": "reasoning",
                    "id": "part_r1",
                    "sessionID": "ses_test123"
                },
                "delta": "Let me think..."
            }
        });
        let events = translate_sse_event(&event, &mut state);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["method"], "item/reasoning/textDelta");
        assert_eq!(events[0]["params"]["delta"], "Let me think...");
    }

    #[test]
    fn tool_part_pending_produces_item_started() {
        let mut state = make_state();
        let event = json!({
            "type": "message.part.updated",
            "properties": {
                "part": {
                    "type": "tool",
                    "id": "tc_1",
                    "sessionID": "ses_test123",
                    "tool": "bash",
                    "state": {
                        "status": "running",
                        "input": { "command": ["ls", "-la"] }
                    }
                }
            }
        });
        let events = translate_sse_event(&event, &mut state);
        assert!(events.len() >= 1);
        assert_eq!(events[0]["method"], "item/started");
        assert_eq!(events[0]["params"]["item"]["type"], "commandExecution");
    }

    #[test]
    fn tool_part_completed_produces_item_completed() {
        let mut state = make_state();
        // First register the tool.
        let running = json!({
            "type": "message.part.updated",
            "properties": {
                "part": {
                    "type": "tool",
                    "id": "tc_2",
                    "tool": "edit",
                    "state": { "status": "running", "input": { "filePath": "foo.rs" } }
                }
            }
        });
        translate_sse_event(&running, &mut state);

        let completed = json!({
            "type": "message.part.updated",
            "properties": {
                "part": {
                    "type": "tool",
                    "id": "tc_2",
                    "tool": "edit",
                    "state": {
                        "status": "completed",
                        "input": { "filePath": "foo.rs" },
                        "output": "File written."
                    }
                }
            }
        });
        let events = translate_sse_event(&completed, &mut state);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["method"], "item/completed");
        assert_eq!(events[0]["params"]["item"]["type"], "fileChange");
        assert_eq!(events[0]["params"]["item"]["status"], "completed");
    }

    #[test]
    fn message_updated_produces_token_usage() {
        let mut state = make_state();
        let event = json!({
            "type": "message.updated",
            "properties": {
                "info": {
                    "inputTokens": 5000,
                    "outputTokens": 1000,
                    "cachedInputTokens": 200,
                    "reasoningOutputTokens": 50
                }
            }
        });
        let events = translate_sse_event(&event, &mut state);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["method"], "thread/tokenUsage/updated");
        assert_eq!(
            events[0]["params"]["tokenUsage"]["total"]["totalTokens"],
            6000
        );
        assert_eq!(
            events[0]["params"]["tokenUsage"]["total"]["inputTokens"],
            5000
        );
        assert_eq!(
            events[0]["params"]["tokenUsage"]["total"]["outputTokens"],
            1000
        );
    }

    #[test]
    fn session_status_idle_produces_turn_completed() {
        let mut state = make_state();
        let event = json!({
            "type": "session.status",
            "properties": {
                "sessionID": "ses_test123",
                "status": { "type": "idle" }
            }
        });
        let events = translate_sse_event(&event, &mut state);
        assert!(events.iter().any(|e| e["method"] == "turn/completed"));
    }

    #[test]
    fn session_status_error_produces_error_event() {
        let mut state = make_state();
        let event = json!({
            "type": "session.status",
            "properties": {
                "sessionID": "ses_test123",
                "status": { "type": "error", "message": "rate limited" }
            }
        });
        let events = translate_sse_event(&event, &mut state);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["method"], "error");
        assert_eq!(events[0]["params"]["error"]["message"], "rate limited");
    }

    #[test]
    fn permission_updated_produces_approval_request() {
        let mut state = make_state();
        let event = json!({
            "type": "permission.updated",
            "properties": {
                "id": "perm_42",
                "type": "bash",
                "sessionID": "ses_test123",
                "pattern": "rm -rf /tmp/test"
            }
        });
        let events = translate_sse_event(&event, &mut state);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["method"], "codex/requestApproval");
        assert_eq!(events[0]["id"], "ses_test123:perm_42");
        assert_eq!(events[0]["params"]["type"], "bash");
    }

    #[test]
    fn turn_lifecycle_events() {
        let started = build_turn_started("ses_abc", "turn_1");
        assert_eq!(started["method"], "turn/started");
        assert_eq!(started["params"]["threadId"], "ses_abc");
        assert_eq!(started["params"]["turn"]["id"], "turn_1");

        let completed = build_turn_completed("ses_abc", "turn_1");
        assert_eq!(completed["method"], "turn/completed");
        assert_eq!(completed["params"]["turn"]["id"], "turn_1");
    }

    #[test]
    fn permission_response_shapes() {
        let accept = build_permission_response(true);
        assert_eq!(accept["response"], "allow");

        let deny = build_permission_response(false);
        assert_eq!(deny["response"], "deny");
    }

    #[test]
    fn unknown_event_type_returns_empty() {
        let mut state = make_state();
        let event = json!({
            "type": "some.unknown.event",
            "properties": {}
        });
        let events = translate_sse_event(&event, &mut state);
        assert!(events.is_empty());
    }

    #[test]
    fn chunk_text_preserves_whitespace() {
        let mut state = make_state();
        let event = json!({
            "type": "message.part.updated",
            "properties": {
                "part": {
                    "type": "text",
                    "id": "part_ws",
                    "sessionID": "ses_test123"
                },
                "delta": " line with trailing space "
            }
        });
        let events = translate_sse_event(&event, &mut state);
        assert_eq!(events[0]["params"]["delta"], " line with trailing space ");
    }
}

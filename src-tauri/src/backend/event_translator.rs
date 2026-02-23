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

/// Per-session turn state — tracks active turn and item IDs for a single session.
#[derive(Default)]
struct PerSessionTurnState {
    /// Synthesized turn ID for this session.
    turn_id: String,
    /// Map tool Part `id` → synthesized CodexMonitor `itemId`.
    tool_call_items: HashMap<String, String>,
    /// Stable item ID for the current agent-message stream.
    agent_message_item_id: Option<String>,
    /// Stable item ID for the current reasoning stream.
    reasoning_item_id: Option<String>,
    /// OpenCode part ID for the current reasoning part (used to route `message.part.delta` events).
    reasoning_part_id: Option<String>,
}

/// Per-workspace state the translator needs to synthesize IDs the frontend
/// expects but the REST SSE protocol does not provide (turn IDs, monotonic
/// item IDs, etc.).
///
/// Multiple sessions can be active in a single workspace (e.g., main session +
/// subagent sessions), so turn state is tracked per session_id.
pub(crate) struct SessionTranslationState {
    /// The most recently active session ID (maps to CodexMonitor's "threadId").
    /// Used as a fallback when events don't specify sessionID.
    pub(crate) session_id: String,
    /// Per-session turn state — allows concurrent sessions to track their own turns.
    session_turns: HashMap<String, PerSessionTurnState>,
    /// Counter for synthesizing unique item IDs (shared across all sessions).
    item_counter: AtomicU64,
    /// Stable item ID for contiguous user-message chunk streams.
    pub(crate) user_message_item_id: Option<String>,
    /// Buffered text for contiguous user-message chunk streams.
    pub(crate) user_message_text: String,
    /// Model context windows keyed by `providerID/modelID`.
    model_context_windows: HashMap<String, u64>,
}

impl SessionTranslationState {
    pub(crate) fn new(session_id: String) -> Self {
        Self {
            session_id,
            session_turns: HashMap::new(),
            item_counter: AtomicU64::new(1),
            user_message_item_id: None,
            user_message_text: String::new(),
            model_context_windows: HashMap::new(),
        }
    }

    pub(crate) fn replace_model_context_windows(&mut self, windows: HashMap<String, u64>) {
        self.model_context_windows = windows;
    }

    pub(crate) fn model_context_window(&self, provider_id: &str, model_id: &str) -> Option<u64> {
        let provider = provider_id.trim();
        let model = model_id.trim();
        if provider.is_empty() && model.is_empty() {
            return None;
        }

        if !provider.is_empty() && !model.is_empty() {
            let key = format!("{provider}/{model}");
            if let Some(value) = self.model_context_windows.get(&key) {
                return Some(*value);
            }
        }

        if model.contains('/') {
            if let Some(value) = self.model_context_windows.get(model) {
                return Some(*value);
            }
            if let Some((model_provider, model_id_only)) = model.split_once('/') {
                let qualified = format!("{model_provider}/{model_id_only}");
                if let Some(value) = self.model_context_windows.get(&qualified) {
                    return Some(*value);
                }
                if !provider.is_empty() {
                    let fallback = format!("{provider}/{model_id_only}");
                    if let Some(value) = self.model_context_windows.get(&fallback) {
                        return Some(*value);
                    }
                }
            }
        }

        None
    }

    fn next_item_id(&self) -> String {
        let n = self.item_counter.fetch_add(1, Ordering::SeqCst);
        format!("item_{n}")
    }

    /// Start a new turn for a specific session.
    pub(crate) fn start_turn(&mut self, session_id: String, turn_id: String) {
        self.session_id = session_id.clone();
        let turn_state = self.session_turns.entry(session_id).or_default();
        turn_state.turn_id = turn_id;
        turn_state.tool_call_items.clear();
        turn_state.agent_message_item_id = None;
        turn_state.reasoning_item_id = None;
        turn_state.reasoning_part_id = None;
        self.user_message_item_id = None;
        self.user_message_text.clear();
    }

    /// Prepare translation state for replaying historical messages.
    pub(crate) fn prepare_replay(&mut self, session_id: String) {
        self.session_id = session_id.clone();
        if let Some(turn_state) = self.session_turns.get_mut(&session_id) {
            turn_state.turn_id.clear();
            turn_state.tool_call_items.clear();
            turn_state.agent_message_item_id = None;
            turn_state.reasoning_item_id = None;
            turn_state.reasoning_part_id = None;
        }
        self.user_message_item_id = None;
        self.user_message_text.clear();
    }

    fn get_turn_state(&self, session_id: &str) -> Option<&PerSessionTurnState> {
        self.session_turns.get(session_id)
    }

    fn get_turn_state_mut(&mut self, session_id: &str) -> &mut PerSessionTurnState {
        self.session_turns
            .entry(session_id.to_string())
            .or_default()
    }

    fn agent_message_item(&mut self, session_id: &str) -> String {
        let turn_state = self.get_turn_state_mut(session_id);
        if let Some(ref id) = turn_state.agent_message_item_id {
            id.clone()
        } else {
            let id = self.next_item_id();
            let turn_state = self.get_turn_state_mut(session_id);
            turn_state.agent_message_item_id = Some(id.clone());
            id
        }
    }

    fn reasoning_item(&mut self, session_id: &str) -> String {
        let turn_state = self.get_turn_state_mut(session_id);
        if let Some(ref id) = turn_state.reasoning_item_id {
            id.clone()
        } else {
            let id = self.next_item_id();
            let turn_state = self.get_turn_state_mut(session_id);
            turn_state.reasoning_item_id = Some(id.clone());
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
        if let Some(turn_state) = self.session_turns.get_mut(&self.session_id) {
            turn_state.tool_call_items.clear();
            turn_state.agent_message_item_id = None;
            turn_state.reasoning_item_id = None;
            turn_state.reasoning_part_id = None;
        }
    }
}

fn parse_u64(value: Option<&Value>) -> Option<u64> {
    value
        .and_then(|v| {
            v.as_u64()
                .or_else(|| v.as_i64().and_then(|n| (n > 0).then_some(n as u64)))
        })
        .or_else(|| {
            value
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .and_then(|s| s.parse::<u64>().ok())
                .filter(|n| *n > 0)
        })
}

pub(crate) fn extract_model_context_windows(config_providers: &Value) -> HashMap<String, u64> {
    let mut windows = HashMap::new();

    let providers = config_providers
        .get("providers")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    for provider in &providers {
        let provider_id = provider
            .get("id")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .unwrap_or_default();
        if provider_id.is_empty() {
            continue;
        }

        let Some(models) = provider.get("models").and_then(|v| v.as_object()) else {
            continue;
        };

        for (fallback_model_id, model) in models {
            let model_id = model
                .get("id")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or(fallback_model_id.as_str());
            if model_id.is_empty() {
                continue;
            }

            let context = parse_u64(model.get("limit").and_then(|v| v.get("context")));
            if let Some(context) = context {
                let key = format!("{provider_id}/{model_id}");
                windows.insert(key, context);
            }
        }
    }

    windows
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
        "message.part.delta" => translate_part_delta(properties, state),
        "message.updated" => translate_message_updated(properties, state),
        "session.status" => translate_session_status(properties, state),
        "session.idle" => translate_session_idle(properties, state),
        "permission.updated" => translate_sse_permission(properties, state),
        "question.asked" => translate_question_asked(properties, state),
        "question.replied" => translate_question_completed(properties),
        "question.rejected" => translate_question_completed(properties),
        "server.heartbeat"
        | "file.watcher.updated"
        | "session.created"
        | "session.updated"
        | "session.deleted"
        | "session.diff"
        | "config.updated" => vec![],
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

    if let Some(sid) = part.get("sessionID").and_then(|v| v.as_str()) {
        if !sid.is_empty() {
            state.session_id = sid.to_string();
        }
    }
    let thread_id = state.session_id.clone();
    let turn_id = state
        .get_turn_state(&thread_id)
        .map(|ts| ts.turn_id.clone())
        .unwrap_or_default();

    match part_type {
        "text" => {
            if delta.is_empty() {
                return vec![];
            }

            let item_id = state.agent_message_item(&thread_id);
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
            if let Some(pid) = part.get("id").and_then(|v| v.as_str()) {
                let turn_state = state.get_turn_state_mut(&thread_id);
                turn_state.reasoning_part_id = Some(pid.to_string());
            }
            if delta.is_empty() {
                return vec![];
            }
            let item_id = state.reasoning_item(&thread_id);
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
// message.part.delta — incremental text/reasoning streaming chunks
// ---------------------------------------------------------------------------

fn translate_part_delta(properties: &Value, state: &mut SessionTranslationState) -> Vec<Value> {
    let delta = match properties.get("delta").and_then(|v| v.as_str()) {
        Some(d) if !d.is_empty() => d,
        _ => return vec![],
    };

    if let Some(sid) = properties.get("sessionID").and_then(|v| v.as_str()) {
        if !sid.is_empty() {
            state.session_id = sid.to_string();
        }
    }

    let field = properties
        .get("field")
        .and_then(|v| v.as_str())
        .unwrap_or("text");
    let thread_id = state.session_id.clone();
    let turn_id = state
        .get_turn_state(&thread_id)
        .map(|ts| ts.turn_id.clone())
        .unwrap_or_default();

    match field {
        "text" => {
            let part_id = properties
                .get("partID")
                .and_then(|v| v.as_str())
                .unwrap_or_default();

            let is_reasoning = state
                .get_turn_state(&thread_id)
                .and_then(|ts| ts.reasoning_part_id.as_ref())
                .map(|rpid| part_id == rpid)
                .unwrap_or(false);

            if is_reasoning {
                let item_id = state.reasoning_item(&thread_id);
                return vec![json!({
                    "method": "item/reasoning/textDelta",
                    "params": {
                        "threadId": thread_id,
                        "turnId": turn_id,
                        "itemId": item_id,
                        "delta": delta
                    }
                })];
            }

            let item_id = state.agent_message_item(&thread_id);
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

    let turn_state = state.get_turn_state_mut(thread_id);
    let item_id = if let Some(existing) = turn_state.tool_call_items.get(part_id) {
        existing.clone()
    } else {
        let id = state.next_item_id();
        if !part_id.is_empty() {
            let turn_state = state.get_turn_state_mut(thread_id);
            turn_state
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

    if let Some(sid) = info.get("sessionID").and_then(|v| v.as_str()) {
        if !sid.is_empty() {
            state.session_id = sid.to_string();
        }
    }

    let thread_id = state.session_id.clone();

    let model = info.get("model");
    let provider_id = info
        .get("providerID")
        .or_else(|| info.get("provider_id"))
        .or_else(|| model.and_then(|m| m.get("providerID")))
        .or_else(|| model.and_then(|m| m.get("provider_id")))
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let model_id = info
        .get("modelID")
        .or_else(|| info.get("model_id"))
        .or_else(|| model.and_then(|m| m.get("modelID")))
        .or_else(|| model.and_then(|m| m.get("model_id")))
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let model_context_window = state
        .model_context_window(provider_id, model_id)
        .unwrap_or(0);

    // Token usage: try nested `tokens` object first (current format), then flat keys (legacy).
    let tokens = info.get("tokens");
    let input_tokens = parse_u64(
        tokens
            .and_then(|t| t.get("input"))
            .or_else(|| info.get("inputTokens")),
    )
    .unwrap_or(0);
    let output_tokens = parse_u64(
        tokens
            .and_then(|t| t.get("output"))
            .or_else(|| info.get("outputTokens")),
    )
    .unwrap_or(0);
    let cached_tokens = parse_u64(
        tokens
            .and_then(|t| t.get("cache").and_then(|c| c.get("read")))
            .or_else(|| info.get("cachedInputTokens"))
            .or_else(|| info.get("cacheReadInputTokens")),
    )
    .unwrap_or(0);
    let reasoning_tokens = parse_u64(
        tokens
            .and_then(|t| t.get("reasoning"))
            .or_else(|| info.get("reasoningOutputTokens"))
            .or_else(|| info.get("reasoningTokens")),
    )
    .unwrap_or(0);
    let total =
        parse_u64(tokens.and_then(|t| t.get("total"))).unwrap_or(input_tokens + output_tokens);

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
                "modelContextWindow": model_context_window
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
    let turn_id = state
        .get_turn_state(&thread_id)
        .map(|ts| ts.turn_id.clone())
        .unwrap_or_default();

    match status {
        "idle" => {
            if !turn_id.is_empty() {
                let mut events = Vec::new();
                if let Some(msg_completed) = build_agent_message_completed(state, &thread_id) {
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
// session.idle — separate event type indicating session has become idle
// ---------------------------------------------------------------------------

fn translate_session_idle(properties: &Value, state: &mut SessionTranslationState) -> Vec<Value> {
    let session_id = properties
        .get("sessionID")
        .or_else(|| properties.get("session_id"))
        .or_else(|| properties.get("id"))
        .and_then(|v| v.as_str())
        .unwrap_or_default();

    if !session_id.is_empty() {
        state.session_id = session_id.to_string();
    }

    let thread_id = state.session_id.clone();
    if thread_id.is_empty() {
        return vec![];
    }

    let turn_id = state
        .get_turn_state(&thread_id)
        .map(|ts| ts.turn_id.clone())
        .unwrap_or_default();

    let mut events = Vec::new();
    if let Some(msg_completed) = build_agent_message_completed(state, &thread_id) {
        events.push(msg_completed);
    }
    // Emit turn/completed even if turn_id is empty - background prompts don't track turns
    events.push(build_turn_completed(&thread_id, &turn_id));
    events
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
// question.asked — user input request from Claude (mcp_question tool)
// ---------------------------------------------------------------------------

fn translate_question_asked(properties: &Value, state: &mut SessionTranslationState) -> Vec<Value> {
    let question_id = properties
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let session_id = properties
        .get("sessionID")
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_else(|| state.session_id.clone());

    if question_id.is_empty() {
        return vec![];
    }

    if !session_id.is_empty() {
        state.session_id = session_id.clone();
    }

    let thread_id = state.session_id.clone();
    let turn_id = state
        .get_turn_state(&thread_id)
        .map(|ts| ts.turn_id.clone())
        .unwrap_or_default();
    let item_id = state.next_item_id();

    // Composite ID for response routing: "sessionId:questionId"
    let composite_id = format!("{session_id}:{question_id}");

    // Transform questions array to frontend-expected shape
    let questions: Vec<Value> = properties
        .get("questions")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .enumerate()
                .map(|(idx, q)| {
                    let header = q.get("header").and_then(|v| v.as_str()).unwrap_or_default();
                    let question_text = q
                        .get("question")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default();
                    // OpenCode uses "custom" field to indicate if custom input is allowed
                    let is_other = q.get("custom").and_then(|v| v.as_bool()).unwrap_or(true);

                    let options: Vec<Value> = q
                        .get("options")
                        .and_then(|v| v.as_array())
                        .map(|opts| {
                            opts.iter()
                                .map(|opt| {
                                    json!({
                                        "label": opt.get("label").and_then(|v| v.as_str()).unwrap_or_default(),
                                        "description": opt.get("description").and_then(|v| v.as_str()).unwrap_or_default()
                                    })
                                })
                                .collect()
                        })
                        .unwrap_or_default();

                    json!({
                        "id": idx.to_string(),
                        "header": header,
                        "question": question_text,
                        "isOther": is_other,
                        "options": options
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    vec![json!({
        "id": composite_id,
        "method": "item/tool/requestUserInput",
        "params": {
            "threadId": thread_id,
            "turnId": turn_id,
            "itemId": item_id,
            "questions": questions
        }
    })]
}

// ---------------------------------------------------------------------------
// question.replied / question.rejected — cleanup after user responds/dismisses
// ---------------------------------------------------------------------------

fn translate_question_completed(properties: &Value) -> Vec<Value> {
    let session_id = properties
        .get("sessionID")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let request_id = properties
        .get("requestID")
        .and_then(|v| v.as_str())
        .unwrap_or_default();

    if request_id.is_empty() {
        return vec![];
    }

    let composite_id = format!("{session_id}:{request_id}");

    vec![json!({
        "method": "item/tool/userInputCompleted",
        "params": {
            "requestId": composite_id,
            "workspaceId": session_id
        }
    })]
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

pub(crate) fn build_agent_message_completed(
    state: &SessionTranslationState,
    session_id: &str,
) -> Option<Value> {
    let turn_state = state.get_turn_state(session_id)?;
    let item_id = turn_state.agent_message_item_id.as_ref()?;
    Some(json!({
        "method": "item/completed",
        "params": {
            "threadId": session_id,
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
    use std::collections::HashMap;

    fn make_state() -> SessionTranslationState {
        let mut s = SessionTranslationState::new("ses_test123".into());
        s.start_turn("ses_test123".into(), "turn_1".into());
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
    fn message_updated_uses_model_context_window_when_available() {
        let mut state = make_state();
        let mut windows = HashMap::new();
        windows.insert("anthropic/claude-sonnet-4-5".to_string(), 200_000);
        state.replace_model_context_windows(windows);

        let event = json!({
            "type": "message.updated",
            "properties": {
                "info": {
                    "providerID": "anthropic",
                    "modelID": "claude-sonnet-4-5",
                    "tokens": {
                        "input": 1200,
                        "output": 300,
                        "reasoning": 50,
                        "cache": { "read": 25, "write": 0 }
                    }
                }
            }
        });

        let events = translate_sse_event(&event, &mut state);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["method"], "thread/tokenUsage/updated");
        assert_eq!(
            events[0]["params"]["tokenUsage"]["modelContextWindow"],
            200_000
        );
    }

    #[test]
    fn extract_model_context_windows_reads_provider_limits() {
        let payload = json!({
            "providers": [
                {
                    "id": "anthropic",
                    "models": {
                        "claude-sonnet-4-5": {
                            "id": "claude-sonnet-4-5",
                            "limit": {
                                "context": 200000,
                                "output": 8192
                            }
                        },
                        "claude-haiku-4-5": {
                            "id": "claude-haiku-4-5",
                            "limit": {
                                "context": "100000"
                            }
                        }
                    }
                }
            ]
        });

        let windows = extract_model_context_windows(&payload);
        assert_eq!(windows.get("anthropic/claude-sonnet-4-5"), Some(&200_000));
        assert_eq!(windows.get("anthropic/claude-haiku-4-5"), Some(&100_000));
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
    fn concurrent_sessions_get_correct_turn_ids() {
        let mut state = SessionTranslationState::new(String::new());

        // Session A starts a turn
        state.start_turn("ses_A".into(), "turn_A1".into());
        // Session B starts a turn (should NOT clobber A's turn)
        state.start_turn("ses_B".into(), "turn_B1".into());

        // Idle event for session A should use turn_A1, not turn_B1
        let event_a = json!({
            "type": "session.status",
            "properties": {
                "sessionID": "ses_A",
                "status": { "type": "idle" }
            }
        });
        let events = translate_sse_event(&event_a, &mut state);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["method"], "turn/completed");
        assert_eq!(events[0]["params"]["threadId"], "ses_A");
        assert_eq!(events[0]["params"]["turn"]["id"], "turn_A1");

        // Idle event for session B should use turn_B1
        let event_b = json!({
            "type": "session.status",
            "properties": {
                "sessionID": "ses_B",
                "status": { "type": "idle" }
            }
        });
        let events = translate_sse_event(&event_b, &mut state);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["params"]["turn"]["id"], "turn_B1");
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

    #[test]
    fn part_delta_text_produces_agent_message_delta() {
        let mut state = make_state();
        let event = json!({
            "type": "message.part.delta",
            "properties": {
                "sessionID": "ses_test123",
                "messageID": "msg_1",
                "partID": "prt_text_1",
                "field": "text",
                "delta": "hello"
            }
        });
        let events = translate_sse_event(&event, &mut state);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["method"], "item/agentMessage/delta");
        assert_eq!(events[0]["params"]["threadId"], "ses_test123");
        assert_eq!(events[0]["params"]["delta"], "hello");
    }

    #[test]
    fn part_delta_routes_reasoning_by_part_id() {
        let mut state = make_state();

        // First, announce a reasoning part via message.part.updated.
        let reasoning_announce = json!({
            "type": "message.part.updated",
            "properties": {
                "part": {
                    "type": "reasoning",
                    "id": "prt_reasoning_1",
                    "sessionID": "ses_test123",
                    "text": ""
                }
            }
        });
        translate_sse_event(&reasoning_announce, &mut state);

        // Now a delta for that reasoning part should produce reasoning event.
        let delta_event = json!({
            "type": "message.part.delta",
            "properties": {
                "sessionID": "ses_test123",
                "partID": "prt_reasoning_1",
                "field": "text",
                "delta": "thinking..."
            }
        });
        let events = translate_sse_event(&delta_event, &mut state);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["method"], "item/reasoning/textDelta");
        assert_eq!(events[0]["params"]["delta"], "thinking...");
    }

    #[test]
    fn nested_token_format_produces_token_usage() {
        let mut state = make_state();
        let event = json!({
            "type": "message.updated",
            "properties": {
                "info": {
                    "sessionID": "ses_test123",
                    "tokens": {
                        "total": 6000,
                        "input": 5000,
                        "output": 1000,
                        "reasoning": 50,
                        "cache": { "read": 200, "write": 0 }
                    }
                }
            }
        });
        let events = translate_sse_event(&event, &mut state);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["method"], "thread/tokenUsage/updated");
        assert_eq!(
            events[0]["params"]["tokenUsage"]["total"]["inputTokens"],
            5000
        );
        assert_eq!(
            events[0]["params"]["tokenUsage"]["total"]["outputTokens"],
            1000
        );
        assert_eq!(
            events[0]["params"]["tokenUsage"]["total"]["cachedInputTokens"],
            200
        );
        assert_eq!(
            events[0]["params"]["tokenUsage"]["total"]["reasoningOutputTokens"],
            50
        );
    }

    #[test]
    fn part_delta_empty_delta_returns_empty() {
        let mut state = make_state();
        let event = json!({
            "type": "message.part.delta",
            "properties": {
                "sessionID": "ses_test123",
                "partID": "prt_1",
                "field": "text",
                "delta": ""
            }
        });
        let events = translate_sse_event(&event, &mut state);
        assert!(events.is_empty());
    }
}

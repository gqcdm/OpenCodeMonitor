//! Translates ACP `session/update` notifications into CodexMonitor-shaped
//! `AppServerEvent` messages that the React frontend already knows how to
//! consume.
//!
//! Design invariant (INV4 from spec): the frontend thread reducer receives
//! events in the **same shape** as the original CodexMonitor protocol. All
//! ACP ↔ CodexMonitor translation happens here in Rust.

use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

/// Per-session state the translator needs to synthesize IDs the frontend
/// expects but ACP does not provide (turn IDs, monotonic item IDs, etc.).
pub(crate) struct SessionTranslationState {
    /// The session ID from ACP (maps to CodexMonitor's "threadId").
    pub(crate) session_id: String,
    /// Synthesized turn ID.  Incremented each time `session/prompt` is called.
    pub(crate) current_turn_id: String,
    /// Counter for synthesizing unique item IDs.
    item_counter: AtomicU64,
    /// Map ACP `toolCallId` → synthesized CodexMonitor `itemId`.
    tool_call_items: HashMap<String, String>,
    /// Stable item ID for the current agent-message stream.
    agent_message_item_id: Option<String>,
    /// Stable item ID for the current reasoning stream.
    reasoning_item_id: Option<String>,
    /// Stable item ID for contiguous user-message chunk streams.
    user_message_item_id: Option<String>,
    /// Buffered text for contiguous user-message chunk streams.
    user_message_text: String,
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

    /// Start a new turn (called before `session/prompt`).  Resets per-turn
    /// ephemeral state so the next batch of events gets fresh item IDs.
    pub(crate) fn start_turn(&mut self, turn_id: String) {
        self.current_turn_id = turn_id;
        self.tool_call_items.clear();
        self.agent_message_item_id = None;
        self.reasoning_item_id = None;
        self.user_message_item_id = None;
        self.user_message_text.clear();
    }

    /// Prepare translation state for replaying historical session updates.
    pub(crate) fn prepare_replay(&mut self, session_id: String) {
        self.session_id = session_id;
        self.current_turn_id.clear();
        self.tool_call_items.clear();
        self.agent_message_item_id = None;
        self.reasoning_item_id = None;
        self.user_message_item_id = None;
        self.user_message_text.clear();
    }

    /// Get or create the stable item-id for the agent message stream within
    /// the current turn.
    fn agent_message_item(&mut self) -> String {
        if let Some(ref id) = self.agent_message_item_id {
            id.clone()
        } else {
            let id = self.next_item_id();
            self.agent_message_item_id = Some(id.clone());
            id
        }
    }

    /// Get or create the stable item-id for the reasoning stream within
    /// the current turn.
    fn reasoning_item(&mut self) -> String {
        if let Some(ref id) = self.reasoning_item_id {
            id.clone()
        } else {
            let id = self.next_item_id();
            self.reasoning_item_id = Some(id.clone());
            id
        }
    }

    fn user_message_item(&mut self) -> String {
        if let Some(ref id) = self.user_message_item_id {
            id.clone()
        } else {
            let id = self.next_item_id();
            self.user_message_item_id = Some(id.clone());
            id
        }
    }

    fn mark_new_replayed_user_message_boundary(&mut self) {
        // ACP replay has no turn lifecycle markers. Treat each replayed user
        // message as a boundary so assistant/reasoning streams do not merge
        // across historical turns.
        self.tool_call_items.clear();
        self.agent_message_item_id = None;
        self.reasoning_item_id = None;
    }
}

// ---------------------------------------------------------------------------
// Public translation entry point
// ---------------------------------------------------------------------------

/// Attempt to translate an ACP `session/update` notification into one or more
/// CodexMonitor-shaped JSON-RPC messages (method + params).
///
/// Returns `None` when the event should be silently dropped (e.g.
/// `available_commands_update` which has no frontend equivalent).
///
/// Some ACP events map to *multiple* CodexMonitor events (e.g. a tool_call
/// with status "pending" emits both an `item/started` and potentially a delta).
/// The caller should emit all returned values in order.
pub(crate) fn translate_acp_event(
    acp_notification: &Value,
    state: &mut SessionTranslationState,
) -> Vec<Value> {
    // ACP notifications look like:
    // { "method": "session/update",
    //   "params": { "sessionId": "...",
    //               "update": { "sessionUpdate": "<type>", ...payload } } }
    let params = match acp_notification.get("params") {
        Some(p) => p,
        None => return vec![],
    };
    let update = match params.get("update") {
        Some(u) => u,
        None => return vec![],
    };
    let update_type = match update.get("sessionUpdate").and_then(|v| v.as_str()) {
        Some(t) => t,
        None => return vec![],
    };

    let event_session_id = params
        .get("sessionId")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    if !event_session_id.is_empty() {
        state.session_id = event_session_id.to_string();
    }
    let thread_id = if !event_session_id.is_empty() {
        event_session_id.to_string()
    } else {
        state.session_id.clone()
    };
    let turn_id = state.current_turn_id.clone();

    if update_type != "user_message_chunk" {
        state.user_message_item_id = None;
        state.user_message_text.clear();
    }

    match update_type {
        "user_message_chunk" => {
            // Foreground sends emit synthetic user items before session/prompt.
            // Translate ACP user chunks only for replay to avoid duplicates.
            if !state.current_turn_id.is_empty() {
                return vec![];
            }
            let text = extract_chunk_text(update);
            if text.is_empty() {
                return vec![];
            }
            if state.user_message_item_id.is_none() {
                state.mark_new_replayed_user_message_boundary();
            }
            let item_id = state.user_message_item();
            state.user_message_text.push_str(&text);
            vec![json!({
                "method": "item/completed",
                "params": {
                    "threadId": thread_id,
                    "item": {
                        "id": item_id,
                        "type": "userMessage",
                        "content": [
                            {
                                "type": "text",
                                "text": state.user_message_text
                            }
                        ]
                    }
                }
            })]
        }

        "agent_message_chunk" => {
            let text = extract_chunk_text(update);
            if text.is_empty() {
                return vec![];
            }
            let item_id = state.agent_message_item();
            vec![json!({
                "method": "item/agentMessage/delta",
                "params": {
                    "threadId": thread_id,
                    "turnId": turn_id,
                    "itemId": item_id,
                    "delta": text
                }
            })]
        }

        "agent_thought_chunk" => {
            let text = extract_chunk_text(update);
            if text.is_empty() {
                return vec![];
            }
            let item_id = state.reasoning_item();
            vec![json!({
                "method": "item/reasoning/textDelta",
                "params": {
                    "threadId": thread_id,
                    "turnId": turn_id,
                    "itemId": item_id,
                    "delta": text
                }
            })]
        }

        // -----------------------------------------------------------------
        // Tool call lifecycle: pending → in_progress → completed|failed
        // -----------------------------------------------------------------
        "tool_call" => translate_tool_call(update, state, &thread_id),
        "tool_call_update" => translate_tool_call_update(update, state, &thread_id),

        // -----------------------------------------------------------------
        // Usage / token tracking
        // -----------------------------------------------------------------
        "usage_update" => {
            // ACP shape: { used, size, cost: { amount, currency } }
            let used = update.get("used").and_then(|v| v.as_u64()).unwrap_or(0);
            let size = update.get("size").and_then(|v| v.as_u64()).unwrap_or(0);
            vec![json!({
                "method": "thread/tokenUsage/updated",
                "params": {
                    "threadId": thread_id,
                    "tokenUsage": {
                        "total": {
                            "totalTokens": used,
                            "inputTokens": used,
                            "cachedInputTokens": 0,
                            "outputTokens": 0,
                            "reasoningOutputTokens": 0
                        },
                        "last": {
                            "totalTokens": used,
                            "inputTokens": used,
                            "cachedInputTokens": 0,
                            "outputTokens": 0,
                            "reasoningOutputTokens": 0
                        },
                        "modelContextWindow": size
                    }
                }
            })]
        }

        "plan" => {
            let explanation = update
                .get("explanation")
                .cloned()
                .or_else(|| update.get("text").cloned())
                .unwrap_or(Value::Null);
            let plan = update
                .get("plan")
                .cloned()
                .or_else(|| update.get("steps").cloned())
                .unwrap_or(Value::Null);
            vec![json!({
                "method": "turn/plan/updated",
                "params": {
                    "threadId": thread_id,
                    "turnId": turn_id,
                    "explanation": explanation,
                    "plan": plan
                }
            })]
        }

        // -----------------------------------------------------------------
        // Events we intentionally drop for MVP
        // -----------------------------------------------------------------
        "available_commands_update" => vec![],

        // Unknown event type — drop silently but log in debug builds.
        other => {
            #[cfg(debug_assertions)]
            eprintln!("[event_translator] unknown sessionUpdate type: {other}");
            vec![]
        }
    }
}

// ---------------------------------------------------------------------------
// Synthetic turn events
// ---------------------------------------------------------------------------

/// Build a synthetic `turn/started` message.
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

/// Build a synthetic `turn/completed` message.
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

/// Build a synthetic `item/completed` for the agent message item at turn end.
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

/// Map ACP tool `kind` to CodexMonitor item type.
fn tool_kind_to_item_type(kind: &str) -> &str {
    match kind {
        "edit" | "write" | "create" => "fileChange",
        "bash" | "command" | "terminal" => "commandExecution",
        _ => "commandExecution", // default to commandExecution for unknown kinds
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

fn parse_file_changes_from_content(update: &Value) -> Vec<Value> {
    let content = match update.get("content").and_then(|value| value.as_array()) {
        Some(content) => content,
        None => return Vec::new(),
    };
    content
        .iter()
        .filter(|entry| entry.get("type").and_then(|value| value.as_str()) == Some("diff"))
        .filter_map(|entry| {
            let path = entry.get("path").and_then(|value| value.as_str())?.trim();
            if path.is_empty() {
                return None;
            }
            let old_text = entry
                .get("oldText")
                .and_then(|value| value.as_str())
                .unwrap_or_default();
            let new_text = entry
                .get("newText")
                .and_then(|value| value.as_str())
                .unwrap_or_default();
            let kind = if old_text.is_empty() && !new_text.is_empty() {
                "add"
            } else if !old_text.is_empty() && new_text.is_empty() {
                "delete"
            } else {
                "modify"
            };
            let diff = if old_text.is_empty() && new_text.is_empty() {
                None
            } else {
                let mut lines = Vec::new();
                lines.push(format!("--- {path}"));
                lines.push(format!("+++ {path}"));
                for line in old_text.lines() {
                    lines.push(format!("-{line}"));
                }
                for line in new_text.lines() {
                    lines.push(format!("+{line}"));
                }
                Some(lines.join("\n"))
            };
            Some(json!({
                "path": path,
                "kind": kind,
                "diff": diff
            }))
        })
        .collect()
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

fn translate_tool_call(
    update: &Value,
    state: &mut SessionTranslationState,
    thread_id: &str,
) -> Vec<Value> {
    let tool_call_id = update
        .get("toolCallId")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let title = update
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let kind = update
        .get("kind")
        .and_then(|v| v.as_str())
        .unwrap_or("command");
    let status = update
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("pending");

    // Assign a stable item ID for this tool call.
    let item_id = state.next_item_id();
    state
        .tool_call_items
        .insert(tool_call_id.to_string(), item_id.clone());

    let item_type = tool_kind_to_item_type(kind);

    if status == "pending" {
        // Emit item/started
        let item = build_tool_item(&item_id, item_type, title, "in_progress", None, None, None);
        vec![json!({
            "method": "item/started",
            "params": {
                "threadId": thread_id,
                "item": item
            }
        })]
    } else {
        vec![]
    }
}

fn translate_tool_call_update(
    update: &Value,
    state: &mut SessionTranslationState,
    thread_id: &str,
) -> Vec<Value> {
    let tool_call_id = update
        .get("toolCallId")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let status = update.get("status").and_then(|v| v.as_str()).unwrap_or("");

    let item_id = match state.tool_call_items.get(tool_call_id) {
        Some(id) => id.clone(),
        None => {
            // We haven't seen the initial tool_call for this ID.  Synthesize one.
            let id = state.next_item_id();
            state
                .tool_call_items
                .insert(tool_call_id.to_string(), id.clone());
            id
        }
    };

    let kind = update
        .get("kind")
        .and_then(|v| v.as_str())
        .unwrap_or("command");
    let item_type = tool_kind_to_item_type(kind);
    let title = update
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or_default();

    let mut events = Vec::new();

    match status {
        "in_progress" => {
            let raw_input = update.get("rawInput");
            let started_item = build_tool_item(
                &item_id,
                item_type,
                title,
                "in_progress",
                raw_input,
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

            // Emit output delta if there's rawInput to show.
            if let Some(input) = raw_input {
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
            // Extract output text from content array.
            let output_text = extract_tool_output(update);
            let item = build_tool_item(
                &item_id,
                item_type,
                title,
                "completed",
                update.get("rawInput"),
                Some(&output_text),
                Some(parse_file_changes_from_content(update)),
            );
            events.push(json!({
                "method": "item/completed",
                "params": {
                    "threadId": thread_id,
                    "item": item
                }
            }));
        }
        "failed" => {
            let error_text = extract_tool_output(update);
            let item = build_tool_item(
                &item_id,
                item_type,
                title,
                "failed",
                update.get("rawInput"),
                Some(&error_text),
                Some(parse_file_changes_from_content(update)),
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

/// Extract text output from an ACP tool_call_update's `content` array.
fn extract_tool_output(update: &Value) -> String {
    let content = match update.get("content").and_then(|v| v.as_array()) {
        Some(arr) => arr,
        None => return String::new(),
    };
    let mut parts = Vec::new();
    for entry in content {
        // Entries can be: { type: "content", content: { type: "text", text: "..." } }
        //                  { type: "diff", path, oldText, newText }
        if let Some(inner) = entry.get("content") {
            if let Some(text) = inner.get("text").and_then(|v| v.as_str()) {
                parts.push(text.to_string());
            }
        } else if entry.get("type").and_then(|v| v.as_str()) == Some("diff") {
            // Format diff for display.
            let path = entry.get("path").and_then(|v| v.as_str()).unwrap_or("");
            let old = entry.get("oldText").and_then(|v| v.as_str()).unwrap_or("");
            let new = entry.get("newText").and_then(|v| v.as_str()).unwrap_or("");
            if !path.is_empty() {
                parts.push(format!("--- {path}\n+++ {path}"));
            }
            if !old.is_empty() || !new.is_empty() {
                // Simple unified-ish diff representation.
                for line in old.lines() {
                    parts.push(format!("-{line}"));
                }
                for line in new.lines() {
                    parts.push(format!("+{line}"));
                }
            }
        }
    }
    parts.join("\n")
}

fn extract_chunk_text(update: &Value) -> String {
    if let Some(text) = update.get("text").and_then(|v| v.as_str()) {
        if !text.is_empty() {
            return text.to_string();
        }
    }

    let content = match update.get("content") {
        Some(content) => content,
        None => return String::new(),
    };

    if let Some(text) = content.get("text").and_then(|v| v.as_str()) {
        return text.to_string();
    }

    if let Some(parts) = content.as_array() {
        let mut text_parts = Vec::new();
        for part in parts {
            if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
                if !text.is_empty() {
                    text_parts.push(text);
                }
            }
        }
        return text_parts.join("");
    }

    String::new()
}

// ---------------------------------------------------------------------------
// Permission / approval translation
// ---------------------------------------------------------------------------

/// Translate an ACP `requestPermission` JSON-RPC request into a
/// CodexMonitor-shaped approval request that the frontend already handles.
///
/// Returns `None` if the message doesn't look like a permission request.
pub(crate) fn translate_permission_request(acp_request: &Value, session_id: &str) -> Option<Value> {
    let method = acp_request.get("method").and_then(|v| v.as_str())?;
    if method != "requestPermission" {
        return None;
    }
    let id = acp_request.get("id")?;
    let params = acp_request.get("params")?;
    let tool_call = params.get("toolCall")?;

    let kind = tool_call
        .get("kind")
        .and_then(|v| v.as_str())
        .unwrap_or("command");
    let title = tool_call
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let raw_input = tool_call.get("rawInput").cloned().unwrap_or(json!({}));

    // Build a CodexMonitor-shaped approval request.
    // The frontend checks method.endsWith("requestApproval").
    Some(json!({
        "id": id,
        "method": "codex/requestApproval",
        "params": {
            "threadId": session_id,
            "type": kind,
            "command": [title],
            "rawInput": raw_input
        }
    }))
}

/// Build the ACP response for a permission decision.
///
/// `accept` = true  → `{ outcome: "selected", optionId: "once" }`
/// `accept` = false → `{ outcome: "selected", optionId: "reject" }`
pub(crate) fn build_permission_response(accept: bool) -> Value {
    let option_id = if accept { "once" } else { "reject" };
    json!({
        "outcome": {
            "outcome": "selected",
            "optionId": option_id
        }
    })
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
    fn agent_message_chunk_produces_delta() {
        let mut state = make_state();
        let notification = json!({
            "method": "session/update",
            "params": {
                "sessionId": "ses_test123",
                "update": {
                    "sessionUpdate": "agent_message_chunk",
                    "content": {
                        "type": "text",
                        "text": "Hello world"
                    }
                }
            }
        });
        let events = translate_acp_event(&notification, &mut state);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["method"], "item/agentMessage/delta");
        assert_eq!(events[0]["params"]["threadId"], "ses_test123");
        assert_eq!(events[0]["params"]["delta"], "Hello world");
        // Same item ID on second call.
        let item_id = events[0]["params"]["itemId"].as_str().unwrap().to_string();
        let events2 = translate_acp_event(&notification, &mut state);
        assert_eq!(events2[0]["params"]["itemId"].as_str().unwrap(), item_id);
    }

    #[test]
    fn agent_thought_chunk_produces_reasoning_delta() {
        let mut state = make_state();
        let notification = json!({
            "method": "session/update",
            "params": {
                "sessionId": "ses_test123",
                "update": {
                    "sessionUpdate": "agent_thought_chunk",
                    "content": {
                        "type": "text",
                        "text": "Let me think..."
                    }
                }
            }
        });
        let events = translate_acp_event(&notification, &mut state);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["method"], "item/reasoning/textDelta");
        assert_eq!(events[0]["params"]["delta"], "Let me think...");
    }

    #[test]
    fn tool_call_pending_produces_item_started() {
        let mut state = make_state();
        let notification = json!({
            "method": "session/update",
            "params": {
                "sessionId": "ses_test123",
                "update": {
                    "sessionUpdate": "tool_call",
                    "toolCallId": "tc_1",
                    "title": "read",
                    "kind": "read",
                    "status": "pending"
                }
            }
        });
        let events = translate_acp_event(&notification, &mut state);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["method"], "item/started");
        assert_eq!(events[0]["params"]["item"]["type"], "commandExecution");
        assert_eq!(events[0]["params"]["item"]["command"][0], "read");
    }

    #[test]
    fn tool_call_update_completed_produces_item_completed() {
        let mut state = make_state();
        // First, register the tool call.
        let tc = json!({
            "method": "session/update",
            "params": {
                "sessionId": "ses_test123",
                "update": {
                    "sessionUpdate": "tool_call",
                    "toolCallId": "tc_2",
                    "title": "write",
                    "kind": "edit",
                    "status": "pending"
                }
            }
        });
        translate_acp_event(&tc, &mut state);

        let update = json!({
            "method": "session/update",
            "params": {
                "sessionId": "ses_test123",
                "update": {
                    "sessionUpdate": "tool_call_update",
                    "toolCallId": "tc_2",
                    "kind": "edit",
                    "title": "write",
                    "status": "completed",
                    "content": [
                        { "type": "content", "content": { "type": "text", "text": "File written." } }
                    ]
                }
            }
        });
        let events = translate_acp_event(&update, &mut state);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["method"], "item/completed");
        assert_eq!(events[0]["params"]["item"]["type"], "fileChange");
        assert_eq!(events[0]["params"]["item"]["status"], "completed");
    }

    #[test]
    fn usage_update_produces_token_usage() {
        let mut state = make_state();
        let notification = json!({
            "method": "session/update",
            "params": {
                "sessionId": "ses_test123",
                "update": {
                    "sessionUpdate": "usage_update",
                    "used": 5000,
                    "size": 200000,
                    "cost": { "amount": 0.05, "currency": "USD" }
                }
            }
        });
        let events = translate_acp_event(&notification, &mut state);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["method"], "thread/tokenUsage/updated");
        assert_eq!(
            events[0]["params"]["tokenUsage"]["total"]["totalTokens"],
            5000
        );
        assert_eq!(
            events[0]["params"]["tokenUsage"]["modelContextWindow"],
            200000
        );
    }

    #[test]
    fn user_message_chunk_produces_user_message_item_in_replay_mode() {
        let mut state = SessionTranslationState::new("ses_test123".into());
        // Replay mode has no active synthetic turn id.
        state.prepare_replay("ses_test123".into());
        let notification = json!({
            "method": "session/update",
            "params": {
                "sessionId": "ses_test123",
                "update": {
                    "sessionUpdate": "user_message_chunk",
                    "text": "Read the file"
                }
            }
        });

        let events = translate_acp_event(&notification, &mut state);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["method"], "item/completed");
        assert_eq!(events[0]["params"]["item"]["type"], "userMessage");
        assert_eq!(
            events[0]["params"]["item"]["content"][0]["text"],
            "Read the file"
        );
    }

    #[test]
    fn plan_update_maps_to_turn_plan_updated() {
        let mut state = make_state();
        let notification = json!({
            "method": "session/update",
            "params": {
                "sessionId": "ses_test123",
                "update": {
                    "sessionUpdate": "plan",
                    "explanation": "Plan for implementation",
                    "plan": [
                        { "description": "Step 1", "status": "pending" }
                    ]
                }
            }
        });

        let events = translate_acp_event(&notification, &mut state);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["method"], "turn/plan/updated");
        assert_eq!(events[0]["params"]["threadId"], "ses_test123");
        assert_eq!(events[0]["params"]["turnId"], "turn_1");
        assert_eq!(
            events[0]["params"]["explanation"],
            "Plan for implementation"
        );
        assert_eq!(events[0]["params"]["plan"][0]["description"], "Step 1");
    }

    #[test]
    fn chunk_text_preserves_whitespace() {
        let mut state = make_state();
        let notification = json!({
            "method": "session/update",
            "params": {
                "sessionId": "ses_test123",
                "update": {
                    "sessionUpdate": "agent_message_chunk",
                    "content": {
                        "type": "text",
                        "text": " line with trailing space "
                    }
                }
            }
        });

        let events = translate_acp_event(&notification, &mut state);
        assert_eq!(events[0]["params"]["delta"], " line with trailing space ");
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
    fn permission_request_translation() {
        let acp_req = json!({
            "jsonrpc": "2.0",
            "id": 42,
            "method": "requestPermission",
            "params": {
                "sessionId": "ses_abc",
                "toolCall": {
                    "toolCallId": "tc_99",
                    "status": "pending",
                    "title": "bash",
                    "rawInput": { "command": "rm -rf /tmp/test" },
                    "kind": "bash",
                    "locations": []
                },
                "options": [
                    { "optionId": "once", "kind": "allow", "name": "Allow once" },
                    { "optionId": "always", "kind": "allow", "name": "Always allow" },
                    { "optionId": "reject", "kind": "deny", "name": "Deny" }
                ]
            }
        });
        let result = translate_permission_request(&acp_req, "ses_abc").unwrap();
        assert_eq!(result["method"], "codex/requestApproval");
        assert_eq!(result["id"], 42);
        assert_eq!(result["params"]["type"], "bash");
    }

    #[test]
    fn permission_response_shapes() {
        let accept = build_permission_response(true);
        assert_eq!(accept["outcome"]["optionId"], "once");

        let deny = build_permission_response(false);
        assert_eq!(deny["outcome"]["optionId"], "reject");
    }

    #[test]
    fn dropped_events_return_empty() {
        let mut state = make_state();
        let notification = json!({
            "method": "session/update",
            "params": {
                "sessionId": "ses_test123",
                "update": {
                    "sessionUpdate": "available_commands_update",
                    "availableCommands": []
                }
            }
        });
        let events = translate_acp_event(&notification, &mut state);
        assert!(events.is_empty());
    }
}

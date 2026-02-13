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

    match update_type {
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
        "tool_call" => translate_tool_call(update, state),
        "tool_call_update" => translate_tool_call_update(update, state),

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
                        "totalTokens": used,
                        "inputTokens": used,
                        "outputTokens": 0,
                        "contextWindowSize": size
                    }
                }
            })]
        }

        // -----------------------------------------------------------------
        // Events we intentionally drop for MVP
        // -----------------------------------------------------------------
        "user_message_chunk" | "available_commands_update" => vec![],

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

fn translate_tool_call(update: &Value, state: &mut SessionTranslationState) -> Vec<Value> {
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
        vec![json!({
            "method": "item/started",
            "params": {
                "threadId": state.session_id,
                "item": {
                    "id": item_id,
                    "type": item_type,
                    "title": title,
                    "status": "in_progress"
                }
            }
        })]
    } else {
        vec![]
    }
}

fn translate_tool_call_update(update: &Value, state: &mut SessionTranslationState) -> Vec<Value> {
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
            // Emit output delta if there's rawInput to show.
            let raw_input = update.get("rawInput");
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
                            "threadId": state.session_id,
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
            events.push(json!({
                "method": "item/completed",
                "params": {
                    "threadId": state.session_id,
                    "item": {
                        "id": item_id,
                        "type": item_type,
                        "title": title,
                        "status": "completed",
                        "output": output_text
                    }
                }
            }));
        }
        "failed" => {
            let error_text = extract_tool_output(update);
            events.push(json!({
                "method": "item/completed",
                "params": {
                    "threadId": state.session_id,
                    "item": {
                        "id": item_id,
                        "type": item_type,
                        "title": title,
                        "status": "failed",
                        "output": error_text
                    }
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
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }

    let content = match update.get("content") {
        Some(content) => content,
        None => return String::new(),
    };

    if let Some(text) = content.get("text").and_then(|v| v.as_str()) {
        return text.trim().to_string();
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
        assert_eq!(events[0]["params"]["item"]["title"], "read");
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
        assert_eq!(events[0]["params"]["tokenUsage"]["totalTokens"], 5000);
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

use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::{oneshot, Mutex};

use crate::backend::app_server::WorkspaceSession;
use crate::backend::event_translator;
use crate::backend::events::{AppServerEvent, EventSink};
use crate::codex::config as codex_config;
use crate::codex::home::{resolve_default_codex_home, resolve_workspace_codex_home};
use crate::rules;
use crate::shared::account::{build_account_response, read_auth_account};
use crate::types::WorkspaceEntry;

pub(crate) enum CodexLoginCancelState {
    PendingStart(oneshot::Sender<()>),
    LoginId(String),
}

async fn get_session_clone(
    sessions: &Mutex<HashMap<String, Arc<WorkspaceSession>>>,
    workspace_id: &str,
) -> Result<Arc<WorkspaceSession>, String> {
    let sessions = sessions.lock().await;
    sessions
        .get(workspace_id)
        .cloned()
        .ok_or_else(|| "workspace not connected".to_string())
}

async fn resolve_workspace_and_parent(
    workspaces: &Mutex<HashMap<String, WorkspaceEntry>>,
    workspace_id: &str,
) -> Result<(WorkspaceEntry, Option<WorkspaceEntry>), String> {
    let workspaces = workspaces.lock().await;
    let entry = workspaces
        .get(workspace_id)
        .cloned()
        .ok_or_else(|| "workspace not found".to_string())?;
    let parent_entry = entry
        .parent_id
        .as_ref()
        .and_then(|parent_id| workspaces.get(parent_id))
        .cloned();
    Ok((entry, parent_entry))
}

async fn resolve_codex_home_for_workspace_core(
    workspaces: &Mutex<HashMap<String, WorkspaceEntry>>,
    workspace_id: &str,
) -> Result<PathBuf, String> {
    let (entry, parent_entry) = resolve_workspace_and_parent(workspaces, workspace_id).await?;
    resolve_workspace_codex_home(&entry, parent_entry.as_ref())
        .or_else(resolve_default_codex_home)
        .ok_or_else(|| "Unable to resolve CODEX_HOME".to_string())
}

fn response_payload(response: &Value) -> &Value {
    response.get("result").unwrap_or(response)
}

fn extract_models_payload(response: &Value) -> Option<Value> {
    response_payload(response).get("models").cloned()
}

pub(crate) async fn start_thread_core(
    sessions: &Mutex<HashMap<String, Arc<WorkspaceSession>>>,
    workspace_id: String,
) -> Result<Value, String> {
    let session = get_session_clone(sessions, &workspace_id).await?;
    let params = json!({
        "cwd": session.entry.path,
        "mcpServers": []
    });
    let response = session.send_request("session/new", params).await?;
    // ACP returns { sessionId, models, modes, ... }.
    // Frontend expects { result: { thread: { id: "..." } } }.
    let session_id = response
        .get("result")
        .and_then(|r| r.get("sessionId"))
        .or_else(|| response.get("sessionId"))
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    if !session_id.is_empty() {
        let mut ts = session.translation_state.lock().await;
        ts.session_id = session_id.clone();
    }
    if let Some(models) = extract_models_payload(&response) {
        *session.models_cache.lock().await = Some(models);
    }
    Ok(json!({
        "result": {
            "thread": { "id": session_id }
        }
    }))
}

pub(crate) async fn resume_thread_core(
    sessions: &Mutex<HashMap<String, Arc<WorkspaceSession>>>,
    workspace_id: String,
    thread_id: String,
) -> Result<Value, String> {
    let session = get_session_clone(sessions, &workspace_id).await?;
    let params = json!({
        "sessionId": thread_id,
        "cwd": session.entry.path,
        "mcpServers": []
    });
    let _response = session.send_request("session/load", params).await?;
    // session/load replays history as sessionUpdate events (handled by stdout reader).
    {
        let mut ts = session.translation_state.lock().await;
        ts.session_id = thread_id.clone();
    }
    if let Some(models) = extract_models_payload(&_response) {
        *session.models_cache.lock().await = Some(models);
    }
    Ok(json!({
        "result": {
            "thread": { "id": thread_id }
        }
    }))
}

pub(crate) async fn fork_thread_core(
    _sessions: &Mutex<HashMap<String, Arc<WorkspaceSession>>>,
    _workspace_id: String,
    _thread_id: String,
) -> Result<Value, String> {
    // ACP has unstable_forkSession but it's too unstable for MVP.
    Err("fork is not supported in OpenCode ACP yet".to_string())
}

pub(crate) async fn list_threads_core(
    sessions: &Mutex<HashMap<String, Arc<WorkspaceSession>>>,
    workspace_id: String,
    _cursor: Option<String>,
    _limit: Option<u32>,
    _sort_key: Option<String>,
) -> Result<Value, String> {
    let session = get_session_clone(sessions, &workspace_id).await?;
    // ACP session/list accepts cwd to scope results.  It does not support
    // cursor/limit/sortKey — we return all results in one page.
    let params = json!({ "cwd": session.entry.path });
    let response = session.send_request("session/list", params).await?;
    // ACP returns { sessions: [{ sessionId, cwd, title, updatedAt }] }.
    // Frontend expects { result: { data: [{ id, cwd, ... }], nextCursor } }.
    let payload = response.get("result").unwrap_or(&response);
    let sessions_arr = payload
        .get("sessions")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let data: Vec<Value> = sessions_arr
        .into_iter()
        .map(|s| {
            let id = s
                .get("sessionId")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let cwd = s
                .get("cwd")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let title = s
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let updated_at = s
                .get("updatedAt")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            json!({
                "id": id,
                "cwd": cwd,
                "name": title,
                "updatedAt": updated_at,
                "createdAt": updated_at
            })
        })
        .collect();
    Ok(json!({
        "result": {
            "data": data,
            "nextCursor": null
        }
    }))
}

pub(crate) async fn list_mcp_server_status_core(
    _sessions: &Mutex<HashMap<String, Arc<WorkspaceSession>>>,
    _workspace_id: String,
    _cursor: Option<String>,
    _limit: Option<u32>,
) -> Result<Value, String> {
    Ok(json!({ "result": { "data": [], "nextCursor": null } }))
}

pub(crate) async fn archive_thread_core(
    _sessions: &Mutex<HashMap<String, Arc<WorkspaceSession>>>,
    _workspace_id: String,
    _thread_id: String,
) -> Result<Value, String> {
    // No ACP equivalent — archive is UI-only (handled by frontend localStorage).
    Ok(json!({ "ok": true }))
}

pub(crate) async fn compact_thread_core(
    sessions: &Mutex<HashMap<String, Arc<WorkspaceSession>>>,
    workspace_id: String,
    thread_id: String,
) -> Result<Value, String> {
    // Send /compact as a prompt — ACP has no dedicated compact method.
    let session = get_session_clone(sessions, &workspace_id).await?;
    let params = json!({
        "sessionId": thread_id,
        "prompt": [{ "type": "text", "text": "/compact" }]
    });
    session.send_request("session/prompt", params).await
}

pub(crate) async fn set_thread_name_core(
    _sessions: &Mutex<HashMap<String, Arc<WorkspaceSession>>>,
    _workspace_id: String,
    _thread_id: String,
    _name: String,
) -> Result<Value, String> {
    // No ACP equivalent — name is stored locally by the frontend.
    Ok(json!({ "ok": true }))
}

/// Build ACP `session/prompt` parts from frontend input.
fn build_acp_prompt_parts(
    text: String,
    images: Option<Vec<String>>,
    app_mentions: Option<Vec<Value>>,
) -> Result<Vec<Value>, String> {
    let trimmed_text = text.trim();
    let mut parts: Vec<Value> = Vec::new();
    if !trimmed_text.is_empty() {
        parts.push(json!({ "type": "text", "text": trimmed_text }));
    }
    if let Some(paths) = images {
        for path in paths {
            let trimmed = path.trim();
            if trimmed.is_empty() {
                continue;
            }
            if trimmed.starts_with("data:") {
                // data: URI — extract mime + base64
                // Format: data:<mime>;base64,<data>
                if let Some(rest) = trimmed.strip_prefix("data:") {
                    if let Some((mime, data)) = rest.split_once(";base64,") {
                        parts.push(json!({
                            "type": "image",
                            "mimeType": mime,
                            "data": data
                        }));
                        continue;
                    }
                }
                parts.push(json!({ "type": "text", "text": format!("[image: {trimmed}]") }));
            } else if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
                // URL images — ACP wants base64.  For now pass as text placeholder.
                parts.push(json!({ "type": "text", "text": format!("[image: {trimmed}]") }));
            } else {
                // Local file path — read and base64-encode.
                match std::fs::read(trimmed) {
                    Ok(bytes) => {
                        use base64::Engine;
                        let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
                        let mime = mime_from_extension(trimmed);
                        parts.push(json!({
                            "type": "image",
                            "mimeType": mime,
                            "data": encoded
                        }));
                    }
                    Err(_) => {
                        parts.push(json!({ "type": "text", "text": format!("[image: {trimmed}]") }));
                    }
                }
            }
        }
    }
    if let Some(mentions) = app_mentions {
        let mut seen_paths: HashSet<String> = HashSet::new();
        for mention in mentions {
            let object = mention
                .as_object()
                .ok_or_else(|| "invalid app mention payload".to_string())?;
            let name = object
                .get("name")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| "invalid app mention name".to_string())?;
            let path = object
                .get("path")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| "invalid app mention path".to_string())?;
            if !path.starts_with("app://") || path.len() <= "app://".len() {
                return Err("invalid app mention path".to_string());
            }
            if !seen_paths.insert(path.to_string()) {
                continue;
            }
            // Convert app:// mention → resource_link with file:// URI.
            let file_path = &path["app://".len()..];
            parts.push(json!({
                "type": "resource_link",
                "uri": format!("file://{file_path}"),
                "name": name
            }));
        }
    }
    if parts.is_empty() {
        return Err("empty user message".to_string());
    }
    Ok(parts)
}

fn mime_from_extension(path: &str) -> &str {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".png") {
        "image/png"
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        "image/jpeg"
    } else if lower.ends_with(".gif") {
        "image/gif"
    } else if lower.ends_with(".webp") {
        "image/webp"
    } else if lower.ends_with(".svg") {
        "image/svg+xml"
    } else {
        "application/octet-stream"
    }
}

static TURN_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

pub(crate) async fn send_user_message_core<E: EventSink>(
    sessions: &Mutex<HashMap<String, Arc<WorkspaceSession>>>,
    workspace_id: String,
    thread_id: String,
    text: String,
    _model: Option<String>,
    _effort: Option<String>,
    _access_mode: Option<String>,
    images: Option<Vec<String>>,
    app_mentions: Option<Vec<Value>>,
    _collaboration_mode: Option<Value>,
    event_sink: &E,
) -> Result<Value, String> {
    let session = get_session_clone(sessions, &workspace_id).await?;
    let parts = build_acp_prompt_parts(text, images, app_mentions)?;

    // Synthesize turn ID and prepare translation state.
    let turn_n = TURN_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let turn_id = format!("turn_{turn_n}");
    {
        let mut ts = session.translation_state.lock().await;
        ts.start_turn(turn_id.clone());
    }

    // Emit synthetic turn/started.
    let started_msg = event_translator::build_turn_started(&thread_id, &turn_id);
    event_sink.emit_app_server_event(AppServerEvent {
        workspace_id: workspace_id.clone(),
        message: started_msg,
    });

    let params = json!({ "sessionId": thread_id, "prompt": parts });
    let result = session.send_request("session/prompt", params).await;

    // Emit synthetic item/completed for agent message (if one was streamed).
    {
        let ts = session.translation_state.lock().await;
        if let Some(msg_completed) = event_translator::build_agent_message_completed(&ts) {
            event_sink.emit_app_server_event(AppServerEvent {
                workspace_id: workspace_id.clone(),
                message: msg_completed,
            });
        }
    }

    // Emit synthetic turn/completed.
    let completed_msg = event_translator::build_turn_completed(&thread_id, &turn_id);
    event_sink.emit_app_server_event(AppServerEvent {
        workspace_id: workspace_id.clone(),
        message: completed_msg,
    });

    let response = result?;
    if response.get("error").is_some() {
        return Ok(response);
    }
    let mut command_result = response_payload(&response)
        .as_object()
        .cloned()
        .unwrap_or_default();
    command_result.insert("turn".to_string(), json!({ "id": turn_id }));
    Ok(json!({ "result": Value::Object(command_result) }))
}

pub(crate) async fn turn_steer_core(
    _sessions: &Mutex<HashMap<String, Arc<WorkspaceSession>>>,
    _workspace_id: String,
    _thread_id: String,
    _turn_id: String,
    _text: String,
    _images: Option<Vec<String>>,
    _app_mentions: Option<Vec<Value>>,
) -> Result<Value, String> {
    Err("turn steering is not supported by OpenCode ACP".to_string())
}

pub(crate) async fn collaboration_mode_list_core(
    _sessions: &Mutex<HashMap<String, Arc<WorkspaceSession>>>,
    _workspace_id: String,
) -> Result<Value, String> {
    Ok(json!({ "result": { "data": [] } }))
}

pub(crate) async fn turn_interrupt_core(
    sessions: &Mutex<HashMap<String, Arc<WorkspaceSession>>>,
    workspace_id: String,
    thread_id: String,
    _turn_id: String,
) -> Result<Value, String> {
    let session = get_session_clone(sessions, &workspace_id).await?;
    // ACP uses a `cancel` notification with sessionId.
    session
        .send_notification("cancel", Some(json!({ "sessionId": thread_id })))
        .await?;
    Ok(json!({ "ok": true }))
}

pub(crate) async fn start_review_core(
    _sessions: &Mutex<HashMap<String, Arc<WorkspaceSession>>>,
    _workspace_id: String,
    _thread_id: String,
    _target: Value,
    _delivery: Option<String>,
) -> Result<Value, String> {
    Err("review is not supported by OpenCode ACP".to_string())
}

pub(crate) async fn model_list_core(
    sessions: &Mutex<HashMap<String, Arc<WorkspaceSession>>>,
    workspace_id: String,
) -> Result<Value, String> {
    let session = get_session_clone(sessions, &workspace_id).await?;
    let models_payload = session.models_cache.lock().await.clone();
    let Some(models_payload) = models_payload else {
        return Ok(json!({ "result": { "data": [] } }));
    };

    let current_model = models_payload
        .get("currentModelId")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    let available_models = models_payload
        .get("availableModels")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();

    let data: Vec<Value> = available_models
        .into_iter()
        .filter_map(|entry| {
            let model_id = entry
                .get("modelId")
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .trim()
                .to_string();
            if model_id.is_empty() {
                return None;
            }
            let display_name = entry
                .get("name")
                .and_then(|value| value.as_str())
                .unwrap_or(&model_id)
                .trim()
                .to_string();

            Some(json!({
                "id": model_id.clone(),
                "model": model_id.clone(),
                "displayName": display_name,
                "description": "",
                "supportedReasoningEfforts": [],
                "defaultReasoningEffort": null,
                "isDefault": model_id == current_model,
            }))
        })
        .collect();

    Ok(json!({ "result": { "data": data } }))
}

pub(crate) async fn account_rate_limits_core(
    _sessions: &Mutex<HashMap<String, Arc<WorkspaceSession>>>,
    _workspace_id: String,
) -> Result<Value, String> {
    Ok(json!({ "result": {} }))
}

pub(crate) async fn account_read_core(
    sessions: &Mutex<HashMap<String, Arc<WorkspaceSession>>>,
    workspaces: &Mutex<HashMap<String, WorkspaceEntry>>,
    workspace_id: String,
) -> Result<Value, String> {
    let session = {
        let sessions = sessions.lock().await;
        sessions.get(&workspace_id).cloned()
    };
    let response = if let Some(session) = session {
        session.send_request("account/read", Value::Null).await.ok()
    } else {
        None
    };

    let (entry, parent_entry) = resolve_workspace_and_parent(workspaces, &workspace_id).await?;
    let codex_home = resolve_workspace_codex_home(&entry, parent_entry.as_ref())
        .or_else(resolve_default_codex_home);
    let fallback = read_auth_account(codex_home);

    Ok(build_account_response(response, fallback))
}

pub(crate) async fn codex_login_core(
    _sessions: &Mutex<HashMap<String, Arc<WorkspaceSession>>>,
    _codex_login_cancels: &Mutex<HashMap<String, CodexLoginCancelState>>,
    _workspace_id: String,
) -> Result<Value, String> {
    // OpenCode uses `opencode auth login` externally — no in-app login flow.
    Err("Login is not supported in-app. Run `opencode auth login` in Terminal.".to_string())
}

pub(crate) async fn codex_login_cancel_core(
    _sessions: &Mutex<HashMap<String, Arc<WorkspaceSession>>>,
    _codex_login_cancels: &Mutex<HashMap<String, CodexLoginCancelState>>,
    _workspace_id: String,
) -> Result<Value, String> {
    Ok(json!({ "canceled": false }))
}

pub(crate) async fn skills_list_core(
    _sessions: &Mutex<HashMap<String, Arc<WorkspaceSession>>>,
    _workspace_id: String,
) -> Result<Value, String> {
    Ok(json!({ "result": { "data": [] } }))
}

pub(crate) async fn apps_list_core(
    _sessions: &Mutex<HashMap<String, Arc<WorkspaceSession>>>,
    _workspace_id: String,
    _cursor: Option<String>,
    _limit: Option<u32>,
    _thread_id: Option<String>,
) -> Result<Value, String> {
    Ok(json!({ "result": { "data": [], "nextCursor": null } }))
}

pub(crate) async fn respond_to_server_request_core(
    sessions: &Mutex<HashMap<String, Arc<WorkspaceSession>>>,
    workspace_id: String,
    request_id: Value,
    result: Value,
) -> Result<(), String> {
    let session = get_session_clone(sessions, &workspace_id).await?;
    // Frontend sends { decision: "accept" | "decline" } or { answers: {...} }.
    // For approval decisions, translate to ACP's permission response format.
    let acp_result = if let Some(decision) = result.get("decision").and_then(|v| v.as_str()) {
        let accept = decision == "accept";
        event_translator::build_permission_response(accept)
    } else {
        result
    };
    session.send_response(request_id, acp_result).await
}

pub(crate) async fn remember_approval_rule_core(
    workspaces: &Mutex<HashMap<String, WorkspaceEntry>>,
    workspace_id: String,
    command: Vec<String>,
) -> Result<Value, String> {
    let command = command
        .into_iter()
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .collect::<Vec<_>>();
    if command.is_empty() {
        return Err("empty command".to_string());
    }

    let codex_home = resolve_codex_home_for_workspace_core(workspaces, &workspace_id).await?;
    let rules_path = rules::default_rules_path(&codex_home);
    rules::append_prefix_rule(&rules_path, &command)?;

    Ok(json!({
        "ok": true,
        "rulesPath": rules_path,
    }))
}

pub(crate) async fn get_config_model_core(
    workspaces: &Mutex<HashMap<String, WorkspaceEntry>>,
    workspace_id: String,
) -> Result<Value, String> {
    let codex_home = resolve_codex_home_for_workspace_core(workspaces, &workspace_id).await?;
    let model = codex_config::read_config_model(Some(codex_home))?;
    Ok(json!({ "model": model }))
}

use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use tokio::sync::{oneshot, Mutex};
use tokio::time::timeout;

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

fn should_include_hidden_sessions(sort_key: &Option<String>) -> bool {
    sort_key
        .as_deref()
        .map(str::trim)
        .map(|value| value.eq_ignore_ascii_case("all"))
        .unwrap_or(false)
}

async fn hidden_session_ids_for_workspace(
    workspaces: &Mutex<HashMap<String, WorkspaceEntry>>,
    workspace_id: &str,
) -> HashSet<String> {
    let workspaces = workspaces.lock().await;
    workspaces
        .get(workspace_id)
        .map(|entry| {
            entry
                .settings
                .hidden_session_ids
                .iter()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .collect::<HashSet<_>>()
        })
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Thread / session lifecycle (REST)
// ---------------------------------------------------------------------------

pub(crate) async fn start_thread_core(
    sessions: &Mutex<HashMap<String, Arc<WorkspaceSession>>>,
    workspace_id: String,
) -> Result<Value, String> {
    let session = get_session_clone(sessions, &workspace_id).await?;

    // Reuse the pre-warmed session if available.
    if let Some(session_id) = session.prewarmed_session_id.lock().await.take() {
        let mut ts = session.translation_state.lock().await;
        ts.session_id = session_id.clone();
        return Ok(json!({
            "result": {
                "thread": { "id": session_id }
            }
        }));
    }

    // POST /session → { id, projectID, directory }
    let response = session.rest_post("/session", json!({})).await?;
    let session_id = response
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    if !session_id.is_empty() {
        let mut ts = session.translation_state.lock().await;
        ts.session_id = session_id.clone();
    }
    Ok(json!({
        "result": {
            "thread": { "id": session_id }
        }
    }))
}

pub(crate) async fn resume_thread_core<E: EventSink>(
    sessions: &Mutex<HashMap<String, Arc<WorkspaceSession>>>,
    workspace_id: String,
    thread_id: String,
    event_sink: &E,
) -> Result<Value, String> {
    let session = get_session_clone(sessions, &workspace_id).await?;

    let path = format!("/session/{thread_id}/message");
    let messages = session.rest_get(&path).await?;

    {
        let mut ts = session.translation_state.lock().await;
        ts.prepare_replay(thread_id.clone());
    }

    let mut replay_item_counter = 0u64;

    if let Some(msg_list) = messages.as_array() {
        for msg_entry in msg_list {
            let role = msg_entry
                .get("info")
                .and_then(|i| i.get("role"))
                .and_then(|v| v.as_str())
                .unwrap_or("assistant");

            let parts = msg_entry
                .get("parts")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();

            let mut content_parts: Vec<Value> = Vec::new();

            for part in &parts {
                let part_type = part.get("type").and_then(|v| v.as_str()).unwrap_or("");
                match part_type {
                    "text" => {
                        let text = part.get("text").and_then(|v| v.as_str()).unwrap_or("");
                        if !text.is_empty() {
                            content_parts.push(json!({ "type": "text", "text": text }));
                        }
                    }
                    "tool" => {
                        let tool_name = part
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown");
                        let tool_state = part
                            .get("state")
                            .and_then(|v| v.as_str())
                            .unwrap_or("completed");
                        let tool_output = part
                            .get("output")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        content_parts.push(json!({
                            "type": "tool",
                            "name": tool_name,
                            "state": tool_state,
                            "output": tool_output
                        }));
                    }
                    "file" => {
                        if let Some(url) = part.get("url").and_then(|v| v.as_str()) {
                            content_parts.push(json!({ "type": "image", "value": url }));
                        }
                    }
                    _ => {}
                }
            }

            if content_parts.is_empty() {
                continue;
            }

            replay_item_counter += 1;
            let item_id = format!("replay_item_{replay_item_counter}");
            let item_type = if role == "user" {
                "userMessage"
            } else {
                "agentMessage"
            };

            event_sink.emit_app_server_event(AppServerEvent {
                workspace_id: workspace_id.clone(),
                message: json!({
                    "method": "item/completed",
                    "params": {
                        "threadId": thread_id,
                        "item": {
                            "id": item_id,
                            "type": item_type,
                            "content": content_parts
                        }
                    }
                }),
            });
        }
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
    Err("fork is not supported yet".to_string())
}

pub(crate) async fn list_threads_core(
    sessions: &Mutex<HashMap<String, Arc<WorkspaceSession>>>,
    workspaces: &Mutex<HashMap<String, WorkspaceEntry>>,
    workspace_id: String,
    _cursor: Option<String>,
    _limit: Option<u32>,
    sort_key: Option<String>,
) -> Result<Value, String> {
    let session = get_session_clone(sessions, &workspace_id).await?;

    // GET /session → Session[]
    let response = session.rest_get("/session").await?;
    let include_hidden = should_include_hidden_sessions(&sort_key);
    let hidden_session_ids = if include_hidden {
        HashSet::new()
    } else {
        hidden_session_ids_for_workspace(workspaces, &workspace_id).await
    };

    let sessions_arr = response.as_array().cloned().unwrap_or_default();
    let data: Vec<Value> = sessions_arr
        .into_iter()
        .filter_map(|s| {
            let id = s
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            if !include_hidden && hidden_session_ids.contains(id) {
                return None;
            }
            let title = s
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let updated_at = s
                .get("updatedAt")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let directory = s
                .get("directory")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            Some(json!({
                "id": id,
                "cwd": directory,
                "name": title,
                "preview": title,
                "updatedAt": updated_at,
                "createdAt": updated_at
            }))
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
    // Archive is UI-only (handled by frontend localStorage).
    Ok(json!({ "ok": true }))
}

pub(crate) async fn compact_thread_core(
    sessions: &Mutex<HashMap<String, Arc<WorkspaceSession>>>,
    workspace_id: String,
    thread_id: String,
) -> Result<Value, String> {
    let session = get_session_clone(sessions, &workspace_id).await?;
    // Send /compact as a message via POST /session/:id/message.
    let path = format!("/session/{thread_id}/message");
    let body = json!({
        "parts": [{ "type": "text", "text": "/compact" }]
    });
    session.rest_post(&path, body).await
}

pub(crate) async fn set_thread_name_core(
    _sessions: &Mutex<HashMap<String, Arc<WorkspaceSession>>>,
    _workspace_id: String,
    _thread_id: String,
    _name: String,
) -> Result<Value, String> {
    // No REST equivalent — name is stored locally by the frontend.
    Ok(json!({ "ok": true }))
}

// ---------------------------------------------------------------------------
// Image handling (kept from ACP — same logic)
// ---------------------------------------------------------------------------

const URL_IMAGE_FETCH_TIMEOUT: Duration = Duration::from_secs(10);
const URL_IMAGE_MAX_BYTES: usize = 8 * 1024 * 1024;

/// Build REST prompt parts from frontend input.
///
/// REST uses `{ type: "file", mime, url: "data:...", filename }` for images
/// instead of ACP's `{ type: "image", mimeType, data }`.
async fn build_rest_prompt_parts(
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
                // data: URI — use directly as file part.
                let mime = trimmed
                    .strip_prefix("data:")
                    .and_then(|rest| rest.split_once(";base64,"))
                    .map(|(m, _)| m)
                    .unwrap_or("application/octet-stream");
                parts.push(json!({
                    "type": "file",
                    "mime": mime,
                    "url": trimmed,
                    "filename": "image"
                }));
            } else if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
                parts.push(fetch_url_image_as_file_part(trimmed).await?);
            } else {
                // Local file path — read and base64-encode.
                match std::fs::read(trimmed) {
                    Ok(bytes) => {
                        use base64::Engine;
                        let encoded =
                            base64::engine::general_purpose::STANDARD.encode(&bytes);
                        let mime = mime_from_extension(trimmed);
                        let filename = std::path::Path::new(trimmed)
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("image");
                        parts.push(json!({
                            "type": "file",
                            "mime": mime,
                            "url": format!("data:{mime};base64,{encoded}"),
                            "filename": filename
                        }));
                    }
                    Err(_) => {
                        parts
                            .push(json!({ "type": "text", "text": format!("[image: {trimmed}]") }));
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
            let file_path = &path["app://".len()..];
            // Read file content and include as text context.
            match std::fs::read_to_string(file_path) {
                Ok(content) => {
                    parts.push(json!({
                        "type": "text",
                        "text": format!("--- {name} ({file_path}) ---\n{content}")
                    }));
                }
                Err(_) => {
                    parts.push(json!({
                        "type": "text",
                        "text": format!("[file: {name} at {file_path}]")
                    }));
                }
            }
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

fn ip_is_disallowed(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let octets = v4.octets();
            v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_multicast()
                || v4.is_unspecified()
                || octets[0] == 0
                || (octets[0] == 100 && (64..=127).contains(&octets[1]))
                || (octets[0] == 198 && (octets[1] == 18 || octets[1] == 19))
                || (octets[0] == 192 && octets[1] == 0 && octets[2] == 0)
                || (octets[0] == 255 && octets[1] == 255 && octets[2] == 255 && octets[3] == 255)
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_multicast()
                || v6.is_unspecified()
                || v6.is_unique_local()
                || v6.is_unicast_link_local()
        }
    }
}

async fn validate_public_image_url(url: &reqwest::Url) -> Result<(), String> {
    let scheme = url.scheme();
    if scheme != "http" && scheme != "https" {
        return Err("Image URL must use http or https.".to_string());
    }
    let host = url
        .host_str()
        .ok_or_else(|| "Image URL must include a host.".to_string())?;
    let host_lower = host.to_ascii_lowercase();
    if host_lower == "localhost"
        || host_lower.ends_with(".localhost")
        || host_lower.ends_with(".local")
    {
        return Err(
            "Blocked image URL host: localhost and local domains are not allowed.".to_string(),
        );
    }

    if let Ok(ip) = host.parse::<IpAddr>() {
        if ip_is_disallowed(ip) {
            return Err(
                "Blocked image URL host: private or local network addresses are not allowed."
                    .to_string(),
            );
        }
        return Ok(());
    }

    let port = url
        .port_or_known_default()
        .unwrap_or(if scheme == "https" { 443 } else { 80 });
    let resolved = timeout(
        URL_IMAGE_FETCH_TIMEOUT,
        tokio::net::lookup_host((host, port)),
    )
    .await
    .map_err(|_| "Timed out resolving image URL host.".to_string())
    .and_then(|result| result.map_err(|err| format!("Failed to resolve image URL host: {err}")))?;

    let mut saw_address = false;
    for socket_addr in resolved {
        saw_address = true;
        if ip_is_disallowed(socket_addr.ip()) {
            return Err(
                "Blocked image URL host: private or local network addresses are not allowed."
                    .to_string(),
            );
        }
    }

    if !saw_address {
        return Err("Image URL host did not resolve to any addresses.".to_string());
    }

    Ok(())
}

/// Fetch an image URL and return it as a REST file part.
async fn fetch_url_image_as_file_part(url: &str) -> Result<Value, String> {
    let parsed = reqwest::Url::parse(url).map_err(|_| "Invalid image URL.".to_string())?;
    validate_public_image_url(&parsed).await?;

    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(URL_IMAGE_FETCH_TIMEOUT)
        .build()
        .map_err(|err| format!("Failed to initialize image downloader: {err}"))?;

    let response = client.get(parsed.clone()).send().await.map_err(|err| {
        if err.is_timeout() {
            "Timed out while fetching image URL.".to_string()
        } else {
            format!("Failed to fetch image URL: {err}")
        }
    })?;

    if !response.status().is_success() {
        return Err(format!(
            "Image URL request failed with status {}.",
            response.status()
        ));
    }

    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "Image URL must return an image Content-Type header.".to_string())?;
    let mime_type = content_type
        .split(';')
        .next()
        .map(str::trim)
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !mime_type.starts_with("image/") {
        return Err("Image URL must return an image Content-Type header.".to_string());
    }

    let mut bytes: Vec<u8> = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result.map_err(|err| format!("Failed reading image bytes: {err}"))?;
        if bytes.len() + chunk.len() > URL_IMAGE_MAX_BYTES {
            return Err(format!(
                "Image URL exceeds max allowed size of {} bytes.",
                URL_IMAGE_MAX_BYTES
            ));
        }
        bytes.extend_from_slice(&chunk);
    }

    use base64::Engine;
    let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
    let filename = parsed
        .path_segments()
        .and_then(|segs| segs.last())
        .filter(|s| !s.is_empty())
        .unwrap_or("image");
    Ok(json!({
        "type": "file",
        "mime": mime_type,
        "url": format!("data:{mime_type};base64,{encoded}"),
        "filename": filename
    }))
}

fn normalize_optional_string(value: Option<String>) -> Option<String> {
    value
        .map(|raw| raw.trim().to_string())
        .filter(|raw| !raw.is_empty())
}

fn emit_turn_error<E: EventSink>(
    event_sink: &E,
    workspace_id: &str,
    thread_id: &str,
    turn_id: &str,
    message: &str,
) {
    event_sink.emit_app_server_event(AppServerEvent {
        workspace_id: workspace_id.to_string(),
        message: json!({
            "method": "error",
            "params": {
                "threadId": thread_id,
                "turnId": turn_id,
                "willRetry": false,
                "error": {
                    "message": message
                }
            }
        }),
    });
}

static TURN_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

// ---------------------------------------------------------------------------
// Send user message (REST: fire-and-forget via prompt_async)
// ---------------------------------------------------------------------------

pub(crate) async fn send_user_message_core<E: EventSink>(
    sessions: &Mutex<HashMap<String, Arc<WorkspaceSession>>>,
    workspace_id: String,
    thread_id: String,
    text: String,
    model: Option<String>,
    effort: Option<String>,
    _access_mode: Option<String>,
    images: Option<Vec<String>>,
    app_mentions: Option<Vec<Value>>,
    _collaboration_mode: Option<Value>,
    event_sink: &E,
) -> Result<Value, String> {
    let session = get_session_clone(sessions, &workspace_id).await?;
    let user_text = text.trim().to_string();
    let user_images = images.clone().unwrap_or_default();
    let parts = build_rest_prompt_parts(text, images, app_mentions).await?;
    let _prompt_guard = session.prompt_lock.lock().await;

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

    // Emit synthetic user message item.
    if !user_text.is_empty() || !user_images.is_empty() {
        let mut content_parts: Vec<Value> = Vec::new();
        if !user_text.is_empty() {
            content_parts.push(json!({ "type": "text", "text": user_text }));
        }
        for image in user_images {
            let trimmed = image.trim();
            if trimmed.is_empty() {
                continue;
            }
            content_parts.push(json!({ "type": "image", "value": trimmed }));
        }
        if !content_parts.is_empty() {
            event_sink.emit_app_server_event(AppServerEvent {
                workspace_id: workspace_id.clone(),
                message: json!({
                    "method": "item/completed",
                    "params": {
                        "threadId": thread_id,
                        "item": {
                            "id": format!("item_user_{turn_id}"),
                            "type": "userMessage",
                            "content": content_parts
                        }
                    }
                }),
            });
        }
    }

    // Build prompt body. Model selection is per-message in REST.
    let mut body = json!({
        "parts": parts
    });
    let requested_model = normalize_optional_string(model);
    let requested_effort = normalize_optional_string(effort);
    if let Some(ref model_id) = requested_model {
        // REST API accepts model as { providerID, modelID } but modelID alone
        // may work depending on the server version. Include both fields.
        body["model"] = json!({ "modelID": model_id });
    }
    if let Some(ref effort_level) = requested_effort {
        body["effort"] = json!(effort_level);
    }

    // Fire-and-forget: POST /session/:id/prompt_async → 204.
    // Turn completion comes from SSE `session.status` → idle.
    let path = format!("/session/{thread_id}/prompt_async");
    let result = session.rest_post(&path, body).await;

    if let Err(ref error) = result {
        emit_turn_error(event_sink, &workspace_id, &thread_id, &turn_id, error);
    }

    // Return immediately — SSE events will drive the rest of the turn.
    Ok(json!({
        "result": {
            "turn": { "id": turn_id }
        }
    }))
}

// ---------------------------------------------------------------------------
// Turn interrupt (REST: POST /session/:id/abort)
// ---------------------------------------------------------------------------

pub(crate) async fn turn_interrupt_core(
    sessions: &Mutex<HashMap<String, Arc<WorkspaceSession>>>,
    workspace_id: String,
    thread_id: String,
    _turn_id: String,
) -> Result<Value, String> {
    let session = get_session_clone(sessions, &workspace_id).await?;
    let path = format!("/session/{thread_id}/abort");
    session.rest_post(&path, json!({})).await?;
    Ok(json!({ "ok": true }))
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
    Err("turn steering is not supported yet".to_string())
}

pub(crate) async fn collaboration_mode_list_core(
    _sessions: &Mutex<HashMap<String, Arc<WorkspaceSession>>>,
    _workspace_id: String,
) -> Result<Value, String> {
    Ok(json!({ "result": { "data": [] } }))
}

pub(crate) async fn start_review_core(
    _sessions: &Mutex<HashMap<String, Arc<WorkspaceSession>>>,
    _workspace_id: String,
    _thread_id: String,
    _target: Value,
    _delivery: Option<String>,
) -> Result<Value, String> {
    Err("review is not supported yet".to_string())
}

// ---------------------------------------------------------------------------
// Model list (REST: GET /config/providers)
// ---------------------------------------------------------------------------

pub(crate) async fn model_list_core(
    sessions: &Mutex<HashMap<String, Arc<WorkspaceSession>>>,
    workspace_id: String,
) -> Result<Value, String> {
    let session = get_session_clone(sessions, &workspace_id).await?;

    // Try to refresh from server; fall back to cache.
    let providers = match session.rest_get("/config/providers").await {
        Ok(fresh) => {
            *session.models_cache.lock().await = Some(fresh.clone());
            fresh
        }
        Err(_) => {
            let cache = session.models_cache.lock().await.clone();
            cache.unwrap_or(json!({}))
        }
    };

    // REST returns:
    //   providers: [{ id, models: { "model-id": { id, name, ... }, ... } }]
    //   default:   { "provider-id": "model-id", ... }
    let defaults = providers
        .get("default")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default();

    let provider_list = providers
        .get("providers")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut data: Vec<Value> = Vec::new();
    for provider in &provider_list {
        let provider_id = provider
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let default_for_provider = defaults
            .get(provider_id)
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        // `models` is a map { "model-id": { id, name, ... } }, not an array.
        let models_map = provider
            .get("models")
            .and_then(|v| v.as_object())
            .cloned()
            .unwrap_or_default();
        for (_key, model) in &models_map {
            let model_id = model
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .trim()
                .to_string();
            if model_id.is_empty() {
                continue;
            }
            let display_name = model
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or(&model_id)
                .trim()
                .to_string();
            let qualified_id = format!("{provider_id}/{model_id}");
            let is_default = model_id == default_for_provider;
            data.push(json!({
                "id": qualified_id,
                "model": model_id,
                "displayName": display_name,
                "description": "",
                "supportedReasoningEfforts": [],
                "defaultReasoningEffort": null,
                "isDefault": is_default,
            }));
        }
    }

    Ok(json!({ "result": { "data": data } }))
}

// ---------------------------------------------------------------------------
// Permission response (REST: POST /session/:sid/permissions/:pid)
// ---------------------------------------------------------------------------

pub(crate) async fn respond_to_server_request_core(
    sessions: &Mutex<HashMap<String, Arc<WorkspaceSession>>>,
    workspace_id: String,
    request_id: Value,
    result: Value,
) -> Result<(), String> {
    let session = get_session_clone(sessions, &workspace_id).await?;

    // request_id is "sessionId:permissionId" composite string.
    let composite = request_id.as_str().unwrap_or_default();
    let (_session_id, permission_id) = composite
        .split_once(':')
        .unwrap_or((composite, ""));

    let accept = result
        .get("decision")
        .and_then(|v| v.as_str())
        .map(|d| d == "accept")
        .unwrap_or(true);

    let body = event_translator::build_permission_response(accept);
    let path = format!("/permission/{permission_id}/reply");
    session.rest_post_bool(&path, body).await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Account / login stubs (unchanged)
// ---------------------------------------------------------------------------

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
    let _session = {
        let sessions = sessions.lock().await;
        sessions.get(&workspace_id).cloned()
    };

    let (entry, parent_entry) = resolve_workspace_and_parent(workspaces, &workspace_id).await?;
    let codex_home = resolve_workspace_codex_home(&entry, parent_entry.as_ref())
        .or_else(resolve_default_codex_home);
    let fallback = read_auth_account(codex_home);

    Ok(build_account_response(None, fallback))
}

pub(crate) async fn codex_login_core(
    _sessions: &Mutex<HashMap<String, Arc<WorkspaceSession>>>,
    _codex_login_cancels: &Mutex<HashMap<String, CodexLoginCancelState>>,
    _workspace_id: String,
) -> Result<Value, String> {
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{WorkspaceKind, WorkspaceSettings};
    use tokio::runtime::Builder;

    #[test]
    fn include_hidden_sessions_enabled_only_for_all_sort_key() {
        assert!(should_include_hidden_sessions(&Some("all".to_string())));
        assert!(should_include_hidden_sessions(&Some(" ALL ".to_string())));
        assert!(!should_include_hidden_sessions(&Some(
            "updated_at".to_string()
        )));
        assert!(!should_include_hidden_sessions(&None));
    }

    #[test]
    fn private_and_loopback_ips_are_disallowed() {
        assert!(ip_is_disallowed(
            "127.0.0.1".parse::<IpAddr>().expect("parse loopback")
        ));
        assert!(ip_is_disallowed(
            "10.0.0.1".parse::<IpAddr>().expect("parse private")
        ));
        assert!(!ip_is_disallowed(
            "8.8.8.8".parse::<IpAddr>().expect("parse public")
        ));
    }

    #[test]
    fn build_rest_prompt_parts_blocks_localhost_image_urls() {
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        runtime.block_on(async {
            let result = build_rest_prompt_parts(
                "hello".to_string(),
                Some(vec!["http://localhost/image.png".to_string()]),
                None,
            )
            .await;

            let error = result.expect_err("localhost URL should be blocked");
            assert!(error.contains("Blocked image URL host"));
        });
    }

    #[test]
    fn hidden_session_ids_are_read_from_workspace_settings() {
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        runtime.block_on(async {
            let workspaces = Mutex::new(HashMap::from([(
                "ws-1".to_string(),
                WorkspaceEntry {
                    id: "ws-1".to_string(),
                    name: "Workspace".to_string(),
                    path: "/tmp/ws-1".to_string(),
                    codex_bin: None,
                    kind: WorkspaceKind::Main,
                    parent_id: None,
                    worktree: None,
                    settings: WorkspaceSettings {
                        hidden_session_ids: vec!["ses_bg_1".to_string(), "ses_bg_2".to_string()],
                        ..WorkspaceSettings::default()
                    },
                },
            )]));

            let hidden = hidden_session_ids_for_workspace(&workspaces, "ws-1").await;
            assert!(hidden.contains("ses_bg_1"));
            assert!(hidden.contains("ses_bg_2"));
            assert!(!hidden.contains("ses_fg"));
        });
    }
}

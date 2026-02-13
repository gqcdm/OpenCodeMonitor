use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use tokio::sync::{oneshot, Mutex};
use tokio::time::timeout;

use crate::backend::app_server::{ModelSetCapability, ModelSetMethod, WorkspaceSession};
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

fn extract_current_model_id(models_payload: &Value) -> Option<String> {
    models_payload
        .get("currentModelId")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
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
        ts.prepare_replay(thread_id.clone());
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
    workspaces: &Mutex<HashMap<String, WorkspaceEntry>>,
    workspace_id: String,
    _cursor: Option<String>,
    _limit: Option<u32>,
    sort_key: Option<String>,
) -> Result<Value, String> {
    let session = get_session_clone(sessions, &workspace_id).await?;
    // ACP session/list accepts cwd to scope results.  It does not support
    // cursor/limit/sortKey — we return all results in one page.
    let params = json!({ "cwd": session.entry.path });
    let response = session.send_request("session/list", params).await?;
    // ACP returns { sessions: [{ sessionId, cwd, title, updatedAt }] }.
    // Frontend expects { result: { data: [{ id, cwd, ... }], nextCursor } }.
    let payload = response.get("result").unwrap_or(&response);
    let include_hidden = should_include_hidden_sessions(&sort_key);
    let hidden_session_ids = if include_hidden {
        HashSet::new()
    } else {
        hidden_session_ids_for_workspace(workspaces, &workspace_id).await
    };
    let sessions_arr = payload
        .get("sessions")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let data: Vec<Value> = sessions_arr
        .into_iter()
        .filter_map(|s| {
            let id = s
                .get("sessionId")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            if !include_hidden && hidden_session_ids.contains(id) {
                return None;
            }
            let cwd = s.get("cwd").and_then(|v| v.as_str()).unwrap_or_default();
            let title = s.get("title").and_then(|v| v.as_str()).unwrap_or_default();
            let updated_at = s
                .get("updatedAt")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            Some(json!({
                "id": id,
                "cwd": cwd,
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

const URL_IMAGE_FETCH_TIMEOUT: Duration = Duration::from_secs(10);
const URL_IMAGE_MAX_BYTES: usize = 8 * 1024 * 1024;

/// Build ACP `session/prompt` parts from frontend input.
async fn build_acp_prompt_parts(
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
                parts.push(fetch_url_image_part(trimmed).await?);
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

async fn fetch_url_image_part(url: &str) -> Result<Value, String> {
    let parsed = reqwest::Url::parse(url).map_err(|_| "Invalid image URL.".to_string())?;
    validate_public_image_url(&parsed).await?;

    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(URL_IMAGE_FETCH_TIMEOUT)
        .build()
        .map_err(|err| format!("Failed to initialize image downloader: {err}"))?;

    let response = client.get(parsed).send().await.map_err(|err| {
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
    Ok(json!({
        "type": "image",
        "mimeType": mime_type,
        "data": encoded
    }))
}

fn normalize_optional_string(value: Option<String>) -> Option<String> {
    value
        .map(|raw| raw.trim().to_string())
        .filter(|raw| !raw.is_empty())
}

fn extract_response_error_message(response: &Value) -> Option<String> {
    let error = response.get("error")?;
    if let Some(message) = error.get("message").and_then(|value| value.as_str()) {
        let trimmed = message.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    if let Some(message) = error.as_str() {
        let trimmed = message.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    Some("request failed".to_string())
}

fn response_is_method_not_found(response: &Value) -> bool {
    let Some(error) = response.get("error") else {
        return false;
    };
    if error.get("code").and_then(|value| value.as_i64()) == Some(-32601) {
        return true;
    }
    let message = error
        .get("message")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    message.contains("method not found") || message.contains("unknown method")
}

fn model_set_method_name(method: ModelSetMethod) -> &'static str {
    match method {
        ModelSetMethod::UnstableSetSessionModelId | ModelSetMethod::UnstableSetSessionModel => {
            "unstable_setSessionModel"
        }
        ModelSetMethod::SessionSetModelId => "session/setModel",
    }
}

fn model_set_model_key(method: ModelSetMethod) -> &'static str {
    match method {
        ModelSetMethod::UnstableSetSessionModel => "model",
        ModelSetMethod::UnstableSetSessionModelId | ModelSetMethod::SessionSetModelId => "modelId",
    }
}

fn effort_param_key_for_model(
    models_payload: &Value,
    requested_model: &str,
) -> Option<&'static str> {
    let available = models_payload
        .get("availableModels")
        .and_then(|value| value.as_array())?;
    let model_entry = available.iter().find(|entry| {
        entry
            .get("modelId")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .unwrap_or_default()
            == requested_model
    })?;

    if model_entry.get("supportedReasoningEfforts").is_some()
        || model_entry.get("defaultReasoningEffort").is_some()
        || model_entry.get("effort").is_some()
    {
        return Some("effort");
    }
    if model_entry.get("variant").is_some() {
        return Some("variant");
    }
    None
}

fn build_model_set_params(
    method: ModelSetMethod,
    thread_id: &str,
    requested_model: &str,
    effort_key: Option<&str>,
    effort: Option<&str>,
) -> Value {
    let mut params = serde_json::Map::new();
    params.insert(
        "sessionId".to_string(),
        Value::String(thread_id.to_string()),
    );
    params.insert(
        model_set_model_key(method).to_string(),
        Value::String(requested_model.to_string()),
    );
    if let (Some(key), Some(value)) = (effort_key, effort) {
        params.insert(key.to_string(), Value::String(value.to_string()));
    }
    Value::Object(params)
}

async fn update_current_model_cache(
    session: &WorkspaceSession,
    model_set_response: &Value,
    requested_model: &str,
) {
    if let Some(models_payload) = extract_models_payload(model_set_response) {
        *session.models_cache.lock().await = Some(models_payload);
        return;
    }
    let mut models_cache = session.models_cache.lock().await;
    if let Some(models) = models_cache
        .as_mut()
        .and_then(|value| value.as_object_mut())
    {
        models.insert(
            "currentModelId".to_string(),
            Value::String(requested_model.to_string()),
        );
    }
}

async fn emit_model_set_warning_once<E: EventSink>(
    session: &WorkspaceSession,
    event_sink: &E,
    workspace_id: &str,
    message: &str,
) {
    let mut emitted = session.model_set_warning_emitted.lock().await;
    if *emitted {
        return;
    }
    *emitted = true;
    event_sink.emit_app_server_event(AppServerEvent {
        workspace_id: workspace_id.to_string(),
        message: json!({
            "method": "codex/stderr",
            "params": {
                "message": message
            }
        }),
    });
}

async fn try_model_set_with_method(
    session: &WorkspaceSession,
    method: ModelSetMethod,
    thread_id: &str,
    requested_model: &str,
    effort_key: Option<&str>,
    effort: Option<&str>,
) -> Result<Value, String> {
    let mut attempts: Vec<(Option<&str>, Option<&str>)> = Vec::new();
    if let (Some(key), Some(value)) = (effort_key, effort) {
        attempts.push((Some(key), Some(value)));
        attempts.push((None, None));
    } else {
        attempts.push((None, None));
    }

    let mut last_error = "model switch request failed".to_string();
    let mut saw_method_not_found = true;
    for (attempt_effort_key, attempt_effort_value) in attempts {
        let params = build_model_set_params(
            method,
            thread_id,
            requested_model,
            attempt_effort_key,
            attempt_effort_value,
        );
        let response = session
            .send_request(model_set_method_name(method), params)
            .await
            .map_err(|err| {
                saw_method_not_found = false;
                err
            })?;

        if let Some(error_message) = extract_response_error_message(&response) {
            if response_is_method_not_found(&response) {
                last_error = error_message;
                continue;
            }
            saw_method_not_found = false;
            last_error = error_message;
            continue;
        }

        return Ok(response);
    }

    if saw_method_not_found {
        Err("method-not-found".to_string())
    } else {
        Err(last_error)
    }
}

async fn maybe_apply_requested_model<E: EventSink>(
    session: &WorkspaceSession,
    workspace_id: &str,
    thread_id: &str,
    model: Option<String>,
    effort: Option<String>,
    event_sink: &E,
) {
    let requested_model = match normalize_optional_string(model) {
        Some(value) => value,
        None => return,
    };

    let models_snapshot = session.models_cache.lock().await.clone();
    let current_model = models_snapshot
        .as_ref()
        .and_then(extract_current_model_id)
        .unwrap_or_default();
    if !current_model.is_empty() && current_model == requested_model {
        return;
    }

    let effort = normalize_optional_string(effort);
    let effort_key = models_snapshot
        .as_ref()
        .and_then(|models| effort_param_key_for_model(models, &requested_model));
    let effort_ref = effort.as_deref();

    let capability = *session.model_set_capability.lock().await;
    match capability {
        ModelSetCapability::Unsupported => {
            emit_model_set_warning_once(
                session,
                event_sink,
                workspace_id,
                "Model switch is not supported by this ACP version; continuing with prompt.",
            )
            .await;
            return;
        }
        ModelSetCapability::Supported(method) => {
            match try_model_set_with_method(
                session,
                method,
                thread_id,
                &requested_model,
                effort_key,
                effort_ref,
            )
            .await
            {
                Ok(response) => {
                    update_current_model_cache(session, &response, &requested_model).await;
                }
                Err(error) => {
                    if error == "method-not-found" {
                        *session.model_set_capability.lock().await =
                            ModelSetCapability::Unsupported;
                    }
                    emit_model_set_warning_once(
                        session,
                        event_sink,
                        workspace_id,
                        &format!(
                            "Failed to apply requested model; continuing with prompt: {error}"
                        ),
                    )
                    .await;
                }
            }
            return;
        }
        ModelSetCapability::Unknown => {}
    }

    for method in [
        ModelSetMethod::UnstableSetSessionModelId,
        ModelSetMethod::UnstableSetSessionModel,
        ModelSetMethod::SessionSetModelId,
    ] {
        match try_model_set_with_method(
            session,
            method,
            thread_id,
            &requested_model,
            effort_key,
            effort_ref,
        )
        .await
        {
            Ok(response) => {
                *session.model_set_capability.lock().await = ModelSetCapability::Supported(method);
                update_current_model_cache(session, &response, &requested_model).await;
                return;
            }
            Err(error) if error == "method-not-found" => continue,
            Err(error) => {
                emit_model_set_warning_once(
                    session,
                    event_sink,
                    workspace_id,
                    &format!("Failed to apply requested model; continuing with prompt: {error}"),
                )
                .await;
                return;
            }
        }
    }

    *session.model_set_capability.lock().await = ModelSetCapability::Unsupported;
    emit_model_set_warning_once(
        session,
        event_sink,
        workspace_id,
        "Model switch is not supported by this ACP version; continuing with prompt.",
    )
    .await;
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
    let parts = build_acp_prompt_parts(text, images, app_mentions).await?;
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

    maybe_apply_requested_model(
        &session,
        &workspace_id,
        &thread_id,
        model,
        effort,
        event_sink,
    )
    .await;

    let params = json!({ "sessionId": thread_id, "prompt": parts });
    let result = session.send_request("session/prompt", params).await;

    let response = match result {
        Ok(response) => response,
        Err(error) => {
            emit_turn_error(event_sink, &workspace_id, &thread_id, &turn_id, &error);
            return Err(error);
        }
    };

    if let Some(error_message) = extract_response_error_message(&response) {
        emit_turn_error(
            event_sink,
            &workspace_id,
            &thread_id,
            &turn_id,
            &error_message,
        );
        return Ok(response);
    }

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{WorkspaceKind, WorkspaceSettings};
    use tokio::runtime::Builder;

    #[test]
    fn response_is_method_not_found_checks_code_and_message() {
        let by_code = json!({
            "error": {
                "code": -32601,
                "message": "method not found"
            }
        });
        assert!(response_is_method_not_found(&by_code));

        let by_message = json!({
            "error": {
                "code": 42,
                "message": "Unknown method: unstable_setSessionModel"
            }
        });
        assert!(response_is_method_not_found(&by_message));

        let other = json!({
            "error": {
                "code": -32000,
                "message": "invalid params"
            }
        });
        assert!(!response_is_method_not_found(&other));
    }

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
    fn build_acp_prompt_parts_blocks_localhost_image_urls() {
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        runtime.block_on(async {
            let result = build_acp_prompt_parts(
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

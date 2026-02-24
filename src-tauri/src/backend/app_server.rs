use serde_json::{json, Value};
use std::collections::HashMap;
use std::env;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use reqwest_eventsource::{Event, EventSource};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, watch, Mutex, OnceCell};
use tokio::time::{sleep, timeout};

use crate::backend::event_translator::{self, SessionTranslationState};
use crate::backend::events::{AppServerEvent, EventSink};
use crate::codex::args::parse_codex_args;
use crate::shared::process_core::{kill_child_process_tree, tokio_command};
use crate::types::WorkspaceEntry;

#[cfg(target_os = "windows")]
use crate::shared::process_core::{build_cmd_c_command, resolve_windows_executable};
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

fn extract_thread_id(value: &Value) -> Option<String> {
    let params = value.get("params")?;

    params
        .get("threadId")
        .or_else(|| params.get("thread_id"))
        .and_then(|t| t.as_str())
        .map(|s| s.to_string())
        .or_else(|| {
            params
                .get("thread")
                .and_then(|thread| thread.get("id"))
                .and_then(|t| t.as_str())
                .map(|s| s.to_string())
        })
}

// ---------------------------------------------------------------------------
// OpenCode REST server process (singleton)
// ---------------------------------------------------------------------------

/// The single `opencode serve` process shared by all workspaces.
static SERVER_PROCESS: OnceCell<Mutex<ServerProcess>> = OnceCell::const_new();

/// Default port for `opencode serve`.
const REST_PORT: u16 = 14096;

struct ServerProcess {
    child: Child,
    base_url: String,
}

fn rest_base_url() -> String {
    format!("http://127.0.0.1:{REST_PORT}")
}

async fn start_managed_server_process(
    codex_bin: Option<String>,
    codex_args: Option<&str>,
) -> Result<ServerProcess, String> {
    let base_url = rest_base_url();
    let mut command = build_codex_command_with_bin(
        codex_bin,
        codex_args,
        vec![
            "serve".to_string(),
            "--port".to_string(),
            REST_PORT.to_string(),
        ],
    )?;
    command.stdin(std::process::Stdio::null());
    command.stdout(std::process::Stdio::null());
    command.stderr(std::process::Stdio::null());

    let child = command.spawn().map_err(|e| {
        if e.kind() == ErrorKind::NotFound {
            "OpenCode CLI not found. Install OpenCode and ensure `opencode` is on your PATH."
                .to_string()
        } else {
            e.to_string()
        }
    })?;

    let start = std::time::Instant::now();
    let health_timeout = Duration::from_secs(30);
    loop {
        if start.elapsed() > health_timeout {
            return Err("OpenCode server did not become healthy within 30 seconds.".to_string());
        }
        if health_check(&base_url).await.is_ok() {
            break;
        }
        sleep(Duration::from_millis(200)).await;
    }

    Ok(ServerProcess { child, base_url })
}

async fn ensure_server_running(
    codex_bin: Option<String>,
    codex_args: Option<&str>,
) -> Result<String, String> {
    let base_url = rest_base_url();

    // Fast path: if already initialized, just return the URL.
    if SERVER_PROCESS.get().is_some() {
        return Ok(base_url);
    }

    // If a server is already listening (e.g. user started one externally), use it.
    if health_check(&base_url).await.is_ok() {
        return Ok(base_url);
    }

    let init_result = SERVER_PROCESS
        .get_or_try_init(|| async {
            let server = start_managed_server_process(codex_bin, codex_args).await?;
            Ok::<Mutex<ServerProcess>, String>(Mutex::new(server))
        })
        .await;

    match init_result {
        Ok(_) => Ok(base_url),
        Err(e) => Err(e),
    }
}

async fn health_check(base_url: &str) -> Result<Value, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .get(format!("{base_url}/global/health"))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("health check returned {}", resp.status()));
    }
    resp.json::<Value>().await.map_err(|e| e.to_string())
}

pub(crate) async fn global_rest_get(
    codex_bin: Option<String>,
    codex_args: Option<&str>,
    path: &str,
    directory: Option<&str>,
) -> Result<Value, String> {
    let base_url = ensure_server_running(codex_bin, codex_args).await?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(300))
        .build()
        .map_err(|e| e.to_string())?;
    let mut url = format!("{base_url}{path}");
    if let Some(directory) = directory.filter(|value| !value.trim().is_empty()) {
        let separator = if path.contains('?') { "&" } else { "?" };
        url = format!("{url}{separator}directory={}", urlencoding::encode(directory));
    }
    let resp = client.get(&url).send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("REST GET {path} failed ({status}): {body}"));
    }
    resp.json::<Value>().await.map_err(|e| e.to_string())
}

pub(crate) async fn opencode_server_status() -> Value {
    let base_url = rest_base_url();
    let managed = SERVER_PROCESS.get().is_some();
    match health_check(&base_url).await {
        Ok(health) => json!({
            "baseUrl": base_url,
            "healthy": true,
            "managed": managed,
            "source": if managed { "managed" } else { "external" },
            "version": health.get("version").cloned().unwrap_or(Value::Null),
            "health": health,
        }),
        Err(error) => json!({
            "baseUrl": base_url,
            "healthy": false,
            "managed": managed,
            "source": if managed { "managed" } else { "none" },
            "version": Value::Null,
            "error": error,
        }),
    }
}

pub(crate) async fn restart_opencode_server(
    codex_bin: Option<String>,
    codex_args: Option<&str>,
) -> Result<Value, String> {
    let base_url = rest_base_url();
    if let Some(server_mutex) = SERVER_PROCESS.get() {
        let mut guard = server_mutex.lock().await;
        let _ = kill_child_process_tree(&mut guard.child).await;
        let replacement = start_managed_server_process(codex_bin, codex_args).await?;
        *guard = replacement;
        return Ok(json!({
            "restarted": true,
            "status": opencode_server_status().await,
        }));
    }

    if health_check(&base_url).await.is_ok() {
        return Err(format!(
            "OpenCode server at {base_url} is running but is not managed by OpenCode Monitor. Stop it manually, then retry."
        ));
    }

    let _ = SERVER_PROCESS
        .get_or_try_init(|| async {
            let server = start_managed_server_process(codex_bin, codex_args).await?;
            Ok::<Mutex<ServerProcess>, String>(Mutex::new(server))
        })
        .await?;

    Ok(json!({
        "restarted": true,
        "status": opencode_server_status().await,
    }))
}

// ---------------------------------------------------------------------------
// WorkspaceSession (REST-based)
// ---------------------------------------------------------------------------

pub(crate) struct WorkspaceSession {
    pub(crate) entry: WorkspaceEntry,
    /// HTTP client for REST calls to the OpenCode server.
    pub(crate) http_client: reqwest::Client,
    /// Base URL of the OpenCode server (e.g. "http://127.0.0.1:14096").
    pub(crate) base_url: String,
    /// Callbacks for background threads — events for these threadIds are sent
    /// through the channel instead of the main event sink.
    pub(crate) background_thread_callbacks: Mutex<HashMap<String, mpsc::UnboundedSender<Value>>>,
    /// SSE → CodexMonitor event translation state (turn IDs, item IDs, tool-call mapping).
    pub(crate) translation_state: Mutex<SessionTranslationState>,
    /// Cached model/provider data from `GET /config/providers`.
    pub(crate) models_cache: Mutex<Option<Value>>,
    /// One in-flight prompt at a time per workspace session.
    pub(crate) prompt_lock: Mutex<()>,
    /// Sender to signal SSE reader shutdown when workspace disconnects.
    shutdown_tx: watch::Sender<bool>,
}

impl WorkspaceSession {
    /// Signal the SSE reader task to shut down.
    pub(crate) fn shutdown(&self) {
        let _ = self.shutdown_tx.send(true);
    }
}

async fn route_translated_event_to_background_callback(
    callbacks: &Mutex<HashMap<String, mpsc::UnboundedSender<Value>>>,
    translated_message: &Value,
) -> bool {
    let Some(thread_id) = extract_thread_id(translated_message) else {
        return false;
    };
    let callbacks = callbacks.lock().await;
    if let Some(tx) = callbacks.get(&thread_id) {
        let _ = tx.send(translated_message.clone());
        return true;
    }
    false
}

impl WorkspaceSession {
    /// Send a GET request to the OpenCode REST API, scoped to this workspace.
    pub(crate) async fn rest_get(&self, path: &str) -> Result<Value, String> {
        let separator = if path.contains('?') { "&" } else { "?" };
        let url = format!(
            "{}{path}{separator}directory={}",
            self.base_url,
            urlencoding::encode(&self.entry.path)
        );
        let resp = self
            .http_client
            .get(&url)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("REST GET {path} failed ({status}): {body}"));
        }
        resp.json::<Value>().await.map_err(|e| e.to_string())
    }

    /// Send a POST request to the OpenCode REST API, scoped to this workspace.
    pub(crate) async fn rest_post(&self, path: &str, body: Value) -> Result<Value, String> {
        let separator = if path.contains('?') { "&" } else { "?" };
        let url = format!(
            "{}{path}{separator}directory={}",
            self.base_url,
            urlencoding::encode(&self.entry.path)
        );
        let resp = self
            .http_client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let status = resp.status();
        if status == reqwest::StatusCode::NO_CONTENT {
            return Ok(Value::Null);
        }
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("REST POST {path} failed ({status}): {body}"));
        }
        // Some endpoints return empty body on success.
        let text = resp.text().await.map_err(|e| e.to_string())?;
        if text.trim().is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_str(&text)
            .map_err(|e| format!("Failed to parse response from {path}: {e}"))
    }

    /// Send a POST request that returns a boolean (e.g. abort, permissions).
    pub(crate) async fn rest_post_bool(&self, path: &str, body: Value) -> Result<bool, String> {
        let separator = if path.contains('?') { "&" } else { "?" };
        let url = format!(
            "{}{path}{separator}directory={}",
            self.base_url,
            urlencoding::encode(&self.entry.path)
        );
        let resp = self
            .http_client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("REST POST {path} failed ({status}): {body}"));
        }
        let text = resp.text().await.map_err(|e| e.to_string())?;
        // Parse as bool; fall back to true on success status.
        Ok(text.trim().parse::<bool>().unwrap_or(true))
    }

    /// Send a PATCH request to the OpenCode REST API, scoped to this workspace.
    pub(crate) async fn rest_patch(&self, path: &str, body: Value) -> Result<Value, String> {
        let separator = if path.contains('?') { "&" } else { "?" };
        let url = format!(
            "{}{path}{separator}directory={}",
            self.base_url,
            urlencoding::encode(&self.entry.path)
        );
        let resp = self
            .http_client
            .patch(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("REST PATCH {path} failed ({status}): {body}"));
        }
        let text = resp.text().await.map_err(|e| e.to_string())?;
        if text.trim().is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_str(&text)
            .map_err(|e| format!("Failed to parse response from {path}: {e}"))
    }
}

// ---------------------------------------------------------------------------
// URL encoding helper (inline, no extra dep)
// ---------------------------------------------------------------------------

mod urlencoding {
    use std::fmt::Write;

    pub(crate) fn encode(input: &str) -> String {
        let mut out = String::with_capacity(input.len() * 3);
        for byte in input.bytes() {
            match byte {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    out.push(byte as char)
                }
                _ => {
                    let _ = write!(out, "%{byte:02X}");
                }
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------
// PATH env and command building
// ---------------------------------------------------------------------------

pub(crate) fn build_codex_path_env(codex_bin: Option<&str>) -> Option<String> {
    let mut paths: Vec<PathBuf> = env::var_os("PATH")
        .map(|value| env::split_paths(&value).collect())
        .unwrap_or_default();

    let mut extras: Vec<PathBuf> = Vec::new();

    #[cfg(not(target_os = "windows"))]
    {
        extras.extend(
            [
                "/opt/homebrew/bin",
                "/usr/local/bin",
                "/usr/bin",
                "/bin",
                "/usr/sbin",
                "/sbin",
            ]
            .into_iter()
            .map(PathBuf::from),
        );

        if let Ok(home) = env::var("HOME") {
            let home_path = Path::new(&home);
            extras.push(home_path.join(".local/bin"));
            extras.push(home_path.join(".local/share/mise/shims"));
            extras.push(home_path.join(".cargo/bin"));
            extras.push(home_path.join(".bun/bin"));
            let nvm_root = home_path.join(".nvm/versions/node");
            if let Ok(entries) = std::fs::read_dir(nvm_root) {
                for entry in entries.flatten() {
                    let bin_path = entry.path().join("bin");
                    if bin_path.is_dir() {
                        extras.push(bin_path);
                    }
                }
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        if let Ok(appdata) = env::var("APPDATA") {
            extras.push(Path::new(&appdata).join("npm"));
        }
        if let Ok(local_app_data) = env::var("LOCALAPPDATA") {
            extras.push(
                Path::new(&local_app_data)
                    .join("Microsoft")
                    .join("WindowsApps"),
            );
        }
        if let Ok(home) = env::var("USERPROFILE").or_else(|_| env::var("HOME")) {
            let home_path = Path::new(&home);
            extras.push(home_path.join(".cargo").join("bin"));
            extras.push(home_path.join("scoop").join("shims"));
        }
        if let Ok(program_data) = env::var("PROGRAMDATA") {
            extras.push(Path::new(&program_data).join("chocolatey").join("bin"));
        }
    }

    if let Some(bin_path) = codex_bin.filter(|value| !value.trim().is_empty()) {
        if let Some(parent) = Path::new(bin_path).parent() {
            extras.push(parent.to_path_buf());
        }
    }

    for extra in extras {
        if !paths.iter().any(|path| path == &extra) {
            paths.push(extra);
        }
    }

    if paths.is_empty() {
        return None;
    }

    env::join_paths(paths)
        .ok()
        .map(|joined| joined.to_string_lossy().to_string())
}

pub(crate) fn build_codex_command_with_bin(
    codex_bin: Option<String>,
    codex_args: Option<&str>,
    args: Vec<String>,
) -> Result<Command, String> {
    let bin = codex_bin
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "opencode".into());

    let path_env = build_codex_path_env(codex_bin.as_deref());
    let mut command_args = parse_codex_args(codex_args)?;
    command_args.extend(args);

    #[cfg(target_os = "windows")]
    let mut command = {
        let bin_trimmed = bin.trim();
        let resolved = resolve_windows_executable(bin_trimmed, path_env.as_deref());
        let resolved_path = resolved
            .as_deref()
            .unwrap_or_else(|| Path::new(bin_trimmed));
        let ext = resolved_path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_ascii_lowercase());

        if matches!(ext.as_deref(), Some("cmd") | Some("bat")) {
            let mut command = tokio_command("cmd");
            let command_line = build_cmd_c_command(resolved_path, &command_args)?;
            command.arg("/D");
            command.arg("/S");
            command.arg("/C");
            command.raw_arg(command_line);
            command
        } else {
            let mut command = tokio_command(resolved_path);
            command.args(command_args);
            command
        }
    };

    #[cfg(not(target_os = "windows"))]
    let mut command = {
        let mut command = tokio_command(bin.trim());
        command.args(command_args);
        command
    };

    if let Some(path_env) = path_env {
        command.env("PATH", path_env);
    }
    Ok(command)
}

pub(crate) async fn check_codex_installation(
    codex_bin: Option<String>,
) -> Result<Option<String>, String> {
    let mut command = build_codex_command_with_bin(codex_bin, None, vec!["--version".to_string()])?;
    command.stdout(std::process::Stdio::piped());
    command.stderr(std::process::Stdio::piped());

    let output = match timeout(Duration::from_secs(5), command.output()).await {
        Ok(result) => result.map_err(|e| {
            if e.kind() == ErrorKind::NotFound {
                "OpenCode CLI not found. Install OpenCode and ensure `opencode` is on your PATH."
                    .to_string()
            } else {
                e.to_string()
            }
        })?,
        Err(_) => {
            return Err(
                "Timed out checking OpenCode CLI. Make sure `opencode --version` runs in Terminal."
                    .to_string(),
            );
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let detail = if stderr.trim().is_empty() {
            stdout.trim()
        } else {
            stderr.trim()
        };
        if detail.is_empty() {
            return Err(
                "OpenCode CLI failed. Try running `opencode --version` in Terminal.".to_string(),
            );
        }
        return Err(format!(
            "OpenCode CLI failed: {detail}. Try running `opencode --version` in Terminal."
        ));
    }

    let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(if version.is_empty() {
        None
    } else {
        Some(version)
    })
}

// ---------------------------------------------------------------------------
// SSE reader task
// ---------------------------------------------------------------------------

fn spawn_sse_reader<E: EventSink>(
    session: Arc<WorkspaceSession>,
    workspace_id: String,
    mut shutdown_rx: watch::Receiver<bool>,
    event_sink: E,
) {
    let workspace_path = session.entry.path.clone();

    tokio::spawn(async move {
        let url = format!("{}/global/event", session.base_url);

        let mut reconnect_delay = Duration::from_millis(500);
        let max_reconnect_delay = Duration::from_secs(10);

        'outer: loop {
            if *shutdown_rx.borrow() {
                break;
            }

            let mut es = EventSource::get(&url);

            loop {
                tokio::select! {
                    _ = shutdown_rx.changed() => {
                        es.close();
                        break 'outer;
                    }
                    event_opt = es.next() => {
                        let Some(event_result) = event_opt else {
                            break;
                        };

                        reconnect_delay = Duration::from_millis(500);

                        match event_result {
                            Ok(Event::Open) => {}
                            Ok(Event::Message(msg)) => {
                                let value: Value = match serde_json::from_str(&msg.data) {
                                    Ok(v) => v,
                                    Err(err) => {
                                        let payload = AppServerEvent {
                                            workspace_id: workspace_id.clone(),
                                            message: json!({
                                                "method": "codex/parseError",
                                                "params": { "error": err.to_string(), "raw": msg.data },
                                            }),
                                        };
                                        event_sink.emit_app_server_event(payload);
                                        continue;
                                    }
                                };

                                // OpenCode SSE format wraps events:
                                //   { "directory": "...", "payload": { "type": "...", "properties": {...} } }
                                // Extract directory from top level; unwrap payload for translation.
                                let event_dir = value
                                    .get("directory")
                                    .and_then(|d| d.as_str())
                                    .unwrap_or("");
                                if !event_dir.is_empty() && event_dir != workspace_path {
                                    continue;
                                }

                                let payload = match value.get("payload") {
                                    Some(p) => p,
                                    None => &value, // fallback: treat top-level as payload
                                };

                                let translated = {
                                    let mut ts = session.translation_state.lock().await;
                                    event_translator::translate_sse_event(payload, &mut ts)
                                };

                                for translated_msg in translated {
                                    let sent_to_background =
                                        route_translated_event_to_background_callback(
                                            &session.background_thread_callbacks,
                                            &translated_msg,
                                        )
                                        .await;
                                    if !sent_to_background {
                                        let payload = AppServerEvent {
                                            workspace_id: workspace_id.clone(),
                                            message: translated_msg,
                                        };
                                        event_sink.emit_app_server_event(payload);
                                    }
                                }
                            }
                            Err(reqwest_eventsource::Error::StreamEnded) => {
                                break;
                            }
                            Err(_err) => {
                                #[cfg(debug_assertions)]
                                eprintln!("[sse_reader] SSE error for {workspace_id}: {_err}");
                                break;
                            }
                        }
                    }
                }
            }

            if *shutdown_rx.borrow() {
                break;
            }

            sleep(reconnect_delay).await;
            reconnect_delay = (reconnect_delay * 2).min(max_reconnect_delay);
        }
    });
}

// ---------------------------------------------------------------------------
// Workspace session lifecycle
// ---------------------------------------------------------------------------

pub(crate) async fn spawn_workspace_session<E: EventSink>(
    entry: WorkspaceEntry,
    default_codex_bin: Option<String>,
    codex_args: Option<String>,
    _codex_home: Option<PathBuf>,
    _client_version: String,
    event_sink: E,
) -> Result<Arc<WorkspaceSession>, String> {
    let codex_bin = entry
        .codex_bin
        .clone()
        .filter(|value| !value.trim().is_empty())
        .or(default_codex_bin);
    let _ = check_codex_installation(codex_bin.clone()).await?;

    // Ensure the shared `opencode serve` process is running.
    let base_url = ensure_server_running(codex_bin, codex_args.as_deref()).await?;

    let http_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(300))
        .build()
        .map_err(|e| e.to_string())?;

    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    let session = Arc::new(WorkspaceSession {
        entry: entry.clone(),
        http_client,
        base_url,
        background_thread_callbacks: Mutex::new(HashMap::new()),
        translation_state: Mutex::new(SessionTranslationState::new(String::new())),
        models_cache: Mutex::new(None),
        prompt_lock: Mutex::new(()),
        shutdown_tx,
    });

    spawn_sse_reader(
        Arc::clone(&session),
        entry.id.clone(),
        shutdown_rx,
        event_sink.clone(),
    );

    // Emit connected event.
    let payload = AppServerEvent {
        workspace_id: entry.id.clone(),
        message: json!({
            "method": "codex/connected",
            "params": { "workspaceId": entry.id.clone() }
        }),
    };
    event_sink.emit_app_server_event(payload);

    // Eagerly fetch providers to populate the model selector.
    let prewarm_session = Arc::clone(&session);
    let prewarm_sink = event_sink.clone();
    let prewarm_workspace_id = entry.id.clone();
    tokio::spawn(async move {
        // Fetch provider/model config.
        match prewarm_session.rest_get("/config/providers").await {
            Ok(providers) => {
                let context_windows = event_translator::extract_model_context_windows(&providers);
                *prewarm_session.models_cache.lock().await = Some(providers);
                let mut state = prewarm_session.translation_state.lock().await;
                state.replace_model_context_windows(context_windows);
                drop(state);

                let payload = AppServerEvent {
                    workspace_id: prewarm_workspace_id.clone(),
                    message: json!({
                        "method": "codex/modelsReady",
                        "params": { "workspaceId": prewarm_workspace_id }
                    }),
                };
                prewarm_sink.emit_app_server_event(payload);
            }
            Err(err) => {
                eprintln!(
                    "Pre-warm GET /config/providers failed for {}: {}",
                    prewarm_workspace_id, err
                );
            }
        }
    });

    Ok(session)
}

/// Shut down the shared OpenCode server process (called on app exit).
pub(crate) async fn shutdown_server() {
    if let Some(server_mutex) = SERVER_PROCESS.get() {
        let mut server = server_mutex.lock().await;
        let _ = kill_child_process_tree(&mut server.child).await;
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::{extract_thread_id, route_translated_event_to_background_callback};
    use serde_json::{json, Value};
    use std::collections::HashMap;
    use tokio::runtime::Builder;
    use tokio::sync::{mpsc, Mutex};

    #[test]
    fn extract_thread_id_reads_camel_case() {
        let value = json!({ "params": { "threadId": "thread-123" } });
        assert_eq!(extract_thread_id(&value), Some("thread-123".to_string()));
    }

    #[test]
    fn extract_thread_id_reads_snake_case() {
        let value = json!({ "params": { "thread_id": "thread-456" } });
        assert_eq!(extract_thread_id(&value), Some("thread-456".to_string()));
    }

    #[test]
    fn extract_thread_id_returns_none_when_missing() {
        let value = json!({ "params": {} });
        assert_eq!(extract_thread_id(&value), None);
    }

    #[test]
    fn routed_translated_event_goes_to_background_callback() {
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        runtime.block_on(async {
            let (tx, mut rx) = mpsc::unbounded_channel();
            let callbacks = Mutex::new(HashMap::from([("ses_bg".to_string(), tx)]));
            let event = json!({
                "method": "item/agentMessage/delta",
                "params": {
                    "threadId": "ses_bg",
                    "delta": "hello"
                }
            });

            let routed = route_translated_event_to_background_callback(&callbacks, &event).await;
            assert!(routed);
            let received = rx.recv().await.expect("background callback event");
            assert_eq!(received["method"], "item/agentMessage/delta");
            assert_eq!(received["params"]["threadId"], "ses_bg");
        });
    }

    #[test]
    fn untranslated_event_without_callback_falls_back_to_sink_path() {
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        runtime.block_on(async {
            let callbacks = Mutex::new(HashMap::<String, mpsc::UnboundedSender<Value>>::new());
            let event = json!({
                "method": "item/agentMessage/delta",
                "params": {
                    "threadId": "ses_fg",
                    "delta": "hello"
                }
            });

            let routed = route_translated_event_to_background_callback(&callbacks, &event).await;
            assert!(!routed);
        });
    }

    #[test]
    fn urlencoding_handles_special_chars() {
        assert_eq!(super::urlencoding::encode("/tmp/test"), "%2Ftmp%2Ftest");
        assert_eq!(super::urlencoding::encode("hello world"), "hello%20world");
        assert_eq!(
            super::urlencoding::encode("abc-def_123.txt~"),
            "abc-def_123.txt~"
        );
    }
}

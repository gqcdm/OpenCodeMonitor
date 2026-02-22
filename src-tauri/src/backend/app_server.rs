use serde_json::{json, Value};
use std::collections::HashMap;
use std::env;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio::time::timeout;

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

fn build_initialize_params(_client_version: &str) -> Value {
    // ACP v1 protocol: only protocolVersion is required.
    // No clientInfo/capabilities/initialized notification needed.
    json!({
        "protocolVersion": 1
    })
}

const REQUEST_TIMEOUT: Duration = Duration::from_secs(300);

pub(crate) struct WorkspaceSession {
    pub(crate) entry: WorkspaceEntry,
    pub(crate) child: Mutex<Child>,
    pub(crate) stdin: Mutex<ChildStdin>,
    pub(crate) pending: Mutex<HashMap<u64, oneshot::Sender<Value>>>,
    pub(crate) next_id: AtomicU64,
    /// Callbacks for background threads - events for these threadIds are sent through the channel
    pub(crate) background_thread_callbacks: Mutex<HashMap<String, mpsc::UnboundedSender<Value>>>,
    /// ACP → CodexMonitor event translation state (turn IDs, item IDs, tool-call mapping).
    pub(crate) translation_state: Mutex<SessionTranslationState>,
    /// Cached ACP model payload from `session/new`/`session/load`.
    pub(crate) models_cache: Mutex<Option<Value>>,
    /// Pre-warmed ACP session ID created eagerly on workspace connect.
    /// Consumed by the first `start_thread` call to avoid a duplicate `session/new`.
    pub(crate) prewarmed_session_id: Mutex<Option<String>>,
    /// One in-flight `session/prompt` at a time per workspace session.
    pub(crate) prompt_lock: Mutex<()>,
    /// Capability state for best-effort model switching.
    pub(crate) model_set_capability: Mutex<ModelSetCapability>,
    /// Emit model-set warning only once per workspace session.
    pub(crate) model_set_warning_emitted: Mutex<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ModelSetMethod {
    UnstableSetSessionModelId,
    UnstableSetSessionModel,
    SessionSetModelId,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ModelSetCapability {
    Unknown,
    Supported(ModelSetMethod),
    Unsupported,
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
    async fn write_message(&self, value: Value) -> Result<(), String> {
        let mut stdin = self.stdin.lock().await;
        let mut line = serde_json::to_string(&value).map_err(|e| e.to_string())?;
        line.push('\n');
        stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|e| e.to_string())
    }

    pub(crate) async fn send_request(&self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id, tx);
        if let Err(error) = self
            .write_message(json!({ "id": id, "method": method, "params": params }))
            .await
        {
            self.pending.lock().await.remove(&id);
            return Err(error);
        }
        match timeout(REQUEST_TIMEOUT, rx).await {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(_)) => Err("request canceled".to_string()),
            Err(_) => {
                self.pending.lock().await.remove(&id);
                Err(format!(
                    "request timed out after {} seconds",
                    REQUEST_TIMEOUT.as_secs()
                ))
            }
        }
    }

    pub(crate) async fn send_notification(
        &self,
        method: &str,
        params: Option<Value>,
    ) -> Result<(), String> {
        let value = if let Some(params) = params {
            json!({ "method": method, "params": params })
        } else {
            json!({ "method": method })
        };
        self.write_message(value).await
    }

    pub(crate) async fn send_response(&self, id: Value, result: Value) -> Result<(), String> {
        self.write_message(json!({ "id": id, "result": result }))
            .await
    }
}

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

pub(crate) async fn spawn_workspace_session<E: EventSink>(
    entry: WorkspaceEntry,
    default_codex_bin: Option<String>,
    codex_args: Option<String>,
    _codex_home: Option<PathBuf>,
    client_version: String,
    event_sink: E,
    acp_port: u16,
) -> Result<Arc<WorkspaceSession>, String> {
    let codex_bin = entry
        .codex_bin
        .clone()
        .filter(|value| !value.trim().is_empty())
        .or(default_codex_bin);
    let _ = check_codex_installation(codex_bin.clone()).await?;

    let mut command = build_codex_command_with_bin(
        codex_bin,
        codex_args.as_deref(),
        vec![
            "acp".to_string(),
            "--port".to_string(),
            acp_port.to_string(),
            "--cwd".to_string(),
            entry.path.clone(),
        ],
    )?;
    // Don't set current_dir — ACP uses --cwd instead.
    // Don't set CODEX_HOME — OpenCode uses ~/.config/opencode/.
    command.stdin(std::process::Stdio::piped());
    command.stdout(std::process::Stdio::piped());
    command.stderr(std::process::Stdio::piped());

    let mut child = command.spawn().map_err(|e| e.to_string())?;
    let stdin = child.stdin.take().ok_or("missing stdin")?;
    let stdout = child.stdout.take().ok_or("missing stdout")?;
    let stderr = child.stderr.take().ok_or("missing stderr")?;

    let session = Arc::new(WorkspaceSession {
        entry: entry.clone(),
        child: Mutex::new(child),
        stdin: Mutex::new(stdin),
        pending: Mutex::new(HashMap::new()),
        next_id: AtomicU64::new(1),
        background_thread_callbacks: Mutex::new(HashMap::new()),
        translation_state: Mutex::new(SessionTranslationState::new(String::new())),
        models_cache: Mutex::new(None),
        prewarmed_session_id: Mutex::new(None),
        prompt_lock: Mutex::new(()),
        model_set_capability: Mutex::new(ModelSetCapability::Unknown),
        model_set_warning_emitted: Mutex::new(false),
    });

    let session_clone = Arc::clone(&session);
    let workspace_id = entry.id.clone();
    let event_sink_clone = event_sink.clone();
    tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if line.trim().is_empty() {
                continue;
            }
            let value: Value = match serde_json::from_str(&line) {
                Ok(value) => value,
                Err(err) => {
                    let payload = AppServerEvent {
                        workspace_id: workspace_id.clone(),
                        message: json!({
                            "method": "codex/parseError",
                            "params": { "error": err.to_string(), "raw": line },
                        }),
                    };
                    event_sink_clone.emit_app_server_event(payload);
                    continue;
                }
            };

            let maybe_id = value.get("id").and_then(|id| id.as_u64());
            let method = value
                .get("method")
                .and_then(|m| m.as_str())
                .map(String::from);
            let has_result_or_error = value.get("result").is_some() || value.get("error").is_some();

            if let Some(id) = maybe_id {
                if has_result_or_error {
                    // Response to a request we sent — resolve the pending oneshot.
                    if let Some(tx) = session_clone.pending.lock().await.remove(&id) {
                        let _ = tx.send(value);
                    }
                } else if let Some(ref m) = method {
                    // JSON-RPC request FROM ACP (has id + method) — e.g. requestPermission.
                    if m == "requestPermission" {
                        let session_id = {
                            session_clone
                                .translation_state
                                .lock()
                                .await
                                .session_id
                                .clone()
                        };
                        if let Some(translated) =
                            event_translator::translate_permission_request(&value, &session_id)
                        {
                            let payload = AppServerEvent {
                                workspace_id: workspace_id.clone(),
                                message: translated,
                            };
                            event_sink_clone.emit_app_server_event(payload);
                        }
                    } else {
                        // Unknown server→client request — forward as-is.
                        let payload = AppServerEvent {
                            workspace_id: workspace_id.clone(),
                            message: value,
                        };
                        event_sink_clone.emit_app_server_event(payload);
                    }
                } else if let Some(tx) = session_clone.pending.lock().await.remove(&id) {
                    let _ = tx.send(value);
                }
            } else if let Some(ref m) = method {
                // Notification (no id).
                if m == "session/update" {
                    // ACP sessionUpdate → translate to CodexMonitor event(s).
                    let translated = {
                        let mut ts = session_clone.translation_state.lock().await;
                        event_translator::translate_acp_event(&value, &mut ts)
                    };
                    for msg in translated {
                        let sent_to_background = route_translated_event_to_background_callback(
                            &session_clone.background_thread_callbacks,
                            &msg,
                        )
                        .await;
                        if !sent_to_background {
                            let payload = AppServerEvent {
                                workspace_id: workspace_id.clone(),
                                message: msg,
                            };
                            event_sink_clone.emit_app_server_event(payload);
                        }
                    }
                } else {
                    // Non-sessionUpdate notification — check background callbacks or forward.
                    let thread_id = extract_thread_id(&value);
                    let mut sent_to_background = false;
                    if let Some(ref tid) = thread_id {
                        let callbacks = session_clone.background_thread_callbacks.lock().await;
                        if let Some(tx) = callbacks.get(tid) {
                            let _ = tx.send(value.clone());
                            sent_to_background = true;
                        }
                    }
                    if !sent_to_background {
                        let payload = AppServerEvent {
                            workspace_id: workspace_id.clone(),
                            message: value,
                        };
                        event_sink_clone.emit_app_server_event(payload);
                    }
                }
            }
        }

        // Signal frontend that the ACP process has disconnected.
        event_sink_clone.emit_app_server_event(AppServerEvent {
            workspace_id: workspace_id.clone(),
            message: json!({
                "method": "codex/disconnected",
                "params": {}
            }),
        });

        session_clone.pending.lock().await.clear();
    });

    let workspace_id = entry.id.clone();
    let event_sink_clone = event_sink.clone();
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if line.trim().is_empty() {
                continue;
            }
            let payload = AppServerEvent {
                workspace_id: workspace_id.clone(),
                message: json!({
                    "method": "codex/stderr",
                    "params": { "message": line },
                }),
            };
            event_sink_clone.emit_app_server_event(payload);
        }
    });

    let init_params = build_initialize_params(&client_version);
    let init_result = timeout(
        Duration::from_secs(30),
        session.send_request("initialize", init_params),
    )
    .await;
    let init_response = match init_result {
        Ok(response) => response,
        Err(_) => {
            let mut child = session.child.lock().await;
            kill_child_process_tree(&mut child).await;
            return Err(
                "OpenCode ACP did not respond to initialize. Check that `opencode acp` works in Terminal."
                    .to_string(),
            );
        }
    };
    init_response?;
    // ACP v1: no `initialized` notification needed (unlike Codex).

    let payload = AppServerEvent {
        workspace_id: entry.id.clone(),
        message: json!({
            "method": "codex/connected",
            "params": { "workspaceId": entry.id.clone() }
        }),
    };
    event_sink.emit_app_server_event(payload);

    // Eagerly create a session to pre-populate the models cache so the
    // frontend model selector is populated before the user sends a prompt.
    let prewarm_session = Arc::clone(&session);
    let prewarm_sink = event_sink.clone();
    let prewarm_workspace_id = entry.id.clone();
    let prewarm_cwd = entry.path.clone();
    tokio::spawn(async move {
        let params = json!({
            "cwd": prewarm_cwd,
            "mcpServers": []
        });
        match prewarm_session.send_request("session/new", params).await {
            Ok(response) => {
                let session_id = response
                    .get("result")
                    .and_then(|r| r.get("sessionId"))
                    .or_else(|| response.get("sessionId"))
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();

                if let Some(models) = response
                    .get("result")
                    .unwrap_or(&response)
                    .get("models")
                    .cloned()
                {
                    *prewarm_session.models_cache.lock().await = Some(models);
                }

                if !session_id.is_empty() {
                    *prewarm_session.prewarmed_session_id.lock().await =
                        Some(session_id);
                }

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
                    "Pre-warm session/new failed for {}: {}",
                    prewarm_workspace_id, err
                );
            }
        }
    });

    Ok(session)
}

#[cfg(test)]
mod tests {
    use super::{
        build_initialize_params, extract_thread_id, route_translated_event_to_background_callback,
    };
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
    fn build_initialize_params_sets_protocol_version() {
        let params = build_initialize_params("1.2.3");
        assert_eq!(
            params
                .get("protocolVersion")
                .and_then(|value| value.as_u64()),
            Some(1)
        );
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
}

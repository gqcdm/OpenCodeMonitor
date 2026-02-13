# Spec: OpenCodeMonitor — Fork CodexMonitor for OpenCode

**Status**: Wire-capture validated — ready for implementation  
**Date**: 2026-02-13  
**Author**: Claude (spec), Jacob (owner)  
**Source repo**: https://github.com/Dimillian/CodexMonitor.git  
**Target backend**: OpenCode ACP (`opencode acp`)

---

## 1. Requirements & Assumptions

### Functional Requirements (MVP)
- FR1: Create/resume/list sessions (Codex "threads" → OpenCode "sessions")
- FR2: Send prompts, stream agent text/reasoning/tool-call responses in real time
- FR3: Permission/approval flow for tool executions (approve/deny in UI)
- FR4: Interrupt running sessions
- FR5: Workspace management (unchanged — independent of Codex/OpenCode)
- FR6: Git integration (unchanged)
- FR7: File tree browsing (unchanged)

### Non-Functional Requirements
- NFR1: Multi-workspace concurrent operation (multiple `opencode acp` processes simultaneously)
- NFR2: Crash resilience — detect process death, surface error, allow reconnect
- NFR3: Pin to specific OpenCode version (ACP has `unstable_` methods)

### Explicit Assumptions
- **VERIFIED**: Both protocols use stdio nd-JSON-RPC — transport compatible
- **VERIFIED**: ACP has `session/new`, `session/load`, `session/prompt`, `cancel` — core flow exists
- **VERIFIED**: ACP streams events via `sessionUpdate` notifications (agent.ts implements streaming despite README caveats)
- **VERIFIED**: `opencode acp` spawns internal HTTP server on port 4096 by default — **must pass `--port` per workspace instance**
- **VERIFIED**: CodexMonitor already has approval UI (`useThreadApprovalEvents.ts`, `respondToServerRequest`) — can be adapted
- **VERIFIED**: ACP has `unstable_listSessions`, `unstable_forkSession`, `unstable_resumeSession` — exist but unstable
- **VERIFIED**: `session/list` filters by `cwd` — confirmed via wire capture (Appendix A.6)
- **VERIFIED**: Streaming works — events arrive before prompt response (Appendix A.3). README caveat is outdated.
- **VERIFIED**: `session/load` replays full session history as events (Appendix A.7)
- **VERIFIED**: Tool lifecycle events are rich: pending → in_progress → completed/failed with rawInput/rawOutput (Appendix A.4)
- **NOTED**: Permission requests are auto-approved by ACP client — approval UI will be dormant until OpenCode changes this (Appendix A.5)
- **NOTED**: Session creation takes 11-17 seconds (provider loading) — frontend needs loading state

### Non-Goals (MVP)
- Collaboration modes, Apps, Skills autocomplete
- Account login/auth via ACP (OpenCode uses `opencode auth login` separately)
- Rate limits meter
- Turn steering (mid-turn injection — no ACP equivalent)
- Remote/daemon backend mode
- Dictation
- iOS support

---

## 2. Design & Architecture

### 2.1 High-Level Approach

**Strategy: Backend adapter + event translation layer. Preserve frontend mostly intact.**

```
┌─────────────────────────────────────────────────────────────┐
│                     React Frontend                           │
│  (minimal changes — update event type guards, hide stubs)    │
└────────────────────┬────────────────────────────────────────┘
                     │ Tauri IPC (unchanged surface)
┌────────────────────▼────────────────────────────────────────┐
│                  Rust Backend (Tauri)                         │
│                                                              │
│  codex/mod.rs ─────► opencode_core.rs ──┐                    │
│  (Tauri cmd adapter)  (new: protocol    │                    │
│                        translation)     │                    │
│                                         ▼                    │
│  backend/app_server.rs ─── WorkspaceSession ───► stdio ──►  │
│  (spawn opencode acp     (pending reqs,         nd-JSON     │
│   --port <N> --cwd <P>)   event dispatch)                   │
│                                                              │
│  event_translator.rs ◄── ACP sessionUpdate notifications     │
│  (NEW: maps ACP events   → CodexMonitor AppServerEvent       │
│   to expected shapes)     format for frontend)               │
└──────────────────────────────────────────────────────────────┘
                     │ stdio (nd-JSON-RPC)
┌────────────────────▼────────────────────────────────────────┐
│               opencode acp (child process)                   │
│  Internal HTTP server (port N) ← SDK ← ACP Agent            │
└──────────────────────────────────────────────────────────────┘
```

### 2.2 Key Design Decisions

**D1: Translate events in Rust, not frontend.**
Reason: The frontend's thread reducer, event router, and ~30 event handlers would require massive changes if we changed event shapes. Instead, the Rust stdout reader loop will intercept ACP `sessionUpdate` JSON and re-emit it in the CodexMonitor event format. This keeps the React layer's changes minimal (mostly hiding UI elements for stubbed features).

**D2: Synthesize `turn/started` and `turn/completed` events.**
ACP has no turn concept. When `session/prompt` is called, we emit a synthetic `turn/started`. When the prompt response completes (ACP returns from `prompt`), we emit a synthetic `turn/completed`. This keeps the frontend reducer working without structural changes.

**D3: Port allocation via atomic counter.**
Each workspace gets a unique port starting from a configurable base (e.g., 14096). The `AppState` holds an `AtomicU16` port counter. When spawning `opencode acp`, pass `--port <next_port>`.

**D4: Permission flow maps directly.**
CodexMonitor's existing approval flow (approval event → `addApproval` dispatch → user clicks approve/deny → `respondToServerRequest`) maps to ACP's permission flow. The translation layer intercepts ACP's `requestPermission` JSON-RPC request (which has an `id` field expecting a response) and emits it as the existing `requestApproval`-style event. When the frontend calls `respondToServerRequest`, the Rust backend replies to that JSON-RPC `id`.

### 2.3 Protocol Method Mapping

| CodexMonitor (current) | OpenCode ACP | Notes |
|---|---|---|
| `initialize` + `initialized` | `initialize` (ACP v1) | Different params shape, no `initialized` notification needed |
| `thread/start` | `session/new` | Pass `cwd`, `mcpServers: []` |
| `thread/resume` | `session/load` | Pass `sessionId`, `cwd`, `mcpServers: []` |
| `thread/list` | `session/list` | Accepts `cwd` param to scope to workspace. **Confirmed working.** |
| `thread/fork` | `unstable_forkSession` | Shape differs; full-session fork only (no mid-conversation) |
| `thread/archive` | N/A | Stub: local-only metadata (mark archived in `localStorage`) |
| `thread/compact/start` | `/compact` command via `session/prompt` | Send `/compact` as prompt text |
| `thread/name/set` | N/A | Stub: store locally |
| `turn/start` (`send_user_message`) | `session/prompt` | Transform input format (see §2.4) |
| `turn/interrupt` | `cancel` notification | Map `threadId`+`turnId` → `sessionId` |
| `turn/steer` | N/A | **Drop for MVP** — no ACP equivalent |
| `model/list` | Returned in `session/new` response (`models` field) | Cache from session creation response |
| `skills/list` | N/A | Stub: empty array |
| `account/read` | N/A | Stub: return minimal shape |
| `account/rateLimits/read` | `usage_update` sessionUpdate | Can surface token usage from events |
| `account/login/start` | N/A | Remove flow; auth is external (`opencode auth login`) |
| `mcpServerStatus/list` | N/A | Stub |
| `review/start` | N/A | Stub |
| `collaborationMode/list` | N/A | Stub |
| `app/list` | N/A | Stub |
| `respond_to_server_request` | JSON-RPC response to `requestPermission` | Direct mapping (see §2.5) |

### 2.4 Input Format Translation

**Codex `turn/start` input:**
```json
{
  "threadId": "...",
  "input": [
    {"type": "text", "text": "..."},
    {"type": "image", "url": "..."},
    {"type": "localImage", "path": "..."},
    {"type": "mention", "name": "...", "path": "app://..."}
  ],
  "cwd": "...",
  "approvalPolicy": "on-request",
  "sandboxPolicy": {},
  "model": "...",
  "effort": "..."
}
```

**ACP `session/prompt` input:**
```json
{
  "sessionId": "...",
  "parts": [
    {"type": "text", "text": "..."},
    {"type": "image", "mimeType": "image/png", "data": "<base64>"},
    {"type": "resource_link", "uri": "file:///path/to/file", "name": "file.ts"}
  ]
}
```

Transform in `opencode_core.rs`:
- `text` → `text` (direct)
- `image` (URL) → read image, base64-encode, send as `image` part
- `localImage` → read file from path, base64-encode, send as `image` part
- `mention` → `resource_link` with `file://` URI (strip `app://` prefix, resolve to filesystem path)
- `model`/`effort` → call `unstable_setSessionModel` before prompt if model changed

### 2.5 Event Translation Map

| ACP sessionUpdate type | → CodexMonitor event method | Payload transform |
|---|---|---|
| `agent_message_chunk` | `item/agentMessage/delta` | Wrap text in `{params: {threadId, turnId, itemId, type: "agentMessage", delta: {text}}}` |
| `agent_thought_chunk` | `item/reasoning/textDelta` | Wrap text in `{params: {threadId, turnId, itemId, type: "reasoning", delta: {text}}}` |
| `tool_call` (status: pending) | `item/started` | Synthesize item with type=`commandExecution` or `fileChange` based on `kind` |
| `tool_call_update` (status: in_progress) | `item/commandExecution/outputDelta` | Stream tool output |
| `tool_call_update` (status: completed) | `item/completed` | Include output content |
| `tool_call_update` (status: failed) | `item/completed` | Include error |
| `plan` | `turn/plan/updated` | Map entries to plan items |
| `usage_update` | `thread/tokenUsage/updated` | Map `{used, size, cost}` → `{inputTokens, outputTokens, totalTokens}` |
| `user_message_chunk` | (no-op for MVP) | Frontend doesn't need echo of user messages |
| `available_commands_update` | (no-op for MVP) | |
| *requestPermission* (JSON-RPC request, not notification) | Approval event via `requestApproval`-like shape | See §2.5.1 |
| *(synthetic)* on `session/prompt` call | `turn/started` | Emit before calling prompt |
| *(synthetic)* on prompt return | `turn/completed` | Emit after prompt returns |

#### 2.5.1 Permission Flow Detail

ACP's permission flow is unique: it comes as a **JSON-RPC request** (has `id` field) on the stdio channel, not a notification. This means the `WorkspaceSession` stdout reader will see a message with both `id` AND `method` = something like `requestPermission`.

Current CodexMonitor handling:
1. Stdout reader sees `{id: N, method: "requestApproval", params: {...}}` from Codex
2. Emits as `AppServerEvent` to frontend
3. Frontend's `useThreadApprovalEvents` checks allowlist, auto-approves or dispatches `addApproval`
4. User clicks approve → `respondToServerRequest(workspaceId, requestId, "accept")` → Rust calls `session.send_response(id, result)`

ACP equivalent:
1. Stdout reader sees `{jsonrpc: "2.0", id: N, method: "requestPermission", params: {sessionId, toolCall: {toolCallId, status, title, rawInput, kind, locations}, options: [{optionId, kind, name}]}}`
2. Transform to CodexMonitor approval shape: `{workspace_id, request_id: N, params: {type: toolCall.kind, command: [rawInput details], ...}}`
3. Frontend flow unchanged
4. User clicks approve → `respondToServerRequest` → Rust sends `{id: N, result: {outcome: {outcome: "selected", optionId: "once|always|reject"}}}`

**Key mapping**: CodexMonitor's `"accept"` → ACP's `{outcome: "selected", optionId: "once"}`. CodexMonitor's `"deny"` → ACP's `{outcome: "selected", optionId: "reject"}`.

### 2.6 Invariants

- **INV1**: One `opencode acp` process per workspace. Never share processes.
- **INV2**: Port assignment is monotonically increasing per app lifetime. No reuse after process death (avoids stale-port races).
- **INV3**: All ACP method calls go through `WorkspaceSession.send_request` — never bypass to talk to ACP's internal HTTP server directly.
- **INV4**: Frontend thread reducer receives events in the same shape as today. All translation happens in Rust.

---

## 3. Step-by-Step Implementation Plan

### Phase 0: Wire Capture Validation (MUST DO FIRST)

**Rationale**: Streaming behavior is unverified. ACP README says "not yet implemented" but source code suggests otherwise.

1. Install OpenCode, run `opencode acp --port 14096 --cwd /tmp/test-project`
2. Send `initialize` → capture response shape
3. Send `session/new` → capture sessionId
4. Send `session/prompt` → capture: do streaming events arrive BEFORE the prompt response?
5. Test multi-instance: spawn second on different port
6. Test permission flow: prompt that triggers tool use

**Gate**: If streaming doesn't work, the event translation approach needs redesign.

### Phase 1: Fork & Rename (~2 hours)

**Files to change:**
- `package.json` — name, description, repository
- `src-tauri/tauri.conf.json` — `productName`, `identifier`, window title
- `src-tauri/Cargo.toml` — package name
- Global find/replace in user-facing strings: "Codex Monitor" → "OpenCode Monitor"
- **Do NOT rename internal Rust module paths yet** — minimizes merge conflicts

### Phase 2: Port Allocation & Process Spawn (~4 hours)

**File: `src-tauri/src/state.rs`**
- Add field: `next_acp_port: AtomicU16` initialized to `14096`

**File: `src-tauri/src/backend/app_server.rs`**
- `spawn_workspace_session`: spawn `opencode acp --port <N> --cwd <path>` instead of `codex app-server`
- `build_codex_command_with_bin` → `build_opencode_command`: default binary `"opencode"`
- `check_codex_installation` → `check_opencode_installation`: run `opencode --version`
- `build_initialize_params`: send `{"protocolVersion": 1}` instead of Codex clientInfo
- Remove `initialized` notification after init

### Phase 3: Event Translation Layer (~8 hours, core work)

**New file: `src-tauri/src/backend/event_translator.rs`**

```rust
pub(crate) fn translate_acp_to_codex_event(
    workspace_id: &str,
    acp_message: &Value,
    session_state: &SessionTranslationState,
) -> Option<AppServerEvent>;

pub(crate) struct SessionTranslationState {
    // Track current turn_id (synthesized)
    // Track item_id counter (synthesized)
    // Map tool_call_id → item_id
}
```

**File: `src-tauri/src/backend/app_server.rs`** — Modify stdout reader loop:
- Intercept JSON-RPC requests (id + method) for permission handling
- Intercept notifications (method, no id) for sessionUpdate translation
- Pass responses (id + result/error) through to pending oneshot channels

### Phase 4: Core Protocol Methods (~6 hours)

**File: `src-tauri/src/shared/codex_core.rs`**

Rewrite methods:
- `start_thread_core` → `session/new`
- `resume_thread_core` → `session/load`
- `list_threads_core` → `unstable/listSessions` (filter by cwd)
- `send_user_message_core` → `session/prompt` (with synthetic turn events)
- `turn_interrupt_core` → `cancel` notification
- Stub: `fork_thread`, `compact_thread`, `set_thread_name`, `archive_thread`, `model_list`, `skills_list`, `apps_list`, `account_*`, `codex_login_*`, `collaboration_mode_list`, `start_review`

### Phase 5: Permission/Approval Flow (~4 hours)

- Store pending permission request IDs in `WorkspaceSession`
- Transform ACP `requestPermission` → CodexMonitor approval event shape
- Transform frontend `respondToServerRequest` → ACP outcome response
- Map `"accept"` → `{outcome: "selected", optionId: "once"}`, `"deny"` → `{outcome: "selected", optionId: "reject"}`

### Phase 6: Config & Settings (~3 hours)

- Update `codex/home.rs`: `CODEX_HOME`/`~/.codex` → OpenCode config paths
- Update `codex/config.rs`: read from OpenCode config format
- Update Settings UI: rename labels, remove Codex-specific sections

### Phase 7: Frontend Cleanup (~4 hours)

- Hide/remove: skills autocomplete, collaboration mode picker, access mode selector, rate limits meter, account login
- Update strings: "Codex" → "OpenCode" in all user-facing UI
- Leave thread reducer and event handling untouched (translation happens in Rust)

### Phase 8: Error Handling & Crash Recovery (~3 hours)

- Flush pending oneshot channels on process exit
- Emit `codex/disconnected` event on process death
- On reconnect: spawn new `opencode acp` with new port
- Previous sessions loadable via `session/load`

---

## 4. Testing & Validation Plan

### Pre-Implementation (Phase 0)
- [ ] Wire capture of `opencode acp` stdio
- [ ] Multi-instance test
- [ ] Pin OpenCode version

### Unit Tests (Rust)
- [ ] `event_translator.rs`: each ACP event → expected CodexMonitor event shape
- [ ] `opencode_core.rs`: input format transformation
- [ ] Port allocation: monotonic increment, no reuse

### Integration Tests
- [ ] Spawn + init + session/new + prompt → events arrive
- [ ] Permission flow end-to-end
- [ ] Session resume after process restart
- [ ] Interrupt running agent

### Manual Verification
- [ ] Create workspace → connects
- [ ] Start conversation → streaming text
- [ ] Tool execution → approval dialog → approve → tool output
- [ ] Interrupt → stops
- [ ] List sessions after restart
- [ ] Git panel works
- [ ] File tree works
- [ ] Two workspaces simultaneously

### Commands
```bash
npm run lint
npm run test
npm run typecheck
cd src-tauri && cargo check
cd src-tauri && cargo test
npm run tauri:dev
```

---

## 5. Rollout / Backout

- **Rollout**: Separate fork/app. No staged rollout needed.
- **Backout**: Trivially reversible — original CodexMonitor unmodified.
- **Alternative**: If ACP proves too immature, pivot to OpenCode's HTTP API (`opencode serve`).

---

## 6. Senior-Consult Findings

### Accepted
- [CRIT-1] Multi-instance port conflict → port allocation via AtomicU16 + `--port`
- [CRIT-2] Permission flow deadlock → full permission flow design (§2.5.1)
- [CRIT-3] Unverified streaming → mandatory Phase 0 wire capture gate
- [HIGH-1] Session lifecycle mismatch → synthetic turn events, workspace-scoped filtering
- [HIGH-2] Event mapping underspecified → complete event translation table (§2.5)
- [HIGH-3] Tool call handling → tool_call → item lifecycle mapping
- [HIGH-4] Crash recovery → error handling phase (Phase 8)

### Rejected
- [MED-2] model/list hardcoding — ACP session/new returns models; cache that
- [MED-4] Fork semantic gap — full-session fork acceptable for MVP

### Remaining Unknowns
1. ~~Does `unstable_listSessions` filter by `cwd`?~~ → **RESOLVED**: `session/list` with `cwd` param works and filters correctly
2. Does OpenCode support per-instance config isolation? → Likely yes via `--cwd`, but untested for config writes
3. Exact OpenCode config file path → `~/.config/opencode/` (standard XDG)
4. Will `unstable_` methods break in next release? → **Partially resolved**: `session/list` is NOT unstable (stable method name). Only `session/resume` and `session/fork` may use unstable names

---

## Appendix A: Wire Capture Results

**Tested**: OpenCode v1.1.64 on macOS, 2026-02-13

### A.1 Initialize (Test 1)

Request:
```json
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":1}}
```

Response:
```json
{
  "protocolVersion": 1,
  "agentCapabilities": {
    "loadSession": true,
    "mcpCapabilities": {"http": true, "sse": true},
    "promptCapabilities": {"embeddedContext": true, "image": true},
    "sessionCapabilities": {"fork": {}, "list": {}, "resume": {}}
  },
  "authMethods": [{
    "description": "Run `opencode auth login` in the terminal",
    "name": "Login with opencode",
    "id": "opencode-login"
  }],
  "agentInfo": {"name": "OpenCode", "version": "1.1.64"}
}
```

**Findings**: Clean init, no `initialized` notification required. `sessionCapabilities` confirms fork/list/resume support.

### A.2 Session Creation (Test 2)

Request:
```json
{"jsonrpc":"2.0","id":2,"method":"session/new","params":{"cwd":"/tmp/acp-test-project","mcpServers":[]}}
```

Response (key fields):
```json
{
  "sessionId": "ses_3abbb012dffeLISHbXsMfU0G1U",
  "models": {
    "currentModelId": "opencode/big-pickle",
    "availableModels": [/* 614 models */]
  },
  "modes": {
    "availableModes": [
      {"id": "build", "name": "build", "description": "The default agent..."},
      {"id": "plan", "name": "plan", "description": "Plan mode. Disallows all edit tools."},
      {"id": "spec", "name": "spec", "description": "Spec / research / planning-only agent..."}
    ],
    "currentModeId": "build"
  },
  "_meta": {
    "opencode": {
      "modelId": "opencode/big-pickle",
      "variant": null,
      "availableVariants": ["low", "medium", "high"]
    }
  }
}
```

Followed by notification:
```json
{"method": "session/update", "params": {"sessionId": "...", "update": {"sessionUpdate": "available_commands_update", "availableCommands": [/* commands list */]}}}
```

**Findings**:
- Session creation takes ~11-17 seconds (provider loading)
- Returns full model list (614 models!) and modes — no need for separate `model/list`
- `available_commands_update` arrives as automatic notification after session creation
- Modes map to OpenCode's agent types (build/plan/spec)

### A.3 Prompt + Streaming (Test 3a — text only)

**CRITICAL FINDING: Streaming WORKS.**

Event sequence during `session/prompt`:
```
t+0.0s  SENT: session/prompt
t+5.0s  RECV: session/update → agent_thought_chunk  ("The user")
t+5.4s  RECV: session/update → agent_thought_chunk  (" is asking me...")
t+5.4s  RECV: session/update → agent_message_chunk  ("Hello from ACP test.")
t+5.4s  RECV: session/update → agent_message_chunk  (" else.")
t+5.7s  RECV: session/update → usage_update
t+5.7s  RECV: response id=3 (end_turn)
```

Streaming events arrive BEFORE the final response. The README caveat ("not yet implemented") is outdated.

### A.4 Tool Calls (Test 3b — file read/write)

Complete tool lifecycle captured:

```
RECV: tool_call       {toolCallId, title:"read",  kind:"read", status:"pending"}
RECV: tool_call_update {toolCallId, status:"in_progress", locations:[{path:"..."}], rawInput:{filePath:"..."}}
RECV: tool_call_update {toolCallId, status:"failed", content:[{type:"content", content:{type:"text", text:"Error:..."}}]}

RECV: tool_call       {toolCallId, title:"write", kind:"edit", status:"pending"}
RECV: tool_call_update {toolCallId, status:"in_progress", locations:[...], rawInput:{filePath, content}}
RECV: tool_call_update {toolCallId, status:"completed", content:[{type:"content",...}, {type:"diff", path, oldText, newText}]}
```

**Findings**:
- Full tool lifecycle: pending → in_progress → completed/failed
- `rawInput` contains tool parameters (filePath, content, etc.)
- `rawOutput` includes metadata on completed tools
- Diff content provided for edit operations (`{type:"diff", path, oldText, newText}`)
- **No permission requests observed** — ACP auto-approves tools currently

### A.5 Permission Flow

**IMPORTANT**: No `requestPermission` events were captured during testing. The ACP client code (`client.ts`) states: "Permission requests (auto-approves for now)". This means:

- For MVP: permission/approval UI will NOT be exercised unless OpenCode changes this behavior
- The existing CodexMonitor approval UI can remain but will be dormant
- When OpenCode adds real permission requests, the translation layer will be ready

### A.6 Session Listing (Test 5)

Correct method name: **`session/list`** (NOT `unstable_listSessions`)

Request:
```json
{"jsonrpc":"2.0","id":2,"method":"session/list","params":{"cwd":"/tmp/acp-test-project"}}
```

Response:
```json
{
  "sessions": [
    {"sessionId": "ses_...", "cwd": "/tmp/acp-test-project", "title": "ACP Session ...", "updatedAt": "2026-02-13T00:18:27.422Z"},
    ...
  ]
}
```

**Findings**: Filters by `cwd` correctly. Returns `sessionId`, `cwd`, `title`, `updatedAt`.

### A.7 Session Load / Resume (Test 6)

Correct method name: **`session/load`**

```json
{"jsonrpc":"2.0","id":2,"method":"session/load","params":{"sessionId":"ses_...","cwd":"/tmp/acp-test-project","mcpServers":[]}}
```

**Replays full session history** as `sessionUpdate` events:
```
Line 2: available_commands_update
Line 3: user_message_chunk     text="Read the file..."
Line 4: agent_thought_chunk    text="Let me first try..."
Line 5: tool_call_update       status=failed title=read
Line 6: agent_message_chunk    text="File doesn't exist..."
Line 7: tool_call_update       status=completed title=...
Line 8: agent_message_chunk    text="The file now contains..."
Line 9: usage_update
Line 10: response id=2 OK
```

**Findings**: Full history replay works. Frontend will receive all past messages as events during session load.

### A.8 Multi-Instance (Test 4)

Two `opencode acp` processes on ports 14200 and 14201 — both initialized successfully with no conflicts. **Multi-workspace is confirmed viable.**

### A.9 Corrections to Spec

Based on wire capture, these spec items need updating:

| Spec Section | Was | Now |
|---|---|---|
| §2.3 `thread/list` method | `unstable_listSessions` | **`session/list`** (stable) |
| §2.3 `thread/resume` method | `session/load` | `session/load` (confirmed) |
| §2.5 Permission flow | Expected `requestPermission` events | **Auto-approved for now** — permission UI will be dormant |
| §2.2 D2 synthetic turns | Assumed needed | **Confirmed needed** — ACP has no turn/started or turn/completed events |
| §3 Phase 2 session creation | Assumed fast | **Takes 11-17s** — frontend needs loading state |
| §2.3 `model/list` | Separate call | **Not needed** — models returned in `session/new` response (614 models) |
| §2.5 Event map | Included `requestPermission` | **Dormant for now** — keep code ready but don't expect it |

### A.10 Usage Update Shape

```json
{
  "sessionUpdate": "usage_update",
  "used": 36085,
  "size": 200000,
  "cost": {"amount": 0, "currency": "USD"}
}
```

### A.11 Prompt Response Shape

```json
{
  "stopReason": "end_turn",
  "usage": {
    "totalTokens": 36150,
    "inputTokens": 34030,
    "outputTokens": 37,
    "thoughtTokens": 28,
    "cachedReadTokens": 2055
  },
  "_meta": {}
}
```

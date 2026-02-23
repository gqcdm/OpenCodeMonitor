---
shaping: true
---

# REST API Migration — Shaping

Replace `opencode acp` (JSON-RPC over stdio) with `opencode serve` (HTTP REST + SSE) as the agent backend.

---

## Frame

### Source

> Is there a way for us to use opencode serve rest api instead of ACP while keeping compatibility with CodexMonitor so we can pull/integrate changes from that repo?

> Images aren't working with ACP. User cannot attach images for model to read. Frequent ACP bugs/issues that aren't addressed quickly by opencode.

### Problem

OpenCodeMonitor uses `opencode acp` as its agent backend. ACP is a thin wrapper around the REST API that `opencode serve` exposes, adding indirection without benefit. ACP has bugs (images broken, others suspected) that aren't addressed quickly upstream. Several CodexMonitor features are stubbed because ACP doesn't expose the underlying capabilities. The fork needs to stay mergeable with upstream CodexMonitor (weekly/monthly pulls).

### Outcome

A backend transport that gives us the richest OpenCode API surface while keeping the upstream merge path clean. Images work. Not blocked by ACP-specific bugs.

---

## Requirements (R)

| ID | Requirement | Status |
|----|-------------|--------|
| R0 | Use OpenCode as the agent backend | Core goal |
| R1 | Image attachments work -- users can attach images for model to read | Must-have |
| R2 | Maintain weekly/monthly merge path with upstream CodexMonitor (frontend changes merge cleanly) | Must-have |
| R3 | All OpenCode protocol translation stays in Rust -- frontend receives only CodexMonitor-shaped events | Must-have |
| R4 | Not blocked by ACP-specific bugs that aren't addressed quickly upstream | Must-have |
| R5 | Real-time streaming of agent output (text chunks, tool calls, reasoning) | Must-have |
| R6 | Session lifecycle works: create, list, resume, interrupt | Must-have |
| R7 | Currently stubbed features become implementable (fork, archive, rename, MCP) | Nice-to-have |
| R8 | Backend protocol layer is maintainable -- fewer workarounds, less probing logic | Nice-to-have |

---

## Shapes

### CURRENT: ACP over stdio JSON-RPC

| Part | Mechanism |
|------|-----------|
| **CUR1** | Spawn `opencode acp --port <N> --cwd <path>`, handshake via JSON-RPC `initialize` over stdin |
| **CUR2** | Stdout line-reader loop parses nd-JSON, routes responses and notifications |
| **CUR3** | JSON-RPC `session/*` methods over stdin for all operations |
| **CUR4** | `session/update` notifications translated to CodexMonitor event shapes |
| **CUR5** | Images written to temp files, sent as `resource_link` (workaround for ACP `data:` bug) |
| **CUR6** | `session/load` triggers automatic replay of history as `session/update` events |
| **CUR7** | Model-set probing: try 3 methods (`unstable_setSessionModelId`, `unstable_setSessionModel`, `session/setModel`), cache first success |
| **CUR8** | Permission via inbound JSON-RPC request + response over stdin |

### A: Single shared opencode serve instance

Selected shape.

One `opencode serve` process serves all workspaces. The server resolves project context per-request via `?directory=<path>` query parameter (or `x-opencode-directory` header), lazily initializing an Instance per directory on first use. SSE events on `GET /global/event` are tagged with `directory` for routing to the correct workspace.

| Part | Mechanism |
|------|-----------|
| **A1** | Spawn one `opencode serve --port <N>` at app startup, handshake via `GET /global/health` poll |
| **A2** | Single SSE client on `GET /global/event` -- events tagged with `directory`, routed to correct workspace |
| **A3** | HTTP REST calls with `?directory=<workspace_path>` on each request to scope to the correct project |
| **A4** | SSE `message.part.updated` / `session.status` events translated to CodexMonitor event shapes |
| **A5** | Images sent as `FilePart` (`{ type: "file", mime: "image/png", url: "data:...;base64,..." }`) -- same format the OpenCode web UI uses |
| **A6** | Session load: fetch `GET /session/:id/message`, synthesize replay events from structured history |
| **A7** | Model specified per-message in POST body -- no probing dance |
| **A8** | Permissions via SSE `permission.updated` + `POST /permission/:id/reply` |

---

## Spike A5: Image Attachments via REST API

### Context

Images don't work via ACP due to a bug in `createUserMessage`'s `data:` branch. The current codebase works around this by writing images to temp files and sending `resource_link` parts, but images still fail. We needed to know if the REST API handles images correctly.

### Questions and Answers

| # | Question | Answer |
|---|----------|--------|
| **A5-Q1** | Does REST accept `FilePart` with images? | Yes. `POST /session/:id/message` validates against `PromptInput` which includes `FilePart` with type "file", mime, and url fields. |
| **A5-Q2** | Does the web UI send images? | Yes. The web UI sends images as `FilePart` with `data:image/png;base64,...` URLs via `POST /session/:id/prompt_async`. |
| **A5-Q3** | Is there ACP-specific vs REST-specific processing? | No. ACP internally converts `image`/`resource_link` parts to `FilePart` and calls the same `SessionPrompt.prompt()`. The bug is in ACP's translation layer. |
| **A5-Q4** | What's the exact request format? | `{ type: "file", mime: "image/png", url: "data:image/png;base64,...", filename: "screenshot.png" }` -- supports `data:` and `file://` URL schemes. |

### Finding

The OpenCode web UI uses the REST API to send images and it works. ACP wraps the same code but has translation bugs in its `image`/`resource_link` to `FilePart` conversion. Shape A bypasses this buggy layer entirely.

### Evidence

From `sst/opencode` @ `0042a07`:

**Web UI sends images as FilePart** (`packages/app/src/components/prompt-input/build-request-parts.ts`):
```typescript
const images = input.images.map((attachment) => ({
  id: Identifier.ascending("part"),
  type: "file",
  mime: attachment.mime,
  url: attachment.dataUrl,  // "data:image/png;base64,<base64data>"
  filename: attachment.filename,
}))
```

**Server processes FilePart images identically for REST and ACP** (`packages/opencode/src/session/prompt.ts`):
```typescript
// createUserMessage() — for data: URLs that are NOT text/plain:
case "data:":
  if (part.mime === "text/plain") { /* convert to text parts */ }
  break  // falls through — images kept as-is
```

**ACP just converts to FilePart and calls REST** (`packages/opencode/src/acp/agent.ts`):
```typescript
case "image":
  parts.push({
    type: "file",
    url: `data:${part.mimeType};base64,${part.data}`,
    mime: part.mimeType,
  })
```

---

## Spike: Multi-Directory Support

### Context

Shape A originally assumed one `opencode serve` per workspace. We investigated whether a single instance can serve all workspaces simultaneously.

### Finding

Yes. `opencode serve` resolves project context **per-request**, not at startup.

- Every request (except `/log`) goes through middleware that reads `?directory=` query param or `x-opencode-directory` header (falls back to `process.cwd()`)
- The server lazily creates and caches an `Instance` per directory on first use (LSP, file watcher, VCS all scoped per-instance)
- `GET /global/event` SSE stream multiplexes events from all instances, each tagged with `directory`
- `GET /project` lists all projects the server has ever seen across all directories
- No explicit registration needed -- just send any request with the directory and the instance is created

### Evidence

From `sst/opencode` @ `0042a07`:

**Per-request middleware** (`server/server.ts`):
```typescript
const raw = c.req.query("directory") || c.req.header("x-opencode-directory") || process.cwd()
return Instance.provide({ directory, init: InstanceBootstrap, async fn() { return next() } })
```

**Lazy instance cache** (`project/instance.ts`):
```typescript
const cache = new Map<string, Promise<Context>>()
// First request for a directory creates the instance; subsequent requests reuse it
```

**Session inherits directory from context** (`session/session.ts`):
```typescript
directory: Instance.directory  // set by the per-request middleware, not a body parameter
```

---

## Fit Check

| Req | Requirement | Status | CURRENT | A |
|-----|-------------|--------|:-------:|:-:|
| R0 | Use OpenCode as the agent backend | Core goal | Pass | Pass |
| R1 | Image attachments work -- users can attach images for model to read | Must-have | Fail | Pass |
| R2 | Maintain weekly/monthly merge path with upstream CodexMonitor | Must-have | Pass | Pass |
| R3 | All protocol translation stays in Rust -- frontend receives only CodexMonitor-shaped events | Must-have | Pass | Pass |
| R4 | Not blocked by ACP-specific bugs that aren't addressed quickly upstream | Must-have | Fail | Pass |
| R5 | Real-time streaming of agent output (text chunks, tool calls, reasoning) | Must-have | Pass | Pass |
| R6 | Session lifecycle works: create, list, resume, interrupt | Must-have | Pass | Pass |
| R7 | Currently stubbed features become implementable (fork, archive, rename, MCP) | Nice-to-have | Fail | Pass |
| R8 | Backend protocol layer is maintainable -- fewer workarounds, less probing logic | Nice-to-have | Fail | Pass |

**Notes:**
- CURRENT fails R1: ACP has a `data:` URI bug in `createUserMessage`. The `resource_link` workaround is in place but images still don't work.
- CURRENT fails R4: ACP bugs block features and upstream response time is slow.
- CURRENT fails R7: ACP doesn't expose fork, archive, rename, MCP endpoints.
- CURRENT fails R8: Model-set probing (3 methods), `resource_link` workaround, pending request map complexity.

**Decision:** Shape A selected. Passes all requirements including 2 must-haves that CURRENT fails.

---

## Breadboard

### Places

| # | Place | Description |
|---|-------|-------------|
| P1 | Frontend (React) | Existing UI -- unchanged. Receives CodexMonitor-shaped events via Tauri. |
| P2 | Rust Backend | `app_server.rs`, `codex_core.rs`, `event_translator.rs` -- the changed layer. |
| P3 | OpenCode Serve | External `opencode serve` process -- HTTP REST + SSE. |

### UI Affordances (P1 -- all existing, unchanged)

| # | Place | Affordance | Control | Wires Out | Returns To |
|---|-------|------------|---------|-----------|------------|
| U1 | P1 | Message input + send | submit | -> N9 | -- |
| U2 | P1 | Image attachment | attach | -> N17 | -- |
| U3 | P1 | Streaming text display | render | -- | -- |
| U4 | P1 | Tool call output | render | -- | -- |
| U5 | P1 | Thread list panel | render | -- | -- |
| U6 | P1 | Thread item (resume) | click | -> N8 | -- |
| U7 | P1 | Interrupt button | click | -> N10 | -- |
| U8 | P1 | Model selector | select | -> N9 | -- |
| U9 | P1 | Permission dialog | approve/deny | -> N19 | -- |
| U10 | P1 | Connection indicator | render | -- | -- |
| U11 | P1 | Token usage display | render | -- | -- |

### Code Affordances (P2 -- the changed layer)

**Server lifecycle (A1):**

| # | Place | Affordance | Control | Wires Out | Returns To |
|---|-------|------------|---------|-----------|------------|
| N1 | P2 | `spawn_server()` | call | -> P3 (spawn once at app startup), -> N2 | -- |
| N2 | P2 | `health_check_poll()` | call | -> P3 `GET /global/health` | -> N3 |
| N3 | P2 | emit `codex/connected` per workspace | event | -- | -> U10 |

**SSE reader (A2):**

| # | Place | Affordance | Control | Wires Out | Returns To |
|---|-------|------------|---------|-----------|------------|
| N4 | P2 | `sse_reader_loop()` | subscribe | -> P3 `GET /global/event` (single connection) | -> N5 |
| N5 | P2 | `route_sse_event()` | call | match `directory` field -> correct workspace -> N11, N12, N13, N14, N15, N18 | -- |

**Session operations (A3):**

| # | Place | Affordance | Control | Wires Out | Returns To |
|---|-------|------------|---------|-----------|------------|
| N6 | P2 | `create_session()` | call | -> P3 `POST /session?directory=<path>` | -> S4 |
| N7 | P2 | `list_sessions()` | call | -> P3 `GET /session?directory=<path>` | -> U5 |
| N8 | P2 | `load_session()` | call | -> P3 `GET /session/:id/message?directory=<path>`, -> N16 | -> U3, U4, U5 |
| N9 | P2 | `send_prompt()` | call | -> N17 (images), -> P3 `POST /session/:id/prompt_async?directory=<path>` | -> U3 (via SSE) |
| N10 | P2 | `abort_session()` | call | -> P3 `POST /session/:id/abort?directory=<path>` | -- |

**Event translation (A4):**

| # | Place | Affordance | Control | Wires Out | Returns To |
|---|-------|------------|---------|-----------|------------|
| N11 | P2 | `translate_text_part()` | call | emit `item/agentMessage/delta` | -> U3 |
| N12 | P2 | `translate_reasoning_part()` | call | emit `item/reasoning/textDelta` | -> U3 |
| N13 | P2 | `translate_tool_part()` | call | emit `item/started` or `item/completed` | -> U4 |
| N14 | P2 | `translate_message_updated()` | call | emit `thread/tokenUsage/updated` | -> U11 |
| N15 | P2 | `translate_session_status()` | call | emit `turn/started` or `turn/completed` | -> U3 |
| N16 | P2 | `synthesize_replay_events()` | call | emit sequence of CodexMonitor events | -> U3, U4, U5 |

**Images (A5):**

| # | Place | Affordance | Control | Wires Out | Returns To |
|---|-------|------------|---------|-----------|------------|
| N17 | P2 | `build_file_part()` | call | -- | -> N9 |

**Permissions (A8):**

| # | Place | Affordance | Control | Wires Out | Returns To |
|---|-------|------------|---------|-----------|------------|
| N18 | P2 | `translate_permission()` | call | emit `codex/requestApproval` | -> U9 |
| N19 | P2 | `reply_to_permission()` | call | -> P3 `POST /permission/:id/reply` | -- |

### Data Stores (P2)

| # | Place | Store | Scope | Description |
|---|-------|-------|-------|-------------|
| S1 | P2 | `http_client` | App-wide | `reqwest::Client` -- single shared HTTP client |
| S2 | P2 | `base_url` | App-wide | `http://127.0.0.1:<port>` -- one server |
| S3 | P2 | `server_process` | App-wide | `Child` -- the single `opencode serve` process |
| S4 | P2 | `models_cache` | Per-workspace | Cached provider/model data from `GET /provider?directory=<path>` |
| S5 | P2 | `prewarmed_session_id` | Per-workspace | Pre-warmed session ID from `POST /session?directory=<path>` |
| S6 | P2 | `translation_state` | Per-workspace | Event translation state (turn counter, accumulated text, etc.) |

---

## Protocol Mapping Reference

### ACP Method -> REST Endpoint

All REST calls include `?directory=<workspace_path>` to scope to the correct project instance.

| ACP Method | REST Equivalent |
|---|---|
| `initialize` | `GET /global/health` (no directory needed) |
| `session/new` | `POST /session?directory=<path>` |
| `session/load` | `GET /session/:id/message?directory=<path>` |
| `session/list` | `GET /session?directory=<path>` |
| `session/prompt` | `POST /session/:id/prompt_async?directory=<path>` (fire-and-forget, events via SSE) |
| `cancel` | `POST /session/:id/abort?directory=<path>` |
| `account/read` | `GET /provider?directory=<path>` |
| `unstable_setSessionModel` / `session/setModel` | Model specified in message POST body -- no separate call |
| `requestPermission` | `POST /permission/:id/reply?directory=<path>` |

### SSE Event -> CodexMonitor Event

| SSE Event | CodexMonitor Event |
|---|---|
| `message.part.updated` (type: "text", delta) | `item/agentMessage/delta` |
| `message.part.updated` (type: "reasoning", delta) | `item/reasoning/textDelta` |
| `message.part.updated` (type: "tool", pending) | `item/started` |
| `message.part.updated` (type: "tool", running) | `item/started` + `item/commandExecution/outputDelta` or `item/fileChange/outputDelta` |
| `message.part.updated` (type: "tool", completed) | `item/completed` |
| `message.part.updated` (type: "tool", error) | `item/completed` (status: "failed") |
| `message.updated` (assistant, with tokens) | `thread/tokenUsage/updated` |
| `session.status` -> busy | Synthetic `turn/started` |
| `session.status` -> idle | Synthetic `turn/completed` |
| `permission.updated` | `codex/requestApproval` |

### REST Image Request Format

```json
{
  "model": { "providerID": "anthropic", "modelID": "claude-sonnet-4-20250514" },
  "parts": [
    { "type": "text", "text": "What is in this image?" },
    {
      "type": "file",
      "mime": "image/png",
      "url": "data:image/png;base64,iVBORw0KGgo...",
      "filename": "screenshot.png"
    }
  ]
}
```

Supported `url` schemes: `data:<mime>;base64,<data>` (inline) or `file:///absolute/path` (local file).

---

## Slices

| # | Slice | Parts | Demo |
|---|-------|-------|------|
| V1 | Connection + basic conversation | A1, A2, A3 (partial), A4 (text only) | Open workspace -> connected -> send message -> streaming response |
| V2 | Full event translation | A4 (tools, reasoning, tokens) | Agent runs tools -> see output, reasoning, tokens |
| V3 | Session management | A3 (list, load, abort), A6 | List threads -> resume -> interrupt |
| V4 | Images + model selection | A5, A7 | Attach image -> model responds. Switch model. |
| V5 | Permissions | A8 | Permission dialog -> approve -> agent continues |

### Affordance Assignments

| Affordance | V1 | V2 | V3 | V4 | V5 |
|---|:---:|:---:|:---:|:---:|:---:|
| N1 spawn | x | | | | |
| N2 health check | x | | | | |
| N3 emit connected | x | | | | |
| N4 SSE reader | x | | | | |
| N5 route events | x | | | | |
| N6 create session | x | | | | |
| N7 list sessions | | | x | | |
| N8 load session | | | x | | |
| N9 send prompt | x | | | | |
| N10 abort | | | x | | |
| N11 text translation | x | | | | |
| N12 reasoning translation | | x | | | |
| N13 tool translation | | x | | | |
| N14 token usage | | x | | | |
| N15 session status -> turns | x | | | | |
| N16 synthesize replay | | | x | | |
| N17 build file part | | | x | | |
| N18 translate permission | | | | | x |
| N19 reply to permission | | | | | x |

### V1: Connection + Basic Conversation

**Adds:** N1, N2, N3, N4, N5, N6, N9, N11, N15, S1, S2, S3, S5, S6

**Changes:**
- Spawn one `opencode serve` process at app startup (not per-workspace)
- Single SSE connection to `GET /global/event` with directory-based event routing
- `WorkspaceSession` struct simplified: remove stdio/process fields; just holds workspace `directory` path and per-workspace state (translation state, session IDs)
- App-wide `ServerState` holds `http_client`, `base_url`, `server_process`
- All REST calls include `?directory=<workspace_path>` to scope to correct project
- SSE events matched to workspace by `directory` field
- `create_session()` via `POST /session?directory=<path>`
- `send_prompt()` via `POST /session/:id/prompt_async?directory=<path>` (text only)
- Basic text streaming translation + synthetic turn events
- Add `reqwest-eventsource` to Cargo.toml

**Demo:** Open a workspace -> indicator shows "connected" -> type a message -> see streaming text response with turn start/end indicators.

### V2: Full Event Translation

**Adds:** N12, N13, N14

**Changes:**
- `translate_reasoning_part()` -- reasoning chunks from SSE
- `translate_tool_part()` -- tool pending/running/completed states with command vs fileChange classification
- `translate_message_updated()` -- token usage from assistant message metadata

**Demo:** Send a prompt that triggers tools -> see tool execution output, reasoning traces, token usage counter updating.

### V3: Session Management

**Adds:** N7, N8, N10, N16

**Changes:**
- `list_sessions()` via `GET /session?directory=<cwd>`
- `load_session()` via `GET /session/:id/message` + `synthesize_replay_events()`
- `abort_session()` via `POST /session/:id/abort`
- Replay synthesis: iterate message history, emit CodexMonitor events in order

**Demo:** See thread list populated -> click a previous thread -> see full conversation history appear -> send new message -> click interrupt -> agent stops.

### V4: Images + Model Selection

**Adds:** N17, model-in-POST-body logic

**Changes:**
- `build_file_part()` -- convert data URIs / file paths to `{ type: "file", mime, url }` format
- Model specified in `POST /session/:id/prompt_async` body instead of separate set-model call
- Remove entire `ModelSetCapability` probing dance

**Demo:** Attach a screenshot -> send "what's in this image?" -> model describes the image. Switch model -> send another message -> handled by new model.

### V5: Permissions

**Adds:** N18, N19

**Changes:**
- SSE `permission.updated` events translated to `codex/requestApproval`
- User approval routed to `POST /permission/:id/reply` with `{ reply: "allow" }` or `{ reply: "deny" }`

**Demo:** Send prompt needing file write permission -> permission dialog appears -> approve -> agent completes the action.

---

## Merge Compatibility

Files changed by this migration vs. upstream CodexMonitor:

| File | Change | Upstream conflict? |
|------|--------|--------------------|
| `src-tauri/Cargo.toml` | Add `reqwest-eventsource` | Low -- additive only |
| `src-tauri/src/backend/app_server.rs` | Single server spawn, SSE reader, directory-based routing | Yes -- already diverged |
| `src-tauri/src/shared/codex_core.rs` | All protocol methods rewritten | Yes -- already diverged |
| `src-tauri/src/backend/event_translator.rs` | New input format, same output | Yes -- already diverged |
| Frontend (`src/**`) | None | Clean merge |
| Tauri commands (`src-tauri/src/lib.rs`) | Minimal -- signatures unchanged | Clean merge |

The three backend files that conflict are the same three that already conflict today. No new merge surface introduced.

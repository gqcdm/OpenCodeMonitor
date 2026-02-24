# OpenCode Monitor — Project Memory

## Project Identity

**OpenCode Monitor** is a macOS desktop app for monitoring and interacting with OpenCode agents across multiple workspaces. It's a fork of [CodexMonitor](https://github.com/Dimillian/CodexMonitor), adapted to use OpenCode's protocol instead of Codex.

## Tech Stack

- **Frontend**: React 19 + Vite + TypeScript
- **Backend**: Tauri 2 (Rust)
- **Protocol**: REST API (`opencode serve` — HTTP + SSE)

## Architecture Invariants

1. Frontend receives events in **same shape as original CodexMonitor** — all protocol translation happens in Rust
2. Put shared/domain logic in `src-tauri/src/shared/*` first
3. Keep app and daemon as thin adapters around shared cores
4. Protocol translation is isolated to 3 Rust files for clean upstream merges

## Key Files

| Area | Primary Files |
|------|--------------|
| Protocol translation | `event_translator.rs`, `codex_core.rs`, `app_server.rs` |
| Frontend composition | `src/App.tsx`, `src/services/tauri.ts`, `src/services/events.ts` |
| Thread state | `useThreadsReducer.ts`, `threadReducer/*` |
| Shared cores | `src-tauri/src/shared/*` |

## Current Focus

- REST backend polish and parity
- Thread lifecycle and event handling polish
- Token usage tracking and session management

## Active Patterns

- Event-driven architecture with single-listener fanout
- Reducer composition for thread state
- Workspace-scoped sessions
- Import aliases: `@/*`, `@app/*`, `@threads/*`, `@services/*`, `@utils/*`

## Recent Work (from git log)

- Phantom session prevention and recency ordering
- Token usage context window population
- Question tool UI with submit/dismiss actions
- Agent/model selection from REST API
- Incremental SSE streaming

## Quick Stats

- Total memories: 0
- Last updated: 2025-02-24
- Sessions since review: 0
- Session tools available: true
- Last bootstrap: 2025-02-24
- Bootstrapped sessions: []
- Project phase: foundation
- Project phase epoch: 1
- Close state: clean
- Index dirty: false
- Active session id: 2025-02-24-1
- Last closed session id: ""

# App-Server Events Reference (OpenCode ACP)

This document describes the events OpenCode Monitor currently emits to the frontend after ACP-to-CodexMonitor translation.

## Source Of Truth

- ACP process + stdout routing: `src-tauri/src/backend/app_server.rs`
- ACP `session/update` translation: `src-tauri/src/backend/event_translator.rs`
- Frontend method parser: `src/utils/appServerEvents.ts`
- Frontend event router: `src/features/app/hooks/useAppServerEvents.ts`

## Supported Event Methods

These methods are currently recognized by `SUPPORTED_APP_SERVER_METHODS` and routed into thread/app state handlers:

- `app/list/updated`
- `codex/connected`
- `codex/disconnected`
- `codex/backgroundThread`
- `*requestApproval` (suffix match)
- `item/tool/requestUserInput`
- `turn/started` (synthetic)
- `turn/completed` (synthetic)
- `turn/plan/updated`
- `turn/diff/updated`
- `thread/tokenUsage/updated`
- `thread/started`
- `thread/name/updated`
- `item/started`
- `item/completed`
- `item/agentMessage/delta`
- `item/reasoning/textDelta`
- `item/reasoning/summaryTextDelta`
- `item/reasoning/summaryPartAdded`
- `item/plan/delta`
- `item/commandExecution/outputDelta`
- `item/commandExecution/terminalInteraction`
- `item/fileChange/outputDelta`
- `account/rateLimits/updated`
- `account/updated`
- `account/login/completed`
- `error`

## ACP Session Update Mapping

Current `session/update` mappings in `event_translator.rs`:

- `agent_message_chunk` -> `item/agentMessage/delta`
- `agent_thought_chunk` -> `item/reasoning/textDelta`
- `tool_call` -> `item/started`
- `tool_call_update` -> tool deltas + `item/completed`
- `usage_update` -> `thread/tokenUsage/updated`
- `plan` -> `turn/plan/updated`
- dropped intentionally: `user_message_chunk`, `available_commands_update`

Unknown ACP `sessionUpdate` values are ignored (debug builds log them to stderr).

## Background Helper Routing

When translated events include a `params.threadId` that matches a registered background helper callback, the backend sends those translated events to the callback channel instead of the app event sink.

This prevents helper traffic from leaking into the visible thread stream while still allowing streamed helper output collection.

## Synthetic Turn Lifecycle

ACP has no native turn lifecycle notifications. OpenCode Monitor emits:

- `turn/started` before `session/prompt`
- `turn/completed` only after a successful `session/prompt` response
- `error` for failed prompt paths (instead of emitting `turn/completed`)

## Notes

- All ACP translation stays in Rust by design.
- Frontend should continue consuming CodexMonitor-shaped events and avoid protocol-specific logic.

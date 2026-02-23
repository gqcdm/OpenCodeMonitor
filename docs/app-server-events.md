# App-Server Events Reference

Events the Rust backend emits to the frontend after protocol-to-CodexMonitor translation.

## Source Of Truth

- Process management + event routing: `src-tauri/src/backend/app_server.rs`
- Protocol event translation: `src-tauri/src/backend/event_translator.rs`
- Frontend method parser: `src/utils/appServerEvents.ts`
- Frontend event router: `src/features/app/hooks/useAppServerEvents.ts`

## Supported Event Methods

These methods are recognized by `SUPPORTED_APP_SERVER_METHODS` and routed into thread/app state handlers:

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

## Background Helper Routing

When translated events include a `params.threadId` that matches a registered background helper callback, the backend sends those translated events to the callback channel instead of the app event sink.

This prevents helper traffic from leaking into the visible thread stream while still allowing streamed helper output collection.

## Synthetic Turn Lifecycle

OpenCode has no native turn lifecycle notifications. The backend emits:

- `turn/started` before sending a prompt
- `turn/completed` after a successful prompt response
- `error` for failed prompt paths (instead of emitting `turn/completed`)

## Notes

- All protocol translation stays in Rust by design.
- Frontend consumes CodexMonitor-shaped events and avoids protocol-specific logic.
- For the protocol-to-CodexMonitor event mapping, see `docs/shaping/rest-api-migration.md`.

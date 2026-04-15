# Daemon Runtime Guide

## Overview

`src-tauri/src/bin/codex_monitor_daemon` is the remote/daemon runtime: transport, RPC surface, and adapter glue around shared cores.

## Where To Look

| Task | Location | Notes |
| --- | --- | --- |
| Daemon entrypoint and state wiring | `../codex_monitor_daemon.rs` | Runtime bootstrap and state assembly |
| RPC surface and response shaping | `rpc.rs`, `rpc/*` | JSON-RPC methods, notifications, dispatch |
| Transport | `transport.rs` | I/O layer for daemon communication |

## Local Rules

- Keep daemon code as adapter/transport glue around shared cores.
- New daemon methods must stay aligned with app surface, frontend IPC expectations, and shared-core behavior.
- RPC method names and payload shapes are contract surfaces; change them deliberately and holistically.
- Event notifications emitted here must remain aligned with frontend event hub expectations.

## Testing Pattern

- When daemon behavior changes, verify shared-core tests plus any daemon-facing contract tests or integration coverage.
- Favor narrow RPC/transport changes over broad duplication of shared logic.

## Hotspots

- `../codex_monitor_daemon.rs`
- `rpc.rs`
- `rpc/dispatcher.rs`
- `rpc/workspace.rs`

## Anti-Patterns

- Do not copy shared domain logic into RPC handlers.
- Do not add RPC-only behavior that app mode cannot reason about.

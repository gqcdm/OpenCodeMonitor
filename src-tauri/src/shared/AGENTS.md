# Shared Rust Core Guide

## Overview

`src-tauri/src/shared` is the backend source of truth across Tauri app and daemon runtimes.

## Where To Look

| Task | Location | Notes |
| --- | --- | --- |
| OpenCode/session/account/model behavior | `codex_core.rs`, `codex_aux_core.rs`, `account.rs`, `codex_update_core.rs` | Shared protocol/domain logic |
| Workspace/worktree behavior | `workspaces_core.rs`, `workspaces_core/*`, `worktree_core.rs` | Workspace lifecycle belongs here first |
| Git/GitHub backend behavior | `git_core.rs`, `git_ui_core.rs`, `git_ui_core/*` | Shared git/UI semantics live here |
| Files, prompts, settings, usage | `files_core.rs`, `prompts_core.rs`, `settings_core.rs`, `local_usage_core.rs` | Cross-runtime contract logic |

## Local Rules

- New cross-runtime behavior starts here before app or daemon adapters.
- Keep shared modules domain-focused; avoid leaking transport/runtime concerns into this layer.
- If a change needs app and daemon parity, the design should be visible here first.
- Contract changes usually imply frontend type/service updates and daemon/app surface updates.

## Testing Pattern

- Shared Rust tests are a mix of inline `mod tests` and adjacent `tests.rs` files.
- Add or extend shared-core tests when behavior changes here; do not rely on adapter-level tests alone.

## Hotspots

- `codex_core.rs`
- `workspaces_core.rs`
- `git_ui_core.rs`
- `local_usage_core.rs`

## Anti-Patterns

- Do not implement real domain logic only in `lib.rs` or daemon RPC handlers.
- Do not duplicate shared logic across app and daemon.

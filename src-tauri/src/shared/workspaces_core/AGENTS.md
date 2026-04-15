# Shared Workspaces Core Guide

## Overview

`src-tauri/src/shared/workspaces_core` is the workspace/worktree lifecycle core used by both runtimes.

## Where To Look

| Task | Location | Notes |
| --- | --- | --- |
| Session connection + runtime callback boundaries | `connect.rs` | Shared/runtime seam lives here |
| CRUD, persistence, cleanup | `crud_persistence.rs` | High-risk lifecycle logic |
| Worktree patching and git orchestration | `git_orchestration.rs` | Parent repo cleanliness and patch semantics |
| Path/io/helper behavior | `io.rs`, `helpers.rs`, `worktree.rs` | Supporting primitives for core flows |

## Local Rules

- Keep runtime-specific spawning abstracted; this module should express workspace semantics, not UI/runtime policy.
- Treat persistence and cleanup changes as lifecycle changes: verify add/remove/restore edge cases.
- Worktree patch application and parent-repo cleanliness are first-class invariants, not optional niceties.

## Testing Pattern

- Add focused Rust tests for lifecycle and cleanup paths when modifying CRUD or orchestration behavior.
- Prefer testing concrete edge cases over broad smoke coverage here.

## Hotspots

- `connect.rs`
- `crud_persistence.rs`
- `git_orchestration.rs`

## Anti-Patterns

- Do not move workspace lifecycle semantics into app-only or daemon-only adapters.
- Do not change cleanup/persistence flows without explicit tests.

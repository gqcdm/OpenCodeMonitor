# Backend Adapter Guide

## Overview

`src-tauri/src/backend` owns OpenCode server lifecycle management, backend event payloads, and the Rust-only translation layer that preserves frontend event contracts.

## Where To Look

| Task | Location | Notes |
| --- | --- | --- |
| OpenCode server lifecycle and session routing | `app_server.rs` | Process spawn, SSE/event routing, shared runtime adapter |
| OpenCode -> frontend event translation | `event_translator.rs` | Critical contract boundary |
| Backend event payload types | `events.rs` | Keep event shapes aligned with frontend parsers |

## Local Rules

- This is the only place protocol translation logic should live.
- Preserve CodexMonitor-shaped frontend events unless intentionally changing the contract across the stack.
- Changes here often require paired validation in `docs/app-server-events.md`, frontend parser utilities, and thread handling.
- Keep server/process management concerns in `app_server.rs`; do not spread them into unrelated modules.

## Testing Pattern

- Translation and event-order changes need Rust tests plus frontend validation where ordering/parsing is consumed.
- Treat event sequencing as an invariant, not just a rendering detail.

## Hotspots

- `event_translator.rs`
- `app_server.rs`

## Anti-Patterns

- Do not push protocol translation into React code.
- Do not change event payload semantics without checking the full frontend consumption path.

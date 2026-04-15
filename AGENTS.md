# OpenCodeMonitor Agent Guide

Canonical instructions only. Describe current truth, not history.

## Overview

OpenCodeMonitor is a Tauri desktop app for monitoring and interacting with OpenCode agents across local workspaces.
It preserves CodexMonitor-shaped frontend contracts while confining OpenCode REST/SSE adaptation to a narrow Rust layer.

Generated: 2026-04-15 Asia/Shanghai
Commit: 37cd863
Branch: main

## Structure

```text
.
├── src/                     # React frontend
│   ├── features/app/        # shell bootstrap, orchestration, layout wiring
│   ├── features/threads/    # thread state machine and event handling
│   ├── features/settings/   # settings surface and orchestration
│   ├── features/git/        # git/github UI workflows
│   └── features/workspaces/ # workspace/worktree UI flows
├── src-tauri/src/shared/    # backend source of truth across app and daemon
├── src-tauri/src/backend/   # OpenCode server lifecycle + event translation
├── src-tauri/src/bin/codex_monitor_daemon/ # daemon runtime + RPC surface
├── docs/                    # canonical navigation and architecture docs
└── scripts/                 # doctor checks and codemods
```

## Where To Look

| Task | Location | Notes |
| --- | --- | --- |
| Frontend composition and shell wiring | `src/App.tsx`, `src/features/app/*` | `App.tsx` is heavy composition root; move stateful logic into hooks/orchestration |
| Thread lifecycle, reducer, event routing | `src/features/threads/*` | Thread state is a local state machine; preserve event ordering invariants |
| Settings flow and persistence wiring | `src/features/settings/*`, `src/services/tauri.ts`, `src-tauri/src/shared/settings_core.rs` | Keep TS/Rust contracts aligned |
| Workspace/worktree behavior | `src/features/workspaces/*`, `src-tauri/src/shared/workspaces_core*` | Shared core first; app/daemon adapters second |
| Git/GitHub UI behavior | `src/features/git/*`, `src-tauri/src/shared/git_ui_core*`, `src-tauri/src/shared/git_core.rs` | Frontend and Rust layers both participate |
| Frontend IPC calls | `src/services/tauri.ts` | Only place frontend should invoke Tauri commands |
| Frontend event fanout | `src/services/events.ts` | Single-listener hub; do not scatter subscriptions |
| Tauri app command surface | `src-tauri/src/lib.rs` | Match any new backend command across IPC and tests |
| OpenCode server lifecycle and event translation | `src-tauri/src/backend/*` | Translation stays in Rust, never frontend |
| Daemon RPC surface | `src-tauri/src/bin/codex_monitor_daemon/*` | Keep RPC methods/payloads aligned with app surface |

## Code Map

| Area | Entry | Role |
| --- | --- | --- |
| Browser bootstrap | `src/main.tsx` | Mounts `App` and handles mobile viewport/bootstrap quirks |
| Frontend shell | `src/App.tsx` | Global composition, layout wiring, orchestration handoff |
| App bootstrap | `src/features/app/bootstrap/useAppBootstrap.ts` | Settings, dictation, debug, liquid-glass boot sequence |
| Thread reducer | `src/features/threads/hooks/useThreadsReducer.ts` | Local thread state machine and slice dispatch |
| App command registry | `src-tauri/src/lib.rs` | Tauri invoke handler and desktop runtime setup |
| Shared backend core | `src-tauri/src/shared/mod.rs` | Cross-runtime source of truth |
| Server/runtime adapter | `src-tauri/src/backend/app_server.rs` | `opencode serve` lifecycle, SSE routing, session spawn |
| Event translation | `src-tauri/src/backend/event_translator.rs` | OpenCode events -> CodexMonitor-shaped frontend events |
| Daemon entrypoint | `src-tauri/src/bin/codex_monitor_daemon.rs` | Remote runtime wiring |
| Daemon RPC | `src-tauri/src/bin/codex_monitor_daemon/rpc.rs` | JSON-RPC dispatch, notifications, response shaping |

## Repo-Wide Rules

1. All OpenCode protocol translation happens in Rust, never in the frontend.
2. Shared/domain backend logic goes in `src-tauri/src/shared/*` first.
3. Keep app and daemon thin adapters around shared cores.
4. Preserve JSON-RPC method names and payload shapes unless intentionally changing contracts.
5. Do not rename internal `codex_*` Rust module paths just to match product wording.
6. Keep frontend Tauri calls in `src/services/tauri.ts` and event fanout in `src/services/events.ts`.
7. Keep Rust and TypeScript contracts synchronized: `src-tauri/src/types.rs` <-> `src/types.ts`.

## Frontend Rules

- `src/App.tsx` is composition root, not a dumping ground for new domain logic.
- Move stateful orchestration into `src/features/app/hooks/*`, `bootstrap/*`, or `orchestration/*`.
- Keep presentational UI in feature components.
- Use import aliases: `@/*`, `@app/*`, `@settings/*`, `@threads/*`, `@services/*`, `@utils/*`.

## Backend Rules

- Backend changes that can run remotely must respect app/daemon parity.
- New backend commands require updates across shared core, app surface, frontend IPC, daemon RPC, and tests.
- If event payload format changes, update parser/guards in `src/utils/appServerEvents.ts` first.

## Design System Rules

- Reuse existing design-system primitives and tokens for shared shell chrome.
- Do not reintroduce duplicated modal/toast/panel/popover shell styling in feature CSS.
- Prefer codemods and DS primitives over ad-hoc shell markup.

## Validation

```bash
npm install
npm run doctor:strict
npm run lint
npm run typecheck
npm run test
cd src-tauri && cargo check
cd src-tauri && cargo test
```

Release/local app build:

```bash
npm run tauri:build
```

Windows Tauri build depends on `npm run doctor:win` and LLVM/clang.

## Hotspots

- `src/App.tsx`
- `src/features/settings/components/SettingsView.tsx`
- `src/features/threads/hooks/useThreadsReducer.ts`
- `src-tauri/src/shared/git_ui_core.rs`
- `src-tauri/src/shared/workspaces_core.rs`
- `src-tauri/src/shared/codex_core.rs`
- `src-tauri/src/backend/event_translator.rs`
- `src-tauri/src/backend/app_server.rs`
- `src-tauri/src/bin/codex_monitor_daemon/rpc.rs`

## Canonical References

- `README.md` - setup, release, validation
- `docs/codebase-map.md` - task-oriented navigation
- `docs/app-server-events.md` - frontend event contract and ordering invariants
- `docs/shaping/rest-api-migration.md` - backend migration and parity notes
- `opencode-server-api.mdx` and `tmp/opencode` - OpenCode API references before protocol changes

## Notes

- `.memory/SUMMARY.md` is referenced but currently absent; do not assume project memory exists.
- Child `AGENTS.md` files define local exceptions and workflow details. Keep them short and avoid repeating this file.

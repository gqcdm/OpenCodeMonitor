# Frontend App Shell Guide

## Overview

`src/features/app` is the frontend shell/orchestration layer: bootstrap, layout wiring, global controllers, and shell components.

## Where To Look

| Task | Location | Notes |
| --- | --- | --- |
| Boot app settings/dictation/debug/liquid glass | `bootstrap/useAppBootstrap.ts` | Keep bootstrap compositional; avoid domain-specific side effects |
| Route backend events into UI state | `hooks/useAppServerEvents.ts` | This is app-level event wiring, not thread business logic |
| Coordinate shell controllers | `hooks/use*Controller.ts` | Compose lower-level feature hooks here |
| Global layout/sidebar/topbar work | `components/AppLayout.tsx`, `components/Sidebar.tsx`, `components/MainHeader.tsx` | Shell UI only; do not bury backend contracts here |
| Cross-feature orchestration | `orchestration/*` | Prefer orchestration files over growing `App.tsx` |

## Local Rules

- Keep `App.tsx` and this directory focused on wiring, shell state, and cross-feature composition.
- New feature-specific behavior belongs in its feature directory unless it truly coordinates multiple domains.
- If a hook talks to Tauri, keep the call in `src/services/tauri.ts`; this layer should orchestrate, not invoke directly.
- Event subscription fanout stays centralized; do not introduce side-channel listeners in shell components.
- Prefer controllers/orchestration hooks over pushing more imperative logic into components.

## Testing Pattern

- Tests are colocated next to hooks/components.
- Shell behavior tests commonly mock events, controllers, or Tauri wrappers.
- If you touch event routing or shell controllers, add/adjust colocated tests here before broader suites.

## Hotspots

- `hooks/useAppServerEvents.ts`
- `hooks/useWorkspaceController.ts`
- `hooks/useLayoutController.ts`
- `components/AppLayout.tsx`
- `components/MainHeader.tsx`

## Anti-Patterns

- Do not turn shell hooks into domain-specific business logic buckets.
- Do not duplicate feature logic already owned by `threads`, `workspaces`, `git`, or `settings`.
- Do not add raw Tauri event listeners outside the central services/hub pattern.

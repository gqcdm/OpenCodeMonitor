# Settings Domain Guide

## Overview

`src/features/settings` owns the settings surface, section orchestration, and frontend-side settings workflows.

## Where To Look

| Task | Location | Notes |
| --- | --- | --- |
| Main settings surface | `components/SettingsView.tsx` | Large hotspot; keep as composition and section orchestration |
| Settings state and persistence flow | `hooks/useAppSettings.ts` | Frontend settings entrypoint |
| View orchestration | `hooks/useSettingsViewOrchestration.ts` | Add section-level coordination here |

## Local Rules

- Keep section wiring and navigation in settings hooks/components; do not push cross-app state back into `App.tsx`.
- Any contract change touching settings must stay aligned with `src/services/tauri.ts`, `src/types.ts`, and Rust settings types/core.
- File-system or backend-backed settings behavior still follows the shared-core-first backend rule from root.
- Prefer adding a new section/component pair over growing `SettingsView.tsx` monolithically.

## Testing Pattern

- Settings tests are colocated and typically exercise section orchestration or large-surface rendering.
- For contract changes, cover both UI behavior and typed data mapping.

## Hotspots

- `components/SettingsView.tsx`
- `hooks/useAppSettings.ts`
- `hooks/useSettingsViewOrchestration.ts`

## Anti-Patterns

- Do not change settings contracts in frontend only.
- Do not hide settings persistence logic inside presentational section components.

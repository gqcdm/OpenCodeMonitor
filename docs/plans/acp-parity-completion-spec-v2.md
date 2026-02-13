# ACP Parity Completion - Execution Spec v2

Date: 2026-02-13
Status: Implementation-ready
Scope: Finish remaining OpenCode ACP parity with minimal blast radius

## 1. Goal

Complete the remaining ACP migration gaps so the app behaves consistently for foreground chat, background helper prompts, model/effort selection, plan updates, and UI/docs surfaces.

## 2. Ground Truth (Inspected)

- ACP spawn/init is live: `src-tauri/src/backend/app_server.rs:330`, `src-tauri/src/backend/app_server.rs:490`
- ACP event translation exists: `src-tauri/src/backend/event_translator.rs:94`
- Core thread methods already use ACP: `src-tauri/src/shared/codex_core.rs:77`, `src-tauri/src/shared/codex_core.rs:148`, `src-tauri/src/shared/codex_core.rs:379`
- Background helpers still call Codex protocol (must migrate): `src-tauri/src/shared/codex_aux_core.rs:280`, `src-tauri/src/shared/codex_aux_core.rs:325`, `src-tauri/src/shared/codex_aux_core.rs:390`
- `session/update` translation currently emits to sink; background callback routing for translated events is missing: `src-tauri/src/backend/app_server.rs:421`
- `plan` updates are not mapped; unknown session updates are dropped: `src-tauri/src/backend/event_translator.rs:194`
- Frontend already handles `turn/plan/updated`: `src/features/app/hooks/useAppServerEvents.ts:294`
- Stale Codex path text remains in settings: `src/features/settings/components/sections/SettingsCodexSection.tsx:586`

## 3. Product Decisions (Locked)

- Hidden helper sessions: hide locally by default in thread list.
- URL images: support enabled with strict fetch policy.
- Model/effort set failure: continue prompt send, warn once.

## 4. Risk Register

- R1 [CRIT]: Prompt event cross-wiring if multiple `session/prompt` overlap per workspace.
  - Mitigation: serialize prompt sends per workspace session (single in-flight prompt lock).
- R2 [CRIT]: Background helper deadlock if translated events never hit callback channel.
  - Mitigation: route translated events by `threadId` to `background_thread_callbacks` before sink emit.
- R3 [CRIT]: SSRF/oversize/invalid content from URL image fetch.
  - Mitigation: strict URL + network + MIME + timeout + byte-cap policy.
- R4 [HIGH]: False `turn/completed` on prompt failure.
  - Mitigation: emit `turn/completed` only on success; emit error path otherwise.
- R5 [HIGH]: ACP model-setting method uncertainty.
  - Mitigation: capability probe + fallback; never block prompt.

## 5. Implementation Plan (PR-sized)

### PR1 - Background helper ACP migration

Files:
- `src-tauri/src/shared/codex_aux_core.rs`

Changes:
- Replace `thread/start` with `session/new` in `run_background_prompt_core`.
- Replace `turn/start` with `session/prompt`.
- Remove ACP-nonexistent `thread/archive`; use local hidden-session registry cleanup only.
- Keep background thread hide behavior (`codex/backgroundThread`) unchanged.

Acceptance:
- Commit message generation and run metadata generation both return valid text.
- No protocol error logs from old method names.

### PR2 - Route translated updates to background callbacks

Files:
- `src-tauri/src/backend/app_server.rs`

Changes:
- In `session/update` branch, after translation:
  - inspect translated event `params.threadId`
  - if callback exists for thread id, send to callback channel
  - otherwise emit to app event sink
- Preserve existing behavior for non-`session/update` notifications.

Acceptance:
- Background helper collects streamed deltas and terminal signal without requiring UI sink path.

### PR3 - Prompt lifecycle correctness + prompt serialization

Files:
- `src-tauri/src/backend/app_server.rs`
- `src-tauri/src/shared/codex_core.rs`

Changes:
- Add per-`WorkspaceSession` prompt mutex/guard to allow one in-flight prompt at a time.
- In `send_user_message_core`, enforce lifecycle:
  - emit `turn/started` before send
  - on prompt error, emit error event and return error payload/result
  - emit `turn/completed` only after successful prompt return

Acceptance:
- No `turn/completed` emitted for failed prompt paths.
- Concurrent send attempts are serialized and do not mix item/turn ids.

### PR4 - Model/effort parity (best-effort)

Files:
- `src-tauri/src/shared/codex_core.rs`

Changes:
- Compare requested model against cached current model.
- If changed, attempt ACP model-set request before `session/prompt`.
- If unsupported/failure, continue prompt and emit one warning per workspace/session.
- Map effort only if ACP supports variant/effort semantics in current protocol response metadata.

Notes:
- Method name/shape is UNVERIFIED; implementation must include fallback and not hard fail.

Acceptance:
- Prompt send always proceeds when model-set fails.
- Warning emitted once for repeated failures.

### PR5 - URL image strict fetch pipeline

Files:
- `src-tauri/src/shared/codex_core.rs`

Changes:
- In `build_acp_prompt_parts`, for `http/https` image URLs:
  - allow only `http`/`https`
  - block localhost/link-local/private CIDR targets
  - enforce timeout
  - enforce max response size
  - require image MIME
  - base64 encode bytes to ACP `image` part
- On policy violation, return explicit user-facing error.

Acceptance:
- Valid public image URL works.
- Blocked host/type/size/timeout produce deterministic error.

### PR6 - Complete event translation for plan updates

Files:
- `src-tauri/src/backend/event_translator.rs`

Changes:
- Add `sessionUpdate: "plan"` mapping to `turn/plan/updated`.
- Payload contract:
  - `params.threadId`
  - `params.turnId`
  - `params.explanation`
  - `params.plan`
- Ensure shape remains compatible with `normalizePlanUpdate`.

Acceptance:
- Frontend plan UI updates with ACP plan events.

### PR7 - Hidden helper session filtering

Files:
- `src-tauri/src/shared/codex_core.rs`
- supporting local state module(s) as needed

Changes:
- Persist helper-created session IDs in local workspace metadata.
- Filter those IDs from default `list_threads_core` output.
- Keep data reversible (toggle/debug path can expose all sessions later).

Acceptance:
- Background helper sessions do not appear in normal thread list by default.

### PR8 - Frontend and docs cleanup

Files:
- `src/features/settings/components/sections/SettingsCodexSection.tsx`
- `src/services/tauri.ts`
- `README.md`
- `docs/app-server-events.md`

Changes:
- Remove/hide Codex-only UI/API surfaces not used in MVP (skills/collab/rate-limit/login paths).
- Correct stale config path copy to OpenCode path.
- Update README status to current migration phase.
- Update events doc to ACP reality and current supported mapping.

Acceptance:
- No stale Codex path language in settings/docs for global config location.
- Removed surfaces no longer shown in main user flows.

## 6. Test Plan

### Rust unit tests

- `event_translator.rs`
  - `plan` -> `turn/plan/updated`
  - chunk parsing keeps meaningful whitespace behavior
- `app_server.rs`
  - translated `session/update` routed to callback when callback exists
- `codex_aux_core.rs`
  - background helper success/error/timeout cleanup
- `codex_core.rs`
  - model-set attempted only when model changes
  - model-set failure does not block send
  - URL image policy allow/deny cases
  - turn lifecycle success vs failure

### Integration tests

- Foreground chat + background helper in same workspace without event cross-talk
- Hidden helper sessions excluded from thread list
- Plan updates visible in existing UI path

### Manual verification

- Start prompt, force error, verify no false completed state
- Generate commit message and run metadata successfully
- Send prompt with URL image (valid + blocked cases)
- Change model, verify best-effort behavior with one warning on failure

### Validation commands (to run during implementation)

- `npm run typecheck`
- `npm run test`
- `cd src-tauri && cargo check`
- `cd src-tauri && cargo test`

## 7. Rollout and Backout

### Rollout

- Stage 1: PR1-PR3 (correctness baseline)
- Stage 2: PR4-PR6 (feature parity)
- Stage 3: PR7-PR8 (cleanup + docs)

### Monitoring

- Count unknown dropped ACP `sessionUpdate` types
- Count model-set failures
- Count URL image policy rejections by reason
- Track prompt lifecycle mismatches (`started` without terminal event)

### Backout

- Disable URL image fetch path and revert to explicit validation error mode.
- Disable hidden-session filtering if visibility regressions are reported.
- Disable model-set pre-call while keeping prompt send path intact.

## 8. Out of Scope

- Reworking reducer contracts or protocol translation into frontend.
- Broad naming refactors of internal `codex_*` modules.
- New daemon/app feature surfaces outside ACP parity gaps.

## 9. UNVERIFIED Items and How to Verify

- ACP model-set RPC name and payload
  - Verify by wire-capture against current `opencode acp` while switching model.
- ACP effort/variant semantics
  - Verify using `session/new` metadata and successful model/variant switch response.
- ACP session archive/delete capabilities
  - Verify protocol docs or wire capture before designing non-local archival behavior.

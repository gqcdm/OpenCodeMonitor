---
shaping: true
---

# Upstream Cherry-Pick Batch (v0.7.50 → v0.7.57) — Shaping

## Frame

**Source**: CodexMonitor releases v0.7.50 → v0.7.57

**Problem**: Our fork is missing useful upstream improvements: build transparency, thread stability, and composer UX.

**Outcome**: Users see build metadata, threads don't disappear during refresh, and users can quote messages into the composer.

---

## Requirements (R)

| ID | Requirement | Status |
|----|-------------|--------|
| **R0** | Users can identify exact build version, commit, and date for debugging/support | Core goal |
| **R1** | Active/processing threads aren't lost during partial list refresh | Core goal |
| **R2** | Parent thread chains preserved when child thread is visible | Core goal |
| **R3** | Users can quote assistant messages into the composer | Core goal |
| **R4** | Changes integrate cleanly without breaking existing functionality | Must-have |
| **R5** | Minimal deviation from upstream to ease future merges | Nice-to-have |

---

## Shape A: Port Upstream with Adaptation

Since upstream has working implementations, the shape is to adapt their code to our codebase.

| Part | Mechanism | Flag |
|------|-----------|:----:|
| **A1** | **Build Metadata** | |
| A1.1 | Add git info extraction to `vite.config.ts` (execSync for commit/branch/date) | |
| A1.2 | Declare `__APP_COMMIT_HASH__`, `__APP_BUILD_DATE__`, `__APP_GIT_BRANCH__` in `vite-env.d.ts` | |
| A1.3 | Add `SettingsAboutSection.tsx` displaying metadata | |
| A1.4 | Add "About" to settings navigation and section containers | |
| A1.5 | Update existing `AboutView.tsx` to include metadata or redirect to settings | |
| **A2** | **Thread Anchor Preservation** | |
| A2.1 | Extend `setThreads` case in `threadLifecycleSlice.ts` with reconciliation logic | |
| A2.2 | Preserve active thread if missing from incoming list | |
| A2.3 | Preserve threads with `isProcessing` status | |
| A2.4 | Walk `threadParentById` to preserve parent chain | |
| A2.5 | Freshen `updatedAt` using `lastAgentMessageByThread` and `processingStartedAt` | |
| **A3** | **Quote Action** | |
| A3.1 | Add `onQuoteMessage` prop to `Messages` component | |
| A3.2 | Add `toMarkdownQuote()` helper (prefix lines with `> `) | |
| A3.3 | Add `handleQuoteMessage` callback in Messages | |
| A3.4 | Wire quote button in `MessageRows.tsx` for assistant messages | |
| A3.5 | Connect via `buildPrimaryNodes` using existing `onInsertComposerText` | |
| A3.6 | Add quote button styles to `messages.css` | |

---

## Fit Check: R × A

| Req | Requirement | Status | A |
|-----|-------------|--------|---|
| R0 | Users can identify exact build version, commit, and date for debugging/support | Core goal | ✅ |
| R1 | Active/processing threads aren't lost during partial list refresh | Core goal | ✅ |
| R2 | Parent thread chains preserved when child thread is visible | Core goal | ✅ |
| R3 | Users can quote assistant messages into the composer | Core goal | ✅ |
| R4 | Changes integrate cleanly without breaking existing functionality | Must-have | ✅ |
| R5 | Minimal deviation from upstream to ease future merges | Nice-to-have | ✅ |

**Notes:**
- R4: All mechanisms use existing patterns (Vite define, reducer extension, existing `onInsertComposerText`)
- R5: Code will closely match upstream with minor naming adjustments

---

## Slices

Each slice is independently deployable and demo-able.

### V1: Build Metadata in About

**Demo**: Open Settings → About, see version/commit/branch/date

| Affordance | Type | Place |
|------------|------|-------|
| Version display | UI | SettingsAboutSection |
| Commit hash display | UI | SettingsAboutSection |
| Branch display | UI | SettingsAboutSection |
| Build date display | UI | SettingsAboutSection |
| Git info extraction | Non-UI | vite.config.ts |
| Type declarations | Non-UI | vite-env.d.ts |

**Files**:
- `vite.config.ts` — add git exec + define
- `src/vite-env.d.ts` — declare globals
- `src/features/settings/components/sections/SettingsAboutSection.tsx` — new file
- `src/features/settings/components/sections/SettingsSectionContainers.tsx` — add case
- `src/features/settings/components/SettingsNav.tsx` — add nav item
- `src/features/settings/components/settingsTypes.ts` — add "about" to CodexSection
- `src/features/settings/components/settingsViewConstants.ts` — add label

### V2: Thread Anchor Preservation

**Demo**: Start a thread, trigger list refresh while processing, thread stays visible

| Affordance | Type | Place |
|------------|------|-------|
| Thread reconciliation | Non-UI | threadLifecycleSlice |
| Active thread preservation | Non-UI | threadLifecycleSlice |
| Processing thread preservation | Non-UI | threadLifecycleSlice |
| Parent chain walking | Non-UI | threadLifecycleSlice |

**Files**:
- `src/features/threads/hooks/threadReducer/threadLifecycleSlice.ts` — extend setThreads case

### V3: Quote Action for Composer

**Demo**: Click quote button on assistant message → text appears in composer as blockquote

| Affordance | Type | Place |
|------------|------|-------|
| Quote button | UI | MessageRows |
| Markdown quote formatter | Non-UI | Messages |
| Quote handler | Non-UI | Messages |
| Composer text insertion | Non-UI | buildPrimaryNodes (existing) |

**Files**:
- `src/features/messages/components/Messages.tsx` — add prop, helper, handler
- `src/features/messages/components/MessageRows.tsx` — add quote button
- `src/features/layout/hooks/layoutNodes/buildPrimaryNodes.tsx` — wire onQuoteMessage
- `src/styles/messages.css` — quote button styles

---

## Open Questions

| # | Question | Impact |
|---|----------|--------|
| Q1 | Should we keep the existing standalone AboutView or redirect it to Settings? | Low — can decide during V1 |
| Q2 | Should quote button be always visible or appear on hover? | Low — follow upstream pattern |

---

## Validation

Each slice validated with:
- `npm run typecheck`
- `npm run test` (for slices touching tested code)
- Manual testing of the demo scenario

---

## Upstream References

| Item | Upstream Commit | Upstream PR |
|------|-----------------|-------------|
| Build metadata | 40f6fcb | #490 |
| Thread anchor fix | c0f144b | #494 |
| Quote action | 5e656d5 | #504 |

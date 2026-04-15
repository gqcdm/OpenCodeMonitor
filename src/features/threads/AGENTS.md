# Threads Domain Guide

## Overview

`src/features/threads` owns thread state, event handling, reducer slices, messaging, approvals, and thread-specific persistence.

## Where To Look

| Task | Location | Notes |
| --- | --- | --- |
| Thread state machine | `hooks/useThreadsReducer.ts`, `hooks/threadReducer/*` | Slice-based reducer; keep responsibilities separated |
| High-level thread orchestration | `hooks/useThreads.ts` | Entry point for thread state integration |
| Event-driven state updates | `hooks/useThread*Events.ts` | Preserve event semantics and item ordering |
| Messaging/send/interrupt flows | `hooks/useThreadMessaging.ts`, `hooks/useQueuedSend.ts` | Keep send pipeline coherent |
| Thread storage and normalization | `hooks/useThreadStorage.ts`, `utils/threadNormalize.ts`, `utils/threadStorage.ts` | Storage shape changes need tests |

## Local Rules

- Preserve the user-message-first ordering invariant for turn items.
- Keep reducer logic inside slice reducers; avoid ad-hoc state mutation in unrelated hooks.
- Prefer explicit thread event handlers over burying event interpretation in components.
- Thread domain owns thread status, approvals, user input requests, plans, and token/account snapshots.
- If event shape changes, verify both Rust translation and frontend thread handling together.

## Testing Pattern

- This directory has dense colocated tests and an explicit integration test.
- Add reducer/slice tests when changing state transitions.
- Add event-hook or messaging tests when changing live event handling.
- Keep `.integration.test.tsx` for multi-hook thread flow changes that span more than one local module.

## Hotspots

- `hooks/useThreadsReducer.ts`
- `hooks/threadReducer/threadItemsSlice.ts`
- `hooks/useThreadMessaging.ts`
- `hooks/useThreadTurnEvents.ts`
- `hooks/useThreads.integration.test.tsx`

## Anti-Patterns

- Do not let UI components own thread business state transitions.
- Do not bypass reducer/event abstractions with one-off array manipulation.
- Do not assume backend item IDs alone guarantee semantic ordering.

---
shaping: true
---

# Multi-Provider Sessions — Shaping

Handle session usage tracking when users can switch between API providers (Anthropic, OpenAI, etc.) and account types (API keys vs OAuth plans).

---

## Frame

### Source

> how should we handle the session feature with opencode? with codex it is a single provider. with opencode users could be using api or various different providers.

Screenshot shows "Session · Resets 2 hours" and "Weekly · Resets 4 days" usage meters at the bottom of the UI.

### Problem

Codex assumes a single provider (OpenAI) with a fixed rate limit structure (session + weekly windows). OpenCode supports multiple providers (Anthropic, OpenAI, OpenRouter, etc.) with different billing models:

1. **OAuth-based plans** (e.g., Anthropic Max, OpenAI ChatGPT) — have rate limit windows (session/weekly)
2. **API keys** — pay-per-use, no rate limits, just credit balance
3. **Self-hosted** — no limits at all

The current UI shows "Session" and "Weekly" meters assuming everyone has time-based rate limits. This is incorrect for API key users and multi-provider setups.

### Outcome

Usage display that accurately reflects the user's actual billing model per provider. API key users see spend/credits. OAuth users see rate limits. Multi-provider users see usage for their active provider.

---

## Requirements (R)

| ID | Requirement | Status |
|----|-------------|--------|
| R0 | Show usage information relevant to the user's current billing model | Core goal |
| R1 | API key users see credit balance or spend, not rate limit windows | Must-have |
| R2 | OAuth/plan users see rate limit windows (session/weekly) when available | Must-have |
| R3 | Usage display updates when user switches providers or models | Must-have |
| R4 | No misleading UI — don't show "Session: 0%" to API key users | Must-have |
| R5 | Support providers that don't expose usage data at all (graceful degradation) | Must-have |
| R6 | Usage is scoped to the active provider, not aggregated across all providers | Undecided |
| R7 | Token usage per-turn/per-thread still works regardless of billing model | Must-have |
| R8 | Local usage tracking (JSONL scanning) continues to work for historical view | Nice-to-have |

---

## Shapes

### CURRENT: Single-provider rate limits

| Part | Mechanism |
|------|-----------|
| **CUR1** | `RateLimitSnapshot` has `primary` (session) and `secondary` (weekly) windows |
| **CUR2** | `usageLabels.ts` computes percentages assuming both windows exist |
| **CUR3** | UI always shows "Session" and "Weekly" meters |
| **CUR4** | `account/rateLimits/updated` event pushes updates |
| **CUR5** | `AccountSnapshot.type` distinguishes `chatgpt` vs `apikey` but UI treats them the same |
| **CUR6** | `CreditsSnapshot` exists but is secondary to rate limits |

### A: Provider-aware usage display

The usage display adapts based on the active provider's billing model. Rate limit windows shown for OAuth plans, credit balance for API keys, nothing for self-hosted.

| Part | Mechanism |
|------|-----------|
| **A1** | `AccountSnapshot` gains `billingModel: "rate_limited" | "pay_per_use" | "unlimited"` derived from provider auth method |
| **A2** | `usageLabels.ts` returns different label shapes based on billing model |
| **A3** | UI conditionally renders rate limit meters OR credit balance OR nothing |
| **A4** | When user switches provider/model, re-fetch usage info for new provider |
| **A5** | `RateLimitSnapshot` becomes optional — null for pay-per-use and unlimited |
| **A6** | `CreditsSnapshot` expanded to show balance, spend-this-session, and cost-per-token hints |

### B: Unified usage abstraction

Abstract all billing models into a single "usage" concept that the UI displays uniformly.

| Part | Mechanism |
|------|-----------|
| **B1** | Normalize all billing models into `UsageSnapshot { percent?: number, label: string, sublabel?: string }` |
| **B2** | Rate limits → percent; API credits → "X credits remaining"; Unlimited → "Unlimited" |
| **B3** | Single meter component that displays whatever the backend provides |
| **B4** | Backend computes the normalized snapshot, frontend just renders |

---

## Open Questions

| # | Question | Status |
|---|----------|--------|
| Q1 | Does OpenCode's REST API expose rate limits and/or credit balance per provider? | Needs spike |
| Q2 | Which providers have rate limits vs pay-per-use vs neither? | Needs spike |
| Q3 | Should we show usage for all connected providers or just the active one? | Undecided |
| Q4 | How does OpenCode report usage for API key providers? | Needs spike |

---

## Spike: OpenCode Usage API

### Context

We need to understand what usage information OpenCode exposes per provider before we can design the UI.

### Questions and Answers

| # | Question | Answer |
|---|----------|--------|
| **S1-Q1** | What does `GET /provider` return? Does it include rate limits or credit info? | No. Returns `{ all: Provider[], default: {...}, connected: string[] }`. Provider includes models and auth methods but no usage/rate data. |
| **S1-Q2** | What does `AccountSnapshot` look like in OpenCode's type system? | Not exposed. The OpenCode REST API has no account/usage endpoint. |
| **S1-Q3** | Is there a per-provider or per-auth-method usage endpoint? | No. The OpenCode server API has no rate limit, credits, or billing endpoints. |
| **S1-Q4** | What events does OpenCode emit for usage updates? | Only per-message token counts via `message.updated` events. No rate limit or account-level usage events. |

### Finding

**OpenCode's REST API does not expose rate limit or credit information.** This is fundamentally different from Codex, which had account-level rate limit data.

What OpenCode DOES provide:
- Per-message token usage (`message.updated` → `tokens: { input, output, cache, reasoning }`)
- Provider/model list with auth method types (`GET /provider/auth`)
- Local session logs that can be scanned for historical token usage

What OpenCode does NOT provide:
- Rate limit windows (session/weekly)
- Credit balances
- Account billing status
- Provider-specific usage quotas

### Implication

The "Session: X%" / "Weekly: X%" UI as designed for Codex **cannot be implemented with OpenCode** because the underlying data doesn't exist. We need a different approach.

---

## Revised Requirements

Based on spike findings, requirements need updating:

| ID | Requirement | Status |
|----|-------------|--------|
| R0 | Show usage information relevant to the user's current session | Core goal |
| R1 | ~~API key users see credit balance~~ **Dropped** — OpenCode doesn't expose this | Out |
| R2 | ~~OAuth/plan users see rate limit windows~~ **Dropped** — OpenCode doesn't expose this | Out |
| R3 | 🟡 Usage display reflects token consumption this session | Must-have |
| R4 | No misleading UI — don't show rate limit meters that have no backing data | Must-have |
| R5 | Support providers that don't expose usage data (graceful degradation) | Must-have |
| R6 | 🟡 Per-thread token usage continues to work (already implemented) | Must-have |
| R7 | 🟡 Local usage tracking shows historical token consumption | Nice-to-have |
| R8 | 🟡 Model context window shown relative to current token usage | Nice-to-have |

---

## Revised Shapes

### C: Token-based usage display (no rate limits)

Since OpenCode doesn't expose rate limits, replace the Session/Weekly meters with token-based metrics derived from what we actually have.

| Part | Mechanism |
|------|-----------|
| **C1** | Remove `RateLimitSnapshot` fetching — it's always empty |
| **C2** | Primary metric: tokens used this thread (already have via `thread/tokenUsage/updated`) |
| **C3** | Secondary metric: context window usage percent (tokens / model.limit.context) |
| **C4** | Show "X / Y tokens" or "X% context" instead of "Session: X%" |
| **C5** | Remove "Resets in X hours" — no reset concept for API usage |
| **C6** | Local usage view shows historical token consumption (existing `LocalUsageSnapshot`) |

### D: Hybrid — preserve UI if provider exposes limits

If OpenCode adds rate limit support in the future (or if we detect specific providers that have it), conditionally show the old UI.

| Part | Mechanism | Flag |
|------|-----------|:----:|
| **D1** | Attempt to fetch rate limits via hypothetical endpoint | ⚠️ |
| **D2** | If rate limits exist, show Session/Weekly meters (CURRENT behavior) | |
| **D3** | If no rate limits, fall back to Shape C token display | |
| **D4** | Provider detection: check auth method type to predict billing model | ⚠️ |

---

## Fit Check

| Req | Requirement | Status | CURRENT | C | D |
|-----|-------------|--------|:-------:|:-:|:-:|
| R0 | Show usage information relevant to the user's current session | Core goal | ❌ | ✅ | ✅ |
| R3 | Usage display reflects token consumption this session | Must-have | ✅ | ✅ | ✅ |
| R4 | No misleading UI — don't show empty rate limit meters | Must-have | ❌ | ✅ | ✅ |
| R5 | Support providers with no usage data (graceful degradation) | Must-have | ❌ | ✅ | ✅ |
| R6 | Per-thread token usage continues to work | Must-have | ✅ | ✅ | ✅ |
| R7 | Local usage tracking shows historical token consumption | Nice-to-have | ✅ | ✅ | ✅ |
| R8 | Model context window shown relative to current token usage | Nice-to-have | ❌ | ✅ | ✅ |

**Notes:**
- CURRENT fails R0/R4: Shows "Session: 0%" and "Weekly: 0%" with "Resets X hours" which is misleading — there's no rate limit data backing it
- D has flagged unknowns (D1, D4) — depends on OpenCode adding rate limit endpoints in the future
- C is fully implementable with current OpenCode capabilities

**Recommendation:** Shape C is the pragmatic choice. It uses data we actually have and provides meaningful usage feedback.

---

## Shape C Detail

### Reference: OpenCode TUI

Screenshot shows OpenCode's native UI displays:
```
Context
68,107 tokens
17% used
$0.00 spent
```

### What our UI would show

**Current (misleading):**
```
Session · Resets 2 hours     0%
Weekly · Resets 4 days      14%
```

**Proposed (Shape C) — match OpenCode UI:**
```
Context
68,107 tokens · 17% used
```

Or compact version for status bar:
```
68.1k tokens · 17%
```

The "$0.00 spent" line is omitted — OpenCode calculates this client-side from token counts × model pricing, but this pricing data isn't exposed via REST API.

### Parts Breakdown

| Part | Mechanism |
|------|-----------|
| **C1** | `usageLabels.ts` → `getContextUsageLabels()` takes `ThreadTokenUsage` |
| **C2** | Compute `contextPercent = (total.totalTokens / modelContextWindow) * 100` |
| **C3** | Format tokens with locale separator: `68,107 tokens` |
| **C4** | Show percent used when `modelContextWindow` is known |
| **C5** | Remove rate limit meters, replace with context display |
| **C6** | Keep `LocalUsageSnapshot` for historical view |

### Migration Path

1. Remove `RateLimitSnapshot` from thread state
2. Remove `account_rate_limits_core` stub and related event handling
3. Update `ComposerMetaBar` to show context usage instead of rate limits
4. Add context window percentage computation
5. Update `usageLabels.ts` with new formatting functions

---

## Open Questions

| # | Question | Status |
|---|----------|--------|
| Q1 | Should we show cumulative tokens across all threads or just active thread? | **Active thread only** — matches OpenCode TUI |
| Q2 | How prominent should context window usage be? | **Primary display** — matches OpenCode TUI |
| Q3 | Should we surface local historical usage (last 7 days) in the status bar? | **No** — keep footer simple, historical view accessible elsewhere |

---

## Slices

| # | Slice | Demo | Status |
|---|-------|------|--------|
| V1 | Remove SidebarFooter usage display, add token tooltip to ComposerMetaBar | Hover context ring → "68,107 of 200,000 tokens" | **Done** |
| V2 | Remove rate limit infrastructure | N/A | **Skipped** |

### V1: Final Implementation

**Approach changed:** Instead of replacing the sidebar footer display, we removed it entirely. Context info is already displayed in the `ComposerMetaBar` ("Context free 30%"). Added token count tooltip on hover.

**Changes:**

| File | Change |
|------|--------|
| `src/features/app/components/Sidebar.tsx` | Removed SidebarFooter, removed `activeTokenUsage` prop |
| `src/features/layout/hooks/layoutNodes/buildPrimaryNodes.tsx` | Removed `activeTokenUsage` prop |
| `src/features/layout/hooks/layoutNodes/types.ts` | Moved `activeTokenUsage` to keep single definition |
| `src/features/composer/components/ComposerMetaBar.tsx` | Added `contextTooltip` showing token counts on hover |
| `src/App.tsx` | Cleaned up unused rate limit references |
| `src/features/app/components/SidebarFooter.tsx` | Reverted to original (unused, kept for upstream compat) |
| `src/features/app/utils/usageLabels.ts` | Reverted to original (kept for upstream compat) |

**UI Result:**
- Sidebar footer: **removed** (no misleading rate limit display)
- ComposerMetaBar: Shows "Context free 30%" with tooltip "68,107 of 200,000 tokens" on hover

### V2 Decision: Skipped

**Rationale:** Keep rate limit infrastructure dormant to minimize upstream merge conflicts.

- Upstream CodexMonitor uses rate limits (OpenAI/Codex has them)
- Removing creates conflicts in reducer, types, hooks across many files
- Dead code costs nothing at runtime
- If OpenCode adds rate limit APIs later, infrastructure is ready

**Demo:** App still works, no rate limit fetching or state, cleaner codebase.

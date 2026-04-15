# Git Frontend Guide

## Overview

`src/features/git` owns Git and GitHub UI workflows: status, diffs, branches, pull requests, issues, and review-oriented panels.

## Where To Look

| Task | Location | Notes |
| --- | --- | --- |
| Git status/panel orchestration | `hooks/useGitPanelController.ts`, `hooks/useGitStatus.ts` | Start here for panel behavior |
| Diff and review surfaces | `components/*Diff*`, `components/*Review*` | UI should stay backed by shared DS and typed git state |
| PR/issues flows | `hooks/usePullRequest*`, `hooks/useGitHub*` | Keep GitHub-specific logic cohesive |
| Branch workflows | `hooks/useGitBranches.ts`, `hooks/useBranchSwitcher.ts` | Coordinate with backend git surfaces |

## Local Rules

- Keep GitHub-specific UI behavior in this domain; backend contract changes still flow through shared Rust cores and Tauri services.
- Reuse shared diff styling/tokens; do not hardcode review shell styling or diff colors.
- Prefer extending existing hooks/controllers over adding one-off git calls inside components.
- Treat root selection, staging, PR composition, and review flows as separate concerns.

## Testing Pattern

- Colocated tests cover panel controllers, review prompts, and PR composition.
- Add tests near the workflow you touched; git UI changes often span both hooks and components.

## Hotspots

- `components/GitDiffViewer.tsx`
- `hooks/useGitPanelController.ts`
- `hooks/usePullRequestComposer.ts`

## Anti-Patterns

- Do not scatter git workflow state across generic app hooks.
- Do not duplicate backend git decision logic in the UI.

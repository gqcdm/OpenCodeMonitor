# Workspaces Frontend Guide

## Overview

`src/features/workspaces` owns workspace lifecycle UI, worktree flows, home surfaces, prompts, and workspace-scoped file/agent interactions.

## Where To Look

| Task | Location | Notes |
| --- | --- | --- |
| Main workspace lifecycle state | `hooks/useWorkspaces.ts` | Primary workspace/worktree UI domain hook |
| Workspace home behavior | `hooks/useWorkspaceHome.ts`, `components/WorkspaceHome.tsx` | Home screen and run controls |
| Clone/worktree prompts | `components/ClonePrompt.tsx`, `components/WorktreePrompt.tsx`, related hooks | Keep prompt-specific logic near prompts |
| Workspace files/Agent.md flows | `hooks/useWorkspaceFiles.ts`, `hooks/useWorkspaceAgentMd.ts` | Coordinate with backend files/workspace APIs |

## Local Rules

- Workspace grouping, selection, clone, worktree, and restore behavior belong here, not in generic app shell hooks.
- Frontend changes that affect worktree semantics usually need matching checks in Rust `workspaces_core`.
- Keep prompt UI separate from persistent lifecycle logic in `useWorkspaces.ts`.
- Use this domain for workspace-home concerns before adding shell-specific exceptions.

## Testing Pattern

- Colocated tests cover prompts, workspace lifecycle hooks, restore flows, and workspace-home behavior.
- Changes to cloning/worktree prompts should keep prompt tests close to the prompt/hook touched.

## Hotspots

- `hooks/useWorkspaces.ts`
- `hooks/useWorkspaceHome.ts`
- `components/WorkspaceHome.tsx`

## Anti-Patterns

- Do not bury worktree lifecycle rules in generic shell controllers.
- Do not change frontend workspace semantics without checking shared Rust workspace behavior.

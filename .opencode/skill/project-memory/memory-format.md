# Memory Format Reference

On-demand reference for `.memory/` file formats. Loaded when creating or editing memories.

## Frontmatter Field Reference

| Field | Type | Required | Description |
|---|---|---|---|
| `id` | string | yes | `category/NNN` (e.g., `decisions/001`) |
| `title` | string | yes | Short descriptive title |
| `tags` | string[] | yes | Searchable keywords |
| `relevance` | enum | yes | `permanent`, `project`, `session`, `ephemeral` |
| `status` | enum | yes | `active`, `superseded` |
| `created` | date | yes | When first written (YYYY-MM-DD) |
| `updated` | date | yes | Last modified (YYYY-MM-DD) |
| `last_accessed` | datetime | yes | Wall-clock time of last access |
| `last_accessed_session` | string | yes | Session ID of last access (deterministic ordering key) |
| `access_count` | number | yes | Number of sessions that referenced this |
| `supersedes` | string? | no | ID of memory this replaced |
| `superseded_by` | string? | no | ID of memory that replaced this |
| `related` | string[]? | no | IDs of related memories |
| `phase_epoch` | number? | no | Project phase epoch (required for `relevance: project`) |

### Wikilink Convention

When referencing other memories in body text, use `[[category/NNN-title]]` syntax.
This enables Obsidian graph view, backlinks, and click-through navigation when
`.memory/` is opened as a vault. Agents can parse these alongside frontmatter `related`.

| Context | Format | Example |
|---|---|---|
| Frontmatter `related` | ID only | `related: [decisions/001]` |
| Body text | Wikilink | `[[decisions/001-auth-jwt]]` |
| Body text (aliased) | Wikilink with display text | `[[decisions/001-auth-jwt\|JWT decision]]` |

Both forms are maintained. Frontmatter `related` is the canonical machine-readable
link list. Wikilinks are the human-navigable supplement in prose.

### Relevance Levels

| Level | Meaning | Decay |
|---|---|---|
| `permanent` | Architectural decisions, core conventions | Never |
| `project` | Active project context, current-phase info | After explicit `project_phase_epoch` change |
| `session` | Debugging context, temp workarounds | Archived when `sessions_since_access > 10` |
| `ephemeral` | Meeting notes, WIP context | Deleted on next Load |

---

## Individual Memory Templates

### decisions/NNN-title.md

```markdown
---
id: decisions/001
title: Auth uses JWT with RS256
tags: [auth, jwt, security]
relevance: permanent
status: active
created: 2025-02-19
updated: 2025-02-19
last_accessed: 2025-02-19
last_accessed_session: 2025-02-19-1
access_count: 1
supersedes: null
superseded_by: null
related: []
---

## Context

[What problem or question prompted this decision]

## Decision

[What was decided]

## Rationale

- [Reason 1]
- [Reason 2]

## Alternatives Considered

- [Alternative]: [why rejected]
```

### bugs/NNN-title.md

```markdown
---
id: bugs/001
title: Race condition in auth middleware
tags: [auth, race-condition, middleware]
relevance: session
status: active
created: 2025-02-20
updated: 2025-02-20
last_accessed: 2025-02-20
last_accessed_session: 2025-02-20-1
access_count: 1
supersedes: null
superseded_by: null
related: [decisions/001]
---

## Symptom

[What was observed]

## Root Cause

[What actually caused it] Related: [[decisions/001-auth-jwt]]

## What Was Tried

- [Attempt 1]: [result]
- [Attempt 2]: [result]

## Fix

[What resolved it]

## Prevention

[How to avoid this in the future]
```

### preferences/NNN-title.md

```markdown
---
id: preferences/001
title: Prefer composition over inheritance
tags: [architecture, composition, style]
relevance: permanent
status: active
created: 2025-02-19
updated: 2025-02-19
last_accessed: 2025-02-19
last_accessed_session: 2025-02-19-1
access_count: 1
supersedes: null
superseded_by: null
related: []
---

## Preference

[What the user prefers]

## Context

[When/why this was stated or discovered]

## Applies To

- [Area or situation where this applies]
```

### sessions/YYYY-MM-DD[-N].md

```markdown
---
session_id: 2025-02-19-1
date: 2025-02-19
---

## Summary

[1-3 sentence overview of what was accomplished]

## Tasks Completed

- [Task 1]
- [Task 2]

## Decisions Made

- [Decision]: [brief rationale] -> [[decisions/NNN-title]]

## Memories Created/Updated

| Action | ID | Reason |
|---|---|---|
| created | decisions/003 | New API auth approach decided |
| updated | preferences/001 | Added exception for legacy module |
| superseded | decisions/001 | Replaced JWT with session tokens |
| skipped | - | Duplicate of bugs/002 |

## Open Items

- [Anything left unfinished or pending]
```

---

## SUMMARY.md Template

```markdown
# Project Memory

> [Brief description derived from codebase on first visit]

## Key Context

- [Top 5-10 most important things about this project]
- [Tech stack, architecture patterns, deployment target]

## Active Focus

- [What is currently being worked on]
- [Open decisions or blockers]

## Quick Stats

- Total memories: 0
- Last updated: YYYY-MM-DD
- Sessions since review: 0
- Session tools available: false
- Last bootstrap: null
- Bootstrapped sessions: []
- Project phase: foundation
- Project phase epoch: 1
- Close state: clean
- Index dirty: false
- Active session id: YYYY-MM-DD-1
- Last closed session id: ""
- Writer owner: <agent-id>
- Writer lease ttl seconds: 900
- Writer lease acquired at: <ISO timestamp>
- Writer lease expires at: <ISO timestamp>
```

### Field Ownership

- **Authoritative** (manually managed): `sessions_since_review`, `session_tools_available`,
  `last_bootstrap`, `bootstrapped_sessions`, `project_phase`, `project_phase_epoch`,
  `close_state`, `index_dirty`, `active_session_id`, `last_closed_session_id`,
  `writer_owner`, `writer_lease_ttl_seconds`, `writer_lease_acquired_at`, `writer_lease_expires_at`
- **Derived** (rebuildable from files): `total_memories`, `last_updated`, active/superseded counts

---

## index.md Template

```markdown
# Project Memory Index

> 0 memories | Last updated: YYYY-MM-DD

| ID | Title | Tags | Relevance | Status | Updated |
|---|---|---|---|---|---|
```

Contains all valid memories (`active` + `superseded`).
Excludes `_archive/` and `_quarantine/` entries.
Rebuildable from valid memory files at any time.

---

## Category _summary.md Template

```markdown
# [Category] Summary

[Brief description of what this category contains]

- **[Title]**: [1-2 line compressed digest] ([id])
- **[Title]**: [1-2 line compressed digest] ([id])

Last updated: YYYY-MM-DD | N active [category]
```

Example:

```markdown
# Decisions Summary

Key architectural decisions for this project:

- **Authentication**: JWT with RS256, stateless approach (decisions/001)
- **Database**: PostgreSQL with Drizzle ORM (decisions/002)

Last updated: 2025-02-19 | 2 active decisions
```

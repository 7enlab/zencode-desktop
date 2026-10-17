# Scope Guard (R3)

> "Ask/surface assumptions and wait for approval before building" is one of
> the top rules engineers write into `CLAUDE.md` files (803 files in a
> 10,000-file study). Scope Guard turns that folk rule into enforcement.

## Concept

Before acting, the agent **declares** the files and directories it will
touch (`ScopeDecl`). The `ScopeGuard` (a `PermissionChecker`) intercepts
every mutating tool call:

| Tool | Check |
|------|-------|
| Write / Edit / MultiEdit | `file_path` must be in scope |
| Bash | destructive patterns never allowed; path tokens in mutating commands must be in scope |
| Read / Glob / Grep | always allowed |

Out-of-scope attempts are **blocked** with a loud denial and a
`scope_violation` system event in the session log ("agent tried to edit
X — not in declared scope").

## Ask-first

`needs_approval(decl, root)` flags ambiguous declarations **before the
agent starts**:

- wildcard patterns (`src/*.rs`)
- path escapes (`../secret`)
- absolute paths outside the project root
- more than 20 files (`MAX_AUTO_FILES`)

In headless mode these are rejected with a clear error listing every
reason. The Tauri UI shows the declaration as a checklist card
(`desktop/src/scope.js`) with **Approve / Expand / Deny**.

## Expansion

`ScopeGuard::expand_scope(files, dirs)` adds paths after user approval and
logs a `scope_expanded` system event — the audit trail shows exactly when
the scope grew and by how much.

## Wiring

`RunOptions.scope: Option<ScopeDecl>` — `None` preserves the previous
pre-approved headless behavior. When `Some`, the run:

1. rejects ambiguous declarations up front,
2. emits `scope_declared`,
3. runs with `ScopeGuard` instead of `AllowAll`.

## Merge notes

- PR #36 (plan mode): let the plan populate `ScopeDecl` automatically.
- PR #54 (approval flow): feed `needs_approval` reasons into the approval
  card; this module stays the enforcement point.
- The frontend card mirrors `approval.js` styling; share CSS when both land.

# D5 - Checkpoints timeline + per-hunk diff (design)

Status: design only, for Z App's implementation. Verified against `origin/main` 2026-10-17.

## Research

**What exists today (verified on main):** nothing. Searched `core/` and `engine/src/` for
`checkpoint`/`shadow`/`undo`/`rewind` - zero matches. The engine registers the CLI's
`zen_tools::EditTool`/`WriteTool` directly (`engine/src/lib.rs:62-63`) with no snapshot hook.

**What the architecture already decided** (`docs/desktop-architecture.md:17`):
- `gix` - git status, checkpoint/undo.
- `similar` - per-hunk diff, accept/reject per part.

**Competitor evidence** (`docs/research/competitors-2026-10-17.md`):
- Cline Desktop: checkpoint snapshots (secondary source).
- Claude Code: rewind to earlier state.
- Gemini CLI 0.64: atomic writes (temp + rename).
- Planned (not built) in our repo: `core/fs/atomic.rs` + `core/checkpoint.rs` snapshotting
  each edited file into `.zencode/ckpt/<turn>/`, with `zencode undo [turn]`.

**Git-based vs file-copy trade-off:**

| | Git shadow repo (`gix`) | File copies (`.zencode/ckpt/`) |
|---|---|---|
| Storage | Deduplicated blobs; cheap for many checkpoints | Full copy per file per checkpoint; grows fast |
| Diff | Free (`git diff` between commits) | Needs `similar` + manual pairing |
| Binary files | Handled | Handled, but copies are large |
| Non-git workspaces | Works (shadow repo is separate) | Works |
| Complexity | Needs `gix` plumbing (init, add, commit) | Trivial `fs::copy` |
| Restore | `git checkout` of paths | Copy back |

Recommendation: **git shadow repo**. The architecture already chose `gix`, storage
deduplication matters for an agent that edits constantly, and diff/restore come for free.
File copies remain the fallback if `gix` proves too heavy.

## Ideas

**When to snapshot.** Before every mutating tool call (Write, Edit) - automatic, no user
action. Plus on-demand (`Checkpoint` command) and before Bash commands that look
destructive (heuristic: `rm -rf`, `git checkout .`, etc. - best-effort, documented as such).
Each snapshot records the triggering `tool_use_id` so the timeline can link a checkpoint
to the exact agent action that caused it.

**Where.** A shadow git repo at `<workspace>/.zencode/checkpoints/` (bare-ish working repo,
never pushed). One commit per checkpoint: message = JSON summary (tool name, files touched,
turn number). The user's own `.git` is never touched.

**Timeline UI data model.** The UI lists checkpoints newest-first with:
`id` (short SHA), `created_at`, `label` (e.g. "Before Edit src/main.rs"), `tool_use_id`,
`turn`, `files_changed` (count + names). Clicking a checkpoint shows the per-hunk diff.

**Per-hunk diff.** Use `similar` (already chosen) to compute line diffs between the
checkpoint commit and its parent (or working tree for "uncommitted changes" view).
Hunks map to tool calls via the `tool_use_id` in the commit message - one checkpoint =
one tool call, so mapping is 1:1, no guessing.

**Restore semantics.**
- **Full restore:** check out all files from the checkpoint commit into the workspace.
  Files created after the checkpoint are left alone (safer) but listed; user can delete.
- **Selective hunk revert:** apply reverse patches for chosen hunks only (via `similar`
  patch application). Refused hunks stay.
- Restore itself creates a new checkpoint first ("before restore"), so restore is undoable.

**Interaction with approvals.** If the D6 approval UI rejects a tool call, no checkpoint is
needed (nothing changed). If approved, the checkpoint is taken *before* execution.

## Results

**Checkpoint record schema** (stored as the git commit message, JSON):

```json
{
  "schema": 1,
  "id": "a1b2c3d",
  "created_at": "2026-10-17T09:00:00Z",
  "label": "Before Edit engine/src/lib.rs",
  "tool": "Edit",
  "tool_use_id": "toolu_abc123",
  "turn": 7,
  "session_id": "sess_xyz",
  "files": ["engine/src/lib.rs"]
}
```

**Core operations (spec):**

1. `checkpoint_create(trigger) -> Checkpoint`
   - Trigger: `{tool: "Edit"|"Write"|"Bash"|"manual"|"restore", tool_use_id?, turn, session_id}`.
   - Copies current workspace file states into the shadow repo and commits.
   - Skips files matching ignore rules (target/, .git/, node_modules/).
   - Returns the record above. Idempotent per `tool_use_id` (no double snapshot).

2. `checkpoint_list(session_id?, limit=50) -> [CheckpointSummary]`
   - Reads shadow repo log, newest first. Summary = record minus nothing (it's small).
   - Includes `files_changed` count derived from the commit diff stat.

3. `checkpoint_restore(id, {hunks?}) -> RestoreReport`
   - Creates a "before restore" checkpoint first.
   - Full: `gix` checkout of all paths in the checkpoint commit.
   - Selective: reverse-apply only the chosen hunks via `similar`.
   - Returns `{restored_files, skipped_new_files, new_checkpoint_id}`.

**Open questions for Z App:**
1. Shadow repo per workspace or per project? (Per workspace is simpler; per project shares history.)
2. Retention policy: keep all checkpoints, or prune (e.g. keep last 100, or 7 days)?
3. Should Bash-tool checkpoints be automatic (heuristic risk) or manual-only to avoid noise?

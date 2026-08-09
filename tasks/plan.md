# Implementation Plan: Automatic Coordination and Safe Integration

## Overview

Turn one plain-language goal into a planner-led Task with independently scoped AgentRuns, then synthesize every successful Run into one immutable review and a conflict-safe local merge. The user's opened checkout remains untouched during execution and on every failed integration; successful integration fast-forwards the clean target branch and preserves per-Run branches.

## Architecture Decisions

- The first Run is a planner that submits a bounded assignment plan through `WorkspaceControl`; SubShell validates and launches the assignments. No second orchestration engine or embedded model is added.
- A Task—not a terminal or branch—is the unit of planning, attention, review, approval, and merge.
- Review fingerprints full, untruncated Git patches plus Run order and evidence. UI patches may be truncated, but approval and merge never use truncated data.
- Approved Run snapshots become one commit per Run on `subshell/<task>/<run>` branches. Integration cherry-picks them in a temporary worktree, runs validations there, and only then fast-forwards the clean local target.
- Same-file/related-file flags inform review but never block or resolve changes automatically.

## Task List

### Foundation

- [x] Add migration 0008 for plans, review evidence, immutable snapshot metadata, and merge attempts.
- [x] Record the automatic-planning and exact-snapshot integration decision in ADR 0003 and update stable contracts.
- [x] Extend `GitService` with exact snapshot, isolated integration, ref creation, and cleanup primitives covered by temporary-repository tests.

### Automatic coordination

- [x] Add a bounded planner submission contract to `WorkspaceControl` with validated assignment DTOs.
- [x] Mark planner and executor Runs explicitly; after a successful planner submission, prepare focused contexts and launch assignments concurrently.
- [x] Update the built-in SubShell skill so planner Runs submit plans while executor Runs consume shared Task decisions and report concise evidence.
- [x] Replace the one-agent quick start with planner-led automatic coordination while retaining manual assignment controls.

### Review and integration

- [x] Generate/reuse immutable Task review attempts with per-Run exact changes, context manifests, validation/activity evidence, and fingerprints.
- [x] Produce deterministic same-file, related-file, and shared-text conflict flags with evidence.
- [x] Support approve and send-back decisions; reject stale or incomplete attempts.
- [x] Integrate the exact approved fingerprint in an owned worktree, preserve Run branches, validate, fast-forward the unchanged target, then archive and release owned resources.
- [x] Preserve target, checkout, logs, Run worktrees, and review evidence on stale/conflicting/failed integration.

### Product experience

- [x] Add a Task-level Review surface with exact combined diff, Run provenance, evidence, warnings, approve/send-back, and merge actions.
- [x] Show planner assignments and automatic execution state in the Task workspace.
- [x] Update product direction/specs to position SubShell as the local control plane that brings parallel agent work back together safely.

## Checkpoints

### Contract checkpoint

- Migration upgrade tests pass.
- Git fixtures prove exact snapshots and unchanged-target failures.
- Planner/review DTOs are stable before UI integration.

### Runtime checkpoint

- A submitted plan launches at least two isolated assignments without manual setup.
- Review generation is deterministic and rejects live/failed/stale inputs.
- Merge conflict and validation failure leave the target and opened checkout unchanged.

### Complete checkpoint

- An approved fanned-out Task reaches `Review → Approved → Merged → Archived` once.
- Per-Run branches and audit evidence remain after success.
- Frontend tests, Rust tests, web build, formatting, Clippy, and diff checks pass.

## Risks and Mitigations

| Risk | Impact | Mitigation |
|---|---|---|
| Planner emits unsafe or malformed assignments | High | Validate count, size, paths, and role at the control boundary; reject recursive planners. |
| Two Run patches conflict | High | Detect overlap during review; cherry-pick only in an integration worktree and never touch target on failure. |
| Target moves after review | High | Compare approved base/fingerprint and live target immediately before integration. |
| Validation mutates the user's checkout | High | Run commands only inside the owned integration worktree with bounded command lists. |
| Provider ignores the planning skill | Medium | Keep manual assignments available and surface the missing-plan state instead of guessing from terminal prose. |

## Open Questions Resolved

- Target users: solo power developers and small engineering teams.
- Primary pain: parallel agent execution and explicit context coordination across different CLIs.
- Desired outcome: one prompt autonomously becomes parallel work and one final human approval.
- Authority: implementation planning and assignment are automatic; final local integration remains human-approved.

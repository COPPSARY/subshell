# Repository Guidelines

## Product and Architecture

SubShell is a Tauri desktop app that coordinates AI coding CLIs around `Project → Task → AgentRun(s)`. It is a feature-oriented modular monolith, not a service system.

- `src/app/` owns composition and navigation only.
- `src/features/<feature>/` owns React UI, API client, model, public `index.ts`, and tests.
- `src/shared/ui/` contains small product-neutral primitives.
- `src-tauri/src/features/<feature>/` owns Rust commands and domain logic.
- `src-tauri/src/platform/` implements SQLite, Git, process, filesystem, and keychain ports.
- `src-tauri/src/contracts/` contains only stable cross-module DTO/error contracts.
- `src-tauri/migrations/` contains immutable `NNNN_name.sql` migrations.

Import another feature only through its public API. Keep product logic out of app composition and platform adapters. Do not create empty modules for hypothetical work.

## Parallel Development

Keep one coherent change per branch, optionally in a Git worktree: `git worktree add ../subshell-timeline -b feat/timeline-feed`. A work item should primarily modify one feature directory and its tests. Build against mocks when a producer is unfinished; merge the smallest contract and fixture first. Defer navigation, command registration, lockfile, and shared-config edits to a small integration commit.

For concurrent desktop runs, give each worktree a unique `SUBSHELL_DATA_DIR`. Avoid unrelated formatting or refactors, and never import another feature's private files.

## Commands

- `npm install` — install frontend/Tauri tooling.
- `npm run tauri dev` — run the desktop app.
- `npm test` — run frontend tests.
- `npm run build:web` — type-check and build the frontend.
- `npm run build` — compile the desktop executable without bundling it.
- `cargo test --manifest-path src-tauri/Cargo.toml` — run Rust tests.
- `cargo fmt --manifest-path src-tauri/Cargo.toml --check` — verify formatting.
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings` — reject Rust warnings.

## Style and Tests

Use two-space TypeScript/TSX indentation and `rustfmt`. Use `PascalCase` for components/types, `camelCase` for TypeScript functions, and `snake_case` for Rust. Name frontend tests `*.test.ts(x)`; keep Rust unit tests beside code. Use temporary repositories and stand-in provider executables—never real accounts or a contributor's working tree. Every bug fix needs a focused regression test.

## PR and Safety Rules

Use imperative commits such as `fix(merge): preserve worktree on conflict`. PRs list owned paths, contracts changed, verification commands, migrations, UI screenshots, and rollback notes. Store secrets only in the OS keychain. Preserve worktrees/logs on failure, avoid destructive Git cleanup, and require approval before creating user-visible branches.

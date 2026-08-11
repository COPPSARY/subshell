# Contributing to SubShell

Thank you for helping build SubShell. The project is optimized for small, independent changes that can be developed in parallel.

## Before You Start

Open or claim an issue before substantial work. State the feature area and paths you expect to own. For cross-module work, agree on the smallest interface and fixtures first so producers and consumers can proceed independently.

## Local Setup

```sh
npm install
npm run tauri dev
```

Use a feature branch such as `feat/timeline-filters` or `fix/merge-preflight`. A separate worktree is recommended for concurrent changes:

```sh
git worktree add ../subshell-timeline -b feat/timeline-filters
```

Set a unique `SUBSHELL_DATA_DIR` when multiple desktop instances may run.

## Where to Start

Start normal feature work in `src/features/<feature>/` for React and
`src-tauri/src/features/<feature>/` for Rust. Use the small health feature as
an end-to-end example, and add app wiring only after the feature works.

## Change Boundaries

- Keep feature code inside its frontend/backend feature directories.
- Import other features only through their public API.
- Build against mocks when another implementation is unfinished.
- Add one immutable numbered migration per schema change.
- Avoid unrelated formatting, dependency updates, or refactors.
- Keep app wiring, navigation, lockfile, and shared-config edits in a small integration commit.

## Verification

Run frontend tests/build and Rust test/fmt/clippy commands documented in the README. Automated tests must use temporary Git repositories and fake provider executables—never real accounts or a contributor's working directory.

Before integration, also verify the full coordination fixture (context sharing, approval decisions, conflict evidence, merge cleanup, and database reopen) through `cargo test --manifest-path src-tauri/Cargo.toml`, and build a host package with `npm run build:bundle`. CI repeats these checks from a clean checkout. UI changes need a keyboard-only pass at the minimum 900×600 window, a reduced-motion pass, and a screenshot in the pull request.

Do not put credentials in fixtures, environment snapshots, logs, screenshots, or issue text. Provider secrets belong only in the OS keychain; test credential paths use the in-memory secret store.

## Pull Requests

Use imperative commits such as `feat(timeline): add provider filter`. A PR should describe the user impact, owned paths, contracts changed, migration, tests run, screenshots for UI work, limitations, and rollback approach. Link the issue it addresses. Review feedback should stay within the PR's coherent scope.

By contributing, you agree that your work is licensed under the repository's Apache 2.0 License.

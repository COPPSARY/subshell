<div align="center">

# SubShell

**The desktop workspace that coordinates your AI coding agents.**

![Tauri](https://img.shields.io/badge/Tauri-2-24C8DB?logo=tauri&logoColor=white)
![Rust](https://img.shields.io/badge/Rust-000000?logo=rust&logoColor=white)
![React](https://img.shields.io/badge/React-19-61DAFB?logo=react&logoColor=111827)
![TypeScript](https://img.shields.io/badge/TypeScript-5-3178C6?logo=typescript&logoColor=white)
![SQLite](https://img.shields.io/badge/SQLite-3-003B57?logo=sqlite&logoColor=white)

</div>

## What is SubShell?

AI coding CLIs can work in separate Git worktrees and still duplicate effort, contradict decisions, or produce incompatible changes. SubShell is **the workspace that coordinates your AI agents**, sitting beside your IDE and organizing those tools around a single unit: the **Task**.

It is more than a terminal multiplexer: SubShell isolates each run, packages focused context, routes your attention to blocked work, and brings every agent's exact changes into one human-controlled review and merge flow.

```text
Plan → Assign → Run → Observe → Review → Merge
```

## Getting Started

Install Node.js 20+, Rust, and the [Tauri system prerequisites](https://v2.tauri.app/start/prerequisites/), then run:

```sh
npm install
npm run tauri dev
```

For concurrent worktrees, isolate local state:

```sh
SUBSHELL_DATA_DIR=/tmp/subshell-my-feature npm run tauri dev
```

## Core Workflow

1. Open a Git repository from **Projects**, then enter a goal.
2. Preview the planner's context and isolated environment before starting it.
3. Approve the proposed assignments; ready agents run in separate worktrees with separate provider config roots and localhost ports.
4. Follow output in **Tasks**, approvals and failures in **Timeline**, and share a previewed summary, file, or output excerpt between live agents when needed.
5. Finish the runs, inspect the exact combined diff and conflict evidence, optionally launch the combined app preview, then approve and merge the immutable review snapshot.

The merge is all-or-nothing. SubShell validates in its own integration worktree, advances the selected target only on success, archives the Task, and cleans owned worktrees while preserving per-run branches.

## Providers and Secrets

Open **Providers** to detect Claude Code, Codex, Kiro, or Gemini, or add a custom executable. Native accounts use an isolated config directory by default. Copying an existing user config requires an explicit path; inheriting the whole user home is a separate full-access choice.

Account credentials are stored in the operating-system keychain, injected only into that account's child process, and redacted from captured output. They are never stored in SQLite. Re-authenticate an account after an authentication failure; remove credentials before deleting an account that still has one.

Automated tests use stand-in executables. Real provider checks are manual and should never run against contributor accounts in CI.

## Keyboard and Recovery

- Press `Ctrl+K` or `Cmd+K` to open the command palette. Arrow keys select, `Enter` runs a command, and `Escape` closes and restores focus.
- All task, approval, review, and merge controls are normal keyboard-focusable controls. A skip link appears when focus enters the window.
- Closing SubShell with active runs asks whether to keep them supervised, stop them safely, or cancel. After a restart, live supervised runs remain attached; a lost process is recorded as failed without deleting its log, worktree, or provider session, so supported providers can resume it.

## Preview and Troubleshooting

The Review screen combines completed run worktrees in merge order without changing the opened checkout. **Preview** shows the exact command before first execution, allocates a unique localhost port, and provides logs plus Restart and Stop controls. Plain HTML/CSS/JavaScript repositories use the built-in static server; detected package or Cargo projects use their normal development command. A conflict blocks only the combined preview and identifies the conflicting files; individual run previews remain available.

When an action fails, follow the preserved-state and next-step text in the error panel. Common recovery paths are:

- **Backend unavailable:** restart the desktop app; repository files are untouched.
- **Provider missing or unauthenticated:** install/configure it in Providers, then retry or resume.
- **Target branch drifted or validation failed:** inspect the review evidence, restore a clean target, and create a fresh review snapshot.
- **Preview failed:** inspect Server logs, correct the project command, and Restart. Closing Preview stops its process and removes only SubShell's temporary preview files.

Run checks before opening a pull request:

```sh
npm test
npm run build:web
npm run build
npm run build:bundle
cargo test --manifest-path src-tauri/Cargo.toml
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings
```

`npm run build` compiles the desktop executable without packaging. `npm run build:bundle` produces the host platform's distributable packages under `src-tauri/target/release/bundle/`.

## Contributing

Contributions are welcome. Start with [CONTRIBUTING.md](CONTRIBUTING.md) and keep pull requests small, feature-focused, independently tested, and easy to revert. Security issues should follow [SECURITY.md](SECURITY.md), not public issue reports.

## License

SubShell is available under the [Apache 2.0 License](LICENSE).

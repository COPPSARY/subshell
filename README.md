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

Run checks before opening a pull request:

```sh
npm test
npm run build:web
npm run build
cargo test --manifest-path src-tauri/Cargo.toml
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings
```

## Contributing

Contributions are welcome. Start with [CONTRIBUTING.md](CONTRIBUTING.md) and keep pull requests small, feature-focused, independently tested, and easy to revert. Security issues should follow [SECURITY.md](SECURITY.md), not public issue reports.

## License

SubShell is available under the [Apache 2.0 License](LICENSE).

# Security Policy

## Supported Versions

SubShell is pre-release. Security fixes are applied to the latest code on the default branch only.

## Reporting a Vulnerability

Do not open a public issue for suspected vulnerabilities. Use GitHub's private security advisory feature for this repository. Include affected versions, reproduction steps, impact, and any suggested mitigation. Avoid including real provider credentials, tokens, private repositories, or raw logs containing sensitive data.

You should receive an acknowledgement within seven days. Maintainers will validate the report, coordinate a fix and disclosure timeline, and credit reporters who want attribution.

## Security Expectations

Secrets belong in the OS keychain, never SQLite, logs, command arguments, fixtures, screenshots, or frontend state. Tests use fake credentials and temporary repositories. Do not weaken worktree ownership checks, branch approval, path validation, process identity verification, or merge preflight to simplify a change.


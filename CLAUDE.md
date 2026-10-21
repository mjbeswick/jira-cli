# Jira CLI Project Guide

## Project overview

This repository builds a non-interactive Rust Jira CLI named `jira`. It uses
Clap for command parsing, blocking reqwest for Jira REST calls, and serde_json
for structured results. The npm/JavaScript implementation is retired; Cargo is
the application build and test system.

## Source layout

- `src/main.rs`: CLI commands, validation, output, and issue workflows.
- `src/client.rs`: authenticated Jira REST client, API-version fallback,
  attachment downloads, Markdown/ADF conversion, and client contract tests.
- `src/config.rs`: secure profile storage, environment overrides, and migration
  of existing Jira CLI config.
- `src/skills.rs`: bundled skill metadata, local skill discovery, display, and
  installation for Claude Code, OpenCode, and `.agents` hosts.
- `plugins/jira/skills/jira/SKILL.md`: the single canonical agent skill,
  embedded into the binary at compile time.
- `.github/workflows/ci.yml`: formatting, Clippy, tests, and release build.

## Agent skill discovery

Keep the canonical bundled skill accurate to the actual Rust CLI flags and
examples. Validate changes with:

```sh
jira --json skills list
jira skills show jira
jira skills install --target opencode
```

The Claude Code plugin is an additional distribution path and must continue to
point to the same skill source. Do not add a duplicate generated skill document.

## Development and verification

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --release --locked
```

Tests use local HTTP fixtures. Do not expose credentials or invoke live Jira
through an unapproved route. Jira writes must remain single-shot; never add
automatic retries to a write operation.

## Releases

Use Conventional Commits. Rust releases are created by pushing a `v*` tag that
matches `Cargo.toml`'s package version. CI runs format, lint, and test checks,
builds the Linux x86_64 binary, and attaches it to a GitHub Release. See
`AGENTS.md` for the manual release procedure.

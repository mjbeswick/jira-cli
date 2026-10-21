# Contributing to Jira CLI

Contributions are welcome through GitHub issues and pull requests. The project is
a Rust CLI; do not add an application dependency on JavaScript/Node tooling.

## Development

Install a stable Rust toolchain with `rustfmt` and `clippy`. Before submitting a
pull request, run:

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --release --locked
```

Tests use local Jira HTTP fixtures; live Jira credentials should not be required.
Never add secrets to fixtures, logs, documentation, or pull requests.

## Code and CLI changes

- Keep command definitions in `src/main.rs` and HTTP behavior in `src/client.rs`.
- Preserve safe, single-shot write semantics and Jira-origin checks for
  attachment downloads.
- Add regression tests for new behavior, including failure and permission cases.
- Keep `plugins/jira/skills/jira/SKILL.md` accurate to the command surface; it
  is embedded into the binary and is the canonical skill content.
- Maintain `jira skills list`, `jira skills show`, and `jira skills install`
  behavior so agents can discover the bundled skill and install it in supported
  agent directories.
- Update the README and `docs/API.md` when the CLI or skill interface changes.

## Commit messages

Use Conventional Commits (for example, `feat(attachments): download issue
images`, `fix(config): preserve active profile`, or `docs: clarify skill setup`).
Release versions are taken from `Cargo.toml` and published through version tags;
see `AGENTS.md`.

## Pull requests

Describe the user-visible behavior, risks, and verification performed. Report
checks that were not run rather than implying they passed. Include related issue
references when applicable.

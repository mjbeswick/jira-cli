# Release playbook

## Version and release process

The Rust package version in `Cargo.toml` is authoritative. Releases are created
by pushing a version tag matching the package, for example `v0.1.0`. The GitHub
release workflow verifies that the tag and manifest versions match, runs the
Rust quality checks, builds the release binary, and attaches the Linux x86_64
archive to the GitHub Release.

Before tagging a release:

1. Run `cargo fmt --all -- --check`.
2. Run `cargo clippy --locked --all-targets -- -D warnings`.
3. Run `cargo test --locked`.
4. Run `cargo build --release --locked`.
5. Update the `version` in `Cargo.toml` and refresh `Cargo.lock` if needed.
6. Commit the version change using Conventional Commit format.
7. Create and push the matching `vX.Y.Z` tag through the approved Git workflow.

Do not publish an npm package. The JavaScript CLI distribution is retired.

## Safe Jira behavior

- Keep Jira writes single-shot; never add automatic retry/replay for writes.
- Attachment content downloads must validate the Jira origin before sending
  configured credentials.
- Do not expose credentials or use a live Jira tenant outside the approved
  Jira-agent workflow.
- A local fixture test is not evidence of live Jira compatibility; report the
  distinction clearly.

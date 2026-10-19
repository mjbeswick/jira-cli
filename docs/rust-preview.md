# Rust CLI preview

This is an **opt-in preview**, not a replacement for the published npm `jira` executable. The existing JavaScript CLI and its release workflow remain intact while the Rust CLI reaches parity. No Jira server or credentials are needed to build the binary.

## Install locally

```sh
sh scripts/install-rust.sh
~/.local/share/jira-cli-rust/bin/jira --help
```

The install script builds from this checked-out repository using its locked Rust dependencies and installs a binary named `jira` into `${JIRA_RUST_INSTALL_ROOT:-$HOME/.local/share/jira-cli-rust}/bin`. The default directory is not on PATH, so the installed npm `jira` command remains active. It does not download and run a remote installer or publish a release. Alternatively use `cargo run -- --help` during development.

## Agent-oriented workflow

```sh
export JIRA_HOST=https://jira.example.com
export JIRA_API_TOKEN=... # supply using your usual secure environment setup
cargo run -- --json issue create-fields --project EX --type Bug
cargo run -- --json issue create --project EX --type Bug --summary 'Checkout blocked' \
  --fields-file ./fields.json --dry-run
# Review and explicitly approve the exact draft before removing --dry-run.
cargo run -- --json issue create --project EX --type Bug --summary 'Checkout blocked' \
  --fields-file ./fields.json
cargo run -- --json issue view EX-123
```

`fields.json` must be an object keyed by IDs discovered for the selected project and type, for example `{"customfield_10010":"Impact text","customfield_10011":{"id":"2"}}`. Example IDs and values are **not** valid for every Jira instance. Required fields, select options and common schema shapes are checked before posting. Cloud textarea strings become ADF; explicit API v2 uses text. `--json` emits a single JSON value on stdout and errors on stderr. Never log tokens, cookies, full config files or issue fields containing sensitive data.

Configure a profile using the full-path Rust `jira config set server URL`, `jira config set authType bearer`, and environment-provided `JIRA_API_TOKEN`; `jira config show` redacts secrets. The Rust preview reads the existing `~/.jira-cli/config.json` multi-profile format, and `--profile`/`JIRA_PROFILE` select a named profile. Use `JIRA_API_VERSION=2` for Data Center if automatic create-metadata discovery is unavailable. All writes are single-shot: after an ambiguous response, search before retrying to avoid duplicate issues.

## Deliberate preview limitations

The Rust CLI supports core issue, comment, remote-link, project, sprint, config, and profile workflows with redesigned flags and JSON output. `issue list` follows v3 page tokens or v2 offsets up to its requested limit. `install-skill` embeds a separate Rust agent skill; the npm plugin remains separate. Some terminal table styling and uncommon ADF markup differ from npm. The legacy single-profile config is migrated on first read. Eligible read-only v3 calls can fall back to v2; writes never replay. Live Rust-to-Jira verification is blocked by this workspace's Jira specialist policy, which permits only the pinned npm CLI. The npm executable and GitHub fork remain intact until that gate is resolved.

Generate Bash, Elvish, Fish, PowerShell, or Zsh completions with `jira completions <shell>`. The generated scripts use the Rust command definitions and do not access Jira. The Rust `install-skill` command defaults to `.claude/skills/jira-rust-candidate`, distinct from the npm skill's `.claude/skills/jira` directory.

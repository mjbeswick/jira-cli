# Jira CLI (Rust candidate)

A non-interactive Jira command-line client built in Rust. Designed for automation: explicit flags, structured JSON output, project-specific field discovery, single-shot writes, and preflight issue creation. This repository still contains the published npm/JavaScript CLI; **the Rust binary is not yet the active `jira` command on PATH**. The local installer places the Rust binary named `jira` in an isolated directory until live Rust-to-Jira verification and a deliberate cutover are complete.

## Build and install the candidate

Requires Rust and Cargo. From this checkout:

```sh
sh scripts/install-rust.sh
~/.local/share/jira-cli-rust/bin/jira --help
```

The script compiles this checkout with `cargo build --release --locked` and installs `${JIRA_RUST_INSTALL_ROOT:-$HOME/.local/share/jira-cli-rust}/bin/jira`. It neither runs a remote installer nor replaces the npm-managed `jira` executable on PATH. Use the full path above to inspect this candidate; do not add its directory to PATH until the Jira specialist workflow and verification gate have been updated. During development use `cargo run -- --help` or `cargo run -- --json issue list --project PROJ` where policy permits.

> For the currently published npm CLI, see [legacy JavaScript documentation](docs/legacy-javascript-cli.md). Its command flags differ from the Rust CLI. [Rust preview status and limitations](docs/rust-preview.md) describe what remains before cutover.

## Command reference

Use the full-path Rust `jira --help` and `<command> --help` for exact options. Root help shows the Cargo package version and a short read-before-write example; `jira --version` prints just the version. Root flags `--profile <name>` and `--json` work before or after a subcommand. With `--json`, a successful command prints one JSON document to stdout; argument or runtime failures print JSON to stderr and exit nonzero. Without `--json`, results are pretty-printed JSON (or Markdown when explicitly requested).

| Group | Commands |
|---|---|
| `config` | `show`, `set <key> <value>`, `unset <key>`, `test` |
| `profile` | `list`, `add`, `use`, `remove` |
| `issue` | `list`, `view`, `create-fields`, `create`, `edit`, `delete`, `transitions`, `transition` |
| `issue comment` | `list`, `add`, `edit`, `delete` |
| `issue remote-link` | `list`, `add`, `update`, `delete` |
| `project` | `list`, `view`, `components`, `versions` |
| `sprint` | `boards`, `list`, `active` |
| `install-skill` | Copy the packaged **Rust** agent skill to a chosen directory |
| `completions` | Generate shell completions for Bash, Elvish, Fish, PowerShell, or Zsh |

```sh
~/.local/share/jira-cli-rust/bin/jira --json issue list --project PROJ --limit 20
~/.local/share/jira-cli-rust/bin/jira --json issue view PROJ-123
~/.local/share/jira-cli-rust/bin/jira issue view PROJ-123 --format markdown --output ./issue.md
~/.local/share/jira-cli-rust/bin/jira --json issue transitions PROJ-123
~/.local/share/jira-cli-rust/bin/jira --json sprint boards
~/.local/share/jira-cli-rust/bin/jira --json sprint list --board 10636 --state active
```

### Shell completions

Generate completions from the Rust candidate's command definitions and install them for your shell. These scripts complete command names and flags; they do not query Jira for issue, project, or profile values.

```sh
# Bash (current shell)
source <(~/.local/share/jira-cli-rust/bin/jira completions bash)

# Zsh (persist in a completion directory on fpath)
mkdir -p ~/.zfunc
~/.local/share/jira-cli-rust/bin/jira completions zsh > ~/.zfunc/_jira
# Add `fpath=(~/.zfunc $fpath); autoload -Uz compinit; compinit` to ~/.zshrc

# Fish (persist for future shells)
mkdir -p ~/.config/fish/completions
~/.local/share/jira-cli-rust/bin/jira completions fish > ~/.config/fish/completions/jira.fish
```

PowerShell and Elvish scripts are available with `jira completions powershell` and `jira completions elvish`; follow the shell's normal profile or completion-script setup to load them.

`issue list` supports project, assignee, reporter, status, type, priority, created, updated, and `--jql` filters. User-supplied structured values are escaped for JQL. Cloud search follows page tokens; Data Center search follows offsets, up to `--limit`. `issue view --json` requests **all** fields, including potentially sensitive custom fields; avoid indiscriminate logging.

## Configuration and credentials

The CLI reads and writes `~/.jira-cli/config.json`, compatible with the npm multi-profile store; `JIRA_CLI_CONFIG_DIR` changes its directory. An older single-profile config is migrated to the default profile on first use without removing the source. On Unix, new config files and directories have restrictive permissions. `config show` and `profile list` redact tokens and cookies.

```sh
# Prefer your approved secret-management process for credentials:
export JIRA_HOST=https://jira.example.com
export JIRA_API_TOKEN=...  # obtain securely; do not paste into logs
~/.local/share/jira-cli-rust/bin/jira config test

# Or configure a named profile:
~/.local/share/jira-cli-rust/bin/jira profile add work --server https://jira.example.com --auth-type bearer
~/.local/share/jira-cli-rust/bin/jira --profile work config show
```

The CLI accepts `JIRA_HOST` (or legacy `JIRA_DOMAIN`), `JIRA_USERNAME`, `JIRA_API_TOKEN`, `JIRA_CLOUD_ID`, `JIRA_COOKIE`, `JIRA_AUTH_TYPE`, `JIRA_API_VERSION`, `JIRA_PROFILE`, and `JIRA_TLS_CLIENT_CERT`, `JIRA_TLS_CLIENT_KEY`, `JIRA_TLS_CA_CERT`. Explicit `--profile` overrides `JIRA_PROFILE`, which overrides the active profile. Authentication modes: Basic (username + token), Bearer (token), cookie, and mTLS (certificate + PKCS#8 PEM key; optional CA). A Cloud ID routes requests through Atlassian's API gateway. `JIRA_API_VERSION=auto` uses v3 with eligible **read-only** fallback to v2; pinned `2` or `3` is supported. The CLI never retries or replays a write because its outcome could be ambiguous.

## Agent-oriented issue creation

Do not infer custom field IDs from example values or another Jira project. Discover the actual create screen for the target project and type:

```sh
~/.local/share/jira-cli-rust/bin/jira --json issue create-fields --project PROJ --type Task
```

Prepare additional fields as JSON keyed by the returned IDs. This **illustrative** file must be updated to use the actual field IDs and allowed values:

```json
{
  "customfield_10010": "Customer checkout impact",
  "customfield_10011": { "id": "2" },
  "components": [{ "id": "35673" }]
}
```

```sh
~/.local/share/jira-cli-rust/bin/jira --json issue create --project PROJ --type Task \
  --summary "Checkout investigation" --description-file ./description.md \
  --fields-file ./fields.json --dry-run
```

`--dry-run` reads Jira metadata and displays the exact locally validated request **without creating an issue**. Review the full proposed draft and get explicit approval before removing `--dry-run`. The CLI checks the create screen, required fields, known value shapes, and available options before submitting **one** POST. Jira validators can require fields that create metadata calls optional; an approved create can still fail. If a response is ambiguous, search for an existing issue **before** retrying. Read back a created issue to verify its fields.

Create flags also support `--assignee` (Cloud account ID or Data Center username), `--priority`, `--labels`, `--parent`, and `--due-date`; `--fields-file` covers project-specific fields. API v3 textarea strings become Atlassian Document Format, while API v2 uses plain text. For edits, use `issue edit <key> --fields-file ./changes.json` to validate against that issue's edit screen before writing.

Comments can be supplied with text or `--file`. `issue comment add <key> --internal --file ./note.txt` uses Jira Service Management's request-comment API and **fails rather than posting publicly** if `public: false` is not confirmed. Transitions are checked against the issue's available transitions before writing. `--force` is required for deletion but never substitutes for user approval.

## Development and release gate

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --release --locked
```

Rust tests use local simulated HTTP responses for custom fields, pagination, auth errors, and single-shot writes. The existing npm CLI has separate Jest/ESLint checks in CI. **A live Rust-to-Jira check has not been completed**: this workspace's Jira specialist is pinned to the installed npm CLI and cannot execute an alternate binary. Local tests and a ticket created through npm do not establish live Rust compatibility. Do not replace the globally installed `jira` command, remove the npm distribution, or publish the Rust CLI as a verified replacement until that policy/verification gate is resolved.

This project has not deleted or recreated `mjbeswick/jira-cli`. Repository replacement is separate from building and testing the CLI and needs exact, fresh confirmation before any irreversible action.

# Jira CLI

A non-interactive Jira command-line client built in Rust. Designed for people and automation: explicit flags, structured JSON output, project-specific field discovery, single-shot writes, preflight issue creation, and agent skill discovery.

## Install

Requires Rust and Cargo. From this checkout:

```sh
cargo install --path . --locked --force
jira --help
```

This installs or updates the Rust binary as `jira` in Cargo's bin directory (normally
`~/.cargo/bin`). Ensure that directory is on `PATH`. To build without installing,
use `cargo run -- --help`.

## Command reference

Use the full-path Rust `jira --help` and `<command> --help` for exact options. Root help shows the Cargo package version and a short read-before-write example; `jira --version` prints just the version. Root flags `--profile <name>` and `--json` work before or after a subcommand. With `--json`, a successful command prints one JSON document to stdout; argument or runtime failures print JSON to stderr and exit nonzero. Without `--json`, results are pretty-printed JSON (or Markdown when explicitly requested).

| Group | Commands |
|---|---|
| `config` | `show`, `set <key> <value>`, `unset <key>`, `test` |
| `profile` | `list`, `add`, `use`, `remove` |
| `issue` | `list`, `view`, `create-fields`, `create`, `edit`, `delete`, `flag`, `unflag`, `transitions`, `transition`, `attachment list`, `attachment download` |
| `issue comment` | `list`, `add`, `edit`, `delete` |
| `issue remote-link` | `list`, `add`, `update`, `delete` |
| `project` | `list`, `view`, `components`, `versions` |
| `sprint` | `boards`, `list`, `active` |
| `skills` | `list`, `show <name>`, `install [--target claude|opencode|agents] [--force]` |
| `install-skill` | Copy the bundled Jira skill to an explicit directory |
| `completions` | Generate shell completions for Bash, Elvish, Fish, PowerShell, or Zsh |

```sh
jira --json issue list --project PROJ --limit 20
jira --json issue view PROJ-123
jira issue view PROJ-123 --format markdown --output ./issue.md
jira --json issue transitions PROJ-123
jira --json issue attachment list PROJ-123
jira issue attachment download PROJ-123 10050 --output ./image.png
jira --json skills list
jira skills show jira
jira skills install
```

`skills list` shows the bundled Jira skill and skills found in common project
and home directories used by Claude Code, OpenCode, and other agents. `skills
show jira` prints the bundled instructions. `skills install` installs the bundled
skill for the supported agent hosts; use `--target` to select one. Discovery
depends on the host scanning the corresponding directory, so listing a skill
does not guarantee a particular agent has loaded it.

### Shell completions

Generate completions from the Rust command definitions and install them for your shell. These scripts complete command names and flags; they do not query Jira for issue, project, or profile values.

```sh
# Bash (current shell)
source <(jira completions bash)

# Zsh (persist in a completion directory on fpath)
mkdir -p ~/.zfunc
jira completions zsh > ~/.zfunc/_jira
# Add `fpath=(~/.zfunc $fpath); autoload -Uz compinit; compinit` to ~/.zshrc

# Fish (persist for future shells)
mkdir -p ~/.config/fish/completions
jira completions fish > ~/.config/fish/completions/jira.fish
```

PowerShell and Elvish scripts are available with `jira completions powershell` and `jira completions elvish`; follow the shell's normal profile or completion-script setup to load them.

`issue list` supports project, assignee, reporter, status, type, priority, created, updated, and `--jql` filters. User-supplied structured values are escaped for JQL. Cloud search follows page tokens; Data Center search follows offsets, up to `--limit`. `issue view --json` requests **all** fields, including potentially sensitive custom fields; avoid indiscriminate logging.

For Jira API v3, `issue create`, `issue edit`, public `issue comment add/edit`,
and textarea values in `--fields-file` convert Markdown to Atlassian Document
Format (ADF). `issue view --format markdown` converts the returned description
back to Markdown, and `issue comment list --markdown` converts ADF comment bodies
to Markdown. Public comment edits preserve rich formatting. Supported formatting
includes paragraphs, headings, emphasis,
links, lists, blockquotes, code, and thematic breaks. Jira Service Management
internal notes and API v2 use plain text; Markdown syntax is stripped on those
paths while preserving visible text and link destinations.

## Configuration and credentials

The CLI reads and writes `~/.jira-cli/config.json`; `JIRA_CLI_CONFIG_DIR` changes its directory. Existing multi-profile configurations are preserved, and an older single-profile config is migrated to the default profile on first use without removing the source. On Unix, new config files and directories have restrictive permissions. `config show` and `profile list` redact tokens and cookies.

```sh
# Prefer your approved secret-management process for credentials:
export JIRA_HOST=https://jira.example.com
export JIRA_API_TOKEN=...  # obtain securely; do not paste into logs
jira config test

# Or configure a named profile:
jira profile add work --server https://jira.example.com --auth-type bearer
jira --profile work config show
```

The CLI accepts `JIRA_HOST` (or legacy `JIRA_DOMAIN`), `JIRA_USERNAME`, `JIRA_API_TOKEN`, `JIRA_CLOUD_ID`, `JIRA_COOKIE`, `JIRA_AUTH_TYPE`, `JIRA_API_VERSION`, `JIRA_PROFILE`, and `JIRA_TLS_CLIENT_CERT`, `JIRA_TLS_CLIENT_KEY`, `JIRA_TLS_CA_CERT`. Explicit `--profile` overrides `JIRA_PROFILE`, which overrides the active profile. Authentication modes: Basic (username + token), Bearer (token), cookie, and mTLS (certificate + PKCS#8 PEM key; optional CA). A Cloud ID routes requests through Atlassian's API gateway. `JIRA_API_VERSION=auto` uses v3 with eligible **read-only** fallback to v2; pinned `2` or `3` is supported. The CLI never retries or replays a write because its outcome could be ambiguous.

## Agent-oriented issue creation

Do not infer custom field IDs from example values or another Jira project. Discover the actual create screen for the target project and type:

```sh
jira --json issue create-fields --project PROJ --type Task
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
jira --json issue create --project PROJ --type Task \
  --summary "Checkout investigation" --description-file ./description.md \
  --fields-file ./fields.json --dry-run
```

`--dry-run` reads Jira metadata and displays the exact locally validated request **without creating an issue**. Review the full proposed draft and get explicit approval before removing `--dry-run`. The CLI checks the create screen, required fields, known value shapes, and available options before submitting **one** POST. Jira validators can require fields that create metadata calls optional; an approved create can still fail. If a response is ambiguous, search for an existing issue **before** retrying. Read back a created issue to verify its fields.

Create flags also support `--assignee` (Cloud account ID or Data Center username), `--priority`, `--labels`, `--parent`, and `--due-date`; `--fields-file` covers project-specific fields. API v3 textarea strings become Atlassian Document Format, while API v2 uses plain text. For edits, use `issue edit <key> --fields-file ./changes.json` to validate against that issue's edit screen before writing.

Comments can be supplied with text or `--file`. `issue comment add <key> --internal --file ./note.txt` uses Jira Service Management's request-comment API and **fails rather than posting publicly** if `public: false` is not confirmed. Transitions are checked against the issue's available transitions before writing. `issue flag` and `issue unflag` inspect editable field metadata and only update the Jira Software Flagged field. `--force` is required for deletion but never substitutes for user approval.

## Development and release

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --release --locked
```

Tests use local simulated HTTP responses for pagination, attachment retrieval,
auth errors, and single-shot writes. Releases are built from the Rust package;
see the release workflow and `AGENTS.md` for the current versioning process.

---
name: jira
description: Use the Rust jira CLI to inspect Jira issues, projects, sprints, comments, remote links, and attachments, and prepare approved issue changes.
---

# Jira CLI skill

Use the Rust `jira` executable. Ask the CLI for current command help when flags
are unclear; do not substitute another Jira client. `jira skills list` discovers
bundled and locally installed skills, `jira skills show jira` prints this skill,
and `jira skills install` installs it for Claude Code, OpenCode, and `.agents`
hosts. Use `--target claude|opencode|agents` to choose one host and `--force` to
replace an existing copy. `jira install-skill --dest <dir>` remains available
for an explicit destination.

Use `--json` for machine-readable output. It works before or after subcommands.
Never log tokens, cookies, full configuration files, or issue data that contains
sensitive information.

## Setup and profiles

Configure credentials through the established secure environment or local
profile configuration. `jira config show` redacts secrets; `jira config test`
checks connectivity without printing account details.

```sh
jira config set server https://your-site.atlassian.net
jira config set authType bearer
# Supply JIRA_API_TOKEN through your approved secret-management mechanism.
jira config test
jira profile list
```

Use `--profile <name>` or `JIRA_PROFILE` to select a profile. Supported
authentication includes Basic, Bearer, cookie, and mTLS; see `jira profile add
--help` and the project README for the supported settings.

## Read issues and images

```sh
jira --json issue view TILLUI2-4695
jira issue view TILLUI2-4695 --format markdown --output ./TILLUI2-4695.md
jira --json issue attachment list TILLUI2-4695
jira issue attachment download TILLUI2-4695 <attachment-id> --output ./example.png
```

Issue JSON includes Jira's returned fields. Text/Markdown renders ADF media as
`[media]`; list attachments to find their Jira attachment IDs, then download
the file. Open the downloaded image with an image-capable local tool to inspect
its contents. Downloading is read-only and does not change Jira. If an attachment
URL is not on the configured Jira origin, the CLI refuses the download rather
than sending credentials to another host.

Useful read commands:

```sh
jira --json issue list --project PROJ --limit 20
jira --json issue transitions PROJ-123
jira --json issue comment list PROJ-123
jira --json issue remote-link list PROJ-123
jira --json project view PROJ
jira --json project components PROJ
jira --json sprint boards
jira --json sprint list --board 12 --state active
```

For structured comments use the global `--json` flag; Rust does not accept the
npm CLI's `--format json` option. Listing comments returns Jira's comment object
including IDs and ADF bodies:

```sh
jira --json issue comment list TILLUI2-4695
```

Public comments accept Markdown either as a positional argument or a UTF-8 file.
Use a file for multiline content to avoid shell-quoting problems:

```sh
jira issue comment add PROJ-123 --file ./comment.md
```

On an HTTP error, the CLI reports Jira's `errorMessages` and field-level
`errors` when Jira supplies them, without logging authentication headers or
credentials. Error response bodies are not retained for later retrieval, so
capture the error output from the failing command itself.

## Create and update safely

Before creating, check for duplicates and discover project/type-specific fields:

```sh
jira --json issue create-fields --project PROJ --type Bug
jira --json issue create --project PROJ --type Bug --summary 'Example' \
  --fields-file ./fields.json --dry-run
```

Review and obtain explicit user approval for the exact fields before removing
`--dry-run`. The preview makes metadata reads only. Writes are single-shot; after
an ambiguous response, search Jira before retrying. Jira may require fields not
marked required in create metadata.

Edits, comments, transitions, flags, and deletion are mutations. Perform one only
when the user explicitly requested it and approved the exact issue and payload.
Read the current state first. `--force` is an additional CLI safeguard, not user
authorization. For an internal JSM note, use `issue comment add <key> --internal`
and never fall back to a public comment if the internal request fails.

```sh
jira issue edit PROJ-123 --summary 'Approved summary'
jira issue transition PROJ-123 --to 'In Progress'
jira issue flag PROJ-123
jira issue comment add PROJ-123 --internal --file ./note.txt
jira issue delete PROJ-123 --force
```

## Help and shell completion

```sh
jira --help
jira issue attachment --help
jira completions zsh
```

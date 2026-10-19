---
name: jira-rust-candidate
description: Read Jira with the Rust CLI and prepare exact issue drafts for user-approved mutations.
---

# Jira Rust CLI

This skill documents the Rust binary named `jira`, installed by default at
`~/.local/share/jira-cli-rust/bin/jira`. The separately installed npm command
on PATH is also named `jira` but has different options. Use the explicit Rust
path only when the active Jira policy permits it; never silently switch clients.
Policies that pin an executable or require a Jira specialist take precedence.

The Rust candidate's `install-skill` command installs this separate skill to
`.claude/skills/jira-rust-candidate` by default. The npm CLI has a different
skill and command syntax; do not install this skill over `.claude/skills/jira`.

## Shell completions

The candidate generates static completions for Bash, Elvish, Fish, PowerShell,
and Zsh from its Rust command definitions. Completion generation is local and
does not require Jira configuration or contact Jira. It completes command and
option names, not live issue/project/profile values.

```sh
# Bash, current shell
source <(~/.local/share/jira-cli-rust/bin/jira completions bash)

# Zsh, persistent (ensure ~/.zfunc is on fpath in ~/.zshrc)
mkdir -p ~/.zfunc
~/.local/share/jira-cli-rust/bin/jira completions zsh > ~/.zfunc/_jira

# Fish, persistent
mkdir -p ~/.config/fish/completions
~/.local/share/jira-cli-rust/bin/jira completions fish > ~/.config/fish/completions/jira.fish
```

PowerShell and Elvish scripts can be generated with `jira completions powershell`
and `jira completions elvish`; load them using the shell's normal completion
setup.

## Reads

```sh
~/.local/share/jira-cli-rust/bin/jira --help
~/.local/share/jira-cli-rust/bin/jira --json issue list --project PROJ --limit 20
~/.local/share/jira-cli-rust/bin/jira --json issue view PROJ-123
~/.local/share/jira-cli-rust/bin/jira --json issue transitions PROJ-123
~/.local/share/jira-cli-rust/bin/jira --json issue create-fields --project PROJ --type Task
```

Use `--json` for automation. A JSON issue view requests all Jira fields; avoid
recording sensitive results unnecessarily. `--profile NAME` selects a configured
profile; `JIRA_PROFILE` does the same. The full-path Rust `jira config show`
redacts credentials.
Never output or persist tokens or cookies in task logs.

## Issue creation

1. Establish project/type and search for duplicates.
2. Discover fields with `issue create-fields` and use only IDs and option values
   from that project/type. Metadata can be incomplete; Jira may still reject a
   create, so do not assume an apparently optional field is unnecessary.
3. Put additional fields in a UTF-8 JSON object keyed by Jira field IDs. Preview:

```sh
~/.local/share/jira-cli-rust/bin/jira --json issue create --project PROJ --type Task \
  --summary 'Proposed summary' --fields-file ./fields.json --dry-run
```

4. Show the exact draft to the user and obtain explicit approval before removing
   `--dry-run`. A general request to create is not approval of the final fields.
   The preview only validates the client-side payload; it makes metadata reads.
5. On an ambiguous create result, search for an existing issue before retrying.
   Writes are single-shot; an error is not proof that nothing was created.
6. Read back the created issue and report the key and verified fields.

Edits, comments, transitions, flagging/unflagging, and deletions also need fresh
explicit approval of their exact payload and target. `issue flag <key>` and
`issue unflag <key>` inspect edit metadata and update only the Jira Software
Flagged field; either fails if the field or Impediment value is unavailable.
List current transitions before choosing one.
`--force` does not replace authorization. Before adding a comment, decide whether
it must be public or internal; `issue comment add --internal` uses the JSM
request-comment API and fails if the response does not confirm `public: false`.
Never fall back to a public comment on an internal-comment failure.

This skill does not authorize running the Rust binary against a live Jira tenant
when the active agent policy pins another executable. Request an approved workflow
change rather than bypassing the boundary.

# Jira CLI development reference

The CLI is implemented in Rust. The command-line contract is defined in
`src/main.rs` with Clap, Jira HTTP and authentication behavior is in
`src/client.rs`, configuration/profile storage is in `src/config.rs`, and skill
discovery/installation is in `src/skills.rs`.

## Agent skills

The canonical skill source is `plugins/jira/skills/jira/SKILL.md`; it is embedded
at compile time by `src/skills.rs`. Keep the frontmatter name and description
useful for discovery. Do not copy skill text into separate generated templates.

```sh
jira --json skills list
jira skills show jira
jira skills install --target opencode
jira skills install --target claude --force
```

`skills list` reports the bundled skill and local skills found under common
Claude Code, OpenCode, `.agents`, Codex, and Tabnine roots, plus paths supplied
through `JIRA_SKILLS_PATH`. `skills show` prints a skill by its frontmatter name.
`skills install` installs the bundled Jira skill for Claude Code, OpenCode, and
`.agents` by default. `--target` selects one host; `--force` replaces an existing
regular `SKILL.md` without following symlinks. `install-skill --dest <dir>` is
retained as the explicit-destination form. Skill discovery reads frontmatter
`name` and `description`, and never contacts Jira or reads Jira credentials.

The Claude Code plugin at `plugins/jira/` is an additional discovery and
distribution surface. It must reference the same canonical skill file.

## Validation

Run the repository checks before proposing a change:

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --release --locked
```

Tests use local HTTP fixtures; they do not require Jira credentials. Never put
tokens or cookies in test fixtures, logs, or examples.

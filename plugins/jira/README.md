# Jira CLI agent skill

This plugin exposes the bundled Rust `jira` CLI skill to Claude Code. The binary
also provides `jira skills list`, `jira skills show jira`, and `jira skills
install` for discovery and installation into other supported agent skill
directories.

## Install

```
/plugin marketplace add pchuri/jira-cli
/plugin install jira@pchuri-jira-cli
```

## Install the CLI

```sh
cargo install --git https://github.com/pchuri/jira-cli --locked jira-cli
jira --help
```

See the [project README](https://github.com/pchuri/jira-cli#readme) for
configuration, authentication, and agent skill installation details.

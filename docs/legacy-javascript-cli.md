# Legacy JavaScript CLI

This repository still contains the published npm `@pchuri/jira-cli` implementation
under `bin/` and `lib/`, with its separate installation and release workflows.
The npm-managed `jira` command on PATH is distinct from the Rust candidate
`~/.local/share/jira-cli-rust/bin/jira`.

The existing [npm plugin skill](../plugins/jira/skills/jira/SKILL.md) documents
the JavaScript command syntax. Refer to `jira --help` for installed npm options
and the full-path Rust `jira --help` for the Rust candidate. Never silently substitute one for
the other in automation.

Run legacy checks with `npm ci`, `npm run lint`, and
`npm run test:coverage -- --runInBand` while the JavaScript code remains in
scope. The Rust cutover has **not** been authorized by local tests alone.

mod client;
mod config;

use anyhow::{bail, Context, Result};
use clap::{Args, CommandFactory, Parser, Subcommand};
use clap_complete::{generate, Shell};
use client::{encoded, markdown_to_adf, markdown_to_plain_text, Jira};
use reqwest::Method;
use serde_json::{json, Map, Value};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Parser)]
#[command(
    name = "jira",
    version,
    about = "A scriptable Jira CLI for people and agents",
    long_about = concat!(
        "A scriptable Jira CLI for people and agents.\n\n",
        "Discover project-specific fields and preview a create request before writing. ",
        "Use --json for machine-readable results; writes are never retried automatically.\n\n",
        "Examples:\n",
        "  jira --json issue list --project DEMO\n",
        "  jira --json skills list\n",
        "  jira --json issue create-fields --project DEMO --type Task\n",
        "  jira --json issue create --project DEMO --type Task --summary 'Example' --dry-run"
    ),
    help_template = concat!(
        "{name} {version}\n{about-with-newline}\n",
        "{usage-heading} {usage}\n\n{all-args}{after-help}"
    )
)]
struct Cli {
    #[arg(long, global = true, help = "Choose a configured Jira profile")]
    profile: Option<String>,
    #[arg(
        long,
        global = true,
        help = "Emit one JSON document to stdout; errors as JSON on stderr"
    )]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Inspect or change locally stored configuration
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    /// Manage named Jira profiles
    Profile {
        #[command(subcommand)]
        command: ProfileCommand,
    },
    /// Search and manage Jira issues
    Issue {
        #[command(subcommand)]
        command: Box<IssueCommand>,
    },
    /// List and view Jira projects
    Project {
        #[command(subcommand)]
        command: ProjectCommand,
    },
    /// List Jira boards and their sprints
    Sprint {
        #[command(subcommand)]
        command: SprintCommand,
    },
    /// Discover agent skills packaged with this Rust CLI
    Skills {
        #[command(subcommand)]
        command: SkillsCommand,
    },
    /// Copy the packaged Jira skill into the current project
    InstallSkill {
        #[arg(long, default_value = "./.claude/skills/jira-rust-candidate")]
        dest: PathBuf,
        #[arg(long)]
        force: bool,
    },
    /// Generate shell completion scripts
    Completions {
        #[arg(value_enum)]
        shell: Shell,
    },
}

#[derive(Subcommand)]
enum ConfigCommand {
    Show,
    Set {
        key: String,
        value: String,
    },
    Unset {
        key: String,
    },
    /// Check authentication without printing account details
    Test,
}

#[derive(Subcommand)]
enum ProfileCommand {
    List,
    Use { name: String },
    Add(Box<ProfileAddArgs>),
    Remove { name: String },
}

#[derive(Args)]
struct ProfileAddArgs {
    name: String,
    #[arg(long)]
    server: String,
    #[arg(long)]
    username: Option<String>,
    #[arg(long)]
    token: Option<String>,
    #[arg(long)]
    auth_type: Option<String>,
    #[arg(long)]
    api_version: Option<String>,
    #[arg(long)]
    cloud_id: Option<String>,
    #[arg(long)]
    cookie: Option<String>,
    #[arg(long)]
    tls_client_cert: Option<String>,
    #[arg(long)]
    tls_client_key: Option<String>,
    #[arg(long)]
    tls_ca_cert: Option<String>,
}

#[derive(Subcommand)]
enum IssueCommand {
    /// Search using JQL and/or structured filters; JSON retains Jira's returned fields
    List(ListArgs),
    /// Read an issue with all fields, including custom fields
    View {
        key: String,
        #[arg(long)]
        markdown: bool,
        #[arg(long)]
        output: Option<PathBuf>,
        #[arg(long = "format", value_parser = ["terminal", "json", "markdown"])]
        format: Option<String>,
    },
    /// Discover required create fields and allowed values for a project and type
    CreateFields {
        #[arg(long)]
        project: String,
        #[arg(long = "type")]
        issue_type: String,
    },
    /// Preview or create exactly one issue; --dry-run never writes
    #[command(
        after_help = "Tip: Run `jira --json issue create-fields --project DEMO --type Task` before creating an issue."
    )]
    Create(CreateArgs),
    /// Edit only the requested fields
    Edit {
        key: String,
        #[arg(long)]
        summary: Option<String>,
        #[arg(long)]
        description: Option<String>,
        #[arg(long, conflicts_with = "description")]
        description_file: Option<PathBuf>,
        #[arg(long)]
        assignee: Option<String>,
        #[arg(long)]
        priority: Option<String>,
        #[arg(long)]
        fields_file: Option<PathBuf>,
    },
    /// Delete an issue after explicit --force confirmation
    Delete {
        key: String,
        #[arg(long)]
        force: bool,
    },
    /// List available workflow transitions without writing
    Transitions { key: String },
    /// Move via a currently available transition
    Transition {
        key: String,
        #[arg(long)]
        to: String,
    },
    /// Mark an issue as an impediment using its Jira Software Flagged field
    Flag { key: String },
    /// Remove the impediment flag from an issue
    Unflag { key: String },
    /// Read and write issue comments
    Comment {
        #[command(subcommand)]
        command: CommentCommand,
    },
    /// Read and write remote links
    RemoteLink {
        #[command(subcommand)]
        command: RemoteLinkCommand,
    },
}

#[derive(Args)]
struct CreateArgs {
    #[arg(long, help = "Project key, for example DEMO")]
    project: String,
    #[arg(long = "type", help = "Issue type name or ID")]
    issue_type: String,
    #[arg(long, help = "Issue summary")]
    summary: String,
    #[arg(
        long,
        conflicts_with = "description_file",
        help = "Markdown description"
    )]
    description: Option<String>,
    #[arg(long, help = "Read a Markdown description from a UTF-8 file")]
    description_file: Option<PathBuf>,
    #[arg(
        long,
        help = "JSON object keyed by create-field IDs discovered for this project and type"
    )]
    fields_file: Option<PathBuf>,
    #[arg(long, help = "Account ID (API v3) or username (API v2)")]
    assignee: Option<String>,
    #[arg(long, help = "Priority name available on the project's create screen")]
    priority: Option<String>,
    #[arg(long, value_delimiter = ',', help = "Comma-separated issue labels")]
    labels: Vec<String>,
    #[arg(long, help = "Parent issue key")]
    parent: Option<String>,
    #[arg(long = "due-date", help = "Due date in YYYY-MM-DD format")]
    due_date: Option<String>,
    #[arg(
        long,
        help = "Validate and show the exact request without creating an issue"
    )]
    dry_run: bool,
}

#[derive(Args)]
struct ListArgs {
    #[arg(long)]
    jql: Option<String>,
    #[arg(long)]
    project: Option<String>,
    #[arg(long)]
    assignee: Option<String>,
    #[arg(long)]
    reporter: Option<String>,
    #[arg(long)]
    status: Option<String>,
    #[arg(long = "type")]
    issue_type: Option<String>,
    #[arg(long)]
    priority: Option<String>,
    #[arg(long)]
    created: Option<String>,
    #[arg(long)]
    updated: Option<String>,
    #[arg(long, default_value_t = 20)]
    limit: u32,
}

#[derive(Subcommand)]
enum CommentCommand {
    List {
        key: String,
        #[arg(long, help = "Render comment bodies as Markdown when supported")]
        markdown: bool,
    },
    Add {
        key: String,
        #[arg(conflicts_with = "file")]
        text: Option<String>,
        #[arg(long)]
        file: Option<PathBuf>,
        #[arg(
            long,
            help = "Post a JSM internal note; require public: false confirmation"
        )]
        internal: bool,
    },
    Edit {
        key: String,
        id: String,
        #[arg(conflicts_with = "file")]
        text: Option<String>,
        #[arg(long)]
        file: Option<PathBuf>,
    },
    Delete {
        key: String,
        id: String,
        #[arg(long)]
        force: bool,
    },
}

#[derive(Subcommand)]
enum RemoteLinkCommand {
    List {
        key: String,
        #[arg(long)]
        global_id: Option<String>,
    },
    Add {
        key: String,
        #[arg(long)]
        url: String,
        #[arg(long)]
        title: String,
        #[arg(long)]
        global_id: Option<String>,
        #[arg(long)]
        summary: Option<String>,
        #[arg(long)]
        relationship: Option<String>,
        #[arg(long)]
        icon_url: Option<String>,
        #[arg(long)]
        icon_title: Option<String>,
    },
    /// Update a remote link without discarding its existing properties
    Update {
        key: String,
        id: String,
        #[arg(long)]
        url: Option<String>,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        summary: Option<String>,
        #[arg(long)]
        relationship: Option<String>,
        #[arg(long)]
        icon_url: Option<String>,
        #[arg(long)]
        icon_title: Option<String>,
    },
    Delete {
        key: String,
        id: String,
        #[arg(long)]
        force: bool,
    },
}

#[derive(Subcommand)]
enum ProjectCommand {
    List,
    View { key: String },
    Components { key: String },
    Versions { key: String },
}

#[derive(Subcommand)]
enum SprintCommand {
    Boards,
    List {
        #[arg(long)]
        board: Option<String>,
        #[arg(long, value_parser = ["active", "future", "closed"])]
        state: Option<String>,
        #[arg(long, conflicts_with = "state")]
        active: bool,
    },
    /// List active sprints for the selected board
    Active {
        #[arg(long)]
        board: Option<String>,
    },
}

#[derive(Subcommand)]
enum SkillsCommand {
    /// List skills packaged with this CLI; does not report agent-specific availability
    List,
}

fn emit(value: &Value, machine: bool) -> Result<()> {
    if machine {
        println!("{}", serde_json::to_string(value)?);
    } else {
        println!("{}", serde_json::to_string_pretty(value)?);
    }
    Ok(())
}

fn main() {
    let arguments: Vec<_> = std::env::args_os().collect();
    let wants_json = arguments.iter().any(|argument| argument == "--json");
    let cli = match Cli::try_parse_from(arguments) {
        Ok(cli) => cli,
        Err(error)
            if matches!(
                error.kind(),
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
            ) =>
        {
            error.exit()
        }
        Err(error) if wants_json => {
            eprintln!(
                "{}",
                json!({"error": "invalid_arguments", "message": error.to_string()})
            );
            std::process::exit(2);
        }
        Err(error) => error.exit(),
    };
    if let Err(error) = run(&cli) {
        if cli.json {
            eprintln!(
                "{}",
                json!({"error": "command_failed", "message": format!("{error:#}")})
            );
        } else {
            eprintln!("Error: {error:#}");
        }
        std::process::exit(1);
    }
}

fn run(cli: &Cli) -> Result<()> {
    match &cli.command {
        Command::Completions { shell } => {
            let mut command = Cli::command();
            generate(*shell, &mut command, "jira", &mut std::io::stdout());
            return Ok(());
        }
        Command::Skills {
            command: SkillsCommand::List,
        } => return emit(&packaged_skills(), cli.json),
        _ => {}
    }

    let mut store = config::load()?;
    let selected = config::selected(&store, cli.profile.as_deref());
    match &cli.command {
        Command::Config { command } => match command {
            ConfigCommand::Show => emit(
                &json!({"profile": selected, "config": store.profiles.get(&selected).cloned().unwrap_or_default().redacted()}),
                cli.json,
            ),
            ConfigCommand::Set { key, value } => {
                config::set(
                    store.profiles.entry(selected.clone()).or_default(),
                    key,
                    value,
                )?;
                if store.active_profile.is_none() {
                    store.active_profile = Some(selected);
                }
                config::save(&store)?;
                emit(&json!({"saved": key}), cli.json)
            }
            ConfigCommand::Unset { key } => {
                let profile = store
                    .profiles
                    .get_mut(&selected)
                    .with_context(|| format!("Profile {selected} does not exist"))?;
                config::unset(profile, key)?;
                config::save(&store)?;
                emit(&json!({"unset": key}), cli.json)
            }
            ConfigCommand::Test => {
                let jira = configured(&store, &selected)?;
                jira.get("/myself")?;
                emit(&json!({"connected": true}), cli.json)
            }
        },
        Command::Profile { command } => match command {
            ProfileCommand::List => emit(
                &json!({"activeProfile": store.active_profile, "profiles": store.profiles.iter().map(|(name, profile)| json!({"name": name, "config": profile.redacted()})).collect::<Vec<_>>()}),
                cli.json,
            ),
            ProfileCommand::Use { name } => {
                if !store.profiles.contains_key(name) {
                    bail!("Profile {name} does not exist");
                }
                store.active_profile = Some(name.clone());
                config::save(&store)?;
                emit(&json!({"activeProfile": name}), cli.json)
            }
            ProfileCommand::Add(args) => {
                let ProfileAddArgs {
                    name,
                    server,
                    username,
                    token,
                    auth_type,
                    api_version,
                    cloud_id,
                    cookie,
                    tls_client_cert,
                    tls_client_key,
                    tls_ca_cert,
                } = args.as_ref();
                if name.is_empty()
                    || !name
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte))
                {
                    bail!("Profile name must contain only letters, numbers, _ and -");
                }
                let profile = store.profiles.entry(name.clone()).or_default();
                config::set(profile, "server", server)?;
                if let Some(value) = username {
                    config::set(profile, "username", value)?;
                }
                if let Some(value) = token {
                    config::set(profile, "token", value)?;
                }
                if let Some(value) = auth_type {
                    config::set(profile, "authType", value)?;
                }
                if let Some(value) = api_version {
                    config::set(profile, "apiVersion", value)?;
                }
                if let Some(value) = cloud_id {
                    config::set(profile, "cloudId", value)?;
                }
                if let Some(value) = cookie {
                    config::set(profile, "cookie", value)?;
                }
                if let Some(value) = tls_client_cert {
                    config::set(profile, "tlsClientCert", value)?;
                }
                if let Some(value) = tls_client_key {
                    config::set(profile, "tlsClientKey", value)?;
                }
                if let Some(value) = tls_ca_cert {
                    config::set(profile, "tlsCaCert", value)?;
                }
                if store.active_profile.is_none() {
                    store.active_profile = Some(name.clone());
                }
                config::save(&store)?;
                emit(&json!({"profile": name, "saved": true}), cli.json)
            }
            ProfileCommand::Remove { name } => {
                if store.profiles.remove(name).is_none() {
                    bail!("Profile {name} does not exist");
                }
                if store.active_profile.as_deref() == Some(name) {
                    store.active_profile = None;
                }
                config::save(&store)?;
                emit(&json!({"removed": name}), cli.json)
            }
        },
        Command::Issue { command } => issue(command, configured(&store, &selected)?, cli.json),
        Command::Project { command } => {
            let jira = configured(&store, &selected)?;
            let path = match command {
                ProjectCommand::List => "/project".into(),
                ProjectCommand::View { key } => format!("/project/{}", encoded(key)),
                ProjectCommand::Components { key } => {
                    format!("/project/{}/components", encoded(key))
                }
                ProjectCommand::Versions { key } => format!("/project/{}/versions", encoded(key)),
            };
            emit(&jira.get(&path)?, cli.json)
        }
        Command::Sprint { command } => {
            let jira = configured(&store, &selected)?;
            let result = match command {
                SprintCommand::Boards => jira.get_agile("/board")?,
                SprintCommand::List {
                    board,
                    state,
                    active,
                } => sprints(
                    &jira,
                    board.as_deref(),
                    if *active {
                        Some("active")
                    } else {
                        state.as_deref()
                    },
                )?,
                SprintCommand::Active { board } => {
                    sprints(&jira, board.as_deref(), Some("active"))?
                }
            };
            emit(&result, cli.json)
        }
        Command::InstallSkill { dest, force } => {
            let path = install_skill(dest, *force)?;
            emit(&json!({"installed": path}), cli.json)
        }
        Command::Skills { .. } => unreachable!("handled before loading configuration"),
        Command::Completions { .. } => unreachable!("handled before loading configuration"),
    }
}

fn packaged_skills() -> Value {
    json!({
        "skills": [{
            "name": "jira-rust-candidate",
            "description": "Read Jira with the Rust CLI and prepare exact issue drafts for user-approved mutations.",
            "distribution": "packaged",
            "install_command": "jira install-skill"
        }]
    })
}

fn configured(store: &config::Store, selected: &str) -> Result<Jira> {
    let profile = if let Some(profile) = store.profiles.get(selected) {
        profile.clone()
    } else if selected == "default" && store.profiles.is_empty() {
        config::Profile::default()
    } else {
        bail!("Profile {selected} does not exist; use `jira profile list`");
    };
    let profile = profile.resolve()?;
    Jira::new(profile)
}

fn read_text(path: &PathBuf) -> Result<String> {
    let content =
        fs::read_to_string(path).with_context(|| format!("Cannot read {}", path.display()))?;
    if content.trim().is_empty() {
        bail!("{} is empty", path.display());
    }
    Ok(content)
}

fn body(text: Option<&str>, file: Option<&PathBuf>) -> Result<Option<String>> {
    if let Some(file) = file {
        Ok(Some(read_text(file)?))
    } else {
        Ok(text.map(str::to_owned))
    }
}

fn jql_literal(value: &str) -> String {
    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t");
    format!("\"{escaped}\"")
}

fn list_jql(args: &ListArgs) -> Result<String> {
    let mut clauses = Vec::new();
    for (field, value, operator) in [
        ("project", &args.project, "="),
        ("assignee", &args.assignee, "="),
        ("reporter", &args.reporter, "="),
        ("status", &args.status, "="),
        ("issuetype", &args.issue_type, "="),
        ("priority", &args.priority, "="),
        ("created", &args.created, ">="),
        ("updated", &args.updated, ">="),
    ] {
        if let Some(value) = value {
            if value.is_empty() {
                bail!("--{field} cannot be empty");
            }
            let expression = if ["assignee", "reporter"].contains(&field) && value == "currentUser"
            {
                "currentUser()".to_string()
            } else {
                jql_literal(value)
            };
            clauses.push(format!("{field} {operator} {expression}"));
        }
    }
    let mut order_by = None;
    if let Some(jql) = &args.jql {
        if jql.trim().is_empty() {
            bail!("--jql cannot be empty");
        }
        if clauses.is_empty() {
            return Ok(jql.trim().to_string());
        }
        let (filter, ordering) = split_order_by(jql);
        if !filter.is_empty() {
            clauses.push(format!("({filter})"));
        }
        order_by = ordering;
    }
    let mut result = if clauses.is_empty() {
        "ORDER BY updated DESC".into()
    } else {
        clauses.join(" AND ")
    };
    if let Some(ordering) = order_by {
        result.push(' ');
        result.push_str(ordering);
    }
    Ok(result)
}

fn split_order_by(jql: &str) -> (&str, Option<&str>) {
    let jql = jql.trim();
    let bytes = jql.as_bytes();
    let mut quote = None;
    let mut depth = 0usize;
    let mut last = None;
    let mut i = 0;
    while i < bytes.len() {
        let byte = bytes[i];
        if let Some(end) = quote {
            if byte == b'\\' {
                i += 2;
                continue;
            }
            if byte == end {
                quote = None;
            }
        } else {
            match byte {
                b'\'' | b'"' => quote = Some(byte),
                b'(' => depth += 1,
                b')' => depth = depth.saturating_sub(1),
                _ if depth == 0 && (i == 0 || !bytes[i - 1].is_ascii_alphanumeric()) => {
                    let rest = &bytes[i..];
                    if rest.len() >= 8
                        && rest[..5].eq_ignore_ascii_case(b"ORDER")
                        && rest[5].is_ascii_whitespace()
                    {
                        let spaces = rest[5..]
                            .iter()
                            .take_while(|b| b.is_ascii_whitespace())
                            .count();
                        let by = 5 + spaces;
                        if rest.len() >= by + 2
                            && rest[by..by + 2].eq_ignore_ascii_case(b"BY")
                            && (rest.len() == by + 2 || !rest[by + 2].is_ascii_alphanumeric())
                        {
                            last = Some(i);
                        }
                    }
                }
                _ => {}
            }
        }
        i += 1;
    }
    match last {
        Some(index) => (jql[..index].trim(), Some(jql[index..].trim())),
        None => (jql, None),
    }
}

fn sprints(jira: &Jira, board: Option<&str>, state: Option<&str>) -> Result<Value> {
    let board = match board {
        Some(board) if !board.is_empty() => board.to_owned(),
        Some(_) => bail!("Board ID cannot be empty"),
        None => {
            let mut start = 0;
            let mut selected = None;
            loop {
                let response = jira.get_agile(&format!("/board?startAt={start}&maxResults=100"))?;
                let boards = response["values"]
                    .as_array()
                    .context("Invalid board response")?;
                for item in boards {
                    let id = item["id"].as_u64().context("Board has no numeric ID")?;
                    if selected.replace(id).is_some() {
                        bail!("Multiple boards found; use --board <ID> (see sprint boards)");
                    }
                }
                if response["isLast"] == true
                    || response["total"]
                        .as_u64()
                        .is_some_and(|total| start as u64 + boards.len() as u64 >= total)
                {
                    break;
                }
                if boards.is_empty() {
                    if response["total"]
                        .as_u64()
                        .is_some_and(|total| (start as u64) < total)
                    {
                        bail!("Incomplete board listing; specify --board <ID>");
                    }
                    break;
                }
                if response["total"].is_null() && response["isLast"].is_null() && boards.len() < 100
                {
                    break;
                }
                if start >= 10_000 {
                    bail!("Too many boards to select safely; use --board <ID>");
                }
                start += boards.len();
            }
            selected
                .context("No boards found; specify --board <ID> if you know one")?
                .to_string()
        }
    };
    let mut path = format!("/board/{}/sprint", encoded(&board));
    if let Some(state) = state {
        path.push_str(&format!("?state={}", encoded(state)));
    }
    jira.get_agile(&path)
}

fn install_skill(dest: &PathBuf, force: bool) -> Result<PathBuf> {
    // Embed the packaged source so installation does not depend on the caller's
    // current directory or the location of the executable.
    const SKILL: &str =
        include_str!("../plugins/jira-rust-candidate/skills/jira-rust-candidate/SKILL.md");
    let dir = if dest == &PathBuf::from("~") || dest.starts_with("~/") {
        let home = std::env::var_os("HOME").context("HOME is not set")?;
        PathBuf::from(home).join(dest.strip_prefix("~")?)
    } else {
        dest.clone()
    };
    let path = dir.join("SKILL.md");
    match fs::symlink_metadata(&path) {
        Ok(metadata) if !force || !metadata.file_type().is_file() => {
            bail!(
                "{} already exists; --force is required to replace a regular file",
                path.display()
            );
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error).with_context(|| format!("Cannot inspect {}", path.display()))
        }
    }
    fs::create_dir_all(&dir).with_context(|| format!("Cannot create {}", dir.display()))?;
    if force {
        let (mut temporary, temp_path) = tempfile_path(&dir)?;
        if let Err(error) = temporary.write_all(SKILL.as_bytes()) {
            let _ = fs::remove_file(&temp_path);
            return Err(error.into());
        }
        drop(temporary);
        if let Err(error) = fs::rename(&temp_path, &path) {
            let _ = fs::remove_file(&temp_path);
            return Err(error).with_context(|| format!("Cannot replace {}", path.display()));
        }
    } else {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .with_context(|| format!("{} already exists or cannot be created", path.display()))?;
        if let Err(error) = file.write_all(SKILL.as_bytes()) {
            drop(file);
            let _ = fs::remove_file(&path);
            return Err(error.into());
        }
    }
    Ok(path)
}

fn tempfile_path(dir: &Path) -> Result<(fs::File, PathBuf)> {
    for index in 0..1000 {
        let path = dir.join(format!(".SKILL.md.{}.{}.tmp", std::process::id(), index));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => return Ok((file, path)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
    bail!("Cannot allocate a temporary skill file")
}

fn issue(command: &IssueCommand, mut jira: Jira, machine: bool) -> Result<()> {
    let result = match command {
        IssueCommand::List(args) => jira.search_issues(&list_jql(args)?, args.limit)?,
        IssueCommand::View {
            key,
            markdown,
            output,
            format,
        } => {
            if (*markdown && format.as_deref() == Some("json"))
                || (machine
                    && (*markdown || format.as_deref() == Some("markdown"))
                    && output.is_none())
            {
                bail!("Markdown stdout cannot be combined with --json or --format json");
            }
            let issue = jira.get(&format!("/issue/{}?fields=*all", encoded(key)))?;
            let as_markdown = *markdown
                || format.as_deref() == Some("markdown")
                || (output.is_some() && format.as_deref() != Some("json") && !machine);
            if as_markdown {
                let document = issue_markdown(&issue, jira.server())?;
                if let Some(path) = output {
                    fs::write(path, document)
                        .with_context(|| format!("Cannot write {}", path.display()))?;
                    json!({"key": key, "saved": path})
                } else {
                    if machine {
                        bail!("--json cannot be combined with markdown output");
                    }
                    println!("{document}");
                    return Ok(());
                }
            } else if let Some(path) = output {
                let document = format!("{}\n", serde_json::to_string_pretty(&issue)?);
                fs::write(path, document)
                    .with_context(|| format!("Cannot write {}", path.display()))?;
                json!({"key": key, "saved": path})
            } else {
                issue
            }
        }
        IssueCommand::CreateFields {
            project,
            issue_type,
        } => jira.create_fields(project, issue_type)?,
        IssueCommand::Create(args) => create(&mut jira, args)?,
        IssueCommand::Edit {
            key,
            summary,
            description,
            description_file,
            assignee,
            priority,
            fields_file,
        } => {
            let mut fields = Map::new();
            if let Some(summary) = summary {
                fields.insert("summary".into(), json!(summary));
            }
            if let Some(description) = body(description.as_deref(), description_file.as_ref())? {
                fields.insert("description".into(), json!(description));
            }
            let extra = read_fields_file(fields_file.as_ref())?;
            for (id, value) in extra {
                insert_field(&mut fields, &id, value)?;
            }
            if let Some(priority) = priority {
                insert_field(&mut fields, "priority", json!({"name": priority}))?;
            }
            if fields.is_empty() && assignee.is_none() {
                bail!("Specify at least one field to edit");
            }
            // Fetch the edit screen so unknown/read-only fields never slip into a PUT.
            let metadata = jira.get(&format!("/issue/{}/editmeta", encoded(key)))?;
            if let Some(assignee) = assignee {
                insert_field(
                    &mut fields,
                    "assignee",
                    user_value(assignee, jira.version()),
                )?;
            }
            let available = metadata["fields"]
                .as_object()
                .context("Edit metadata has no fields")?;
            for (id, value) in &mut fields {
                let definition = available
                    .get(id)
                    .with_context(|| format!("Field {id} is not on the edit screen"))?;
                *value = validate_field(id, value, definition, jira.version())?;
                if id == "priority" {
                    validate_named_option(id, value, definition)?;
                }
            }
            jira.write(
                Method::PUT,
                &format!("/issue/{}", encoded(key)),
                Some(&json!({"fields": fields})),
            )?;
            json!({"key": key, "updated": true})
        }
        IssueCommand::Delete { key, force } => {
            if !force {
                bail!("Deletion requires --force");
            }
            jira.write(Method::DELETE, &format!("/issue/{}", encoded(key)), None)?;
            json!({"key": key, "deleted": true})
        }
        IssueCommand::Transitions { key } => {
            let response = jira.get(&format!("/issue/{}/transitions", encoded(key)))?;
            json!({"issueKey": key, "transitions": response["transitions"]})
        }
        IssueCommand::Transition { key, to } => {
            let response = jira.get(&format!("/issue/{}/transitions", encoded(key)))?;
            let available = response["transitions"]
                .as_array()
                .context("Invalid transition response")?;
            let matches: Vec<_> = available
                .iter()
                .filter(|item| {
                    item["id"] == *to
                        || item["name"]
                            .as_str()
                            .is_some_and(|name| name.eq_ignore_ascii_case(to))
                        || item["to"]["name"]
                            .as_str()
                            .is_some_and(|name| name.eq_ignore_ascii_case(to))
                })
                .collect();
            if matches.len() != 1 {
                bail!("Transition must uniquely match an available ID, name or target status; found {} matches", matches.len());
            }
            let id = matches[0]["id"].as_str().context("Transition has no ID")?;
            jira.write(
                Method::POST,
                &format!("/issue/{}/transitions", encoded(key)),
                Some(&json!({"transition": {"id": id}})),
            )?;
            json!({"key": key, "transitionId": id, "target": matches[0]["to"]["name"]})
        }
        IssueCommand::Flag { key } => set_issue_flag(&jira, key, true)?,
        IssueCommand::Unflag { key } => set_issue_flag(&jira, key, false)?,
        IssueCommand::Comment { command } => comment(&jira, command)?,
        IssueCommand::RemoteLink { command } => remote_link(&jira, command)?,
    };
    emit(&result, machine)
}

fn set_issue_flag(jira: &Jira, key: &str, flagged: bool) -> Result<Value> {
    let path = format!("/issue/{}/editmeta", encoded(key));
    let metadata = jira.get(&path)?;
    let fields = metadata["fields"]
        .as_object()
        .context("Edit metadata has no fields")?;
    let flagged_field = fields
        .iter()
        .find(|(_, definition)| {
            definition["name"]
                .as_str()
                .is_some_and(|name| name.eq_ignore_ascii_case("Flagged"))
        })
        .with_context(|| format!("Issue {key} has no editable Jira Software Flagged field"))?;
    let (field_id, definition) = flagged_field;
    if definition["schema"]["type"] != "array" {
        bail!("Issue {key} has an unsupported Flagged field schema");
    }

    let value = if flagged {
        let options = definition["allowedValues"]
            .as_array()
            .context("Flagged field metadata has no allowed values; cannot safely flag issue")?;
        let impediment = options
            .iter()
            .find(|option| {
                option["value"]
                    .as_str()
                    .is_some_and(|value| value.eq_ignore_ascii_case("Impediment"))
            })
            .context("Flagged field does not allow the Impediment value")?;
        json!([{"value": impediment["value"]}])
    } else {
        json!([])
    };

    let mut update = Map::new();
    update.insert(field_id.clone(), value);
    jira.write(
        Method::PUT,
        &format!("/issue/{}", encoded(key)),
        Some(&json!({"fields": update})),
    )?;
    Ok(json!({"key": key, "flagged": flagged}))
}

fn read_fields_file(path: Option<&PathBuf>) -> Result<Map<String, Value>> {
    let Some(path) = path else {
        return Ok(Map::new());
    };
    let parsed: Value =
        serde_json::from_str(&read_text(path)?).context("Fields file must be valid JSON")?;
    parsed
        .as_object()
        .cloned()
        .context("Fields file must contain an object keyed by Jira field ID")
}

fn insert_field(fields: &mut Map<String, Value>, id: &str, value: Value) -> Result<()> {
    if fields.contains_key(id) {
        bail!("Field {id} duplicates an explicit option or fields-file entry");
    }
    fields.insert(id.to_string(), value);
    Ok(())
}

fn user_value(user: &str, version: u8) -> Value {
    if version == 3 {
        json!({"accountId": user})
    } else {
        json!({"name": user})
    }
}

fn validate_field(id: &str, value: &Value, definition: &Value, version: u8) -> Result<Value> {
    if value.is_null() || value == "" || value.as_array().is_some_and(Vec::is_empty) {
        bail!("Field {id} must have a value");
    }
    let custom = definition["schema"]["custom"].as_str().unwrap_or("");
    let schema_type = definition["schema"]["type"].as_str().unwrap_or("");
    if definition["schema"].is_null() && id.starts_with("customfield_") {
        bail!("Missing schema for {id}; cannot safely write");
    }
    if custom.ends_with(":textarea")
        && !value.is_string()
        && !(version == 3 && value["type"] == "doc")
    {
        bail!("Textarea {id} requires a string or API v3 ADF document");
    }
    if custom.ends_with(":textfield") && !value.is_string() {
        bail!("Text field {id} requires a string");
    }
    if schema_type == "number" && !value.is_number() {
        bail!("Field {id} requires a number");
    }
    if schema_type == "array" && !value.is_array() {
        bail!("Field {id} requires an array");
    }
    if schema_type == "array"
        && id == "labels"
        && value.as_array().is_some_and(|items| {
            items
                .iter()
                .any(|item| item.as_str().is_none_or(str::is_empty))
        })
    {
        bail!("Labels require nonempty strings");
    }
    if ["date", "datetime"].contains(&schema_type) && !value.is_string() {
        bail!("Field {id} requires a date string");
    }
    if schema_type == "option" {
        if !value.is_object() || (value["id"].is_null() && value["value"].is_null()) {
            bail!("Option field {id} requires an object with id or value");
        }
        if let Some(options) = definition["allowedValues"].as_array() {
            if !options.is_empty()
                && !options.iter().any(|option| {
                    (!value["id"].is_null() && option["id"] == value["id"])
                        || (!value["value"].is_null() && option["value"] == value["value"])
                })
            {
                bail!("Invalid option for {id}; inspect metadata for allowedValues");
            }
        }
    }
    Ok(if id == "description" || custom.ends_with(":textarea") {
        if version == 3 {
            value
                .as_str()
                .map(markdown_to_adf)
                .unwrap_or_else(|| value.clone())
        } else {
            value
                .as_str()
                .map(markdown_to_plain_text)
                .map(Value::String)
                .unwrap_or_else(|| value.clone())
        }
    } else {
        value.clone()
    })
}

fn create(jira: &mut Jira, args: &CreateArgs) -> Result<Value> {
    let metadata = jira.create_fields(&args.project, &args.issue_type)?;
    let extra = read_fields_file(args.fields_file.as_ref())?;
    let mut fields = Map::new();
    fields.insert("project".into(), json!({"key": args.project}));
    fields.insert(
        "issuetype".into(),
        json!({"id": metadata["issueType"]["id"]}),
    );
    fields.insert("summary".into(), json!(args.summary));
    if let Some(description) = body(args.description.as_deref(), args.description_file.as_ref())? {
        fields.insert(
            "description".into(),
            if jira.version() == 3 {
                markdown_to_adf(&description)
            } else {
                json!(markdown_to_plain_text(&description))
            },
        );
    }
    if let Some(assignee) = &args.assignee {
        insert_field(
            &mut fields,
            "assignee",
            user_value(assignee, jira.version()),
        )?;
    }
    if let Some(priority) = &args.priority {
        insert_field(&mut fields, "priority", json!({"name": priority}))?;
    }
    if !args.labels.is_empty() {
        if args.labels.iter().any(|label| label.trim().is_empty()) {
            bail!("Labels must not be empty");
        }
        insert_field(&mut fields, "labels", json!(args.labels))?;
    }
    if let Some(parent) = &args.parent {
        insert_field(&mut fields, "parent", json!({"key": parent}))?;
    }
    if let Some(due_date) = &args.due_date {
        if !valid_date(due_date) {
            bail!("--due-date must be a valid YYYY-MM-DD date");
        }
        insert_field(&mut fields, "duedate", json!(due_date))?;
    }
    let available = metadata["fields"]
        .as_array()
        .context("Create metadata has no fields")?;
    for (id, value) in extra {
        insert_field(&mut fields, &id, value)?;
    }
    for (id, value) in &mut fields {
        if id == "project" || id == "issuetype" {
            continue;
        }
        let definition = available
            .iter()
            .find(|field| field["id"] == id.as_str())
            .with_context(|| format!("Field {id} is not on the create screen"))?;
        *value = validate_field(id, value, definition, jira.version())?;
        if id == "priority" {
            validate_named_option(id, value, definition)?;
        }
    }
    let missing: Vec<_> = available
        .iter()
        .filter(|field| field["required"] == true && field["hasDefaultValue"] != true)
        .filter(|field| {
            field["id"].as_str().is_some_and(|id| {
                fields.get(id).is_none_or(|value| {
                    value.is_null() || value == "" || value.as_array().is_some_and(Vec::is_empty)
                })
            })
        })
        .map(|field| {
            format!(
                "{} ({})",
                field["name"].as_str().unwrap_or("unknown"),
                field["id"].as_str().unwrap_or("unknown")
            )
        })
        .collect();
    if !missing.is_empty() {
        bail!("Missing required create fields: {}", missing.join(", "));
    }
    let payload = json!({"fields": fields});
    if args.dry_run {
        return Ok(json!({"dryRun": true, "apiVersion": jira.version(), "request": payload}));
    }
    let response = jira.create(&payload)?;
    let key = response["key"].as_str().context("Missing issue key")?;
    Ok(json!({"key": key, "url": format!("{}/browse/{key}", jira.server())}))
}

fn validate_named_option(id: &str, value: &Value, definition: &Value) -> Result<()> {
    if let Some(allowed) = definition["allowedValues"].as_array() {
        if !allowed.is_empty() && !allowed.iter().any(|item| item["name"] == value["name"]) {
            bail!("Invalid {id}; inspect create-fields for allowedValues");
        }
    }
    Ok(())
}

fn valid_date(date: &str) -> bool {
    let parts: Vec<_> = date.split('-').collect();
    if parts.len() != 3
        || parts[0].len() != 4
        || parts[1].len() != 2
        || parts[2].len() != 2
        || !date.bytes().all(|b| b.is_ascii_digit() || b == b'-')
    {
        return false;
    }
    let year = parts[0].parse::<u32>().unwrap_or(0);
    let month = parts[1].parse::<u32>().unwrap_or(0);
    let day = parts[2].parse::<u32>().unwrap_or(0);
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => 0,
    };
    year > 0 && day > 0 && day <= days
}

fn issue_markdown(issue: &Value, server: &str) -> Result<String> {
    let key = issue["key"].as_str().context("Issue response has no key")?;
    let fields = issue["fields"]
        .as_object()
        .context("Issue response has no fields")?;
    let summary = fields
        .get("summary")
        .and_then(Value::as_str)
        .context("Issue response has no summary")?;
    let mut output = format!("# {key}: {summary}\n\n## Metadata\n\n");
    for (label, id, fallback) in [
        ("Status", "status", "N/A"),
        ("Type", "issuetype", "N/A"),
        ("Priority", "priority", "N/A"),
        ("Assignee", "assignee", "Unassigned"),
        ("Reporter", "reporter", "N/A"),
    ] {
        let item = &issue["fields"][id];
        let value = item["displayName"]
            .as_str()
            .or(item["name"].as_str())
            .unwrap_or(fallback);
        output.push_str(&format!("- **{label}**: {value}\n"));
    }
    for (label, id) in [("Created", "created"), ("Updated", "updated")] {
        if let Some(value) = issue["fields"][id].as_str() {
            output.push_str(&format!("- **{label}**: {value}\n"));
        }
    }
    if let Some(labels) = issue["fields"]["labels"].as_array() {
        if !labels.is_empty() {
            output.push_str(&format!(
                "- **Labels**: {}\n",
                labels
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }
    output.push_str(&format!("- **URL**: {server}/browse/{}\n", encoded(key)));
    let description = &issue["fields"]["description"];
    if !description.is_null() {
        output.push_str("\n## Description\n\n");
        if let Some(text) = description.as_str() {
            output.push_str(text);
        } else if description["type"] == "doc" {
            let blocks = description["content"]
                .as_array()
                .context("Invalid ADF description: missing content")?;
            for block in blocks {
                output.push_str(&adf_text(block)?);
                output.push_str("\n\n");
            }
        } else {
            bail!("Unsupported issue description format for markdown output");
        }
        output.push('\n');
    }
    Ok(output.trim_end().to_string() + "\n")
}

fn adf_text(node: &Value) -> Result<String> {
    let kind = node["type"].as_str().context("ADF node has no type")?;
    let children = || -> Result<Vec<String>> {
        node["content"]
            .as_array()
            .map(|content| content.iter().map(adf_text).collect())
            .unwrap_or_else(|| Ok(Vec::new()))
    };
    let result = match kind {
        "text" => {
            let mut text = node["text"].as_str().unwrap_or_default().to_string();
            if let Some(marks) = node["marks"].as_array() {
                for mark in marks.iter().rev() {
                    match mark["type"].as_str().unwrap_or_default() {
                        "strong" => text = format!("**{text}**"),
                        "em" => text = format!("*{text}*"),
                        "strike" => text = format!("~~{text}~~"),
                        "code" => text = format!("`{text}`"),
                        "link" => {
                            let url = mark["attrs"]["href"].as_str().unwrap_or_default();
                            text = format!("[{text}]({url})");
                        }
                        "underline" => text = format!("__{text}__"),
                        "subsup" => match mark["attrs"]["type"].as_str() {
                            Some("sub") => text = format!("~{text}~"),
                            Some("sup") => text = format!("^{text}^"),
                            _ => {}
                        },
                        "textColor" | "backgroundColor" | "annotation" => {}
                        other => bail!("Unsupported ADF mark type {other} in markdown output"),
                    }
                }
            }
            text
        }
        "hardBreak" => "\n".into(),
        "mention" => node["attrs"]["text"].as_str().unwrap_or("@unknown").into(),
        "emoji" => node["attrs"]["shortName"]
            .as_str()
            .unwrap_or_default()
            .into(),
        "inlineCard" | "blockCard" => node["attrs"]["url"].as_str().unwrap_or_default().into(),
        "media" | "mediaGroup" | "mediaSingle" => "[media]".into(),
        "rule" => "---".into(),
        "paragraph" => children()?.join(""),
        "heading" => {
            let level = node["attrs"]["level"].as_u64().unwrap_or(2).clamp(1, 6) as usize;
            format!("{} {}", "#".repeat(level), children()?.join(""))
        }
        "bulletList" | "orderedList" => {
            let ordered = kind == "orderedList";
            let start = node["attrs"]["order"].as_u64().unwrap_or(1);
            children()?
                .iter()
                .enumerate()
                .map(|(index, item)| {
                    let prefix = if ordered {
                        format!("{}. ", start + index as u64)
                    } else {
                        "- ".into()
                    };
                    format!("{prefix}{}", item.trim().replace('\n', "\n  "))
                })
                .collect::<Vec<_>>()
                .join("\n")
        }
        "listItem" => children()?.join("\n"),
        "blockquote" => children()?
            .join("\n\n")
            .lines()
            .map(|line| format!("> {line}"))
            .collect::<Vec<_>>()
            .join("\n"),
        "codeBlock" => {
            let language = node["attrs"]["language"].as_str().unwrap_or_default();
            format!("```{language}\n{}\n```", children()?.join(""))
        }
        "table" => {
            let rows = children()?;
            if let Some(header) = rows.first() {
                let columns = header.matches('|').count().saturating_sub(1);
                let separator = format!("|{}|", " --- |".repeat(columns));
                std::iter::once(header.clone())
                    .chain(std::iter::once(separator))
                    .chain(rows.iter().skip(1).cloned())
                    .collect::<Vec<_>>()
                    .join("\n")
            } else {
                String::new()
            }
        }
        "tableRow" => format!("| {} |", children()?.join(" | ")),
        "tableCell" | "tableHeader" => children()?.join(""),
        _ => bail!("Unsupported ADF node type {kind} in markdown output"),
    };
    Ok(result)
}

fn comment(jira: &Jira, command: &CommentCommand) -> Result<Value> {
    let (key, id, method, text) = match command {
        CommentCommand::List { key, markdown } => {
            let response = jira.get(&format!("/issue/{}/comment", encoded(key)))?;
            return if *markdown {
                if jira.version() != 3 {
                    bail!("Markdown comment rendering requires Jira API v3 ADF responses");
                }
                comments_markdown(&response)
            } else {
                Ok(response)
            };
        }
        CommentCommand::Add {
            key,
            text,
            file,
            internal,
        } => {
            if *internal {
                let text = body(text.as_deref(), file.as_ref())?
                    .context("Comment text or --file is required")?;
                return jira.add_internal_comment(key, &markdown_to_plain_text(&text));
            }
            (
                key,
                None,
                Method::POST,
                body(text.as_deref(), file.as_ref())?,
            )
        }
        CommentCommand::Edit {
            key,
            id,
            text,
            file,
        } => (
            key,
            Some(id),
            Method::PUT,
            body(text.as_deref(), file.as_ref())?,
        ),
        CommentCommand::Delete { key, id, force } => {
            if !force {
                bail!("Deleting a comment requires --force");
            }
            (key, Some(id), Method::DELETE, None)
        }
    };
    let path = format!(
        "/issue/{}/comment{}",
        encoded(key),
        id.map(|id| format!("/{}", encoded(id))).unwrap_or_default()
    );
    let payload = if method == Method::DELETE {
        None
    } else {
        let text = text.context("Comment text or --file is required")?;
        Some(
            json!({"body": if jira.version() == 3 { markdown_to_adf(&text) } else { json!(markdown_to_plain_text(&text)) }}),
        )
    };
    let response = jira.write(method.clone(), &path, payload.as_ref())?;
    Ok(if method == Method::DELETE {
        json!({"deleted": true, "key": key, "id": id})
    } else if method == Method::PUT {
        json!({"updated": true, "key": key, "id": id})
    } else {
        response
    })
}

fn comments_markdown(response: &Value) -> Result<Value> {
    let comments = response["comments"]
        .as_array()
        .or_else(|| response["values"].as_array());
    let Some(comments) = comments else {
        return Ok(response.clone());
    };
    let comments = comments
        .iter()
        .map(|comment| -> Result<Value> {
            let mut rendered = comment.clone();
            if comment["body"]["type"] == "doc" {
                let blocks = comment["body"]["content"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(adf_text)
                    .collect::<Result<Vec<_>>>()?;
                rendered["body"] = json!(blocks.join("\n\n"));
            }
            Ok(rendered)
        })
        .collect::<Result<Vec<_>>>()?;
    if response["comments"].is_array() {
        let mut result = response.clone();
        result["comments"] = json!(comments);
        Ok(result)
    } else {
        let mut result = response.clone();
        result["values"] = json!(comments);
        Ok(result)
    }
}

fn remote_link(jira: &Jira, command: &RemoteLinkCommand) -> Result<Value> {
    match command {
        RemoteLinkCommand::List { key, global_id } => {
            let mut path = format!("/issue/{}/remotelink", encoded(key));
            if let Some(id) = global_id {
                path.push_str(&format!("?globalId={}", encoded(id)));
            }
            jira.get(&path)
        }
        RemoteLinkCommand::Add {
            key,
            url,
            title,
            global_id,
            summary,
            relationship,
            icon_url,
            icon_title,
        } => {
            let mut payload = json!({"object": {"url": url, "title": title}});
            if let Some(id) = global_id {
                payload["globalId"] = json!(id);
            }
            if let Some(value) = summary {
                payload["object"]["summary"] = json!(value);
            }
            if let Some(value) = relationship {
                payload["relationship"] = json!(value);
            }
            if let Some(value) = icon_url {
                payload["object"]["icon"]["url16x16"] = json!(value);
            }
            if let Some(value) = icon_title {
                payload["object"]["icon"]["title"] = json!(value);
            }
            jira.write(
                Method::POST,
                &format!("/issue/{}/remotelink", encoded(key)),
                Some(&payload),
            )
        }
        RemoteLinkCommand::Update {
            key,
            id,
            url,
            title,
            summary,
            relationship,
            icon_url,
            icon_title,
        } => {
            if url.is_none()
                && title.is_none()
                && summary.is_none()
                && relationship.is_none()
                && icon_url.is_none()
                && icon_title.is_none()
            {
                bail!("Specify at least one remote-link field to update");
            }
            let path = format!("/issue/{}/remotelink/{}", encoded(key), encoded(id));
            let existing = jira.get(&path)?;
            let mut payload = existing
                .as_object()
                .context("Invalid remote-link response")?
                .clone();
            payload.remove("id");
            payload.remove("self");
            let object = payload
                .get_mut("object")
                .and_then(Value::as_object_mut)
                .context("Remote link has no object; cannot safely update")?;
            if let Some(value) = url {
                object.insert("url".into(), json!(value));
            }
            if let Some(value) = title {
                object.insert("title".into(), json!(value));
            }
            if let Some(value) = summary {
                object.insert("summary".into(), json!(value));
            }
            if icon_url.is_some() || icon_title.is_some() {
                let icon = object.entry("icon").or_insert_with(|| json!({}));
                let icon = icon
                    .as_object_mut()
                    .context("Remote-link icon is not an object")?;
                if let Some(value) = icon_url {
                    icon.insert("url16x16".into(), json!(value));
                }
                if let Some(value) = icon_title {
                    icon.insert("title".into(), json!(value));
                }
            }
            if let Some(value) = relationship {
                payload.insert("relationship".into(), json!(value));
            }
            jira.write(Method::PUT, &path, Some(&Value::Object(payload)))?;
            Ok(json!({"updated": true, "key": key, "id": id}))
        }
        RemoteLinkCommand::Delete { key, id, force } => {
            if !force {
                bail!("Deleting a link requires --force");
            }
            jira.write(
                Method::DELETE,
                &format!("/issue/{}/remotelink/{}", encoded(key), encoded(id)),
                None,
            )?;
            Ok(json!({"deleted": true, "key": key, "id": id}))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    fn fixture(
        responses: Vec<(u16, Value)>,
    ) -> (String, Arc<Mutex<Vec<String>>>, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&requests);
        let thread = std::thread::spawn(move || {
            for (status, response) in responses {
                let (mut connection, _) = listener.accept().unwrap();
                let mut buffer = vec![0u8; 16384];
                let count = connection.read(&mut buffer).unwrap();
                let request = String::from_utf8_lossy(&buffer[..count]).into_owned();
                captured.lock().unwrap().push(request);
                let data = response.to_string();
                let reply = format!("HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{data}", data.len());
                connection.write_all(reply.as_bytes()).unwrap();
            }
        });
        (address, requests, thread)
    }

    fn create_fixture_fields() -> Value {
        json!({"fields": [
            {"fieldId": "summary", "name": "Summary", "required": true},
            {"fieldId": "customfield_10010", "name": "Impact Details", "required": true,
                "schema": {"type": "string", "custom": "jira:textarea"}},
            {"fieldId": "customfield_10011", "name": "Severity", "required": true,
                "schema": {"type": "option"}, "allowedValues": [{"id": "2", "value": "High"}]}
        ], "total": 3})
    }

    fn args(file: Option<PathBuf>, dry_run: bool) -> CreateArgs {
        CreateArgs {
            project: "TEST".into(),
            issue_type: "Bug".into(),
            summary: "Failure".into(),
            description: None,
            description_file: None,
            fields_file: file,
            assignee: None,
            priority: None,
            labels: vec![],
            parent: None,
            due_date: None,
            dry_run,
        }
    }

    fn configured_client(server: String) -> Jira {
        Jira::new(config::Profile {
            server,
            auth_type: "bearer".into(),
            token: "test-token".into(),
            api_version: "3".into(),
            ..Default::default()
        })
        .unwrap()
    }

    #[test]
    fn json_flag_parses_before_or_after_subcommand() {
        assert!(
            Cli::try_parse_from([
                "jira",
                "--json",
                "issue",
                "create-fields",
                "--project",
                "EX",
                "--type",
                "Bug"
            ])
            .unwrap()
            .json
        );
        assert!(
            Cli::try_parse_from([
                "jira",
                "issue",
                "create-fields",
                "--project",
                "EX",
                "--type",
                "Bug",
                "--json"
            ])
            .unwrap()
            .json
        );
        assert_eq!(
            Cli::try_parse_from(["jira", "--json", "issue", "create"])
                .err()
                .expect("missing arguments must fail")
                .kind(),
            clap::error::ErrorKind::MissingRequiredArgument
        );
    }

    #[test]
    fn issue_flag_commands_use_only_editable_flagged_field() {
        for (name, expected) in [("flag", r#"[{"value":"Impediment"}]"#), ("unflag", "[]")] {
            let cli = Cli::try_parse_from(["jira", "issue", name, "TEST-1"]).unwrap();
            let Command::Issue { command } = cli.command else {
                panic!("expected issue command");
            };
            let expected_flagged = name == "flag";
            let issue_command = *command;
            let metadata = json!({"fields": {"customfield_10021": {
                "name": "Flagged", "schema": {"type": "array"},
                "allowedValues": [{"value": "Impediment"}]
            }}});
            let (server, captured, thread) = fixture(vec![(200, metadata), (204, Value::Null)]);
            issue(&issue_command, configured_client(server), true).unwrap();
            thread.join().unwrap();
            let requests = captured.lock().unwrap();
            assert!(requests[0].starts_with("GET /rest/api/3/issue/TEST-1/editmeta "));
            assert!(requests[1].starts_with("PUT /rest/api/3/issue/TEST-1 "));
            assert!(requests[1].contains(&format!("\"customfield_10021\":{expected}")));
            assert_eq!(
                matches!(issue_command, IssueCommand::Flag { .. }),
                expected_flagged
            );
        }
    }

    #[test]
    fn issue_flag_refuses_when_flagged_field_is_not_editable() {
        let (server, captured, thread) = fixture(vec![(
            200,
            json!({"fields": {"summary": {"name": "Summary"}}}),
        )]);
        let error = set_issue_flag(&configured_client(server), "TEST-1", true).unwrap_err();
        assert!(error
            .to_string()
            .contains("no editable Jira Software Flagged field"));
        thread.join().unwrap();
        assert_eq!(captured.lock().unwrap().len(), 1);
    }

    #[test]
    fn root_help_identifies_the_build_and_shows_the_read_before_write_flow() {
        use clap::CommandFactory;

        let mut command = Cli::command();
        let help = command.render_long_help().to_string();
        assert!(help.contains(concat!("jira ", env!("CARGO_PKG_VERSION"))));
        assert!(help.contains("jira --json issue create-fields --project DEMO --type Task"));
        assert!(help.contains("--dry-run"));
        assert!(help.contains("--json"));
        assert!(help.contains("A scriptable Jira CLI for people and agents"));
    }

    #[test]
    fn skills_list_reports_only_packaged_skill_metadata() {
        let cli = Cli::try_parse_from(["jira", "--json", "skills", "list"]).unwrap();
        assert!(cli.json);
        assert!(matches!(
            cli.command,
            Command::Skills {
                command: SkillsCommand::List
            }
        ));

        let catalog = packaged_skills();
        assert_eq!(catalog["skills"][0]["name"], "jira-rust-candidate");
        assert_eq!(catalog["skills"][0]["distribution"], "packaged");
        assert_eq!(
            catalog["skills"][0]["install_command"],
            "jira install-skill"
        );
        assert!(catalog["skills"][0].get("available").is_none());
    }

    #[test]
    fn dry_run_reports_required_fields_and_does_not_write() {
        let (server, captured, thread) = fixture(vec![
            (
                200,
                json!({"issueTypes": [{"id": "1", "name": "Bug"}], "total": 1}),
            ),
            (200, create_fixture_fields()),
        ]);
        let mut client = configured_client(server);
        let error = create(&mut client, &args(None, true)).unwrap_err();
        assert!(error
            .to_string()
            .contains("Impact Details (customfield_10010)"));
        thread.join().unwrap();
        assert_eq!(captured.lock().unwrap().len(), 2);
    }

    #[test]
    fn dry_run_validates_and_converts_textarea_without_posting() {
        let (server, captured, thread) = fixture(vec![
            (
                200,
                json!({"issueTypes": [{"id": "1", "name": "Bug"}], "total": 1}),
            ),
            (200, create_fixture_fields()),
        ]);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fields.json");
        fs::write(
            &path,
            r#"{"customfield_10010":"**Orders** blocked","customfield_10011":{"id":"2"}}"#,
        )
        .unwrap();
        let mut client = configured_client(server);
        let preview = create(&mut client, &args(Some(path), true)).unwrap();
        assert_eq!(
            preview["request"]["fields"]["customfield_10010"]["type"],
            "doc"
        );
        assert_eq!(
            preview["request"]["fields"]["customfield_10010"]["content"][0]["content"][0]["marks"]
                [0]["type"],
            "strong"
        );
        thread.join().unwrap();
        assert_eq!(captured.lock().unwrap().len(), 2);
    }

    #[test]
    fn invalid_severity_option_does_not_write() {
        let (server, captured, thread) = fixture(vec![
            (
                200,
                json!({"issueTypes": [{"id": "1", "name": "Bug"}], "total": 1}),
            ),
            (200, create_fixture_fields()),
        ]);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fields.json");
        fs::write(
            &path,
            r#"{"customfield_10010":"Orders blocked","customfield_10011":{"id":"999"}}"#,
        )
        .unwrap();
        let mut client = configured_client(server);
        let error = create(&mut client, &args(Some(path), false)).unwrap_err();
        assert!(error.to_string().contains("Invalid option"));
        thread.join().unwrap();
        assert_eq!(captured.lock().unwrap().len(), 2);
    }

    #[test]
    fn confirmed_create_posts_exactly_once() {
        let (server, captured, thread) = fixture(vec![
            (
                200,
                json!({"issueTypes": [{"id": "1", "name": "Bug"}], "total": 1}),
            ),
            (200, create_fixture_fields()),
            (201, json!({"key": "TEST-1"})),
        ]);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fields.json");
        fs::write(
            &path,
            r#"{"customfield_10010":"Orders blocked","customfield_10011":{"id":"2"}}"#,
        )
        .unwrap();
        let mut client = configured_client(server);
        let result = create(&mut client, &args(Some(path), false)).unwrap();
        assert_eq!(result["key"], "TEST-1");
        thread.join().unwrap();
        let requests = captured.lock().unwrap();
        assert_eq!(requests.len(), 3);
        assert_eq!(
            requests
                .iter()
                .filter(|request| request.starts_with("POST /rest/api/3/issue "))
                .count(),
            1
        );
    }

    #[test]
    fn rejected_write_is_sent_once_and_cannot_claim_success() {
        let (server, captured, thread) =
            fixture(vec![(503, json!({"errorMessages": ["Unavailable"]}))]);
        let client = configured_client(server);
        let error = client
            .create(&json!({"fields": {"summary": "Example"}}))
            .unwrap_err();
        assert!(error.to_string().contains("503"));
        thread.join().unwrap();
        assert_eq!(captured.lock().unwrap().len(), 1);
    }

    #[test]
    fn v2_create_uses_plain_text_for_impact_details() {
        let (server, captured, thread) = fixture(vec![
            (
                200,
                json!({"projects": [{"key": "TEST", "issuetypes": [{"id": "1", "name": "Bug", "fields": {
                    "summary": {"name": "Summary", "required": true},
                    "customfield_10010": {"name": "Impact Details", "required": true, "schema": {"type": "string", "custom": "jira:textarea"}}
                }}]}]}),
            ),
            (201, json!({"key": "TEST-1"})),
        ]);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fields.json");
        fs::write(&path, r#"{"customfield_10010":"Orders blocked"}"#).unwrap();
        let mut client = Jira::new(config::Profile {
            server,
            auth_type: "bearer".into(),
            token: "test-token".into(),
            api_version: "2".into(),
            ..Default::default()
        })
        .unwrap();
        let created = create(&mut client, &args(Some(path), false)).unwrap();
        assert_eq!(created["key"], "TEST-1");
        thread.join().unwrap();
        let requests = captured.lock().unwrap();
        assert!(requests[1].starts_with("POST /rest/api/2/issue "));
        assert!(requests[1].contains("Orders blocked"));
        assert!(!requests[1].contains("\\\"type\\\":\\\"doc\\\""));
    }

    #[test]
    fn remote_link_update_preserves_unmodified_properties() {
        let (server, captured, thread) = fixture(vec![
            (
                200,
                json!({"id": 42, "self": "https://jira.example/rest/api/3/issue/TEST-1/remotelink/42",
                "relationship": "relates to", "application": {"type": "app"},
                "object": {"url": "https://example.com", "title": "Old", "summary": "Keep"}}),
            ),
            (204, Value::Null),
        ]);
        let jira = configured_client(server);
        let result = remote_link(
            &jira,
            &RemoteLinkCommand::Update {
                key: "TEST-1".into(),
                id: "42".into(),
                url: None,
                title: Some("New".into()),
                summary: None,
                relationship: None,
                icon_url: None,
                icon_title: None,
            },
        )
        .unwrap();
        assert_eq!(result["updated"], true);
        thread.join().unwrap();
        let requests = captured.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert!(requests[1].starts_with("PUT /rest/api/3/issue/TEST-1/remotelink/42 "));
        assert!(requests[1].contains("\"title\":\"New\""));
        assert!(requests[1].contains("\"summary\":\"Keep\""));
        assert!(requests[1].contains("\"application\":{\"type\":\"app\"}"));
    }

    #[test]
    fn list_filters_escape_values_and_search_uses_client_pagination() {
        let cli = Cli::try_parse_from([
            "jira",
            "issue",
            "list",
            "--project",
            "A\" OR B",
            "--assignee",
            "currentUser",
            "--limit",
            "1",
        ])
        .unwrap();
        let Command::Issue { command } = cli.command else {
            panic!("expected issue");
        };
        let IssueCommand::List(args) = *command else {
            panic!("expected list");
        };
        let jql = list_jql(&args).unwrap();
        assert_eq!(jql, "project = \"A\\\" OR B\" AND assignee = currentUser()");
        let (server, captured, thread) = fixture(vec![(
            200,
            json!({"issues": [{"key": "TEST-1"}], "isLast": true}),
        )]);
        let mut jira = configured_client(server);
        assert_eq!(
            jira.search_issues(&jql, args.limit).unwrap()["issues"][0]["key"],
            "TEST-1"
        );
        thread.join().unwrap();
        assert!(captured.lock().unwrap()[0].starts_with("GET /rest/api/3/search/jql?jql="));
    }

    #[test]
    fn list_preserves_trailing_order_by_outside_quoted_jql() {
        let cli = Cli::try_parse_from([
            "jira",
            "issue",
            "list",
            "--project",
            "TEST",
            "--jql",
            "summary ~ \"ORDER BY\" ORDER BY updated DESC",
        ])
        .unwrap();
        let Command::Issue { command } = cli.command else {
            panic!("expected issue");
        };
        let IssueCommand::List(args) = *command else {
            panic!("expected list");
        };
        assert_eq!(
            list_jql(&args).unwrap(),
            "project = \"TEST\" AND (summary ~ \"ORDER BY\") ORDER BY updated DESC"
        );
    }

    #[test]
    fn create_flags_are_included_and_checked_against_screen() {
        let metadata = json!({"fields": [
            {"fieldId": "summary", "name": "Summary", "required": true},
            {"fieldId": "assignee", "schema": {"type": "user"}},
            {"fieldId": "priority", "schema": {"type": "priority"}, "allowedValues": [{"name": "High"}]},
            {"fieldId": "labels", "schema": {"type": "array"}},
            {"fieldId": "parent", "schema": {"type": "issuelink"}},
            {"fieldId": "duedate", "schema": {"type": "date"}}
        ], "total": 6});
        let (server, captured, thread) = fixture(vec![
            (
                200,
                json!({"issueTypes": [{"id": "1", "name": "Task"}], "total": 1}),
            ),
            (200, metadata),
        ]);
        let mut jira = configured_client(server);
        let args = CreateArgs {
            issue_type: "Task".into(),
            assignee: Some("account-1".into()),
            priority: Some("High".into()),
            labels: vec!["one".into(), "two".into()],
            parent: Some("TEST-10".into()),
            due_date: Some("2026-02-28".into()),
            ..args(None, true)
        };
        let preview = create(&mut jira, &args).unwrap();
        let fields = &preview["request"]["fields"];
        assert_eq!(fields["assignee"], json!({"accountId": "account-1"}));
        assert_eq!(fields["priority"], json!({"name": "High"}));
        assert_eq!(fields["labels"], json!(["one", "two"]));
        assert_eq!(fields["parent"], json!({"key": "TEST-10"}));
        assert_eq!(fields["duedate"], "2026-02-28");
        thread.join().unwrap();
        assert_eq!(captured.lock().unwrap().len(), 2);
        assert!(!valid_date("2026-02-29"));
    }

    #[test]
    fn edit_fields_file_checks_screen_before_writing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("edit.json");
        fs::write(&path, r#"{"customfield_10010":"New text"}"#).unwrap();
        let (server, captured, thread) = fixture(vec![
            (
                200,
                json!({"fields": {
                    "customfield_10010": {"schema": {"type": "string", "custom": "jira:textarea"}}
                }}),
            ),
            (204, Value::Null),
        ]);
        let command = IssueCommand::Edit {
            key: "TEST-1".into(),
            summary: None,
            description: None,
            description_file: None,
            fields_file: Some(path),
            assignee: None,
            priority: None,
        };
        issue(&command, configured_client(server), true).unwrap();
        thread.join().unwrap();
        let requests = captured.lock().unwrap();
        assert!(requests[0].starts_with("GET /rest/api/3/issue/TEST-1/editmeta "));
        assert!(requests[1].contains("\"customfield_10010\":{\"content\":"));
    }

    #[test]
    fn edit_refuses_fields_missing_from_edit_screen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("edit.json");
        fs::write(&path, r#"{"customfield_10010":"Not editable"}"#).unwrap();
        let (server, captured, thread) = fixture(vec![(200, json!({"fields": {}}))]);
        let command = IssueCommand::Edit {
            key: "TEST-1".into(),
            summary: None,
            description: None,
            description_file: None,
            fields_file: Some(path),
            assignee: None,
            priority: None,
        };
        let error = issue(&command, configured_client(server), true).unwrap_err();
        assert!(error.to_string().contains("not on the edit screen"));
        thread.join().unwrap();
        assert_eq!(captured.lock().unwrap().len(), 1);
    }

    #[test]
    fn edit_metadata_fallback_sends_v2_plain_text_description_and_username() {
        let (server, captured, thread) = fixture(vec![
            (404, json!({})),
            (
                200,
                json!({"fields": {
                    "description": {"schema": {"type": "string"}},
                    "assignee": {"schema": {"type": "user"}}
                }}),
            ),
            (204, Value::Null),
        ]);
        let jira = Jira::new(config::Profile {
            server,
            auth_type: "bearer".into(),
            token: "test-token".into(),
            api_version: "auto".into(),
            ..Default::default()
        })
        .unwrap();
        issue(
            &IssueCommand::Edit {
                key: "TEST-1".into(),
                summary: None,
                description: Some("**Plain** text".into()),
                description_file: None,
                fields_file: None,
                assignee: Some("john".into()),
                priority: None,
            },
            jira,
            true,
        )
        .unwrap();
        thread.join().unwrap();
        let requests = captured.lock().unwrap();
        assert!(requests[0].starts_with("GET /rest/api/3/issue/TEST-1/editmeta "));
        assert!(requests[1].starts_with("GET /rest/api/2/issue/TEST-1/editmeta "));
        assert!(requests[2].starts_with("PUT /rest/api/2/issue/TEST-1 "));
        assert!(
            requests[2].contains("\"description\":\"Plain text\""),
            "{}",
            requests[2]
        );
        assert!(requests[2].contains("\"assignee\":{\"name\":\"john\"}"));
    }

    #[test]
    fn internal_comment_uses_service_desk_endpoint() {
        let (server, captured, thread) = fixture(vec![(201, json!({"id": "10", "public": false}))]);
        let response = comment(
            &configured_client(server),
            &CommentCommand::Add {
                key: "TEST-1".into(),
                text: Some("private".into()),
                file: None,
                internal: true,
            },
        )
        .unwrap();
        assert_eq!(response["public"], false);
        thread.join().unwrap();
        let requests = captured.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].starts_with("POST /rest/servicedeskapi/request/TEST-1/comment "));
        assert!(requests[0].contains("\"body\":\"private\""));
        assert!(requests[0].contains("\"public\":false"));
    }

    #[test]
    fn internal_comment_flag_parses_for_add() {
        let cli = Cli::try_parse_from([
            "jira",
            "issue",
            "comment",
            "add",
            "TEST-1",
            "private",
            "--internal",
        ])
        .unwrap();
        let Command::Issue { command } = cli.command else {
            panic!("expected issue");
        };
        assert!(matches!(
            *command,
            IssueCommand::Comment {
                command: CommentCommand::Add { internal: true, .. }
            }
        ));
    }

    #[test]
    fn internal_comment_from_file_remains_plain_text_and_public_add_is_unchanged() {
        let (server, captured, thread) = fixture(vec![
            (201, json!({"public": false, "id": "10"})),
            (201, json!({"id": "11"})),
        ]);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("note.txt");
        fs::write(&path, "**internal** [note](https://example.com)").unwrap();
        let client = configured_client(server);
        assert_eq!(
            comment(
                &client,
                &CommentCommand::Add {
                    key: "TEST-1".into(),
                    text: None,
                    file: Some(path),
                    internal: true,
                },
            )
            .unwrap()["public"],
            false
        );
        assert_eq!(
            comment(
                &client,
                &CommentCommand::Add {
                    key: "TEST-1".into(),
                    text: Some("**public** note".into()),
                    file: None,
                    internal: false,
                },
            )
            .unwrap()["id"],
            "11"
        );
        thread.join().unwrap();
        let requests = captured.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert!(requests[0].starts_with("POST /rest/servicedeskapi/request/TEST-1/comment "));
        assert!(requests[0].contains("\"body\":\"internal note (https://example.com)\""));
        assert!(requests[1].starts_with("POST /rest/api/3/issue/TEST-1/comment "));
        assert!(requests[1].contains("\"type\":\"doc\""));
        assert!(requests[1].contains("\"type\":\"strong\""));
    }

    #[test]
    fn internal_comment_without_text_does_not_post() {
        let (server, captured, thread) = fixture(vec![]);
        let error = comment(
            &configured_client(server),
            &CommentCommand::Add {
                key: "TEST-1".into(),
                text: None,
                file: None,
                internal: true,
            },
        )
        .unwrap_err();
        assert!(error
            .to_string()
            .contains("Comment text or --file is required"));
        thread.join().unwrap();
        assert!(captured.lock().unwrap().is_empty());
    }

    #[test]
    fn sprint_without_board_selects_single_board() {
        let (server, captured, thread) = fixture(vec![
            (
                200,
                json!({"values": [{"id": 8, "name": "Team"}], "isLast": true}),
            ),
            (200, json!({"values": [{"id": 3, "state": "active"}]})),
        ]);
        let result = sprints(&configured_client(server), None, Some("active")).unwrap();
        assert_eq!(result["values"][0]["id"], 3);
        thread.join().unwrap();
        assert!(captured.lock().unwrap()[1]
            .starts_with("GET /rest/agile/1.0/board/8/sprint?state=active "));
    }

    #[test]
    fn sprint_refuses_to_choose_between_multiple_boards() {
        let (server, captured, thread) = fixture(vec![(
            200,
            json!({"values": [{"id": 8}, {"id": 9}], "isLast": true}),
        )]);
        assert!(sprints(&configured_client(server), None, None)
            .unwrap_err()
            .to_string()
            .contains("Multiple boards"));
        thread.join().unwrap();
        assert_eq!(captured.lock().unwrap().len(), 1);
    }

    #[test]
    fn install_skill_refuses_overwrite_and_uses_packaged_text() {
        let directory = tempfile::tempdir().unwrap();
        let dest = directory.path().join("skills/jira");
        let path = install_skill(&dest, false).unwrap();
        assert!(fs::read_to_string(&path).unwrap().contains("Jira"));
        assert!(install_skill(&dest, false).is_err());
        install_skill(&dest, true).unwrap();
    }

    #[test]
    fn install_skill_defaults_to_rust_specific_skill_directory() {
        let cli = Cli::try_parse_from(["jira", "install-skill"]).unwrap();
        let Command::InstallSkill { dest, .. } = cli.command else {
            panic!("expected install-skill command");
        };
        assert_eq!(dest, PathBuf::from("./.claude/skills/jira-rust-candidate"));
    }

    #[test]
    fn shell_completions_include_cli_commands() {
        for shell in [
            Shell::Bash,
            Shell::Elvish,
            Shell::Fish,
            Shell::PowerShell,
            Shell::Zsh,
        ] {
            let mut command = Cli::command();
            let mut output = Vec::new();
            generate(shell, &mut command, "jira", &mut output);
            let output = String::from_utf8(output).unwrap();
            assert!(!output.is_empty(), "{shell:?} completion output is empty");
            assert!(
                output.contains("issue"),
                "{shell:?} completion lacks commands"
            );
        }
    }

    #[test]
    fn issue_markdown_renders_adf_and_rejects_unknown_description() {
        let issue = json!({"key": "TEST-1", "fields": {"summary": "Task", "description": markdown_to_adf("Hello **bold** and [link](https://example.com)\nworld")}});
        let output = issue_markdown(&issue, "https://jira.example").unwrap();
        assert!(output.contains("Hello **bold** and [link](https://example.com)\nworld"));
        assert!(output.contains("https://jira.example/browse/TEST-1"));
        let invalid = json!({"key": "TEST-1", "fields": {"summary": "Task", "description": {"unknown": true}}});
        assert!(issue_markdown(&invalid, "https://jira.example").is_err());
    }

    #[test]
    fn adf_description_markdown_preserves_lists_and_code_blocks() {
        let description = json!({"type":"doc", "version":1, "content":[
            {"type":"bulletList", "content":[
                {"type":"listItem", "content":[{"type":"paragraph", "content":[{"type":"text", "text":"first"}]}]},
                {"type":"listItem", "content":[{"type":"paragraph", "content":[{"type":"text", "text":"second"}]}]}
            ]},
            {"type":"codeBlock", "attrs":{"language":"rust"}, "content":[{"type":"text", "text":"let x = 1;"}]}
        ]});
        let issue = json!({"key":"TEST-1", "fields":{"summary":"Task", "description":description}});
        let markdown = issue_markdown(&issue, "https://jira.example").unwrap();
        assert!(markdown.contains("- first\n- second"));
        assert!(markdown.contains("```rust\nlet x = 1;\n```"));
    }

    #[test]
    fn comment_list_markdown_renders_adf_bodies() {
        let response = json!({"startAt": 0, "comments": [{
            "id": "7",
            "body": markdown_to_adf("**Rich** comment with [link](https://example.com)")
        }]});
        let rendered = comments_markdown(&response).unwrap();
        assert_eq!(
            rendered["comments"][0]["body"],
            "**Rich** comment with [link](https://example.com)"
        );
        assert_eq!(rendered["startAt"], 0);
    }

    #[test]
    fn remote_link_add_includes_optional_properties() {
        let (server, captured, thread) = fixture(vec![(201, json!({"id": 8}))]);
        let result = remote_link(
            &configured_client(server),
            &RemoteLinkCommand::Add {
                key: "TEST-1".into(),
                url: "https://example.com".into(),
                title: "Document".into(),
                global_id: Some("doc-8".into()),
                summary: Some("Spec".into()),
                relationship: Some("references".into()),
                icon_url: Some("https://example.com/icon.png".into()),
                icon_title: Some("icon".into()),
            },
        )
        .unwrap();
        assert_eq!(result["id"], 8);
        thread.join().unwrap();
        let request = &captured.lock().unwrap()[0];
        assert!(request.contains("\"globalId\":\"doc-8\""));
        assert!(request.contains("\"summary\":\"Spec\""));
        assert!(request.contains("\"relationship\":\"references\""));
        assert!(request.contains("\"url16x16\":\"https://example.com/icon.png\""));
    }
}

use anyhow::{bail, Context, Result};
use clap::ValueEnum;
use serde::Serialize;
use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const BUNDLED_SKILL: &str = include_str!("../plugins/jira/skills/jira/SKILL.md");
const SKILL_NAME: &str = "jira";
const MAX_SKILL_BYTES: u64 = 256 * 1024;
const MAX_DEPTH: usize = 8;
static NEXT_TEMPORARY: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Serialize)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub path: Option<PathBuf>,
    pub bundled: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum SkillTarget {
    Claude,
    Opencode,
    Agents,
}

impl SkillTarget {
    fn relative_dir(self) -> &'static str {
        match self {
            Self::Claude => ".claude/skills",
            Self::Opencode => ".config/opencode/skills",
            Self::Agents => ".agents/skills",
        }
    }
}

pub fn bundled_text() -> &'static str {
    BUNDLED_SKILL
}

pub fn discover() -> Result<Vec<Skill>> {
    discover_from_roots(&discovery_roots())
}

fn discover_from_roots(explicit_roots: &[PathBuf]) -> Result<Vec<Skill>> {
    let (name, description) = parse_frontmatter(BUNDLED_SKILL);
    let mut skills = vec![Skill {
        name: name.unwrap_or_else(|| SKILL_NAME.to_owned()),
        description: description.unwrap_or_default(),
        path: None,
        bundled: true,
    }];

    let mut roots = explicit_roots.to_vec();
    roots.sort();
    let mut visited = HashSet::new();
    let mut seen_paths = HashSet::new();
    for root in roots {
        discover_under(&root, 0, &mut visited, &mut seen_paths, &mut skills)?;
    }
    skills.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then_with(|| left.path.cmp(&right.path))
            .then_with(|| right.bundled.cmp(&left.bundled))
    });
    Ok(skills)
}

fn discovery_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(cwd) = env::current_dir() {
        roots.extend([
            cwd.join(".agents/skills"),
            cwd.join(".claude/skills"),
            cwd.join(".opencode/skills"),
        ]);
    }
    if let Some(home) = home_dir() {
        roots.extend([
            home.join(".agents/skills"),
            home.join(".claude/skills"),
            home.join(".config/opencode/skills"),
            home.join(".codex/skills"),
            home.join(".tabnine/agent/skills"),
        ]);
    }
    if let Some(extra) = env::var_os("JIRA_SKILLS_PATH") {
        roots.extend(env::split_paths(&extra));
    }
    roots
}

fn home_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        env::var_os("USERPROFILE").map(PathBuf::from)
    }
    #[cfg(not(windows))]
    {
        env::var_os("HOME").map(PathBuf::from)
    }
}

fn discover_under(
    root: &Path,
    depth: usize,
    visited: &mut HashSet<PathBuf>,
    seen_paths: &mut HashSet<PathBuf>,
    skills: &mut Vec<Skill>,
) -> Result<()> {
    if depth > MAX_DEPTH {
        return Ok(());
    }
    let Ok(canonical) = fs::canonicalize(root) else {
        return Ok(());
    };
    if !visited.insert(canonical.clone()) {
        return Ok(());
    }

    let skill_file = canonical.join("SKILL.md");
    if let Ok(metadata) = fs::symlink_metadata(&skill_file) {
        if metadata.file_type().is_file()
            && metadata.len() <= MAX_SKILL_BYTES
            && seen_paths.insert(skill_file.clone())
        {
            let contents = fs::read_to_string(&skill_file)
                .with_context(|| format!("reading skill file {}", skill_file.display()))?;
            let (name, description) = parse_frontmatter(&contents);
            let fallback_name = skill_file
                .parent()
                .and_then(Path::file_name)
                .and_then(|name| name.to_str())
                .unwrap_or("unnamed-skill");
            skills.push(Skill {
                name: name.unwrap_or_else(|| fallback_name.to_owned()),
                description: description.unwrap_or_default(),
                path: Some(skill_file),
                bundled: false,
            });
        }
        // A skill directory owns its subtree; avoid double-counting nested skills.
        return Ok(());
    }

    let Ok(entries) = fs::read_dir(canonical) else {
        return Ok(());
    };
    for entry in entries.flatten() {
        if entry.file_type().is_ok_and(|file_type| file_type.is_dir()) {
            discover_under(&entry.path(), depth + 1, visited, seen_paths, skills)?;
        }
    }
    Ok(())
}

fn parse_frontmatter(contents: &str) -> (Option<String>, Option<String>) {
    let mut lines = contents.lines();
    if lines.next().map(str::trim) != Some("---") {
        return (None, None);
    }
    let mut name = None;
    let mut description = None;
    let mut lines = lines.peekable();
    while let Some(line) = lines.next() {
        let line = line.trim();
        if line == "---" {
            break;
        }
        if let Some((key, value)) = line.split_once(':') {
            let raw_value = value.trim();
            match key.trim() {
                "name" if !raw_value.is_empty() => {
                    name = Some(unquote(raw_value).to_owned());
                }
                "description" if matches!(raw_value, ">" | ">-" | ">+" | "|" | "|-" | "|+") => {
                    let mut paragraphs = Vec::new();
                    while let Some(next) = lines.peek() {
                        if next.trim().is_empty() {
                            lines.next();
                            if !paragraphs.is_empty() {
                                paragraphs.push(String::new());
                            }
                            continue;
                        }
                        if !next.starts_with(char::is_whitespace) {
                            break;
                        }
                        paragraphs.push(lines.next().unwrap().trim().to_owned());
                    }
                    let folded = raw_value.starts_with('>');
                    let paragraphs = paragraphs
                        .split(String::is_empty)
                        .map(|paragraph| {
                            if folded {
                                paragraph.join(" ")
                            } else {
                                paragraph.join("\n")
                            }
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    if !paragraphs.trim().is_empty() {
                        description = Some(paragraphs.trim().to_owned());
                    }
                }
                "description" if !raw_value.is_empty() => {
                    description = Some(unquote(raw_value).to_owned());
                }
                _ => {}
            }
        }
    }
    (name, description)
}

fn unquote(value: &str) -> &str {
    let bytes = value.as_bytes();
    if value.len() >= 2
        && ((bytes[0] == b'"' && bytes[value.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[value.len() - 1] == b'\''))
    {
        &value[1..value.len() - 1]
    } else {
        value
    }
}

pub fn install_bundled(targets: &[SkillTarget], force: bool) -> Result<Vec<PathBuf>> {
    let targets = if targets.is_empty() {
        vec![
            SkillTarget::Claude,
            SkillTarget::Opencode,
            SkillTarget::Agents,
        ]
    } else {
        targets.to_vec()
    };
    let Some(home) = home_dir() else {
        bail!("Cannot install agent skills because the home directory is unknown");
    };
    let mut installed = Vec::new();
    for target in targets {
        let directory = home.join(target.relative_dir()).join(SKILL_NAME);
        installed.push(write_skill(&directory, force)?);
    }
    Ok(installed)
}

pub fn install_to_directory(directory: &Path, force: bool) -> Result<PathBuf> {
    let directory = expand_home(directory)?;
    write_skill(&directory, force)
}

fn expand_home(path: &Path) -> Result<PathBuf> {
    if path == Path::new("~") || path.starts_with("~/") {
        let home = home_dir().context("Cannot expand ~ because the home directory is unknown")?;
        Ok(home.join(path.strip_prefix("~")?))
    } else {
        Ok(path.to_path_buf())
    }
}

fn write_skill(directory: &Path, force: bool) -> Result<PathBuf> {
    let path = directory.join("SKILL.md");
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
    fs::create_dir_all(directory)
        .with_context(|| format!("Cannot create {}", directory.display()))?;

    if force {
        let (mut temporary, temp_path) = tempfile_path(directory)?;
        if let Err(error) = std::io::Write::write_all(&mut temporary, BUNDLED_SKILL.as_bytes()) {
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
        if let Err(error) = std::io::Write::write_all(&mut file, BUNDLED_SKILL.as_bytes()) {
            drop(file);
            let _ = fs::remove_file(&path);
            return Err(error.into());
        }
    }
    Ok(path)
}

fn tempfile_path(directory: &Path) -> Result<(fs::File, PathBuf)> {
    for _ in 0..1000 {
        let index = NEXT_TEMPORARY.fetch_add(1, Ordering::Relaxed);
        let path = directory.join(format!(".SKILL.md.{}.{}.tmp", std::process::id(), index));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_skill_is_discoverable_without_jira_configuration() {
        let skills = discover().unwrap();
        assert!(skills
            .iter()
            .any(|skill| skill.name == "jira" && skill.bundled));
    }

    #[test]
    fn discovers_local_skills_from_explicit_roots() {
        let directory = tempfile::tempdir().unwrap();
        let skill_dir = directory.path().join("review-code");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: review-code\ndescription: Review local changes\n---\n# Review\n",
        )
        .unwrap();

        let skills = discover_from_roots(&[directory.path().to_path_buf()]).unwrap();
        let discovered = skills
            .iter()
            .find(|skill| skill.name == "review-code")
            .unwrap();
        let expected_path = fs::canonicalize(skill_dir.join("SKILL.md")).unwrap();
        assert!(!discovered.bundled);
        assert_eq!(discovered.description, "Review local changes");
        assert_eq!(discovered.path.as_deref(), Some(expected_path.as_path()));
    }

    #[test]
    fn parses_skill_frontmatter_name_and_description() {
        let (name, description) =
            parse_frontmatter("---\nname: sample\ndescription: A sample skill\n---\n# Body\n");
        assert_eq!(name.as_deref(), Some("sample"));
        assert_eq!(description.as_deref(), Some("A sample skill"));
    }

    #[test]
    fn parses_folded_multiline_description() {
        let (_, description) = parse_frontmatter(
            "---\nname: sample\ndescription: >\n  First line\n  second line\n\n  Another paragraph\n---\n",
        );
        assert_eq!(
            description.as_deref(),
            Some("First line second line\nAnother paragraph")
        );
    }

    #[test]
    fn install_copies_bundled_skill_and_requires_force_to_replace() {
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join(".agents/skills/jira");
        let path = write_skill(&destination, false).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), BUNDLED_SKILL);
        assert!(write_skill(&destination, false).is_err());
        write_skill(&destination, true).unwrap();
    }
}

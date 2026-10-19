use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT_TEMPORARY: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    #[serde(default)]
    pub server: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub token: String,
    #[serde(default)]
    pub cloud_id: String,
    #[serde(default)]
    pub cookie: String,
    #[serde(default)]
    pub auth_type: String,
    #[serde(default)]
    pub api_version: String,
    #[serde(default)]
    pub tls_client_cert: String,
    #[serde(default)]
    pub tls_client_key: String,
    #[serde(default)]
    pub tls_ca_cert: String,
    #[serde(flatten)]
    pub other: BTreeMap<String, serde_json::Value>,
}

impl Profile {
    pub fn redacted(&self) -> serde_json::Value {
        let server = safe_server_for_display(&self.server);
        serde_json::json!({
            "server": server, "username": self.username, "cloudId": self.cloud_id,
            "authType": self.auth_type, "apiVersion": self.api_version,
            "tokenConfigured": !self.token.is_empty(), "cookieConfigured": !self.cookie.is_empty(),
            "tlsClientCert": self.tls_client_cert, "tlsClientKeyConfigured": !self.tls_client_key.is_empty(),
            "tlsCaCert": self.tls_ca_cert
        })
    }

    pub fn resolve(mut self) -> Result<Self> {
        if let Ok(value) = env::var("JIRA_HOST") {
            self.server = value;
        } else if let Ok(value) = env::var("JIRA_DOMAIN") {
            self.server = value;
        }
        if let Ok(value) = env::var("JIRA_USERNAME") {
            self.username = value;
        }
        if let Ok(value) = env::var("JIRA_API_TOKEN") {
            self.token = value;
        }
        if let Ok(value) = env::var("JIRA_CLOUD_ID") {
            self.cloud_id = value;
        }
        if let Ok(value) = env::var("JIRA_COOKIE") {
            self.cookie = value;
        }
        if let Ok(value) = env::var("JIRA_AUTH_TYPE") {
            self.auth_type = value;
        }
        if let Ok(value) = env::var("JIRA_API_VERSION") {
            self.api_version = value;
        }
        if let Ok(value) = env::var("JIRA_TLS_CLIENT_CERT") {
            self.tls_client_cert = value;
        }
        if let Ok(value) = env::var("JIRA_TLS_CLIENT_KEY") {
            self.tls_client_key = value;
        }
        if let Ok(value) = env::var("JIRA_TLS_CA_CERT") {
            self.tls_ca_cert = value;
        }
        if self.server.is_empty() {
            bail!(
                "Jira server is not configured. Use `jira config set server <URL>` or JIRA_HOST."
            );
        }
        self.server = validate_server(&self.server)?;
        if !self.cloud_id.is_empty()
            && !self
                .cloud_id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-')
        {
            bail!("Cloud ID must contain only letters, numbers, and hyphens");
        }
        if self.auth_type.is_empty() {
            self.auth_type = if self.username.is_empty() {
                "bearer"
            } else {
                "basic"
            }
            .into();
        }
        match self.auth_type.as_str() {
            "basic" if self.username.is_empty() || self.token.is_empty() => {
                bail!("Basic authentication needs a username and token")
            }
            "bearer" if self.token.is_empty() => bail!("Bearer authentication needs a token"),
            "cookie" if self.cookie.is_empty() => bail!("Cookie authentication needs a cookie"),
            "mtls" if self.tls_client_cert.is_empty() || self.tls_client_key.is_empty() => {
                bail!("mTLS needs a client certificate and key")
            }
            "basic" | "bearer" | "cookie" | "mtls" => {}
            other => bail!("Unsupported auth type {other}"),
        }
        if self.api_version.is_empty() {
            self.api_version = "auto".into();
        }
        if !["auto", "2", "3"].contains(&self.api_version.as_str()) {
            bail!("API version must be auto, 2, or 3");
        }
        Ok(self)
    }
}

fn validate_server(value: &str) -> Result<String> {
    let url = if value.starts_with("http://") || value.starts_with("https://") {
        value.to_owned()
    } else {
        format!("https://{value}")
    };
    let parsed = reqwest::Url::parse(&url).context("Invalid Jira server URL")?;
    if !["http", "https"].contains(&parsed.scheme())
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        bail!("Jira server must be an http(s) URL without credentials, query or fragment");
    }
    Ok(url.trim_end_matches('/').to_owned())
}

fn safe_server_for_display(server: &str) -> String {
    validate_server(server).unwrap_or_else(|_| "(invalid or unsafe server URL)".into())
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Store {
    pub active_profile: Option<String>,
    #[serde(default)]
    pub profiles: BTreeMap<String, Profile>,
    #[serde(flatten)]
    pub other: BTreeMap<String, serde_json::Value>,
}

pub fn path() -> PathBuf {
    env::var_os("JIRA_CLI_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(
                env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).unwrap_or_default(),
            )
            .join(".jira-cli")
        })
        .join("config.json")
}

fn legacy_path_for(home: &Path, xdg: Option<&Path>, app_data: Option<&Path>, os: &str) -> PathBuf {
    let directory = match os {
        "macos" => home.join("Library/Preferences/jira-cli-nodejs"),
        "windows" => app_data
            .map(Path::to_path_buf)
            .unwrap_or_else(|| home.join("AppData/Roaming"))
            .join("jira-cli-nodejs/Config"),
        _ => xdg
            .map(Path::to_path_buf)
            .unwrap_or_else(|| home.join(".config"))
            .join("jira-cli-nodejs"),
    };
    directory.join("config.json")
}

fn legacy_path() -> PathBuf {
    let home = PathBuf::from(
        env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).unwrap_or_default(),
    );
    let xdg = env::var_os("XDG_CONFIG_HOME").map(PathBuf::from);
    let app_data = env::var_os("APPDATA").map(PathBuf::from);
    legacy_path_for(&home, xdg.as_deref(), app_data.as_deref(), env::consts::OS)
}

fn read_store(destination: &Path) -> Result<Store> {
    let bytes = fs::read(destination)
        .with_context(|| format!("Cannot read config at {}", destination.display()))?;
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|_| anyhow::anyhow!("Invalid config at {}", destination.display()))?;
    if !value
        .get("profiles")
        .is_some_and(serde_json::Value::is_object)
    {
        bail!(
            "Invalid config at {}: missing profiles",
            destination.display()
        );
    }
    serde_json::from_value(value)
        .map_err(|_| anyhow::anyhow!("Invalid config at {}", destination.display()))
}

fn load_from_paths(destination: &Path, legacy: &Path) -> Result<Store> {
    match fs::metadata(destination) {
        Ok(_) => return read_store(destination),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error).with_context(|| format!("Cannot read {}", destination.display()))
        }
    }
    let bytes = match fs::read(legacy) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Store::default()),
        Err(error) => {
            return Err(error).with_context(|| format!("Cannot read {}", legacy.display()))
        }
    };
    let Ok(serde_json::Value::Object(fields)) = serde_json::from_slice(&bytes) else {
        return Ok(Store::default());
    };
    if fields.is_empty() {
        return Ok(Store::default());
    }
    let profile = serde_json::from_value(serde_json::Value::Object(fields))
        .map_err(|_| anyhow::anyhow!("Invalid legacy config at {}", legacy.display()))?;
    let store = Store {
        active_profile: Some("default".into()),
        profiles: BTreeMap::from([("default".into(), profile)]),
        ..Store::default()
    };
    if write_store(destination, &store, true)? {
        Ok(store)
    } else {
        read_store(destination)
    }
}

pub fn load() -> Result<Store> {
    load_from_paths(&path(), &legacy_path())
}

pub fn selected(store: &Store, requested: Option<&str>) -> String {
    requested
        .map(str::to_owned)
        .or_else(|| env::var("JIRA_PROFILE").ok())
        .or_else(|| store.active_profile.clone())
        .unwrap_or_else(|| "default".into())
}

pub fn save(store: &Store) -> Result<()> {
    write_store(&path(), store, false).map(|_| ())
}

fn write_store(destination: &Path, store: &Store, only_if_absent: bool) -> Result<bool> {
    let bytes = serde_json::to_vec_pretty(store)?;
    let directory = destination.parent().context("No config directory")?;
    fs::create_dir_all(directory).context("Cannot create config directory")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(directory, fs::Permissions::from_mode(0o700))?;
    }
    use std::io::Write;
    let (temporary, mut file) = loop {
        let temporary = directory.join(format!(
            "config.json.{}.{}.tmp",
            std::process::id(),
            NEXT_TEMPORARY.fetch_add(1, Ordering::Relaxed)
        ));
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&temporary) {
            Ok(file) => break (temporary, file),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error).context("Cannot create temporary config"),
        }
    };
    let result = (|| {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(fs::Permissions::from_mode(0o600))?;
        }
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        if only_if_absent {
            match fs::hard_link(&temporary, destination) {
                Ok(()) => Ok(true),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
                Err(error) => Err(error).context("Cannot migrate config"),
            }
        } else {
            fs::rename(&temporary, destination).context("Cannot save config")?;
            Ok(true)
        }
    })();
    let _ = fs::remove_file(temporary);
    result
}

pub fn set(profile: &mut Profile, key: &str, value: &str) -> Result<()> {
    match key {
        "server" => profile.server = validate_server(value)?,
        "username" => profile.username = value.into(),
        "token" => profile.token = value.into(),
        "cloudId" => profile.cloud_id = value.into(),
        "cookie" => profile.cookie = value.into(),
        "authType" if ["basic", "bearer", "cookie", "mtls"].contains(&value) => {
            profile.auth_type = value.into()
        }
        "apiVersion" if ["auto", "2", "3"].contains(&value) => profile.api_version = value.into(),
        "tlsClientCert" => profile.tls_client_cert = value.into(),
        "tlsClientKey" => profile.tls_client_key = value.into(),
        "tlsCaCert" => profile.tls_ca_cert = value.into(),
        "authType" | "apiVersion" => bail!("Invalid {key} value"),
        _ => bail!("Unknown config key {key}"),
    }
    Ok(())
}

pub fn unset(profile: &mut Profile, key: &str) -> Result<()> {
    let field = match key {
        "server" => &mut profile.server,
        "username" => &mut profile.username,
        "token" => &mut profile.token,
        "cloudId" => &mut profile.cloud_id,
        "cookie" => &mut profile.cookie,
        "authType" => &mut profile.auth_type,
        "apiVersion" => &mut profile.api_version,
        "tlsClientCert" => &mut profile.tls_client_cert,
        "tlsClientKey" => &mut profile.tls_client_key,
        "tlsCaCert" => &mut profile.tls_ca_cert,
        _ => bail!("Unknown config key {key}"),
    };
    field.clear();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let destination = dir.path().join("current/config.json");
        let legacy = dir.path().join("legacy/config.json");
        (dir, destination, legacy)
    }

    #[test]
    fn legacy_locations_match_conf_on_all_platforms() {
        let home = Path::new("home");
        assert_eq!(
            legacy_path_for(home, None, None, "macos"),
            home.join("Library/Preferences/jira-cli-nodejs/config.json")
        );
        assert_eq!(
            legacy_path_for(home, None, None, "linux"),
            home.join(".config/jira-cli-nodejs/config.json")
        );
        assert_eq!(
            legacy_path_for(home, Some(Path::new("xdg")), None, "linux"),
            Path::new("xdg/jira-cli-nodejs/config.json")
        );
        assert_eq!(
            legacy_path_for(home, None, None, "windows"),
            home.join("AppData/Roaming/jira-cli-nodejs/Config/config.json")
        );
        assert_eq!(
            legacy_path_for(home, None, Some(Path::new("appdata")), "windows"),
            Path::new("appdata/jira-cli-nodejs/Config/config.json")
        );
    }

    #[test]
    fn missing_or_empty_legacy_store_does_not_create_current_store() {
        let (_dir, destination, legacy) = paths();
        assert!(load_from_paths(&destination, &legacy)
            .unwrap()
            .profiles
            .is_empty());
        fs::create_dir_all(legacy.parent().unwrap()).unwrap();
        fs::write(&legacy, "{}").unwrap();
        assert!(load_from_paths(&destination, &legacy)
            .unwrap()
            .profiles
            .is_empty());
        fs::write(&legacy, "not json").unwrap();
        assert!(load_from_paths(&destination, &legacy)
            .unwrap()
            .profiles
            .is_empty());
        assert!(!destination.exists());
    }

    #[test]
    fn malformed_legacy_profile_cannot_be_silently_discarded() {
        let (_dir, destination, legacy) = paths();
        fs::create_dir_all(legacy.parent().unwrap()).unwrap();
        let original = r#"{"token":{"secret":"private-token"}}"#;
        fs::write(&legacy, original).unwrap();
        let error = load_from_paths(&destination, &legacy).unwrap_err();
        assert!(!format!("{error:#}").contains("private-token"));
        assert!(!destination.exists());
        assert_eq!(fs::read_to_string(&legacy).unwrap(), original);
    }

    #[test]
    fn first_read_migrates_legacy_once_without_changing_original() {
        let (_dir, destination, legacy) = paths();
        let original = r#"{"server":"https://jira.example","token":"private-token","apiVersion":"2","customOption":{"retain":true}}"#;
        fs::create_dir_all(legacy.parent().unwrap()).unwrap();
        fs::write(&legacy, original).unwrap();
        let migrated = load_from_paths(&destination, &legacy).unwrap();
        assert_eq!(migrated.active_profile.as_deref(), Some("default"));
        assert_eq!(migrated.profiles["default"].token, "private-token");
        assert_eq!(migrated.profiles["default"].api_version, "2");
        assert_eq!(
            migrated.profiles["default"].other["customOption"]["retain"],
            true
        );
        assert_eq!(fs::read_to_string(&legacy).unwrap(), original);
        assert_eq!(
            read_store(&destination).unwrap().profiles["default"].token,
            "private-token"
        );
        fs::write(&legacy, r#"{"token":"different"}"#).unwrap();
        assert_eq!(
            load_from_paths(&destination, &legacy).unwrap().profiles["default"].token,
            "private-token"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&destination).unwrap().permissions().mode() & 0o777,
                0o600
            );
            assert_eq!(
                fs::metadata(destination.parent().unwrap())
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
        }
    }

    #[test]
    fn existing_store_wins_and_invalid_store_is_never_overwritten() {
        let (_dir, destination, legacy) = paths();
        fs::create_dir_all(destination.parent().unwrap()).unwrap();
        fs::create_dir_all(legacy.parent().unwrap()).unwrap();
        fs::write(&legacy, r#"{"token":"legacy-secret"}"#).unwrap();
        let existing = r#"{"activeProfile":"work","profiles":{"work":{"token":"current-secret","unknown":7}},"extra":true}"#;
        fs::write(&destination, existing).unwrap();
        let loaded = load_from_paths(&destination, &legacy).unwrap();
        assert_eq!(loaded.profiles["work"].token, "current-secret");
        assert_eq!(loaded.other["extra"], true);
        assert_eq!(loaded.profiles["work"].other["unknown"], 7);
        assert_eq!(fs::read_to_string(&destination).unwrap(), existing);

        for invalid in [
            "bad json",
            "{}",
            r#"{"profiles":null}"#,
            r#"{"profiles":{"default":{"token":{"secret":"private-token"}}}}"#,
        ] {
            fs::write(&destination, invalid).unwrap();
            let error = load_from_paths(&destination, &legacy).unwrap_err();
            assert!(!format!("{error:#}").contains("legacy-secret"));
            assert!(!format!("{error:#}").contains("private-token"));
            assert_eq!(fs::read_to_string(&destination).unwrap(), invalid);
        }
    }

    #[test]
    fn write_only_if_absent_preserves_existing_store_and_cleans_up_temp_files() {
        let (_dir, destination, _legacy) = paths();
        let store = Store::default();
        assert!(write_store(&destination, &store, true).unwrap());
        let before = fs::read(&destination).unwrap();
        assert!(!write_store(&destination, &store, true).unwrap());
        assert_eq!(fs::read(&destination).unwrap(), before);
        assert_eq!(
            fs::read_dir(destination.parent().unwrap()).unwrap().count(),
            1
        );
        write_store(&destination, &store, false).unwrap();
        assert_eq!(
            fs::read_dir(destination.parent().unwrap()).unwrap().count(),
            1
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&destination).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn set_and_unset_keep_other_profile_fields_and_reject_invalid_keys() {
        let mut profile: Profile =
            serde_json::from_str(r#"{"token":"secret","customOption":{"retain":true}}"#).unwrap();
        set(&mut profile, "server", "jira.example/").unwrap();
        assert_eq!(profile.server, "https://jira.example");
        assert!(set(&mut profile, "authType", "invalid").is_err());
        assert!(unset(&mut profile, "invalid").is_err());
        unset(&mut profile, "token").unwrap();
        assert!(profile.token.is_empty());
        assert_eq!(profile.other["customOption"]["retain"], true);
    }

    #[test]
    fn unset_allows_an_existing_empty_profile() {
        let mut profile = Profile::default();
        unset(&mut profile, "token").unwrap();
        assert!(unset(&mut profile, "unknown").is_err());
    }

    #[test]
    fn writing_non_active_profile_preserves_active_and_other_profiles() {
        let (_dir, destination, _legacy) = paths();
        let mut store = Store {
            active_profile: Some("default".into()),
            ..Store::default()
        };
        store.profiles.insert(
            "default".into(),
            Profile {
                token: "keep-me".into(),
                ..Profile::default()
            },
        );
        store.profiles.insert("work".into(), Profile::default());
        let work = selected(&store, Some("work"));
        set(
            store.profiles.get_mut(&work).unwrap(),
            "token",
            "work-token",
        )
        .unwrap();
        write_store(&destination, &store, false).unwrap();
        let mut reloaded = read_store(&destination).unwrap();
        assert_eq!(reloaded.active_profile.as_deref(), Some("default"));
        assert_eq!(reloaded.profiles["default"].token, "keep-me");
        unset(reloaded.profiles.get_mut("work").unwrap(), "token").unwrap();
        assert_eq!(reloaded.profiles["default"].token, "keep-me");
        assert!(reloaded.profiles["work"].token.is_empty());
    }

    #[test]
    fn old_profile_store_is_readable_and_never_exposes_secrets() {
        let store: Store = serde_json::from_str(r#"{"activeProfile":"default","profiles":{"default":{"server":"https://jira.example","token":"secret","cookie":"session"}}}"#).unwrap();
        assert_eq!(store.profiles["default"].token, "secret");
        let output = store.profiles["default"].redacted().to_string();
        assert!(!output.contains("secret"));
        assert!(!output.contains("session"));
    }
    #[test]
    fn invalid_config_values_are_rejected() {
        assert!(set(&mut Profile::default(), "apiVersion", "42").is_err());
        assert!(set(
            &mut Profile::default(),
            "server",
            "https://user:secret@example.com"
        )
        .is_err());
        assert!(!Profile {
            server: "https://user:secret@example.com".into(),
            ..Default::default()
        }
        .redacted()
        .to_string()
        .contains("secret"));
    }
}

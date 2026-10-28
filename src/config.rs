use crate::driver::{self, Family};
use anyhow::{Context, Result};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::{ErrorKind, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use url::Url;

const CONFIG_FILE: &str = "config.toml";
const RECENT_FILE: &str = "recent.toml";
const LOCAL_HOSTS: [&str; 4] = ["localhost", "127.0.0.1", "[::1]", "0.0.0.0"];

#[derive(Debug, Default, Deserialize)]
pub struct Config {
    #[serde(default, rename = "connection")]
    pub connections: Vec<Connection>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Connection {
    pub name: String,
    pub url: String,
    /// Set by hand (`prod`, `test`); when unset, `env()` derives it from the URL.
    #[serde(default)]
    pub env: Option<String>,
}

impl Connection {
    pub fn env(&self) -> &str {
        match &self.env {
            Some(env) => env,
            None if self.is_local() => "local",
            None => "other",
        }
    }

    /// The database type shown in the list: the URL scheme, or the file type for plain paths.
    pub fn kind(&self) -> String {
        if let Some(kind) = file_kind(&self.url) {
            return kind.to_string();
        }
        let scheme = self.url.split_once(':').map_or(self.url.as_str(), |(scheme, _)| scheme);
        scheme.to_ascii_lowercase()
    }

    pub fn redacted_url(&self) -> String {
        driver::redact(&self.url, &driver::secrets_in(&self.url))
    }

    fn is_local(&self) -> bool {
        if file_kind(&self.url).is_some() {
            return true;
        }
        let host = Url::parse(&self.url).ok().and_then(|url| url.host_str().map(str::to_ascii_lowercase));
        host.is_some_and(|host| LOCAL_HOSTS.contains(&host.as_str()))
    }
}

fn file_kind(url: &str) -> Option<&'static str> {
    match driver::family(url).ok()? {
        Family::Sqlite => Some("sqlite"),
        Family::Duckdb => Some("duckdb"),
        Family::Beancount => Some("beancount"),
        _ => None,
    }
}

pub fn dir() -> PathBuf {
    dirs::home_dir().unwrap_or_default().join(".config/dbtui")
}

pub fn config_file(dir: &Path) -> PathBuf {
    dir.join(CONFIG_FILE)
}

pub fn load(dir: &Path) -> Result<Config> {
    read_toml(&config_file(dir))
}

/// Appends instead of rewriting, so comments and formatting in the file survive.
pub fn append(dir: &Path, connection: &Connection) -> Result<()> {
    write_config(dir, &block(connection), OpenOptions::new().append(true))
}

/// Rewrites the whole file: comments and formatting written by hand are lost.
pub fn save(dir: &Path, connections: &[Connection]) -> Result<()> {
    let text: String = connections.iter().map(block).collect();
    write_config(dir, &text, OpenOptions::new().write(true).truncate(true))
}

fn block(connection: &Connection) -> String {
    let mut block = format!("\n[[connection]]\nname = {}\nurl = {}\n", quoted(&connection.name), quoted(&connection.url));
    if let Some(env) = &connection.env {
        block += &format!("env = {}\n", quoted(env));
    }
    block
}

fn write_config(dir: &Path, text: &str, options: &mut OpenOptions) -> Result<()> {
    let path = config_file(dir);
    std::fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
    // URLs carry passwords and tokens: a new file is readable by its owner only.
    let mut file = options.create(true).mode(0o600).open(&path).with_context(|| format!("cannot open {}", path.display()))?;
    file.write_all(text.as_bytes()).with_context(|| format!("cannot write {}", path.display()))
}

fn quoted(text: &str) -> String {
    toml::Value::String(text.to_string()).to_string()
}

/// Connection name to the unix time it was last opened.
pub fn load_recent(dir: &Path) -> Result<HashMap<String, u64>> {
    read_toml(&dir.join(RECENT_FILE))
}

pub fn save_recent(dir: &Path, recent: &HashMap<String, u64>) -> Result<()> {
    let path = dir.join(RECENT_FILE);
    std::fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
    std::fs::write(&path, toml::to_string(recent)?).with_context(|| format!("cannot write {}", path.display()))
}

fn read_toml<T: DeserializeOwned + Default>(path: &Path) -> Result<T> {
    match std::fs::read_to_string(path) {
        Ok(text) => toml::from_str(&text).with_context(|| format!("invalid {}", path.display())),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(T::default()),
        Err(error) => Err(error).with_context(|| format!("cannot read {}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn connection(url: &str, env: Option<&str>) -> Connection {
        Connection { name: "x".into(), url: url.into(), env: env.map(String::from) }
    }

    #[test]
    fn parses_connection_list() {
        let config: Config = toml::from_str("[[connection]]\nname = \"local\"\nurl = \"sqlite://a.db\"\n").unwrap();
        assert_eq!(config.connections[0].url, "sqlite://a.db");
        assert_eq!(config.connections[0].env, None);
    }

    #[test]
    fn env_is_local_for_files_and_localhost_unless_set() {
        assert_eq!(connection("a.db", None).env(), "local");
        assert_eq!(connection("postgres://u:p@localhost:5432/db", None).env(), "local");
        assert_eq!(connection("redis://127.0.0.1:6379/0", None).env(), "local");
        assert_eq!(connection("postgres://db.example.com/app", None).env(), "other");
        assert_eq!(connection("postgres://localhost/app", Some("test")).env(), "test");
    }

    #[test]
    fn kind_is_scheme_or_file_type() {
        assert_eq!(connection("ledger.beancount", None).kind(), "beancount");
        assert_eq!(connection("MariaDB://h/db", None).kind(), "mariadb");
        assert_eq!(connection("odbc:Driver=HDBODBC;Pwd=x", None).kind(), "odbc");
    }

    #[test]
    fn appended_connections_and_recent_read_back() {
        let dir = std::env::temp_dir().join(format!("dbtui-config-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let tricky = Connection { name: "a \"b\"\nc".into(), url: "postgres://u:p@h/db".into(), env: Some("prod".into()) };
        append(&dir, &connection("a.db", None)).unwrap();
        append(&dir, &tricky).unwrap();
        assert_eq!(load(&dir).unwrap().connections, vec![connection("a.db", None), tricky]);
        let mode = std::fs::metadata(config_file(&dir)).unwrap().permissions();
        assert_eq!(std::os::unix::fs::PermissionsExt::mode(&mode) & 0o777, 0o600);

        let recent = HashMap::from([("a \"b\"\nc".to_string(), 42)]);
        save_recent(&dir, &recent).unwrap();
        assert_eq!(load_recent(&dir).unwrap(), recent);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

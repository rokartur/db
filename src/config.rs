use anyhow::{Context, Result};
use serde::Deserialize;
use std::io::ErrorKind;
use std::path::PathBuf;

#[derive(Debug, Default, Deserialize)]
pub struct Config {
    #[serde(default, rename = "connection")]
    pub connections: Vec<Connection>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Connection {
    pub name: String,
    pub url: String,
}

pub fn path() -> PathBuf {
    dirs::home_dir().unwrap_or_default().join(".config/dbtui/config.toml")
}

pub fn load() -> Result<Config> {
    let path = path();
    match std::fs::read_to_string(&path) {
        Ok(text) => toml::from_str(&text).with_context(|| format!("invalid {}", path.display())),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(Config::default()),
        Err(error) => Err(error).with_context(|| format!("cannot read {}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_connection_list() {
        let config: super::Config =
            toml::from_str("[[connection]]\nname = \"local\"\nurl = \"sqlite://a.db\"\n").unwrap();
        assert_eq!(config.connections[0].url, "sqlite://a.db");
    }
}

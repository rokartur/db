use anyhow::Result;
use dbtui::{app, config};

const USAGE: &str = "usage: dbtui [URL | FILE]
  without arguments, pick a connection from ~/.config/dbtui/config.toml:
    [[connection]]
    name = \"local\"
    url = \"postgres://user:pass@localhost:5432/db\"";

#[tokio::main]
async fn main() -> Result<()> {
    let url = std::env::args().nth(1);
    if matches!(url.as_deref(), Some("-h" | "--help")) {
        println!("{USAGE}");
        return Ok(());
    }
    let connections = config::load()?.connections;
    app::run(url, connections).await
}

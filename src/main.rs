use anyhow::Result;
use dbtui::picker::Picker;
use dbtui::{app, config};

const USAGE: &str = "usage: dbtui [URL | FILE]
  without arguments, pick a connection from ~/.config/dbtui/config.toml
  (`a` in the list adds one):
    [[connection]]
    name = \"local\"
    url = \"postgres://user:pass@localhost:5432/db\"
    env = \"prod\"   # optional; localhost and files default to local";

#[tokio::main]
async fn main() -> Result<()> {
    let url = std::env::args().nth(1);
    if matches!(url.as_deref(), Some("-h" | "--help")) {
        println!("{USAGE}");
        return Ok(());
    }
    let picker = Picker::load(config::dir())?;
    app::run(url, picker).await
}

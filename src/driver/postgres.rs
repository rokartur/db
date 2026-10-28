use super::{Driver, Grid, quote_ident};
use anyhow::Result;
use async_trait::async_trait;
use postgres_native_tls::MakeTlsConnector;
use tokio_postgres::{Client, SimpleQueryMessage};

/// PostgreSQL wire: Postgres, CockroachDB, Redshift, PGlite.
pub struct Postgres {
    client: Client,
}

impl Postgres {
    pub async fn connect(url: &str) -> Result<Self> {
        let (_, rest) = url.split_once("://").unwrap_or(("", url));
        let mode = ssl_mode(rest);
        // tokio-postgres only parses disable/prefer/require; verification is done by the connector below.
        let verify = matches!(mode, "verify-ca" | "verify-full");
        let rest = if verify { rest.replace(&format!("sslmode={mode}"), "sslmode=require") } else { rest.to_string() };
        let tls = native_tls::TlsConnector::builder()
            .danger_accept_invalid_certs(!verify)
            .danger_accept_invalid_hostnames(mode != "verify-full")
            .build()?;
        let (client, connection) = tokio_postgres::connect(&format!("postgres://{rest}"), MakeTlsConnector::new(tls)).await?;
        tokio::spawn(connection);
        Ok(Postgres { client })
    }
}

/// libpq semantics: `prefer` (the default) and `require` encrypt without checking the certificate,
/// `verify-ca` checks the chain, `verify-full` also the hostname.
fn ssl_mode(rest: &str) -> &str {
    let query = rest.split_once('?').map_or("", |(_, query)| query);
    query.split('&').find_map(|pair| pair.strip_prefix("sslmode=")).unwrap_or("prefer")
}

#[async_trait]
impl Driver for Postgres {
    async fn query(&self, group: &str, text: &str) -> Result<Grid> {
        if !group.is_empty() {
            self.client.simple_query(&format!("SET search_path TO {}", quote_ident(group, '"'))).await?;
        }
        let messages = self.client.simple_query(text).await?;
        Ok(last_result(messages))
    }
}

/// The last statement's rows, or its affected-row count when it returned none.
fn last_result(messages: Vec<SimpleQueryMessage>) -> Grid {
    let mut result = Grid::message("OK");
    let mut current: Option<Grid> = None;
    for message in messages {
        match message {
            SimpleQueryMessage::RowDescription(columns) => {
                let columns = columns.iter().map(|c| c.name().to_string()).collect();
                current = Some(Grid { columns, rows: Vec::new() });
            }
            SimpleQueryMessage::Row(row) => {
                if let Some(grid) = &mut current {
                    grid.rows.push((0..row.len()).map(|i| row.get(i).unwrap_or("NULL").to_string()).collect());
                }
            }
            SimpleQueryMessage::CommandComplete(count) => {
                result = current.take().unwrap_or_else(|| Grid::message(format!("{count} rows affected")));
            }
            _ => {}
        }
    }
    result
}

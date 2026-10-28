use super::http::{client, json_cell, send};
use super::{Driver, Grid, PAGE, param, parse_url, password, path_segments, user};
use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;

/// ClickHouse HTTP interface: `clickhouse://user:pass@host:8123/db`, `?tls=1` for https.
pub struct Clickhouse {
    endpoint: String,
    user: String,
    password: Option<String>,
    database: String,
}

impl Clickhouse {
    pub fn new(url: &str) -> Result<Self> {
        let parsed = parse_url(url)?;
        let scheme = if param(&parsed, "tls").as_deref() == Some("1") { "https" } else { "http" };
        let host = parsed.host_str().unwrap_or("localhost");
        let port = parsed.port().unwrap_or(8123);
        Ok(Clickhouse {
            endpoint: format!("{scheme}://{host}:{port}/"),
            user: user(&parsed).unwrap_or_else(|| "default".into()),
            password: password(&parsed),
            database: path_segments(&parsed).into_iter().next().unwrap_or_else(|| "default".into()),
        })
    }

    async fn run(&self, group: &str, text: &str, params: &[(&str, &str)]) -> Result<Grid> {
        let database = if group.is_empty() { &self.database } else { group };
        let request = client()
            .post(&self.endpoint)
            .query(&[("database", database), ("default_format", "JSONCompact")])
            .query(params)
            .basic_auth(&self.user, self.password.as_ref())
            .body(text.to_string());
        Ok(parse_json_compact(&send(request).await?))
    }
}

#[async_trait]
impl Driver for Clickhouse {
    async fn query(&self, group: &str, text: &str) -> Result<Grid> {
        self.run(group, text, &[]).await
    }

    /// Names travel as `Identifier` query parameters: ClickHouse quoting also honours backslash escapes,
    /// so a crafted table name could otherwise close the quotes.
    async fn browse(&self, group: &str, item: &str, page: usize) -> Result<Grid> {
        let sql = format!("SELECT * FROM {{db:Identifier}}.{{table:Identifier}} LIMIT {PAGE} OFFSET {}", page * PAGE);
        self.run(group, &sql, &[("param_db", group), ("param_table", item)]).await
    }
}

fn parse_json_compact(response: &Value) -> Grid {
    let Some(meta) = response["meta"].as_array() else {
        return Grid::message("OK");
    };
    let columns = meta.iter().map(|c| json_cell(&c["name"])).collect();
    let rows = response["data"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|row| row.as_array().into_iter().flatten().map(json_cell).collect())
        .collect();
    Grid { columns, rows }
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_json_compact() {
        let response = serde_json::json!({"meta": [{"name": "n", "type": "UInt64"}, {"name": "s", "type": "Nullable(String)"}],
            "data": [["1", "a"], ["2", null]], "rows": 2});
        let grid = super::parse_json_compact(&response);
        assert_eq!(grid.columns, ["n", "s"]);
        assert_eq!(grid.rows, [["1", "a"], ["2", "NULL"]]);
    }
}

use super::http::{client, send};
use super::{Driver, Grid, Group, group_rows, param, parse_url};
use anyhow::{Result, bail};
use async_trait::async_trait;
use base64::Engine;
use serde_json::{Value, json};

/// libSQL / Turso over Hrana HTTP: `libsql://host?authToken=...`, `?tls=0` for plain http.
pub struct Libsql {
    endpoint: String,
    token: Option<String>,
}

impl Libsql {
    pub fn new(url: &str) -> Result<Self> {
        let parsed = parse_url(url)?;
        let scheme = if param(&parsed, "tls").as_deref() == Some("0") { "http" } else { "https" };
        let host = parsed.host_str().unwrap_or("localhost");
        let port = parsed.port().map(|p| format!(":{p}")).unwrap_or_default();
        let token = param(&parsed, "authToken").or_else(|| std::env::var("LIBSQL_AUTH_TOKEN").ok());
        Ok(Libsql { endpoint: format!("{scheme}://{host}{port}/v2/pipeline"), token })
    }
}

#[async_trait]
impl Driver for Libsql {
    async fn query(&self, _group: &str, text: &str) -> Result<Grid> {
        let body = json!({"requests": [{"type": "execute", "stmt": {"sql": text}}, {"type": "close"}]});
        let mut request = client().post(&self.endpoint).json(&body);
        if let Some(token) = &self.token {
            request = request.bearer_auth(token);
        }
        parse_pipeline(&send(request).await?)
    }

    async fn groups(&self) -> Result<Vec<Group>> {
        let sql = "SELECT 'main', name FROM sqlite_master \
                   WHERE type IN ('table', 'view') AND name NOT LIKE 'sqlite_%' AND name NOT LIKE '_litestream%' ORDER BY name";
        Ok(group_rows(&self.query("", sql).await?))
    }
}

fn parse_pipeline(response: &Value) -> Result<Grid> {
    let first = &response["results"][0];
    if first["type"] == "error" {
        bail!("{}", first["error"]["message"].as_str().unwrap_or("libsql error"));
    }
    let result = &first["response"]["result"];
    let Some(cols) = result["cols"].as_array().filter(|cols| !cols.is_empty()) else {
        return Ok(Grid::message(format!("{} rows affected", result["affected_row_count"])));
    };
    let columns = cols.iter().map(|c| c["name"].as_str().unwrap_or("").to_string()).collect();
    let rows = result["rows"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|row| row.as_array().into_iter().flatten().map(hrana_cell).collect())
        .collect();
    Ok(Grid { columns, rows })
}

fn hrana_cell(value: &Value) -> String {
    match value["type"].as_str() {
        Some("null") => "NULL".into(),
        Some("blob") => {
            let encoded = value["base64"].as_str().unwrap_or("");
            let bytes = base64::engine::general_purpose::STANDARD_NO_PAD.decode(encoded.trim_end_matches('='));
            bytes.map_or_else(|_| encoded.to_string(), |b| format!("0x{}", hex::encode(b)))
        }
        _ => super::http::json_cell(&value["value"]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hrana_execute_result() {
        let response = json!({"baton": null, "results": [
            {"type": "ok", "response": {"type": "execute", "result": {
                "cols": [{"name": "id", "decltype": "INTEGER"}, {"name": "name", "decltype": "TEXT"}, {"name": "b"}],
                "rows": [[{"type": "integer", "value": "1"}, {"type": "text", "value": "Ala"}, {"type": "blob", "base64": "AQI="}],
                         [{"type": "integer", "value": "2"}, {"type": "null"}, {"type": "float", "value": 1.5}]],
                "affected_row_count": 0}}},
            {"type": "ok", "response": {"type": "close"}}]});
        let grid = parse_pipeline(&response).unwrap();
        assert_eq!(grid.columns, ["id", "name", "b"]);
        assert_eq!(grid.rows, [["1", "Ala", "0x0102"], ["2", "NULL", "1.5"]]);
        let error = json!({"results": [{"type": "error", "error": {"message": "SQLite error: no such table: x"}}]});
        assert_eq!(parse_pipeline(&error).unwrap_err().to_string(), "SQLite error: no such table: x");
    }
}

use super::http::{client, json_cell, send};
use super::{Driver, Grid, Group, group_rows, param, parse_url, path_segments};
use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use serde_json::{Value, json};

/// Cloudflare D1 over the REST `/raw` endpoint: `d1://account_id/database_id?token=...`.
pub struct D1 {
    endpoint: String,
    token: String,
}

impl D1 {
    pub fn new(url: &str) -> Result<Self> {
        let parsed = parse_url(url)?;
        let account = parsed.host_str().context("d1 URL needs an account id: d1://account/database")?;
        let database = path_segments(&parsed).into_iter().next().context("d1 URL needs a database id")?;
        let token = param(&parsed, "token")
            .or_else(|| std::env::var("CLOUDFLARE_API_TOKEN").ok())
            .context("d1 needs ?token= or CLOUDFLARE_API_TOKEN")?;
        let endpoint = format!("https://api.cloudflare.com/client/v4/accounts/{account}/d1/database/{database}/raw");
        Ok(D1 { endpoint, token })
    }
}

#[async_trait]
impl Driver for D1 {
    async fn query(&self, _group: &str, text: &str) -> Result<Grid> {
        let request = client().post(&self.endpoint).bearer_auth(&self.token).json(&json!({"sql": text}));
        parse_raw(&send(request).await?)
    }

    async fn groups(&self) -> Result<Vec<Group>> {
        let sql = "SELECT 'main', name FROM sqlite_master WHERE type IN ('table', 'view') \
                   AND name NOT LIKE 'sqlite_%' AND name NOT LIKE '_cf_%' ORDER BY name";
        Ok(group_rows(&self.query("", sql).await?))
    }
}

/// `result` holds one entry per statement; the last one is shown.
fn parse_raw(response: &Value) -> Result<Grid> {
    if response["success"] == false {
        let messages: Vec<&str> = response["errors"].as_array().into_iter().flatten().filter_map(|e| e["message"].as_str()).collect();
        bail!("{}", messages.join("; "));
    }
    let Some(last) = response["result"].as_array().and_then(|results| results.last()) else {
        return Ok(Grid::message("OK"));
    };
    let results = &last["results"];
    let columns = results["columns"].as_array().into_iter().flatten().map(json_cell).collect();
    let rows = results["rows"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|row| row.as_array().into_iter().flatten().map(json_cell).collect())
        .collect();
    Ok(Grid { columns, rows })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_raw_response_from_docs() {
        let response = json!({
            "errors": [], "messages": [], "success": true,
            "result": [{
                "meta": {"changed_db": false, "changes": 0, "duration": 0.1, "rows_read": 2, "rows_written": 0},
                "results": {"columns": ["id", "name"], "rows": [[1, "Ala"], [2, null]]},
                "success": true
            }]
        });
        let grid = parse_raw(&response).unwrap();
        assert_eq!(grid.columns, ["id", "name"]);
        assert_eq!(grid.rows, [["1", "Ala"], ["2", "NULL"]]);
        let failed = json!({"success": false, "errors": [{"code": 7500, "message": "no such table: x"}], "result": []});
        assert_eq!(parse_raw(&failed).unwrap_err().to_string(), "no such table: x");
    }
}

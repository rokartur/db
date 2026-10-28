use super::http::{client, json_cell, send};
use super::{Driver, Grid, param, parse_url};
use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::{Value, json};
use std::time::Duration;

/// Snowflake SQL API v2: `snowflake://account?token=...&warehouse=&database=&role=&token_type=PROGRAMMATIC_ACCESS_TOKEN`.
pub struct Snowflake {
    base: String,
    token: String,
    token_type: Option<String>,
    context: Value,
}

impl Snowflake {
    pub fn new(url: &str) -> Result<Self> {
        let parsed = parse_url(url)?;
        let account = parsed.host_str().context("snowflake URL needs an account: snowflake://account")?;
        let token = param(&parsed, "token")
            .or_else(|| std::env::var("SNOWFLAKE_TOKEN").ok())
            .context("snowflake needs ?token= or SNOWFLAKE_TOKEN")?;
        let mut context = json!({});
        for key in ["warehouse", "database", "schema", "role"] {
            if let Some(value) = param(&parsed, key) {
                context[key] = json!(value);
            }
        }
        Ok(Snowflake {
            base: format!("https://{account}.snowflakecomputing.com/api/v2/statements"),
            token,
            token_type: param(&parsed, "token_type"),
            context,
        })
    }

    fn request(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        let request = request.bearer_auth(&self.token).header("Accept", "application/json");
        match &self.token_type {
            Some(kind) => request.header("X-Snowflake-Authorization-Token-Type", kind),
            None => request,
        }
    }
}

const STILL_RUNNING: &str = "333334";

#[async_trait]
impl Driver for Snowflake {
    async fn query(&self, group: &str, text: &str) -> Result<Grid> {
        let mut body = self.context.clone();
        body["statement"] = json!(text);
        body["timeout"] = json!(60);
        if !group.is_empty() {
            body["schema"] = json!(group);
        }
        let mut response = send(self.request(client().post(&self.base).json(&body))).await?;
        while response["code"] == STILL_RUNNING {
            tokio::time::sleep(Duration::from_millis(500)).await;
            let handle = json_cell(&response["statementHandle"]);
            response = send(self.request(client().get(format!("{}/{handle}", self.base)))).await?;
        }
        Ok(parse_result_set(&response))
    }
}

/// shortcut: only the first partition of a large result; later ones need GET ?partition=n.
fn parse_result_set(response: &Value) -> Grid {
    let columns = response["resultSetMetaData"]["rowType"].as_array().into_iter().flatten().map(|c| json_cell(&c["name"])).collect();
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
    fn parses_sql_api_example() {
        // Shape of the "Response" example in the Snowflake SQL API reference (POST /api/v2/statements).
        let response = serde_json::json!({
            "resultSetMetaData": {"numRows": 2, "format": "jsonv2",
                "partitionInfo": [{"rowCount": 2, "uncompressedSize": 56}],
                "rowType": [{"name": "COLUMN1", "type": "FIXED", "nullable": false},
                            {"name": "COLUMN2", "type": "TEXT", "nullable": true}]},
            "data": [["0", "zero"], ["1", null]],
            "code": "090001", "statementStatusUrl": "/api/v2/statements/01b2?requestId=x",
            "sqlState": "00000", "statementHandle": "01b2", "message": "Statement executed successfully.",
            "createdOn": 1620151693299u64});
        let grid = super::parse_result_set(&response);
        assert_eq!(grid.columns, ["COLUMN1", "COLUMN2"]);
        assert_eq!(grid.rows, [["0", "zero"], ["1", "NULL"]]);
    }
}
